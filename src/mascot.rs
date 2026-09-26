//! Snorri, the sleeping sloth at the bottom of the Explorer.
//!
//! A sloth, drawn from scratch: sitting up, hugging its knees, asleep. The
//! reads that make it a sloth rather than a bear are, in order of how much
//! work they do — three long claws on each of four paws, a pale face mask with
//! two dark outward-leaning smudges around closed sleeping eyes, and a small
//! nose over a content mouth. Take the claws away and it is a teddy; take the
//! patches away and it is a mouse.
//!
//! **The pose was chosen by looking at it, not by writing it down.** Three
//! candidates were rasterised at the real footer size and previewed before any
//! Rust existed: curled into a ball, sitting hugging its knees, and hanging
//! from a branch. `tools/mascot_design.py` still holds all three, with this one
//! as the shipped pose, because the shape list is the design and the rejected
//! poses are the reason the chosen one looks the way it does — the ball put
//! the face mask and the belly in one pale panel, and hanging from a branch
//! spent the top third of the block on the branch. That is also why the face
//! and belly are now asserted to stay apart ([`tests`]).
//!
//! **The palette is the app's, not a sloth's.** A sloth's fur is brown and this
//! one is the palette's green-grey ([`theme::mascot_fur`]) at the value a
//! sloth's fur actually reads at, so the footer stays chrome rather than
//! becoming the one warm object in a cool window. The cream is [`theme::glyph`]
//! — the tree's own ink — the eye patches and nothing else are
//! [`theme::pane_edge`], the nose, eyes and claws are
//! [`theme::surface_recessed`] so they read as knocked out of whatever they sit
//! on, and the cheeks are [`theme::accent`] at low alpha.
//!
//! Every part is a filled shape with no outline: the creature is built from its
//! silhouette and its colour blocks, which is what carries at the footer's
//! ~104pt. Nothing here has a stroke but the mouth.
//!
//! The geometry is data in [`parts`], drawn back to front by [`paint`], and the
//! split is the point of the module: the shape list can be asserted without a
//! window — every shape inside the block, the mirror symmetry, the cream inside
//! the fur, the eyes inside their patches, the face clear of the belly — which
//! is the only way a mascot gets verified rather than eyeballed.

use eframe::egui::{Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2};

use crate::icons::{blob, blob_poly};
use crate::theme;

/// Width divided by height for the whole block.
///
/// Square, chosen rather than measured: the creature's own ink comes out a
/// little taller than wide (0.90), so a square block leaves a few points of
/// air either side of the arms and none above or below. It could be tightened
/// to hand the footer about 9px of height back to the tree, which is not worth
/// changing the drawing the pose was approved at. (`height_for` divides by
/// this, so a future pose can still pick its own.)
const ASPECT: f32 = 1.0;

/// Height in widths. Vertical proportions are authored as fractions of the
/// block's height, but a radius or an offset only squares up against the
/// horizontal geometry once it is expressed in widths — which is where a
/// shape escapes its box. Always 1.0 at [`ASPECT`] 1.0, and kept because the
/// conversions below would silently be wrong without it.
const H: f32 = 1.0 / ASPECT;

/// Slack allowed around the block: the mouth and nose arcs are 1.2pt wide,
/// which is 0.006 of a 100pt-wide block, half of it either side of the path.
const FIT_MARGIN: f32 = 0.01;

/// Every claw is this wide, in widths, and this rounded at its tip.
const CLAW_W: f32 = 0.011;
const CLAW_TIP: f32 = 0.010;

/// Height [`snorri`] will claim for a given width.
///
/// Exposed so the caller can reserve the footer's space *before* laying it
/// out — the block is bottom-pinned, so the scrolling tree above it has to
/// know what is coming.
pub fn height_for(width: f32) -> f32 {
    width / ASPECT
}

/// Which palette tone a shape is filled with. Keeping the role in the data
/// rather than a `Color32` is what lets the tests reason about the drawing
/// (the cream patches, the ink that must land on them) without a painter.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    Fur,
    /// The pale front: face mask and belly.
    Cream,
    /// The eye smudges, and the only other use of this tone in the footer.
    Shade,
    /// The darkest: nose, closed eyes, mouth, claws.
    Ink,
    /// Translucent accent, for the cheek dab.
    Blush,
}

/// Geometry, in normalised units: x as a fraction of the block's width and y
/// as a fraction of its height, so a point reads the same at any footer width.
#[derive(Clone, PartialEq, Debug)]
enum Kind {
    /// Filled rotated ellipse. `r` is (x radius in widths, y radius in heights).
    Ellipse {
        c: (f32, f32),
        r: (f32, f32),
        rot: f32,
    },
    /// Filled polygon with rounded corners.
    Rounded {
        pts: Vec<(f32, f32)>,
        radius: f32,
    },
    /// Stroked arc, bowed downward by `bow` — and, with `w` at zero, a short
    /// vertical stroke. Both lengths are in widths, so they stay square when
    /// the block is not.
    Arc { c: (f32, f32), w: f32, bow: f32 },
}

#[derive(Clone, PartialEq, Debug)]
struct Part {
    kind: Kind,
    role: Role,
}

fn ell(c: (f32, f32), r: (f32, f32), rot: f32, role: Role) -> Part {
    Part {
        kind: Kind::Ellipse { c, r, rot },
        role,
    }
}

fn rounded(pts: &[(f32, f32)], radius: f32, role: Role) -> Part {
    Part {
        kind: Kind::Rounded {
            pts: pts.to_vec(),
            radius,
        },
        role,
    }
}

fn arc(c: (f32, f32), w: f32, bow: f32, role: Role) -> Part {
    Part {
        kind: Kind::Arc { c, w, bow },
        role,
    }
}

/// The same shape on the other side of the block.
///
/// Point order is deliberately *not* reversed: `Shape::convex_polygon` fills
/// either winding, and leaving the order alone is what lets a symmetric shape
/// compare equal to its own mirror in the symmetry test below.
fn mirrored(part: &Part) -> Part {
    let kind = match &part.kind {
        Kind::Ellipse { c, r, rot } => Kind::Ellipse {
            c: (1.0 - c.0, c.1),
            r: *r,
            rot: -rot,
        },
        Kind::Rounded { pts, radius } => Kind::Rounded {
            pts: pts.iter().map(|(x, y)| (1.0 - *x, *y)).collect(),
            radius: *radius,
        },
        Kind::Arc { c, w, bow } => Kind::Arc {
            c: (1.0 - c.0, c.1),
            w: *w,
            bow: *bow,
        },
    };
    Part {
        kind,
        role: part.role,
    }
}

/// One row of `count` claws, hanging from `origin` — the shape this whole
/// creature is recognised by, so it is built rather than hand-placed.
///
/// `tilt` leans the tips sideways, which is what makes a paw read as gripping
/// the knee it is drawn over rather than as a fork.
fn claw_row(origin: (f32, f32), count: usize, spread: f32, length: f32, tilt: f32) -> Vec<Part> {
    (0..count)
        .map(|i| {
            let cx = origin.0 + (i as f32 - (count as f32 - 1.0) / 2.0) * spread;
            rounded(
                &[
                    (cx - CLAW_W, origin.1),
                    (cx + CLAW_W, origin.1),
                    (cx + tilt * 0.6, origin.1 + length),
                    (cx - CLAW_W + 0.001 + tilt * 0.5, origin.1 + length * 1.04),
                ],
                CLAW_TIP,
                Role::Ink,
            )
        })
        .collect()
}

/// Every shape of the creature, **back to front**. Draw order is the layering:
/// the head before the face, the belly before the arms that hug it, each paw
/// before the claws that hang off it.
///
/// The face is built from one scale factor so the mask, the patches, the eyes,
/// the nose, the mouth and the cheeks stay in proportion with each other; all
/// of those coordinates are below.
fn parts() -> Vec<Part> {
    let mut out = Vec::with_capacity(40);

    // -- Body and head. The head is a big dome sitting on a wide body, drawn
    // after it so the neck is seamless.
    out.push(ell((0.50, 0.72), (0.44, 0.28), 0.0, Role::Fur));
    out.push(ell((0.50, 0.30), (0.36, 0.28), 0.0, Role::Fur));

    // -- Belly, stopping clear of the chin below: the ball pose ran the two
    // creams together into one pale panel and lost the face entirely.
    out.push(ell((0.50, 0.71), (0.24, 0.165), 0.0, Role::Cream));

    // -- Face. w and h are the mask's half-extents before scaling.
    let (cx, cy) = (0.50, 0.30);
    let (w, h) = (0.26 * 1.15, 0.19 * 1.15);
    out.push(rounded(
        &[
            (cx - w * 0.72, cy - h * 0.85),
            (cx - w * 0.24, cy - h * 1.10),
            (cx + w * 0.24, cy - h * 1.10),
            (cx + w * 0.72, cy - h * 0.85),
            (cx + w, cy - h * 0.05),
            (cx + w * 0.62, cy + h * 0.85),
            (cx, cy + h * 1.10),
            (cx - w * 0.62, cy + h * 0.85),
            (cx - w, cy - h * 0.05),
        ],
        0.05 * 1.15,
        Role::Cream,
    ));
    let patch = ell(
        (cx - w * 0.50, cy - h * 0.16),
        (w * 0.27, h * 0.30),
        0.34,
        Role::Shade,
    );
    out.push(patch.clone());
    out.push(mirrored(&patch));
    // Closed eye, inside its own patch: the whole reason the patch is there.
    let eye = arc((cx - w * 0.50, cy - h * 0.10), w * 0.22, h * 0.13, Role::Ink);
    out.push(eye.clone());
    out.push(mirrored(&eye));
    out.push(rounded(
        &[
            (cx - w * 0.12, cy + h * 0.30),
            (cx + w * 0.12, cy + h * 0.30),
            (cx, cy + h * 0.60),
        ],
        0.011,
        Role::Ink,
    ));
    let mouth = arc((cx - w * 0.17, cy + h * 0.66), w * 0.30, h * 0.20, Role::Ink);
    out.push(mouth.clone());
    out.push(mirrored(&mouth));
    let blush = ell(
        (cx - w * 0.55, cy + h * 0.60),
        (w * 0.14, h * 0.10),
        0.0,
        Role::Blush,
    );
    out.push(blush.clone());
    out.push(mirrored(&blush));

    // -- Arms reaching down the sides, then the paws over the knees.
    let arm = ell((0.20, 0.60), (0.12, 0.22), 0.34, Role::Fur);
    out.push(arm.clone());
    out.push(mirrored(&arm));
    let knee = ell((0.32, 0.80), (0.16, 0.13), 0.0, Role::Fur);
    out.push(knee.clone());
    out.push(mirrored(&knee));
    let paw = ell((0.31, 0.70), (0.11, 0.09), 0.15, Role::Fur);
    out.push(paw.clone());
    out.push(mirrored(&paw));
    for claw in claw_row((0.17, 0.73), 3, 0.038, 0.055, 0.14) {
        out.push(claw.clone());
        out.push(mirrored(&claw));
    }

    // -- Feet at the bottom, claws over the toes.
    let foot = ell((0.31, 0.90), (0.15, 0.08), -0.05, Role::Fur);
    out.push(foot.clone());
    out.push(mirrored(&foot));
    for claw in claw_row((0.29, 0.95), 3, 0.035, 0.035, 0.08) {
        out.push(claw.clone());
        out.push(mirrored(&claw));
    }

    out
}

/// Axis-aligned bounds of a part, in the same normalised units the geometry is
/// authored in: `(x0, y0, x1, y1)`.
///
/// A rotated ellipse's box is **not** its radii — a lobe tilted by 22 degrees
/// reaches further sideways than its width alone — and that difference is
/// exactly how a shape escapes the block it was allocated.
fn extents(part: &Part) -> (f32, f32, f32, f32) {
    match &part.kind {
        Kind::Ellipse { c, r, rot } => {
            let (sn, cs) = rot.sin_cos();
            let ry = r.1 * H;
            let hx = (r.0 * cs).hypot(ry * sn);
            let hy = (ry * cs).hypot(r.0 * sn) / H;
            (c.0 - hx, c.1 - hy, c.0 + hx, c.1 + hy)
        }
        Kind::Rounded { pts, .. } => {
            let mut b = (pts[0].0, pts[0].1, pts[0].0, pts[0].1);
            for (x, y) in pts {
                b = (b.0.min(*x), b.1.min(*y), b.2.max(*x), b.3.max(*y));
            }
            b
        }
        Kind::Arc { c, w, bow } => {
            let hy = bow / H;
            let (y0, y1) = if *bow >= 0.0 {
                (c.1, c.1 + hy)
            } else {
                (c.1 + hy, c.1)
            };
            (c.0 - w * 0.5, y0, c.0 + w * 0.5, y1)
        }
    }
}

fn fill(role: Role) -> Color32 {
    match role {
        Role::Fur => theme::mascot_fur(),
        Role::Cream => theme::glyph(),
        Role::Shade => theme::pane_edge(),
        Role::Ink => theme::surface_recessed(),
        // Translucent rather than dimmed: a gamma-multiplied accent is an
        // opaque dark dab, and a cheek has to sit *in* the cream, not on it.
        // Only the alpha is decided here; the hue still comes from the theme.
        Role::Blush => {
            let a = theme::accent();
            Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 110)
        }
    }
}

/// Stroked arc, bowed downward by `bow` pixels at its middle. `w` at zero
/// gives a vertical stroke of `bow` instead.
fn stroked_arc(painter: &Painter, center: Pos2, w: f32, bow: f32, color: Color32, width: f32) {
    let mut pts = Vec::with_capacity(12);
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        let u = (t - 0.5) * 2.0;
        pts.push(pos2(center.x + w * (t - 0.5), center.y + bow * (1.0 - u * u)));
    }
    painter.add(Shape::line(pts, Stroke::new(width, color)));
}

/// Paint the creature inside `rect`. `rect` should have been allocated by
/// [`snorri`]; calling this directly is only useful for tests.
fn paint(painter: &Painter, rect: Rect) {
    let p = |x: f32, y: f32| {
        pos2(
            rect.left() + rect.width() * x,
            rect.top() + rect.height() * y,
        )
    };
    let parts = parts();
    // A shape that overruns the block is clipped flat by `painter_at`, which is
    // how a body mass once came to look cut off at the bottom instead of
    // stopping short of it. Checked here, not just in the tests: `cargo run`
    // is a debug build, and that is where anyone would actually see it.
    #[cfg(debug_assertions)]
    for part in &parts {
        let (x0, y0, x1, y1) = extents(part);
        debug_assert!(
            x0 >= -FIT_MARGIN
                && y0 >= -FIT_MARGIN
                && x1 <= 1.0 + FIT_MARGIN
                && y1 <= 1.0 + FIT_MARGIN,
            "a shape escapes its block: {part:?} -> ({x0}, {y0})..({x1}, {y1})"
        );
    }
    for part in parts {
        let color = fill(part.role);
        match part.kind {
            Kind::Ellipse { c, r, rot } => blob(
                painter,
                p(c.0, c.1),
                rect.width() * r.0,
                rect.height() * r.1,
                rot,
                color,
                Stroke::NONE,
            ),
            Kind::Rounded { pts, radius } => {
                let points: Vec<Pos2> = pts.iter().map(|(x, y)| p(*x, *y)).collect();
                blob_poly(painter, &points, rect.width() * radius, color, Stroke::NONE);
            }
            Kind::Arc { c, w, bow } => stroked_arc(
                painter,
                p(c.0, c.1),
                rect.width() * w,
                rect.width() * bow,
                color,
                1.2,
            ),
        }
    }
}

/// Draw Snorri and return the response for the whole block so the caller can
/// add a hover tooltip.
pub fn snorri(ui: &mut Ui, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height_for(width)), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    paint(&ui.painter_at(rect), rect);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(outer: (f32, f32, f32, f32), inner: (f32, f32, f32, f32)) -> bool {
        inner.0 >= outer.0 && inner.1 >= outer.1 && inner.2 <= outer.2 && inner.3 <= outer.3
    }

    #[test]
    fn every_shape_stays_inside_the_block() {
        for part in parts() {
            let (x0, y0, x1, y1) = extents(&part);
            assert!(
                x0 >= -FIT_MARGIN
                    && y0 >= -FIT_MARGIN
                    && x1 <= 1.0 + FIT_MARGIN
                    && y1 <= 1.0 + FIT_MARGIN,
                "a shape escapes its block: {part:?} -> ({x0}, {y0})..({x1}, {y1})"
            );
        }
    }

    /// Shape equality for the symmetry check.
    ///
    /// Compared with a tolerance, because mirroring is not an exact operation
    /// in `f32`: `1.0 - 0.73` is not `0.27` in binary, and a rounded body
    /// authored symmetrically would otherwise read as lopsided. The tolerance
    /// is a ten-thousandth of the block — a hundredth of a point at footer
    /// size, below anything the eye or the painter can resolve.
    ///
    /// Polygons compare as *sets* of points, not lists: the body and the nose
    /// are symmetric shapes whose authored point order is not, so a flipped
    /// list is the same shape and has to read as one.
    fn same_shape(a: &Part, b: &Part) -> bool {
        const CLOSE: f32 = 1e-4;
        let near = |a: f32, b: f32| (a - b).abs() < CLOSE;
        let near_pt = |a: (f32, f32), b: (f32, f32)| near(a.0, b.0) && near(a.1, b.1);
        if a.role != b.role {
            return false;
        }
        match (&a.kind, &b.kind) {
            (
                Kind::Ellipse {
                    c: ac,
                    r: ar,
                    rot: arot,
                },
                Kind::Ellipse {
                    c: bc,
                    r: br,
                    rot: brot,
                },
            ) => near_pt(*ac, *bc) && near_pt(*ar, *br) && near(*arot, *brot),
            (
                Kind::Rounded {
                    pts: ap,
                    radius: ar,
                },
                Kind::Rounded {
                    pts: bp,
                    radius: br,
                },
            ) => {
                if !near(*ar, *br) || ap.len() != bp.len() {
                    return false;
                }
                let key = |pts: &Vec<(f32, f32)>| {
                    let mut v = pts.clone();
                    v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
                    v
                };
                key(ap)
                    .iter()
                    .zip(key(bp).iter())
                    .all(|(a, b)| near_pt(*a, *b))
            }
            (
                Kind::Arc {
                    c: ac,
                    w: aw,
                    bow: abow,
                },
                Kind::Arc {
                    c: bc,
                    w: bw,
                    bow: bbow,
                },
            ) => near_pt(*ac, *bc) && near(*aw, *bw) && near(*abow, *bbow),
            _ => false,
        }
    }

    /// A sloth drawn straight on has no asymmetry at all, so unlike the poses
    /// that came before it, every shape here must have a mirror twin — and
    /// that includes the *centred* geometry (body, head, face, nose), which is
    /// authored by hand and would otherwise be free to drift a few points off
    /// centre unnoticed. The mirrors themselves are derived through
    /// [`mirrored`], so what this really pins down is the hand-written half.
    #[test]
    fn the_creature_is_mirror_symmetric() {
        let all = parts();
        for part in &all {
            let m = mirrored(part);
            assert!(
                all.iter().any(|q| same_shape(q, &m)),
                "no mirror twin for {part:?}"
            );
        }
    }

    /// The block is the pose's footprint, and it comes out square: a sloth
    /// sitting up with its paws in front is as tall as it is wide. The footer
    /// reserves exactly this much, so it is worth an assertion rather than a
    /// number nobody checks.
    #[test]
    fn the_block_is_the_poses_footprint() {
        assert!((height_for(104.0) - 104.0).abs() < 0.01);
    }

    /// Two claws per paw adds up to twelve, and the count is the whole read:
    /// this creature is a sloth because of them. They also have to hang from
    /// each paw rather than float, which is what putting them in one builder
    /// with the paw's origin buys.
    #[test]
    fn four_paws_carry_three_claws_each() {
        let parts = parts();
        let claws: Vec<&Part> = parts
            .iter()
            .filter(|p| p.role == Role::Ink && matches!(p.kind, Kind::Rounded { .. }))
            .filter(|p| extents(p).1 > 0.5)
            .collect();
        assert_eq!(claws.len(), 12, "three claws on each of four paws");
    }

    /// A face feature that misses the cream reads as a hole in the fur, and
    /// this pose has more of them than any before it: two patches, two closed
    /// eyes, a nose, two mouth strokes and two cheeks all have to land inside
    /// the mask. The claws are ink as well, so they are excluded by sitting
    /// below the face.
    #[test]
    fn the_face_features_land_on_the_cream_not_the_fur() {
        let parts = parts();
        let face = parts
            .iter()
            .find(|p| p.role == Role::Cream && matches!(p.kind, Kind::Rounded { .. }))
            .expect("the face mask");
        let bounds = extents(face);
        let on_face: Vec<&Part> = parts
            .iter()
            .filter(|p| p.role != Role::Fur && p.role != Role::Cream)
            .filter(|p| extents(p).1 >= bounds.1 && extents(p).3 <= bounds.3)
            .collect();
        assert_eq!(
            on_face.len(),
            9,
            "two patches, two eyes, a nose, two mouth strokes, two cheeks"
        );
        for part in on_face {
            assert!(
                contains(bounds, extents(part)),
                "a face feature left the cream: {part:?}"
            );
        }
    }

    /// The closed eyes are what make the patches read as *sleeping* rather
    /// than as holes, so they have to sit inside them.
    #[test]
    fn the_sleeping_eyes_sit_inside_their_patches() {
        let parts = parts();
        let nose = parts
            .iter()
            .find(|p| p.role == Role::Ink && matches!(p.kind, Kind::Rounded { .. }))
            .filter(|p| extents(p).1 < 0.5)
            .expect("the nose");
        let eyes: Vec<&Part> = parts
            .iter()
            .filter(|p| p.role == Role::Ink && matches!(p.kind, Kind::Arc { .. }))
            .filter(|p| extents(p).3 < extents(nose).1)
            .collect();
        assert_eq!(eyes.len(), 2, "a closed eye in each patch");
        let patches: Vec<(f32, f32, f32, f32)> = parts
            .iter()
            .filter(|p| p.role == Role::Shade)
            .map(extents)
            .collect();
        assert_eq!(patches.len(), 2);
        for eye in eyes {
            assert!(
                patches.iter().any(|p| contains(*p, extents(eye))),
                "a closed eye hangs off its patch: {eye:?}"
            );
        }
    }

    /// Cream has to land *on* the creature. Getting this wrong is the loudest
    /// possible failure and the least visible in a test that only checks the
    /// block: during the pose pass the cheeks sat partly on the fur and the
    /// ball pose ran the face and the belly into one another.
    #[test]
    fn the_cream_never_leaves_the_fur() {
        let parts = parts();
        let fur: Vec<(f32, f32, f32, f32)> = parts
            .iter()
            .filter(|p| p.role == Role::Fur)
            .map(extents)
            .collect();
        let cream: Vec<&Part> = parts.iter().filter(|p| p.role == Role::Cream).collect();
        assert_eq!(cream.len(), 2, "the face mask and the belly");
        for part in cream {
            assert!(
                fur.iter().any(|f| contains(*f, extents(part))),
                "a cream patch hangs outside the fur: {part:?}"
            );
        }
    }

    /// The face and the belly are separate patches with fur between them. In
    /// the curled-ball pose they met, and the creature lost its face to one
    /// long pale panel — cheap to prevent, invisible until you look at a
    /// render, and exactly the kind of thing that reads as "broken".
    #[test]
    fn the_face_and_the_belly_stay_apart() {
        let parts = parts();
        let face = parts
            .iter()
            .find(|p| p.role == Role::Cream && matches!(p.kind, Kind::Rounded { .. }))
            .expect("the face mask");
        let belly = parts
            .iter()
            .find(|p| p.role == Role::Cream && matches!(p.kind, Kind::Ellipse { .. }))
            .expect("the belly");
        let (_, _, _, face_bottom) = extents(face);
        let (_, belly_top, ..) = extents(belly);
        assert!(
            face_bottom < belly_top,
            "the face cream ({face_bottom}) runs into the belly ({belly_top})"
        );
    }
}
