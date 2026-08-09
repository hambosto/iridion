use anyhow::{Context, Result};
use image::imageops::FilterType;
use std::path::Path;

use crate::color::Color;

pub fn load_pixels(path: &Path) -> Result<Vec<Color>> {
    let source = image::open(path).context("failed to open image")?;
    let resized = source.resize_exact(256, 256, FilterType::Nearest);
    let rgb = resized.into_rgb8();
    let pixels: Vec<Color> = rgb.pixels().map(|p| Color::from_rgb(p[0], p[1], p[2])).collect();

    Ok(pixels)
}
