//! Renders the app's own SVG into a Windows icon, without an external image tool.
use std::io::{Cursor, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args_os().nth(1).ok_or("Supply the .ico path")?;
    let mut images = Vec::new();
    for size in [16, 32, 48, 256] {
        let image = egui_extras::image::load_svg_bytes_with_size(
            include_bytes!("../assets/dutchman-app-icon.svg"),
            eframe::egui::SizeHint::Size {
                width: size,
                height: size,
                maintain_aspect_ratio: true,
            },
            &Default::default(),
        )?;
        let raw = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_srgba_unmultiplied())
            .collect();
        let rgba =
            image::RgbaImage::from_raw(size, size, raw).ok_or("Could not render the icon")?;
        let mut png = Cursor::new(Vec::new());
        rgba.write_to(&mut png, image::ImageFormat::Png)?;
        images.push((u8::try_from(size).unwrap_or(0), png.into_inner()));
    }
    let mut file = std::fs::File::create(out)?;
    file.write_all(&[0, 0, 1, 0, 4, 0])?;
    let mut offset = 6 + 4 * 16;
    for (size, png) in &images {
        file.write_all(&[*size, *size, 0, 0, 1, 0, 32, 0])?;
        let len = u32::try_from(png.len())?;
        file.write_all(&len.to_le_bytes())?;
        file.write_all(&u32::to_le_bytes(offset))?;
        offset += len;
    }
    for (_, png) in images {
        file.write_all(&png)?;
    }
    Ok(())
}
