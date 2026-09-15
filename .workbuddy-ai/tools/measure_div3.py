from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
def rm(y,x0,x1):
    xs=range(x0,x1,2); n=len(list(xs)); return sum(l(x,y) for x in xs)/n

print("-- full-width rows 620..960 (x 700..1200) --")
for y in range(620,960):
    r=rm(y,700,1200)
    if r > 26 or y<640: print(y, round(r,2))

print("\n-- rail rule x=1312 fine profile, y 540..700 --")
for y in range(540,700):
    nb=(l(1309,y)+l(1315,y))/2
    print(y, round(l(1312,y),2), round(nb,2), round(l(1312,y)-nb,2))
