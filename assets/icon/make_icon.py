# Renders the Tracked mark (same geometry as ui::icons::logo) as an app icon.
import sys
from PIL import Image, ImageDraw
out = sys.argv[1]
S = 1024  # supersampled canvas
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
# Dark rounded tile.
pad = int(S * 0.04)
d.rounded_rectangle([pad, pad, S - pad, S - pad], radius=int(S * 0.22), fill=(29, 33, 44, 255))
# Logo geometry on a 20x20 grid, centred (as in icons::logo).
s = S * 0.62 / 20.0
ox = S / 2 - 10 * s
oy = S / 2 - 10 * s + S * 0.01
P = lambda x, y: (ox + x * s, oy + y * s)
coral = (236, 104, 114, 255)
bar = (236, 104, 114, 255)
d.polygon([P(10, 2), P(18.5, 12), P(14.5, 12), P(10, 6.8)], fill=coral)
d.polygon([P(10, 2), P(10, 6.8), P(5.5, 12), P(1.5, 12)], fill=coral)
# Bar at 60% strength over the dark tile.
mix = tuple(int(c * 0.6 + b * 0.4) for c, b in zip((236, 104, 114), (29, 33, 44))) + (255,)
d.polygon([P(4, 15), P(16, 15), P(14.5, 17.5), P(5.5, 17.5)], fill=mix)
big = img.resize((256, 256), Image.LANCZOS)
big.save(f"{out}/tracked.png")
big.save(f"{out}/tracked.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print("ok")
