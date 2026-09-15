from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b

# horizontal hairlines: rows where mean luma across x 100..1200 exceeds
# the mean of the rows 6 above and 6 below by >1.2
print("-- horizontal hairlines, y 100..1000 --")
for y in range(100, 1000):
    row = sum(l(x,y) for x in range(100,1200,4))/275
    up  = sum(l(x,y-6) for x in range(100,1200,4))/275
    dn  = sum(l(x,y+6) for x in range(100,1200,4))/275
    if row - (up+dn)/2 > 1.2:
        print(y, round(row,2), "base", round((up+dn)/2,2))

# vertical rules: columns where mean luma over y 200..500 beats neighbours
print("\n-- vertical rules, x 60..1500 (y 200..500) --")
for x in range(60, 1500):
    c = sum(l(x,y) for y in range(200,500,3))/100
    a = sum(l(x-3,y) for y in range(200,500,3))/100
    b = sum(l(x+3,y) for y in range(200,500,3))/100
    if c - (a+b)/2 > 2.5:
        print(x, round(c,2), "base", round((a+b)/2,2))
