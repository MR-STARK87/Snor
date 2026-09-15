from PIL import Image
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB")
# wide look at the rail region
c = im.crop((1240, 60, 1536, 1010))
c = c.resize((c.width*2, c.height*2), Image.NEAREST)
c.save(r"C:/My work folder/Snor/.workbuddy-ai/screenshots/ref_rail_region.png")
print("saved", c.size)
# and a brightened version so faint ink is visible
from PIL import ImageEnhance
b = ImageEnhance.Brightness(c).enhance(3.2)
b.save(r"C:/My work folder/Snor/.workbuddy-ai/screenshots/ref_rail_region_bright.png")
