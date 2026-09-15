from PIL import Image, ImageEnhance
REF = r"C:/My work folder/Snor/file_00000000996c81faaa1c3fe9743de377.png"
im = Image.open(REF).convert("RGB").crop((42,30,1497,982))
im = ImageEnhance.Brightness(im).enhance(2.2)
im = im.resize((1080, int(im.height*1080/im.width)), Image.LANCZOS)
im.save(r"C:/My work folder/Snor/.workbuddy-ai/screenshots/ref_overview.png")
print(im.size)
