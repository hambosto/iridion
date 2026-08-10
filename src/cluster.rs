use crate::color::Color;

pub const NUM_CLUSTERS: usize = 8;
const BASE_HUES: [f64; NUM_CLUSTERS] = [0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
const HUE_OFFSETS: [f64; 3] = [0.0, 15.0, 30.0];
const HUE_SECTOR_HALF_WIDTH: f64 = 22.5;
const MAX_ITERS: usize = 100;
const TOLERANCE: f64 = 1e-4;
const UNASSIGNED: u8 = u8::MAX;
const CHROMA_THRESHOLD: f64 = 0.01;

#[derive(Clone, Copy, Debug)]
pub struct Swatch {
    pub color: Color,
    pub pixel_count: usize,
    pub active: bool,
}

pub struct ClusteringResult {
    pub swatches: Vec<Swatch>,
    pub labels: Vec<u8>,
    pub avg_chroma: f64,
    pub pixels: Vec<Color>,
}

impl ClusteringResult {
    pub fn build_from_pixels(pixels: Vec<Color>) -> Self {
        let saturated: Vec<Color> = pixels.into_iter().filter(|p| p.chroma >= CHROMA_THRESHOLD).collect();
        let avg_chroma = saturated.iter().map(|p| p.chroma).sum::<f64>() / saturated.len() as f64;

        let mut normalized: Vec<[f64; 3]> = saturated.iter().map(|p| [p.l, p.chroma, p.hue]).collect();
        let (mean, std) = normalize_in_place(&mut normalized);
        let (swatches, labels) = select_optimal_offset(&normalized, mean, std);

        Self { swatches: swatches.to_vec(), labels, avg_chroma, pixels: saturated }
    }
}

fn normalize_in_place(pixels: &mut [[f64; 3]]) -> ([f64; 3], [f64; 3]) {
    let n = pixels.len() as f64;

    let mut mean = [0.0; 3];
    for p in pixels.iter() {
        for c in 0..3 {
            mean[c] += p[c];
        }
    }
    for m in &mut mean {
        *m /= n;
    }

    let mut variance = [0.0; 3];
    for p in pixels.iter() {
        for c in 0..3 {
            variance[c] += (p[c] - mean[c]).powi(2);
        }
    }

    let mut std = [0.0; 3];
    for c in 0..3 {
        std[c] = (variance[c] / n).sqrt().max(1e-8);
    }

    for p in pixels.iter_mut() {
        for c in 0..3 {
            p[c] = (p[c] - mean[c]) / std[c];
        }
    }

    (mean, std)
}

fn select_optimal_offset(normalized: &[[f64; 3]], mean: [f64; 3], std: [f64; 3]) -> ([Swatch; NUM_CLUSTERS], Vec<u8>) {
    let total = normalized.len();
    let mut optimal = cluster_with_offset(normalized, mean, std, HUE_OFFSETS[0]);
    let mut opt_score = score_active_clusters(&optimal.0, total);

    for &offset in &HUE_OFFSETS[1..] {
        let candidate = cluster_with_offset(normalized, mean, std, offset);
        let s = score_active_clusters(&candidate.0, total);
        if s > opt_score {
            optimal = candidate;
            opt_score = s;
        }
    }

    optimal
}

fn score_active_clusters(swatches: &[Swatch; NUM_CLUSTERS], total: usize) -> (usize, f64) {
    let mut count = 0;
    let mut covered = 0usize;
    for s in swatches {
        if s.active {
            count += 1;
            covered += s.pixel_count;
        }
    }

    (count, covered as f64 / total as f64)
}

fn cluster_with_offset(normalized: &[[f64; 3]], mean: [f64; 3], std: [f64; 3], offset: f64) -> ([Swatch; NUM_CLUSTERS], Vec<u8>) {
    let norm_hue = |hue: f64| (hue - mean[2]) / std[2];
    let mut centers = [[0.0; 3]; NUM_CLUSTERS];
    let mut sectors = [[0.0; 2]; NUM_CLUSTERS];
    for i in 0..NUM_CLUSTERS {
        let hue = (BASE_HUES[i] + offset).rem_euclid(360.0);
        centers[i][2] = norm_hue(hue);

        let lo = (hue - HUE_SECTOR_HALF_WIDTH).rem_euclid(360.0);
        let hi = (hue + HUE_SECTOR_HALF_WIDTH).rem_euclid(360.0);
        sectors[i] = [norm_hue(lo), norm_hue(hi)];
    }

    let mut active = [true; NUM_CLUSTERS];
    let labels = run_lloyds_clustering(normalized, &mut centers, &sectors, &mut active);
    let swatches = build_color_swatches(&centers, &active, &labels, mean, std);

    (swatches, labels)
}

fn run_lloyds_clustering(normalized: &[[f64; 3]], centers: &mut [[f64; 3]; NUM_CLUSTERS], sectors: &[[f64; 2]; NUM_CLUSTERS], active: &mut [bool; NUM_CLUSTERS]) -> Vec<u8> {
    let mut labels = vec![UNASSIGNED; normalized.len()];

    for _ in 0..MAX_ITERS {
        let has_work = active.iter().any(|&a| a) && labels.contains(&UNASSIGNED);
        if !has_work {
            break;
        }

        let mut sums = [[0.0; 3]; NUM_CLUSTERS];
        let mut counts = [0usize; NUM_CLUSTERS];
        let assignments: Vec<u8> = labels
            .iter()
            .zip(normalized)
            .map(|(label, point)| {
                if *label != UNASSIGNED {
                    return *label;
                }
                let k = find_nearest_active(*point, centers, active);
                sums[k].iter_mut().zip(point).for_each(|(s, p)| *s += p);
                counts[k] += 1;
                k as u8
            })
            .collect();

        let converged = (0..NUM_CLUSTERS).fold(true, |stable, k| {
            if !active[k] || counts[k] == 0 {
                return stable;
            }

            let inv = 1.0 / counts[k] as f64;
            let new_center = [sums[k][0] * inv, sums[k][1] * inv, sums[k][2] * inv];
            let shift_sq: f64 = (0..3).map(|c| new_center[c] - centers[k][c]).map(|d| d * d).sum();

            let [lo, hi] = sectors[k];
            let in_bounds = if lo < hi { new_center[2] >= lo && new_center[2] < hi } else { new_center[2] >= lo || new_center[2] < hi };

            if in_bounds {
                centers[k] = new_center;
                return stable && shift_sq <= TOLERANCE * TOLERANCE;
            }

            let hue = new_center[2];
            let clamped = if lo < hi {
                if hue < lo { lo } else { hi }
            } else {
                let d_lo = (hue - lo).abs().min(360.0 - (hue - lo).abs());
                let d_hi = (hue - hi).abs().min(360.0 - (hue - hi).abs());
                if d_lo < d_hi { lo } else { hi }
            };
            centers[k] = [new_center[0], new_center[1], clamped];
            active[k] = false;

            labels.iter_mut().zip(&assignments).filter(|(_, a)| **a == k as u8).for_each(|(label, _)| {
                if *label == UNASSIGNED {
                    *label = k as u8;
                }
            });

            false
        });

        if converged {
            break;
        }
    }

    labels.iter_mut().zip(normalized).for_each(|(label, point)| {
        if *label == UNASSIGNED {
            *label = find_nearest_active(*point, centers, active) as u8;
        }
    });

    labels
}

fn find_nearest_active(point: [f64; 3], centers: &[[f64; 3]; NUM_CLUSTERS], active: &[bool; NUM_CLUSTERS]) -> usize {
    let mut min_dist = f64::INFINITY;
    let mut nearest = 0;
    for (k, &is_active) in active.iter().enumerate() {
        if !is_active {
            continue;
        }

        let d: f64 = (0..3).map(|c| point[c] - centers[k][c]).map(|d| d * d).sum();
        if d < min_dist {
            min_dist = d;
            nearest = k;
        }
    }

    nearest
}

fn build_color_swatches(centers: &[[f64; 3]; NUM_CLUSTERS], active: &[bool; NUM_CLUSTERS], labels: &[u8], mean: [f64; 3], std: [f64; 3]) -> [Swatch; NUM_CLUSTERS] {
    let mut swatches = [Swatch { color: Color::default(), pixel_count: 0, active: false }; NUM_CLUSTERS];
    for k in 0..NUM_CLUSTERS {
        let [l, ch, h] = centers[k];
        let color = Color::new(l * std[0] + mean[0], ch * std[1] + mean[1], h * std[2] + mean[2]);
        swatches[k] = Swatch { color, pixel_count: 0, active: active[k] };
    }

    for &label in labels {
        swatches[label as usize].pixel_count += 1;
    }

    swatches
}
