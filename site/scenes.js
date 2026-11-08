// Tonal's living themes, painted on a canvas behind the window. Names, scenes and
// palettes come from src/scenes/mod.rs (gallery and PALETTES); the painters are
// smaller versions of the app's, drawn at three quarters resolution and at most 30
// times a second.

export const THEMES = [
  { name: "Classic", scene: null, colors: ["#0b0b0d", "#1b1b20", "#3c3c44", "#9a9aa4", "#ececee"] },
  { name: "Northern lights", scene: "aurora", colors: ["#030a14", "#0a1d33", "#134e5e", "#3ddc97", "#7cf5ff"] },
  { name: "Golden hour", scene: "dunes", colors: ["#1a0f12", "#5b2a2a", "#c4683a", "#f2b45a", "#ffd98a"] },
  { name: "Outrun", scene: "synthwave", colors: ["#0d0221", "#261447", "#ff2a6d", "#ffb86b", "#05d9e8"] },
  { name: "Sakura drift", scene: "petals", colors: ["#2b1b2e", "#6d4a6e", "#e7a3c0", "#ffd1e1", "#fff1f6"] },
  { name: "City rain", scene: "rain", colors: ["#05060d", "#11132a", "#2c2f6b", "#ff3cac", "#2be4ff"] },
  { name: "Deep space", scene: "nebula", colors: ["#05030c", "#1d0b3b", "#5a189a", "#e0469b", "#ffd2f0"] },
];

const rand = (seed) => () => {
  seed = (seed * 1664525 + 1013904223) >>> 0;
  return seed / 4294967296;
};

const alpha = (hex, a) => hex + Math.round(Math.max(0, Math.min(1, a)) * 255).toString(16).padStart(2, "0");

function stars(ctx, w, h, c, t, count, seed) {
  const r = rand(seed);
  for (let i = 0; i < count; i++) {
    const x = r() * w, y = r() * h * 0.7, s = r() * 1.2 + 0.3, p = r() * 6;
    ctx.fillStyle = alpha(c, 0.25 + 0.35 * (0.5 + 0.5 * Math.sin(t * 1.3 + p)));
    ctx.fillRect(x, y, s, s);
  }
}

const painters = {
  aurora(ctx, w, h, c, t, level) {
    const sky = ctx.createLinearGradient(0, 0, 0, h);
    sky.addColorStop(0, c[0]);
    sky.addColorStop(1, c[1]);
    ctx.fillStyle = sky;
    ctx.fillRect(0, 0, w, h);
    stars(ctx, w, h, c[4], t, 90, 7);
    ctx.globalCompositeOperation = "lighter";
    for (let k = 0; k < 3; k++) {
      const color = k === 1 ? c[4] : c[3];
      for (let x = -20; x < w + 20; x += 3) {
        const u = x / w;
        const y = h * (0.22 + 0.08 * k) + Math.sin(u * 5 + t * 0.25 + k * 2) * h * 0.06 + Math.sin(u * 13 - t * 0.4 + k) * h * 0.02;
        const len = h * (0.22 + 0.1 * Math.sin(u * 7 + t * 0.3 + k));
        const a = (0.05 + 0.05 * Math.sin(u * 9 + t * 0.5 + k * 3)) * (1 + level * 0.8);
        const g = ctx.createLinearGradient(0, y, 0, y + len);
        g.addColorStop(0, alpha(color, 0));
        g.addColorStop(0.25, alpha(color, a));
        g.addColorStop(1, alpha(color, 0));
        ctx.fillStyle = g;
        ctx.fillRect(x, y, 3, len);
      }
    }
    ctx.globalCompositeOperation = "source-over";
    ctx.fillStyle = c[0];
    ctx.beginPath();
    ctx.moveTo(0, h);
    for (let x = 0; x <= w; x += 8) ctx.lineTo(x, h * 0.8 - Math.abs(Math.sin(x * 0.011) * h * 0.08) - Math.sin(x * 0.031) * h * 0.02);
    ctx.lineTo(w, h);
    ctx.fill();
  },

  dunes(ctx, w, h, c, t, level) {
    const sky = ctx.createLinearGradient(0, 0, 0, h * 0.6);
    sky.addColorStop(0, c[1]);
    sky.addColorStop(1, c[3]);
    ctx.fillStyle = sky;
    ctx.fillRect(0, 0, w, h);
    const sx = w * 0.68, sy = h * 0.42, sr = h * (0.09 + level * 0.01);
    const glow = ctx.createRadialGradient(sx, sy, 0, sx, sy, sr * 5);
    glow.addColorStop(0, alpha(c[4], 0.55));
    glow.addColorStop(1, alpha(c[4], 0));
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, w, h);
    ctx.fillStyle = c[4];
    ctx.beginPath();
    ctx.arc(sx, sy, sr, 0, Math.PI * 2);
    ctx.fill();
    const layers = [c[3], c[2], c[1], c[0]];
    layers.forEach((color, k) => {
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.moveTo(0, h);
      for (let x = 0; x <= w; x += 6) {
        const u = x / w;
        ctx.lineTo(x, h * (0.5 + k * 0.12) - Math.sin(u * (2.2 + k) + k * 1.7 + t * 0.03 * (k + 1)) * h * (0.06 - k * 0.008));
      }
      ctx.lineTo(w, h);
      ctx.fill();
    });
  },

  synthwave(ctx, w, h, c, t, level) {
    const horizon = h * 0.58;
    const sky = ctx.createLinearGradient(0, 0, 0, horizon);
    sky.addColorStop(0, c[0]);
    sky.addColorStop(0.7, c[1]);
    sky.addColorStop(1, alpha(c[2], 0.6));
    ctx.fillStyle = sky;
    ctx.fillRect(0, 0, w, h);
    stars(ctx, w, horizon, c[4], t, 50, 3);
    const sx = w / 2, sr = h * (0.2 + level * 0.015), sy = horizon - sr * 0.35;
    const sun = ctx.createLinearGradient(0, sy - sr, 0, sy + sr);
    sun.addColorStop(0, c[3]);
    sun.addColorStop(1, c[2]);
    ctx.save();
    ctx.beginPath();
    ctx.arc(sx, sy, sr, 0, Math.PI * 2);
    ctx.clip();
    ctx.fillStyle = sun;
    ctx.fillRect(sx - sr, sy - sr, sr * 2, sr * 2);
    ctx.fillStyle = c[1];
    for (let i = 0; i < 7; i++) {
      const y = sy + i * sr * 0.14;
      ctx.fillRect(sx - sr, y, sr * 2, 1.5 + i * 1.2);
    }
    ctx.restore();
    ctx.fillStyle = c[0];
    ctx.fillRect(0, horizon, w, h - horizon);
    ctx.strokeStyle = alpha(c[4], 0.55 + level * 0.3);
    ctx.lineWidth = 1;
    for (let i = -14; i <= 14; i++) {
      ctx.beginPath();
      ctx.moveTo(sx + i * w * 0.02, horizon);
      ctx.lineTo(sx + i * w * 0.2, h);
      ctx.stroke();
    }
    const shift = (t * 0.35) % 1;
    for (let i = 0; i < 12; i++) {
      const z = (i + shift) / 12;
      const y = horizon + (h - horizon) * z * z;
      ctx.globalAlpha = z;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  },

  petals(ctx, w, h, c, t, level) {
    const bg = ctx.createLinearGradient(0, 0, w, h);
    bg.addColorStop(0, c[1]);
    bg.addColorStop(1, c[0]);
    ctx.fillStyle = bg;
    ctx.fillRect(0, 0, w, h);
    for (let k = 0; k < 3; k++) {
      const x = w * (0.2 + 0.3 * k) + Math.sin(t * 0.1 + k) * w * 0.05, y = h * (0.3 + 0.2 * (k % 2));
      const g = ctx.createRadialGradient(x, y, 0, x, y, h * 0.5);
      g.addColorStop(0, alpha(c[2], 0.28));
      g.addColorStop(1, alpha(c[2], 0));
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, w, h);
    }
    const r = rand(12);
    for (let i = 0; i < 46; i++) {
      const speed = 0.02 + r() * 0.03, sway = r() * 6, size = 3 + r() * 5;
      const y = ((r() + t * speed) % 1.1 - 0.05) * h;
      const x = (r() * w + Math.sin(t * 0.6 + sway) * 30 + t * 8) % (w + 20) - 10;
      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(t * (0.5 + r()) + sway);
      ctx.fillStyle = alpha(r() > 0.5 ? c[3] : c[4], 0.7 + level * 0.3);
      ctx.beginPath();
      ctx.ellipse(0, 0, size, size * 0.55, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }
  },

  rain(ctx, w, h, c, t, level) {
    const bg = ctx.createLinearGradient(0, 0, 0, h);
    bg.addColorStop(0, c[0]);
    bg.addColorStop(1, c[1]);
    ctx.fillStyle = bg;
    ctx.fillRect(0, 0, w, h);
    const r = rand(19);
    for (let i = 0; i < 26; i++) {
      const x = r() * w, y = r() * h * 0.7, s = 20 + r() * 60;
      const g = ctx.createRadialGradient(x, y, 0, x, y, s);
      const color = r() > 0.5 ? c[3] : c[4];
      g.addColorStop(0, alpha(color, 0.12 + level * 0.1));
      g.addColorStop(1, alpha(color, 0));
      ctx.fillStyle = g;
      ctx.fillRect(x - s, y - s, s * 2, s * 2);
    }
    let x = 0;
    while (x < w) {
      const bw = 30 + r() * 70, bh = h * (0.2 + r() * 0.35);
      ctx.fillStyle = r() > 0.5 ? c[1] : c[0];
      ctx.fillRect(x, h - bh, bw, bh);
      for (let wy = h - bh + 8; wy < h - 6; wy += 9) {
        for (let wx = x + 5; wx < x + bw - 5; wx += 8) {
          if (r() > 0.78) {
            ctx.fillStyle = alpha(r() > 0.6 ? c[4] : c[3], 0.5 + 0.3 * Math.sin(t + wx));
            ctx.fillRect(wx, wy, 3, 4);
          }
        }
      }
      x += bw + 2;
    }
    ctx.strokeStyle = alpha(c[4], 0.25);
    ctx.lineWidth = 1;
    const drops = rand(4);
    for (let i = 0; i < 140; i++) {
      const dx = drops() * w, speed = 0.6 + drops() * 0.6;
      const dy = ((drops() + t * speed) % 1) * h;
      ctx.beginPath();
      ctx.moveTo(dx, dy);
      ctx.lineTo(dx - 3, dy + 14);
      ctx.stroke();
    }
  },

  nebula(ctx, w, h, c, t, level) {
    ctx.fillStyle = c[0];
    ctx.fillRect(0, 0, w, h);
    ctx.globalCompositeOperation = "lighter";
    const blobs = [[0.3, 0.35, c[2], 0.5], [0.65, 0.55, c[3], 0.42], [0.5, 0.25, c[1], 0.6], [0.75, 0.3, c[2], 0.35]];
    blobs.forEach(([bx, by, color, size], k) => {
      const x = w * bx + Math.sin(t * 0.07 + k) * w * 0.06, y = h * by + Math.cos(t * 0.05 + k * 2) * h * 0.05;
      const g = ctx.createRadialGradient(x, y, 0, x, y, h * size);
      g.addColorStop(0, alpha(color, 0.38 + level * 0.15));
      g.addColorStop(1, alpha(color, 0));
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, w, h);
    });
    ctx.globalCompositeOperation = "source-over";
    stars(ctx, w, h / 0.7, c[4], t, 160, 6);
  },
};

/// Paints `theme` on `canvas` while `running()` says so; `level()` is the music's
/// loudness from 0 to 1, for the Pulse reaction.
export function paint(canvas, getTheme, running, level) {
  const ctx = canvas.getContext("2d");
  let w = 0, h = 0, t = 0, last = 0, drawn = -1;
  const size = () => {
    w = Math.max(1, Math.round(canvas.clientWidth * 0.75));
    h = Math.max(1, Math.round(canvas.clientHeight * 0.75));
    canvas.width = w;
    canvas.height = h;
    drawn = -1;
  };
  new ResizeObserver(size).observe(canvas);
  size();
  const frame = (ms) => {
    requestAnimationFrame(frame);
    const theme = getTheme();
    if (!theme.scene || document.hidden) return;
    const moving = running();
    if (ms - last < 33 && moving) return;
    const dt = Math.min(0.1, (ms - last) / 1000);
    last = ms;
    if (moving) t += dt;
    else if (drawn === t) return;
    painters[theme.scene](ctx, w, h, theme.colors, t, moving ? level() : 0);
    drawn = t;
  };
  requestAnimationFrame(frame);
  return () => (drawn = -1);
}
