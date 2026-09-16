//! Space Grotesk, the app's UI face.
//!
//! Three static weights are vendored under `assets/fonts` (OFL 1.1, licence
//! alongside them) rather than the variable build: `FontData` renders a
//! variable font at its default instance, so `SpaceGrotesk[wght].ttf` would
//! give Regular and nothing else.
//!
//! Monospace is deliberately untouched. Space Grotesk is proportional, and the
//! editor's gutter and the terminal's grid both depend on a fixed advance
//! width — swapping it in would break the one-buffer-line-per-visual-row
//! invariant that the gutter and the code share.

use std::sync::Arc;

/// Family name for the medium weight. `RichText::strong()` cannot be used for
/// this: in egui it only swaps the *colour* for `strong_text_color()` and
/// leaves the face alone, so wanting real weight means naming a family.
pub const MEDIUM: &str = "space-grotesk-medium";

/// Family name for the bold weight.
pub const BOLD: &str = "space-grotesk-bold";

const REGULAR: &str = "space-grotesk";

pub fn install(ctx: &eframe::egui::Context) {
    let mut fonts = eframe::egui::FontDefinitions::default();

    let faces: [(&str, &'static [u8]); 3] = [
        (
            REGULAR,
            include_bytes!("../assets/fonts/SpaceGrotesk-Regular.ttf"),
        ),
        (
            MEDIUM,
            include_bytes!("../assets/fonts/SpaceGrotesk-Medium.ttf"),
        ),
        (
            BOLD,
            include_bytes!("../assets/fonts/SpaceGrotesk-Bold.ttf"),
        ),
    ];
    for (name, bytes) in faces {
        fonts.font_data.insert(
            name.to_owned(),
            Arc::new(eframe::egui::FontData::from_static(bytes)),
        );
    }

    // Our face leads and egui's built-ins stay behind it as fallbacks, so the
    // emoji and the rarer symbols still resolve instead of drawing tofu.
    let inherited = fonts
        .families
        .get(&eframe::egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut proportional = vec![REGULAR.to_owned()];
    proportional.extend(inherited);
    fonts
        .families
        .insert(eframe::egui::FontFamily::Proportional, proportional);

    for (family, face) in [(MEDIUM, MEDIUM), (BOLD, BOLD)] {
        fonts.families.insert(
            eframe::egui::FontFamily::Name(family.into()),
            vec![face.to_owned(), REGULAR.to_owned()],
        );
    }

    ctx.set_fonts(fonts);
}
