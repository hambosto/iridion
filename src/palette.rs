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
    pub fn generate(result: &ClusteringResult, contrast: f64) -> Self {
        let contrast = contrast.clamp(0.0, 1.0);
        let accents = Self::accents(result, contrast);
        let (primary, secondary) = Self::dual_tone(result);
        let (lighter, darker) = if primary.l > secondary.l { (primary, secondary) } else { (secondary, primary) };

        let mut colors = [Color::default(); 16];
        for (slot, &lightness) in colors[..6].iter_mut().zip(&BG_LIGHTNESS) {
            *slot = Color::new(lightness, primary.chroma / 2.0, primary.hue);
        }

        colors[6] = Color::new(FG_LIGHTNESS, lighter.chroma / 2.0, lighter.hue);
        colors[7] = Color::new(BRIGHT_BG_LIGHTNESS, darker.chroma / 2.0, darker.hue);
        colors[8..].copy_from_slice(&accents);

        Self(colors)
    }

    pub fn to_json(&self) -> Result<String> {
        let map: Map<String, Value> = LABELS.iter().zip(self.0).map(|(&label, color)| (label.to_string(), Value::String(color.to_hex()))).collect();

        serde_json::to_string_pretty(&map).context("failed to serialize palette to JSON")
    }

    fn accents(result: &ClusteringResult, contrast: f64) -> [Color; 8] {
        let mut centers = [Color::new(0.7, 0.04, 0.0); NUM_CLUSTERS];
        for (center, swatch) in centers.iter_mut().zip(&result.swatches) {
            *center = swatch.color;
        }

        let chroma_scale = 1.0 + contrast * 3.0;
        let mean_chroma = centers.iter().map(|c| c.chroma).sum::<f64>() / NUM_CLUSTERS as f64;
        if mean_chroma > 0.0 && chroma_scale != 1.0 {
            let chroma_factor = result.avg_chroma * chroma_scale / mean_chroma;
            for center in centers.iter_mut() {
                center.chroma *= chroma_factor;
            }
        }

        let mut min_lightness = f64::INFINITY;
        let mut max_lightness = f64::NEG_INFINITY;
        for center in &centers {
            min_lightness = min_lightness.min(center.l);
            max_lightness = max_lightness.max(center.l);
        }

        if min_lightness < MIN_LIGHTNESS_FLOOR {
            let target = 0.7 + contrast * 0.3;

            let blend_alpha = (target > min_lightness).then(|| {
                let alpha = (MIN_LIGHTNESS_FLOOR - min_lightness) / (target - min_lightness);
                let projected_max = max_lightness + alpha * (target - max_lightness);
                (alpha, projected_max)
            });

            let blend_alpha = match blend_alpha {
                Some((alpha, projected_max)) if projected_max <= 1.0 => Some(alpha),
                Some((alpha, _)) if target < max_lightness => {
                    let capped = alpha.min((1.0 - max_lightness) / (target - max_lightness));
                    (capped < 1.0).then_some(capped)
                }
                _ => None,
            };

            match blend_alpha {
                Some(alpha) => {
                    for center in centers.iter_mut() {
                        center.l += alpha * (target - center.l);
                    }
                }
                None => {
                    let range = max_lightness - min_lightness;
                    for center in centers.iter_mut() {
                        center.l = if range > 0.0 {
                            (center.l - min_lightness) / range * (1.0 - MIN_LIGHTNESS_FLOOR) + MIN_LIGHTNESS_FLOOR
                        } else {
                            MIN_LIGHTNESS_FLOOR
                        };
                    }
                }
            }
        }

        centers
    }

    fn dual_tone(result: &ClusteringResult) -> (Color, Color) {
        let mut active: Vec<(usize, &Swatch)> = result.swatches.iter().enumerate().filter(|(_, s)| s.active).collect();
        active.sort_by(|a, b| a.1.color.hue.total_cmp(&b.1.color.hue));

        let neutral = Color::new(0.5, 0.0, 0.0);

        match active.as_slice() {
            [] => (neutral, neutral),

            [(cluster_index, swatch)] => {
                let center = swatch.color;
                let mut min_l = f64::INFINITY;
                let mut max_l = f64::NEG_INFINITY;
                for (&label, pixel) in result.labels.iter().zip(&result.pixels) {
                    if label as usize == *cluster_index {
                        min_l = min_l.min(pixel.l);
                        max_l = max_l.max(pixel.l);
                    }
                }
                let (lo, hi) = if min_l.is_finite() { (min_l, max_l) } else { (center.l, center.l) };
                (Color::new(lo, center.chroma, center.hue), Color::new(hi, center.chroma, center.hue))
            }

            _ => {
                let hue_ordered: Vec<&Swatch> = active.iter().map(|(_, s)| *s).collect();
                let n = hue_ordered.len();
                let hue_gap = |from: f64, to: f64| {
                    if to >= from { to - from } else { 360.0 - from + to }
                };

                let mut widest_gap = 0.0;
                let mut widest_gap_start = 0;
                for i in 0..n {
                    let previous_hue = hue_ordered[(i + n - 1) % n].color.hue;
                    let gap = hue_gap(previous_hue, hue_ordered[i].color.hue);
                    if gap > widest_gap {
                        widest_gap = gap;
                        widest_gap_start = i;
                    }
                }
                let mut cut_ordered = Vec::with_capacity(n);
                cut_ordered.extend_from_slice(&hue_ordered[widest_gap_start..]);
                cut_ordered.extend_from_slice(&hue_ordered[..widest_gap_start]);

                let mut zones: Vec<Vec<&Swatch>> = Vec::new();
                let mut current_zone = vec![cut_ordered[0]];
                for &swatch in &cut_ordered[1..] {
                    let previous_hue = current_zone.last().unwrap().color.hue;
                    if hue_gap(previous_hue, swatch.color.hue) <= ZONE_MERGE_THRESHOLD {
                        current_zone.push(swatch);
                    } else {
                        zones.push(std::mem::take(&mut current_zone));
                        current_zone.push(swatch);
                    }
                }
                zones.push(current_zone);

                if let [zone] = zones.as_slice() {
                    let first = zone.first().map_or_else(Color::default, |s| s.color);
                    let last = zone.last().map_or_else(Color::default, |s| s.color);
                    return (first, last);
                }

                let mut ranked: Vec<(Vec<&Swatch>, usize)> = zones
                    .into_iter()
                    .map(|zone| {
                        let pixel_count = zone.iter().map(|s| s.pixel_count).sum();
                        (zone, pixel_count)
                    })
                    .filter(|&(_, pixel_count)| pixel_count > 0)
                    .collect();
                ranked.sort_by_key(|&(_, pixel_count)| Reverse(pixel_count));

                match ranked.as_slice() {
                    [] => (neutral, neutral),
                    [(zone, _)] => {
                        let first = zone.first().map_or(neutral, |s| s.color);
                        let last = zone.last().map_or(neutral, |s| s.color);
                        (first, last)
                    }
                    [primary, secondary, ..] => {
                        let primary_average = Self::zone_weighted_average(&primary.0);
                        let secondary_average = Self::zone_weighted_average(&secondary.0);
                        if primary.1 > secondary.1 { (primary_average, secondary_average) } else { (secondary_average, primary_average) }
                    }
                }
            }
        }
    }

    fn zone_weighted_average(zone: &[&Swatch]) -> Color {
        let total_pixels: f64 = zone.iter().map(|s| s.pixel_count as f64).sum();
        if total_pixels == 0.0 {
            return Color::new(0.5, 0.0, 0.0);
        }

        let reciprocal_total = 1.0 / total_pixels;
        let mut lightness_sum = 0.0;
        let mut chroma_sum = 0.0;
        let mut hue_cos_sum = 0.0;
        let mut hue_sin_sum = 0.0;
        for swatch in zone {
            let weight = swatch.pixel_count as f64 * reciprocal_total;
            lightness_sum += swatch.color.l * weight;
            chroma_sum += swatch.color.chroma * weight;

            let hue_radians = swatch.color.hue.to_radians();
            hue_cos_sum += hue_radians.cos() * weight;
            hue_sin_sum += hue_radians.sin() * weight;
        }

        Color::new(lightness_sum, chroma_sum, hue_sin_sum.atan2(hue_cos_sum).to_degrees())
    }
}
