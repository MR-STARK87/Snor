//! Snorri, the dozing moss-blob at the bottom of the Explorer.
//!
//! An original creature in the IDE's own line-art theme: a round, heavy body
//! slumped sideways in sleep, one paw tucked over its belly, small rounded
//! ears, closed content eyes, and a fading trail of sleep marks above. The
//! *vibe* is "big sleepy guy guarding the footer" — the shapes, proportions,
//! palette and face are all drawn from scratch for this app, not traced from
//! anything. No colour: every part is a stroked outline in [`theme::glyph`]
//! with the belly tufts in [`theme::moss`], so it reads as part of the chrome
//! rather than a sticker pasted onto it.
//!
//! Painted with the shared [`crate::icons`] helpers (`blob` / `blob_poly`),
//! back-to-front. Each shape is filled with the panel background (not left
//! transparent) so near parts occlude far ones the way painted art does —
//! transparent fills leave every hidden contour visible and the creature
//! reads as stacked circles instead of one body.

use eframe::egui::{Align2, Color32, FontId, Painter, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2};

use crate::icons::{blob, blob_poly};
use crate::theme;

/// Aspect ratio of the whole block: a wide slumped body plus a band of sleep
/// marks above it.
const ASPECT: f32 = 1.0 / 0.94;
/// Fraction of the block's height taken by the floating sleep marks.
const MARK_BAND: f32 = 0.22;

/// Height [`snorri`] will claim for a given width.
///
/// Exposed so the caller can reserve the footer's space *before* laying it
/// out — the block is bottom-pinned, so the scrolling tree above it has to
/// know what is coming.
pub fn height_for(width: f32) -> f32 {
    width / ASPECT
}

fn arc(painter: &Painter, center: eframe::egui::Pos2, w: f32, bow: f32, color: Color32, width: f32) {
    let mut pts = Vec::with_capacity(12);
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        let u = (t - 0.5) * 2.0;
        pts.push(pos2(
            center.x + w * (t - 0.5),
            center.y + bow * (1.0 - u * u),
        ));
    }
    painter.add(Shape::line(pts, Stroke::new(width, color)));
}

/// Paint the creature inside `rect`. `rect` should have been allocated by
/// [`snorri`]; calling this directly is only useful for tests.
///
/// Back-to-front: far ear, near ear, head, far paw flat behind, body mass,
/// near paw draped over the belly, then the face and belly ticks on top.
fn paint(painter: &Painter, rect: Rect) {
    let line = theme::glyph();
    let belly = theme::moss();
    // Opaque panel fill: each back-to-front shape covers the contours behind
    // it. Sampled live off the explorer panel; keep in sync with
    // `surface_recessed`.
    let hide = Color32::from_rgb(0x11, 0x18, 0x17);
    let stroke = Stroke::new(1.3, line);
    let faint = Stroke::new(1.1, line.gamma_multiply(0.55));
    // Normalised helper: (0,0) is the top-left of the drawing box.
    let p = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    let s = |v: f32| rect.width() * v;

    // Ears first, so the head covers their bases. Short and round — nubs,
    // not points.
    blob_poly(
        painter,
        &[p(0.30, 0.24), p(0.33, 0.06), p(0.44, 0.18)],
        s(0.05),
        hide,
        stroke,
    );
    blob_poly(
        painter,
        &[p(0.58, 0.17), p(0.65, 0.05), p(0.68, 0.25)],
        s(0.05),
        hide,
        stroke,
    );
    // Head, tipped sideways as it rests against the body.
    blob(painter, p(0.48, 0.36), s(0.28), s(0.23), -0.14, hide, stroke);
    // Far paw, flat on the ground behind everything.
    blob(painter, p(0.16, 0.70), s(0.15), s(0.21), 0.18, hide, stroke);
    // Body mass: the dominant slump.
    blob(painter, p(0.55, 0.72), s(0.30), s(0.23), -0.06, hide, stroke);
    // Near paw draped across the belly.
    blob(painter, p(0.50, 0.70), s(0.27), s(0.20), -0.10, hide, faint);
    // Foot poking out at the bottom right.
    blob(painter, p(0.80, 0.84), s(0.13), s(0.11), -0.20, hide, stroke);

    // Belly patch: a few short moss strokes suggesting fur tufts, not a
    // filled region. Colour is the only tint on the creature.
    for (tx, ty0, ty1) in [(0.46_f32, 0.66_f32, 0.72_f32), (0.54, 0.68, 0.74), (0.62, 0.66, 0.71)] {
        painter.add(Shape::line(
            vec![p(tx, ty0), p(tx, ty1)],
            Stroke::new(1.1, belly),
        ));
    }
    // Toe separations along the foot's lower-right edge. Short radial ticks:
    // arcs there read as a second pair of eyes.
    for (tx, ty0, ty1) in [(0.855_f32, 0.87_f32, 0.93_f32), (0.895, 0.85, 0.90)] {
        painter.add(Shape::line(
            vec![p(tx, ty0), p(tx, ty1)],
            Stroke::new(1.1, line),
        ));
    }
    // Closed, contented eyes, clear of the paw's top edge.
    arc(painter, p(0.38, 0.30), s(0.075), s(0.022), line, 1.5);
    arc(painter, p(0.55, 0.31), s(0.075), s(0.022), line, 1.5);
    // Sleepy mouth: one small downward bow under the left eye.
    arc(painter, p(0.44, 0.42), s(0.045), -s(0.016), line, 1.2);
}

/// Draw Snorri plus his floating sleep marks, and return the response for the
/// whole block so the caller can add a hover tooltip.
pub fn snorri(ui: &mut Ui, width: f32) -> Response {
    let height = width / ASPECT;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let painter = ui.painter_at(rect);
    let z_color = theme::moss().gamma_multiply(0.7);
    painter.text(
        pos2(rect.left() + width * 0.30, rect.top() + width * 0.08),
        Align2::CENTER_CENTER,
        "z",
        FontId::proportional(11.0),
        z_color,
    );
    painter.text(
        pos2(rect.left() + width * 0.60, rect.top() + width * 0.02),
        Align2::CENTER_CENTER,
        "Z",
        FontId::proportional(13.0),
        z_color,
    );
    let body = Rect::from_min_max(pos2(rect.left(), rect.top() + width * MARK_BAND), rect.max);
    paint(&painter, body);
    resp
}
