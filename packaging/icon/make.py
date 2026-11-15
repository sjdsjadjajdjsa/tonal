"""Draws Tonal's icon and writes every size the builds use.

    python3 packaging/icon/make.py
    (needs rsvg-convert, `brew install librsvg`, and fonttools, `pip install fonttools`)

Writes:
  packaging/icon/tonal.svg    the icon, tile filling the canvas
  ui/icon.png                 256 px: window icon, Linux, the website
  packaging/windows/tonal.ico 16 to 256 px
  packaging/macos/icon.png    1024 px on Apple's grid (an 824 px tile with a 100 px
                              margin), so macOS shows it as-is instead of putting it
                              on a white tile
  packaging/macos/dmg-background.tiff
                              the window the .dmg opens to (packaging/macos/dmg.py),
                              at 1x and 2x
"""

import math
import struct
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT / "docs"))
from banner import text_path, wave  # noqa: E402
GREEN, GLOW = "#3cf07c", "#2ee06f"


def squircle(size, steps=360, n=5):
    """Apple's continuous-corner tile, as a superellipse."""
    pts = []
    for i in range(steps):
        t = 2 * math.pi * i / steps
        c, s = math.cos(t), math.sin(t)
        x = math.copysign(abs(c) ** (2 / n), c)
        y = math.copysign(abs(s) ** (2 / n), s)
        pts.append(f"{(x + 1) / 2 * size:.2f},{(y + 1) / 2 * size:.2f}")
    return "M" + "L".join(pts) + "Z"


def smooth(points):
    """A smooth curve through the points (Catmull-Rom as cubic Béziers)."""
    d = f"M{points[0][0]},{points[0][1]}"
    for i in range(len(points) - 1):
        p0 = points[max(i - 1, 0)]
        p1, p2 = points[i], points[i + 1]
        p3 = points[min(i + 2, len(points) - 1)]
        c1 = (p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6)
        c2 = (p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6)
        d += f" C{c1[0]:.2f},{c1[1]:.2f} {c2[0]:.2f},{c2[1]:.2f} {p2[0]},{p2[1]}"
    return d


def svg(canvas, tile, offset):
    """The icon drawn in a 100-unit tile, scaled to `tile` px at `offset` on a `canvas` px image."""
    wave = smooth([(-6, 66), (12, 62), (30, 36), (44, 50), (56, 66), (70, 44), (84, 42), (106, 58)])
    scale = tile / 100
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{canvas}" height="{canvas}" viewBox="0 0 {canvas} {canvas}">
<defs>
  <clipPath id="tile"><path d="{squircle(100)}"/></clipPath>
  <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#1c1f26"/><stop offset="1" stop-color="#0a0b0e"/>
  </linearGradient>
  <linearGradient id="fill" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0.3" stop-color="{GLOW}" stop-opacity="0.45"/><stop offset="1" stop-color="{GLOW}" stop-opacity="0"/>
  </linearGradient>
  <filter id="glow" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="2.4"/></filter>
</defs>
<g transform="translate({offset} {offset}) scale({scale})" clip-path="url(#tile)">
  <rect width="100" height="100" fill="url(#bg)"/>
  <path d="{wave} L106,106 L-6,106 Z" fill="url(#fill)"/>
  <path d="{wave}" fill="none" stroke="{GLOW}" stroke-width="6" stroke-linecap="round" filter="url(#glow)" opacity="0.7"/>
  <path d="{wave}" fill="none" stroke="{GREEN}" stroke-width="5" stroke-linecap="round"/>
</g>
</svg>
"""


def render(source, size, out):
    subprocess.run(
        ["rsvg-convert", "-w", str(size), "-h", str(size), "-o", str(out), str(source)],
        check=True,
    )


def ico(pngs, out):
    """A Windows .ico holding each PNG as is (Vista and later read PNG entries)."""
    header = struct.pack("<HHH", 0, 1, len(pngs))
    entries, data = b"", b""
    offset = 6 + 16 * len(pngs)
    for size, png in pngs:
        side = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(png), offset + len(data))
        data += png
    out.write_bytes(header + entries + data)


def dmg_background():
    """640 x 400, light so Finder's black file names stay readable. The icons sit at
    (160, 200) and (480, 200), as packaging/macos/dmg.py places them."""
    W, H = 640, 400
    title, title_w = text_path("Inter-SemiBold.ttf", "Drag Tonal to Applications", 22, 0, 0)
    hint, hint_w = text_path("Inter-Regular.ttf", "Then open it from your Applications folder", 13, 0, 0)
    waves = [
        (520, 14, 360, [(1, 1.0, 0.4), (2, 0.35, 1.3)], 0.10),
        (420, 11, 370, [(1, 1.0, 2.1), (3, 0.25, 0.4)], 0.16),
        (330, 8, 380, [(1, 1.0, 4.0), (2, 0.3, 2.2)], 0.24),
    ]
    layers = "".join(
        f'<path d="{wave(period, amp, mid, harmonics, W, H)}" fill="#13a64b" fill-opacity="{alpha}"/>'
        for period, amp, mid, harmonics, alpha in waves
    )
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">
<defs>
  <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#fbfbfc"/><stop offset="1" stop-color="#eef0f2"/>
  </linearGradient>
  <radialGradient id="glow" cx="0.5" cy="1.1" r="0.8">
    <stop offset="0" stop-color="#1ed760" stop-opacity="0.16"/><stop offset="1" stop-color="#1ed760" stop-opacity="0"/>
  </radialGradient>
</defs>
<rect width="{W}" height="{H}" fill="url(#bg)"/>
<rect width="{W}" height="{H}" fill="url(#glow)"/>
{layers}
<path transform="translate({(W - title_w) / 2:.1f} 74)" d="{title}" fill="#111113"/>
<path transform="translate({(W - hint_w) / 2:.1f} 100)" d="{hint}" fill="#6b6b74"/>
<g fill="none" stroke="#9a9aa3" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
  <path d="M262 196 H372"/>
  <path d="M360 184 L374 196 L360 208"/>
</g>
</svg>
"""


def main():
    here = Path(__file__).resolve().parent
    full = here / "tonal.svg"
    full.write_text(svg(1024, 1024, 0))
    render(full, 256, ROOT / "ui" / "icon.png")

    mac = here / "tonal-macos.svg"
    mac.write_text(svg(1024, 824, 100))
    render(mac, 1024, ROOT / "packaging" / "macos" / "icon.png")
    mac.unlink()

    tmp = here / "ico.png"
    pngs = []
    for size in (16, 24, 32, 48, 64, 128, 256):
        render(full, size, tmp)
        pngs.append((size, tmp.read_bytes()))
    tmp.unlink()
    ico(pngs, ROOT / "packaging" / "windows" / "tonal.ico")

    bg = here / "dmg-background.svg"
    bg.write_text(dmg_background())
    one, two = here / "bg.png", here / "bg@2x.png"
    for width, png in ((640, one), (1280, two)):
        subprocess.run(["rsvg-convert", "-w", str(width), "-o", str(png), str(bg)], check=True)
    out = ROOT / "packaging" / "macos" / "dmg-background.tiff"
    subprocess.run(["tiffutil", "-cathidpicheck", str(one), str(two), "-out", str(out)], check=True)
    for f in (bg, one, two):
        f.unlink()


if __name__ == "__main__":
    main()
