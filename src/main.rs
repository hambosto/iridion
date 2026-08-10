mod cli;
mod cluster;
mod color;
mod palette;

use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use image::imageops::FilterType;

use crate::cli::Cli;
use crate::color::Color;
use crate::palette::Palette;

const RESIZE: u32 = 256;

fn load_pixels(path: &Path) -> Result<Vec<Color>> {
    let source = image::open(path).context("failed to open image")?;
    let rgb = source.resize_exact(RESIZE, RESIZE, FilterType::Nearest).into_rgb8();

    Ok(rgb.pixels().map(|p| Color::from_rgb(p[0], p[1], p[2])).collect())
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let pixels = load_pixels(&args.image)?;
    let result = cluster::extract_dominant_colors(pixels);
    let palette = Palette::generate(&result, args.contrast);

    println!("{}", palette.to_json()?);

    Ok(())
}
