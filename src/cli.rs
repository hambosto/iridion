use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "iridion", about = "Extract base16 color palettes from images using Oklch perceptual clustering")]
pub struct Cli {
    /// Path to the source image
    #[arg(short = 'i', long = "image")]
    pub image: PathBuf,

    /// Contrast level: 0.0 (minimum) to 1.0 (maximum)
    #[arg(short, long, default_value_t = 0.5)]
    pub contrast: f64,
}
