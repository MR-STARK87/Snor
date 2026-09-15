from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()

print("-- background samples (clean rows) --")
for (x,y) in [(1250,600),(1290,600),(1311,600),(1312,600),(1313,600),(1320,600),
              (1400,600),(1450,600),(1490,600),(1250,300),(1450,300),
              (1312,150),(1312,200),(1312,900),(1312,930),(1312,940),(1312,120)]:
    print((x,y), px[x,y])

# vertical extent of the rule at x=1312: rows where it is brighter than x-2 and x+2
print("\n-- rule vertical extent (x=1312 vs neighbours) --")
def l(x,y):
    r,g,b = px[x,y]; return 0.2126*r+0.7152*g+0.0722*b
rows=[]
for y in range(30, 1010):
    if l(1312,y) - (l(1310,y)+l(1314,y))/2 > 4.0:
        rows.append(y)
print("count", len(rows))
if rows:
    # find contiguous runs
    runs=[]; s=rows[0]; p=rows[0]
    for y in rows[1:]:
        if y==p+1: p=y
        else: runs.append((s,p)); s=y; p=y
    runs.append((s,p))
    print("runs:", [r for r in runs if r[1]-r[0] > 2])

# rail text ink: leftmost dark..no, ink is LIGHT on dark. find bright columns x>1312
print("\n-- bright ink columns x 1312..1500, rows 150..930 --")
for x in range(1312, 1500):
    c=0
    for y in range(150,930):
        if l(x,y) > 45: c+=1
    if c>0: print(x, c)
