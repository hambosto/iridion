# Iridion

A CLI tool that generates [base16](https://github.com/chriskempson/base16) color schemes from images. Runs in Oklch color space, uses hue-locked K-means to pick dominant colors, spits out JSON.

## Install

### Cargo

```bash
git clone https://github.com/hambosto/iridion.git
cd iridion
cargo build --release
```

Binary lands at `target/release/iridion`. Move it somewhere on your `$PATH`:

```bash
cp target/release/iridion ~/.local/bin/
```

Requires Rust nightly (edition 2024).

### Nix

Run directly:

```bash
nix run github:hambosto/iridion -- -i photo.jpg
```

Build the package:

```bash
nix build github:hambosto/iridion
# result/bin/iridion
```

Enter a dev shell with all dependencies:

```bash
nix develop github:hambosto/iridion
```

#### As an overlay

```nix
{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    iridion.url = "github:hambosto/iridion";
  };

  outputs = { self, nixpkgs, iridion, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ({ pkgs, ... }: {
          nixpkgs.overlays = [ iridion.overlays.default ];
          environment.systemPackages = [ pkgs.iridion ];
        })
      ];
    };
  };
}
```

## Usage

```
iridion -i <IMAGE> [-c CONTRAST]
```

| Flag | Description | Default |
|------|-------------|---------|
| `-i` | Source image path | required |
| `-c` | Contrast (`0.0`–`1.0`) | `0.5` |

### Examples

```bash
# pull a palette from your wallpaper
iridion -i ~/wallpaper.png

# punch up the accent colors
iridion -i photo.jpg --contrast 0.8

# muted palette (low contrast)
iridion -i sunset.jpg -c 0.2

# save output to a file
iridion -i background.webp > ~/.config/theme.json
```

Output is a JSON object with 16 hex colors:

```json
{
  "base00": "#1a1c2a",
  "base01": "#171a27",
  "base02": "#2a2e3e",
  "base03": "#3a3f52",
  "base04": "#5a5f72",
  "base05": "#c8ccd8",
  "base06": "#d0d4e0",
  "base07": "#607080",
  "base08": "#e06070",
  "base09": "#e0a060",
  "base0A": "#e0d060",
  "base0B": "#60c070",
  "base0C": "#60b0c0",
  "base0D": "#6080e0",
  "base0E": "#b060d0",
  "base0F": "#d060a0"
}
```

`base00`–`base05` are background shades, `base06` is foreground, `base07` is a bright background, and `base08`–`base0F` are accent colors.

## How It Works

1. Load image, resize to 256x256, convert RGB to Oklch
2. Drop near-grayscale pixels (chroma < 0.01)
3. Z-score normalize [L, chroma, hue]
4. Run K-means with 8 clusters locked to 45-degree hue sectors (tries 3 hue offsets, picks the best)
5. Merge clusters into two tone zones
6. Map to base16 slots, boost contrast

The key trick is **hue-locked K-means**: each cluster is pinned to a 45-degree slice of the hue wheel. If a cluster center drifts out of its slice, it gets frozen in place. This stops clusters from rotating around the hue wheel, which is what naive K-means does when you feed it color data.

Dual-tone detection sorts active clusters by hue and groups them into zones (50-degree gap threshold). The two biggest zones become your primary/secondary tones — background shades come from the primary, accents from the secondary.

## Supported Formats

Anything the [`image`](https://crates.io/crates/image) crate can read: PNG, JPEG, GIF, BMP, TIFF, WebP, QOI, etc.

## License

[MIT](LICENSE) © 2026 Ilham Putra Husada
