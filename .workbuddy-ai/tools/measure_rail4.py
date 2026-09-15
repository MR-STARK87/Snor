from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
def rowmean(y, x0, x1, step=2):
    xs = range(x0,x1,step); n=len(list(xs))
    return sum(l(x,y) for x in range(x0,x1,step))/n

print("-- rows in the EMPTY rail strip x 1350..1480, y 140..700 --")
for y in range(140, 700):
    r = rowmean(y,1350,1480)
    a = rowmean(y-5,1350,1480); b = rowmean(y+5,1350,1480)
    if r - (a+b)/2 > 1.0:
        print(y, round(r,2), "base", round((a+b)/2,2))

print("\n-- rows in the EMPTY editor strip x 1150..1300, y 140..700 --")
for y in range(140, 700):
    r = rowmean(y,1150,1300)
    a = rowmean(y-5,1150,1300); b = rowmean(y+5,1150,1300)
    if r - (a+b)/2 > 1.0:
        print(y, round(r,2), "base", round((a+b)/2,2))
