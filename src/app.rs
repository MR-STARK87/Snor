use eframe::egui;
use std::path::PathBuf;

use crate::editor::Editor;
use crate::file_tree::FileTree;
use crate::git::GitPanel;
use crate::search::Search;
use crate::terminal::Terminal;

pub struct SnorApp {
    root: PathBuf,
    tree: FileTree,
    editor: Editor,
    terminal: Terminal,
    search: Search,
    git: GitPanel,
    status: String,
    git_loaded: bool,
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
            search: Search::new(),
            git: GitPanel::new(),
            status: String::from("ready"),
            git_loaded: false,
        }
    }
}

impl eframe::App for SnorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(opened) = self.tree.opened_file.take()
            && opened.is_file()
        {
            self.editor.open_file(opened.clone());
            self.status = format!("opened {}", opened.display());
        }
        if let Some(opened) = self.search.opened.take()
            && opened.is_file()
        {
            self.editor.open_file(opened.clone());
            self.status = format!("opened {}", opened.display());
        }
        if let Some(opened) = self.git.opened.take()
            && opened.is_file()
        {
            self.editor.open_file(opened.clone());
            self.status = format!("opened {}", opened.display());
        }
        if self.editor.saved_tick > 0 {
            self.git.refresh(&self.root);
            self.editor.saved_tick = 0;
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
                        egui::RichText::new(&self.git.branch)
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

        egui::Panel::right("snor_right")
            .default_size(320.0)
            .min_size(240.0)
            .resizable(true)
            .show(ui, |ui| {
                self.search.ui(ui, &self.root);
                ui.add_space(8.0);
                ui.separator();
                if !self.git_loaded {
                    self.git.refresh(&self.root);
                    self.git_loaded = true;
                }
                self.git.ui(ui, &self.root);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical(|ui| {
                let avail_w = ui.available_width().max(50.0);
                let avail_h = ui.available_height().max(50.0);
                let status_h = 26.0;
                let term_h = if self.terminal.collapsed { 34.0 } else { 280.0 };
                let editor_h = (avail_h - status_h - term_h - 16.0).max(80.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(avail_w, editor_h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| self.editor.ui(ui),
                );
                ui.separator();
                ui.allocate_ui_with_layout(
                    egui::vec2(avail_w, term_h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| self.terminal.ui(ui, &self.root),
                );
                ui.separator();
                ui.allocate_ui_with_layout(
                    egui::vec2(avail_w, status_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
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
                    },
                );
            });
        });

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(150));
    }
}
