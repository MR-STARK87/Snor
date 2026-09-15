"""Print the distinct ink colours down the reference mock's tree glyph columns.

Used to decide the tree's palette: which cream the glyphs are drawn in, and
whether the *selected* row tints its chevron/glyph with the accent or leaves
them cream. Sampling a whole column strip at once is deliberate -- eyeballing
one pixel is how you mistake an antialiased diagonal for a second ink.

    python sample_tree_ink.py [x0] [x1] [y0] [y1]
"""

import sys
from collections import Counter

from PIL import Image

REF = r"C:\My work folder\Snor\file_00000000996c81faaa1c3fe9743de377.png"
# The mock's window interior, found by scanning for its 1px border.
BOX = (42, 30, 1497, 982)


def main():
    x0, x1, y0, y1 = (int(v) for v in (sys.argv[1:5] or (48, 120, 30, 420)))
    img = Image.open(REF).convert("RGB")
    print(f"reference {img.size}, interior {BOX}, strip x{x0}..{x1} y{y0}..{y1}")

    # Per scan-line: the brightest pixel in the strip. For a tree row that is
    # the peak of whichever glyph the line crosses, which is what we want --
    # the row's baseline colour rather than an average of ink and panel.
    for y in range(y0, y1):
        row = [img.getpixel((x, y)) for x in range(x0, x1)]
        peak = max(row, key=lambda c: sum(c))
        lum = sum(peak) / 3.0
        if lum > 60:
            # Where in the strip the peak sat, so chevron/folder/badge columns
            # can be told apart.
            x = x0 + max(range(len(row)), key=lambda i: sum(row[i]))
            print(f"y={y:4d} x={x:4d} peak={peak} lum={lum:6.1f}")

    print("\ndistinct colours above luma 90 (count, colour):")
    c = Counter()
    for y in range(y0, y1):
        for x in range(x0, x1):
            p = img.getpixel((x, y))
            if sum(p) / 3.0 > 90:
                c[p] += 1
    for col, n in c.most_common(12):
        print(f"  {n:5d}  {col}")


if __name__ == "__main__":
    main()
