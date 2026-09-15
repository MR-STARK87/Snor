from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
def rm(y,x0,x1):
    xs=range(x0,x1,2); n=len(list(xs)); return sum(l(x,y) for x in xs)/n
print("-- x 700..1200 (no text) rows 600..700 --")
for y in range(600,700):
    print(y, round(rm(y,700,1200),2))
print("\n-- x 350..600 rows 600..700 --")
for y in range(600,700):
    print(y, round(rm(y,350,600),2))
print("\n-- x 60..280 (explorer) rows 600..700 --")
for y in range(600,700,2):
    print(y, round(rm(y,60,280),2))
print("\n-- vertical: where does the divider start? row 621/622 profile x 250..360 --")
for x in range(250, 370):
    print(x, round(l(x,621),2), round(l(x,622),2))
