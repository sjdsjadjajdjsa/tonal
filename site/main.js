const PROJECT = "https://gitlab.com/api/v4/projects/sjdsjadjajdjsa%2Ftonal";
const FILES = "https://gitlab.com/sjdsjadjajdjsa/tonal/-/releases/permalink/latest/downloads/";
const VIEWS = ["home", "equalizer", "themes", "light", "download"];
const TITLES = { home: "Home", equalizer: "Equalizer", themes: "Themes", light: "Light", download: "Download" };
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
let moving = !reduced;

const panel = document.querySelector(".panel");
const $ = (s, root = document) => root.querySelector(s);
const $$ = (s, root = document) => [...root.querySelectorAll(s)];

/* Pages: the sidebar swaps the main panel the way Tonal does. */
function show(name, enter = true) {
  if (!VIEWS.includes(name)) name = "home";
  for (const view of $$(".view")) {
    const on = view.dataset.view === name;
    view.hidden = !on;
    view.classList.toggle("entering", on && enter && moving);
  }
  for (const link of $$(".nav[data-view]")) link.classList.toggle("active", link.dataset.view === name);
  $("[data-now]").textContent = TITLES[name];
  panel.scrollTop = 0;
  progress();
  current = name;
}
let current = "home";
const route = () => show(location.hash.replace(/^#\/?/, "") || "home");
addEventListener("hashchange", route);
const go = (step) => {
  const next = VIEWS[(VIEWS.indexOf(current) + step + VIEWS.length) % VIEWS.length];
  location.hash = next === "home" ? "#/" : `#/${next}`;
};
$$("[data-step]").forEach((b) => b.addEventListener("click", () => go(Number(b.dataset.step))));

/* The progress bar is the panel's scroll: dragging it scrolls the page. */
const scrub = $(".scrub");
function progress() {
  const max = panel.scrollHeight - panel.clientHeight;
  const p = max > 0 ? panel.scrollTop / max : 0;
  scrub.style.setProperty("--p", p);
  scrub.setAttribute("aria-valuenow", Math.round(p * 100));
  const total = 210;
  const t = Math.round(p * total);
  $("[data-elapsed]").textContent = `${Math.floor(t / 60)}:${String(t % 60).padStart(2, "0")}`;
}
panel.addEventListener("scroll", progress, { passive: true });
addEventListener("resize", progress);
const seek = (x) => {
  const r = scrub.getBoundingClientRect();
  const p = Math.max(0, Math.min(1, (x - r.left) / r.width));
  panel.scrollTop = p * (panel.scrollHeight - panel.clientHeight);
};
scrub.addEventListener("pointerdown", (e) => {
  scrub.setPointerCapture(e.pointerId);
  seek(e.clientX);
  const move = (m) => seek(m.clientX);
  scrub.addEventListener("pointermove", move);
  scrub.addEventListener("pointerup", () => scrub.removeEventListener("pointermove", move), { once: true });
});
scrub.addEventListener("keydown", (e) => {
  const step = { ArrowRight: 0.1, ArrowLeft: -0.1 }[e.key];
  if (step) panel.scrollBy({ top: step * (panel.scrollHeight - panel.clientHeight) });
});

/* Downloads: point at the file for this computer and mark its card. */
function system() {
  const p = (navigator.userAgentData?.platform || navigator.platform || navigator.userAgent).toLowerCase();
  if (/mac/.test(p)) return "mac";
  if (/win/.test(p)) return "windows";
  if (/linux|x11/.test(p) && !/android/.test(navigator.userAgent.toLowerCase())) return "linux";
  return null;
}
const os = system();
const pick = { mac: ["Download for Mac", "tonal-macos-arm64.dmg"], windows: ["Download for Windows", "tonal-setup-x86_64.exe"], linux: ["Download for Linux", "tonal-x86_64.AppImage"] }[os];
if (pick) {
  const link = $("[data-download]");
  link.href = FILES + pick[1];
  $("[data-download-label]", link).textContent = pick[0];
  const card = $(`.group-card[data-os="${os}"]`);
  card?.classList.add("mine");
  card?.parentElement.prepend(card);
}

fetch(`${PROJECT}/releases/permalink/latest`)
  .then((r) => (r.ok ? r.json() : null))
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
  }, moving ? 160 : 0);
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

/* Sound: the app's bubble and bell (src/sfx.rs), at the player bar's volume. */
let audio = null;
let volume = 0.7;
let muted = false;
function voice(freq, level, delay, kind) {
  if (muted || volume === 0) return;
  audio ??= new AudioContext();
  if (audio.state === "suspended") audio.resume();
  const t = audio.currentTime + 0.005 + delay;
  const bell = kind === "bell";
  const out = audio.createGain();
  out.gain.setValueAtTime(0, t);
  out.gain.linearRampToValueAtTime(level * volume, t + (bell ? 0.004 : 0.002));
  out.gain.setTargetAtTime(0, t + 0.004, bell ? 0.14 : 0.04);
  out.connect(audio.destination);
  for (const [ratio, part] of bell ? [[1, 1], [2.76, 0.22]] : [[1, 1], [2, 0.18]]) {
    const osc = audio.createOscillator();
    const amp = audio.createGain();
    amp.gain.value = part;
    osc.frequency.setValueAtTime(freq * ratio, t);
    if (!bell) osc.frequency.setTargetAtTime(freq * ratio * 1.9, t, 0.03);
    osc.connect(amp).connect(out);
    osc.start(t);
    osc.stop(t + (bell ? 0.6 : 0.18));
  }
}
const volumeIcon = $("[data-mute] use");
$(".volume").addEventListener("input", (e) => {
  volume = e.target.value / 100;
  muted = false;
  volumeIcon.setAttribute("href", volume ? "#i-volume" : "#i-volume-off");
});
$("[data-mute]").addEventListener("click", () => {
  muted = !muted;
  volumeIcon.setAttribute("href", muted ? "#i-volume-off" : "#i-volume");
});
$(".heart").addEventListener("click", (e) => {
  const on = e.currentTarget.getAttribute("aria-pressed") !== "true";
  e.currentTarget.setAttribute("aria-pressed", String(on));
  if (on) {
    voice(1318.5, 0.18, 0, "bell");
    voice(1975.5, 0.16, 0.07, "bell");
  } else {
    voice(987.8, 0.14, 0, "bell");
  }
});

/* Play and pause: pausing stops every moving thing on the page. */
const play = $(".play");
function setMoving(on) {
  moving = on;
  document.body.classList.toggle("still", !on);
  $("use", play).setAttribute("href", on ? "#i-pause" : "#i-play");
  play.setAttribute("aria-label", on ? "Pause motion" : "Play motion");
}
setMoving(moving);
play.addEventListener("click", () => setMoving(!moving));
addEventListener("keydown", (e) => {
  if (e.key === " " && !/INPUT|BUTTON|A/.test(document.activeElement.tagName)) {
    e.preventDefault();
    setMoving(!moving);
  }
});

/* The waves along the player bar, drawn the way the visualizer looks at rest. */
(() => {
  const canvas = $(".waves");
  const ctx = canvas.getContext("2d");
  let w = 0, h = 0;
  const size = () => {
    const dpr = Math.min(devicePixelRatio, 2);
    w = canvas.clientWidth;
    h = canvas.clientHeight;
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  };
  size();
  addEventListener("resize", size);
  const layers = [
    { a: 0.05, line: 0.12, amp: 0.34, speed: 0.23, freq: 1.4, phase: 1.7 },
    { a: 0.08, line: 0.3, amp: 0.46, speed: 0.37, freq: 2.1, phase: 0 },
  ];
  let t = 0, last = 0;
  const draw = () => {
    ctx.clearRect(0, 0, w, h);
    for (const l of layers) {
      const y = (x) => {
        const u = x / w;
        return h * 0.78 - h * l.amp * Math.sin(Math.PI * u) ** 0.5 * (
          Math.sin(u * Math.PI * 2 * l.freq + t * l.speed + l.phase) * 0.55 +
          Math.sin(u * Math.PI * 2 * l.freq * 2.4 - t * l.speed * 1.3 + l.phase) * 0.25 + 0.2);
      };
      ctx.beginPath();
      ctx.moveTo(0, h);
      for (let x = 0; x <= w; x += 6) ctx.lineTo(x, y(x));
      ctx.lineTo(w, h);
      ctx.fillStyle = `rgba(243,243,245,${l.a})`;
      ctx.fill();
      ctx.beginPath();
      for (let x = 0; x <= w; x += 6) x ? ctx.lineTo(x, y(x)) : ctx.moveTo(x, y(x));
      ctx.strokeStyle = `rgba(243,243,245,${l.line})`;
      ctx.lineWidth = 1.2;
      ctx.stroke();
    }
  };
  const loop = (ms) => {
    const dt = Math.min(0.05, (ms - last) / 1000);
    last = ms;
    if (moving && !document.hidden) {
      t += dt;
      draw();
    }
    requestAnimationFrame(loop);
  };
  draw();
  requestAnimationFrame(loop);
})();

/* The equalizer: the app's presets, springs and bubble notes (src/app/equalizer.rs). */
(() => {
  const PRESETS = [
    ["Flat", [0, 0, 0, 0, 0, 0]],
    ["Bass", [10, 7, 2, -1, 0, 0]],
    ["Vocal", [-5, -3, 2, 6, 5, -1]],
    ["Bright", [-2, -1, 0, 2, 7, 10]],
    ["Warm", [6, 4, 2, 0, -3, -6]],
    ["Late night", [4, 2, 0, -2, -5, -8]],
  ];
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
  let bounce = null;
  let running = false;

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
  function note(band, gain, level) {
    const degree = Math.max(0, Math.min(SCALE.length - 1, band + 2 + Math.round(gain / 3)));
    voice(392 * 2 ** (SCALE[degree] / 12), level, 0, "bubble");
  }
  function choose(gains) {
    const now = performance.now();
    if (!moving) {
      gains.forEach((g, i) => Math.abs(g - shown[i]) > 0.05 && note(i, g, 0.2));
      shown = gains.slice();
      return render(now);
    }
    bounce = { start: now, last: now, from: shown.slice(), goal: gains, speed: [0, 0, 0, 0, 0, 0], started: [], landed: [] };
    if (!running) {
      running = true;
      requestAnimationFrame(step);
    }
  }
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
        note(i, b.goal[i], 0.2 * (0.75 + 0.25 * Math.min(1, Math.abs(b.goal[i] - b.from[i]) / 6)));
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
  for (const [name, gains] of PRESETS) {
    const chip = document.createElement("button");
    chip.className = "chip";
    chip.type = "button";
    chip.textContent = name;
    chip.setAttribute("aria-pressed", String(name === "Flat"));
    chip.addEventListener("click", () => {
      $$(".chip", chips).forEach((c) => c.setAttribute("aria-pressed", String(c === chip)));
      choose(gains);
    });
    chips.appendChild(chip);
  }
  render(performance.now());
})();

$$(".row").forEach((el, i) => el.style.setProperty("--i", i));
show(location.hash.replace(/^#\/?/, "") || "home", false);
