#!/usr/bin/env python3
# renders the app icon: a voxel log stump, end grain on top, the pith core in the theme's pink with a shoot coming out of it.
# isometric, painted back to front, drawn at 4x and scaled down so the edges stay crisp but not jagged.
# usage: packaging/voxel-icon.py out.png [size]
import math, sys
from PIL import Image, ImageDraw

out = sys.argv[1]
size = int(sys.argv[2]) if len(sys.argv) > 2 else 1024
S = size * 4

# the palette is the default matugen theme pith ships with, so the icon sits next to the app
BG = (26, 17, 18)
CORE = (255, 178, 186)        # primary
CORE_DEEP = (204, 112, 124)
RING_A = (233, 191, 142)      # tertiary
RING_B = (196, 150, 102)
BARK = (118, 80, 64)
BARK_DARK = (84, 56, 46)
LEAF = (150, 196, 120)
LEAF_DARK = (110, 156, 92)

vox = {}
R, H = 5.5, 5
for x in range(-6, 7):
    for y in range(-6, 7):
        d = math.hypot(x, y)
        if d > R: continue
        for z in range(H):
            if d > R - 1.1:
                # bark, with grooves running down it
                c = BARK_DARK if (x * 3 + y * 5) % 4 == 0 else BARK
            elif d < 1.5:
                c = CORE_DEEP
            else:
                c = RING_A if int(d - 1.5) % 2 == 0 else RING_B
            vox[(x, y, z)] = c
        # the pith stands a voxel proud of the grain
        if d < 1.5: vox[(x, y, H)] = CORE
# a seedling out of the core: a stem and two leaves opening to either side
for z in (H + 1, H + 2, H + 3): vox[(0, 0, z)] = LEAF_DARK
for p in [(1, 0, H + 3), (2, 0, H + 4), (3, 0, H + 4), (0, 1, H + 4), (0, 2, H + 5), (0, 3, H + 5)]: vox[p] = LEAF
# fit the shape to the tile: measure it at unit scale, then scale and centre
def unit(x, y, z): return ((x - y) / 2, (x + y) / 4 - z / 2)
pts = [unit(x + i, y + j, z + k) for (x, y, z) in vox for i in (0, 1) for j in (0, 1) for k in (0, 1)]
x0, x1 = min(p[0] for p in pts), max(p[0] for p in pts)
y0, y1 = min(p[1] for p in pts), max(p[1] for p in pts)
W = S * 0.7 / max(x1 - x0, y1 - y0)   # width of one voxel's top diamond
cx, cy = S / 2 - (x0 + x1) / 2 * W, S / 2 - (y0 + y1) / 2 * W

def proj(x, y, z):
    u = unit(x, y, z)
    return (cx + u[0] * W, cy + u[1] * W)

def shade(c, k):
    return tuple(max(0, min(255, round(v * k))) for v in c)

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
dr = ImageDraw.Draw(img)
r = S * 0.22
dr.rounded_rectangle((0, 0, S - 1, S - 1), radius=r, fill=BG + (255,))

line = max(1, round(S / 512))
for (x, y, z) in sorted(vox, key=lambda p: (p[0] + p[1] + p[2], p[2])):
    c = vox[(x, y, z)]
    a, b, cc, d = proj(x, y, z + 1), proj(x + 1, y, z + 1), proj(x + 1, y + 1, z + 1), proj(x, y + 1, z + 1)
    e, f, g = proj(x + 1, y, z), proj(x + 1, y + 1, z), proj(x, y + 1, z)
    faces = []
    if (x, y, z + 1) not in vox: faces.append(([a, b, cc, d], 1.0))
    if (x + 1, y, z) not in vox: faces.append(([b, cc, f, e], 0.78))
    if (x, y + 1, z) not in vox: faces.append(([d, cc, f, g], 0.62))
    for poly, k in faces:
        dr.polygon(poly, fill=shade(c, k) + (255,), outline=shade(c, k * 0.9) + (255,), width=line)

# a soft floor shadow would fight the tile, so the stump just sits on it
img = img.resize((size, size), Image.LANCZOS)
img.save(out)
print("wrote", out)
