use crate::color::{Color, Hue, sector_clamp, sector_contains};

pub const NUM_CLUSTERS: usize = 8;

const BASE_HUES: [f64; NUM_CLUSTERS] = [0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
const HUE_SECTOR_HALF_WIDTH: f64 = 22.5;
const HUE_OFFSETS: [f64; 3] = [0.0, 15.0, 30.0];
const MAX_ITERS: usize = 100;
const TOLERANCE: f64 = 1e-4;
const UNASSIGNED: u8 = u8::MAX;

#[derive(Clone, Copy, Debug)]
pub struct Cluster {
    pub center: Color,
    pub pixel_count: usize,
    pub active: bool,
}

pub struct ClusterSet {
    pub clusters: [Cluster; NUM_CLUSTERS],
    pub labels: Vec<u8>,
}

impl ClusterSet {
    pub fn new(pixels: &[Color]) -> Self {
        let n = pixels.len();
        let mut data: Vec<[f64; 3]> = pixels.iter().map(|p| [p.l, p.chroma, p.hue.degrees()]).collect();

        let mut mean = [0.0f64; 3];
        for row in &data {
            mean[0] += row[0];
            mean[1] += row[1];
            mean[2] += row[2];
        }
        let inv_n = 1.0 / n as f64;
        mean[0] *= inv_n;
        mean[1] *= inv_n;
        mean[2] *= inv_n;

        let mut var = [0.0f64; 3];
        for row in &data {
            let dx = row[0] - mean[0];
            let dy = row[1] - mean[1];
            let dz = row[2] - mean[2];
            var[0] += dx * dx;
            var[1] += dy * dy;
            var[2] += dz * dz;
        }

        let mut std = [0.0f64; 3];
        std[0] = (var[0] * inv_n).sqrt().max(1e-8);
        std[1] = (var[1] * inv_n).sqrt().max(1e-8);
        std[2] = (var[2] * inv_n).sqrt().max(1e-8);

        for row in data.iter_mut() {
            row[0] = (row[0] - mean[0]) / std[0];
            row[1] = (row[1] - mean[1]) / std[1];
            row[2] = (row[2] - mean[2]) / std[2];
        }

        let mut best = Self::run(&data, &mean, &std, HUE_OFFSETS[0]);
        let (mut best_active, mut best_ratio) = {
            let (c, p) = best.clusters.iter().filter(|c| c.active).fold((0, 0usize), |(count, pixels), c| (count + 1, pixels + c.pixel_count));
            (c, p as f64 / best.labels.len() as f64)
        };

        for &offset in &HUE_OFFSETS[1..] {
            let candidate = Self::run(&data, &mean, &std, offset);
            let (active, ratio) = {
                let (c, p) = candidate
                    .clusters
                    .iter()
                    .filter(|c| c.active)
                    .fold((0, 0usize), |(count, pixels), c| (count + 1, pixels + c.pixel_count));
                (c, p as f64 / candidate.labels.len() as f64)
            };
            if active > best_active || (active == best_active && ratio > best_ratio) {
                best = candidate;
                best_active = active;
                best_ratio = ratio;
            }
        }
        best
    }

    fn run(normalized: &[[f64; 3]], mean: &[f64; 3], std: &[f64; 3], offset: f64) -> Self {
        let n = normalized.len();

        let mut centers = [[0.0f64; 3]; NUM_CLUSTERS];
        for i in 0..NUM_CLUSTERS {
            let hue = Hue::new(BASE_HUES[i] + offset);
            centers[i][2] = (hue.degrees() - mean[2]) / std[2];
        }

        let mut sectors = [[0.0f64; 2]; NUM_CLUSTERS];
        for i in 0..NUM_CLUSTERS {
            let hue = Hue::new(BASE_HUES[i] + offset);
            let (lo, hi) = hue.sector(HUE_SECTOR_HALF_WIDTH);
            sectors[i][0] = (lo.degrees() - mean[2]) / std[2];
            sectors[i][1] = (hi.degrees() - mean[2]) / std[2];
        }

        let mut active = [true; NUM_CLUSTERS];
        let mut labels = vec![UNASSIGNED; n];
        let mut assignments = vec![0u8; n];
        let mut sum = [[0.0f64; 3]; NUM_CLUSTERS];
        let mut count = [0usize; NUM_CLUSTERS];
        let mut unassigned_count = n;

        for _ in 0..MAX_ITERS {
            if active.iter().all(|&a| !a) || unassigned_count == 0 {
                break;
            }

            for row in sum.iter_mut() {
                row[0] = 0.0;
                row[1] = 0.0;
                row[2] = 0.0;
            }
            count.fill(0);

            for i in 0..n {
                if labels[i] != UNASSIGNED {
                    continue;
                }

                let s0 = normalized[i][0];
                let s1 = normalized[i][1];
                let s2 = normalized[i][2];

                let mut best_dist = f64::INFINITY;
                let mut best_j = 0;
                for (j, is_active) in active.iter().enumerate() {
                    if !is_active {
                        continue;
                    }
                    let dx = s0 - centers[j][0];
                    let dy = s1 - centers[j][1];
                    let dz = s2 - centers[j][2];
                    let d = dx * dx + dy * dy + dz * dz;
                    if d < best_dist {
                        best_dist = d;
                        best_j = j;
                    }
                }

                assignments[i] = best_j as u8;
                sum[best_j][0] += s0;
                sum[best_j][1] += s1;
                sum[best_j][2] += s2;
                count[best_j] += 1;
            }

            let mut any_moved = false;
            let mut any_locked = false;

            for i in 0..NUM_CLUSTERS {
                if !active[i] || count[i] == 0 {
                    continue;
                }

                let inv = 1.0 / count[i] as f64;
                let new_l = sum[i][0] * inv;
                let new_c = sum[i][1] * inv;
                let new_h = sum[i][2] * inv;
                let dx = new_l - centers[i][0];
                let dy = new_c - centers[i][1];
                let dz = new_h - centers[i][2];
                if dx * dx + dy * dy + dz * dz > TOLERANCE * TOLERANCE {
                    any_moved = true;
                }

                let lo = sectors[i][0];
                let hi = sectors[i][1];
                if sector_contains(new_h, lo, hi) {
                    centers[i][0] = new_l;
                    centers[i][1] = new_c;
                    centers[i][2] = new_h;
                    continue;
                }

                centers[i][0] = new_l;
                centers[i][1] = new_c;
                centers[i][2] = sector_clamp(new_h, lo, hi);
                active[i] = false;
                any_locked = true;

                let idx = i as u8;
                for (label, &a) in labels.iter_mut().zip(&assignments) {
                    if a == idx && *label == UNASSIGNED {
                        *label = idx;
                        unassigned_count -= 1;
                    }
                }
            }

            if !any_moved && !any_locked {
                break;
            }
        }

        for (label, &a) in labels.iter_mut().zip(&assignments) {
            if *label == UNASSIGNED {
                *label = a;
            }
        }

        let mut clusters = [Cluster { center: Color::default(), pixel_count: 0, active: false }; NUM_CLUSTERS];
        for i in 0..NUM_CLUSTERS {
            clusters[i] = Cluster { center: Color::new(centers[i][0] * std[0] + mean[0], centers[i][1] * std[1] + mean[1], centers[i][2] * std[2] + mean[2]), pixel_count: 0, active: active[i] };
        }

        for &label in &labels {
            clusters[label as usize].pixel_count += 1;
        }

        Self { clusters, labels }
    }
}
