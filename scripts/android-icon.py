r"""Build Android launcher icons for the Lokalomat mobile app from the project icon.

usage (needs Pillow and resvg-py):
  python scripts/android-icon.py apps/desktop/app-icon.svg \
      apps/mobile/src-tauri/gen/android/app/src/main/res
  python scripts/android-icon.py apps/mobile/app-icon-pro.svg \
      apps/mobile/src-tauri/gen/android/app/src/main/res ic_launcher_pro

The optional third argument is the resource name (default ic_launcher). The
gold ic_launcher_pro set is the launcher icon once the phone is paired with
Lokalomat Pro (activity-alias .LauncherPro, apps/mobile/src-tauri/src/launcher.rs).

- adaptive icon (API 26+): solid background colour + mark-only foreground
  (+ monochrome layer for themed icons, Android 13+), rendered from the SVG;
- legacy <name>.png (the project icon as is) and <name>_round.png;
- values/<name>_background.xml (colour of the icon square) and the adaptive XMLs.
The mark is the 'L' inside app-icon.svg (group with the gradients).
"""
import io
import re
import sys
from pathlib import Path

import resvg_py
from PIL import Image, ImageDraw

svg_path, res = Path(sys.argv[1]), Path(sys.argv[2])
NAME = sys.argv[3] if len(sys.argv) > 3 else "ic_launcher"
svg = svg_path.read_text(encoding="utf-8")
# Background of the icon square: the fill of the first <rect> in the SVG.
BG_HEX = re.search(r'<rect [^>]*fill="#([0-9a-fA-F]{6})"', svg).group(1).upper()
BG = tuple(int(BG_HEX[i : i + 2], 16) for i in (0, 2, 4))

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
    placed(mark, fg, 60 / 108).save(d / f"{NAME}_foreground.png", optimize=True)
    placed(mono_body(), fg, 60 / 108).save(d / f"{NAME}_monochrome.png", optimize=True)

    legacy = round(48 * k)
    full = resvg_py.svg_to_bytes(svg_string=svg, width=legacy, height=legacy)
    Image.open(io.BytesIO(bytes(full))).convert("RGBA").save(d / f"{NAME}.png", optimize=True)

    # Round legacy icon: background circle (44 of 48 dp) with the mark.
    ss = legacy * 4
    circle = Image.new("RGBA", (ss, ss), (0, 0, 0, 0))
    pad = ss * 2 / 48
    ImageDraw.Draw(circle).ellipse((pad, pad, ss - pad, ss - pad), fill=(*BG, 255))
    circle = circle.resize((legacy, legacy), Image.LANCZOS)
    circle.alpha_composite(placed(mark, legacy, 30 / 48))
    circle.save(d / f"{NAME}_round.png", optimize=True)

bg_xml = res / "values" / f"{NAME}_background.xml"
bg_xml.write_text(
    f"""<?xml version="1.0" encoding="utf-8"?>
<resources>
  <!-- Background of the icon square ({svg_path.as_posix()}). -->
  <color name="{NAME}_background">#{BG_HEX}</color>
</resources>
""",
    encoding="utf-8",
)
adaptive = f"""<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
  <background android:drawable="@color/{NAME}_background"/>
  <foreground android:drawable="@mipmap/{NAME}_foreground"/>
  <monochrome android:drawable="@mipmap/{NAME}_monochrome"/>
</adaptive-icon>
"""
anydpi = res / "mipmap-anydpi-v26"
anydpi.mkdir(parents=True, exist_ok=True)
for xml in (f"{NAME}.xml", f"{NAME}_round.xml"):
    (anydpi / xml).write_text(adaptive, encoding="utf-8")
print("done")
