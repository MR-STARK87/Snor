from PIL import Image
LIVE = r"C:/My work folder/Snor/.workbuddy-ai/screenshots/live.png"
im = Image.open(LIVE).convert("RGB"); px = im.load()
W,H = im.size
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
def rm(y,x0,x1):
    xs=range(x0,x1,4); return sum(l(x,y) for x in xs)/len(list(xs))
print("live", im.size)
print("-- full-width rows 600..991 (x 700..1900) --")
for y in range(600,H):
    r = rm(y,700,1900)
    a = rm(y-5,700,1900); b = rm(y+5,700,1900)
    if r - (a+b)/2 > 1.0:
        print(y, round(r,2), "base", round((a+b)/2,2))
