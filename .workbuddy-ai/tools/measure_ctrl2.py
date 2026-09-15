from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
def cols(x0,x1,y0,y1,thr):
    return [x for x in range(x0,x1) if any(l(x,y)>thr for y in range(y0,y1))]
def rows(x0,x1,y0,y1,thr):
    return [y for y in range(y0,y1) if any(l(x,y)>thr for x in range(x0,x1))]
print("title-bar ink cols x 1350..1497, y 45..80 (thr 45):")
c = cols(1350,1497,45,80,45)
# group
runs=[]; s=c[0]; p=c[0]
for x in c[1:]:
    if x<=p+4: p=x
    else: runs.append((s,p)); s=x; p=x
runs.append((s,p))
print(runs)
for (a,b) in runs:
    print((a,b), "rows", rows(a,b+1,40,90,45), "px", px[(a+b)//2, 62])
