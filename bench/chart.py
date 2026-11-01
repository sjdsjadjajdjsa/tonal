#!/usr/bin/env python3
"""Draws bench/results.json as docs/images/benchmark-{dark,light}.svg (needs fonttools)."""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "docs"))
from banner import text_path, wave  # noqa: E402

RESULTS = json.loads((ROOT / "bench/results.json").read_text())
# Spotifast is not measured here: playback needs Spotify Premium. This is the range
# its README states.
CLAIMED = {"Spotifast": (100, 250, "their claim, needs Premium")}

LOOKS = {
    "dark": dict(
        bg="#0e0f12", text="#ececee", muted="#8d8d96", track="#1b1c21", bar="#3b3c44",
        accent="#1ed760", accent2="#0fa34a", tile="#15161a", line="#26272d", glow="0.16",
    ),
    "light": dict(
        bg="#f5f6f7", text="#111113", muted="#6b6b74", track="#e6e8eb", bar="#c3c6cc",
        accent="#13a64b", accent2="#0b7d37", tile="#ffffff", line="#e1e3e6", glow="0.10",
    ),
}

W, PAD, ROW, BAR = 1280, 56, 38, 14
COL = (W - 2 * PAD - 48) / 2
LABEL = 170


def text(font, words, size, x, y, fill, anchor="start", opacity=1.0, tracking=0.0):
    d, width = text_path(font, words, size, 0, 0, tracking)
    if anchor == "end":
        x -= width
    elif anchor == "middle":
        x -= width / 2
    alpha = f' fill-opacity="{opacity}"' if opacity < 1 else ""
    return f'<path transform="translate({x:.1f} {y:.1f})" d="{d}" fill="{fill}"{alpha}/>', width


def panel(c, x0, y0, title, unit, rows, maximum, fmt):
    out = [text("Inter-SemiBold.ttf", title, 17, x0, y0, c["text"])[0]]
    out.append(text("Inter-Regular.ttf", "lower is better", 13, x0 + COL, y0, c["muted"], "end")[0])
    span = COL - LABEL - 120
    for i, (name, value, note) in enumerate(rows):
        y = y0 + 26 + i * ROW
        mid = y + ROW / 2
        tonal = name.startswith("Tonal")
        font = "Inter-SemiBold.ttf" if tonal else "Inter-Regular.ttf"
        out.append(text(font, name, 14, x0 + LABEL - 16, mid + 5, c["text"] if tonal else c["muted"], "end")[0])
        bx, by = x0 + LABEL, mid - BAR / 2
        out.append(f'<rect x="{bx}" y="{by}" width="{span}" height="{BAR}" rx="{BAR / 2}" fill="{c["track"]}"/>')
        if isinstance(value, tuple):
            lo, hi = value
            a, b = bx + span * lo / maximum, bx + span * hi / maximum
            out.append(
                f'<rect x="{a:.1f}" y="{by + 0.5}" width="{b - a:.1f}" height="{BAR - 1}" rx="{BAR / 2}" '
                f'fill="{c["bar"]}" fill-opacity="0.25" stroke="{c["muted"]}" stroke-width="1.5" stroke-dasharray="5 4"/>'
            )
            out.append(text("Inter-Regular.ttf", f"{lo}–{hi} {unit}  ·  {note}", 13, b + 10, mid + 5, c["muted"])[0])
            continue
        width = max(BAR, span * value / maximum)
        fill = "url(#tonal)" if tonal else c["bar"]
        glow = ' filter="url(#soft)"' if tonal else ""
        if tonal:
            out.append(f'<rect x="{bx}" y="{by}" width="{width:.1f}" height="{BAR}" rx="{BAR / 2}" fill="{c["accent"]}" opacity="0.5"{glow}/>')
        out.append(f'<rect x="{bx}" y="{by}" width="{width:.1f}" height="{BAR}" rx="{BAR / 2}" fill="{fill}"/>')
        out.append(
            text(font, f"{fmt(value)}{'' if unit == '%' else ' '}{unit}", 13.5, bx + width + 10, mid + 5, c["accent"] if tonal else c["muted"])[0]
        )
    return out, y0 + 26 + len(rows) * ROW


def tiles(c, y, facts):
    out, gap = [], 16
    width = (W - 2 * PAD - gap * (len(facts) - 1)) / len(facts)
    for i, (big, small) in enumerate(facts):
        x = PAD + i * (width + gap)
        out.append(f'<rect x="{x:.1f}" y="{y}" width="{width:.1f}" height="92" rx="18" fill="{c["tile"]}" stroke="{c["line"]}"/>')
        out.append(text("Inter-Bold.ttf", big, 34, x + 24, y + 48, c["accent"], tracking=-0.02)[0])
        out.append(text("Inter-Regular.ttf", small, 14, x + 24, y + 74, c["muted"])[0])
    return out, y + 92


def facts():
    playing = {n: r["playing"] for n, r in RESULTS.items()}
    lean = playing["Tonal (waves off)"]
    others = {n: p for n, p in playing.items() if not n.startswith("Tonal")}
    heaviest = max(others, key=lambda n: others[n]["mb"])
    out = [
        (f"{others[heaviest]['mb'] / lean['mb']:.0f}× less", f"memory than {heaviest} while playing"),
        (f"{lean['cpu']:.1f}% CPU", "playing a song with the waves off"),
    ]
    if all(lean["mb"] < p["mb"] and lean["cpu"] < p["cpu"] for p in others.values()):
        out.append((f"{lean['mb']:.0f} MB", f"lightest of the {len(playing) - 2} players measured"))
    return out


def draw(look):
    c = LOOKS[look]
    memory = sorted(((n, r["playing"]["mb"], "") for n, r in RESULTS.items()), key=lambda r: r[1])
    memory += [(n, (lo, hi), note) for n, (lo, hi, note) in CLAIMED.items()]
    cpu = sorted(((n, r["playing"]["cpu"], "") for n, r in RESULTS.items()), key=lambda r: r[1])
    body = [
        text("Inter-Bold.ttf", "How light is Tonal?", 40, PAD, PAD + 38, c["text"], tracking=-0.025)[0],
        text(
            "Inter-Regular.ttf",
            "Music players playing the same song, measured the same way by bench/bench.py. Arch Linux, Ryzen 5 7600.",
            15, PAD, PAD + 70, c["muted"],
        )[0],
    ]
    part, y = tiles(c, PAD + 100, facts())
    body += part
    top = max(r[1] for r in memory if not isinstance(r[1], tuple))
    left, ly = panel(c, PAD, y + 60, "Memory", "MB", memory, top * 1.02, lambda v: f"{v:.0f}")
    right, ry = panel(c, PAD + COL + 48, y + 60, "CPU", "%", cpu, max(r[1] for r in cpu) * 1.02, lambda v: f"{v:.1f}")
    body += left + right
    foot = max(ly, ry) + 28
    body.append(
        text(
            "Inter-Regular.ttf",
            "Memory is PSS over every process the app starts, so shared libraries count once. CPU is the share of one core.",
            13, PAD, foot, c["muted"], opacity=0.85,
        )[0]
    )
    height = int(foot + 70)
    ripple = wave(900, 10, height - 8, [(1, 1.0, 0.0), (2, 0.4, 1.1)], W + 900).replace(" L", " L").replace(
        f"M0,360", f"M0,{height}"
    ).replace(f",360 Z", f",{height} Z")
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {height}" width="{W}" height="{height}" role="img" aria-label="Memory and CPU of music players while playing a song">
<defs>
<clipPath id="card"><rect width="{W}" height="{height}" rx="28"/></clipPath>
<radialGradient id="glow" cx="0.92" cy="0" r="0.6"><stop offset="0" stop-color="{c["accent"]}" stop-opacity="{c["glow"]}"/><stop offset="1" stop-color="{c["accent"]}" stop-opacity="0"/></radialGradient>
<linearGradient id="tonal" x1="0" x2="1"><stop offset="0" stop-color="{c["accent2"]}"/><stop offset="1" stop-color="{c["accent"]}"/></linearGradient>
<filter id="soft" x="-50%" y="-200%" width="200%" height="500%"><feGaussianBlur stdDeviation="5"/></filter>
</defs>
<g clip-path="url(#card)">
<rect width="{W}" height="{height}" fill="{c["bg"]}"/>
<rect width="{W}" height="{height}" fill="url(#glow)"/>
<path d="{ripple}" fill="{c["accent"]}" fill-opacity="0.08"><animateTransform attributeName="transform" type="translate" from="0 0" to="-900 0" dur="30s" repeatCount="indefinite"/></path>
</g>
{chr(10).join(body)}
</svg>
"""


for look in LOOKS:
    (ROOT / f"docs/images/benchmark-{look}.svg").write_text(draw(look))
