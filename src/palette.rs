use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::cluster::{Cluster, ClusterSet, NUM_CLUSTERS};
use crate::color::Color;

const ZONE_MERGE_HUE_THRESHOLD: f64 = 50.0;
const BG_LIGHTNESS: [f64; 6] = [0.24, 0.22, 0.32, 0.40, 0.48, 0.88];
const FG_LIGHTNESS: f64 = 0.90;
const BRIGHT_BG_LIGHTNESS: f64 = 0.60;
const MIN_LIGHTNESS_FLOOR: f64 = 0.6;
const LABELS: [&str; 16] = ["base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07", "base08", "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F"];

pub struct Palette {
    colors: [Color; 16],
}

impl Palette {
    pub fn new(cluster_set: &ClusterSet, contrast: f64, avg_chroma: f64, pixels: &[Color]) -> Self {
        let accents = Self::contrast_boost(&cluster_set.clusters, contrast, avg_chroma);
        let (primary, secondary) = Self::find_dual_tone(cluster_set, pixels);
        let (lighter, darker) = if primary.l > secondary.l { (primary, secondary) } else { (secondary, primary) };

        let mut colors = [Color::default(); 16];
        for (slot, l) in colors[..6].iter_mut().zip(BG_LIGHTNESS) {
            *slot = Color::new(l, primary.chroma / 2.0, primary.hue.degrees());
        }

        colors[6] = Color::new(FG_LIGHTNESS, lighter.chroma / 2.0, lighter.hue.degrees());
        colors[7] = Color::new(BRIGHT_BG_LIGHTNESS, darker.chroma / 2.0, darker.hue.degrees());
        colors[8..].copy_from_slice(&accents);

        Self { colors }
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        let map: Map<String, Value> = LABELS.iter().zip(self.colors).map(|(&k, c)| (k.to_string(), Value::String(c.to_hex()))).collect();

        serde_json::to_string_pretty(&map).context("failed to serialize theme to JSON")
    }

    fn contrast_boost(clusters: &[Cluster; NUM_CLUSTERS], contrast: f64, avg_chroma: f64) -> [Color; NUM_CLUSTERS] {
        let mut centers = [Color::default(); NUM_CLUSTERS];
        for (c, cl) in centers.iter_mut().zip(clusters) {
            *c = cl.center;
        }

        let scale = 1.0 + contrast * 3.0;
        let mean_chroma = centers.iter().map(|c| c.chroma).sum::<f64>() / NUM_CLUSTERS as f64;

        if mean_chroma > 0.0 && scale != 1.0 {
            let factor = avg_chroma * scale / mean_chroma;
            for c in centers.iter_mut() {
                c.chroma *= factor;
            }
        }

        let (min_l, max_l) = centers.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), c| (mn.min(c.l), mx.max(c.l)));
        if min_l < MIN_LIGHTNESS_FLOOR {
            let target = 0.7 + contrast * 0.3;
            if target > min_l {
                let alpha = (MIN_LIGHTNESS_FLOOR - min_l) / (target - min_l);
                let max_new_l = max_l + alpha * (target - max_l);
                let alpha = if max_new_l > 1.0 && target < max_l { alpha.min((1.0 - max_l) / (target - max_l)) } else { alpha };
                if max_new_l <= 1.0 || (target < max_l && alpha < 1.0) {
                    for c in centers.iter_mut() {
                        c.l += alpha * (target - c.l);
                    }
                    return centers;
                }
            }

            let range = max_l - min_l;
            for c in centers.iter_mut() {
                c.l = if range > 0.0 {
                    (c.l - min_l) / range * (1.0 - MIN_LIGHTNESS_FLOOR) + MIN_LIGHTNESS_FLOOR
                } else {
                    MIN_LIGHTNESS_FLOOR
                };
            }
        }

        centers
    }

    fn weighted_average(zone: &[&Cluster]) -> Color {
        let total: f64 = zone.iter().map(|c| c.pixel_count as f64).sum();
        let inv = 1.0 / total;
        let mut lx = 0.0;
        let mut cx = 0.0;
        let mut hx = 0.0;
        let mut hy = 0.0;
        for c in zone {
            let w = c.pixel_count as f64 * inv;
            lx += c.center.l * w;
            cx += c.center.chroma * w;
            let r = c.center.hue.degrees().to_radians();
            hx += r.cos() * w;
            hy += r.sin() * w;
        }
        Color::new(lx, cx, hy.atan2(hx).to_degrees())
    }

    fn merge_zones(active: &[(usize, &Cluster)]) -> (Color, Color) {
        let mut zones: Vec<Vec<&Cluster>> = Vec::new();
        let mut current = vec![active[0].1];

        for &(_, cluster) in &active[1..] {
            let prev = current.last().unwrap().center.hue.degrees();
            if cluster.center.hue.degrees() - prev <= ZONE_MERGE_HUE_THRESHOLD {
                current.push(cluster);
            } else {
                zones.push(std::mem::take(&mut current));
                current.clear();
                current.push(cluster);
            }
        }
        zones.push(current);

        if zones.len() > 1 {
            let first = zones.first().unwrap().first().unwrap().center.hue.degrees();
            let last = zones.last().unwrap().last().unwrap().center.hue.degrees();
            if (360.0 - last) + first <= ZONE_MERGE_HUE_THRESHOLD {
                let mut tail = zones.pop().unwrap();
                let mut head = zones.remove(0);

                tail.append(&mut head);
                zones.insert(0, tail);
            }
        }

        if zones.len() == 1 {
            let z = &zones[0];
            return (z.first().unwrap().center, z.last().unwrap().center);
        }

        let mut ranked: Vec<_> = zones
            .into_iter()
            .map(|z| {
                let px: usize = z.iter().map(|c| c.pixel_count).sum();
                (z, px)
            })
            .filter(|(_, px)| *px > 0)
            .collect();
        ranked.sort_by_key(|x| std::cmp::Reverse(x.1));

        match ranked.as_slice() {
            [] => {
                let n = Color::new(0.5, 0.0, 0.0);
                (n, n)
            }
            [(z, _)] => (z.first().unwrap().center, z.last().unwrap().center),
            [(z1, p1), (z2, p2), ..] => {
                let a = Self::weighted_average(z1);
                let b = Self::weighted_average(z2);
                if p1 > p2 { (a, b) } else { (b, a) }
            }
        }
    }

    fn single_cluster_dual(index: usize, center: Color, cluster_set: &ClusterSet, pixels: &[Color]) -> (Color, Color) {
        let (min_l, max_l) = cluster_set.labels.iter().zip(pixels).fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(min_l, max_l), (&label, p)| {
                if label as usize == index { (min_l.min(p.l), max_l.max(p.l)) } else { (min_l, max_l) }
            },
        );
        let (lo, hi) = if min_l.is_finite() { (min_l, max_l) } else { (center.l, center.l) };

        (Color::new(lo, center.chroma, center.hue.degrees()), Color::new(hi, center.chroma, center.hue.degrees()))
    }

    fn find_dual_tone(cluster_set: &ClusterSet, pixels: &[Color]) -> (Color, Color) {
        let mut active: Vec<(usize, &Cluster)> = cluster_set.clusters.iter().enumerate().filter(|(_, c)| c.active).collect();
        active.sort_by(|a, b| a.1.center.hue.degrees().total_cmp(&b.1.center.hue.degrees()));

        match active.as_slice() {
            [] => {
                let n = Color::new(0.5, 0.0, 0.0);
                (n, n)
            }
            [(i, c)] => Self::single_cluster_dual(*i, c.center, cluster_set, pixels),
            _ => Self::merge_zones(&active),
        }
    }
}
