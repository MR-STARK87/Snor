from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
cols=[x for x in range(900,1497) if any(l(x,y)>60 for y in range(48,72))]
runs=[]; s=cols[0]; p=cols[0]
for x in cols[1:]:
    if x<=p+8: p=x
    else: runs.append((s,p)); s=x; p=x
runs.append((s,p))
print("title-bar ink groups x900..1497:", runs)
# and the left side
cols2=[x for x in range(42,600) if any(l(x,y)>60 for y in range(48,72))]
runs2=[]; s=cols2[0]; p=cols2[0]
for x in cols2[1:]:
    if x<=p+8: p=x
    else: runs2.append((s,p)); s=x; p=x
runs2.append((s,p))
print("title-bar ink groups x42..600:", runs2)
