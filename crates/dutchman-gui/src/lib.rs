//! `dutchman-gui`: Dutchman as a small desktop app. Drop designs in, choose an output folder,
//! convert, and read what became of each file. Conversions use the same shared code as the
//! `dutchman` command, on background threads, so the window never waits on them.

pub mod app;
pub mod platform;
pub mod theme;
pub mod worker;

pub use app::App;

/// The desktop window's title, including the version of this build.
pub const WINDOW_TITLE: &str = concat!("Dutchman ", env!("CARGO_PKG_VERSION"));

/// The issue form for a file that did not convert.
pub const REPORT_PROBLEM_URL: &str =
    "https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml";

/// The window's options: its size, title and icon.
pub fn options() -> eframe::NativeOptions {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(WINDOW_TITLE)
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

/// The window icon: on macOS, which shows it in the Dock while the app runs, the app icon
/// (the mark on its light plate, as the app bundle's icon), elsewhere the mark itself.
fn icon() -> Option<eframe::egui::IconData> {
    let svg: &[u8] = if cfg!(target_os = "macos") {
        include_bytes!("../assets/dutchman-app-icon.svg")
    } else {
        include_bytes!("../assets/dutchman-mark.svg")
    };
    let image = egui_extras::image::load_svg_bytes_with_size(
        svg,
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
        // egui's pixels are premultiplied; an icon's are not.
        rgba: image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_srgba_unmultiplied())
            .collect(),
        width: u32::try_from(width).ok()?,
        height: u32::try_from(height).ok()?,
    })
}
