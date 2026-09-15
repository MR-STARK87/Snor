from PIL import Image

def groups(im, y0, y1, thr=70, gap=8):
    px = im.load(); W, _ = im.size
    def l(x, y):
        r, g, b = px[x, y]; return 0.2126*r + 0.7152*g + 0.0722*b
    cols = [x for x in range(W) if any(l(x, y) > thr for y in range(y0, y1))]
    if not cols: return []
    runs = []; s = cols[0]; p = cols[0]
    for x in cols[1:]:
        if x <= p + gap: p = x
        else: runs.append((s, p)); s = x; p = x
    runs.append((s, p))
    out = []
    for (a, b) in runs:
        ys = [y for y in range(y0, y1) if any(l(x, y) > thr for x in range(a, b+1))]
        out.append(((a, b), b-a+1, (ys[0], ys[-1])))
    return out

REF = r"C:\My work folder\Snor\file_00000000996c81faaa1c3fe9743de377.png"
LIVE = r"C:\My work folder\Snor\.workbuddy-ai\screenshots\live.png"

ref = Image.open(REF).convert("RGB")
live = Image.open(LIVE).convert("RGB")
# crop each to its window interior, then flip horizontally so "distance from the
# right edge" becomes plain x. That makes the two directly comparable.
ref = ref.crop((42, 30, 1497, 982)).transpose(Image.FLIP_LEFT_RIGHT)
live = live.crop((0, 0, 1920, 1020)).transpose(Image.FLIP_LEFT_RIGHT)

print("=== REFERENCE  (x = distance from right edge, physical px; /1.25 = pt) ===")
for (a, b), w, (ry0, ry1) in groups(ref, 10, 50):
    print(f"  x {a:4d}..{b:<4d}  w {w:3d}  rows {ry0:3d}..{ry1:<3d}  centre x={((a+b)/2):6.1f}px = {((a+b)/2)/1.25:5.1f}pt  h={ry1-ry0+1}px")

print()
print("=== LIVE ===")
for (a, b), w, (ry0, ry1) in groups(live, 8, 48):
    print(f"  x {a:4d}..{b:<4d}  w {w:3d}  rows {ry0:3d}..{ry1:<3d}  centre x={((a+b)/2):6.1f}px = {((a+b)/2)/1.25:5.1f}pt  h={ry1-ry0+1}px")
