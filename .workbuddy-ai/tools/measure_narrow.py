from PIL import Image
im = Image.open(r"C:/My work folder/Snor/.workbuddy-ai/screenshots/narrow.png").convert("RGB")
px = im.load(); W, H = im.size
def l(x, y):
    r, g, b = px[x, y]; return 0.2126*r + 0.7152*g + 0.0722*b

# the explorer panel's right edge: scan x 150..600 for the divider column
print("-- panel right edge (column with a bright vertical run, y 150..900) --")
for x in range(150, 600):
    c = sum(1 for y in range(150, 900) if l(x, y) > 30)
    if c > 500:
        print("  candidate", x, c)

# header row band: find it
print("\n-- rows with ink in x 0..520 --")
rows = [y for y in range(60, 200) if sum(1 for x in range(0, 520) if l(x, y) > 70) > 2]
print("  ink rows", rows[0] if rows else None, "..", rows[-1] if rows else None)

y0, y1 = 60, 130
cols = [x for x in range(0, 520) if any(l(x, y) > 70 for y in range(y0, y1))]
groups = []; s = p = cols[0]
for x in cols[1:]:
    if x <= p + 8: p = x
    else: groups.append((s, p)); s = x; p = x
groups.append((s, p))
print("\n-- header ink groups (px, and pt = px/1.25) --")
for (a, b) in groups:
    print(f"  x {a:4d}..{b:<4d}  {a/1.25:6.1f}..{b/1.25:6.1f}pt  w {(b-a+1)/1.25:5.1f}pt")
