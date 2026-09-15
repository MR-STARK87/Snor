from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB"); px = im.load()
print("size", im.size)
print("\n-- top edge, column x=700, y 20..40 --")
for y in range(20,40): print(y, px[700,y])
print("\n-- left edge, row y=500, x 32..50 --")
for x in range(32,50): print(x, px[x,500])
print("\n-- right edge, row y=500, x 1490..1505 --")
for x in range(1490,1506): print(x, px[x,500])
print("\n-- bottom edge, column x=700, y 974..992 --")
for y in range(974,992): print(y, px[700,y])
print("\n-- corners --")
for (x,y) in [(41,29),(42,30),(43,31),(41,31),(43,29),(1496,29),(1496,981),(41,981),(45,33)]:
    print((x,y), px[x,y])
