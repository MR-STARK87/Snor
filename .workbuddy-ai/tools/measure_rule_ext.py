from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
print("-- x=1312 vs neighbours, y 120..700 --")
for y in range(120, 700):
    nb=(l(1309,y)+l(1315,y))/2
    d=l(1312,y)-nb
    if y<180 or y>580 or d>8: print(y, round(l(1312,y),2), round(nb,2), round(d,2))
