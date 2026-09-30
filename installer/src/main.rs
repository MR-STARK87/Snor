//! Snor Setup — window setup and the mode hand-off.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use snor_installer::app::{Launch, SetupApp};

fn main() -> anyhow::Result<()> {
    // Which mode this binary is in is decided before the window exists: the
    // uninstaller is a copy of this file, told apart by what sits beside it
    // (see `Launch::detect`), and the title has to say which one the user got.
    let launch = Launch::detect();
    let title = launch.window_title();
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([620.0, 460.0])
            .with_title(title)
            // Undecorated, like the app: the title bar, its controls and the
            // window's own edge are all drawn by `SetupApp` off the shared
            // palette. The window is fixed-size — there is no maximise to
            // draw and nothing to maximise — so the app's resize bands are
            // deliberately absent here.
            .with_decorations(false)
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        title,
        native_options,
        Box::new(move |cc| Ok(Box::new(SetupApp::new(cc, launch)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe failed: {e}"))
}
