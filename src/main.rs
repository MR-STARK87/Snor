//! Snor — lightweight native IDE in full Rust.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

mod app;
mod editor;
mod file_tree;
mod search;
mod syntax;
mod terminal;
mod theme;

fn main() -> anyhow::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("Snor")
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Snor",
        native_options,
        Box::new(|cc| Ok(Box::new(app::SnorApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe failed: {e}"))
}
