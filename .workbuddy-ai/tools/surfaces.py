from PIL import Image
from collections import Counter

REF = r"C:\My work folder\Snor\file_00000000996c81faaa1c3fe9743de377.png"
ref = Image.open(REF).convert("RGB"); px = ref.load()

def modal_row(y, x0, x1):
    c = Counter(px[x, y] for x in range(x0, x1, 2))
    return c.most_common(1)[0][0]

def luma(t):
    return round(0.2126*t[0] + 0.7152*t[1] + 0.0722*t[2], 1)

print("row | explorer(x60..300)      | editor(x330..1290)")
prev_e = prev_d = None
for y in range(30, 210):
    e = modal_row(y, 60, 300)
    d = modal_row(y, 330, 1290)
    mark = ""
    if e != prev_e: mark += "  <- explorer changes"
    if d != prev_d: mark += "  <- editor changes"
    if mark or y < 40:
        print(f"{y:4d} | {e} l={luma(e):5} | {d} l={luma(d):5}{mark}")
    prev_e, prev_d = e, d
print()
print("-- further down --")
for y in [300, 400, 500, 600, 620, 630, 700, 800, 900, 920, 925, 928, 930, 935, 940, 950, 960, 970]:
    e = modal_row(y, 60, 300)
    d = modal_row(y, 330, 1290)
    print(f"{y:4d} | explorer {e} l={luma(e):5} | editor {d} l={luma(d):5}")
