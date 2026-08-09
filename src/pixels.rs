use anyhow::Result;

use crate::color::Color;

const CHROMA_THRESHOLD: f64 = 0.01;

pub struct Pixels {
    pub pixels: Vec<Color>,
    pub avg_chroma: f64,
}

impl Pixels {
    pub fn new(mut pixels: Vec<Color>) -> Result<Self> {
        pixels.retain(|p| p.chroma >= CHROMA_THRESHOLD);
        if pixels.is_empty() {
            anyhow::bail!("all pixels filtered out — chroma threshold too high");
        }

        let avg_chroma = pixels.iter().map(|p| p.chroma).sum::<f64>() / pixels.len() as f64;

        Ok(Self { pixels, avg_chroma })
    }
}
