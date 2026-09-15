from PIL import Image
from collections import Counter
LIVE = r"C:/My work folder/Snor/.workbuddy-ai/screenshots/live.png"
im = Image.open(LIVE).convert("RGB"); px = im.load()
def modal(box, label):
    c = Counter(px[x, y] for y in range(box[1], box[3]) for x in range(box[0], box[2], 3))
    t = c.most_common(1)[0][0]
    print(f"{label:28s} {t}  luma {0.2126*t[0]+0.7152*t[1]+0.0722*t[2]:.1f}")
print("REFERENCE for comparison:")
print("  title bar  (22,30,28) luma 28.2 | editor (19,26,25) luma 24.4 | explorer/terminal (17,24,23) luma 22.4")
print()
modal((600, 6, 1300, 20),    "LIVE title bar")
modal((600, 100, 1300, 200), "LIVE editor")
modal((60, 200, 250, 400),   "LIVE explorer")
modal((600, 700, 1300, 900), "LIVE terminal")
modal((600, 940, 1300, 990), "LIVE status bar")
