from PIL import Image
import sys

REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB")
W, H = im.size
px = im.load()
print("ref size", im.size)

# 1) locate the vertical rule: scan columns 1280..1340, count rows in y 150..930
#    where the column is darker than its neighbourhood but not pure background.
def col_luma(x, y0, y1):
    s = 0; n = 0
    for y in range(y0, y1):
        r, g, b = px[x, y]
        s += 0.2126*r + 0.7152*g + 0.0722*b; n += 1
    return s / n

print("\n-- columns 1290..1330 mean luma over y 150..930 --")
for x in range(1290, 1331):
    print(x, round(col_luma(x, 150, 930), 2))
