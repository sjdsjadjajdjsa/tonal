import { THEMES, paint, preview as themePreview, setCustom } from "./scenes.js";
import { createMusic, DURATION, LYRICS } from "./music.js";

const PROJECT = "https://gitlab.com/api/v4/projects/sjdsjadjajdjsa%2Ftonal";
const FILES = "https://gitlab.com/sjdsjadjajdjsa/tonal/-/releases/permalink/latest/downloads/";
const VIEWS = ["home", "equalizer", "themes", "light", "download"];
const TITLES = { home: "Home", equalizer: "Equalizer", themes: "Themes", light: "Light", download: "Download" };
const PRESETS = [
  ["Flat", [0, 0, 0, 0, 0, 0]],
  ["Bass", [10, 7, 2, -1, 0, 0]],
  ["Vocal", [-5, -3, 2, 6, 5, -1]],
  ["Bright", [-2, -1, 0, 2, 7, 10]],
  ["Warm", [6, 4, 2, 0, -3, -6]],
  ["Late night", [4, 2, 0, -2, -5, -8]],
];

/// Where the site lives: "/" on its own domain, "/tonal/" on GitLab's address.
const BASE = document.querySelector('meta[name="base"]')?.content || "/";
const $ = (s, root = document) => root.querySelector(s);
const $$ = (s, root = document) => [...root.querySelectorAll(s)];
const store = {
  get: (k, d) => { try { return localStorage.getItem(k) ?? d; } catch { return d; } },
  set: (k, v) => { try { localStorage.setItem(k, v); } catch {} },
};
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
let motion = store.get("tonal-motion", reduced ? "off" : "on") === "on";
const music = createMusic();
const panel = $(".panel");

/* The visitor's system: the title bar, the download button and the shortcut key. */
function system() {
  const p = (navigator.userAgentData?.platform || navigator.platform || navigator.userAgent).toLowerCase();
  if (/mac/.test(p)) return "mac";
  if (/win/.test(p)) return "windows";
  if (/linux|x11/.test(p) && !/android/.test(navigator.userAgent.toLowerCase())) return "linux";
  return "other";
}
const os = system();
document.body.dataset.os = os;
$$("[data-mod]").forEach((k) => (k.textContent = os === "mac" ? "⌘K" : "Ctrl K"));
const pick = { mac: ["Download for Mac", "tonal-macos-arm64.dmg"], windows: ["Download for Windows", "tonal-setup-x86_64.exe"], linux: ["Download for Linux", "tonal-x86_64.AppImage"] }[os];
if (pick) {
  const link = $("[data-download]");
  link.href = FILES + pick[1];
  $("[data-download-label]", link).textContent = pick[0];
  const card = $(`.group-card[data-os="${os}"]`);
  card?.classList.add("mine");
  card?.parentElement.prepend(card);
}

/* Pages live at real addresses (/equalizer/ and so on) and swap like Tonal's do. */
let current = "home";
const viewOf = (path) => {
  const name = (path.startsWith(BASE) ? path.slice(BASE.length) : path).replace(/^\/|\/$/g, "") || "home";
  return VIEWS.includes(name) ? name : "home";
};
function show(name, enter = true) {
  for (const view of $$(".view")) {
    const on = view.dataset.view === name;
    view.hidden = !on;
    view.classList.toggle("entering", on && enter && motion);
  }
  for (const link of $$(".nav[data-view]")) link.classList.toggle("active", link.dataset.view === name);
  document.title = name === "home" ? "Tonal, a music player that moves with your music" : `${TITLES[name]}, Tonal`;
  panel.scrollTop = 0;
  current = name;
}
function go(name) {
  closeNow();
  if (name === current) return;
  history.pushState({}, "", name === "home" ? BASE : `${BASE}${name}/`);
  show(name);
  music.sound("tap");
}
document.addEventListener("click", (e) => {
  const a = e.target.closest("a[href]");
  if (!a || e.metaKey || e.ctrlKey || e.shiftKey || a.target) return;
  const url = new URL(a.href);
  if (url.origin !== location.origin || !url.pathname.startsWith(BASE)) return;
  const rest = url.pathname.slice(BASE.length).replace(/\//g, "");
  if (rest && !VIEWS.includes(rest)) return;
  e.preventDefault();
  go(viewOf(url.pathname));
});
addEventListener("popstate", () => show(viewOf(location.pathname)));
const step = (by) => go(VIEWS[(VIEWS.indexOf(current) + by + VIEWS.length) % VIEWS.length]);
$$("[data-step]").forEach((b) => b.addEventListener("click", () => step(Number(b.dataset.step))));

/* Version and downloads from GitLab's latest release, asked at most once an hour. */
const cached = JSON.parse(store.get("tonal-release", "null") || "null");
(cached && Date.now() - cached.at < 3600e3
  ? Promise.resolve(cached.release)
  : fetch(`${PROJECT}/releases/permalink/latest`)
      .then((r) => (r.ok ? r.json() : null))
      .then((r) => {
        if (r) store.set("tonal-release", JSON.stringify({ at: Date.now(), release: { tag_name: r.tag_name, released_at: r.released_at } }));
        return r;
      }))
  .then((release) => {
    if (!release) return;
    const version = release.tag_name.replace(/^v/, "");
    const date = new Date(release.released_at).toLocaleDateString(undefined, { month: "long", day: "numeric", year: "numeric" });
    $("[data-version]").textContent = `Tonal ${version}`;
    $("[data-version-line]").textContent = `Version ${version}, released ${date}. Free and open source.`;
  })
  .catch(() => {});
$$("[data-copy]").forEach((button) => {
  button.addEventListener("click", async () => {
    await navigator.clipboard.writeText(button.dataset.copy);
    const label = $("span", button);
    label.textContent = "Copied";
    setTimeout(() => (label.textContent = "Copy"), 1600);
  });
});

/* Home's track list: hovering a row shows its screen in the big window. */
const preview = $("[data-preview]");
const heroShot = preview.getAttribute("src");
let swapTimer;
function previewShot(src) {
  if (preview.getAttribute("src") === src) return;
  clearTimeout(swapTimer);
  preview.classList.add("swap");
  swapTimer = setTimeout(() => {
    preview.src = src;
    preview.classList.remove("swap");
  }, motion ? 160 : 0);
}
for (const row of $$(".tracks li")) {
  row.addEventListener("pointerenter", () => {
    $$(".tracks li").forEach((r) => r.classList.toggle("on", r === row));
    previewShot(row.dataset.shot);
  });
}
$(".tracks").addEventListener("pointerleave", () => {
  $$(".tracks li").forEach((r) => r.classList.remove("on"));
  previewShot(heroShot);
});

/* Themes: the window becomes the chosen scene, and Tonal's accent follows it. */
let theme = THEMES.find((t) => t.name === store.get("tonal-theme", "Northern lights")) || THEMES[1];
const redraw = paint($(".scene"), () => theme, () => motion, () => {
  const [low, mid, high = mid] = music.levels();
  return [low, high, low * 0.6 + mid * 0.4];
});
function setTheme(next, sound = true) {
  theme = next;
  store.set("tonal-theme", next.name);
  const root = document.documentElement.style;
  next.colors.forEach((c, i) => root.setProperty(`--c${i}`, c));
  root.setProperty("--accent", next.scene ? next.accent : "#1ed760");
  document.body.classList.toggle("scened", Boolean(next.scene));
  $$(".theme-card").forEach((card) => card.setAttribute("aria-checked", String(card.dataset.name === next.name)));
  redraw();
  if (sound) music.sound("tap");
}
const themesEl = $(".themes");
for (const t of THEMES) {
  const card = document.createElement("button");
  card.type = "button";
  card.className = "theme-card";
  card.dataset.name = t.name;
  card.setAttribute("role", "radio");
  card.innerHTML = `<canvas></canvas><span><b></b><small></small></span>`;
  $("b", card).textContent = t.name;
  $("small", card).textContent = t.scene ? `${t.scene} · ${t.palette}` : "See-through";
  themePreview($("canvas", card), t);
  card.addEventListener("click", () => setTheme(t));
  themesEl.appendChild(card);
}
setTheme(theme, false);

/* Your colors: the app's custom palette, repainted as the visitor picks. */
const mine = THEMES.find((t) => t.palette === "Your colors");
const mixEl = $("[data-mix]");
if (mine && mixEl) {
  const saved = store.get("tonal-mix", "").split(",").filter((c) => /^#[0-9a-f]{6}$/i.test(c));
  const colors = saved.length === 5 ? saved : ["#140a1c", "#3d1450", "#b8326a", "#ff8a5c", "#ffe2b8"];
  const apply = () => {
    colors.forEach((c, i) => setCustom(i, c));
    mine.colors = colors.slice();
    mine.accent = colors[4];
    store.set("tonal-mix", colors.join(","));
    themePreview($(`.theme-card[data-name="${mine.name}"] canvas`), mine);
    if (theme === mine) setTheme(mine, false);
  };
  colors.forEach((c, i) => {
    const input = document.createElement("input");
    input.type = "color";
    input.value = c;
    input.setAttribute("aria-label", `Colour ${i + 1} of 5`);
    input.addEventListener("input", () => {
      colors[i] = input.value;
      apply();
      if (theme !== mine) setTheme(mine, false);
    });
    mixEl.appendChild(input);
  });
  apply();
}

function setMotion(on) {
  motion = on;
  store.set("tonal-motion", on ? "on" : "off");
  document.body.classList.toggle("still", !on);
  $("[data-motion]").setAttribute("aria-checked", String(on));
  redraw();
}
$("[data-motion]").addEventListener("click", () => setMotion(!motion));
setMotion(motion);

/* Playback: the real song, the time, the scrub bar and the like button. */
const play = $(".play");
const fmt = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
$("[data-total]").textContent = fmt(DURATION);
function syncPlay() {
  const on = music.playing;
  $("use", play).setAttribute("href", on ? "#i-pause" : "#i-play");
  play.setAttribute("aria-label", on ? "Pause" : "Play");
  document.body.classList.toggle("playing", on);
  $("[data-play-hint] span").textContent = on ? "Pause" : "Press play";
  $("[data-play-hint] use").setAttribute("href", on ? "#i-pause" : "#i-play");
}
function toggle() {
  music.playing ? music.pause() : music.play();
  syncPlay();
}
play.addEventListener("click", toggle);
$("[data-play-hint]").addEventListener("click", toggle);
const scrub = $(".scrub");
const seekAt = (x) => {
  const r = scrub.getBoundingClientRect();
  music.seek(Math.max(0, Math.min(1, (x - r.left) / r.width)) * DURATION);
};
scrub.addEventListener("pointerdown", (e) => {
  scrub.setPointerCapture(e.pointerId);
  seekAt(e.clientX);
  const move = (m) => seekAt(m.clientX);
  scrub.addEventListener("pointermove", move);
  scrub.addEventListener("pointerup", () => scrub.removeEventListener("pointermove", move), { once: true });
});
scrub.addEventListener("keydown", (e) => {
  const by = { ArrowRight: 5, ArrowLeft: -5 }[e.key];
  if (by) {
    e.stopPropagation();
    music.seek(music.position() + by);
  }
});
$(".heart").addEventListener("click", (e) => {
  const on = e.currentTarget.getAttribute("aria-pressed") !== "true";
  e.currentTarget.setAttribute("aria-pressed", String(on));
  music.sound(on ? "like" : "unlike");
});
let volume = 0.7, muted = false;
const volumeIcon = $("[data-mute] use");
const applyVolume = () => {
  music.setVolume(muted ? 0 : volume);
  volumeIcon.setAttribute("href", muted || !volume ? "#i-volume-off" : "#i-volume");
};
$(".volume").addEventListener("input", (e) => {
  volume = e.target.value / 100;
  muted = false;
  applyVolume();
});
$("[data-mute]").addEventListener("click", () => {
  muted = !muted;
  applyVolume();
});

/* Now playing with synced lyrics: the line playing grows, the others fade by distance. */
const np = $(".nowplaying");
const lyricsEl = $(".lyrics");
for (const line of LYRICS) {
  const li = document.createElement("li");
  li.textContent = line.text;
  li.addEventListener("click", () => {
    music.seek(line.time + 0.01);
    if (!music.playing) toggle();
  });
  lyricsEl.appendChild(li);
}
function openNow() {
  np.hidden = false;
  np.classList.toggle("entering", motion);
  music.sound("tap");
}
function closeNow() {
  np.hidden = true;
}
$$("[data-open-now]").forEach((b) => b.addEventListener("click", () => (np.hidden ? openNow() : closeNow())));
$("[data-close-now]").addEventListener("click", closeNow);
let lastLine = -1;

/* The frame loop: time, lyrics and the waves along the bar, which settle when paused. */
const waves = $(".waves");
const wctx = waves.getContext("2d");
let ww = 0, wh = 0, wt = 0, energy = 0, lastMs = 0;
const sizeWaves = () => {
  const dpr = Math.min(devicePixelRatio, 2);
  ww = waves.clientWidth;
  wh = waves.clientHeight;
  waves.width = ww * dpr;
  waves.height = wh * dpr;
  wctx.setTransform(dpr, 0, 0, dpr, 0, 0);
};
new ResizeObserver(sizeWaves).observe(waves);
sizeWaves();
function drawWaves(levels) {
  wctx.clearRect(0, 0, ww, wh);
  const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#1ed760";
  const layers = [
    { fill: 0.05, line: 0.14, amp: 0.3, speed: 0.23, freq: 1.4, phase: 1.7, band: 0 },
    { fill: 0.09, line: 0.35, amp: 0.42, speed: 0.37, freq: 2.1, phase: 0, band: 1 },
  ];
  for (const l of layers) {
    const lift = 0.15 + energy * 0.85 * (0.6 + levels[l.band] * 0.8);
    const y = (x) => {
      const u = x / ww;
      return wh * 0.82 - wh * l.amp * lift * Math.sin(Math.PI * u) ** 0.5 * (
        Math.sin(u * Math.PI * 2 * l.freq + wt * l.speed + l.phase) * 0.55 +
        Math.sin(u * Math.PI * 2 * l.freq * 2.4 - wt * l.speed * 1.3 + l.phase) * 0.25 + 0.2);
    };
    wctx.beginPath();
    wctx.moveTo(0, wh);
    for (let x = 0; x <= ww; x += 6) wctx.lineTo(x, y(x));
    wctx.lineTo(ww, wh);
    wctx.globalAlpha = l.fill;
    wctx.fillStyle = accent;
    wctx.fill();
    wctx.beginPath();
    for (let x = 0; x <= ww; x += 6) x ? wctx.lineTo(x, y(x)) : wctx.moveTo(x, y(x));
    wctx.globalAlpha = l.line;
    wctx.strokeStyle = accent;
    wctx.lineWidth = 1.2;
    wctx.stroke();
  }
  wctx.globalAlpha = 1;
}
function frame(ms) {
  requestAnimationFrame(frame);
  const dt = Math.min(0.05, (ms - lastMs) / 1000);
  lastMs = ms;
  const pos = music.position();
  scrub.style.setProperty("--p", pos / DURATION);
  scrub.setAttribute("aria-valuenow", Math.round((pos / DURATION) * 100));
  $("[data-elapsed]").textContent = fmt(pos);
  let line = -1;
  LYRICS.forEach((l, i) => { if (l.time <= pos) line = i; });
  if (line !== lastLine) {
    lastLine = line;
    $$("li", lyricsEl).forEach((li, i) => {
      li.classList.toggle("on", i === line);
      li.style.setProperty("--d", Math.abs(i - line));
    });
    const on = $$("li", lyricsEl)[line];
    if (on && !np.hidden) lyricsEl.scrollTo({ top: on.offsetTop - lyricsEl.clientHeight / 2 + on.clientHeight / 2, behavior: motion ? "smooth" : "auto" });
  }
  const levels = music.levels();
  const target = music.playing ? 1 : 0;
  energy += (target - energy) * Math.min(1, dt * (music.playing ? 3 : 1.6));
  if (motion && (music.playing || energy > 0.01)) wt += dt;
  if (!document.hidden) drawWaves(motion ? levels : [0, 0, 0]);
  document.documentElement.style.setProperty("--beat", motion ? (levels[0] * 0.9).toFixed(3) : 0);
}
requestAnimationFrame(frame);

/* The equalizer: the app's presets, springs and bubble notes (src/app/equalizer.rs). */
const choosePreset = (() => {
  const SCALE = [0, 2, 4, 7, 9, 12, 14, 16, 19, 21, 24];
  const STAGGER = 0.045, STIFFNESS = 260, DAMPING = 13, POP = 0.45, WIGGLE = 10;
  const W = 600, H = 240, INSET = 50, MAX = 12;
  const xs = [0, 1, 2, 3, 4, 5].map((i) => INSET + (i * (W - 2 * INSET)) / 5);
  const y = (g) => H / 2 - (Math.max(-MAX, Math.min(MAX, g)) / MAX) * (H / 2 - 22);
  const svg = $(".eq-graph");
  const line = $(".eq-line", svg);
  const fill = $(".eq-fill", svg);
  const ns = "http://www.w3.org/2000/svg";
  const dots = xs.map(() => $(".eq-dots", svg).appendChild(document.createElementNS(ns, "circle")));
  const rings = xs.map(() => $(".eq-rings", svg).appendChild(document.createElementNS(ns, "circle")));
  let shown = [0, 0, 0, 0, 0, 0];
  const pops = [null, null, null, null, null, null];
  let bounce = null, running = false;
  const curve = (pts) => {
    let d = `M ${pts[0][0]} ${pts[0][1]}`;
    for (let i = 0; i < pts.length - 1; i++) {
      const p0 = pts[Math.max(0, i - 1)], p1 = pts[i], p2 = pts[i + 1], p3 = pts[Math.min(pts.length - 1, i + 2)];
      d += ` C ${p1[0] + (p2[0] - p0[0]) / 6} ${p1[1] + (p2[1] - p0[1]) / 6} ${p2[0] - (p3[0] - p1[0]) / 6} ${p2[1] - (p3[1] - p1[1]) / 6} ${p2[0]} ${p2[1]}`;
    }
    return d;
  };
  function render(now) {
    const d = curve([[-20, y(shown[0])], ...xs.map((x, i) => [x, y(shown[i])]), [W + 20, y(shown[5])]]);
    line.setAttribute("d", d);
    fill.setAttribute("d", `${d} L ${W + 20} ${H + 1} L -20 ${H + 1} Z`);
    xs.forEach((x, i) => {
      const age = pops[i] == null ? 1 : Math.min(1, (now - pops[i]) / 1000 / POP);
      const bump = age < 1 ? 5 * Math.sin(Math.min(age * 3, 1) * Math.PI) : 0;
      for (const c of [dots[i], rings[i]]) {
        c.setAttribute("cx", x);
        c.setAttribute("cy", y(shown[i]));
      }
      dots[i].setAttribute("r", 5.5 + bump / 2);
      rings[i].setAttribute("r", age < 1 ? 5 + 18 * (1 - (1 - age) ** 3) : 0);
      rings[i].style.opacity = age < 1 ? (1 - age) ** 2 : 0;
    });
  }
  const note = (band, gain, lift) => {
    const degree = Math.max(0, Math.min(SCALE.length - 1, band + 2 + Math.round(gain / 3)));
    music.sound("bubble", { freq: 392 * 2 ** (SCALE[degree] / 12), lift });
  };
  function step(now) {
    const b = bounce;
    let done = true;
    const elapsed = (now - b.start) / 1000;
    const dt = Math.min(0.05, (now - b.last) / 1000);
    b.last = now;
    for (let i = 0; i < 6; i++) {
      if (elapsed < i * STAGGER) { done = false; continue; }
      if (!b.started[i]) {
        b.started[i] = true;
        if (Math.abs(b.goal[i] - b.from[i]) <= 0.05) { b.landed[i] = true; b.speed[i] = WIGGLE; }
      }
      const steps = Math.max(1, Math.ceil(dt / 0.004)), h = dt / steps;
      for (let s = 0; s < steps; s++) {
        b.speed[i] += (STIFFNESS * (b.goal[i] - shown[i]) - DAMPING * b.speed[i]) * h;
        shown[i] += b.speed[i] * h;
      }
      if (!b.landed[i] && (b.goal[i] - shown[i]) * Math.sign(b.goal[i] - b.from[i]) <= 0.05) {
        b.landed[i] = true;
        pops[i] = now;
        note(i, b.goal[i], Math.min(1, Math.abs(b.goal[i] - b.from[i]) / 6));
      }
      if (Math.abs(b.goal[i] - shown[i]) < 0.02 && Math.abs(b.speed[i]) < 0.2) {
        shown[i] = b.goal[i];
        b.speed[i] = 0;
      } else {
        done = false;
      }
    }
    render(now);
    if (done && !pops.some((p) => p != null && (now - p) / 1000 < POP)) {
      running = false;
      return;
    }
    requestAnimationFrame(step);
  }
  const chips = $(".chips");
  const chipEls = PRESETS.map(([name]) => {
    const chip = document.createElement("button");
    chip.className = "chip";
    chip.type = "button";
    chip.textContent = name;
    chip.setAttribute("aria-pressed", String(name === "Flat"));
    chips.appendChild(chip);
    return chip;
  });
  function choose(index) {
    const gains = PRESETS[index][1];
    chipEls.forEach((c, i) => c.setAttribute("aria-pressed", String(i === index)));
    music.setEq(gains);
    const now = performance.now();
    if (!motion) {
      gains.forEach((g, i) => Math.abs(g - shown[i]) > 0.05 && note(i, g, 1));
      shown = gains.slice();
      return render(now);
    }
    bounce = { start: now, last: now, from: shown.slice(), goal: gains, speed: [0, 0, 0, 0, 0, 0], started: [], landed: [] };
    if (!running) {
      running = true;
      requestAnimationFrame(step);
    }
  }
  chipEls.forEach((chip, i) => chip.addEventListener("click", () => choose(i)));
  render(performance.now());
  return choose;
})();

/* Quick search, like Tonal's Ctrl K: pages, presets, themes and actions. */
const palette = $(".palette");
const input = $("input", palette);
const list = $("ul", palette);
let entries = [], selected = 0;
function actions() {
  return [
    ...VIEWS.map((v) => ({ title: TITLES[v], detail: "Page", run: () => go(v) })),
    ...PRESETS.map(([name], i) => ({ title: `Equalizer: ${name}`, detail: "Preset", run: () => { go("equalizer"); choosePreset(i); } })),
    ...THEMES.map((t) => ({ title: `Theme: ${t.name}`, detail: "Theme", run: () => setTheme(t) })),
    { title: music.playing ? "Pause" : "Play", detail: "Space", run: toggle },
    { title: "Now playing and lyrics", detail: "L", run: openNow },
    { title: "Download Tonal", detail: "Action", run: () => go("download") },
    { title: motion ? "Turn animations off" : "Turn animations on", detail: "Setting", run: () => setMotion(!motion) },
  ];
}
function search() {
  const q = input.value.trim().toLowerCase();
  const score = (t) => {
    const at = t.toLowerCase().indexOf(q);
    return at < 0 ? -1 : at === 0 ? 3 : /[\s:]/.test(t[at - 1]) ? 2 : 1;
  };
  entries = actions().map((a) => ({ ...a, s: q ? score(a.title) : 1 })).filter((a) => a.s > 0).sort((a, b) => b.s - a.s).slice(0, 8);
  selected = 0;
  list.replaceChildren(...entries.map((a, i) => {
    const li = document.createElement("li");
    li.setAttribute("role", "option");
    li.innerHTML = "<span></span><small></small>";
    $("span", li).textContent = a.title;
    $("small", li).textContent = a.detail;
    li.addEventListener("pointermove", () => mark(i));
    li.addEventListener("click", () => runEntry(i));
    return li;
  }));
  mark(0);
}
function mark(i) {
  selected = i;
  $$("li", list).forEach((li, k) => li.setAttribute("aria-selected", String(k === i)));
}
function runEntry(i) {
  const entry = entries[i];
  palette.close();
  entry?.run();
}
function openPalette() {
  input.value = "";
  search();
  palette.showModal();
  input.focus();
}
$("[data-palette]").addEventListener("click", openPalette);
input.addEventListener("input", search);
input.addEventListener("keydown", (e) => {
  if (e.key === "ArrowDown") { e.preventDefault(); mark(Math.min(selected + 1, entries.length - 1)); }
  if (e.key === "ArrowUp") { e.preventDefault(); mark(Math.max(selected - 1, 0)); }
  if (e.key === "Enter") { e.preventDefault(); runEntry(selected); }
});
palette.addEventListener("click", (e) => { if (e.target === palette) palette.close(); });

/* Tonal's keys. */
addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
    e.preventDefault();
    palette.open ? palette.close() : openPalette();
    return;
  }
  if (palette.open || e.metaKey || e.ctrlKey || e.altKey || /INPUT|TEXTAREA/.test(document.activeElement.tagName)) return;
  if (e.key === " " && !/BUTTON|A/.test(document.activeElement.tagName)) { e.preventDefault(); toggle(); }
  else if (/^[1-6]$/.test(e.key)) { if (current !== "equalizer") go("equalizer"); choosePreset(Number(e.key) - 1); }
  else if (e.key === "ArrowRight" && document.activeElement !== scrub) step(1);
  else if (e.key === "ArrowLeft" && document.activeElement !== scrub) step(-1);
  else if (e.key.toLowerCase() === "l") np.hidden ? openNow() : closeNow();
  else if (e.key === "Escape" && !np.hidden) closeNow();
});

$$(".row").forEach((el, i) => el.style.setProperty("--i", i));
syncPlay();
show(viewOf(location.pathname), false);
