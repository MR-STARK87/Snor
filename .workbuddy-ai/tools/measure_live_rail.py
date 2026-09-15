from PIL import Image
LIVE = r"C:/My work folder/Snor/.workbuddy-ai/screenshots/live.png"
im = Image.open(LIVE).convert("RGB"); px = im.load()
W,H = im.size
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
print("live", im.size)
print("bg samples:", [(x, px[x,300]) for x in (100, 400, 900, 1500, 1750, 1850, 1900)])

print("\n-- vertical rules x 1500..1916 over y 200..500 --")
for x in range(1500, W-4):
    c = sum(l(x,y) for y in range(200,500,3))/100
    a = sum(l(x-3,y) for y in range(200,500,3))/100
    b = sum(l(x+3,y) for y in range(200,500,3))/100
    if c - (a+b)/2 > 2.0:
        print(x, round(c,2), "base", round((a+b)/2,2))

print("\n-- ink columns x 1500..1919, y 150..900 --")
run=[(x, sum(1 for y in range(150,900) if l(x,y)>45)) for x in range(1500, W)]
run=[r for r in run if r[1]>40]
print("first:", run[0] if run else None, "last:", run[-1] if run else None)

print("\n-- rows with ink in x 1700..1919 --")
for y in range(100, H-60):
    c=sum(1 for x in range(1700,1919) if l(x,y)>45)
    if c>3: print(y, c)
