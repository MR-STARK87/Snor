from PIL import Image
LIVE = r"C:/My work folder/Snor/.workbuddy-ai/screenshots/live.png"
im = Image.open(LIVE).convert("RGB"); px = im.load()
W,H = im.size
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
print("live", im.size, "-> points", W/1.25, H/1.25)
# title bar row: find the band with ink
print("\n-- title bar ink groups, x 0..W, y 40..90 (thr 70) --")
cols=[x for x in range(0,W) if any(l(x,y)>70 for y in range(40,90))]
runs=[]; s=cols[0]; p=cols[0]
for x in cols[1:]:
    if x<=p+8: p=x
    else: runs.append((s,p)); s=x; p=x
runs.append((s,p))
print(runs)
for (a,b) in runs:
    ys=[y for y in range(30,100) if any(l(x,y)>70 for x in range(a,b+1))]
    print((a,b), "w", b-a+1, "rows", ys[0], ys[-1])
print("\nwindow right edge px:", W-1)
