use palette::{Clamp, IntoColor, Oklch, Srgb};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Hue(f64);

impl Hue {
    pub fn new(degrees: f64) -> Self {
        Self(degrees.rem_euclid(360.0))
    }

    pub fn degrees(self) -> f64 {
        self.0
    }

    pub fn sector(self, half_width: f64) -> (Hue, Hue) {
        (Hue::new(self.0 - half_width), Hue::new(self.0 + half_width))
    }
}

pub(crate) fn sector_contains(value: f64, start: f64, end: f64) -> bool {
    if start < end { value >= start && value < end } else { value >= start || value < end }
}

pub(crate) fn sector_clamp(value: f64, start: f64, end: f64) -> f64 {
    if sector_contains(value, start, end) {
        return value;
    }

    if start < end {
        if value < start { start } else { end }
    } else {
        let to_start = circular_gap(value, start);
        let to_end = circular_gap(value, end);
        if to_start < to_end { start } else { end }
    }
}

fn circular_gap(a: f64, b: f64) -> f64 {
    let diff = (a - b).abs();
    diff.min(360.0 - diff)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub l: f64,
    pub chroma: f64,
    pub hue: Hue,
}

impl Color {
    pub fn new(l: f64, chroma: f64, hue: f64) -> Self {
        Self { l, chroma, hue: Hue::new(hue) }
    }

    pub fn to_rgb(self) -> (u8, u8, u8) {
        let srgb: Srgb<f64> = Oklch::new(self.l, self.chroma, self.hue.degrees()).into_color();
        let srgb = srgb.clamp();

        ((srgb.red * 255.0) as u8, (srgb.green * 255.0) as u8, (srgb.blue * 255.0) as u8)
    }

    pub fn to_hex(self) -> String {
        let (r, g, b) = self.to_rgb();

        hex::encode([r, g, b])
    }

    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        let oklch: Oklch<f64> = Srgb::new(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0).into_color();

        Self { l: oklch.l, chroma: oklch.chroma, hue: Hue::new(oklch.hue.into_inner()) }
    }
}
