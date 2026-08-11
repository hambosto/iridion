use std::cmp::Reverse;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::cluster::{ClusteringResult, NUM_CLUSTERS, Swatch};
use crate::color::Color;

const ZONE_MERGE_THRESHOLD: f64 = 50.0;
const BG_LIGHTNESS: [f64; 6] = [0.24, 0.22, 0.32, 0.40, 0.48, 0.88];
const FG_LIGHTNESS: f64 = 0.90;
const BRIGHT_BG_LIGHTNESS: f64 = 0.60;
const MIN_LIGHTNESS_FLOOR: f64 = 0.6;
const LABELS: [&str; 16] = ["base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07", "base08", "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F"];

pub struct Palette([Color; 16]);

impl Palette {
    pub fn from_clusters(result: &ClusteringResult, contrast: f64) -> Self {
        let contrast = contrast.clamp(0.0, 1.0);
        let accents = build_accent_colors(result, contrast);
        let zones = extract_dominant_zones(result);
        let (lighter, darker) = if zones.color1.l > zones.color2.l { (zones.color1, zones.color2) } else { (zones.color2, zones.color1) };

        let mut colors = [Color::default(); 16];
        for (slot, &lightness) in colors[..6].iter_mut().zip(&BG_LIGHTNESS) {
            *slot = Color::new(lightness, zones.dominant.chroma / 2.0, zones.dominant.hue);
        }

        colors[6] = Color::new(FG_LIGHTNESS, lighter.chroma / 2.0, lighter.hue);
        colors[7] = Color::new(BRIGHT_BG_LIGHTNESS, darker.chroma / 2.0, darker.hue);
        colors[8..].copy_from_slice(&accents);

        Self(colors)
    }

    pub fn to_json_string(&self) -> Result<String> {
        let map: Map<String, Value> = LABELS.iter().zip(self.0).map(|(&label, color)| (label.to_owned(), Value::String(color.to_hex()))).collect();

        serde_json::to_string_pretty(&map).context("failed to serialize palette to JSON")
    }
}

fn build_accent_colors(result: &ClusteringResult, contrast: f64) -> [Color; NUM_CLUSTERS] {
    let mut centers = [Color::default(); NUM_CLUSTERS];
    for (c, s) in centers.iter_mut().zip(&result.swatches) {
        *c = s.color;
    }

    let chroma_scale = 1.0 + contrast * 3.0;
    let target_avg_c = result.avg_chroma * chroma_scale;
    let current_mean = centers.iter().map(|c| c.chroma).sum::<f64>() / NUM_CLUSTERS as f64;
    if current_mean > 0.0 && chroma_scale != 1.0 {
        let factor = target_avg_c / current_mean;
        for c in &mut centers {
            c.chroma *= factor;
        }
    }

    let target = 0.7 + contrast * 0.3;
    let mut min_l = f64::INFINITY;
    let mut max_l = f64::NEG_INFINITY;

    for c in &centers {
        min_l = min_l.min(c.l);
        max_l = max_l.max(c.l);
    }

    if min_l < MIN_LIGHTNESS_FLOOR {
        if target <= min_l {
            remap_lightness(&mut centers, min_l, max_l);
        } else {
            let mut alpha = (MIN_LIGHTNESS_FLOOR - min_l) / (target - min_l);
            let projected_max = max_l + alpha * (target - max_l);

            if projected_max > 1.0 {
                if target >= max_l {
                    remap_lightness(&mut centers, min_l, max_l);
                    return centers;
                }
                alpha = alpha.min((1.0 - max_l) / (target - max_l));
            }

            for c in &mut centers {
                c.l += alpha * (target - c.l);
            }
        }
    }

    centers
}

fn remap_lightness(centers: &mut [Color; NUM_CLUSTERS], min_l: f64, max_l: f64) {
    let range = max_l - min_l;
    for c in centers.iter_mut() {
        c.l = if range > 0.0 {
            (c.l - min_l) / range * (1.0 - MIN_LIGHTNESS_FLOOR) + MIN_LIGHTNESS_FLOOR
        } else {
            MIN_LIGHTNESS_FLOOR
        };
    }
}

struct DominantZones {
    color1: Color,
    color2: Color,
    dominant: Color,
}

fn extract_dominant_zones(result: &ClusteringResult) -> DominantZones {
    let neutral = Color::new(0.5, 0.0, 0.0);
    let mut active: Vec<(usize, &Swatch)> = result.swatches.iter().enumerate().filter(|(_, s)| s.active).collect();
    active.sort_by(|a, b| a.1.color.hue.total_cmp(&b.1.color.hue));

    match active.as_slice() {
        [] => DominantZones { color1: neutral, color2: neutral, dominant: neutral },

        [(idx, swatch)] => {
            let mut min_l = f64::INFINITY;
            let mut max_l = f64::NEG_INFINITY;
            for (&label, pixel) in result.labels.iter().zip(&result.pixels) {
                if label as usize == *idx {
                    min_l = min_l.min(pixel.l);
                    max_l = max_l.max(pixel.l);
                }
            }
            let (lo, hi) = if min_l.is_finite() { (min_l, max_l) } else { (swatch.color.l, swatch.color.l) };
            let color1 = Color::new(lo, swatch.color.chroma, swatch.color.hue);
            let color2 = Color::new(hi, swatch.color.chroma, swatch.color.hue);

            DominantZones { color1, color2, dominant: color1 }
        }

        _ => extract_multi_cluster_zones(&active),
    }
}

fn extract_multi_cluster_zones(active: &[(usize, &Swatch)]) -> DominantZones {
    let mut zones: Vec<Vec<(usize, &Swatch)>> = vec![vec![active[0]]];
    for &(idx, swatch) in &active[1..] {
        let prev_hue = zones.last().and_then(|z| z.last()).map_or(0.0, |(_, s)| s.color.hue);
        if swatch.color.hue - prev_hue <= ZONE_MERGE_THRESHOLD {
            if let Some(last_zone) = zones.last_mut() {
                last_zone.push((idx, swatch));
            }
        } else {
            zones.push(vec![(idx, swatch)]);
        }
    }

    if zones.len() > 1 {
        let first_hue = zones[0].first().map_or(0.0, |(_, s)| s.color.hue);
        let last_hue = zones.last().and_then(|z| z.last()).map_or(0.0, |(_, s)| s.color.hue);

        if (360.0 - last_hue) + first_hue <= ZONE_MERGE_THRESHOLD
            && let Some(mut wrapped) = zones.pop()
        {
            wrapped.append(&mut zones[0]);
            zones[0] = wrapped;
        }
    }

    match zones.as_slice() {
        [zone] => {
            let color1 = zone.first().map_or(Color::default(), |(_, s)| s.color);
            let color2 = zone.last().map_or(Color::default(), |(_, s)| s.color);

            DominantZones { color1, color2, dominant: color1 }
        }

        _ => {
            zones.sort_by_key(|zone| Reverse(zone.iter().map(|(_, s)| s.pixel_count).sum::<usize>()));

            let zone1_pixels: usize = zones[0].iter().map(|(_, s)| s.pixel_count).sum();
            let zone2_pixels: usize = zones[1].iter().map(|(_, s)| s.pixel_count).sum();

            let color1 = zone_weighted_average(&zones[0]);
            let color2 = zone_weighted_average(&zones[1]);
            let dominant = if zone1_pixels >= zone2_pixels { color1 } else { color2 };

            DominantZones { color1, color2, dominant }
        }
    }
}

fn zone_weighted_average(zone: &[(usize, &Swatch)]) -> Color {
    let total: f64 = zone.iter().map(|(_, s)| s.pixel_count as f64).sum();
    if total == 0.0 {
        return Color::new(0.5, 0.0, 0.0);
    }

    let mut l_sum = 0.0;
    let mut chroma_sum = 0.0;
    let mut cos_sum = 0.0;
    let mut sin_sum = 0.0;

    for (_, s) in zone {
        let weight = s.pixel_count as f64 / total;
        l_sum += s.color.l * weight;
        chroma_sum += s.color.chroma * weight;

        let rad = s.color.hue.to_radians();
        cos_sum += rad.cos() * weight;
        sin_sum += rad.sin() * weight;
    }

    Color::new(l_sum, chroma_sum, sin_sum.atan2(cos_sum).to_degrees())
}
