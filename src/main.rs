mod cli;
mod cluster;
mod color;
mod loader;
mod palette;
mod pixels;

use anyhow::Result;
use clap::Parser;

use crate::cli::Cli;
use crate::cluster::ClusterSet;
use crate::color::Color;
use crate::palette::Palette;
use crate::pixels::Pixels;

fn generate_theme(pixels: Vec<Color>, contrast: f64) -> Result<Palette> {
    let prepared = Pixels::new(pixels)?;
    let cluster_set = ClusterSet::new(&prepared.pixels);

    Ok(Palette::new(&cluster_set, contrast, prepared.avg_chroma, &prepared.pixels))
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let pixels = loader::load_pixels(&args.image)?;
    let theme = generate_theme(pixels, args.contrast)?;

    println!("{}", theme.to_json_pretty()?);

    Ok(())
}
