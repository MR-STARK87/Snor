from PIL import Image

def stats(path, box, label, yband):
    im = Image.open(path).convert("RGB")
    if box: im = im.crop(box)
    px = im.load(); W, H = im.size
    def l(x, y):
        r, g, b = px[x, y]; return 0.2126*r + 0.7152*g + 0.0722*b
    # background = the modal (most common) luma in the band
    from collections import Counter
    c = Counter()
    for y in range(*yband):
        for x in range(0, W, 2):
            c[round(l(x, y))] += 1
    bg = c.most_common(1)[0][0]
    # brightest pixel in the whole band
    best = (0, None, None)
    for y in range(*yband):
        for x in range(W):
            v = l(x, y)
            if v > best[0]: best = (v, x, y, px[x, y])
    print(f"{label}: size {im.size}  bg luma {bg}  brightest {best[0]:.1f} at {best[1:]}")
    return bg

REF = r"C:\My work folder\Snor\file_00000000996c81faaa1c3fe9743de377.png"
LIVE = r"C:\My work folder\Snor\.workbuddy-ai\screenshots\live.png"
stats(REF, (42, 30, 1497, 982), "REF  band y10..50", (10, 50))
stats(LIVE, None, "LIVE band y8..48", (8, 48))

# now specifically the control glyphs: brightest ink in each control's box
def ctrl_ink(path, box, flipped, label, boxes):
    im = Image.open(path).convert("RGB")
    if box: im = im.crop(box)
    if flipped: im = im.transpose(Image.FLIP_LEFT_RIGHT)
    px = im.load()
    def l(x, y):
        r, g, b = px[x, y]; return 0.2126*r + 0.7152*g + 0.0722*b
    for name, (a, b) in boxes.items():
        best = max(((l(x, y), x, y, px[x, y]) for x in range(a, b+1) for y in range(8, 52)))
        print(f"  {label} {name}: peak luma {best[0]:.1f} at x={best[1]} y={best[2]} rgb={best[3]}")

print("\n-- control glyph ink --")
ctrl_ink(REF, (42, 30, 1497, 982), True, "REF ", {"close": (20, 55), "max": (70, 110), "min": (125, 165)})
ctrl_ink(LIVE, None, True, "LIVE", {"close": (20, 60), "max": (70, 115), "min": (125, 170)})
