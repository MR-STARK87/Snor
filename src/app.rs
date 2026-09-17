use eframe::egui;
use std::path::PathBuf;

use crate::dim::DimManager;
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

// --- Window placement --------------------------------------------------
/// Room left at the bottom of the monitor for the taskbar when deciding how
/// tall a floating window may be.
///
/// egui reports `monitor_size`, which is the *whole* monitor — there is no
/// work-area field to read, so the taskbar has to be estimated. 48pt is the
/// Windows default taskbar at 125% (60px / 1.25), which is what this machine
/// runs; at 100% the same bar is 48px, so the number holds on both.
const TASKBAR_RESERVE: f32 = 48.0;
/// Floor for the usable height, so a monitor that reports something absurd
/// (or nothing) cannot produce a zero or negative size.
const MIN_USABLE_H: f32 = 240.0;

/// Where a floating window should sit so it is fully visible.
///
/// Takes the monitor size and the size the window wants, both in egui points,
/// and returns the size to ask for plus the top-left corner to ask for.
/// Centred horizontally on the monitor, and vertically on the monitor minus
/// the taskbar reserve — a window centred on the full monitor height would
/// still hang its bottom edge under the taskbar.
///
/// Pure so it can be tested without a window; see
/// `floating_window_is_placed_fully_on_screen`.
fn fit_to_monitor(monitor: egui::Vec2, want: egui::Vec2) -> (egui::Vec2, egui::Pos2) {
    let usable_h = (monitor.y - TASKBAR_RESERVE).max(MIN_USABLE_H);
    let size = egui::vec2(want.x.min(monitor.x.max(1.0)), want.y.min(usable_h));
    let pos = egui::pos2((monitor.x - size.x) * 0.5, (usable_h - size.y) * 0.5);
    (size, pos)
}

// --- Chrome heights ----------------------------------------------------
//
// Measured off the reference mock (which renders at 125%) and converted to
// points: a 57px title bar and a 49px status bar.
/// Padding above the title bar's row. The row itself is 24pt (the window
/// controls' hit box sets its height), so the two pads together put the bar at
/// the reference's 45.6pt.
const TITLE_PAD: f32 = 9.0;
/// Padding below the title bar's row.
///
/// Deliberately smaller than the pad above. The reference centres its
/// title-bar content at y=28 of a 57px bar — dead centre — while ours measured
/// 25.5 of 58, sitting 2pt high. Equal pads cannot fix that: the bar's height
/// is already right, so the only way to move the row down is to move the split
/// down with it.
const TITLE_PAD_BELOW: f32 = 5.0;
/// Gap between the window controls and the "Stay consistent." mark.
///
/// The reference leaves a wide, deliberate stretch of empty bar between the
/// two: its leaf glyph starts 362px from the right edge and the minimise
/// control ends at 150, so 212px = 170pt of nothing. Ours had 151px, which
/// read as the mark being tacked onto the controls rather than parked in the
/// bar on its own.
const TITLE_RIGHT_GAP: f32 = 44.8;
/// Extra inset before the title bar's wordmark.
///
/// On top of the panel's 8pt inner margin, taking the wordmark's left edge to
/// the reference's 22.4pt from the window edge.
const TITLE_LEFT_PAD: f32 = 13.6;
/// Diameter of the dot separating "Snor" from its tagline.
const TITLE_DOT: f32 = 4.0;
/// Extra space before that dot. The reference's gap from the wordmark to the
/// dot is 19px = 15.2pt, against 9px after it — so the dot sits closer to the
/// tagline than to the name.
const TITLE_DOT_LEAD: f32 = 8.0;
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

// --- Custom window chrome ----------------------------------------------
//
// `main.rs` turns the OS decorations off, so these three constants plus
// `window_controls` and `window_resize_bands` are what the OS used to give us.
//
// Measured off the mock's title bar: its minimise dash, maximise square and
// close cross are centred 43.2pt apart, the close cross sits 24pt in from the
// window's right edge, and all three are centred in the 45.6pt bar.
/// Hit box of one window control. Larger than the ink it draws.
const CTRL_BOX: f32 = 24.0;
/// Centre-to-centre distance between the controls.
const CTRL_STEP: f32 = 43.2;
/// Gap between the close control's centre and the window's *content* edge.
///
/// The panel holding this row carries egui's default `Frame::side_top_panel`
/// inner margin of 8pt, so the drawn distance to the window edge is this plus
/// that margin. The reference puts the close cross 35.5px = 28.4pt from its
/// right edge, so 20.4 here lands it there — measured back off a live capture
/// at 35.5px against the reference's 35.5px.
const CTRL_EDGE: f32 = 20.4;
/// Thickness of the grab band along each window edge.
const RESIZE_BAND: f32 = 5.0;

pub struct SnorApp {
    tree: FileTree,
    editor: Editor,
    terminal: Terminal,
    /// Dim Mode: lowers the display backlight while agents keep running.
    /// Owned here, next to the other global UI state, and deliberately
    /// separate from `Terminal` — toggling brightness must never touch a
    /// pty or a child process.
    dim: DimManager,
    /// Flow Mode: the whole window becomes a multi-pane terminal
    /// workspace for supervising agents. The editor, explorer and normal
    /// terminal tabs hide, but nothing is destroyed — sessions, scrollback,
    /// editor buffers and pane sizes all survive the round trip.
    flow: bool,
    show_explorer: bool,
    /// Explorer width, driven by our own grip rather than egui's built-in
    /// panel resize. See [`SnorApp::tree_grip`] for why.
    tree_w: f32,
    /// Session whose directory the explorer is currently showing. Auto context
    /// switching keys on a *change* of this, not on the directories differing
    /// — see [`SnorApp::sync_context_root`] for why that distinction is the
    /// whole design.
    ctx_session: Option<u64>,
    /// Was the window maximised when F11 was pressed? Entering fullscreen
    /// deliberately restores the window first (see the F11 handler), so
    /// leaving it has to put the maximise back by hand — winit can only
    /// restore the placement it saved, and it saves it *after* the restore.
    /// Without this, exiting fullscreen would drop the window back to its
    /// floating size instead of the maximised one the user left.
    restore_maximized: bool,
    /// Has the one-shot "put the floating window fully on screen" run?
    ///
    /// The window cannot be placed before it exists, so this happens on the
    /// first frame rather than in `main.rs`. See
    /// [`SnorApp::fit_window_on_first_frame`].
    window_fitted: bool,
}

impl SnorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::fonts::install(&cc.egui_ctx);
        crate::theme::apply_dark(&cc.egui_ctx);
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let tree = FileTree::new(root);
        Self {
            tree,
            editor: Editor::new(),
            terminal: Terminal::new(),
            dim: DimManager::new(),
            flow: false,
            show_explorer: true,
            tree_w: TREE_DEFAULT_W,
            ctx_session: None,
            restore_maximized: false,
            window_fitted: false,
        }
    }

    /// Put the floating window fully on screen, once.
    ///
    /// `main.rs` asks for 1280x800 and sets no position, so Windows cascades
    /// the window wherever it likes. On this machine — 1920x1080 at 125%, so a
    /// 1600x1000 physical window on a 1020px work area — it landed at y=96,
    /// which put the bottom 76px below the work area: the window covered the
    /// taskbar and its last 17px were off the screen entirely. A *borderless*
    /// window gets no help from the OS here; a decorated one would have been
    /// clamped to the work area, and this one is deliberately undecorated.
    ///
    /// Only the floating case is touched. A maximised or fullscreen window is
    /// already placed by the OS, and moving it would fight the user.
    ///
    /// Nothing happens until egui has actually reported a monitor size, and
    /// the flag is only set once the move is issued — a `None` on the first
    /// frame must retry, not give up permanently.
    fn fit_window_on_first_frame(&mut self, ctx: &egui::Context) {
        if self.window_fitted {
            return;
        }
        let (monitor, size, maximized, fullscreen) = ctx.input(|i| {
            let v = i.viewport();
            (
                v.monitor_size,
                v.inner_rect.or(v.outer_rect).map(|r| r.size()),
                v.maximized.unwrap_or(false),
                v.fullscreen.unwrap_or(false),
            )
        });
        let (Some(monitor), Some(size)) = (monitor, size) else {
            return;
        };
        // A monitor of 1x1 (or less) is not a real measurement; retry.
        if monitor.x <= 1.0 || monitor.y <= 1.0 {
            return;
        }
        self.window_fitted = true;
        if maximized || fullscreen {
            return;
        }
        let (fitted, pos) = fit_to_monitor(monitor, size);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(fitted));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    }

    fn title_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("snor_top")
            .frame(egui::Frame::side_top_panel(ui.style()).fill(theme::surface_title()))
            .show(ui, |ui| {
                // Drag-to-move, registered *first* so the controls added below win
                // the hit test where they overlap it: egui resolves a press to the
                // most recently registered widget under the pointer, and a drag
                // band registered last would swallow every click on "close".
                let drag = ui.interact(
                    ui.max_rect(),
                    egui::Id::new("snor_title_drag"),
                    egui::Sense::click_and_drag(),
                );
                if drag.dragged() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                // Double-click the bar to toggle maximise, as every desktop does.
                if drag.double_clicked() {
                    let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }

                // The reference's title bar is 45.6pt tall with its row centred;
                // the text row itself only comes to ~20pt.
                ui.add_space(TITLE_PAD);
                ui.horizontal(|ui| {
                    // The reference insets its wordmark 28px = 22.4pt from the
                    // window's left edge; the panel's own 8pt inner margin only
                    // gets us to 8.8pt, which left the bar looking like the name
                    // had been shoved into the corner.
                    ui.add_space(TITLE_LEFT_PAD);
                    // No explorer toggle up here. The reference opens straight on
                    // "Snor" with nothing beside it, and a folder glyph sitting on
                    // the product name reads as a logo — which is exactly what the
                    // mock's wordmark is not. Ctrl+B still toggles the panel.
                    // 18.8 rather than 18: the reference's wordmark inks 47px wide
                    // against our 45 at 18pt.
                    ui.label(
                        egui::RichText::new("Snor")
                            .size(18.8)
                            .family(theme::medium())
                            .color(theme::accent()),
                    );
                    // The reference separates the product name from its tagline
                    // with a raised dot, not a plus. Drawn rather than typed: the
                    // mock's dot is a solid 4pt disc, and the bullet glyph at any
                    // sensible text size comes out 2.4pt — half the size — while
                    // also moving if the UI font changes.
                    ui.add_space(TITLE_DOT_LEAD);
                    let (dot, _) = ui.allocate_exact_size(
                        egui::vec2(TITLE_DOT, TITLE_DOT),
                        egui::Sense::hover(),
                    );
                    if ui.is_rect_visible(dot) {
                        ui.painter()
                            .circle_filled(dot.center(), TITLE_DOT * 0.5, theme::faint());
                    }
                    ui.label(
                        egui::RichText::new("Calm tools for focused minds.")
                            .size(11.5)
                            .color(theme::tagline()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Self::window_controls(ui);
                        ui.add_space(TITLE_RIGHT_GAP);

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
                ui.add_space(TITLE_PAD_BELOW);
            });
    }

    /// Minimise / maximise-or-restore / close, in the theme.
    ///
    /// With `with_decorations(false)` these are the only window controls there
    /// are, so they carry the real commands rather than being decoration. The
    /// reference draws its own for the same reason.
    ///
    /// The glyph helpers in `icons` each shrink their rect by their own fixed
    /// fraction (0.4 of the width for the dash and the cross, 0.56 for the
    /// square) and then stroke the result, so a glyph's *ink* is the shrunk
    /// rect plus one stroke width. Each control is handed the box that lands
    /// that ink on the mock's measured size rather than a shared one.
    ///
    /// Measured off the reference: cross 12x13px, square 14x14px, dash 14x2px
    /// — that is 9.6pt of ink for the cross, 11.2pt for the square and the
    /// dash. Subtracting each glyph's 1.3-1.4pt stroke is where the 8.3 / 9.8
    /// / 9.9 line lengths below come from. A shared 24pt box draws every glyph
    /// ~1.6pt oversize, which reads as coarse beside the reference's.
    fn window_controls(ui: &mut egui::Ui) {
        let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        let ink = theme::window_control();

        let step = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = CTRL_STEP - CTRL_BOX;
        ui.add_space(CTRL_EDGE - CTRL_BOX * 0.5);

        if icons::icon_button(ui, CTRL_BOX, "close", |p, r, _| {
            icons::close_x(
                p,
                egui::Rect::from_center_size(r.center(), egui::vec2(8.3 / 0.4, 8.3 / 0.4)),
                ink,
            );
        })
        .clicked()
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }

        if icons::icon_button(ui, CTRL_BOX, "maximize", |p, r, _| {
            icons::maximize(
                p,
                egui::Rect::from_center_size(r.center(), egui::vec2(9.8 / 0.56, 9.8 / 0.56)),
                ink,
                maximized,
            );
        })
        .clicked()
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }

        if icons::icon_button(ui, CTRL_BOX, "minimize", |p, r, _| {
            icons::minus(
                p,
                egui::Rect::from_center_size(r.center(), egui::vec2(9.9 / 0.4, 9.9 / 0.4)),
                ink,
            );
        })
        .clicked()
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        ui.spacing_mut().item_spacing.x = step;
    }

    /// Dim Mode says so, on screen.
    ///
    /// Lowering the backlight is invisible to anyone who did not press the
    /// key: the window just looks dark, and "dark theme on a dim monitor" is
    /// indistinguishable from "Dim Mode is on". The title-bar moon and the
    /// status-bar word are both inside chrome the eye skips. So this is the
    /// acknowledgement — a pill that states the mode and the way out.
    ///
    /// Three deliberate properties:
    ///
    /// * Painted on the foreground layer, last, so no panel can cover it.
    /// * Painted, never built from widgets: a badge that can take a click is
    ///   a badge that can swallow one meant for the shell underneath.
    /// * Ink is the bright accent on a dark pill rather than the reverse. The
    ///   screen is at `dim_level`% while this is visible, so it has to stay
    ///   legible after the backlight has already taken most of its contrast
    ///   away; the pill's own fill is the part that can afford to be dark.
    fn dim_overlay(&self, ui: &egui::Ui) {
        if !self.dim.is_active() {
            return;
        }
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("snor_dim_overlay"),
        ));

        let title_font = egui::FontId::new(11.5, theme::medium());
        let hint_font = egui::FontId::new(11.0, egui::FontFamily::Proportional);
        let title =
            painter.layout_no_wrap("DIM MODE ACTIVE".to_owned(), title_font, theme::accent());
        let hint = painter.layout_no_wrap(
            format!(
                "screen at {}%  ·  Ctrl+Shift+D to restore",
                self.dim.dim_level()
            ),
            hint_font,
            theme::dim_text(),
        );

        const ICON: f32 = 13.0;
        const ICON_GAP: f32 = 9.0;
        let pad = egui::vec2(14.0, 9.0);
        let line_gap = 3.0;
        let title_h = title.size().y;
        let text_w = title.size().x.max(hint.size().x);
        let text_h = title_h + line_gap + hint.size().y;
        let size = egui::vec2(pad.x * 2.0 + ICON + ICON_GAP + text_w, pad.y * 2.0 + text_h);

        // Bottom-centre, clear of the status bar's own row.
        let viewport = ui.ctx().viewport_rect();
        let pill = egui::Rect::from_min_size(
            egui::pos2(
                viewport.center().x - size.x * 0.5,
                viewport.bottom() - 34.0 - size.y,
            ),
            size,
        );
        painter.rect_filled(pill, 8.0, theme::surface_title());
        painter.rect_stroke(
            pill,
            8.0,
            egui::Stroke::new(1.5, theme::accent().gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );

        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(pill.left() + pad.x + ICON * 0.5, pill.center().y),
            egui::vec2(ICON, ICON),
        );
        icons::moon(&painter, icon_rect, theme::accent());

        let text_x = pill.left() + pad.x + ICON + ICON_GAP;
        let text_y = pill.center().y - text_h * 0.5;
        painter.galley(egui::pos2(text_x, text_y), title, theme::accent());
        painter.galley(
            egui::pos2(text_x, text_y + title_h + line_gap),
            hint,
            theme::dim_text(),
        );
    }

    /// The window's resize bands: the outermost few points of the client area,
    /// Grab bands along the window's edges and corners.
    ///
    /// Undecorated windows have no OS frame to grab, so resizing has to be
    /// requested from here. Registered after every panel — nothing else claims
    /// the outermost few points — with the corners after the edges, because a
    /// corner rect overlaps both of its edges and egui gives the press to the
    /// most recently registered widget under the pointer.
    fn window_resize_bands(ui: &mut egui::Ui) {
        use egui::ViewportCommand as Cmd;

        // Fullscreen too: the window covers the monitor exactly, so there is
        // nothing to drag it out to, and leaving the bands live would put
        // resize arrows on the screen's own edges — including over the top
        // edge where the title bar used to be.
        if ui.ctx().input(|i| {
            i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
        }) {
            return;
        }
        let r = ui.ctx().viewport_rect();
        let t = RESIZE_BAND;
        let edges = [
            (
                egui::Rect::from_min_max(r.min, egui::pos2(r.max.x, r.min.y + t)),
                egui::viewport::ResizeDirection::North,
                egui::CursorIcon::ResizeNorth,
            ),
            (
                egui::Rect::from_min_max(egui::pos2(r.min.x, r.max.y - t), r.max),
                egui::viewport::ResizeDirection::South,
                egui::CursorIcon::ResizeSouth,
            ),
            (
                egui::Rect::from_min_max(r.min, egui::pos2(r.min.x + t, r.max.y)),
                egui::viewport::ResizeDirection::West,
                egui::CursorIcon::ResizeWest,
            ),
            (
                egui::Rect::from_min_max(egui::pos2(r.max.x - t, r.min.y), r.max),
                egui::viewport::ResizeDirection::East,
                egui::CursorIcon::ResizeEast,
            ),
        ];
        let corners = [
            (
                egui::Rect::from_min_max(r.min, r.min + egui::vec2(t, t)),
                egui::viewport::ResizeDirection::NorthWest,
                egui::CursorIcon::ResizeNorthWest,
            ),
            (
                egui::Rect::from_min_max(
                    egui::pos2(r.max.x - t, r.min.y),
                    egui::pos2(r.max.x, r.min.y + t),
                ),
                egui::viewport::ResizeDirection::NorthEast,
                egui::CursorIcon::ResizeNorthEast,
            ),
            (
                egui::Rect::from_min_max(
                    egui::pos2(r.min.x, r.max.y - t),
                    egui::pos2(r.min.x + t, r.max.y),
                ),
                egui::viewport::ResizeDirection::SouthWest,
                egui::CursorIcon::ResizeSouthWest,
            ),
            (
                egui::Rect::from_min_max(egui::pos2(r.max.x - t, r.max.y - t), r.max),
                egui::viewport::ResizeDirection::SouthEast,
                egui::CursorIcon::ResizeSouthEast,
            ),
        ];

        for (i, (rect, dir, cursor)) in edges.into_iter().chain(corners).enumerate() {
            let resp = ui.interact(rect, egui::Id::new(("snor_resize", i)), egui::Sense::drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(cursor);
            }
            if resp.dragged() {
                ui.ctx().send_viewport_cmd(Cmd::BeginResize(dir));
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("snor_status")
            .frame(egui::Frame::side_top_panel(ui.style()).fill(theme::surface_recessed()))
            .show(ui, |ui| {
                // The reference's status bar is 39.2pt tall, so the 16pt row of
                // readouts sits in a lot of air.
                ui.add_space(STATUS_PAD);
                ui.horizontal(|ui| {
                    // Far left, mirroring the terminal's toggle at the far right,
                    // so the two panel toggles read as a pair. This is the only
                    // control that survives hiding the panel — the explorer header
                    // goes with it — so unlike the terminal's it has to read both
                    // ways rather than only offering to close what is already up.
                    let sidebar = self.show_explorer;
                    if icons::icon_button(ui, 20.0, "toggle sidebar (Ctrl+B)", |p, r, c| {
                        icons::panel_left(p, r.shrink(4.0), c, sidebar)
                    })
                    .clicked()
                    {
                        self.show_explorer = !self.show_explorer;
                    }
                    ui.add_space(STATUS_GAP);
                    // Flow Mode indicator: a small accent word, nothing more.
                    // The tooltip doubles as the command list — Snor has no
                    // palette, so every Flow shortcut is documented here and
                    // on the toggle that owns it.
                    if self.flow {
                        ui.label(
                            egui::RichText::new("Flow")
                                .size(12.5)
                                .color(theme::accent()),
                        )
                        .on_hover_text(
                            "Flow Mode — the window is yours, agents.\n\
                             Toggle: Ctrl+Shift+F · Add pane: Ctrl+Shift+H/V/T (max 4) · \
                             Focus next/previous: Alt+Right/Left · Close pane: Ctrl+Shift+W · \
                             Dividers drag — sizes stick.",
                        );
                        // A refused pane creation reports here rather than in
                        // the grid: the grid's top-left corner is the first
                        // pane's header, so painting there overlaid the note on
                        // the pane title and neither was readable.
                        if let Some(note) = self.terminal.notice_text().map(str::to_owned) {
                            ui.add_space(STATUS_GAP);
                            ui.label(egui::RichText::new(note).size(11.5).color(theme::faint()))
                                .on_hover_text(
                                    "Snor keeps four live shells: enough for a \
                                 supervisor and three agents. Close a pane \
                                 (Ctrl+Shift+W) to open another.",
                                );
                        }
                        ui.add_space(STATUS_GAP);
                    }
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
                    ui.label(egui::RichText::new("0").size(12.5).color(theme::dim_text()));
                    let (tri, _) =
                        ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    if ui.is_rect_visible(tri) {
                        icons::triangle_outline(&ui.painter_at(tri), tri, theme::dim_text());
                    }
                    ui.label(egui::RichText::new("0").size(12.5).color(theme::dim_text()));
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
                            // Closing the last terminal tab removes the section
                            // entirely, so un-hiding has to ask for a shell rather
                            // than just flipping a flag.
                            if self.terminal.hidden {
                                self.terminal.reveal(&self.tree.root);
                            } else {
                                self.terminal.hidden = true;
                            }
                        }
                        ui.add_space(STATUS_GAP);
                        // Dim Mode: a moon that fills with the accent while the
                        // screen is dimmed, plus the word itself so the state
                        // reads at a glance. Clicking toggles, same as
                        // Ctrl+Shift+D. Backend failures show as a short
                        // non-blocking note beside it; terminals keep running.
                        let dim_active = self.dim.is_active();
                        let dim_tip = format!(
                            "Dim Mode (Ctrl+Shift+D) — dims to {}%",
                            self.dim.dim_level()
                        );
                        if icons::icon_button(ui, 20.0, &dim_tip, |p, r, c| {
                            icons::moon(p, r, if dim_active { theme::accent() } else { c })
                        })
                        .clicked()
                        {
                            self.dim.toggle(
                                crate::brightness::get_brightness,
                                crate::brightness::set_brightness,
                            );
                        }
                        if dim_active {
                            ui.label(egui::RichText::new("Dim").size(12.5).color(theme::accent()));
                        }
                        if let Some(note) = self.dim.notice().map(str::to_owned) {
                            let short: String = note.chars().take(48).collect();
                            ui.label(egui::RichText::new(short).size(11.5).color(theme::danger()))
                                .on_hover_text(note);
                        }
                        // The reference spreads the readouts out rather than
                        // packing them: "Ln 6, Col 1   Spaces: 4   UTF-8   CRLF
                        // Rust".
                        for item in [
                            self.editor.active_lang(),
                            "CRLF".to_string(),
                            "UTF-8".to_string(),
                            "Spaces: 4".to_string(),
                            format!(
                                "Ln {}, Col {}",
                                self.editor.cursor_line, self.editor.cursor_col
                            ),
                        ] {
                            ui.label(
                                egui::RichText::new(item)
                                    .size(12.5)
                                    .color(theme::dim_text()),
                            );
                            ui.add_space(STATUS_GAP);
                        }
                    });
                });
                ui.add_space(STATUS_PAD);
            });
    }

    /// Editor + terminal column, split at an explicit divider.
    ///
    /// In Flow Mode the whole column goes to the terminal panes instead;
    /// see [`SnorApp::flow_workspace`].
    ///
    /// The panes are laid out against rects computed up front rather than by
    /// asking the parent `Ui` to reserve space for them. `Ui::scope` advances
    /// the parent cursor to the child's *content* rect, not the size that was
    /// requested, so a short editor used to collapse the column and leave the
    /// divider stranded at the top — dragging it changed the terminal height
    /// while the divider itself never moved.
    fn workspace(&mut self, ui: &mut egui::Ui) {
        if self.flow {
            self.flow_workspace(ui);
            return;
        }
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
            // Fullscreen means the terminal *takes* the column, not merely that
            // the editor is hidden. Leaving `term_rect` at `term_h` while
            // skipping the editor drew the terminal at its usual height with a
            // dead band of empty panel above it — the maximize button looked
            // like it did nothing.
            let term_rect = if full_scr {
                column
            } else {
                egui::Rect::from_min_max(
                    egui::pos2(column.left(), column.bottom() - term_h),
                    column.max,
                )
            };
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
                    egui::UiBuilder::new()
                        .max_rect(editor_rect)
                        .layout(top_down),
                    |ui| self.editor.ui(ui, &self.tree.root),
                );
            }
            if show_term {
                // The terminal sits on the recessed surface, one step below the
                // editor, so the two panes read as separate slabs. Painted
                // before the scope so it lands behind the header and the grid.
                ui.painter()
                    .rect_filled(term_rect, 0.0, theme::surface_recessed());
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
                    ui.painter().circle_filled(
                        egui::pos2(c.x + i as f32 * 8.0, c.y),
                        1.7,
                        theme::accent(),
                    );
                }
            }
            resp.on_hover_cursor(egui::CursorIcon::ResizeVertical);
        }
    }

    /// Flow Mode workspace: terminal panes take the whole column. The
    /// editor, its tabs and the explorer are simply not drawn — their state
    /// sits untouched in `self`, so leaving restores the exact arrangement.
    fn flow_workspace(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let root = self.tree.root.clone();
            self.terminal.flow_ui(ui, &root);
        });
    }

    fn toggle_flow(&mut self) {
        if self.flow {
            self.exit_flow();
        } else {
            self.enter_flow();
        }
    }

    fn enter_flow(&mut self) {
        self.flow = true;
        let root = self.tree.root.clone();
        self.terminal.flow_enter(&root);
    }

    fn exit_flow(&mut self) {
        self.flow = false;
        // Land the tab strip on the focused pane; the pane layout itself is
        // kept, so toggling back restores splits and sizes.
        self.terminal.flow_exit_sync();
    }

    /// Auto context switching: the explorer follows the focused terminal.
    ///
    /// Moving to a terminal that was opened somewhere else re-roots the tree at
    /// that directory, so the file list always describes the project the
    /// terminal you are typing into belongs to. Works in both modes — the
    /// focused pane in Flow Mode, the active tab otherwise.
    ///
    /// **It fires on a change of focus, not on the directories differing.**
    /// That distinction is the whole design. Comparing directories every frame
    /// would fight the user: pick a folder from the header's dialog and the
    /// very next frame would put the focused shell's directory back, leaving
    /// the dialog unable to do anything while any terminal is open. Keying on
    /// the session id means a manual choice survives until focus actually
    /// moves, which is what "switch context when I move to another terminal"
    /// is supposed to mean.
    ///
    /// The limit worth knowing: `session.cwd` is the directory a shell was
    /// *spawned* in, not one it has `cd`-ed into since. Nothing in this app
    /// reads the shell's own output, so a pane that changes directory by hand
    /// keeps the directory it started with, and the explorer will not follow
    /// it. Following a `cd` needs OSC 7 emitted by the shell and parsed in
    /// `terminal.rs`, which PowerShell does not do by default.
    fn sync_context_root(&mut self) {
        let Some((id, cwd)) = self.terminal.context_session(self.flow) else {
            return;
        };
        if self.ctx_session == Some(id) {
            return;
        }
        self.ctx_session = Some(id);
        if cwd != self.tree.root {
            self.tree.set_root(cwd);
        }
    }

    /// Explorer resize grip.
    ///
    /// Hidden with the explorer in Flow Mode; `tree_w` itself is untouched,
    /// so the panel comes back at its old width.
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
                ui.painter().circle_filled(
                    egui::pos2(c.x, c.y + i as f32 * 8.0),
                    1.7,
                    theme::accent(),
                );
            }
        }
        resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    }
}

impl eframe::App for SnorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.fit_window_on_first_frame(ui.ctx());
        if let Some(opened) = self.tree.opened_file.take()
            && opened.is_file()
        {
            self.editor.open_file(opened);
        }
        if std::mem::take(&mut self.editor.want_run) {
            self.terminal.send_line("cargo run", &self.tree.root);
        }
        // The empty editor's two actions. The editor has no handle on the file
        // tree, so it raises a flag and the shell does the work. Both reveal
        // the panel first: a create field or a file list you cannot see is no
        // use.
        if std::mem::take(&mut self.editor.want_new_file) {
            self.show_explorer = true;
            self.tree.begin_create_at_root(false);
        }
        if std::mem::take(&mut self.editor.want_workspace) {
            self.show_explorer = true;
        }
        // "Open terminal here" from a folder's context menu. Handled up here
        // with the other polls so the shell exists by the time the terminal is
        // drawn this frame. This is also what makes auto context switching
        // reachable at all: it is the only way to get two terminals that
        // disagree about which directory they are in.
        if let Some(dir) = self.tree.take_terminal_request() {
            self.terminal.open_at(&dir);
        }

        // Ctrl+Tab and Ctrl+B rearrange the *normal* workspace, so they
        // rest while Flow Mode owns the window. Otherwise toggling a hidden
        // panel would silently rewrite the arrangement Flow Mode restores.
        if !self.flow
            && ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::Tab,
                ))
            })
        {
            // Ctrl+Tab is the way back from an emptied panel: closing the last
            // tab takes the whole section away, and this brings it back with a
            // fresh shell rather than an empty strip.
            if self.terminal.hidden {
                self.terminal.reveal(&self.tree.root);
            } else {
                self.terminal.hidden = true;
            }
        }
        if !self.flow
            && ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::B,
                ))
            })
        {
            self.show_explorer = !self.show_explorer;
        }
        // F11: fullscreen. The only window-level command with no modifier —
        // every plain key belongs to the shell or the editor, but F11 is not a
        // key either of them has a use for, and it is what every other
        // application on the machine uses for this.
        //
        // The current state is read back from the viewport rather than kept in
        // a mirrored bool: the OS can leave fullscreen on its own (a screen
        // lock, another window going fullscreen), and a local flag would then
        // be wrong in the direction that makes the first press do nothing.
        //
        // Handled up here with the other global shortcuts, so it works in
        // every layout and `consume_key` stops the shell ever seeing the key.
        //
        // Read once here and reused by the title bar and the resize bands
        // below, so all three agree within a frame. A `send_viewport_cmd` does
        // not take effect until egui-winit has processed the platform output
        // and winit has reported the new size back, so the flag lags the key by
        // one frame — harmless, because the window is still being resized.
        let fullscreen = ui.ctx().input(|i| i.viewport().fullscreen).unwrap_or(false);
        let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
            if fullscreen {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                // Put the maximise back only if that is how we found it.
                // `take` so a stale flag cannot re-maximise on a later,
                // unrelated exit.
                if std::mem::take(&mut self.restore_maximized) {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Maximized(true));
                }
            } else {
                // Leave maximised before going fullscreen.
                //
                // A window that still carries WS_MAXIMIZE keeps the maximised
                // *client* size whatever rectangle winit hands `SetWindowPos`:
                // the window ends up monitor-sized on the outside — so the
                // taskbar is covered and `GetWindowRect` reads 1920x1080 — while
                // the client stays at the old work-area height. egui is told the
                // smaller height, paints only that far, and the bottom band of
                // the screen is left holding stale pixels. Measured on a
                // maximised window: fullscreen left the app painting 1920x1020
                // inside a 1920x1080 window.
                //
                // Both commands go to winit's window thread and run in order,
                // and the un-maximise is a plain `ShowWindow(SW_RESTORE)` — so
                // the `SetWindowPos` that follows already sees a normal window.
                self.restore_maximized = maximized;
                if maximized {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                }
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            }
        }
        // Dim Mode (Ctrl+Shift+D): lower the display backlight without
        // touching any terminal or agent process. Handled up here with the
        // other global shortcuts so it works in every layout — editor,
        // terminal split, fullscreen — and before the terminal sees the
        // key, so the shell never receives it as input. Ctrl+Shift rather
        // than plain Ctrl+D, which the shell needs for EOF.
        if ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                egui::Key::D,
            ))
        }) {
            self.dim.toggle(
                crate::brightness::get_brightness,
                crate::brightness::set_brightness,
            );
        }
        // Flow Mode (Ctrl+Shift+F): the whole window becomes terminal
        // panes for supervising agents. The Ctrl+Shift family is free —
        // the terminal only maps Ctrl+C/D/Z/L, and Ctrl+F/B/S/Tab are the
        // existing app shortcuts — and Alt+arrows are ignored by shells
        // entirely, so panes never steal shell input. Handled up here, so
        // every command works in any layout and the shell never sees the
        // chords.
        if ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                egui::Key::F,
            ))
        }) {
            self.toggle_flow();
        }
        // Split commands enter Flow Mode when it is off: asking for a pane
        // is asking for the workspace that shows panes. Every split adds
        // one shell and the adaptive grid tiles it: two side by side,
        // three as two over one full-width pane, four as 2x2.
        if ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                egui::Key::H,
            ))
        }) {
            self.enter_flow();
            let root = self.tree.root.clone();
            self.terminal.flow_add_pane(&root);
        }
        if ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                egui::Key::V,
            ))
        }) {
            self.enter_flow();
            let root = self.tree.root.clone();
            self.terminal.flow_add_pane(&root);
        }
        // New terminal pane. Flow-only: normal mode already grows shells
        // through the tab strip's `+`, and this must not invent a second
        // meaning there.
        if self.flow
            && ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                    egui::Key::T,
                ))
            })
        {
            let root = self.tree.root.clone();
            self.terminal.flow_add_pane(&root);
        }
        // Close the focused pane. Flow-only: outside Flow Mode Ctrl+W-family
        // chords belong to shells and the editor, not to us.
        if self.flow
            && ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                    egui::Key::W,
                ))
            })
        {
            self.terminal.flow_close_focused();
        }
        // Pane focus, wrapping in grid order. Flow-only and Alt-based, so
        // normal-mode arrow keys (editor caret, shell history) are untouched.
        if self.flow
            && ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::ALT,
                    egui::Key::ArrowRight,
                ))
            })
        {
            self.terminal.flow_step_focus(1);
        }
        if self.flow
            && ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::ALT,
                    egui::Key::ArrowLeft,
                ))
            })
        {
            self.terminal.flow_step_focus(-1);
        }

        // No title bar in fullscreen. Ours is drawn by us precisely because the
        // OS's is off, so hiding it is the only thing that makes the transition
        // legible: with it left in place the window grows to the whole screen,
        // the taskbar disappears, and nothing else moves — which is exactly how
        // "F11 doesn't work, it just hides the taskbar" was reported. The three
        // window controls go with it; minimise and close have no meaning on a
        // fullscreen window, and the shortcut that got us here is the one that
        // leaves.
        if !fullscreen {
            self.title_bar(ui);
        }
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
        // The explorer hides with everything else in Flow Mode; the flag
        // and width are left alone so the panel returns exactly as it was.
        if self.show_explorer && !self.flow {
            let panel = egui::Panel::left("snor_tree")
                .exact_size(self.tree_w)
                .resizable(false)
                .frame(egui::Frame::side_top_panel(ui.style()).fill(theme::surface_recessed()))
                .show(ui, |ui| {
                    self.tree.ui(ui);
                });
            panel_rect = Some(panel.response.rect);
        }

        self.workspace(ui);

        // After the workspace, because that is where focus changes: a click on
        // a pane or a tab has been applied by the time this runs, so the
        // explorer re-roots in the same frame the user moved. The tree panel
        // is drawn above, so it paints the new root on the next frame — one
        // frame of lag, and a repaint is already requested every 150ms.
        self.sync_context_root();

        if let Some(rect) = panel_rect {
            self.tree_grip(ui, rect);
        }

        // After every panel: the resize bands own the outermost points of the
        // window, and nothing else may claim them.
        Self::window_resize_bands(ui);

        // The window's own frame. Drawn last, on the foreground layer, so it
        // sits over every panel instead of being clipped by the one under the
        // pointer. With the OS decorations off this *is* the window's boundary,
        // not a line drawn inside someone else's.
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

        // After the frame, so the badge is the topmost thing on screen and
        // cannot be clipped by the window's own edge.
        self.dim_overlay(ui);

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(150));
    }
}

impl Drop for SnorApp {
    fn drop(&mut self) {
        // Leaving while dimmed hands the screen back at its pre-dim level.
        // Best effort by design: shutdown must never panic or hang waiting
        // on display control, and terminals are reaped by their own `Drop`.
        self.dim.restore_on_exit(crate::brightness::set_brightness);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live numbers this was written against: a 1920x1080 monitor at 125%
    /// is 1536x864 points, and the requested 1280x800 window lands at y=96
    /// physical when Windows cascades it — 76px below the 1020px work area.
    #[test]
    fn floating_window_is_placed_fully_on_screen() {
        let monitor = egui::vec2(1536.0, 864.0);
        let want = egui::vec2(1280.0, 800.0);
        let (size, pos) = fit_to_monitor(monitor, want);

        // 800pt fits inside 864 - 48, so the size is left alone...
        assert_eq!(size, want, "a window that fits must not be shrunk");
        // ...and it is centred in the usable band, not the full monitor.
        let usable_h = 864.0 - TASKBAR_RESERVE;
        assert_eq!(pos.x, (monitor.x - size.x) * 0.5);
        assert_eq!(pos.y, (usable_h - size.y) * 0.5);

        // The whole point: the bottom edge clears the taskbar.
        assert!(
            pos.y + size.y <= usable_h,
            "bottom {} must not reach past the usable height {usable_h}",
            pos.y + size.y
        );
        assert!(pos.x >= 0.0 && pos.y >= 0.0, "must not start off-screen");
        assert!(pos.x + size.x <= monitor.x, "must not run off the right edge");
    }

    #[test]
    fn a_window_taller_than_the_monitor_is_shrunk_to_fit() {
        // A 4K-tall request on a short monitor: the size has to give, and the
        // result still has to sit inside the usable band.
        let monitor = egui::vec2(1280.0, 720.0);
        let (size, pos) = fit_to_monitor(monitor, egui::vec2(1600.0, 1200.0));
        let usable_h = 720.0 - TASKBAR_RESERVE;

        assert_eq!(size.x, 1280.0, "width clamps to the monitor");
        assert_eq!(size.y, usable_h, "height clamps to the usable band");
        assert!(pos.y + size.y <= usable_h + 0.001);
        assert!(pos.x >= 0.0);
    }

    #[test]
    fn a_misreported_monitor_cannot_produce_a_negative_size() {
        // Guards the floor: a 0-height or absurd monitor must not yield a
        // negative size, which would be rejected by the viewport command.
        for (mx, my) in [(0.0, 0.0), (1.0, 1.0), (800.0, 10.0)] {
            let (size, pos) = fit_to_monitor(egui::vec2(mx, my), egui::vec2(1280.0, 800.0));
            assert!(size.x >= 0.0 && size.y >= 0.0, "negative size at {mx}x{my}");
            assert!(size.y <= MIN_USABLE_H.max(my - TASKBAR_RESERVE));
            assert!(pos.x.is_finite() && pos.y.is_finite(), "non-finite at {mx}x{my}");
        }
    }
}
