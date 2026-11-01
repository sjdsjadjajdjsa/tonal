"""Draws the README banner: python3 docs/banner.py (needs fonttools)."""

import base64
import math
from pathlib import Path

from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parent.parent
FONTS = ROOT / "ui" / "fonts"
W, H = 1280, 360

LOOKS = {
    "dark": dict(bg="#0e0f12", glow="#1ed760", text="#ececee", muted="#8d8d96", wave="#1ed760", ring="0.14"),
    "light": dict(bg="#f5f6f7", glow="#1ed760", text="#111113", muted="#6b6b74", wave="#13a64b", ring="0"),
}


def text_path(font_file, text, size, x, baseline, tracking=0.0):
    font = TTFont(FONTS / font_file)
    glyphs = font.getGlyphSet()
    cmap = font.getBestCmap()
    scale = size / font["head"].unitsPerEm
    pen = SVGPathPen(glyphs, ntos=lambda v: f"{v:.1f}".rstrip("0").rstrip("."))
    cursor = 0.0
    for ch in text:
        name = cmap[ord(ch)]
        glyph = glyphs[name]
        glyph.draw(TransformPen(pen, (scale, 0, 0, -scale, x + cursor, baseline)))
        cursor += glyph.width * scale + tracking * size
    return pen.getCommands(), cursor - tracking * size


def wave(period, amp, mid, harmonics, width):
    points = []
    for i in range(0, int(width) + 1, 4):
        t = i / period * 2 * math.pi
        y = mid - amp * sum(a * math.sin(k * t + p) for k, a, p in harmonics)
        points.append(f"{i},{y:.1f}")
    return f"M0,{H} L" + " L".join(points) + f" L{int(width)},{H} Z"


def banner(look):
    c = LOOKS[look]
    icon = base64.b64encode((ROOT / "ui" / "icon.png").read_bytes()).decode()
    word, word_w = text_path("Inter-Bold.ttf", "tonal", 112, 0, 0, -0.03)
    line_one, line_w = text_path("Inter-Medium.ttf", "A native music player that moves with your music.", 26, 0, 0)
    line_two, _ = text_path(
        "Inter-Regular.ttf", "YouTube Music  ·  SoundCloud  ·  Audius  ·  local files  ·  Spotify import", 18, 0, 0, 0.01
    )
    icon_size, gap = 120, 36
    left = (W - (icon_size + gap + max(word_w, line_w + 6))) / 2
    text_x = left + icon_size + gap
    waves = [
        (640, 34, 300, [(1, 1.0, 0.0), (2, 0.35, 1.3)], 0.10, 26),
        (520, 26, 312, [(1, 1.0, 2.1), (3, 0.25, 0.4)], 0.16, 19),
        (400, 18, 324, [(1, 1.0, 4.0), (2, 0.3, 2.2)], 0.28, 13),
    ]
    layers = []
    for period, amp, mid, harmonics, alpha, secs in waves:
        d = wave(period, amp, mid, harmonics, W + period)
        layers.append(
            f'<path d="{d}" fill="{c["wave"]}" fill-opacity="{alpha}">'
            f'<animateTransform attributeName="transform" type="translate" from="0 0" to="-{period} 0" '
            f'dur="{secs}s" repeatCount="indefinite"/></path>'
        )
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img" aria-label="tonal, a native music player">
<defs>
<clipPath id="card"><rect width="{W}" height="{H}" rx="28"/></clipPath>
<radialGradient id="glow" cx="0.5" cy="1.05" r="0.75"><stop offset="0" stop-color="{c["glow"]}" stop-opacity="0.22"/><stop offset="1" stop-color="{c["glow"]}" stop-opacity="0"/></radialGradient>
</defs>
<g clip-path="url(#card)">
<rect width="{W}" height="{H}" fill="{c["bg"]}"/>
<rect width="{W}" height="{H}" fill="url(#glow)"/>
{"".join(layers)}
</g>
<image x="{left:.1f}" y="68" width="{icon_size}" height="{icon_size}" href="data:image/png;base64,{icon}"/>
<rect x="{left + 0.5:.1f}" y="68.5" width="{icon_size - 1}" height="{icon_size - 1}" rx="27" fill="none" stroke="{c["text"]}" stroke-opacity="{c["ring"]}"/>
<path transform="translate({text_x:.1f} 166)" d="{word}" fill="{c["text"]}"/>
<path transform="translate({text_x + 6:.1f} 218)" d="{line_one}" fill="{c["text"]}" fill-opacity="0.92"/>
<path transform="translate({text_x + 6:.1f} 252)" d="{line_two}" fill="{c["muted"]}"/>
</svg>
"""


for look in LOOKS:
    (ROOT / "docs" / "images" / f"banner-{look}.svg").write_text(banner(look))
