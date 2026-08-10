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

struct ZScore {
    mean: [f64; 3],
    std: [f64; 3],
}

impl ZScore {
    fn normalize_in_place(data: &mut [[f64; 3]]) -> Self {
        let num_pixels = data.len() as f64;

        let mut mean = [0.0f64; 3];
        for pixel in data.iter() {
            for channel in 0..3 {
                mean[channel] += pixel[channel];
            }
        }
        for mean_val in mean.iter_mut() {
            *mean_val /= num_pixels;
        }

        let mut variance = [0.0f64; 3];
        for pixel in data.iter() {
            for channel in 0..3 {
                variance[channel] += (pixel[channel] - mean[channel]).powi(2);
            }
        }

        let mut std = [0.0f64; 3];
        for channel in 0..3 {
            std[channel] = (variance[channel] / num_pixels).sqrt().max(1e-8);
        }

        for pixel in data.iter_mut() {
            for channel in 0..3 {
                pixel[channel] = (pixel[channel] - mean[channel]) / std[channel];
            }
        }

        Self { mean, std }
    }
}

fn hue_in_sector(hue: f64, sector_start: f64, sector_end: f64) -> bool {
    if sector_start < sector_end {
        hue >= sector_start && hue < sector_end
    } else {
        hue >= sector_start || hue < sector_end
    }
}

fn circular_distance(a: f64, b: f64) -> f64 {
    let diff = (a - b).abs();
    diff.min(360.0 - diff)
}

fn clamp_to_sector(hue: f64, sector_start: f64, sector_end: f64) -> f64 {
    if hue_in_sector(hue, sector_start, sector_end) {
        return hue;
    }

    if sector_start < sector_end {
        if hue < sector_start { sector_start } else { sector_end }
    } else {
        if circular_distance(hue, sector_start) < circular_distance(hue, sector_end) { sector_start } else { sector_end }
    }
}

fn find_nearest_active_center(point: [f64; 3], centers: &[[f64; 3]; NUM_CLUSTERS], active: &[bool; NUM_CLUSTERS]) -> usize {
    let mut best_distance = f64::INFINITY;
    let mut best_cluster = 0;
    for (cluster_idx, &is_active) in active.iter().enumerate() {
        if !is_active {
            continue;
        }

        let squared_distance: f64 = (0..3).map(|channel| point[channel] - centers[cluster_idx][channel]).map(|delta| delta * delta).sum();
        if squared_distance < best_distance {
            best_distance = squared_distance;
            best_cluster = cluster_idx;
        }
    }

    best_cluster
}

fn denormalize_center(center: [f64; 3], stats: &ZScore) -> [f64; 3] {
    [center[0] * stats.std[0] + stats.mean[0], center[1] * stats.std[1] + stats.mean[1], center[2] * stats.std[2] + stats.mean[2]]
}

fn run_hue_locked_kmeans(normalized: &[[f64; 3]], stats: &ZScore, offset: f64) -> ([Swatch; NUM_CLUSTERS], Vec<u8>) {
    let num_pixels = normalized.len();

    let mut centers = [[0.0f64; 3]; NUM_CLUSTERS];
    let mut sectors = [[0.0f64; 2]; NUM_CLUSTERS];
    for cluster_idx in 0..NUM_CLUSTERS {
        let hue = (BASE_HUES[cluster_idx] + offset).rem_euclid(360.0);
        centers[cluster_idx][2] = (hue - stats.mean[2]) / stats.std[2];

        let sector_lo = (hue - HUE_SECTOR_HALF_WIDTH).rem_euclid(360.0);
        let sector_hi = (hue + HUE_SECTOR_HALF_WIDTH).rem_euclid(360.0);
        sectors[cluster_idx][0] = (sector_lo - stats.mean[2]) / stats.std[2];
        sectors[cluster_idx][1] = (sector_hi - stats.mean[2]) / stats.std[2];
    }

    let mut active = [true; NUM_CLUSTERS];
    let mut labels = vec![UNASSIGNED; num_pixels];
    let mut assignments = vec![0u8; num_pixels];
    let mut center_sums = [[0.0f64; 3]; NUM_CLUSTERS];
    let mut pixel_counts = [0usize; NUM_CLUSTERS];
    let mut unassigned_count = num_pixels;

    for _ in 0..MAX_ITERS {
        if active.iter().all(|&is_active| !is_active) || unassigned_count == 0 {
            break;
        }

        for sum in center_sums.iter_mut() {
            *sum = [0.0; 3];
        }
        pixel_counts.fill(0);

        for pixel_idx in 0..num_pixels {
            if labels[pixel_idx] != UNASSIGNED {
                continue;
            }

            let cluster_idx = find_nearest_active_center(normalized[pixel_idx], &centers, &active);
            assignments[pixel_idx] = cluster_idx as u8;

            for channel in 0..3 {
                center_sums[cluster_idx][channel] += normalized[pixel_idx][channel];
            }

            pixel_counts[cluster_idx] += 1;
        }

        let mut any_changed = false;
        for cluster_idx in 0..NUM_CLUSTERS {
            if !active[cluster_idx] || pixel_counts[cluster_idx] == 0 {
                continue;
            }

            let reciprocal_count = 1.0 / pixel_counts[cluster_idx] as f64;
            let new_center = [center_sums[cluster_idx][0] * reciprocal_count, center_sums[cluster_idx][1] * reciprocal_count, center_sums[cluster_idx][2] * reciprocal_count];
            let shift_squared: f64 = (0..3).map(|channel| new_center[channel] - centers[cluster_idx][channel]).map(|delta| delta * delta).sum();
            if shift_squared > TOLERANCE * TOLERANCE {
                any_changed = true;
            }

            let sector_lo = sectors[cluster_idx][0];
            let sector_hi = sectors[cluster_idx][1];
            if hue_in_sector(new_center[2], sector_lo, sector_hi) {
                centers[cluster_idx] = new_center;
            } else {
                centers[cluster_idx] = [new_center[0], new_center[1], clamp_to_sector(new_center[2], sector_lo, sector_hi)];
                active[cluster_idx] = false;
                any_changed = true;

                let locked_cluster = cluster_idx as u8;
                for (label, &assignment) in labels.iter_mut().zip(&assignments) {
                    if assignment == locked_cluster && *label == UNASSIGNED {
                        *label = locked_cluster;
                        unassigned_count -= 1;
                    }
                }
            }
        }

        if !any_changed {
            break;
        }
    }

    for (label, &assignment) in labels.iter_mut().zip(&assignments) {
        if *label == UNASSIGNED {
            *label = assignment;
        }
    }

    let mut swatches = [Swatch { color: Color::default(), pixel_count: 0, active: false }; NUM_CLUSTERS];
    for cluster_idx in 0..NUM_CLUSTERS {
        let center = denormalize_center(centers[cluster_idx], stats);
        swatches[cluster_idx] = Swatch { color: Color::new(center[0], center[1], center[2]), pixel_count: 0, active: active[cluster_idx] };
    }

    for &label in &labels {
        swatches[label as usize].pixel_count += 1;
    }

    (swatches, labels)
}

fn select_best_offset(data: &[[f64; 3]], stats: &ZScore) -> ([Swatch; NUM_CLUSTERS], Vec<u8>) {
    let total_pixels = data.len();

    let mut best_result = run_hue_locked_kmeans(data, stats, HUE_OFFSETS[0]);
    let mut best_score = score_active_clusters(&best_result.0, total_pixels);

    for &offset in &HUE_OFFSETS[1..] {
        let candidate = run_hue_locked_kmeans(data, stats, offset);
        let candidate_score = score_active_clusters(&candidate.0, total_pixels);
        if candidate_score > best_score {
            best_result = candidate;
            best_score = candidate_score;
        }
    }

    best_result
}

fn score_active_clusters(swatches: &[Swatch; NUM_CLUSTERS], total_pixels: usize) -> (usize, f64) {
    let (active_count, total_cluster_pixels) = swatches
        .iter()
        .filter(|swatch| swatch.active)
        .fold((0, 0usize), |(count, pixels), swatch| (count + 1, pixels + swatch.pixel_count));

    (active_count, total_cluster_pixels as f64 / total_pixels as f64)
}

pub fn extract_dominant_colors(pixels: Vec<Color>) -> ClusteringResult {
    let filtered_pixels: Vec<Color> = pixels.iter().copied().filter(|pixel| pixel.chroma >= CHROMA_THRESHOLD).collect();
    let avg_chroma = filtered_pixels.iter().map(|p| p.chroma).sum::<f64>() / filtered_pixels.len() as f64;

    let mut data: Vec<[f64; 3]> = filtered_pixels.iter().map(|p| [p.l, p.chroma, p.hue]).collect();
    let stats = ZScore::normalize_in_place(&mut data);
    let (swatches, labels) = select_best_offset(&data, &stats);

    ClusteringResult { swatches: swatches.to_vec(), labels, avg_chroma, pixels: filtered_pixels }
}
