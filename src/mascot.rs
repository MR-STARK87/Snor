//! The sleeping Snorlax that lives at the bottom of the Explorer.
//!
//! Drawn as vector line art rather than shipped as an image: the reference
//! design's mascot is a stroked outline with a fill barely lighter than the
//! panel behind it, and keeping it as geometry means it stays crisp at any DPI
//! and costs nothing to load. Every limb is filled before it is stroked so the
//! shapes occlude each other the way the reference art does.

use eframe::egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2};

use crate::icons::{blob, blob_poly};
use crate::theme;

/// Aspect ratio of the whole block: a wide body plus a band of sleep marks
/// above it.
const ASPECT: f32 = 1.0 / 0.94;
/// Fraction of the block's height taken by the floating sleep marks.
const MARK_BAND: f32 = 0.22;

/// Height [`snorlax`] will claim for a given width.
///
/// Exposed so the caller can reserve the footer's space *before* laying it
/// out — the block is bottom-pinned, so the scrolling tree above it has to
/// know what is coming.
pub fn height_for(width: f32) -> f32 {
    width / ASPECT
}

fn arc(painter: &Painter, center: Pos2, w: f32, bow: f32, color: Color32, width: f32) {
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

/// Paint the mascot inside `rect`. `rect` should have been allocated by
/// [`snorlax`]; calling this directly is only useful for tests.
///
/// Composed the way the reference art is: one silhouette built from a few
/// filled blobs, each drawn back-to-front so the near arm covers the head's
/// lower half and only the outer contours survive.
fn paint(painter: &Painter, rect: Rect) {
    let line = theme::outline();
    let fill = theme::mascot_fill();
    let stroke = Stroke::new(1.3, line);
    // Normalised helper: (0,0) is the top-left of the drawing box.
    let p = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    let s = |v: f32| rect.width() * v;

    // Ears first, so the head covers their bases.
    blob_poly(
        painter,
        &[p(0.22, 0.26), p(0.29, 0.00), p(0.45, 0.18)],
        s(0.05),
        fill,
        stroke,
    );
    blob_poly(
        painter,
        &[p(0.55, 0.17), p(0.69, 0.00), p(0.74, 0.27)],
        s(0.05),
        fill,
        stroke,
    );
    // Head, tipped slightly as it rests on the arm.
    blob(painter, p(0.46, 0.37), s(0.31), s(0.26), -0.10, fill, stroke);
    // Far arm, flat on the ground behind everything.
    blob(painter, p(0.17, 0.67), s(0.17), s(0.23), 0.16, fill, stroke);
    // Near arm draped across the belly: the dominant shape.
    blob(painter, p(0.55, 0.70), s(0.31), s(0.25), -0.05, fill, stroke);
    // Foot poking out at the bottom right.
    blob(painter, p(0.79, 0.83), s(0.14), s(0.12), -0.18, fill, stroke);

    // Toe separations along the foot's lower-right edge. Kept as short radial
    // ticks rather than arcs: arcs there read as a second pair of eyes.
    for (tx, ty0, ty1) in [(0.855_f32, 0.87_f32, 0.93_f32), (0.895, 0.85, 0.90)] {
        painter.add(Shape::line(
            vec![p(tx, ty0), p(tx, ty1)],
            Stroke::new(1.1, line),
        ));
    }
    // Closed, contented eyes, clear of the arm's top edge.
    arc(painter, p(0.35, 0.30), s(0.085), s(0.024), line, 1.5);
    arc(painter, p(0.53, 0.32), s(0.085), s(0.024), line, 1.5);
}

/// Draw the mascot plus its floating sleep marks, and return the response for
/// the whole block so the caller can add a hover tooltip.
pub fn snorlax(ui: &mut Ui, width: f32) -> Response {
    let height = width / ASPECT;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let painter = ui.painter_at(rect);
    let z_color = theme::sleep_mark();
    painter.text(
        pos2(rect.left() + width * 0.34, rect.top() + width * 0.08),
        Align2::CENTER_CENTER,
        "z",
        FontId::proportional(11.0),
        z_color,
    );
    painter.text(
        pos2(rect.left() + width * 0.62, rect.top() + width * 0.02),
        Align2::CENTER_CENTER,
        "Z",
        FontId::proportional(13.0),
        z_color,
    );
    let body = Rect::from_min_max(pos2(rect.left(), rect.top() + width * MARK_BAND), rect.max);
    paint(&painter, body);
    resp
}

/// Colours used by the mascot, exposed so tests can assert they stay distinct
/// from the panel behind them.
#[cfg(test)]
fn colors() -> (Color32, Color32) {
    (theme::outline(), theme::mascot_fill())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mascot_colors_separate_from_panel() {
        let (line, fill) = colors();
        assert_ne!(line, fill, "outline must be visible against the fill");
        // The outline has to be the brighter of the two or the drawing
        // disappears into the panel.
        let lum = |c: Color32| 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
        assert!(lum(line) > lum(fill) + 20.0, "outline too dim");
    }
}
