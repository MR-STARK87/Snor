from PIL import Image
from collections import Counter

def modal(im, box, label):
    px = im.load()
    c = Counter()
    for y in range(box[1], box[3]):
        for x in range(box[0], box[2], 3):
            c[px[x, y]] += 1
    top = c.most_common(3)
    print(f"{label}: {top}")

REF = r"C:\My work folder\Snor\file_00000000996c81faaa1c3fe9743de377.png"
LIVE = r"C:\My work folder\Snor\.workbuddy-ai\screenshots\live.png"
ref = Image.open(REF).convert("RGB"); live = Image.open(LIVE).convert("RGB")

modal(ref,  (600, 36, 1300, 50), "REF title band  y36..50")
modal(ref,  (600, 40, 1300, 55), "REF title band  y40..55")
modal(ref,  (600, 60, 1300, 74), "REF just under rule y60..74")
modal(ref,  (600, 100, 1300, 200), "REF editor area y100..200")
modal(ref,  (60, 200, 250, 400), "REF explorer    y200..400")
print()
modal(live, (600, 6, 1300, 20), "LIVE title band  y6..20")
modal(live, (600, 20, 1300, 34), "LIVE title band  y20..34")
modal(live, (600, 60, 1300, 74), "LIVE just under rule")
modal(live, (600, 100, 1300, 200), "LIVE editor area")
modal(live, (60, 200, 250, 400), "LIVE explorer")

print()
print("-- REF rows 30..80 mean luma x600..1300 --")
px = ref.load()
for y in range(30, 82):
    xs = list(range(600, 1300, 4))
    print(y, round(sum(0.2126*px[x,y][0]+0.7152*px[x,y][1]+0.0722*px[x,y][2] for x in xs)/len(xs), 2))
