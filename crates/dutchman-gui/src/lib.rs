//! `dutchman-gui`: Dutchman as a small desktop app. Drop designs in, choose an output folder,
//! convert, and read what became of each file. Conversions use the same shared code as the
//! `dutchman` command, on background threads, so the window never waits on them.

pub mod app;
pub mod platform;
pub mod theme;
pub mod worker;

pub use app::App;

/// The window's options: its size, title and icon.
pub fn options() -> eframe::NativeOptions {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Dutchman")
        .with_app_id("dutchman")
        .with_inner_size([760.0, 680.0])
        .with_min_inner_size([520.0, 420.0])
        .with_drag_and_drop(true);
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::NativeOptions {
        viewport,
        ..Default::default()
    }
}

/// The mark as the window icon.
fn icon() -> Option<eframe::egui::IconData> {
    let image = egui_extras::image::load_svg_bytes_with_size(
        include_bytes!("../assets/dutchman-mark.svg"),
        eframe::egui::SizeHint::Size {
            width: 256,
            height: 256,
            maintain_aspect_ratio: true,
        },
        &Default::default(),
    )
    .ok()?;
    let [width, height] = image.size;
    Some(eframe::egui::IconData {
        rgba: image.as_raw().to_vec(),
        width: u32::try_from(width).ok()?,
        height: u32::try_from(height).ok()?,
    })
}
