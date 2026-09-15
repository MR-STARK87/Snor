use eframe::egui;
use std::path::PathBuf;

use crate::file_tree::FileTree;

pub struct SnorApp {
    root: PathBuf,
    tree: FileTree,
    status: String,
    preview_path: Option<PathBuf>,
}

impl SnorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::apply_dark(&cc.egui_ctx);
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let tree = FileTree::new(root.clone());
        Self {
            root,
            tree,
            status: String::from("ready"),
            preview_path: None,
        }
    }
}

impl eframe::App for SnorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(opened) = self.tree.opened_file.take() {
            self.preview_path = Some(opened.clone());
            self.status = format!("opened {}", opened.display());
        }

        egui::Panel::top("snor_top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("Snor")
                        .color(crate::theme::accent())
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(self.root.display().to_string())
                        .color(crate::theme::dim_text())
                        .small(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("main")
                            .small()
                            .color(crate::theme::dim_text()),
                    );
                });
            });
        });

        egui::Panel::left("snor_tree")
            .default_size(260.0)
            .min_size(200.0)
            .show(ui, |ui| {
                self.tree.ui(ui);
            });

        egui::Panel::bottom("snor_terminal")
            .default_size(200.0)
            .min_size(120.0)
            .resizable(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Terminal");
                    ui.label(
                        egui::RichText::new("ConPTY powershell lands here (next)")
                            .small()
                            .color(crate::theme::dim_text()),
                    );
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.monospace("$ powershell.exe");
                    ui.monospace("# run: opencode  (wired in terminal milestone)");
                });
            });

        egui::Panel::bottom("snor_status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&self.status).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let sel = self
                        .tree
                        .selected
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "-".to_string());
                    ui.label(
                        egui::RichText::new(format!("sel: {sel}"))
                            .small()
                            .color(crate::theme::dim_text()),
                    );
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(p) = &self.preview_path {
                    let name = p
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let _ = ui.selectable_label(true, name);
                } else {
                    let _ = ui.selectable_label(true, "welcome.rs");
                }
                let _ = ui.selectable_label(false, "+");
            });
            ui.separator();
            egui::ScrollArea::both().show(ui, |ui| {
                if let Some(p) = &self.preview_path {
                    ui.monospace(format!("// {}", p.display()));
                    ui.monospace("// editor buffer + highlight lands next milestone.");
                    match std::fs::read_to_string(p) {
                        Ok(text) => {
                            let preview: String =
                                text.lines().take(80).collect::<Vec<_>>().join("\n");
                            ui.monospace(preview);
                        }
                        Err(e) => {
                            ui.label(format!("cannot preview: {e}"));
                        }
                    }
                } else {
                    ui.monospace("// Snor shell up. Double-click a file to preview it.");
                    ui.monospace("// V1 target: <250MB idle vs Zed 980MB.");
                    ui.add_space(12.0);
                    ui.label(format!("root: {}", self.root.display()));
                }
            });
        });

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
}
