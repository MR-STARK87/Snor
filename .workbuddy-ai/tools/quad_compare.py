"""Quadrant-by-quadrant comparison between the reference mock and the live app.

Looking at a whole 1500x950 window at once hides detail -- small icons, 11pt
labels and 1px hairlines all read as noise. This splits both images into the
same four quadrants and stacks each pair side by side, so one region can be
judged at a time.

The two windows are different sizes, so nothing is stretched to match: each
half is scaled to the same *displayed* width and the header records the real
pixel size of both, so a size difference stays visible instead of being
normalised away.

Regions are addressed relative to an edge: a negative x is measured from the
right, a negative y from the bottom. `0 0 340 220` therefore picks the top-left
340x220 of *both* images -- the explorer column in each -- regardless of the
fact that the two windows are different sizes.

Usage:
    quad_compare.py capture <out.png>    maximize Snor, grab the client area
    quad_compare.py compare <mine.png>   write quad1..quad4 comparison images
    quad_compare.py region <mine.png> x0 y0 x1 y1 <out.png> [zoom]
"""

import os
import sys
import time

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
REF_PATH = os.path.join(ROOT, "file_00000000996c81faaa1c3fe9743de377.png")

# The mock draws a window with a 1px border and a drop shadow. These are the
# interior bounds, found by scanning for that border on each side: left x=41,
# right x=1496, top y=29, bottom y=981.
REF_BOX = (42, 30, 1497, 982)

SHOT_DIR = os.path.join(ROOT, ".workbuddy-ai", "screenshots")
BG = (10, 14, 13)
GUTTER = 16
HEAD = 26
HALF_W = 1150  # displayed width of each half in the quadrant views

sys.path.insert(0, HERE)
import snor_ui_probe as probe  # noqa: E402


def capture(out):
    hwnd = probe.find_window()
    probe.user32.ShowWindow(hwnd, probe.SW_MAXIMIZE)
    probe.user32.SetForegroundWindow(hwnd)
    time.sleep(0.8)
    probe.cmd_shot(hwnd, out)
    probe.cmd_info(hwnd)


def load(mine_path):
    """Reference (cropped to its window) and the live capture, both raw."""
    ref = Image.open(REF_PATH).convert("RGB").crop(REF_BOX)
    live = Image.open(mine_path).convert("RGB")
    return ref, live


def resolve(box, size):
    """Edge-relative coords -> an absolute PIL box for an image of `size`."""
    w, h = size
    x0, y0, x1, y1 = box
    x0 = w + x0 if x0 < 0 else x0
    x1 = w + x1 if x1 < 0 else x1
    y0 = h + y0 if y0 < 0 else y0
    y1 = h + y1 if y1 < 0 else y1
    return (x0, y0, x1, y1)


def header(draw, zoom, text):
    draw.rectangle([0, 0, 10_000, int(HEAD * zoom)], fill=(22, 30, 28))
    draw.text((8, 6), text, fill=(196, 206, 196))


def main(argv):
    if len(argv) < 3:
        raise SystemExit(__doc__)
    mode, arg = argv[1], argv[2]
    os.makedirs(SHOT_DIR, exist_ok=True)

    if mode == "capture":
        capture(arg)
        return

    ref, live = load(arg)
    rw, rh = ref.size
    lw, lh = live.size

    if mode == "region":
        x0, y0, x1, y1 = (int(v) for v in argv[3:7])
        out_path = argv[7]
        zoom = float(argv[8]) if len(argv) > 8 else 2.5
        rb = resolve((x0, y0, x1, y1), (rw, rh))
        lb = resolve((x0, y0, x1, y1), (lw, lh))
        r = ref.crop(rb)
        m = live.crop(lb)
        out = Image.new("RGB", (r.width + m.width + GUTTER, max(r.height, m.height) + HEAD), BG)
        out.paste(r, (0, HEAD))
        out.paste(m, (r.width + GUTTER, HEAD))
        out = out.resize((int(out.width * zoom), int(out.height * zoom)), Image.LANCZOS)
        header(
            ImageDraw.Draw(out),
            zoom,
            f"region {x0},{y0}..{x1},{y1}   ref {r.width}x{r.height}px   live {m.width}x{m.height}px   LEFT=ref RIGHT=live",
        )
        out.save(out_path)
        print(f"{out_path}  {out.width}x{out.height}   (ref {r.width}x{r.height}, live {m.width}x{m.height})")
        return

    if mode != "compare":
        raise SystemExit(f"unknown mode {mode!r}\n{__doc__}")

    quads = {
        "1_topleft": (0.0, 0.0, 0.5, 0.5),
        "2_topright": (0.5, 0.0, 1.0, 0.5),
        "3_bottomleft": (0.0, 0.5, 0.5, 1.0),
        "4_bottomright": (0.5, 0.5, 1.0, 1.0),
    }
    print(f"reference window {rw}x{rh}px   live window {lw}x{lh}px")
    for name, (fx0, fy0, fx1, fy1) in quads.items():
        r = ref.crop((int(fx0 * rw), int(fy0 * rh), int(fx1 * rw), int(fy1 * rh)))
        m = live.crop((int(fx0 * lw), int(fy0 * lh), int(fx1 * lw), int(fy1 * lh)))
        rs, ms = r.size, m.size
        r = r.resize((HALF_W, int(r.height * HALF_W / r.width)), Image.LANCZOS)
        m = m.resize((HALF_W, int(m.height * HALF_W / m.width)), Image.LANCZOS)
        h = max(r.height, m.height)
        out = Image.new("RGB", (HALF_W * 2 + GUTTER, h + HEAD), BG)
        out.paste(r, (0, HEAD))
        out.paste(m, (HALF_W + GUTTER, HEAD))
        d = ImageDraw.Draw(out)
        header(
            d,
            1.0,
            f"Q{name[0]} {name[2:]}   LEFT=reference {rs[0]}x{rs[1]}px   RIGHT=live {ms[0]}x{ms[1]}px",
        )
        path = os.path.join(SHOT_DIR, f"quad{name}.png")
        out.save(path)
        print(f"  {path}  {out.width}x{out.height}  (ref half {rs[0]}x{rs[1]}, live half {ms[0]}x{ms[1]})")


if __name__ == "__main__":
    main(sys.argv)
