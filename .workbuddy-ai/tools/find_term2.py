from PIL import Image
LIVE = r"C:/My work folder/Snor/.workbuddy-ai/screenshots/live.png"
im = Image.open(LIVE).convert("RGB"); px = im.load()
W,H = im.size
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
def rm(y,x0,x1):
    xs=list(range(x0,x1,4)); return sum(l(x,y) for x in xs)/len(xs)
print("-- live rows 540..660 (x 700..1900) --")
for y in range(540,660):
    print(y, round(rm(y,700,1900),2))
