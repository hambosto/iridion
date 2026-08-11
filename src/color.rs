use palette::{Clamp, IntoColor, Oklch, Srgb};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub l: f64,
    pub chroma: f64,
    pub hue: f64,
}

impl Color {
    pub fn new(l: f64, chroma: f64, hue: f64) -> Self {
        Self { l, chroma, hue: hue.rem_euclid(360.0) }
    }

    pub fn from_srgb(r: u8, g: u8, b: u8) -> Self {
        let oklch: Oklch<f64> = Srgb::new(f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0).into_color();

        Self::new(oklch.l, oklch.chroma, oklch.hue.into_inner())
    }

    pub fn to_srgb(self) -> (u8, u8, u8) {
        let srgb: Srgb<f64> = Oklch::new(self.l, self.chroma, self.hue).into_color();
        let srgb = srgb.clamp();

        ((srgb.red * 255.0) as u8, (srgb.green * 255.0) as u8, (srgb.blue * 255.0) as u8)
    }

    pub fn to_hex(self) -> String {
        let (r, g, b) = self.to_srgb();

        hex::encode([r, g, b])
    }
}
