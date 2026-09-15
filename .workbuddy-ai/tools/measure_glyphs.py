from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
def rows_with_ink(x0,x1,y0,y1,thr=45):
    out=[]
    for y in range(y0,y1):
        if any(l(x,y)>thr for x in range(x0,x1)): out.append(y)
    return (out[0],out[-1]) if out else None
def cols_with_ink(x0,x1,y0,y1,thr=45):
    out=[]
    for x in range(x0,x1):
        if any(l(x,y)>thr for y in range(y0,y1)): out.append(x)
    return (out[0],out[-1]) if out else None

print("G of Good (x1360..1380):", rows_with_ink(1360,1380,440,475))
print("d of Good (x1390..1400):", rows_with_ink(1390,1400,440,475))
print("f of software (asc+desc) x1365..1385:", rows_with_ink(1365,1385,478,505))
print("line1 cols:", cols_with_ink(1330,1492,445,475))
print("line2 cols:", cols_with_ink(1330,1492,478,500))
print("line3 cols:", cols_with_ink(1330,1492,503,530))
print("line4 cols:", cols_with_ink(1330,1492,527,550))
print("line5 cols:", cols_with_ink(1330,1492,552,572))
print("em-dash rows (x1355..1400):", rows_with_ink(1355,1400,575,600,35))
print("em-dash cols (y578..592):", cols_with_ink(1330,1492,578,592,35))
print()
print("quote2 line1 cols:", cols_with_ink(1330,1492,860,885))
print("quote2 line2 cols:", cols_with_ink(1330,1492,886,915))
print("quote2 rows x1360..1380:", rows_with_ink(1360,1380,855,915))
