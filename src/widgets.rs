//! Small shared widget helpers.
//!
//! These exist because the plain egui building blocks each get one detail
//! wrong for how Snor uses them, and the details are not obvious from the call
//! site. Every rule below was a bug first.

use eframe::egui;

/// A text label that behaves like a button — a tab, a link, a prompt.
///
/// Three separate things have to be true at once, and each was its own bug:
///
/// * `Sense::click()`, so it is clickable at all.
/// * `selectable(false)`. `Label::ui` otherwise resolves `selectable` from
///   `style.interaction.selectable_labels` (true by default) and hands the
///   label to egui's text-selection machinery. That machinery writes
///   `CursorIcon::Text` twice: once from `Label::ui`, and again from
///   `LabelSelectionState::on_end_pass` while it is dragging. The end-of-pass
///   write lands *after* anything the call site asks for, so a terminal tab
///   hovered as a pointing hand and showed the I-beam the whole time the
///   button was held — reported as "when we tap on terminal tabs the copy
///   cursor shows". Opting out of selection removes the write instead of
///   racing it, and a tab genuinely is not selectable text.
/// * The cursor is keyed off `contains_pointer`, not `hovered()`.
///   `Response::on_hover_cursor` only asks for the icon while `hovered()`, and
///   `hovered()` goes false as soon as the gesture stops being a click: a
///   widget that senses clicks but not drags has its press abandoned once it
///   is held past `InputOptions::max_click_duration` (0.6s by default), and
///   `is_pointer_button_down_on` goes false with it. Measured on the tab
///   strip: holding a tab showed a hand for ~0.5s and then an arrow, which
///   reads as a glitch. `contains_pointer` asks only whether the pointer is
///   over the label and nothing is covering it — which is the whole of what
///   "this is a button" means — so the icon holds for as long as the pointer
///   is there, press or no press.
///
/// The returned `Response` is the label's, so callers can chain
/// `on_hover_text` or read `clicked()` as usual.
pub fn clickable_label(ui: &mut egui::Ui, text: egui::RichText) -> egui::Response {
    let resp = ui.add(
        egui::Label::new(text)
            .sense(egui::Sense::click())
            .selectable(false),
    );
    if resp.contains_pointer() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Padding and face for a prompt button, shared by the measuring pass and the
/// painting pass so the two cannot drift apart.
const PROMPT_PAD_X: f32 = 16.0;
const PROMPT_PAD_Y: f32 = 7.0;
const PROMPT_FONT: f32 = 13.0;

/// Width a prompt button will occupy, so a row of them can be centred before
/// any of them has been laid out.
pub fn prompt_button_width(ui: &egui::Ui, label: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(PROMPT_FONT),
        egui::Color32::WHITE,
    );
    PROMPT_PAD_X * 2.0 + galley.size().x
}

/// Height of a prompt button. Both take this so the pair line up.
pub fn prompt_button_height(ui: &egui::Ui) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        "X".to_owned(),
        egui::FontId::proportional(PROMPT_FONT),
        egui::Color32::WHITE,
    );
    PROMPT_PAD_Y * 2.0 + galley.size().y
}

/// One action in an empty state, painted into a rect the caller has already
/// reserved.
///
/// The rect is passed in rather than allocated here because buttons are
/// centred as a *pair*. Laying them out with
/// `Layout::left_to_right(Align::Center)` looks like the obvious way to do
/// that, but egui sizes such a frame to the whole available height
/// (`Layout::next_frame_ignore_wrap`: a horizontal layout whose vertical align
/// is `Center` fills the height) and then `Placer::advance_after_rects`
/// expands the parent to the whole frame. The row therefore sank to the middle
/// of the pane and swallowed the space the tertiary link below it needed, so
/// the link was laid out past the bottom and clipped away. Reserving one rect
/// and dividing it keeps the row's height exactly the button height.
///
/// `primary` fills with the accent and knocks the label out of it; the other is
/// outlined. That asymmetry is the point — the two are a recommendation and an
/// alternative, not two equal choices.
///
/// Lives here rather than in the editor because the first-run screen draws the
/// same pair, and two implementations of one button is how the two drift.
pub fn prompt_button_at(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    label: &str,
    primary: bool,
) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(label), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        let color = if primary {
            crate::theme::on_accent()
        } else {
            crate::theme::text()
        };
        let fill = if primary {
            if resp.hovered() {
                crate::theme::accent().gamma_multiply(1.18)
            } else {
                crate::theme::accent()
            }
        } else if resp.hovered() {
            egui::Color32::from_rgb(0x25, 0x38, 0x2E)
        } else {
            crate::theme::tab_active()
        };
        painter.rect_filled(rect, 7.0, fill);
        if !primary {
            painter.rect_stroke(
                rect,
                7.0,
                egui::Stroke::new(1.0, crate::theme::hairline()),
                egui::StrokeKind::Middle,
            );
        }
        let galley = painter.layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(PROMPT_FONT),
            color,
        );
        painter.galley(rect.center() - galley.size() * 0.5, galley, color);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(time: f64, events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        }
    }

    /// Hold the primary button on the widget for `hold_secs` and report the
    /// cursor egui asked for on the last frame.
    ///
    /// `build` is the widget under test, so the same harness can measure the
    /// helper and a bare `Label` side by side — which is the whole point,
    /// since the difference between them only shows up while a press is held.
    ///
    /// The hover point is read off the widget's own rect rather than guessed:
    /// a `Label`'s rect is its galley, and aiming a few points off lands in
    /// padding where every cursor assertion quietly measures nothing.
    fn cursor_while_held(
        build: impl Fn(&mut egui::Ui) -> egui::Response,
        hold_secs: f64,
    ) -> egui::CursorIcon {
        let ctx = egui::Context::default();
        let mut at = egui::Pos2::ZERO;

        // Frame 0: the label exists, the pointer is elsewhere. egui hit-tests
        // each frame against the *previous* frame's widget rects, so a widget
        // cannot be hovered on the frame it first appears.
        ctx.run_ui(raw(0.0, vec![]), |ui| {
            at = build(ui).rect.center();
        })
        .drop_without_applying_deltas();
        assert!(at != egui::Pos2::ZERO, "test widget has an empty rect");

        // Frame 1: the pointer arrives on the label and presses.
        let events = vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        ctx.run_ui(raw(0.1, events), |ui| {
            build(ui);
        })
        .drop_without_applying_deltas();

        // Then hold without moving. Past `InputOptions::max_click_duration`
        // (0.8s by default) egui stops treating the press as a click, and
        // `hovered()` goes false with it.
        let mut t = 0.1;
        let mut icon = egui::CursorIcon::Default;
        while t < 0.1 + hold_secs {
            t += 0.3;
            let out = ctx.run_ui(raw(t, vec![]), |ui| {
                build(ui);
            });
            icon = out.platform_output.cursor_icon;
            out.drop_without_applying_deltas();
        }
        icon
    }

    /// The helper is a hand for as long as the pointer is on the label, press
    /// or no press.
    #[test]
    fn clickable_label_keeps_the_hand_through_a_long_press() {
        let icon = cursor_while_held(
            |ui| clickable_label(ui, egui::RichText::new("tab")),
            2.0,
        );
        assert_eq!(
            icon,
            egui::CursorIcon::PointingHand,
            "holding a tab must not fall back to the default arrow"
        );
    }

    /// The contrast that justifies the helper. A bare clickable `Label` with
    /// `on_hover_cursor` is a hand while hovered and something else once the
    /// press outlives the click window — verified live on the tab strip, which
    /// showed a hand for ~0.5s and then an arrow.
    ///
    /// If this ever starts passing as `PointingHand`, egui has changed and the
    /// `contains_pointer` workaround can probably go.
    #[test]
    fn a_bare_hover_cursor_gives_up_during_a_long_press() {
        let icon = cursor_while_held(
            |ui| {
                ui.add(egui::Label::new("tab").sense(egui::Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
            },
            2.0,
        );
        assert_ne!(
            icon,
            egui::CursorIcon::PointingHand,
            "egui now holds the cursor itself; `clickable_label` may no longer need to"
        );
    }

    /// And the helper does not claim the cursor from somewhere else on screen,
    /// so the two tests above are not passing for free.
    #[test]
    fn clickable_label_leaves_the_cursor_alone_away_from_the_label() {
        let ctx = egui::Context::default();
        ctx.run_ui(raw(0.0, vec![]), |ui| {
            clickable_label(ui, egui::RichText::new("tab"));
        })
        .drop_without_applying_deltas();

        let far = egui::pos2(300.0, 150.0);
        let out = ctx.run_ui(raw(0.1, vec![egui::Event::PointerMoved(far)]), |ui| {
            clickable_label(ui, egui::RichText::new("tab"));
        });
        let icon = out.platform_output.cursor_icon;
        out.drop_without_applying_deltas();

        assert_ne!(
            icon,
            egui::CursorIcon::PointingHand,
            "a label must not claim the cursor from across the window"
        );
    }
}
