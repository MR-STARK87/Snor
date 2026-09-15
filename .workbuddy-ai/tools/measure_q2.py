from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
def rows(x0,x1,y0,y1,thr=45):
    out=[y for y in range(y0,y1) if any(l(x,y)>thr for x in range(x0,x1))]
    return (out[0],out[-1]) if out else None
def cols(x0,x1,y0,y1,thr=45):
    out=[x for x in range(x0,x1) if any(l(x,y)>thr for y in range(y0,y1))]
    return (out[0],out[-1]) if out else None
print("q2 line1 rows:", rows(1355,1470,845,884))
print("q2 line2 rows:", rows(1355,1470,885,925))
print("q2 line1 cols:", cols(1330,1492,850,884))
print("q2 line2 cols:", cols(1330,1492,885,925))
print("q2 dash rows (x1355..1400, y915..935):", rows(1355,1400,915,936,35))
print("q2 dash cols:", cols(1330,1492,928,934,35))
print("status bar top rule row:", [(y, round(sum(l(x,y) for x in range(700,1200,4))/125,1)) for y in range(925,940)])
print()
print("-- reference: title bar bottom / editor pane top --")
print("rule rows near 136:", [(y, round(sum(l(x,y) for x in range(400,1200,4))/200,1)) for y in range(130,145)])
print()
print("-- live rail: quote2 rows --")
LIVE = r"C:/My work folder/Snor/.workbuddy-ai/screenshots/live.png"
im2 = Image.open(LIVE).convert("RGB"); px2 = im2.load()
def l2(x,y):
    r,g,b = px2[x,y]; return 0.2126*r+0.7152*g+0.0722*b
def rows2(x0,x1,y0,y1,thr=45):
    out=[y for y in range(y0,y1) if any(l2(x,y)>thr for x in range(x0,x1))]
    return (out[0],out[-1]) if out else None
print("live q2 rows:", rows2(1720,1900,860,991))
print("live status top rule:", [(y, round(sum(l2(x,y) for x in range(700,1200,4))/125,1)) for y in range(940,965)])
