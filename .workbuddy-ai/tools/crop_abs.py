import sys
from PIL import Image, ImageEnhance
src, dst = sys.argv[1], sys.argv[2]
x0,y0,x1,y1 = (int(v) for v in sys.argv[3:7])
z = float(sys.argv[7]) if len(sys.argv)>7 else 2.0
f = float(sys.argv[8]) if len(sys.argv)>8 else 1.0
im = Image.open(src).convert("RGB").crop((x0,y0,x1,y1))
if f != 1.0: im = ImageEnhance.Brightness(im).enhance(f)
im = im.resize((int(im.width*z), int(im.height*z)), Image.LANCZOS)
im.save(dst); print(dst, im.size)
