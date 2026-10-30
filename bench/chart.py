#!/usr/bin/env python3
"""Draws bench/results.json as docs/images/benchmark-{dark,light}.svg."""

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RESULTS = json.loads((ROOT / "bench/results.json").read_text())
# Spotifast is not measured here: playback needs Spotify Premium. This is the range
# its README states.
CLAIMED = {"Spotifast": (100, 250, "their claim, needs Premium")}

THEMES = {
    "dark": {"text": "#f3f3f5", "muted": "#a2a2a8", "faint": "#3a3a40", "bar": "#6e6e74", "accent": "#1ed760"},
    "light": {"text": "#1f2328", "muted": "#59636e", "faint": "#d8dee4", "bar": "#9aa1a9", "accent": "#1a7f37"},
}

W, LABEL, ROW, TOP = 820, 190, 30, 64


def panel(theme, title, unit, rows, y0, maximum, fmt):
    c = THEMES[theme]
    out = [f'<text x="0" y="{y0}" font-size="15" font-weight="600" fill="{c["text"]}">{title}</text>']
    span = W - LABEL - 90
    for i, (name, value, note) in enumerate(rows):
        y = y0 + 18 + i * ROW
        tonal = name.startswith("Tonal")
        colour = c["accent"] if tonal else c["bar"]
        weight = 600 if tonal else 400
        out.append(f'<text x="{LABEL - 12}" y="{y + 15}" font-size="13" text-anchor="end" font-weight="{weight}" fill="{c["text"]}">{name}</text>')
        out.append(f'<rect x="{LABEL}" y="{y + 3}" width="{span}" height="16" rx="4" fill="{c["faint"]}" opacity="0.35"/>')
        if isinstance(value, tuple):
            lo, hi = value
            x0, x1 = LABEL + span * lo / maximum, LABEL + span * hi / maximum
            out.append(f'<rect x="{x0:.1f}" y="{y + 3}" width="{x1 - x0:.1f}" height="16" rx="4" fill="none" stroke="{c["bar"]}" stroke-dasharray="4 3"/>')
            label = f"{lo}–{hi} {unit} ({note})"
            out.append(f'<text x="{x1 + 8:.1f}" y="{y + 15}" font-size="12" fill="{c["muted"]}">{label}</text>')
            continue
        width = max(2.0, span * value / maximum)
        out.append(f'<rect x="{LABEL}" y="{y + 3}" width="{width:.1f}" height="16" rx="4" fill="{colour}"/>')
        out.append(f'<text x="{LABEL + width + 8:.1f}" y="{y + 15}" font-size="12" font-weight="{weight}" fill="{c["text"] if tonal else c["muted"]}">{fmt(value)} {unit}</text>')
    return out, y0 + 18 + len(rows) * ROW


def draw(theme):
    c = THEMES[theme]
    memory = sorted(((n, r["playing"]["mb"], "") for n, r in RESULTS.items()), key=lambda r: r[1])
    for name, (lo, hi, note) in CLAIMED.items():
        memory.append((name, (lo, hi), note))
    cpu = sorted(((n, r["playing"]["cpu"], "") for n, r in RESULTS.items()), key=lambda r: r[1])
    top = max(r[1] for r in memory if not isinstance(r[1], tuple))
    body = [
        f'<text x="0" y="22" font-size="19" font-weight="700" fill="{c["text"]}">Music players while playing a song</text>',
        f'<text x="0" y="44" font-size="12.5" fill="{c["muted"]}">Lower is better. Memory is PSS over every process the app starts; CPU is share of one core. Linux, Ryzen 5 7600.</text>',
    ]
    part, y = panel(theme, "Memory", "MB", memory, TOP + 18, top * 1.05, lambda v: f"{v:.0f}")
    body += part
    part, y = panel(theme, "CPU", "%", cpu, y + 34, max(r[1] for r in cpu) * 1.05, lambda v: f"{v:.1f}")
    body += part
    height = y + 12
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{height}" viewBox="0 0 {W} {height}" '
        f'font-family="Inter, -apple-system, Segoe UI, Helvetica, Arial, sans-serif">\n'
        + "\n".join(body)
        + "\n</svg>\n"
    )


for name in THEMES:
    (ROOT / f"docs/images/benchmark-{name}.svg").write_text(draw(name))
