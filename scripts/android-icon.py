r"""Build Android launcher icons for the Lokalomat mobile app from the project icon.

usage (needs Pillow and resvg-py):
  python scripts/android-icon.py apps/desktop/app-icon.svg \
      apps/mobile/src-tauri/gen/android/app/src/main/res

- adaptive icon (API 26+): solid background colour + mark-only foreground
  (+ monochrome layer for themed icons, Android 13+), rendered from the SVG;
- legacy ic_launcher.png (the project icon as is) and ic_launcher_round.png.
The mark is the 'L' inside app-icon.svg (group with the gradients).
"""
import io
import re
import sys
from pathlib import Path

import resvg_py
from PIL import Image, ImageDraw

svg_path, res = Path(sys.argv[1]), Path(sys.argv[2])
svg = svg_path.read_text(encoding="utf-8")
BG = (246, 240, 253)  # background of the icon square (#f6f0fd)

mark = re.search(r"<g [^>]*>(.*)</g>", svg, re.S).group(1)
# The mark occupies x 160..505, y 40..345 in its own coordinates.
MARK_VIEWBOX = "142 20 381 345"


def render(body: str, size: int, viewbox: str) -> Image.Image:
    doc = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{viewbox}">{body}</svg>'
    data = resvg_py.svg_to_bytes(svg_string=doc, width=size, height=size)
    return Image.open(io.BytesIO(bytes(data))).convert("RGBA")


def mono_body() -> str:
    flat = re.sub(r"<defs>.*?</defs>", "", mark, flags=re.S)
    return re.sub(r'fill="url\(#[^)]*\)"', 'fill="#fff"', flat)


def placed(body: str, size: int, visible: float) -> Image.Image:
    """The mark centred, spanning `visible` of `size`."""
    side = round(size * visible)
    layer = render(body, side, MARK_VIEWBOX)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas.alpha_composite(layer, ((size - side) // 2, (size - side) // 2))
    return canvas


DENSITIES = {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}
for name, k in DENSITIES.items():
    d = res / f"mipmap-{name}"
    d.mkdir(parents=True, exist_ok=True)
    fg = round(108 * k)
    # The launcher mask shows ~72 of 108 dp; the mark stays inside the 66 dp safe zone.
    placed(mark, fg, 60 / 108).save(d / "ic_launcher_foreground.png", optimize=True)
    placed(mono_body(), fg, 60 / 108).save(d / "ic_launcher_monochrome.png", optimize=True)

    legacy = round(48 * k)
    full = resvg_py.svg_to_bytes(svg_string=svg, width=legacy, height=legacy)
    Image.open(io.BytesIO(bytes(full))).convert("RGBA").save(d / "ic_launcher.png", optimize=True)

    # Round legacy icon: background circle (44 of 48 dp) with the mark.
    ss = legacy * 4
    circle = Image.new("RGBA", (ss, ss), (0, 0, 0, 0))
    pad = ss * 2 / 48
    ImageDraw.Draw(circle).ellipse((pad, pad, ss - pad, ss - pad), fill=(*BG, 255))
    circle = circle.resize((legacy, legacy), Image.LANCZOS)
    circle.alpha_composite(placed(mark, legacy, 30 / 48))
    circle.save(d / "ic_launcher_round.png", optimize=True)

bg_xml = res / "values" / "ic_launcher_background.xml"
bg_xml.write_text(
    '<?xml version="1.0" encoding="utf-8"?>\n<resources>\n'
    "  <!-- Background of the project icon (apps/desktop/app-icon.svg). -->\n"
    '  <color name="ic_launcher_background">#F4FAF6</color>\n</resources>\n',
    encoding="utf-8",
)
print("done")
