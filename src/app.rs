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
    show_explorer: bool,
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
            show_explorer: true,
        }
    }

    fn title_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("snor_top").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .small_button("≡")
                    .on_hover_text("toggle explorer (Ctrl+B)")
                    .clicked()
                {
                    self.show_explorer = !self.show_explorer;
                }
                ui.label(
                    egui::RichText::new("Snor")
                        .color(crate::theme::accent())
                        .strong(),
                );
                ui.label(
                    egui::RichText::new("Calm tools for focused minds.")
                        .color(crate::theme::dim_text())
                        .small(),
                );
                ui.label(
                    egui::RichText::new(self.root.display().to_string())
                        .color(crate::theme::faint())
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
                    ui.label(
                        egui::RichText::new("Stay consistent.")
                            .color(crate::theme::dim_text())
                            .small()
                            .italics(),
                    );
                });
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("snor_status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("main").small().color(crate::theme::dim_text()));
                ui.label(
                    egui::RichText::new("●")
                        .small()
                        .color(crate::theme::accent()),
                );
                ui.label(
                    egui::RichText::new("0 / 0")
                        .small()
                        .color(crate::theme::dim_text()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for item in [
                        self.editor.active_lang(),
                        "CRLF".to_string(),
                        "UTF-8".to_string(),
                        "Spaces: 4".to_string(),
                        format!("Ln {}, Col {}", self.editor.cursor_line, self.editor.cursor_col),
                    ] {
                        ui.label(egui::RichText::new(item).small().color(crate::theme::dim_text()));
                    }
                    if ui
                        .small_button(if self.terminal.hidden {
                            "show terminal"
                        } else {
                            "hide terminal"
                        })
                        .on_hover_text("toggle terminal (Ctrl+Tab)")
                        .clicked()
                    {
                        self.terminal.hidden = !self.terminal.hidden;
                    }
                });
            });
        });
    }

    fn quote_rail(ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.add_space(60.0);
            ui.label(
                egui::RichText::new("Good software takes time, but it makes time for you.")
                    .small()
                    .italics()
                    .color(crate::theme::faint()),
            );
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.add_space(60.0);
                ui.label(
                    egui::RichText::new("Small steps build big things.")
                        .small()
                        .italics()
                        .color(crate::theme::faint()),
                );
            });
        });
    }
}

impl eframe::App for SnorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(opened) = self.tree.opened_file.take()
            && opened.is_file()
        {
            self.editor.open_file(opened);
        }
        if std::mem::take(&mut self.editor.want_run) {
            self.terminal.send_line("cargo run");
        }

        if ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::Tab,
            ))
        }) {
            self.terminal.hidden = !self.terminal.hidden;
            if !self.terminal.hidden {
                self.terminal.collapsed = false;
            }
        }
        if ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::B,
            ))
        }) {
            self.show_explorer = !self.show_explorer;
        }

        self.title_bar(ui);

        if self.show_explorer {
            egui::Panel::left("snor_tree")
                .default_size(260.0)
                .min_size(200.0)
                .resizable(true)
                .show(ui, |ui| {
                    self.tree.ui(ui);
                });
        }

        self.status_bar(ui);

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let rail_w = if ui.available_width() > 1020.0 {
                    150.0
                } else {
                    0.0
                };
                let main_w = (ui.available_width() - rail_w - 8.0).max(50.0);
                let main_h = ui.available_height().max(50.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(main_w, main_h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.vertical(|ui| {
                            let avail_w = ui.available_width().max(50.0);
                            let avail_h = ui.available_height().max(50.0);
                            let show_term = !self.terminal.hidden;
                            let full = show_term && self.terminal.fullscreen;
                            let resizable =
                                show_term && !full && !self.terminal.collapsed;
                            let term_h = if !show_term {
                                0.0
                            } else if full {
                                (avail_h - 8.0).max(80.0)
                            } else if self.terminal.collapsed {
                                34.0
                            } else {
                                self.terminal.term_h.clamp(120.0, (avail_h - 220.0).max(140.0))
                            };
                            let chrome =
                                if show_term && !full && resizable { 14.0 } else { 8.0 };
                            let editor_h = (avail_h - term_h - chrome).max(80.0);
                            if !full {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(avail_w, editor_h),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| self.editor.ui(ui, &self.root),
                                );
                            }
                            if resizable {
                                // Draggable divider: pull up for more terminal,
                                // down for more editor.
                                let div = ui
                                    .allocate_response(
                                        egui::vec2(avail_w, 6.0),
                                        egui::Sense::drag(),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::ResizeVertical);
                                if div.dragged() {
                                    let max_h = (avail_h - 220.0).max(140.0);
                                    self.terminal.term_h =
                                        (self.terminal.term_h - div.drag_delta().y)
                                            .clamp(120.0, max_h);
                                }
                                let c = if div.dragged() || div.hovered() {
                                    crate::theme::accent()
                                } else {
                                    crate::theme::faint()
                                };
                                ui.painter().line_segment(
                                    [
                                        egui::pos2(div.rect.left(), div.rect.center().y),
                                        egui::pos2(div.rect.right(), div.rect.center().y),
                                    ],
                                    egui::Stroke::new(1.0, c),
                                );
                            } else if show_term {
                                ui.separator();
                            }
                            if show_term {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(avail_w, term_h),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| self.terminal.ui(ui, &self.root),
                                );
                            }
                        });
                    },
                );
                if rail_w > 0.0 {
                    ui.separator();
                    ui.allocate_ui_with_layout(
                        egui::vec2(rail_w, main_h),
                        egui::Layout::top_down(egui::Align::LEFT),
                        Self::quote_rail,
                    );
                }
            });
        });

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(150));
    }
}

