from PIL import Image

REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
LIVE = r"C:/My work folder/Snor/.workbuddy-ai/screenshots/live.png"
ref = Image.open(REF).convert("RGB").crop((42, 30, 1497, 982))
live = Image.open(LIVE).convert("RGB").crop((0, 0, 1920, 1020))

def l(px, x, y):
    r, g, b = px[x, y]; return 0.2126*r + 0.7152*g + 0.0722*b

for im, label, band in ((ref, "REF ", (10, 50)), (live, "LIVE", (8, 48))):
    px = im.load(); W, _ = im.size
    # leftmost ink column
    cols = [x for x in range(0, 400) if any(l(px, x, y) > 70 for y in range(*band))]
    print(f"{label}: leftmost ink at x={cols[0]}px = {cols[0]/1.25:.1f}pt from the window's left edge")
    # the dot: the small group between the wordmark and the tagline
    groups = []
    s = p = cols[0]
    for x in cols[1:]:
        if x <= p + 8: p = x
        else: groups.append((s, p)); s = x; p = x
    groups.append((s, p))
    for (a, b) in groups[:4]:
        ys = [y for y in range(*band) if any(l(px, x, y) > 70 for x in range(a, b+1))]
        peak = max((l(px, x, y), x, y) for x in range(a, b+1) for y in range(*band))
        print(f"   group x {a:4d}..{b:<4d} w {b-a+1:3d} rows {ys[0]}..{ys[-1]} h {ys[-1]-ys[0]+1:3d} peak luma {peak[0]:.0f}")
