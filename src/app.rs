use eframe::egui;
use std::path::PathBuf;

use crate::editor::Editor;
use crate::file_tree::FileTree;

pub struct SnorApp {
    root: PathBuf,
    tree: FileTree,
    editor: Editor,
    status: String,
}

impl SnorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::apply_dark(&cc.egui_ctx);
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let tree = FileTree::new(root.clone());
        Self {
            root,
            tree,
            editor: Editor::new(),
            status: String::from("ready"),
        }
    }
}

impl eframe::App for SnorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(opened) = self.tree.opened_file.take() {
            if opened.is_file() {
                self.editor.open_file(opened.clone());
                self.status = format!("opened {}", opened.display());
            } else {
                self.status = format!("selected {}", opened.display());
            }
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
                        .editor
                        .active_path()
                        .or_else(|| self.tree.selected.clone())
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "-".to_string());
                    ui.label(
                        egui::RichText::new(sel)
                            .small()
                            .color(crate::theme::dim_text()),
                    );
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            self.editor.ui(ui);
        });

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
}
