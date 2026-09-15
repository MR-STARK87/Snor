import sys
from PIL import Image, ImageEnhance
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
x0,y0,x1,y1 = (int(v) for v in sys.argv[1:5])
out = sys.argv[5]; z = float(sys.argv[6]) if len(sys.argv)>6 else 4.0
f = float(sys.argv[7]) if len(sys.argv)>7 else 1.0
im = Image.open(REF).convert("RGB").crop((x0,y0,x1,y1))
if f != 1.0: im = ImageEnhance.Brightness(im).enhance(f)
im = im.resize((int(im.width*z), int(im.height*z)), Image.LANCZOS)
im.save(out); print(out, im.size)
