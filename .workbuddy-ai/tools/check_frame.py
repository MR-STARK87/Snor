from PIL import Image
LIVE = r"C:/My work folder/Snor/.workbuddy-ai/screenshots/live.png"
im = Image.open(LIVE).convert("RGB"); px = im.load()
W,H = im.size
print("live", im.size)
print("top rows x=900:", [(y, px[900,y]) for y in range(0,5)])
print("left cols y=500:", [(x, px[x,500]) for x in range(0,5)])
print("right cols y=500:", [(x, px[x,500]) for x in range(W-5,W)])
print("bottom rows x=900:", [(y, px[900,y]) for y in range(H-5,H)])
# expected edge colour
print("expected edge #373E3A =", (0x37,0x3E,0x3A))
