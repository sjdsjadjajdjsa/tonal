// Tonal's living themes, painted by the app's own scene code (src/scenes) compiled to
// WebAssembly in scenes.wasm (built by site/wasm/build.sh). The gallery, palettes and
// painters are the app's, so every theme here looks the way it does in Tonal.

const url = new URL("./scenes.wasm" + new URL(import.meta.url).search, import.meta.url);
const { instance } = await WebAssembly.instantiateStreaming(fetch(url)).catch(async () =>
  WebAssembly.instantiate(await (await fetch(url)).arrayBuffer()),
);
const wasm = instance.exports;
const text = (ptr, len) => new TextDecoder().decode(new Uint8Array(wasm.memory.buffer, ptr, len));

export const THEMES = JSON.parse(text(wasm.info(), wasm.info_len())).map((t, index) => ({
  ...t,
  index,
  scene: t.scene === "Classic" ? null : t.scene,
}));

/// Paints theme `t` into a 2D canvas context, sized to what the app would paint for `w` by `h`.
function draw(ctx, t, w, h, time, [bass, high, energy] = [0, 0, 0]) {
  const ptr = wasm.paint(t.index, w, h, time, bass, high, energy);
  if (!ptr) return;
  const cw = wasm.width(), ch = wasm.height();
  if (ctx.canvas.width !== cw || ctx.canvas.height !== ch) {
    ctx.canvas.width = cw;
    ctx.canvas.height = ch;
  }
  ctx.putImageData(new ImageData(new Uint8ClampedArray(wasm.memory.buffer, ptr, cw * ch * 4).slice(), cw, ch), 0, 0);
}

/// Sets one of the five colours of "Your colors", darkest first, as "#rrggbb".
export function setCustom(slot, hex) {
  wasm.set_custom(slot, parseInt(hex.slice(1), 16));
}

/// A still card picture for theme `t`, the way the app's gallery shows it.
export function preview(canvas, t) {
  draw(canvas.getContext("2d"), t, 480, 300, 24);
}

/// Paints the chosen theme on `canvas` while `running()` says so. `levels()` gives the
/// music's bass, highs and energy from 0 to 1, for the theme's reaction to it.
export function paint(canvas, getTheme, running, levels) {
  const ctx = canvas.getContext("2d");
  let t = 0, last = 0, drawn = -1, shown = null;
  let w = 0, h = 0;
  const size = () => {
    const dpr = Math.min(2, devicePixelRatio || 1);
    w = Math.max(1, Math.round(canvas.clientWidth * dpr));
    h = Math.max(1, Math.round(canvas.clientHeight * dpr));
    drawn = -1;
  };
  new ResizeObserver(size).observe(canvas);
  size();
  const frame = (ms) => {
    requestAnimationFrame(frame);
    const theme = getTheme();
    if (!theme.scene || document.hidden) return;
    const moving = running();
    if (moving && ms - last < 1000 / 30) return;
    const dt = Math.min(0.1, (ms - last) / 1000);
    last = ms;
    if (moving) t += dt;
    else if (drawn === t && shown === theme) return;
    draw(ctx, theme, w, h, t, moving ? levels() : undefined);
    drawn = t;
    shown = theme;
  };
  requestAnimationFrame(frame);
  return () => (drawn = -1);
}
