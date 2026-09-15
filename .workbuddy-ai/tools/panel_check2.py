import sys
from PIL import Image
for name in sys.argv[1:]:
    im = Image.open(rf"C:/My work folder/Snor/.workbuddy-ai/screenshots/{name}.png").convert("RGB")
    px = im.load()
    def l(x, y):
        r, g, b = px[x, y]; return 0.2126*r + 0.7152*g + 0.0722*b
    ink = sum(1 for y in range(150, 600) for x in range(0, 300) if l(x, y) > 70)
    # icon band: is the left band of the status-bar icon filled?
    # icon box is 20pt at 8pt margin -> physical x 10..35, y ~ 975..1005
    filled = sum(1 for y in range(985, 1000) for x in range(16, 22) if l(x, y) > 90)
    print(f"{name}: tree ink {ink:5d}   icon band bright px {filled}")
