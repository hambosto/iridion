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
        let accents = boost_contrast(&result.swatches, contrast, result.avg_chroma);
        let (primary, secondary) = detect_dual_tone(&result.swatches, &result.labels, &result.pixels);
        let (lighter_tone, darker_tone) = if primary.l > secondary.l { (primary, secondary) } else { (secondary, primary) };

        let mut colors = [Color::default(); 16];
        for (slot, &lightness) in colors[..6].iter_mut().zip(&BG_LIGHTNESS) {
            *slot = Color::new(lightness, primary.chroma / 2.0, primary.hue);
        }

        colors[6] = Color::new(FG_LIGHTNESS, lighter_tone.chroma / 2.0, lighter_tone.hue);
        colors[7] = Color::new(BRIGHT_BG_LIGHTNESS, darker_tone.chroma / 2.0, darker_tone.hue);
        colors[8..].copy_from_slice(&accents);

        Self(colors)
    }

    pub fn to_json(&self) -> Result<String> {
        let map: Map<String, Value> = LABELS.iter().zip(self.0).map(|(&label, color)| (label.to_string(), Value::String(color.to_hex()))).collect();

        serde_json::to_string_pretty(&map).context("failed to serialize palette to JSON")
    }
}

fn boost_contrast(swatches: &[Swatch], contrast: f64, avg_chroma: f64) -> [Color; 8] {
    let mut centers = [Color::new(0.7, 0.04, 0.0); NUM_CLUSTERS];
    for (idx, swatch) in swatches.iter().take(NUM_CLUSTERS).enumerate() {
        centers[idx] = swatch.color;
    }

    let chroma_scale = 1.0 + contrast * 3.0;
    let mean_chroma = centers.iter().map(|c| c.chroma).sum::<f64>() / NUM_CLUSTERS as f64;
    if mean_chroma > 0.0 && chroma_scale != 1.0 {
        let chroma_factor = avg_chroma * chroma_scale / mean_chroma;
        for center in centers.iter_mut() {
            center.chroma *= chroma_factor;
        }
    }
    apply_lightness_floor(&mut centers, contrast);

    centers
}

fn apply_lightness_floor(centers: &mut [Color; 8], contrast: f64) {
    let (min_lightness, max_lightness) = centers
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lowest, highest), color| (lowest.min(color.l), highest.max(color.l)));

    if min_lightness >= MIN_LIGHTNESS_FLOOR {
        return;
    }

    let target_lightness = 0.7 + contrast * 0.3;
    if target_lightness > min_lightness {
        let blend_alpha = (MIN_LIGHTNESS_FLOOR - min_lightness) / (target_lightness - min_lightness);
        let projected_max = max_lightness + blend_alpha * (target_lightness - max_lightness);
        let clamped_alpha = if projected_max > 1.0 && target_lightness < max_lightness {
            blend_alpha.min((1.0 - max_lightness) / (target_lightness - max_lightness))
        } else {
            blend_alpha
        };
        if projected_max <= 1.0 || (target_lightness < max_lightness && clamped_alpha < 1.0) {
            for center in centers.iter_mut() {
                center.l += clamped_alpha * (target_lightness - center.l);
            }
            return;
        }
    }

    let lightness_range = max_lightness - min_lightness;
    for center in centers.iter_mut() {
        center.l = if lightness_range > 0.0 {
            (center.l - min_lightness) / lightness_range * (1.0 - MIN_LIGHTNESS_FLOOR) + MIN_LIGHTNESS_FLOOR
        } else {
            MIN_LIGHTNESS_FLOOR
        };
    }
}

fn zone_weighted_average(zone: &[&Swatch]) -> Color {
    let total_pixels: f64 = zone.iter().map(|swatch| swatch.pixel_count as f64).sum();
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

fn build_zones<'a>(active_swatches: &[&'a Swatch]) -> Vec<Vec<&'a Swatch>> {
    let mut zones: Vec<Vec<&Swatch>> = Vec::new();
    let mut current_zone = vec![active_swatches[0]];

    for &swatch in &active_swatches[1..] {
        let previous_hue = current_zone.last().map_or(0.0, |s| s.color.hue);
        if swatch.color.hue - previous_hue <= ZONE_MERGE_THRESHOLD {
            current_zone.push(swatch);
        } else {
            zones.push(std::mem::take(&mut current_zone));
            current_zone.clear();
            current_zone.push(swatch);
        }
    }
    zones.push(current_zone);

    if zones.len() > 1 {
        let first_hue = zones.first().and_then(|z| z.first()).map_or(0.0, |s| s.color.hue);
        let last_hue = zones.last().and_then(|z| z.last()).map_or(0.0, |s| s.color.hue);
        if (360.0 - last_hue) + first_hue <= ZONE_MERGE_THRESHOLD {
            let tail_zone = zones.pop();
            let head_zone = zones.drain(..1).next();
            if let (Some(mut tail), Some(head)) = (tail_zone, head_zone) {
                tail.extend(head);
                zones.insert(0, tail);
            }
        }
    }

    zones
}

fn merge_zones(active_swatches: &[&Swatch]) -> (Color, Color) {
    let zones = build_zones(active_swatches);
    if zones.len() == 1
        && let Some(single_zone) = zones.first()
        && let Some(first) = single_zone.first()
        && let Some(last) = single_zone.last()
    {
        return (first.color, last.color);
    }

    let mut ranked_zones: Vec<_> = zones
        .into_iter()
        .map(|zone| {
            let zone_pixels: usize = zone.iter().map(|swatch| swatch.pixel_count).sum();
            (zone, zone_pixels)
        })
        .filter(|(_, pixel_count)| *pixel_count > 0)
        .collect();
    ranked_zones.sort_by_key(|(_, pixel_count)| std::cmp::Reverse(*pixel_count));

    let neutral = Color::new(0.5, 0.0, 0.0);
    if ranked_zones.is_empty() {
        return (neutral, neutral);
    }

    if let [(zone, _)] = ranked_zones.as_slice() {
        let first = zone.first().map(|s| s.color).unwrap_or_default();
        let last = zone.last().map(|s| s.color).unwrap_or_default();
        return (first, last);
    }

    let (primary_zone, primary_pixels) = &ranked_zones[0];
    let (secondary_zone, secondary_pixels) = &ranked_zones[1];
    let primary_average = zone_weighted_average(primary_zone);
    let secondary_average = zone_weighted_average(secondary_zone);
    if primary_pixels > secondary_pixels {
        (primary_average, secondary_average)
    } else {
        (secondary_average, primary_average)
    }
}

fn single_cluster_lightness_range(cluster_index: usize, cluster_center: Color, labels: &[u8], pixels: &[Color]) -> (Color, Color) {
    let (min_lightness, max_lightness) = labels
        .iter()
        .zip(pixels)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lowest, highest), (&label, pixel)| if label as usize == cluster_index { (lowest.min(pixel.l), highest.max(pixel.l)) } else { (lowest, highest) });
    let (lo, hi) = if min_lightness.is_finite() { (min_lightness, max_lightness) } else { (cluster_center.l, cluster_center.l) };

    (Color::new(lo, cluster_center.chroma, cluster_center.hue), Color::new(hi, cluster_center.chroma, cluster_center.hue))
}

fn detect_dual_tone(swatches: &[Swatch], labels: &[u8], pixels: &[Color]) -> (Color, Color) {
    let mut active_swatches: Vec<(usize, &Swatch)> = swatches.iter().enumerate().filter(|(_, swatch)| swatch.active).collect();
    active_swatches.sort_by(|a, b| a.1.color.hue.total_cmp(&b.1.color.hue));

    let neutral = Color::new(0.5, 0.0, 0.0);
    if active_swatches.is_empty() {
        return (neutral, neutral);
    }

    if let [(index, swatch)] = active_swatches.as_slice() {
        return single_cluster_lightness_range(*index, swatch.color, labels, pixels);
    }
    let swatch_refs: Vec<&Swatch> = active_swatches.iter().map(|(_, s)| *s).collect();

    merge_zones(&swatch_refs)
}
