from PIL import Image
def l(px,x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b

def bbox(px, x0,y0,x1,y1, thr):
    xs=[]; ys=[]
    for y in range(y0,y1):
        for x in range(x0,x1):
            if l(px,x,y)>thr: xs.append(x); ys.append(y)
    if not xs: return None
    return (min(xs),min(ys),max(xs),max(ys))

REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
print("=== REFERENCE ===")
print("quote1 block:", bbox(px, 1330,430,1500,600, 45))
print("em-dash1    :", bbox(px, 1330,600,1500,640, 40))
print("quote2 block:", bbox(px, 1330,840,1500,930, 45))
print("em-dash2    :", bbox(px, 1330,930,1500,960, 40))
# per-line extents of quote 1
for (a,b) in [(451,470),(478,500),(503,530),(527,545),(552,565)]:
    print(" line", (a,b), bbox(px, 1330,a,1500,b, 45))

LIVE = r"C:/My work folder/Snor/.workbuddy-ai/screenshots/live.png"
im2 = Image.open(LIVE).convert("RGB"); px2 = im2.load()
print("\n=== LIVE ===")
print("quote1 block:", bbox(px2, 1600,350,1919,430, 45))
print("quote2 block:", bbox(px2, 1600,860,1919,930, 45))
for (a,b) in [(360,380),(380,400),(400,420)]:
    print(" line", (a,b), bbox(px2, 1600,a,1919,b, 45))
