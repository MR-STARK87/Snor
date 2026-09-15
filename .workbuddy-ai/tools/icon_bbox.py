import sys
from PIL import Image
name = sys.argv[1]
im = Image.open(rf"C:/My work folder/Snor/.workbuddy-ai/screenshots/{name}.png").convert("RGB")
px = im.load()
def l(x, y):
    r, g, b = px[x, y]; return 0.2126*r + 0.7152*g + 0.0722*b
xs = [x for x in range(0, 80) for y in range(960, 1020) if l(x, y) > 90]
ys = [y for y in range(960, 1020) for x in range(0, 80) if l(x, y) > 90]
print(f"{name}: icon ink bbox x {min(xs)}..{max(xs)}  y {min(ys)}..{max(ys)}")
print(f"   centre px ({(min(xs)+max(xs))/2}, {(min(ys)+max(ys))/2}) = pt ({(min(xs)+max(xs))/2/1.25:.1f}, {(min(ys)+max(ys))/2/1.25:.1f})")
