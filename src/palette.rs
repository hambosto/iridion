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
    pub fn generate_from_clusters(result: &ClusteringResult, contrast: f64) -> Self {
        let contrast = contrast.clamp(0.0, 1.0);
        let accents = build_accent_colors(result, contrast);
        let (primary, secondary) = find_dual_tones(result);
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

    pub fn to_json_string(&self) -> Result<String> {
        let map: Map<String, Value> = LABELS.iter().zip(self.0).map(|(&label, color)| (label.to_string(), Value::String(color.to_hex()))).collect();

        serde_json::to_string_pretty(&map).context("failed to serialize palette to JSON")
    }
}

fn build_accent_colors(result: &ClusteringResult, contrast: f64) -> [Color; 8] {
    let mut centers = [Color::new(0.7, 0.04, 0.0); NUM_CLUSTERS];
    for (c, s) in centers.iter_mut().zip(&result.swatches) {
        *c = s.color;
    }

    let scale = 1.0 + contrast * 3.0;
    let mean_ch = centers.iter().map(|c| c.chroma).sum::<f64>() / NUM_CLUSTERS as f64;
    if mean_ch > 0.0 && scale != 1.0 {
        let factor = result.avg_chroma * scale / mean_ch;
        for c in &mut centers {
            c.chroma *= factor;
        }
    }

    let mut min_l = f64::INFINITY;
    let mut max_l = f64::NEG_INFINITY;
    for c in &centers {
        min_l = min_l.min(c.l);
        max_l = max_l.max(c.l);
    }

    if min_l < MIN_LIGHTNESS_FLOOR {
        let target = 0.7 + contrast * 0.3;

        let blend = (target > min_l).then(|| {
            let a = (MIN_LIGHTNESS_FLOOR - min_l) / (target - min_l);
            let proj_max = max_l + a * (target - max_l);
            (a, proj_max)
        });

        let blend = match blend {
            Some((a, proj_max)) if proj_max <= 1.0 => Some(a),
            Some((a, _)) if target < max_l => {
                let capped = a.min((1.0 - max_l) / (target - max_l));
                (capped < 1.0).then_some(capped)
            }
            _ => None,
        };

        if let Some(a) = blend {
            for c in &mut centers {
                c.l += a * (target - c.l);
            }
        } else {
            let range = max_l - min_l;
            for c in &mut centers {
                c.l = if range > 0.0 {
                    (c.l - min_l) / range * (1.0 - MIN_LIGHTNESS_FLOOR) + MIN_LIGHTNESS_FLOOR
                } else {
                    MIN_LIGHTNESS_FLOOR
                };
            }
        }
    }

    centers
}

fn find_dual_tones(result: &ClusteringResult) -> (Color, Color) {
    let mut active: Vec<(usize, &Swatch)> = result.swatches.iter().enumerate().filter(|(_, s)| s.active).collect();
    active.sort_by(|a, b| a.1.color.hue.total_cmp(&b.1.color.hue));

    let neutral = Color::new(0.5, 0.0, 0.0);
    match active.as_slice() {
        [] => (neutral, neutral),

        [(idx, swatch)] => {
            let c = swatch.color;
            let mut min_l = f64::INFINITY;
            let mut max_l = f64::NEG_INFINITY;
            for (&label, px) in result.labels.iter().zip(&result.pixels) {
                if label as usize == *idx {
                    min_l = min_l.min(px.l);
                    max_l = max_l.max(px.l);
                }
            }
            let (lo, hi) = if min_l.is_finite() { (min_l, max_l) } else { (c.l, c.l) };
            (Color::new(lo, c.chroma, c.hue), Color::new(hi, c.chroma, c.hue))
        }

        _ => {
            let by_hue: Vec<&Swatch> = active.iter().map(|(_, s)| *s).collect();
            let n = by_hue.len();
            let gap = |from: f64, to: f64| {
                if to >= from { to - from } else { 360.0 - from + to }
            };

            let mut max_gap = 0.0;
            let mut gap_start = 0;
            for i in 0..n {
                let prev = by_hue[(i + n - 1) % n].color.hue;
                let g = gap(prev, by_hue[i].color.hue);
                if g > max_gap {
                    max_gap = g;
                    gap_start = i;
                }
            }
            let mut rotated = Vec::with_capacity(n);
            rotated.extend_from_slice(&by_hue[gap_start..]);
            rotated.extend_from_slice(&by_hue[..gap_start]);

            let mut zones: Vec<Vec<&Swatch>> = Vec::new();
            let mut zone = vec![rotated[0]];
            for &sw in &rotated[1..] {
                let prev = zone.last().unwrap().color.hue;
                if gap(prev, sw.color.hue) <= ZONE_MERGE_THRESHOLD {
                    zone.push(sw);
                } else {
                    zones.push(std::mem::take(&mut zone));
                    zone.push(sw);
                }
            }
            zones.push(zone);

            if let [z] = zones.as_slice() {
                let first = z.first().map_or_else(Color::default, |s| s.color);
                let last = z.last().map_or_else(Color::default, |s| s.color);
                return (first, last);
            }

            let mut ranked: Vec<(Vec<&Swatch>, usize)> = zones
                .into_iter()
                .map(|z| {
                    let count = z.iter().map(|s| s.pixel_count).sum();
                    (z, count)
                })
                .filter(|&(_, count)| count > 0)
                .collect();
            ranked.sort_by_key(|&(_, count)| Reverse(count));

            match ranked.as_slice() {
                [] => (neutral, neutral),
                [(z, _)] => {
                    let first = z.first().map_or(neutral, |s| s.color);
                    let last = z.last().map_or(neutral, |s| s.color);
                    (first, last)
                }
                [primary, secondary, ..] => {
                    let p_avg = zone_weighted_average(&primary.0);
                    let s_avg = zone_weighted_average(&secondary.0);
                    if primary.1 > secondary.1 { (p_avg, s_avg) } else { (s_avg, p_avg) }
                }
            }
        }
    }
}

fn zone_weighted_average(zone: &[&Swatch]) -> Color {
    let total: f64 = zone.iter().map(|s| s.pixel_count as f64).sum();
    if total == 0.0 {
        return Color::new(0.5, 0.0, 0.0);
    }

    let inv = 1.0 / total;
    let mut l_sum = 0.0;
    let mut ch_sum = 0.0;
    let mut cos_sum = 0.0;
    let mut sin_sum = 0.0;
    for s in zone {
        let w = s.pixel_count as f64 * inv;
        l_sum += s.color.l * w;
        ch_sum += s.color.chroma * w;

        let rad = s.color.hue.to_radians();
        cos_sum += rad.cos() * w;
        sin_sum += rad.sin() * w;
    }

    Color::new(l_sum, ch_sum, sin_sum.atan2(cos_sum).to_degrees())
}
