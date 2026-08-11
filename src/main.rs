mod cli;
mod cluster;
mod color;
mod palette;

use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use image::imageops::FilterType;

use crate::cli::Cli;
use crate::cluster::ClusteringResult;
use crate::color::Color;
use crate::palette::Palette;

fn load_pixels(path: &Path) -> Result<Vec<Color>> {
    let source = image::open(path).context("failed to open image")?;
    let resized = source.resize_exact(256, 256, FilterType::Nearest);
    let rgb = resized.into_rgb8();

    Ok(rgb.pixels().map(|p| Color::from_srgb(p[0], p[1], p[2])).collect())
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let pixels = load_pixels(&args.image)?;
    let result = ClusteringResult::from_pixels(&pixels);
    let palette = Palette::from_clusters(&result, args.contrast);

    println!("{}", palette.to_json_string()?);

    Ok(())
}
