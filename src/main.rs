//! Snor — lightweight native IDE in full Rust.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

mod app;
mod brightness;
mod dim;
mod editor;
mod file_tree;
mod fonts;
mod icons;
mod mascot;
mod syntax;
mod terminal;
mod theme;
mod widgets;

fn main() -> anyhow::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("Snor")
            .with_min_inner_size([900.0, 600.0])
            // The reference is a decorationless window that draws its own
            // frame and its own minimise / maximise / close. Turning the OS
            // chrome off is what makes `theme::window_edge` the window's real
            // boundary rather than a line drawn just inside someone else's.
            //
            // The cost is real and deliberate: no Aero Snap, no drop shadow,
            // and the resize bands and the three controls have to be ours.
            // `SnorApp::window_resize_bands` and `SnorApp::window_controls`
            // are the other halves of this decision — none of the three can be
            // removed on its own.
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "Snor",
        native_options,
        Box::new(|cc| Ok(Box::new(app::SnorApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe failed: {e}"))
}
