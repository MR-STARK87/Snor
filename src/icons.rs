//! Hand-painted line-art icons.
//!
//! The reference design draws its chrome with stroked outlines rather than font
//! glyphs, and egui's bundled fonts have no dependable icon coverage — a
//! character that is missing renders as a tofu box. Every icon here is
//! therefore painted with the painter, and each one fits inside the `Rect` it
//! is handed (no bleed past the allocation).

use eframe::egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    pos2, vec2,
};

/// Number of segments used to approximate a curve. 24 keeps circles smooth at
/// the ~14pt sizes used here without measurable cost.
const CURVE_STEPS: usize = 24;

/// Points of an ellipse, closed. `rot` is in radians.
pub fn ellipse_points(center: Pos2, rx: f32, ry: f32, rot: f32) -> Vec<Pos2> {
    let (sn, cs) = rot.sin_cos();
    (0..CURVE_STEPS)
        .map(|i| {
            let a = i as f32 / CURVE_STEPS as f32 * std::f32::consts::TAU;
            let (x, y) = (a.cos() * rx, a.sin() * ry);
            pos2(center.x + x * cs - y * sn, center.y + x * sn + y * cs)
        })
        .collect()
}

/// A closed polygon whose corners are rounded by `radius` (quadratic arcs).
/// Used for the folder, document and Snorlax's ears, which are all
/// rounded-corner outlines rather than true ellipses.
pub fn rounded_poly(points: &[Pos2], radius: f32) -> Vec<Pos2> {
    let n = points.len();
    let mut out = Vec::with_capacity(n * 8);
    for i in 0..n {
        let prev = points[(i + n - 1) % n];
        let cur = points[i];
        let next = points[(i + 1) % n];
        let to_prev = prev - cur;
        let to_next = next - cur;
        let r = radius
            .min(to_prev.length() * 0.5)
            .min(to_next.length() * 0.5);
        let p_in = cur + to_prev.normalized() * r;
        let p_out = cur + to_next.normalized() * r;
        for s in 0..=4 {
            let t = s as f32 / 4.0;
            // de Casteljau: keeps every intermediate on `Pos2` instead of
            // mixing `Pos2`/`Vec2` arithmetic, which egui types strictly.
            let l1 = p_in + (cur - p_in) * t;
            let l2 = cur + (p_out - cur) * t;
            out.push(l1 + (l2 - l1) * t);
        }
    }
    out
}

fn fill_stroke(painter: &Painter, points: Vec<Pos2>, fill: Color32, stroke: Stroke) {
    painter.add(Shape::convex_polygon(points, fill, stroke));
}

/// Filled ellipse with an outline.
pub fn blob(painter: &Painter, center: Pos2, rx: f32, ry: f32, rot: f32, fill: Color32, stroke: Stroke) {
    fill_stroke(painter, ellipse_points(center, rx, ry, rot), fill, stroke);
}

/// Filled rounded polygon with an outline.
pub fn blob_poly(painter: &Painter, points: &[Pos2], radius: f32, fill: Color32, stroke: Stroke) {
    fill_stroke(painter, rounded_poly(points, radius), fill, stroke);
}

/// Disclosure chevron. `openness` 0.0 points right (collapsed), 1.0 points down.
pub fn chevron(painter: &Painter, rect: Rect, openness: f32, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5;
    let angle = openness.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    let (sn, cs) = angle.sin_cos();
    let rot = |p: Pos2| {
        let v = p - c;
        pos2(c.x + v.x * cs - v.y * sn, c.y + v.x * sn + v.y * cs)
    };
    let pts = vec![
        rot(pos2(c.x - s * 0.36, c.y - s * 0.68)),
        rot(pos2(c.x + s * 0.34, c.y)),
        rot(pos2(c.x - s * 0.36, c.y + s * 0.68)),
    ];
    painter.add(Shape::line(pts, Stroke::new(1.4, color)));
}

/// Outlined folder, matching the reference's tree rows.
pub fn folder(painter: &Painter, rect: Rect, color: Color32) {
    let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let (w, h) = (rect.width(), rect.height());
    let pts = vec![
        pos2(x0, y1),
        pos2(x0, y0 + h * 0.30),
        pos2(x0 + w * 0.42, y0 + h * 0.30),
        pos2(x0 + w * 0.52, y0 + h * 0.08),
        pos2(x1, y0 + h * 0.08),
        pos2(x1, y1),
    ];
    painter.add(Shape::closed_line(
        rounded_poly(&pts, w * 0.12),
        Stroke::new(1.2, color),
    ));
}

/// Open folder: the silhouette of [`folder`] with its front flap tipped
/// forward, the conventional "choose a folder" glyph.
///
/// The back panel is drawn as an *open* path so the flap reads as depth rather
/// than as a second rectangle stacked behind the first, and both shapes start
/// and end on the bottom corners so the left edge is only stroked once.
pub fn folder_open(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let (w, h) = (rect.width(), rect.height());
    // Back panel: up the left side, across the tab, then down the right side
    // only as far as the flap's hinge.
    painter.add(Shape::line(
        vec![
            pos2(x0, y1),
            pos2(x0, y0 + h * 0.30),
            pos2(x0 + w * 0.40, y0 + h * 0.30),
            pos2(x0 + w * 0.50, y0 + h * 0.08),
            pos2(x1, y0 + h * 0.08),
            pos2(x1, y0 + h * 0.42),
        ],
        stroke,
    ));
    // Front flap, hinged on that line and splayed towards the viewer.
    let flap = vec![
        pos2(x0, y1),
        pos2(x0 + w * 0.18, y0 + h * 0.42),
        pos2(x1, y0 + h * 0.42),
        pos2(x1 - w * 0.18, y1),
    ];
    painter.add(Shape::closed_line(rounded_poly(&flap, w * 0.08), stroke));
}

/// Outlined document with a folded corner and text rules.
///
/// Stroked at 1.0 rather than the folder's 1.2: the reference draws the page
/// noticeably finer than the folder beside it, and at a 17px-wide glyph the
/// heavier weight closes the interior up into a solid block.
pub fn doc(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.0, color);
    let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let fold = rect.width() * 0.34;
    let pts = vec![
        pos2(x0, y0),
        pos2(x1 - fold, y0),
        pos2(x1, y0 + fold),
        pos2(x1, y1),
        pos2(x0, y1),
    ];
    painter.add(Shape::closed_line(rounded_poly(&pts, 1.2), stroke));
    // The folded corner itself.
    painter.add(Shape::line(
        vec![pos2(x1 - fold, y0), pos2(x1 - fold, y0 + fold), pos2(x1, y0 + fold)],
        stroke,
    ));
    let rule = Stroke::new(1.0, color);
    for k in [0.52_f32, 0.72] {
        let y = y0 + rect.height() * k;
        painter.add(Shape::line(
            vec![pos2(x0 + 2.5, y), pos2(x1 - 2.5, y)],
            rule,
        ));
    }
}

/// Filled letter badge: the editor tabs, and the *selected* explorer row,
/// where the reference swaps the outlined badge for a solid accent one with
/// the letter knocked out.
///
/// `font_size` is explicit rather than derived from `rect` because the two
/// callers draw the same glyph in noticeably different boxes (16pt tabs,
/// 13.6pt tree rows) and each was sized against its own reference ink.
pub fn badge_filled(
    painter: &Painter,
    rect: Rect,
    letter: &str,
    font_size: f32,
    fill: Color32,
    fg: Color32,
) {
    painter.rect_filled(rect, 3.0, fill);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        letter,
        FontId::monospace(font_size),
        fg,
    );
}

pub fn close_x(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.3);
    let stroke = Stroke::new(1.3, color);
    painter.add(Shape::line(vec![r.left_top(), r.right_bottom()], stroke));
    painter.add(Shape::line(vec![r.right_top(), r.left_bottom()], stroke));
}

pub fn plus(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.3);
    let stroke = Stroke::new(1.3, color);
    let c = r.center();
    painter.add(Shape::line(vec![pos2(r.left(), c.y), pos2(r.right(), c.y)], stroke));
    painter.add(Shape::line(vec![pos2(c.x, r.top()), pos2(c.x, r.bottom())], stroke));
}

pub fn minus(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.3);
    let c = r.center();
    painter.add(Shape::line(
        vec![pos2(r.left(), c.y), pos2(r.right(), c.y)],
        Stroke::new(1.3, color),
    ));
}

/// Trash can, used for the terminal's clear action.
pub fn trash(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let w = rect.width();
    // Lid + handle.
    painter.add(Shape::line(
        vec![pos2(x0, y0 + rect.height() * 0.22), pos2(x1, y0 + rect.height() * 0.22)],
        stroke,
    ));
    painter.add(Shape::line(
        vec![
            pos2(x0 + w * 0.34, y0 + rect.height() * 0.10),
            pos2(x0 + w * 0.66, y0 + rect.height() * 0.10),
        ],
        stroke,
    ));
    // Tapered body.
    painter.add(Shape::closed_line(
        vec![
            pos2(x0 + w * 0.14, y0 + rect.height() * 0.22),
            pos2(x0 + w * 0.24, y1),
            pos2(x0 + w * 0.76, y1),
            pos2(x0 + w * 0.86, y0 + rect.height() * 0.22),
        ],
        stroke,
    ));
}

/// Maximize / restore glyph for the terminal header. Takes the colour third so
/// it fits the [`icon_button`] callback shape.
pub fn maximize(painter: &Painter, rect: Rect, color: Color32, fullscreen: bool) {
    let stroke = Stroke::new(1.4, color);
    let inner = rect.shrink(rect.width() * 0.22);
    if fullscreen {
        let back = Rect::from_min_size(inner.min + inner.size() * 0.32, inner.size() * 0.68);
        let front = Rect::from_min_size(inner.min, inner.size() * 0.68);
        painter.rect_stroke(front, 1.0, stroke, StrokeKind::Middle);
        painter.add(Shape::line(
            vec![back.right_top(), back.right_bottom(), back.left_bottom()],
            stroke,
        ));
    } else {
        painter.rect_stroke(inner, 1.0, stroke, StrokeKind::Middle);
    }
}

/// Terminal-panel toggle for the status bar: a rounded rectangle whose bottom
/// band is filled while the panel is open, hollow while it is hidden.
///
/// The reference puts a single icon at the far right of its status bar (a
/// settings gear, for a panel we do not have). This fills that slot with the
/// one control ours actually has, rather than leaving a default-chrome text
/// button sitting among the readouts.
pub fn panel_bottom(painter: &Painter, rect: Rect, color: Color32, open: bool) {
    painter.rect_stroke(rect, 2.0, Stroke::new(1.2, color), StrokeKind::Middle);
    let band = Rect::from_min_max(
        pos2(rect.left() + 1.5, rect.bottom() - rect.height() * 0.38),
        pos2(rect.right() - 1.5, rect.bottom() - 1.5),
    );
    if open {
        painter.rect_filled(band, 1.0, color);
    } else {
        painter.rect_stroke(band, 1.0, Stroke::new(1.0, color), StrokeKind::Middle);
    }
}

/// Sidebar-panel toggle for the status bar: [`panel_bottom`] turned on its
/// side, with the band down the left edge instead of across the bottom.
///
/// It sits at the far left of the status bar, mirroring the terminal's toggle
/// at the far right, so the two panel toggles read as a pair. Unlike the
/// terminal's, this one has to work in both directions from the same place:
/// the explorer header disappears with the panel, so the status bar is the
/// only control left once it is hidden.
pub fn panel_left(painter: &Painter, rect: Rect, color: Color32, open: bool) {
    painter.rect_stroke(rect, 2.0, Stroke::new(1.2, color), StrokeKind::Middle);
    let band = Rect::from_min_max(
        pos2(rect.left() + 1.5, rect.top() + 1.5),
        pos2(rect.left() + rect.width() * 0.38, rect.bottom() - 1.5),
    );
    if open {
        painter.rect_filled(band, 1.0, color);
    } else {
        painter.rect_stroke(band, 1.0, Stroke::new(1.0, color), StrokeKind::Middle);
    }
}

/// Play triangle for the Run button.
pub fn play(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5;
    let pts = vec![
        pos2(c.x - s * 0.42, c.y - s * 0.62),
        pos2(c.x + s * 0.55, c.y),
        pos2(c.x - s * 0.42, c.y + s * 0.62),
    ];
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}

/// Git branch glyph for the status bar.
pub fn branch(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let dot = |p: Pos2| Shape::circle_stroke(p, rect.width() * 0.14, stroke);
    let trunk_x = x0 + rect.width() * 0.26;
    painter.add(Shape::line(
        vec![pos2(trunk_x, y0 + rect.height() * 0.18), pos2(trunk_x, y1 - rect.height() * 0.18)],
        stroke,
    ));
    // Fork curving off to the right.
    painter.add(Shape::line(
        rounded_poly(
            &[
                pos2(trunk_x, y0 + rect.height() * 0.52),
                pos2(x1 - rect.width() * 0.26, y0 + rect.height() * 0.52),
                pos2(x1 - rect.width() * 0.26, y1 - rect.height() * 0.18),
            ],
            rect.width() * 0.18,
        ),
        stroke,
    ));
    painter.add(dot(pos2(trunk_x, y0 + rect.height() * 0.18)));
    painter.add(dot(pos2(trunk_x, y1 - rect.height() * 0.18)));
    painter.add(dot(pos2(x1 - rect.width() * 0.26, y1 - rect.height() * 0.18)));
}

/// Outlined triangle, the reference's "changed lines" marker.
pub fn triangle_outline(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5;
    let pts = vec![
        pos2(c.x, c.y - s * 0.8),
        pos2(c.x + s * 0.85, c.y + s * 0.6),
        pos2(c.x - s * 0.85, c.y + s * 0.6),
    ];
    painter.add(Shape::closed_line(
        rounded_poly(&pts, s * 0.22),
        Stroke::new(1.2, color),
    ));
}

/// Two-lobed leaf for the title bar's tagline.
pub fn leaf(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5;
    // Half-length and half-width of the blade. The blade is a lens: pointed at
    // both ends, widest in the middle, which is what makes it read as a leaf
    // rather than as a second dot beside the tagline.
    let (len, wide) = (s * 0.94, s * 0.50);
    // Local (u, v) -> screen, with +u running up and to the right and +v
    // perpendicular to it. Kept as a closure so every point below is written
    // in blade coordinates instead of pre-rotated by hand.
    const R: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let at = |u: f32, v: f32| pos2(c.x + (u + v) * R, c.y + (-u + v) * R);

    let steps = CURVE_STEPS;
    let mut blade = Vec::with_capacity(2 * steps + 2);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let u = len * (2.0 * t - 1.0);
        blade.push(at(u, -wide * (std::f32::consts::PI * t).sin()));
    }
    for i in (0..=steps).rev() {
        let t = i as f32 / steps as f32;
        let u = len * (2.0 * t - 1.0);
        blade.push(at(u, wide * (std::f32::consts::PI * t).sin()));
    }
    painter.add(Shape::convex_polygon(blade, color, Stroke::NONE));

    // Stem: the blade's lower-left point carried on out of the box.
    painter.add(Shape::line(
        vec![at(-len, 0.0), at(-len - s * 0.36, 0.0)],
        Stroke::new(1.1, color),
    ));
    // Vein, knocked out of the fill rather than drawn on top of the panel, so
    // the icon stays a single self-contained shape.
    painter.add(Shape::line(
        vec![at(-len * 0.55, 0.0), at(len * 0.70, 0.0)],
        Stroke::new(1.0, color.gamma_multiply(0.30)),
    ));
}

/// Circular arrow for "rescan the tree".
pub fn refresh(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5 * 0.72;
    let stroke = Stroke::new(1.3, color);
    let pts: Vec<Pos2> = (0..=20)
        .map(|i| {
            let a = std::f32::consts::TAU * (i as f32 / 20.0) * 0.78 - 0.6;
            pos2(c.x + a.cos() * s, c.y + a.sin() * s)
        })
        .collect();
    // Arrow head at the open end of the arc.
    let tip = pts[pts.len() - 1];
    painter.add(Shape::line(pts, stroke));
    painter.add(Shape::line(
        vec![
            tip + vec2(-s * 0.05, -s * 0.55),
            tip,
            tip + vec2(s * 0.55, -s * 0.1),
        ],
        stroke,
    ));
}

/// Shorten `text` with a trailing ellipsis so it fits `max_w` in `font`.
/// Cheap by design: one layout to measure, one estimate for the cut.
pub fn truncate(painter: &Painter, text: &str, font: FontId, max_w: f32) -> String {
    let w = painter
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
        .size()
        .x;
    if w <= max_w || text.is_empty() {
        return text.to_owned();
    }
    let n = text.chars().count();
    let keep = ((max_w / (w / n as f32)).floor() as usize).saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    out.push('…');
    out
}

/// A flat, square icon button. `paint` receives the icon rect and the colour it
/// should use; the hover background is handled here so every icon button in the
/// app looks the same.
pub fn icon_button(
    ui: &mut Ui,
    size: f32,
    tooltip: &str,
    paint: impl FnOnce(&Painter, Rect, Color32),
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        if resp.hovered() {
            painter.rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
        }
        let color = if resp.hovered() {
            crate::theme::text()
        } else {
            crate::theme::dim_text()
        };
        paint(&painter, rect, color);
    }
    resp.on_hover_text(tooltip)
}
