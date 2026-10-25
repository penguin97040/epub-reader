#!/usr/bin/env python3
"""Generate the epub-reader app icon (product-specific symbol, not the penguin).

An open book drawn flat on solid black with a beak-orange bookmark, per
the project icon rules (square corners, solid token colours only).
Run: python3 tools/make_icon.py   (needs pillow)
Outputs: assets/icon-1024.png, assets/icon-256.png, assets/icon.ico
"""

from PIL import Image, ImageDraw

S = 1024
BLACK = (0, 0, 0, 255)
TEXT = (242, 242, 242, 255)
BEAK = (219, 119, 0, 255)
LINE = (44, 44, 44, 255)

img = Image.new("RGBA", (S, S), BLACK)
d = ImageDraw.Draw(img)

# Open book: two flat pages meeting at the spine, drawn as quadrilaterals.
top = 332
bottom = 732
mid = S // 2
d.polygon([(132, top + 40), (mid - 16, top), (mid - 16, bottom), (132, bottom - 40)], fill=TEXT)
d.polygon([(S - 132, top + 40), (mid + 16, top), (mid + 16, bottom), (S - 132, bottom - 40)], fill=TEXT)

# Spine gap.
d.rectangle([mid - 16, top - 4, mid + 16, bottom + 4], fill=BLACK)

# Text lines on the pages, in the divider colour.
for i in range(5):
    y = top + 96 + i * 72
    d.line([(196, y + 10), (mid - 76, y)], fill=LINE, width=18)
    d.line([(mid + 76, y), (S - 196, y + 10)], fill=LINE, width=18)

# Beak bookmark hanging from the right page.
d.polygon([(S - 320, top + 18), (S - 224, top + 14), (S - 224, bottom + 104),
           (S - 272, bottom + 52), (S - 320, bottom + 100)], fill=BEAK)

img.save("assets/icon-1024.png")
img.resize((256, 256), Image.LANCZOS).save("assets/icon-256.png")
img.save("assets/icon.ico", sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print("wrote assets/icon-1024.png, assets/icon-256.png, assets/icon.ico")
