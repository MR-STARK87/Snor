//! The installer's few widgets, built from the app's palette and faces.
//!
//! Everything here paints with `theme::*` and sets type with the shared Space
//! Grotesk families — if a colour is ever needed that the palette does not
//! have, the palette is the file to change, not this one.

use eframe::egui::{self, Color32, FontId, Rect, Response, RichText, Sense, Ui, vec2};

use crate::theme;

/// The progress bar's empty track: `theme`'s `extreme_bg_color`, the tone the
/// app gives empty text fields. Not exposed as a `theme` function because
/// nothing else paints with it.
const TRACK: Color32 = Color32::from_rgb(0x0F, 0x16, 0x15);

/// Primary action: the accent fill with the badge ink knocked out of it —
/// `on_accent` on `accent` is the same pair the app's tab badges use, which is
/// what makes this read as Snor's own button rather than a general one.
///
/// The cursor is keyed off `contains_pointer`, not `hovered()`, for the reason
/// the app's `widgets.rs` documents at length: a press held past egui's click
/// window stops being "hovered", and the pointing hand would decay to an
/// arrow mid-press.
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let font = FontId::new(13.5, theme::medium());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, theme::on_accent());
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(36.0, 16.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if resp.is_pointer_button_down_on() {
            mix(theme::accent(), Color32::BLACK, 0.14)
        } else if resp.contains_pointer() {
            mix(theme::accent(), Color32::WHITE, 0.10)
        } else {
            theme::accent()
        };
        let pos = rect.center() - galley.size() * 0.5;
        let painter = ui.painter();
        painter.rect_filled(rect, 4.0, fill);
        painter.galley(pos, galley, theme::on_accent());
    }
    if resp.contains_pointer() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Secondary action: the themed button, with its ink pinned to `theme::text()`
/// so it does not ride on a default the palette never set.
pub fn ghost_button(ui: &mut Ui, label: &str) -> Response {
    ui.add(egui::Button::new(
        RichText::new(label).size(13.5).color(theme::text()),
    ))
}

/// A card is the recessed surface with the hairline border: the tone the
/// explorer and the terminal sit on, used here for the one slab of detail.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let width = (ui.available_width() - 28.0).max(120.0);
    egui::Frame::NONE
        .fill(theme::surface_recessed())
        .stroke(egui::Stroke::new(1.0, theme::hairline()))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_min_width(width);
            add(ui)
        })
        .inner
}
pub fn heading(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(21.0)
            .family(theme::medium())
            .color(theme::text_bright()),
    );
}

pub fn subtitle(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).color(theme::dim_text()));
}

pub fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(theme::faint()));
}

/// A fact with the accent dot in front — the welcome card's list, and the
/// same mark the app puts on the things it wants read as live.
pub fn bullet(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        if ui.is_rect_visible(dot) {
            ui.painter().circle_filled(dot.center(), 2.0, theme::accent());
        }
        ui.label(RichText::new(text).size(13.0).color(theme::dim_text()));
    });
}

/// How far along one stage is, drawn as the dot beside its label.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepState {
    Pending,
    Active,
    Done,
}

pub fn step_row(ui: &mut Ui, label: &str, state: StepState) {
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        if ui.is_rect_visible(dot) {
            let painter = ui.painter();
            let centre = dot.center();
            match state {
                StepState::Done => {
                    painter.circle_filled(centre, 3.5, theme::accent());
                }
                StepState::Active => {
                    painter.circle_stroke(centre, 4.0, egui::Stroke::new(1.5, theme::accent()));
                    painter.circle_filled(centre, 1.8, theme::accent());
                }
                StepState::Pending => {
                    painter.circle_stroke(centre, 3.5, egui::Stroke::new(1.0, theme::pane_edge()));
                }
            }
        }
        let colour = match state {
            StepState::Active => theme::text_strong(),
            StepState::Done => theme::dim_text(),
            StepState::Pending => theme::faint(),
        };
        ui.label(RichText::new(label).size(13.0).color(colour));
    });
}

/// The one moving part of the wizard: a slot with the accent filling it.
pub fn progress_bar(ui: &mut Ui, fraction: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, TRACK);
    let width = rect.width() * fraction.clamp(0.0, 1.0);
    if width > 0.5 {
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(width, rect.height())),
            3.0,
            theme::accent(),
        );
    }
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0, theme::hairline()),
        egui::StrokeKind::Inside,
    );
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let channel = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
    )
}