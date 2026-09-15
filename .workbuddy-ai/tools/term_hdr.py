from PIL import Image
im = Image.open(r"C:/My work folder/Snor/.workbuddy-ai/screenshots/t1b.png").convert("RGB")
px = im.load(); W, H = im.size
def l(x, y):
    r, g, b = px[x, y]; return 0.2126*r + 0.7152*g + 0.0722*b
# find the terminal header row: the row with the "Terminal" pill, near y 600-680
best = None
for y in range(560, 720):
    c = sum(1 for x in range(0, W, 2) if l(x, y) > 70)
    if best is None or c > best[1]: best = (y, c)
print("densest header row:", best)
y0 = best[0] - 22
y1 = best[0] + 22
cols = [x for x in range(W - 400, W) if any(l(x, y) > 70 for y in range(y0, y1))]
groups = []; s = p = cols[0]
for x in cols[1:]:
    if x <= p + 6: p = x
    else: groups.append((s, p)); s = x; p = x
groups.append((s, p))
print(f"right-side ink groups in y {y0}..{y1}  (window W={W})")
for (a, b) in groups:
    ys = [y for y in range(y0, y1) if any(l(x, y) > 70 for x in range(a, b+1))]
    print(f"  x {a:4d}..{b:<4d}  centre {((a+b)/2)/1.25:7.1f}pt  w {(b-a+1)/1.25:5.1f}pt  y centre {((ys[0]+ys[-1])/2)/1.25:.1f}pt")
