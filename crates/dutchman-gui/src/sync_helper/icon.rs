//! The tray icon's pixels: the Dutchman mark, drawn from the app's own SVG.

/// The mark as `size` by `size` unmultiplied RGBA pixels.
pub fn mark_rgba(size: u32) -> Option<Vec<u8>> {
    let image = egui_extras::image::load_svg_bytes_with_size(
        include_bytes!("../../assets/dutchman-mark.svg"),
        eframe::egui::SizeHint::Size {
            width: size,
            height: size,
            maintain_aspect_ratio: true,
        },
        &Default::default(),
    )
    .ok()?;
    if image.size != [usize::try_from(size).ok()?; 2] {
        return None;
    }
    Some(
        image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_srgba_unmultiplied())
            .collect(),
    )
}

/// RGBA pixels as the StatusNotifierItem wants them: ARGB, one pixel after another.
pub fn argb(rgba: &[u8]) -> Vec<u8> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|[r, g, b, a]| [*a, *r, *g, *b])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_draws_and_argb_moves_alpha_first() {
        let rgba = mark_rgba(32).unwrap();
        assert_eq!(rgba.len(), 32 * 32 * 4);
        assert!(rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
        assert_eq!(argb(&[1, 2, 3, 4]), [4, 1, 2, 3]);
    }
}
