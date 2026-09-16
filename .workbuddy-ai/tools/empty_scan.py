"""Profile the empty-editor prompt column row by row.

The earlier scan used an absolute luma threshold of 45 and found nothing
between the buttons and the terminal header. `theme::faint()` is
rgb(0x5C,0x63,0x5E) -- luma ~97 -- so an absolute cut at 45 should have
caught it. This scan instead measures each pixel against the *modal*
colour of the band, which is the panel background, so a faint glyph at
luma 97 on a dark panel still registers.
"""
import sys
from collections import Counter
from PIL import Image

src = sys.argv[1] if len(sys.argv) > 1 else ".workbuddy-ai/screenshots/empty_state.png"
x0, x1 = (int(v) for v in (sys.argv[2:4] if len(sys.argv) > 3 else (200, 1910)))
y0, y1 = (int(v) for v in (sys.argv[4:6] if len(sys.argv) > 5 else (60, 760)))
delta = int(sys.argv[6]) if len(sys.argv) > 6 else 22

im = Image.open(src).convert("RGB")
px = im.load()

# Modal colour over the whole band = the panel background.
c = Counter()
for y in range(y0, y1, 3):
    for x in range(x0, x1, 3):
        c[px[x, y]] += 1
bg, bgn = c.most_common(1)[0]
print(f"band x{x0}..{x1} y{y0}..{y1}  bg={bg} ({bgn} samples)")
print(f"delta={delta}\n")

rows = []
for y in range(y0, y1):
    n = 0
    lo, hi = None, None
    for x in range(x0, x1):
        r, g, b = px[x, y]
        if abs(r - bg[0]) + abs(g - bg[1]) + abs(b - bg[2]) > delta:
            n += 1
            if lo is None:
                lo = x
            hi = x
    rows.append((y, n, lo, hi))

# Group consecutive inked rows into bands, with a 2-row gap tolerance.
groups = []
cur = None
for y, n, lo, hi in rows:
    if n:
        if cur is None:
            cur = [y, y, 0, lo, hi]
        cur[1] = y
        cur[2] += n
        cur[3] = min(cur[3], lo)
        cur[4] = max(cur[4], hi)
    elif cur is not None and y - cur[1] > 2:
        groups.append(cur)
        cur = None
if cur is not None:
    groups.append(cur)

print(f"{len(groups)} inked band(s):")
for gy0, gy1, n, lo, hi in groups:
    w = hi - lo + 1
    tag = "  <-- full-width rule" if w > (x1 - x0) * 0.5 else ""
    print(f"  y {gy0:4d}..{gy1:4d}  h={gy1-gy0+1:3d}  ink={n:6d}  x {lo:4d}..{hi:4d}  w={w:4d}{tag}")
