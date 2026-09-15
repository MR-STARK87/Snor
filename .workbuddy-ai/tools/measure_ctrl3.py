from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
best={}
for (name,(x0,x1)) in {"min":(1340,1370),"max":(1395,1425),"close":(1450,1480)}.items():
    bx=None
    for x in range(x0,x1):
        for y in range(45,75):
            v=l(x,y)
            if bx is None or v>bx[0]: bx=(v,x,y,px[x,y])
    print(name, "brightest", bx)
# glyph bboxes at a moderate threshold
def bbox(x0,x1,thr=70):
    xs=[];ys=[]
    for x in range(x0,x1):
        for y in range(40,80):
            if l(x,y)>thr: xs.append(x); ys.append(y)
    return (min(xs),min(ys),max(xs),max(ys)) if xs else None
for name,(x0,x1) in {"min":(1330,1380),"max":(1390,1430),"close":(1445,1485)}.items():
    print(name, "bbox", bbox(x0,x1))
