from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
# title bar interior is y 30..75 (ref box top 30, rule at ~76?). find the row band
print("-- title bar rows: mean luma x 700..1400 --")
for y in range(28, 90):
    xs=list(range(700,1400,4)); print(y, round(sum(l(x,y) for x in xs)/len(xs),2))
