#!/usr/bin/env python3
"""Draws bench/results.json as docs/images/benchmark-{dark,light}.svg (needs fonttools)."""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "docs"))
from art import LOOKS, PAD, W, frame, text, tile  # noqa: E402

RESULTS = json.loads((ROOT / "bench/results.json").read_text())
# Spotifast is not measured here: playback needs Spotify Premium. This is the range
# its README states.
CLAIMED = {"Spotifast": (100, 250, "their claim, needs Premium")}

ROW, BAR = 38, 14
COL = (W - 2 * PAD - 48) / 2
LABEL = 170


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
        out.append(tile(c, x, y, width, 92, 18))
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
    defs = (
        f'<linearGradient id="tonal" x1="0" x2="1"><stop offset="0" stop-color="{c["accent2"]}"/><stop offset="1" stop-color="{c["accent"]}"/></linearGradient>'
        '<filter id="soft" x="-50%" y="-200%" width="200%" height="500%"><feGaussianBlur stdDeviation="5"/></filter>'
    )
    return frame(c, height, body, "Memory and CPU of music players while playing a song", defs=defs)


for look in LOOKS:
    (ROOT / f"docs/images/benchmark-{look}.svg").write_text(draw(look))
