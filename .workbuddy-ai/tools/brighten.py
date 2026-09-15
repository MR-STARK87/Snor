import sys
from PIL import Image, ImageEnhance
src, dst, f = sys.argv[1], sys.argv[2], float(sys.argv[3]) if len(sys.argv)>3 else 2.4
im = Image.open(src).convert("RGB")
ImageEnhance.Brightness(im).enhance(f).save(dst)
print(dst, im.size)
