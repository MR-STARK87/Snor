import sys
from PIL import Image, ImageEnhance
src, dst = sys.argv[1], sys.argv[2]
f = float(sys.argv[3]) if len(sys.argv)>3 else 1.0
im = Image.open(src).convert("RGB")
if f != 1.0: im = ImageEnhance.Brightness(im).enhance(f)
im = im.resize((1080, int(im.height*1080/im.width)), Image.LANCZOS)
im.save(dst); print(dst, im.size)
