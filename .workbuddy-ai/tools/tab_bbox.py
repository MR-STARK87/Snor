"""Locate the terminal tab strip's hit targets in a client-area screenshot.

Prints, in egui points, the ink bounding boxes of every glyph group on the
terminal header row. Used to aim probe clicks at the tab pill, the "+" button
and a tab's close cross instead of guessing -- a click that misses by a few
points reads as "the feature is broken" when it is only the aim that is off.

Two traps this script exists to avoid:

  * The explorer tree shares the header's rows to the left, so an unscoped row
    scan measures the file list and reports a 60px-tall "header".
  * The first terminal output line sits ~25px below the header with only a
    thin gap, so a symmetric +/- window around the hint merges the two.

The band is therefore found by taking the densest row near the hint and
expanding only while rows stay at least 30% as dense, which stops at that gap.

Usage:
    tab_bbox.py <shot.png> [x_start_phys] [y_hint_phys]
"""

import sys

from PIL import Image

path = sys.argv[1]
x_start = int(sys.argv[2]) if len(sys.argv) > 2 else 385
y_hint = int(sys.argv[3]) if len(sys.argv) > 3 else 623
im = Image.open(path).convert("RGB")
px = im.load()
W, H = im.size
SCALE = 1.25
THRESH = 70


def luma(x, y):
    r, g, b = px[x, y]
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def count(y):
    return sum(1 for x in range(x_start, W, 2) if luma(x, y) > THRESH)


lo = max(0, y_hint - 40)
hi = min(H, y_hint + 40)
counts = {y: count(y) for y in range(lo, hi)}
peak_y = max(counts, key=lambda y: counts[y])
peak = counts[peak_y]
if peak <= 3:
    raise SystemExit("no ink near the header hint in the terminal column")

floor = 3
y0 = peak_y
while y0 - 1 >= lo and counts[y0 - 1] >= floor:
    y0 -= 1
y1 = peak_y
while y1 + 1 < hi and counts[y1 + 1] >= floor:
    y1 += 1
y1 += 1  # half-open

print(f"image {W}x{H}  peak row {peak_y} ({peak} ink cols)")
print(f"header band {y0}..{y1} physical "
      f"({y0 / SCALE:.1f}..{y1 / SCALE:.1f}pt, "
      f"centre {(y0 + y1) / 2 / SCALE:.1f}pt)")

cols = [x for x in range(x_start, W) if any(luma(x, y) > THRESH for y in range(y0, y1))]
groups = []
start = prev = cols[0]
for x in cols[1:]:
    if x <= prev + 5:
        prev = x
    else:
        groups.append((start, prev))
        start = prev = x
groups.append((start, prev))

print(f"{len(groups)} ink group(s):")
for a, b in groups:
    ys = [y for y in range(y0, y1) if any(luma(x, y) > THRESH for x in range(a, b + 1))]
    cx = ((a + b) / 2) / SCALE
    cy = ((ys[0] + ys[-1]) / 2) / SCALE
    print(f"  x {a:4d}..{b:<4d} phys  w {(b - a + 1) / SCALE:5.1f}pt  "
          f"centre ({cx:6.1f}, {cy:5.1f})pt")
