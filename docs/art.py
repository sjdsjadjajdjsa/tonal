"""Draws the README cards in docs/images (needs fonttools): python3 docs/art.py

Every card shares the banner's look: a rounded dark or light frame, a soft green glow,
Inter drawn as outlines so it looks the same everywhere, and the app's own icons.
"""

import math
import re
from functools import cache
from pathlib import Path

from banner import text_path

ROOT = Path(__file__).resolve().parent.parent
W, PAD = 1280, 56

LOOKS = {
    "dark": dict(
        bg="#0e0f12", text="#ececee", muted="#8d8d96", track="#1b1c21", bar="#3b3c44",
        accent="#1ed760", accent2="#0fa34a", tile="#15161a", line="#26272d", glow="0.16", chip="0.12",
    ),
    "light": dict(
        bg="#f5f6f7", text="#111113", muted="#6b6b74", track="#e6e8eb", bar="#c3c6cc",
        accent="#13a64b", accent2="#0b7d37", tile="#ffffff", line="#e1e3e6", glow="0.10", chip="0.10",
    ),
}


def text(font, words, size, x, y, fill, anchor="start", opacity=1.0, tracking=0.0):
    d, width = text_path(font, words, size, 0, 0, tracking)
    if anchor == "end":
        x -= width
    elif anchor == "middle":
        x -= width / 2
    alpha = f' fill-opacity="{opacity}"' if opacity < 1 else ""
    return f'<path transform="translate({x:.1f} {y:.1f})" d="{d}" fill="{fill}"{alpha}/>', width


def width_of(font, words, size):
    return text_path(font, words, size, 0, 0)[1]


@cache
def icons():
    source = (ROOT / "ui" / "theme.slint").read_text()
    return dict(re.findall(r'out property <string> ([\w-]+): "([^"]+)"', source))


def icon(name, x, y, size, colour):
    return (
        f'<path transform="translate({x:.1f} {y:.1f}) scale({size / 24:.3f})" d="{icons()[name]}" fill="none" '
        f'stroke="{colour}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>'
    )


def ripple(height, period=900, amp=10):
    points = []
    for i in range(0, W + period + 1, 4):
        t = i / period * 2 * math.pi
        y = height - 8 - amp * (math.sin(t) + 0.4 * math.sin(2 * t + 1.1))
        points.append(f"{i},{y:.1f}")
    return f"M0,{height} L" + " L".join(points) + f" L{W + period},{height} Z"


def frame(c, height, body, label, glow=(0.92, 0.0), waves=True, defs=""):
    sea = (
        f'<path d="{ripple(height)}" fill="{c["accent"]}" fill-opacity="0.08"><animateTransform attributeName="transform" '
        f'type="translate" from="0 0" to="-900 0" dur="30s" repeatCount="indefinite"/></path>'
        if waves
        else ""
    )
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {height}" width="{W}" height="{height}" role="img" aria-label="{label}">
<defs>
<clipPath id="card"><rect width="{W}" height="{height}" rx="28"/></clipPath>
<radialGradient id="glow" cx="{glow[0]}" cy="{glow[1]}" r="0.6"><stop offset="0" stop-color="{c["accent"]}" stop-opacity="{c["glow"]}"/><stop offset="1" stop-color="{c["accent"]}" stop-opacity="0"/></radialGradient>
{defs}
</defs>
<g clip-path="url(#card)">
<rect width="{W}" height="{height}" fill="{c["bg"]}"/>
<rect width="{W}" height="{height}" fill="url(#glow)"/>
{sea}
</g>
{chr(10).join(body)}
</svg>
"""


def tile(c, x, y, w, h, radius=20):
    return f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" rx="{radius}" fill="{c["tile"]}" stroke="{c["line"]}"/>'


def chip(c, name, x, y, size=46):
    return [
        f'<rect x="{x:.1f}" y="{y:.1f}" width="{size}" height="{size}" rx="14" fill="{c["accent"]}" fill-opacity="{c["chip"]}"/>',
        icon(name, x + (size - 24) / 2, y + (size - 24) / 2, 24, c["accent"]),
    ]


FEATURES = [
    ("search", "One search, four sources", "YouTube Music, SoundCloud, Audius, files"),
    ("palette", "Living themes", "15 moving scenes, 18 palettes, share codes"),
    ("equalizer", "Six-point equalizer", "Drag a smooth curve, hold to compare"),
    ("lyrics", "Synced lyrics", "Highlighted line by line, from LRCLIB"),
    ("radio", "Endless radio", "Keeps going with similar songs"),
    ("import", "Spotify import", "Bring your playlists, no Premium needed"),
    ("artist", "Artist and album pages", "Popular songs, albums, similar artists"),
    ("heart", "Your library", "Likes, playlists, history, a saved queue"),
    ("download", "Downloads", "Offline listening with tags and covers"),
    ("mini", "Mini player", "A small window, waves included"),
    ("music", "Gapless and crossfade", "Volume levelling and shuffle, by mpv"),
    ("headphones", "Feels native", "Media keys, MPRIS, Last.fm, ListenBrainz"),
]


def features(c):
    cols, gap, h = 3, 16, 148
    w = (W - 2 * PAD - gap * (cols - 1)) / cols
    body = []
    for i, (name, title, line) in enumerate(FEATURES):
        x, y = PAD + (i % cols) * (w + gap), PAD + (i // cols) * (h + gap)
        assert width_of("Inter-Regular.ttf", line, 15) < w - 48, line
        body.append(tile(c, x, y, w, h))
        body += chip(c, name, x + 24, y + 24)
        body.append(text("Inter-SemiBold.ttf", title, 19, x + 24, y + 102, c["text"], tracking=-0.01)[0])
        body.append(text("Inter-Regular.ttf", line, 15, x + 24, y + 126, c["muted"])[0])
    rows = math.ceil(len(FEATURES) / cols)
    height = PAD * 2 + rows * h + (rows - 1) * gap + 20
    return frame(c, height, body, "Tonal's features")


PLATFORMS = [
    ("Linux", "tonal-x86_64.AppImage", ["Make it executable and run it.", "Install mpv first."]),
    ("Windows", "tonal-setup-x86_64.exe", ["Run it, no admin rights needed.", "mpv and yt-dlp are inside."]),
    ("macOS", "tonal-macos-arm64.dmg", ["Drag Tonal to Applications.", "brew install mpv yt-dlp deno"]),
    ("Arch Linux", "packaging/aur/tonal", ["makepkg -si from a clone.", "Uses your own mpv."]),
]


def install(c):
    cols, gap, h = 4, 16, 210
    w = (W - 2 * PAD - gap * (cols - 1)) / cols
    body = []
    for i, (name, file, lines) in enumerate(PLATFORMS):
        x, y = PAD + i * (w + gap), PAD
        body.append(tile(c, x, y, w, h))
        body += chip(c, "download", x + 24, y + 24, 40)
        body.append(text("Inter-Bold.ttf", name, 24, x + 24, y + 102, c["text"], tracking=-0.02)[0])
        fw = width_of("Inter-Medium.ttf", file, 13)
        body.append(f'<rect x="{x + 24:.1f}" y="{y + 118}" width="{fw + 20:.1f}" height="26" rx="8" fill="{c["track"]}"/>')
        body.append(text("Inter-Medium.ttf", file, 13, x + 34, y + 136, c["accent"])[0])
        for j, line in enumerate(lines):
            assert width_of("Inter-Regular.ttf", line, 14) < w - 48, line
            body.append(text("Inter-Regular.ttf", line, 14, x + 24, y + 172 + j * 20, c["muted"])[0])
    return frame(c, PAD * 2 + h + 20, body, "Install Tonal on Linux, Windows, macOS or Arch Linux", glow=(0.08, 0.0))


KEYS = [
    (["Space"], "Play or pause"),
    (["Ctrl", "F"], "Search"),
    (["Ctrl", "L"], "Like the song"),
    (["Ctrl", "←", "→"], "Previous, next"),
    (["←", "→"], "Seek 5 seconds"),
    (["Ctrl", "↑", "↓"], "Volume"),
    (["Ctrl", "N"], "Now playing"),
    (["Ctrl", "M"], "Mini player"),
    (["Alt", "←", "→"], "Back, forward"),
]


def keycap(c, label, x, y):
    w = max(40, width_of("Inter-SemiBold.ttf", label, 15) + 24)
    return [
        f'<rect x="{x:.1f}" y="{y + 3}" width="{w:.1f}" height="40" rx="10" fill="{c["line"]}"/>',
        f'<rect x="{x:.1f}" y="{y}" width="{w:.1f}" height="40" rx="10" fill="{c["tile"]}" stroke="{c["line"]}"/>',
        text("Inter-SemiBold.ttf", label, 15, x + w / 2, y + 26, c["text"], "middle")[0],
    ], w


def keys(c):
    cols, gap, h = 3, 16, 76
    w = (W - 2 * PAD - gap * (cols - 1)) / cols
    body = []
    for i, (caps, action) in enumerate(KEYS):
        x, y = PAD + (i % cols) * (w + gap), PAD + (i // cols) * (h + gap)
        body.append(tile(c, x, y, w, h, 18))
        cx = x + 20
        for cap in caps:
            part, cw = keycap(c, cap, cx, y + 16)
            body += part
            cx += cw + 6
        body.append(text("Inter-Medium.ttf", action, 16, x + w - 22, y + 43, c["muted"], "end")[0])
    rows = math.ceil(len(KEYS) / cols)
    return frame(c, PAD * 2 + rows * h + (rows - 1) * gap + 4, body, "Keyboard shortcuts", glow=(0.5, 0.0), waves=False)


FLOW = [
    ("search", "Sources", "YouTube Music, SoundCloud,", "Audius, local files", "src/sources"),
    ("play", "Player", "One mpv process over IPC,", "gapless and crossfade", "src/player"),
    ("equalizer", "Visualizer", "Hears Tonal's own audio,", "FFT into smooth waves", "src/visualizer"),
    ("sparkle", "Interface", "Native Slint window,", "no browser inside", "ui"),
]
BELOW = [
    (0, "import", "Integrations", "Spotify import, LRCLIB lyrics,", "Last.fm, ListenBrainz", "src/integrations"),
    (3, "album", "Library", "SQLite: tracks, likes,", "playlists, history, session", "src/library"),
]


def block(c, x, y, w, h, name, title, one, two, path):
    body = [tile(c, x, y, w, h)]
    body += chip(c, name, x + 22, y + 22, 40)
    body.append(text("Inter-SemiBold.ttf", title, 19, x + 76, y + 49, c["text"])[0])
    body.append(text("Inter-Regular.ttf", one, 14, x + 22, y + 92, c["muted"])[0])
    body.append(text("Inter-Regular.ttf", two, 14, x + 22, y + 112, c["muted"])[0])
    pw = width_of("Inter-Medium.ttf", path, 12)
    body.append(f'<rect x="{x + 22:.1f}" y="{y + 128}" width="{pw + 16:.1f}" height="22" rx="7" fill="{c["track"]}"/>')
    body.append(text("Inter-Medium.ttf", path, 12, x + 30, y + 143, c["accent"])[0])
    return body


def arrow(c, x1, y1, x2, y2, dashed=False):
    dash = ' stroke-dasharray="5 5"' if dashed else ""
    line = f'<path d="M{x1:.1f},{y1:.1f} L{x2:.1f},{y2:.1f}" stroke="{c["accent"]}" stroke-width="2" stroke-linecap="round"{dash} fill="none" stroke-opacity="0.8"/>'
    angle = math.atan2(y2 - y1, x2 - x1)
    tip = [
        (x2 - 9 * math.cos(angle - 0.5), y2 - 9 * math.sin(angle - 0.5)),
        (x2, y2),
        (x2 - 9 * math.cos(angle + 0.5), y2 - 9 * math.sin(angle + 0.5)),
    ]
    head = "M" + " L".join(f"{a:.1f},{b:.1f}" for a, b in tip)
    return [line, f'<path d="{head}" stroke="{c["accent"]}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>']


def flow(c):
    gap, h = 48, 166
    w = (W - 2 * PAD - gap * (len(FLOW) - 1)) / len(FLOW)
    body, top, low = [], PAD, PAD + h + 56
    for i, item in enumerate(FLOW):
        x = PAD + i * (w + gap)
        body += block(c, x, top, w, h, *item)
        if i:
            body += arrow(c, x - gap + 8, top + h / 2, x - 8, top + h / 2)
    for col, *item in BELOW:
        x = PAD + col * (w + gap)
        body += block(c, x, low, w, h, *item)
        body += arrow(c, x + w / 2, low - 8, x + w / 2, top + h + 8, dashed=True)
        if col == 3:
            body += arrow(c, x + w / 2 - 30, top + h + 8, x + w / 2 - 30, low - 8, dashed=True)
    return frame(c, low + h + PAD + 20, body, "How Tonal fits together", glow=(0.5, 1.0))


CARDS = {"features": features, "install": install, "keys": keys, "how": flow}

if __name__ == "__main__":
    for name, draw in CARDS.items():
        for look, c in LOOKS.items():
            (ROOT / "docs" / "images" / f"{name}-{look}.svg").write_text(draw(c))
