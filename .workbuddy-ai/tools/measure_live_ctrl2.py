from PIL import Image
LIVE = r"C:/My work folder/Snor/.workbuddy-ai/screenshots/live.png"
im = Image.open(LIVE).convert("RGB"); px = im.load()
W,H = im.size
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
# title bar = first 57px. Text row centred.
print("-- rows 0..60 mean luma x 600..1400 --")
for y in range(0,60,2):
    xs=list(range(600,1400,4)); print(y, round(sum(l(x,y) for x in xs)/len(xs),2))
print("\n-- ink groups x 1400..1599, y 20..50 (thr 70) --")
cols=[x for x in range(1400,W) if any(l(x,y)>70 for y in range(20,50))]
runs=[]; s=cols[0]; p=cols[0]
for x in cols[1:]:
    if x<=p+6: p=x
    else: runs.append((s,p)); s=x; p=x
runs.append((s,p))
for (a,b) in runs:
    ys=[y for y in range(10,60) if any(l(x,y)>70 for x in range(a,b+1))]
    print((a,b), "w", b-a+1, "rows", ys[0], ys[-1], "centre", ((a+b)/2, (ys[0]+ys[-1])/2))
print("\nwindow right edge px:", W-1, " (points", (W-1)/1.25, ")")
