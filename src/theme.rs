use eframe::egui;

// Calm dark-green IDE palette, matched to the reference mock: near-black
// green-tinted surfaces, a soft yellow-green accent, and chrome drawn with
// stroked outlines rather than filled colour blocks.
pub fn apply_dark(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(0x14, 0x1B, 0x1A);
    visuals.window_fill = egui::Color32::from_rgb(0x17, 0x20, 0x1E);
    visuals.extreme_bg_color = egui::Color32::from_rgb(0x0F, 0x16, 0x15);
    visuals.code_bg_color = egui::Color32::from_rgb(0x12, 0x19, 0x18);
    visuals.faint_bg_color = egui::Color32::from_rgb(0x1B, 0x25, 0x23);
    visuals.selection.bg_fill = egui::Color32::from_rgb(0x25, 0x38, 0x2E);
    visuals.selection.stroke = egui::Stroke::new(1.0, accent());
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(0x14, 0x1B, 0x1A);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x1C, 0x25, 0x23);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x23, 0x2E, 0x2B);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0x2A, 0x36, 0x32);
    visuals.widgets.noninteractive.fg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(0x24, 0x2D, 0x2B));
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(0x22, 0x2B, 0x29));
    for w in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        w.corner_radius = egui::CornerRadius::same(4);
    }
    ctx.set_visuals(visuals);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        // The reference's panel edges are grabbable across a comfortable band;
        // egui's 3pt default is a 6pt target that is easy to miss.
        style.interaction.resize_grab_radius_side = 6.0;
    });
}

/// Soft yellow-green used for the title, tab badges, prompts and the sync dot.
pub fn accent() -> egui::Color32 {
    egui::Color32::from_rgb(0xBC, 0xDF, 0x9C)
}

/// Ink on top of a filled badge or an [`accent`] fill: the reference knocks
/// the letter out of the fill rather than drawing it on the panel, so this is
/// the *absence* of ink, not a highlight. Measured at (0, 1, 2) inside an
/// unselected badge and (6, 19, 15) inside a selected one.
pub fn on_accent() -> egui::Color32 {
    egui::Color32::from_rgb(0x12, 0x19, 0x17)
}

/// Primary body text.
pub fn text() -> egui::Color32 {
    egui::Color32::from_rgb(0xC0, 0xC4, 0xC1)
}

/// The selected explorer row's label. Brighter than [`text`] — that lift is
/// what makes a selected row read as selected; the pill alone does not carry
/// it. Measured at (220, 221, 219) against (182, 186, 184) for an unselected
/// file label one row down.
pub fn text_strong() -> egui::Color32 {
    egui::Color32::from_rgb(0xDC, 0xDD, 0xDB)
}

/// The explorer root row's label: the brightest text in the panel, measured at
/// (245, 246, 244). The root is set as a heading, so it outranks both
/// [`text`] and [`text_strong`].
pub fn text_bright() -> egui::Color32 {
    egui::Color32::from_rgb(0xF5, 0xF6, 0xF4)
}

/// Secondary text: tree labels, terminal status, panel headings.
pub fn dim_text() -> egui::Color32 {
    egui::Color32::from_rgb(0x9A, 0xA0, 0x9B)
}

/// Faint text: line numbers, quotes, hints.
pub fn faint() -> egui::Color32 {
    egui::Color32::from_rgb(0x5C, 0x63, 0x5E)
}

/// Title-bar tagline. Sampled from the reference (`#6C716A`): noticeably
/// dimmer than body text so "Calm tools for focused minds." recedes behind
/// the product name instead of competing with it.
pub fn tagline() -> egui::Color32 {
    egui::Color32::from_rgb(0x6C, 0x71, 0x6A)
}

/// Explorer footer caption. The reference's green (`#6E8670`) sits between
/// [`faint`] and [`dim_text`], and is greener than either — it is the one
/// place in the chrome where the accent leaks into text.
pub fn moss() -> egui::Color32 {
    egui::Color32::from_rgb(0x6E, 0x86, 0x70)
}

/// The mascot's floating sleep marks (`#38433A`). Deliberately dimmer than
/// [`outline`]: the reference draws them as a fading trail, not as part of
/// the silhouette.
pub fn sleep_mark() -> egui::Color32 {
    egui::Color32::from_rgb(0x38, 0x43, 0x3A)
}

/// Stroke colour for line-art icons and the mascot.
pub fn outline() -> egui::Color32 {
    egui::Color32::from_rgb(0x5B, 0x6A, 0x59)
}

/// Ink for the explorer tree's glyphs: the folder, document and badge
/// outlines, and the disclosure chevrons.
///
/// Sampled from the reference at (208, 203, 189) on the folder's horizontal
/// top edge, which is fully covered and therefore reports the true stroke
/// colour. Diagonal strokes read ~16% dimmer than this in the mock
/// ((175, 178, 171) on a chevron, (175, 171, 166) on a badge corner) — that
/// is antialiasing spread over two pixels, not a second, darker ink, so
/// everything in the tree uses this one colour.
///
/// Deliberately *not* [`outline`]: that green-grey is right for the mascot,
/// but the reference's tree is pale warm line art. Painting it in `outline`
/// left the glyphs at (89, 104, 88) — far too dim, and the wrong hue.
pub fn glyph() -> egui::Color32 {
    egui::Color32::from_rgb(0xD0, 0xCB, 0xBD)
}

/// Fill of the explorer's letter badges.
///
/// The reference draws *every* badge as a solid rounded square with the letter
/// knocked out — the outlined badge this replaced was simply wrong. The fill
/// is a light warm grey, measured at (188, 185, 179): a touch dimmer than
/// [`glyph`]'s strokes, which is what a flat fill reports next to a
/// thin stroke's brightest pixel.
pub fn badge_fill() -> egui::Color32 {
    egui::Color32::from_rgb(0xBC, 0xB9, 0xB3)
}

/// Fill behind the mascot's outlines: a hair lighter than the panel so the
/// silhouette reads without turning into a solid block.
pub fn mascot_fill() -> egui::Color32 {
    egui::Color32::from_rgb(0x19, 0x22, 0x1F)
}

/// Active editor tab / selected explorer row.
pub fn tab_active() -> egui::Color32 {
    egui::Color32::from_rgb(0x1C, 0x25, 0x23)
}

/// 1px hairline between regions, matching the reference's subtle separators.
pub fn hairline() -> egui::Color32 {
    egui::Color32::from_rgb(0x22, 0x2B, 0x29)
}

pub fn danger() -> egui::Color32 {
    egui::Color32::from_rgb(0xE0, 0x6C, 0x75)
}

/// Letter shown in a file's badge, or `None` when the file is not source code
/// and should get the document glyph instead (the reference draws `Cargo.toml`
/// and `README.md` as plain pages).
pub fn file_letter(filename: &str) -> Option<&'static str> {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "rs" => Some("R"),
        "js" | "mjs" | "cjs" => Some("J"),
        "ts" | "mts" | "tsx" => Some("T"),
        "py" => Some("P"),
        "ps1" => Some("P"),
        "go" => Some("G"),
        "c" | "h" | "cpp" | "hpp" => Some("C"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_source_files_get_letter_badges() {
        assert_eq!(file_letter("main.rs"), Some("R"));
        assert_eq!(file_letter("app.py"), Some("P"));
        // The reference draws these as plain document glyphs.
        assert_eq!(file_letter("Cargo.toml"), None);
        assert_eq!(file_letter("README.md"), None);
        assert_eq!(file_letter("Cargo.lock"), None);
        assert_eq!(file_letter(".gitignore"), None);
    }

    #[test]
    fn accent_is_readable_on_panel() {
        let lum = |c: egui::Color32| {
            0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32
        };
        assert!(
            lum(accent()) > lum(on_accent()) + 80.0,
            "tab badge letter would not read"
        );
    }

    /// The footer caption is the one text colour that is greener than it is
    /// grey. If that tilt is lost it stops reading as the accent's echo.
    #[test]
    fn footer_caption_keeps_its_green_tilt() {
        let tilt = |c: egui::Color32| c.g() as i32 - c.r() as i32;
        assert!(tilt(moss()) > tilt(dim_text()));
        assert!(tilt(moss()) > tilt(faint()));
        // ...but it still has to be dimmer than body text, or it shouts.
        let lum = |c: egui::Color32| {
            0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32
        };
        assert!(lum(moss()) < lum(text()));
        assert!(lum(moss()) > lum(faint()));
    }

    #[test]
    fn sleep_marks_stay_dimmer_than_the_silhouette() {
        let lum = |c: egui::Color32| {
            0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32
        };
        assert!(lum(sleep_mark()) < lum(outline()));
    }
}
