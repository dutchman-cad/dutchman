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

pub use app::{set_diagnostics_folder, App};

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

/// The name eframe stores the window's saved state under.
const APP_ID: &str = "dutchman";
/// The smallest window the app can be used in, in points.
const MIN_INNER_SIZE: [f32; 2] = [520.0, 420.0];

/// Width in points from which the main screen puts Designs and Output folder beside Results.
pub const WIDE_BREAKPOINT: f32 = 1100.0;
/// The size in points the main screen fits in with nothing scrolled, when the screen is large
/// enough: wide enough for two columns, tall enough for the Advanced choices.
pub const FIT_SIZE: [f32; 2] = [1180.0, 800.0];
/// The share of the monitor the default window may take.
const MONITOR_SHARE: f32 = 0.9;

/// The default window size for a monitor of `monitor` points: [`FIT_SIZE`], and never more than
/// 90% of the monitor, nor less than the smallest usable window.
pub fn fit_size(monitor: [f32; 2]) -> [f32; 2] {
    let size = |fit: f32, monitor: f32, min: f32| fit.min(monitor * MONITOR_SHARE).max(min);
    [
        size(FIT_SIZE[0], monitor[0], MIN_INNER_SIZE[0]),
        size(FIT_SIZE[1], monitor[1], MIN_INNER_SIZE[1]),
    ]
}

/// The top left corner that centres a window of `size` points on a monitor of `monitor` points.
pub fn centred_position(monitor: [f32; 2], size: [f32; 2]) -> [f32; 2] {
    [
        ((monitor[0] - size[0]) / 2.0).max(0.0),
        ((monitor[1] - size[1]) / 2.0).max(0.0),
    ]
}

/// The window's options: its size, title and icon.
pub fn options() -> eframe::NativeOptions {
    if let Some(dir) = eframe::storage_dir(APP_ID) {
        forget_unusable_window(&dir.join("app.ron"), MIN_INNER_SIZE);
    }
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(window_title())
        .with_app_id(APP_ID)
        // Open at the fitted size; the first frame caps it to the monitor once its size is known.
        .with_inner_size(FIT_SIZE)
        .with_min_inner_size(MIN_INNER_SIZE)
        .with_drag_and_drop(true);
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::NativeOptions {
        viewport,
        ..Default::default()
    }
}

/// The window part of eframe's saved state that decides whether it can be restored.
#[derive(serde::Deserialize)]
struct SavedWindow {
    #[serde(default)]
    inner_size_points: Option<SavedSize>,
}

#[derive(serde::Deserialize)]
struct SavedSize {
    x: f32,
    y: f32,
}

/// Removes a saved window size smaller than `min` from eframe's state file at `path`, so the
/// window opens at its default size instead. eframe restores any saved size down to 64 by 64
/// points, which is too small to use, for example after the app was closed while minimized.
/// Returns whether the file was changed; a missing or unreadable file is left alone.
pub fn forget_unusable_window(path: &std::path::Path, min: [f32; 2]) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(mut state) = ron::from_str::<std::collections::HashMap<String, String>>(&text) else {
        return false;
    };
    let unusable = state
        .get("window")
        .and_then(|window| ron::from_str::<SavedWindow>(window).ok())
        .and_then(|window| window.inner_size_points)
        .is_some_and(|size| {
            !(size.x.is_finite() && size.y.is_finite()) || size.x < min[0] || size.y < min[1]
        });
    if !unusable {
        return false;
    }
    state.remove("window");
    match ron::to_string(&state) {
        Ok(text) => std::fs::write(path, text).is_ok(),
        Err(_) => false,
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
