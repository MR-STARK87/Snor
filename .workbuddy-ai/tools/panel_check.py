from PIL import Image
for name in ("sb_before", "sb_after"):
    im = Image.open(rf"C:/My work folder/Snor/.workbuddy-ai/screenshots/{name}.png").convert("RGB")
    px = im.load()
    def l(x, y):
        r, g, b = px[x, y]; return 0.2126*r + 0.7152*g + 0.0722*b
    # explorer tree ink in x 0..300, y 150..600
    ink = sum(1 for y in range(150, 600) for x in range(0, 300) if l(x, y) > 70)
    print(f"{name}: tree ink pixels in x0..300 y150..600 = {ink}")
