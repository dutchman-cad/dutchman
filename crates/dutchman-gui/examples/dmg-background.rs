//! Render both DMG background sizes from the original SVG:
//!
//!     cargo run --locked -p dutchman-gui --example dmg-background -- assets/dmg

use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let out = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("Give the output directory, for example assets/dmg")?;
    let svg = include_bytes!("../../../assets/dmg/background.svg");
    for (name, scale) in [("background.png", 1), ("background@2x.png", 2)] {
        let width = 640 * scale;
        let height = 480 * scale;
        let image = egui_extras::image::load_svg_bytes_with_size(
            svg,
            eframe::egui::SizeHint::Size {
                width,
                height,
                maintain_aspect_ratio: true,
            },
            &Default::default(),
        )?;
        let raw = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_srgba_unmultiplied())
            .collect();
        let rgba = image::RgbaImage::from_raw(width, height, raw)
            .ok_or("The rendered background has unexpected dimensions")?;
        rgba.save(out.join(name))?;
    }
    Ok(())
}
