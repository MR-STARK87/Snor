"""Stack an explicit region of the reference above the matching region of the
live capture, each scaled to the same displayed width.

quad_compare's `region` mode addresses both images with the same coordinates,
which only lines up when the two windows are the same size. This takes a box
per image instead.
"""
import sys
from PIL import Image, ImageDraw, ImageEnhance

REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
REF_BOX = (42, 30, 1497, 982)  # the mock's window interior

ref_crop = tuple(int(v) for v in sys.argv[1:5])
live_crop = tuple(int(v) for v in sys.argv[5:9])
out_path = sys.argv[9]
zoom = float(sys.argv[10]) if len(sys.argv) > 10 else 2.0
bright = float(sys.argv[11]) if len(sys.argv) > 11 else 1.0

ref = Image.open(REF).convert("RGB").crop(REF_BOX).crop(ref_crop)
live = Image.open(r"C:/My work folder/Snor/.workbuddy-ai/screenshots/live.png").convert("RGB").crop(live_crop)

w = 1150
ref = ref.resize((w, max(1, round(ref.height * w / ref.width))), Image.LANCZOS)
live = live.resize((w, max(1, round(live.height * w / live.width))), Image.LANCZOS)

HEAD = 22
out = Image.new("RGB", (w, ref.height + live.height + HEAD * 2), (10, 14, 13))
d = ImageDraw.Draw(out)
d.text((6, 5), f"REFERENCE  {ref_crop[2]-ref_crop[0]}x{ref_crop[3]-ref_crop[1]}px", fill=(196, 206, 196))
out.paste(ref, (0, HEAD))
d.text((6, HEAD + ref.height + 5), f"LIVE  {live_crop[2]-live_crop[0]}x{live_crop[3]-live_crop[1]}px", fill=(196, 206, 196))
out.paste(live, (0, HEAD * 2 + ref.height))
if bright != 1.0:
    out = ImageEnhance.Brightness(out).enhance(bright)
out = out.resize((int(out.width * zoom), int(out.height * zoom)), Image.LANCZOS)
out.save(out_path)
print(out_path, out.size)
