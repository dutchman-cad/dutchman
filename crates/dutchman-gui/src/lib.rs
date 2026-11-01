//! `dutchman-gui`: Dutchman Convert, Dutchman as a small desktop app. Drop designs in, choose an output folder,
//! convert, and read what became of each file. Conversions use the same shared code as the
//! `dutchman` command, on background threads, so the window never waits on them.

pub mod app;
mod doctor;
pub mod platform;
pub mod report_consent;
pub mod report_policy;
pub mod theme;
pub mod worker;

pub use app::App;

/// The app's name, as its window and the macOS app bundle show it.
pub const APP_NAME: &str = "Dutchman Convert";

/// The name eframe keeps the app's settings under. It follows the package version, not
/// [`dutchman::BUILD_VERSION`], so one nightly's settings carry over to the next.
pub const SETTINGS_NAME: &str = concat!("Dutchman Convert ", env!("CARGO_PKG_VERSION"));

/// The version beside the app's name in its top row.
pub fn version_label() -> String {
    format!("v{}", dutchman::BUILD_VERSION)
}

/// The desktop window's title, including the version of this build.
pub fn window_title() -> String {
    format!("{APP_NAME} {}", dutchman::BUILD_VERSION)
}

/// The issue form for a file that did not convert.
pub const REPORT_PROBLEM_URL: &str = dutchman::issue::NEW_ISSUE;

/// The window's options: its size, title and icon.
pub fn options() -> eframe::NativeOptions {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(window_title())
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

pub mod update;

mod diagnose;
