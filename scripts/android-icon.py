r"""Build Android launcher icons for the Lokalomat mobile app from the project icon.

usage (needs Pillow):
  python scripts/android-icon.py apps/desktop/src-tauri/icons/icon.png \
      apps/mobile/src-tauri/gen/android/app/src/main/res

- adaptive icon (API 26+): solid background colour + glyph-only foreground
  (+ monochrome layer for themed icons, Android 13+);
- legacy ic_launcher.png (the project icon as is) and ic_launcher_round.png.
"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw

src_path, res = Path(sys.argv[1]), Path(sys.argv[2])
src = Image.open(src_path).convert("RGBA")
W = src.width
BG = (35, 39, 43)  # icon background, sampled from icon.png
GLYPHS = [(243, 241, 236), (87, 196, 138)]  # "L" and the dot

# Bounds of the rounded square in the source (opaque pixels).
alpha = src.getchannel("A")
x0, y0, x1, y1 = alpha.point(lambda a: 255 if a > 250 else 0).getbbox()
square = x1 - x0  # ~448 px in a 512 px icon


def glyph_layer(mono: bool) -> Image.Image:
    """Glyph pixels un-blended from the background (exact anti-aliasing)."""
    out = Image.new("RGBA", src.size, (0, 0, 0, 0))
    sp, op = src.load(), out.load()
    for y in range(W):
        for x in range(W):
            r, g, b, a = sp[x, y]
            if a < 250:
                continue
            best = None
            for f in GLYPHS:
                d = [f[i] - BG[i] for i in range(3)]
                c = [(r, g, b)[i] - BG[i] for i in range(3)]
                t = sum(c[i] * d[i] for i in range(3)) / sum(v * v for v in d)
                t = max(0.0, min(1.0, t))
                err = sum((BG[i] + t * d[i] - (r, g, b)[i]) ** 2 for i in range(3))
                if best is None or err < best[0]:
                    best = (err, t, f)
            _, t, f = best
            if t > 0.004:
                colour = (255, 255, 255) if mono else f
                op[x, y] = (*colour, round(t * 255))
    return out


def placed(layer: Image.Image, size: int, visible: float) -> Image.Image:
    """Scale so the source square spans `visible` of `size`, centred."""
    scale = size * visible / square
    w = round(W * scale)
    small = layer.resize((w, w), Image.LANCZOS)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    cx = (x0 + x1) / 2 * scale
    cy = (y0 + y1) / 2 * scale
    canvas.alpha_composite(small, (round(size / 2 - cx), round(size / 2 - cy)))
    return canvas


colour_glyph = glyph_layer(mono=False)
mono_glyph = glyph_layer(mono=True)

DENSITIES = {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}
for name, k in DENSITIES.items():
    d = res / f"mipmap-{name}"
    d.mkdir(parents=True, exist_ok=True)
    fg = round(108 * k)
    # The launcher mask shows ~72 of 108 dp; the glyph keeps its place in the
    # project icon's square and stays inside the 66 dp safe zone.
    placed(colour_glyph, fg, 72 / 108).save(d / "ic_launcher_foreground.png", optimize=True)
    placed(mono_glyph, fg, 72 / 108).save(d / "ic_launcher_monochrome.png", optimize=True)

    legacy = round(48 * k)
    src.resize((legacy, legacy), Image.LANCZOS).save(d / "ic_launcher.png", optimize=True)

    # Round legacy icon: background circle (44 of 48 dp) with the glyph.
    ss = legacy * 4
    circle = Image.new("RGBA", (ss, ss), (0, 0, 0, 0))
    pad = ss * 2 / 48
    ImageDraw.Draw(circle).ellipse((pad, pad, ss - pad, ss - pad), fill=(*BG, 255))
    circle = circle.resize((legacy, legacy), Image.LANCZOS)
    circle.alpha_composite(placed(colour_glyph, legacy, 44 / 48))
    circle.save(d / "ic_launcher_round.png", optimize=True)

print("square", square, "bbox", (x0, y0, x1, y1))
