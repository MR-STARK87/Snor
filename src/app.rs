use eframe::egui;
use std::path::PathBuf;

use crate::editor::Editor;
use crate::file_tree::FileTree;
use crate::terminal::Terminal;

pub struct SnorApp {
    root: PathBuf,
    tree: FileTree,
    editor: Editor,
    terminal: Terminal,
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
            terminal: Terminal::new(),
        }
    }
}

impl eframe::App for SnorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(opened) = self.tree.opened_file.take()
            && opened.is_file()
        {
            self.editor.open_file(opened);
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
                    if ui.small_button("open folder").clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_directory(&self.root)
                            .pick_folder()
                    {
                        self.root = dir.clone();
                        self.tree.set_root(dir);
                    }
                });
            });
        });

        egui::Panel::left("snor_tree")
            .default_size(260.0)
            .min_size(200.0)
            .show(ui, |ui| {
                self.tree.ui(ui);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical(|ui| {
                let avail_w = ui.available_width().max(50.0);
                let avail_h = ui.available_height().max(50.0);
                let full = self.terminal.fullscreen;
                let term_h = if full {
                    (avail_h - 8.0).max(80.0)
                } else if self.terminal.collapsed {
                    34.0
                } else {
                    280.0
                };
                let editor_h = (avail_h - term_h - 8.0).max(80.0);
                if !full {
                    ui.allocate_ui_with_layout(
                        egui::vec2(avail_w, editor_h),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| self.editor.ui(ui),
                    );
                    ui.separator();
                }
                ui.allocate_ui_with_layout(
                    egui::vec2(avail_w, term_h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| self.terminal.ui(ui, &self.root),
                );
            });
        });

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(150));
    }
}
