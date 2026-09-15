"""Measure Snor's panel edge and split divider from a screenshot.

Both resize bugs came down to geometry being somewhere other than where it
looked, so verifying them means reading the pixels rather than trusting the
layout maths.

Chrome hairlines are lighter than the surfaces they separate, but a naive
"brightest column" scan is fooled by code text. Instead each candidate line is
scored by how many rows (or columns) it stays bright along, which is what
separates a full-height divider from a scattering of glyphs.

Usage:
    measure.py <shot.png> edge              # x of the explorer/editor edge
    measure.py <shot.png> divider [x]       # y of the editor/terminal divider
"""

import sys

from PIL import Image

# Above the panel fill (#141B1A, luma ~25) and below body text (luma > 100).
LINE_LUMA = 32.0
MARGIN = 40


def luma(p):
    return 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]


def edge_x(im):
    y0, y1 = MARGIN, im.height - MARGIN
    best, best_x = -1, -1
    for x in range(120, min(im.width, 1000)):
        hits = sum(1 for y in range(y0, y1, 3) if luma(im.getpixel((x, y))) > LINE_LUMA)
        if hits > best:
            best, best_x = hits, x
    return best_x, best


def divider_y(im, x0, x1):
    y0, y1 = MARGIN, im.height - MARGIN
    best, best_y = -1, -1
    for y in range(y0, y1):
        hits = sum(1 for x in range(x0, x1, 6) if luma(im.getpixel((x, y))) > LINE_LUMA)
        if hits > best:
            best, best_y = hits, y
    return best_y, best


def main(argv):
    im = Image.open(argv[1]).convert("RGB")
    what = argv[2] if len(argv) > 2 else "edge"
    if what == "edge":
        x, hits = edge_x(im)
        print(f"panel edge x = {x}px  ({x / 1.25:.1f} logical)  span={hits}")
    else:
        # Scan only the editor column so the explorer's rows can't win.
        x0 = int(argv[3]) if len(argv) > 3 else 600
        x1 = int(argv[4]) if len(argv) > 4 else 900
        y, hits = divider_y(im, x0, x1)
        print(f"divider y = {y}px  ({y / 1.25:.1f} logical)  span={hits}")


if __name__ == "__main__":
    main(sys.argv)
