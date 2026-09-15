use eframe::egui;

// Calm dark-green IDE palette borrowed from the reference mock:
// near-black green-tinted surfaces, mint accent, warm amber strings.
pub fn apply_dark(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(0x16, 0x1C, 0x1E);
    visuals.window_fill = egui::Color32::from_rgb(0x1A, 0x21, 0x24);
    visuals.extreme_bg_color = egui::Color32::from_rgb(0x0E, 0x12, 0x14);
    visuals.code_bg_color = egui::Color32::from_rgb(0x12, 0x17, 0x1A);
    visuals.faint_bg_color = egui::Color32::from_rgb(0x1E, 0x2A, 0x2C);
    visuals.selection.bg_fill = egui::Color32::from_rgb(0x24, 0x4A, 0x3C);
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0x7D, 0xD3, 0xA8));
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(0x16, 0x1C, 0x1E);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x1E, 0x26, 0x28);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x26, 0x32, 0x34);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0x2C, 0x3A, 0x3C);
    visuals.widgets.noninteractive.fg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(0x3A, 0x46, 0x48));
    ctx.set_visuals(visuals);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
    });
}

pub fn accent() -> egui::Color32 {
    egui::Color32::from_rgb(0x7D, 0xD3, 0xA8)
}

pub fn dim_text() -> egui::Color32 {
    egui::Color32::from_rgb(0x8B, 0x94, 0xA3)
}

/// Faint text: line numbers, quotes, hints.
pub fn faint() -> egui::Color32 {
    egui::Color32::from_rgb(0x5A, 0x66, 0x62)
}

/// Active editor tab / selected explorer row.
pub fn tab_active() -> egui::Color32 {
    egui::Color32::from_rgb(0x1F, 0x2B, 0x2C)
}

pub fn amber() -> egui::Color32 {
    egui::Color32::from_rgb(0xE5, 0xC0, 0x7B)
}

pub fn blue() -> egui::Color32 {
    egui::Color32::from_rgb(0x7A, 0xA2, 0xF7)
}

pub fn purple() -> egui::Color32 {
    egui::Color32::from_rgb(0xC6, 0x78, 0xDD)
}

/// (letter, pill background, letter color) for explorer rows + editor tabs.
pub fn file_badge(filename: &str) -> (String, egui::Color32, egui::Color32) {
    let c = |r, g, b| egui::Color32::from_rgb(r, g, b);
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "rs" => ("R".to_string(), c(0x2B, 0x4A, 0x38), accent()),
        "toml" => ("T".to_string(), c(0x4A, 0x3D, 0x22), amber()),
        "json" | "js" | "mjs" | "cjs" | "ts" | "mts" | "tsx" => (
            "J".to_string(),
            c(0x2B, 0x3D, 0x55),
            blue(),
        ),
        "md" => ("M".to_string(), c(0x3A, 0x33, 0x50), purple()),
        "py" | "ps1" => ("P".to_string(), c(0x2B, 0x45, 0x4A), c(0x56, 0xB6, 0xC2)),
        _ => ("=".to_string(), c(0x2A, 0x2F, 0x33), dim_text()),
    }
}
