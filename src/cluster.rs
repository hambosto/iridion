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
    pub swatches: [Swatch; NUM_CLUSTERS],
    pub labels: Vec<u8>,
    pub avg_chroma: f64,
    pub pixels: Vec<Color>,
}

impl ClusteringResult {
    pub fn from_pixels(pixels: &[Color]) -> Self {
        let filtered: Vec<Color> = pixels.iter().copied().filter(|p| p.chroma >= CHROMA_THRESHOLD).collect();
        let n = filtered.len() as f64;
        let avg_chroma = filtered.iter().map(|p| p.chroma).sum::<f64>() / n;

        let mut normalized: Vec<[f64; 3]> = filtered.iter().map(|p| [p.l, p.chroma, p.hue]).collect();
        let stats = z_normalize(&mut normalized);
        let (swatches, labels) = select_best_offset(&normalized, &stats);

        Self { swatches, labels, avg_chroma, pixels: filtered }
    }
}

struct NormStats {
    mean: [f64; 3],
    std: [f64; 3],
}

impl NormStats {
    fn denormalize(&self, normalized: [f64; 3]) -> [f64; 3] {
        [normalized[0] * self.std[0] + self.mean[0], normalized[1] * self.std[1] + self.mean[1], normalized[2] * self.std[2] + self.mean[2]]
    }

    fn normalize_hue(&self, hue: f64) -> f64 {
        (hue - self.mean[2]) / self.std[2]
    }
}

fn z_normalize(pixels: &mut [[f64; 3]]) -> NormStats {
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

    NormStats { mean, std }
}

fn select_best_offset(normalized: &[[f64; 3]], stats: &NormStats) -> ([Swatch; NUM_CLUSTERS], Vec<u8>) {
    let total = normalized.len();
    let mut best = cluster_with_offset(normalized, stats, HUE_OFFSETS[0]);
    let mut best_score = score(&best.0, total);

    for &offset in &HUE_OFFSETS[1..] {
        let candidate = cluster_with_offset(normalized, stats, offset);
        let s = score(&candidate.0, total);
        if s > best_score {
            best = candidate;
            best_score = s;
        }
    }

    best
}

fn score(swatches: &[Swatch; NUM_CLUSTERS], total: usize) -> (usize, f64) {
    let mut active_count = 0;
    let mut active_pixels = 0usize;
    for s in swatches {
        if s.active {
            active_count += 1;
            active_pixels += s.pixel_count;
        }
    }

    (active_count, active_pixels as f64 / total as f64)
}

fn cluster_with_offset(normalized: &[[f64; 3]], stats: &NormStats, offset: f64) -> ([Swatch; NUM_CLUSTERS], Vec<u8>) {
    let mut centers = [[0.0; 3]; NUM_CLUSTERS];
    let mut sectors = [[0.0; 2]; NUM_CLUSTERS];

    for i in 0..NUM_CLUSTERS {
        let hue = (BASE_HUES[i] + offset).rem_euclid(360.0);
        centers[i][2] = stats.normalize_hue(hue);

        let lo = (hue - HUE_SECTOR_HALF_WIDTH).rem_euclid(360.0);
        let hi = (hue + HUE_SECTOR_HALF_WIDTH).rem_euclid(360.0);
        sectors[i] = [stats.normalize_hue(lo), stats.normalize_hue(hi)];
    }

    let mut active = [true; NUM_CLUSTERS];
    let labels = run_constrained_kmeans(normalized, &mut centers, &sectors, &mut active);
    let swatches = build_swatches(&centers, &active, &labels, stats);

    (swatches, labels)
}

fn run_constrained_kmeans(normalized: &[[f64; 3]], centers: &mut [[f64; 3]; NUM_CLUSTERS], sectors: &[[f64; 2]; NUM_CLUSTERS], active: &mut [bool; NUM_CLUSTERS]) -> Vec<u8> {
    let mut labels = vec![UNASSIGNED; normalized.len()];

    for _ in 0..MAX_ITERS {
        if !active.iter().any(|&a| a) {
            break;
        }

        let (assignments, sums, counts) = assign_to_nearest(normalized, &labels, centers, active);
        let converged = update_and_clamp(centers, sectors, active, &sums, &counts, &mut labels, &assignments);

        if converged {
            break;
        }
    }

    for (label, point) in labels.iter_mut().zip(normalized) {
        if *label == UNASSIGNED {
            *label = nearest_active_center(*point, centers, active) as u8;
        }
    }

    labels
}

fn assign_to_nearest(normalized: &[[f64; 3]], labels: &[u8], centers: &[[f64; 3]; NUM_CLUSTERS], active: &[bool; NUM_CLUSTERS]) -> (Vec<u8>, [[f64; 3]; NUM_CLUSTERS], [usize; NUM_CLUSTERS]) {
    let mut assignments = vec![0u8; normalized.len()];
    let mut sums = [[0.0; 3]; NUM_CLUSTERS];
    let mut counts = [0usize; NUM_CLUSTERS];

    for (i, point) in normalized.iter().enumerate() {
        let k = if labels[i] != UNASSIGNED {
            labels[i] as usize
        } else {
            let k = nearest_active_center(*point, centers, active);
            for c in 0..3 {
                sums[k][c] += point[c];
            }
            counts[k] += 1;
            k
        };
        assignments[i] = k as u8;
    }

    (assignments, sums, counts)
}

fn update_and_clamp(
    centers: &mut [[f64; 3]; NUM_CLUSTERS], sectors: &[[f64; 2]; NUM_CLUSTERS], active: &mut [bool; NUM_CLUSTERS], sums: &[[f64; 3]; NUM_CLUSTERS], counts: &[usize; NUM_CLUSTERS], labels: &mut [u8],
    assignments: &[u8],
) -> bool {
    let mut converged = true;
    for k in 0..NUM_CLUSTERS {
        if !active[k] || counts[k] == 0 {
            continue;
        }

        let inv = 1.0 / counts[k] as f64;
        let new_center = [sums[k][0] * inv, sums[k][1] * inv, sums[k][2] * inv];
        if hue_in_sector(new_center[2], sectors[k]) {
            let shift_sq: f64 = (0..3).map(|c| (new_center[c] - centers[k][c]).powi(2)).sum();
            centers[k] = new_center;
            if shift_sq > TOLERANCE * TOLERANCE {
                converged = false;
            }
            continue;
        }

        let clamped_hue = clamp_to_sector(new_center[2], sectors[k]);
        centers[k] = [new_center[0], new_center[1], clamped_hue];
        active[k] = false;
        converged = false;

        for (label, &assigned) in labels.iter_mut().zip(assignments) {
            if *label == UNASSIGNED && assigned as usize == k {
                *label = k as u8;
            }
        }
    }

    converged
}

fn hue_in_sector(hue: f64, [lo, hi]: [f64; 2]) -> bool {
    if lo < hi { hue >= lo && hue < hi } else { hue >= lo || hue < hi }
}

fn clamp_to_sector(hue: f64, [lo, hi]: [f64; 2]) -> f64 {
    if lo < hi {
        if hue < lo { lo } else { hi }
    } else {
        let dist_lo = (hue - lo).abs().min(360.0 - (hue - lo).abs());
        let dist_hi = (hue - hi).abs().min(360.0 - (hue - hi).abs());
        if dist_lo < dist_hi { lo } else { hi }
    }
}

fn nearest_active_center(point: [f64; 3], centers: &[[f64; 3]; NUM_CLUSTERS], active: &[bool; NUM_CLUSTERS]) -> usize {
    let mut min_dist = f64::INFINITY;
    let mut nearest = 0;
    for (k, &is_active) in active.iter().enumerate() {
        if !is_active {
            continue;
        }

        let d: f64 = (0..3).map(|c| (point[c] - centers[k][c]).powi(2)).sum();
        if d < min_dist {
            min_dist = d;
            nearest = k;
        }
    }

    nearest
}

fn build_swatches(centers: &[[f64; 3]; NUM_CLUSTERS], active: &[bool; NUM_CLUSTERS], labels: &[u8], stats: &NormStats) -> [Swatch; NUM_CLUSTERS] {
    let default = Swatch { color: Color::default(), pixel_count: 0, active: false };
    let mut swatches = [default; NUM_CLUSTERS];

    for k in 0..NUM_CLUSTERS {
        let [l, ch, h] = stats.denormalize(centers[k]);
        swatches[k] = Swatch { color: Color::new(l, ch, h), pixel_count: 0, active: active[k] };
    }

    for &label in labels {
        swatches[label as usize].pixel_count += 1;
    }

    swatches
}
