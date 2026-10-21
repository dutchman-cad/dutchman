//! Renders the app icon (`assets/logo/dutchman-app-icon.svg`) into a macOS icon file, with a PNG
//! at each size macOS asks for:
//!
//!     cargo run -p dutchman-gui --example icns -- crates/dutchman-gui/assets/Dutchman.icns

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;

/// Each entry's type and the size of the PNG it holds (`ic11` to `ic14` are the Retina sizes
/// of 16, 32, 128 and 256 points).
const ENTRIES: [(&[u8; 4], u32); 11] = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic13", 256),
    (b"ic14", 512),
];

fn main() {
    let out = std::env::args_os().nth(1).expect("the .icns file to write");
    let svg = include_bytes!("../../../assets/logo/dutchman-app-icon.svg");
    let mut body = Vec::new();
    for (kind, size) in ENTRIES {
        let png = png(svg, size);
        body.extend_from_slice(kind);
        body.extend_from_slice(&u32::try_from(png.len() + 8).unwrap().to_be_bytes());
        body.extend_from_slice(&png);
    }
    let mut file = std::fs::File::create(out).unwrap();
    file.write_all(b"icns").unwrap();
    file.write_all(&u32::try_from(body.len() + 8).unwrap().to_be_bytes())
        .unwrap();
    file.write_all(&body).unwrap();
}

/// The icon at `size` × `size` pixels, as a PNG.
fn png(svg: &[u8], size: u32) -> Vec<u8> {
    let image = egui_extras::image::load_svg_bytes_with_size(
        svg,
        eframe::egui::SizeHint::Size {
            width: size,
            height: size,
            maintain_aspect_ratio: true,
        },
        &Default::default(),
    )
    .unwrap();
    let rgba = image::RgbaImage::from_raw(size, size, image.as_raw().to_vec()).unwrap();
    let mut out = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}
