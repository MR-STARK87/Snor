use eframe::egui;
use std::path::PathBuf;

use crate::editor::Editor;
use crate::file_tree::FileTree;
use crate::icons;
use crate::terminal::Terminal;
use crate::theme;

/// Height of the editor/terminal divider's grab band.
const SPLIT_GRAB: f32 = 7.0;
/// Neither pane may be squeezed below these, so the split can't be dragged
/// into a state the user can't drag back out of.
const EDITOR_MIN_H: f32 = 140.0;
const TERM_MIN_H: f32 = 120.0;
/// Height of the terminal when collapsed down to its header strip.
const TERM_COLLAPSED_H: f32 = 34.0;

const TREE_DEFAULT_W: f32 = 260.0;
const TREE_MIN_W: f32 = 180.0;
const TREE_MAX_W: f32 = 520.0;
/// Half-width of the explorer's resize grip.
const TREE_GRAB: f32 = 5.0;

// --- Chrome heights ----------------------------------------------------
//
// Measured off the reference mock (which renders at 125%) and converted to
// points: a 57px title bar and a 49px status bar.
/// Padding above and below the title bar's row. The row itself is ~24pt (the
/// explorer toggle sets its height), so twice this puts the bar at the
/// reference's 45.6pt.
const TITLE_PAD: f32 = 7.0;
/// Padding above and below the status bar's row.
///
/// The reference's bar is 50px tall — its top rule sits at y=932 with the
/// window's interior ending at y=981 — which is 40.0pt. The row itself
/// measures 26.6pt live (the 14pt branch glyph is not the tallest thing in it;
/// the "show terminal" button is), leaving this split either side.
const STATUS_PAD: f32 = 6.7;
/// Gap between the status bar's right-hand readouts.
///
/// The reference spreads them much further than egui would: its readouts sit
/// 29.6–33.6pt apart, measured edge to edge. This is the added space on top of
/// egui's own 6pt `item_spacing.x`, which already separates the labels.
const STATUS_GAP: f32 = 24.5;
/// Width of the frame drawn around the whole client area.
///
/// The reference's is 2px at 125% = 1.6pt. See [`theme::window_edge`] for why
/// it is a different colour from every other divider in the app.
const WINDOW_EDGE_W: f32 = 1.6;

pub struct SnorApp {
    tree: FileTree,
    editor: Editor,
    terminal: Terminal,
    show_explorer: bool,
    /// Explorer width, driven by our own grip rather than egui's built-in
    /// panel resize. See [`SnorApp::tree_grip`] for why.
    tree_w: f32,
}

impl SnorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::apply_dark(&cc.egui_ctx);
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let tree = FileTree::new(root);
        Self {
            tree,
            editor: Editor::new(),
            terminal: Terminal::new(),
            show_explorer: true,
            tree_w: TREE_DEFAULT_W,
        }
    }

    fn title_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("snor_top").show(ui, |ui| {
            // The reference's title bar is 45.6pt tall with its row centred;
            // the text row itself only comes to ~20pt.
            ui.add_space(TITLE_PAD);
            ui.horizontal(|ui| {
                // No explorer toggle up here. The reference opens straight on
                // "Snor" with nothing beside it, and a folder glyph sitting on
                // the product name reads as a logo — which is exactly what the
                // mock's wordmark is not. Ctrl+B still toggles the panel.
                ui.label(
                    egui::RichText::new("Snor")
                        .size(18.0)
                        .strong()
                        .color(theme::accent()),
                );
                // The reference separates the product name from its tagline
                // with a raised dot, not a plus.
                ui.label(egui::RichText::new("\u{2022}").size(11.5).color(theme::faint()));
                ui.label(
                    egui::RichText::new("Calm tools for focused minds.")
                        .size(11.5)
                        .color(theme::tagline()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Right-to-left: the label goes in first so the leaf ends
                    // up on its *left*, matching the reference's leaf-then-text
                    // reading order.
                    ui.label(
                        egui::RichText::new("Stay consistent.")
                            .size(12.5)
                            .color(theme::dim_text()),
                    );
                    let (slot, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    if ui.is_rect_visible(slot) {
                        icons::leaf(&ui.painter_at(slot), slot, theme::accent());
                    }
                });
            });
            ui.add_space(TITLE_PAD);
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("snor_status").show(ui, |ui| {
            // The reference's status bar is 39.2pt tall, so the 16pt row of
            // readouts sits in a lot of air.
            ui.add_space(STATUS_PAD);
            ui.horizontal(|ui| {
                let (branch, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                if ui.is_rect_visible(branch) {
                    icons::branch(&ui.painter_at(branch), branch, theme::dim_text());
                }
                ui.label(
                    egui::RichText::new("main")
                        .size(12.5)
                        .color(theme::dim_text()),
                );
                // Sync indicator: a filled accent dot with a knocked-out centre.
                let (dot, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                if ui.is_rect_visible(dot) {
                    let p = ui.painter_at(dot);
                    p.circle_filled(dot.center(), 4.6, theme::accent());
                    p.circle_filled(dot.center(), 1.6, theme::on_accent());
                }
                ui.label(
                    egui::RichText::new("0")
                        .size(12.5)
                        .color(theme::dim_text()),
                );
                let (tri, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                if ui.is_rect_visible(tri) {
                    icons::triangle_outline(&ui.painter_at(tri), tri, theme::dim_text());
                }
                ui.label(
                    egui::RichText::new("0")
                        .size(12.5)
                        .color(theme::dim_text()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Right-to-left, so the first thing added lands furthest
                    // right: the toggle goes in first to sit where the
                    // reference's own status-bar icon does, with the readouts
                    // marching away to its left. An icon rather than a
                    // `small_button` — a filled pill among flat text was the
                    // same "bolted on" problem as the old open-folder button.
                    let open = !self.terminal.hidden;
                    if icons::icon_button(ui, 20.0, "toggle terminal (Ctrl+Tab)", |p, r, c| {
                        icons::panel_bottom(p, r.shrink(4.0), c, open)
                    })
                    .clicked()
                    {
                        self.terminal.hidden = !self.terminal.hidden;
                    }
                    ui.add_space(STATUS_GAP);
                    // The reference spreads the readouts out rather than
                    // packing them: "Ln 6, Col 1   Spaces: 4   UTF-8   CRLF
                    // Rust".
                    for item in [
                        self.editor.active_lang(),
                        "CRLF".to_string(),
                        "UTF-8".to_string(),
                        "Spaces: 4".to_string(),
                        format!("Ln {}, Col {}", self.editor.cursor_line, self.editor.cursor_col),
                    ] {
                        ui.label(egui::RichText::new(item).size(12.5).color(theme::dim_text()));
                        ui.add_space(STATUS_GAP);
                    }
                });
            });
            ui.add_space(STATUS_PAD);
        });
    }

    /// Editor + terminal column, split at an explicit divider.
    ///
    /// The panes are laid out against rects computed up front rather than by
    /// asking the parent `Ui` to reserve space for them. `Ui::scope` advances
    /// the parent cursor to the child's *content* rect, not the size that was
    /// requested, so a short editor used to collapse the column and leave the
    /// divider stranded at the top — dragging it changed the terminal height
    /// while the divider itself never moved.
    fn workspace(&mut self, ui: &mut egui::Ui) {
        let mut split: Option<(egui::Rect, f32)> = None;

        egui::CentralPanel::default().show(ui, |ui| {
            let column = ui.available_rect_before_wrap();
            // No quote rail. The reference parks two aphorisms in a 147pt
            // column down the right-hand edge, but a whole column of chrome
            // for decoration is not worth the horizontal space an editor
            // actually wants, so the editor and terminal get the full width.

            let show_term = !self.terminal.hidden;
            let full_scr = show_term && self.terminal.fullscreen;
            let collapsed = show_term && self.terminal.collapsed;
            let max_term = (column.height() - EDITOR_MIN_H - SPLIT_GRAB).max(TERM_MIN_H);

            let term_h = if collapsed {
                TERM_COLLAPSED_H
            } else {
                self.terminal.term_h.clamp(TERM_MIN_H, max_term)
            };
            let term_rect = egui::Rect::from_min_max(
                egui::pos2(column.left(), column.bottom() - term_h),
                column.max,
            );
            let editor_rect = if show_term {
                egui::Rect::from_min_max(
                    column.min,
                    egui::pos2(column.right(), term_rect.top() - SPLIT_GRAB),
                )
            } else {
                column
            };
            if show_term && !full_scr {
                split = Some((
                    egui::Rect::from_min_max(
                        egui::pos2(column.left(), term_rect.top() - SPLIT_GRAB),
                        egui::pos2(column.right(), term_rect.top()),
                    ),
                    max_term,
                ));
            }

            let top_down = egui::Layout::top_down(egui::Align::LEFT);
            if !full_scr {
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(editor_rect).layout(top_down),
                    |ui| self.editor.ui(ui, &self.tree.root),
                );
            }
            if show_term {
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(term_rect).layout(top_down),
                    |ui| self.terminal.ui(ui, &self.tree.root),
                );
            }
        });

        // Registered after both panes on purpose: egui resolves a press to the
        // most recently registered widget under the pointer, so a divider
        // registered with the editor would lose its outer half to the editor's
        // scroll area.
        if let Some((rect, max_term)) = split {
            let resp = ui.interact(rect, egui::Id::new("snor_split"), egui::Sense::drag());
            if resp.dragged() {
                self.terminal.term_h =
                    (self.terminal.term_h - resp.drag_delta().y).clamp(TERM_MIN_H, max_term);
            }
            let (color, width) = if resp.dragged() {
                (theme::accent(), 2.0)
            } else if resp.hovered() {
                (theme::text(), 2.0)
            } else {
                (theme::hairline(), 1.0)
            };
            ui.painter().hline(
                rect.x_range(),
                rect.center().y,
                egui::Stroke::new(width, color),
            );
            if resp.hovered() || resp.dragged() {
                // Dotted grip: the idle hairline is deliberately faint, so the
                // hover state has to say "this is draggable" out loud.
                let c = rect.center();
                for i in -1..=1 {
                    ui.painter()
                        .circle_filled(egui::pos2(c.x + i as f32 * 8.0, c.y), 1.7, theme::accent());
                }
            }
            resp.on_hover_cursor(egui::CursorIcon::ResizeVertical);
        }
    }

    /// Explorer resize grip.
    ///
    /// egui's own panel resize handle is a `resize_grab_radius_side`-wide band
    /// centred on the panel edge, so half of it lies over the editor and is
    /// claimed by whatever the central panel registers later. Grabbing just
    /// past the edge therefore did nothing. Registering our own grip *after*
    /// every other widget, and driving the width ourselves via
    /// `Panel::exact_size`, gives the whole band to the grip.
    fn tree_grip(&mut self, ui: &mut egui::Ui, panel: egui::Rect) {
        let edge = panel.max.x;
        let grip = egui::Rect::from_min_max(
            egui::pos2(edge - TREE_GRAB, panel.top()),
            egui::pos2(edge + TREE_GRAB, panel.bottom()),
        );
        let resp = ui.interact(grip, egui::Id::new("snor_tree_grip"), egui::Sense::drag());
        if resp.dragged() {
            let max_w = (ui.available_width() - 400.0).clamp(TREE_MIN_W, TREE_MAX_W);
            self.tree_w = (self.tree_w + resp.drag_delta().x).clamp(TREE_MIN_W, max_w);
        }
        if resp.hovered() || resp.dragged() {
            ui.painter().vline(
                edge,
                panel.y_range(),
                egui::Stroke::new(2.0, theme::accent()),
            );
            let c = egui::pos2(edge, panel.center().y);
            for i in -1..=1 {
                ui.painter()
                    .circle_filled(egui::pos2(c.x, c.y + i as f32 * 8.0), 1.7, theme::accent());
            }
        }
        resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
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
        // Shown before the explorer on purpose: in the reference the panel
        // divider stops at the status bar and the bar runs the full window
        // width. Reversing these two puts the bar back on the right of the
        // explorer, which is what the divider crossing it would look like.
        self.status_bar(ui);

        if self.tree.take_open_request()
            && let Some(dir) = rfd::FileDialog::new()
                .set_directory(&self.tree.root)
                .pick_folder()
        {
            self.tree.set_root(dir);
        }

        let mut panel_rect = None;
        if self.show_explorer {
            let panel = egui::Panel::left("snor_tree")
                .exact_size(self.tree_w)
                .resizable(false)
                .show(ui, |ui| {
                    self.tree.ui(ui);
                });
            panel_rect = Some(panel.response.rect);
        }

        self.workspace(ui);

        if let Some(rect) = panel_rect {
            self.tree_grip(ui, rect);
        }

        // The window's own frame. Drawn last, on the foreground layer, so it
        // sits over every panel instead of being clipped by the one under the
        // pointer. The reference draws its edge as part of the product rather
        // than leaving the boundary to the OS.
        ui.ctx()
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("snor_window_edge"),
            ))
            .rect_stroke(
                ui.ctx().viewport_rect(),
                0.0,
                egui::Stroke::new(WINDOW_EDGE_W, theme::window_edge()),
                egui::StrokeKind::Inside,
            );

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(150));
    }
}
