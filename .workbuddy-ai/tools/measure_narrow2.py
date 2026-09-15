from PIL import Image
im = Image.open(r"C:/My work folder/Snor/.workbuddy-ai/screenshots/narrow2.png").convert("RGB")
px = im.load()
def l(x, y):
    r, g, b = px[x, y]; return 0.2126*r + 0.7152*g + 0.0722*b
y0, y1 = 60, 120
cols = [x for x in range(0, 300) if any(l(x, y) > 70 for y in range(y0, y1))]
groups = []; s = p = cols[0]
for x in cols[1:]:
    if x <= p + 3: p = x
    else: groups.append((s, p)); s = x; p = x
groups.append((s, p))
print("header ink groups, gap<=3px  (px  |  pt = px/1.25)")
for (a, b) in groups:
    ys = [y for y in range(y0, y1) if any(l(x, y) > 70 for x in range(a, b+1))]
    print(f"  x {a:4d}..{b:<4d}  {a/1.25:6.1f}..{b/1.25:6.1f}pt  w {(b-a+1)/1.25:5.1f}pt  rows {ys[0]}..{ys[-1]}")
