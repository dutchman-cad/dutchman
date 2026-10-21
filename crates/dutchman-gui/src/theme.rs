//! Dutchman's look: the Day and Night themes, the type scale and the fonts, from the Dutchman
//! design system's tokens.
//!
//! Spectral sets the product name only; IBM Plex Sans sets the interface and IBM Plex Mono
//! paths and loss lines. All three are bundled (SIL Open Font License 1.1, see
//! `assets/fonts/`).

use std::collections::BTreeMap;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Theme, Visuals,
};

/// One theme's colour tokens.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Window background.
    pub surface: Color32,
    /// Raised panels and code on `surface`.
    pub surface_2: Color32,
    /// Primary text.
    pub ink: Color32,
    /// Secondary text, and Info loss lines.
    pub tide: Color32,
    /// Rules and borders; never text.
    pub hairline: Color32,
    /// The brand colour: the primary button, links, the selected state.
    pub spectre: Color32,
    /// Text on a `spectre` fill.
    pub on_spectre: Color32,
    /// The one highlight in a view, as a fill.
    pub lantern: Color32,
    /// Warnings and Degraded loss lines, as text.
    pub lantern_text: Color32,
    /// Errors and Dropped loss lines.
    pub danger: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Day: sea mist.
pub const DAY: Palette = Palette {
    surface: hex(0xF3F6F4),
    surface_2: hex(0xDCE6E2),
    ink: hex(0x0E1A24),
    tide: hex(0x4D5E66),
    hairline: hex(0xC3D0CB),
    spectre: hex(0x1F7A6A),
    on_spectre: hex(0xFFFFFF),
    lantern: hex(0xD98E2B),
    lantern_text: hex(0x8F5408),
    danger: hex(0xB3412E),
};

/// Night: open water.
pub const NIGHT: Palette = Palette {
    surface: hex(0x0E1A24),
    surface_2: hex(0x1B2D3B),
    ink: hex(0xE6EEEA),
    tide: hex(0x9DB0B5),
    hairline: hex(0x2A4152),
    spectre: hex(0x6FD3BC),
    on_spectre: hex(0x0E1A24),
    lantern: hex(0xE8A64A),
    lantern_text: hex(0xE8A64A),
    danger: hex(0xF08A73),
};

/// The palette of the theme `ui` is drawn in.
pub fn palette(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode {
        &NIGHT
    } else {
        &DAY
    }
}

/// Spectral SemiBold, for the product name.
pub const DISPLAY: &str = "display";
/// IBM Plex Sans SemiBold, for headings and labels.
pub const SEMIBOLD: &str = "semibold";

/// The `title` style: the product name in the title area.
pub fn title() -> TextStyle {
    TextStyle::Name("title".into())
}

/// The `label` style: small uppercase section labels.
pub fn label() -> TextStyle {
    TextStyle::Name("label".into())
}

// Spacing tokens, in points.
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_8: f32 = 32.0;

// Radius tokens.
pub const RADIUS_SM: u8 = 2;
pub const RADIUS_MD: u8 = 4;
pub const RADIUS_LG: u8 = 8;

/// Installs the fonts, the type scale and both themes on `ctx`.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    for (theme, palette) in [(Theme::Light, &DAY), (Theme::Dark, &NIGHT)] {
        ctx.style_mut_of(theme, |style| {
            style.text_styles = text_styles();
            style.visuals = visuals(theme, palette);
            style.spacing.item_spacing = egui::vec2(SPACE_2, SPACE_2);
            style.spacing.button_padding = egui::vec2(SPACE_4 * 0.75, SPACE_1 + 2.0);
            style.spacing.interact_size.y = 30.0;
        });
    }
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        &mut fonts,
        "Spectral-SemiBold",
        include_bytes!("../assets/fonts/Spectral-SemiBold.ttf"),
    );
    add(
        &mut fonts,
        "IBMPlexSans-Regular",
        include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf"),
    );
    add(
        &mut fonts,
        "IBMPlexSans-SemiBold",
        include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf"),
    );
    add(
        &mut fonts,
        "IBMPlexMono-Regular",
        include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    );
    // egui's own fonts stay behind each family, for symbols the brand fonts lack.
    let mut family = |family: FontFamily, first: &str| {
        let list = fonts.families.entry(family).or_default();
        list.insert(0, first.to_owned());
    };
    family(FontFamily::Proportional, "IBMPlexSans-Regular");
    family(FontFamily::Monospace, "IBMPlexMono-Regular");
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (name, first) in [
        (DISPLAY, "Spectral-SemiBold"),
        (SEMIBOLD, "IBMPlexSans-SemiBold"),
    ] {
        let mut list = vec![first.to_owned()];
        list.extend(fallbacks.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), list);
    }
    fonts
}

fn text_styles() -> BTreeMap<TextStyle, FontId> {
    let semibold = || FontFamily::Name(SEMIBOLD.into());
    [
        (title(), FontId::new(28.0, FontFamily::Name(DISPLAY.into()))),
        (TextStyle::Heading, FontId::new(18.0, semibold())),
        (TextStyle::Body, FontId::proportional(15.0)),
        (TextStyle::Button, FontId::proportional(15.0)),
        (TextStyle::Small, FontId::proportional(13.0)),
        (label(), FontId::new(12.0, semibold())),
        (TextStyle::Monospace, FontId::monospace(14.0)),
    ]
    .into()
}

fn visuals(theme: Theme, p: &Palette) -> Visuals {
    let mut v = match theme {
        Theme::Light => Visuals::light(),
        Theme::Dark => Visuals::dark(),
    };
    v.override_text_color = Some(p.ink);
    v.panel_fill = p.surface;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.hairline);
    v.extreme_bg_color = p.surface;
    v.faint_bg_color = p.surface_2;
    v.code_bg_color = p.surface_2;
    v.hyperlink_color = p.spectre;
    v.warn_fg_color = p.lantern_text;
    v.error_fg_color = p.danger;
    v.selection.bg_fill = p.spectre;
    v.selection.stroke = Stroke::new(1.0, p.on_spectre);
    v.window_corner_radius = CornerRadius::same(RADIUS_LG);
    v.menu_corner_radius = CornerRadius::same(RADIUS_MD);
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    // Borders, not shadows; small corners.
    let widgets = &mut v.widgets;
    for w in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(RADIUS_MD);
        w.fg_stroke = Stroke::new(1.0, p.ink);
        w.expansion = 0.0;
    }
    widgets.noninteractive.bg_fill = p.surface;
    widgets.noninteractive.weak_bg_fill = p.surface;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.hairline);
    widgets.inactive.bg_fill = p.surface_2;
    widgets.inactive.weak_bg_fill = p.surface_2;
    widgets.inactive.bg_stroke = Stroke::new(1.0, p.hairline);
    widgets.hovered.bg_fill = p.surface_2;
    widgets.hovered.weak_bg_fill = p.surface_2;
    widgets.hovered.bg_stroke = Stroke::new(1.0, p.spectre);
    widgets.active.bg_fill = p.surface_2;
    widgets.active.weak_bg_fill = p.surface_2;
    widgets.active.bg_stroke = Stroke::new(1.5, p.spectre);
    widgets.open.bg_fill = p.surface_2;
    widgets.open.weak_bg_fill = p.surface_2;
    widgets.open.bg_stroke = Stroke::new(1.0, p.spectre);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance contrast between two colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let ch = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        };
        let (hi, lo) = {
            let (x, y) = (lum(a), lum(b));
            if x > y {
                (x, y)
            } else {
                (y, x)
            }
        };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn text_colours_read_on_the_grounds_the_app_puts_them_on() {
        for p in [&DAY, &NIGHT] {
            // Panels (surface-2) carry ink and tide; result cards (surface) carry every colour.
            for text in [p.ink, p.tide, p.lantern_text] {
                assert!(contrast(text, p.surface_2) >= 4.5, "{text:?} on surface-2");
            }
            for text in [p.ink, p.tide, p.lantern_text, p.danger, p.spectre] {
                assert!(contrast(text, p.surface) >= 4.5, "{text:?} on surface");
            }
            assert!(contrast(p.on_spectre, p.spectre) >= 4.5);
        }
        // Why cards are on surface: by day, danger and spectre fall short on surface-2.
        assert!(contrast(DAY.danger, DAY.surface_2) < 4.5);
        assert!(contrast(DAY.spectre, DAY.surface_2) < 4.5);
    }
}
