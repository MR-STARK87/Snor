use eframe::egui;

pub fn apply_dark(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(0x16, 0x19, 0x21);
    visuals.window_fill = egui::Color32::from_rgb(0x1A, 0x1E, 0x26);
    visuals.extreme_bg_color = egui::Color32::from_rgb(0x10, 0x12, 0x17);
    visuals.code_bg_color = egui::Color32::from_rgb(0x1E, 0x23, 0x2C);
    visuals.selection.bg_fill = egui::Color32::from_rgb(0x2E, 0x5E, 0x4E);
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0x7D, 0xD3, 0xA8));
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(0x1A, 0x1E, 0x26);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x22, 0x27, 0x33);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x2A, 0x31, 0x3D);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0x32, 0x3A, 0x48);
    ctx.set_visuals(visuals);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}

pub fn accent() -> egui::Color32 {
    egui::Color32::from_rgb(0x7D, 0xD3, 0xA8)
}

pub fn dim_text() -> egui::Color32 {
    egui::Color32::from_rgb(0x8B, 0x94, 0xA3)
}
