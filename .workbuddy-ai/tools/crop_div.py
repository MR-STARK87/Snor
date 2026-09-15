from PIL import Image, ImageEnhance
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB")
c = im.crop((40, 570, 1500, 700))
c = ImageEnhance.Brightness(c).enhance(3.5)
c = c.resize((c.width, c.height*3), Image.NEAREST)
c.save(r"C:/My work folder/Snor/.workbuddy-ai/screenshots/ref_divider.png")
print(c.size)
