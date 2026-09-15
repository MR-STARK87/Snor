import sys
from PIL import ImageGrab
out = sys.argv[1]
x0,y0,x1,y1 = (int(v) for v in sys.argv[2:6])
im = ImageGrab.grab(bbox=(x0,y0,x1,y1), all_screens=True)
im.save(out); print(out, im.size)
