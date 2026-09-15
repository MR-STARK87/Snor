from PIL import Image, ImageEnhance
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB")
c = im.crop((1300, 440, 1500, 650))
c = ImageEnhance.Brightness(c).enhance(2.6)
c = c.resize((c.width*4, c.height*4), Image.LANCZOS)
c.save(r"C:/My work folder/Snor/.workbuddy-ai/screenshots/ref_quote_zoom.png")
print(c.size)
