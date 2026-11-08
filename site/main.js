const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
const PROJECT = "https://gitlab.com/api/v4/projects/sjdsjadjajdjsa%2Ftonal";
const FILES = "https://gitlab.com/sjdsjadjajdjsa/tonal/-/releases/permalink/latest/downloads/";

const seen = new IntersectionObserver((entries) => {
  for (const e of entries) {
    if (e.isIntersecting) {
      e.target.classList.add("seen");
      seen.unobserve(e.target);
    }
  }
}, { threshold: 0.15 });
document.querySelectorAll(".reveal").forEach((el, i) => {
  el.style.transitionDelay = `${(i % 3) * 70}ms`;
  seen.observe(el);
});
document.querySelectorAll(".bars").forEach((el) => seen.observe(el));
document.querySelectorAll(".row").forEach((el, i) => el.style.setProperty("--i", i));

function system() {
  const p = (navigator.userAgentData?.platform || navigator.platform || navigator.userAgent).toLowerCase();
  if (/mac|iphone|ipad/.test(p)) return "mac";
  if (/win/.test(p)) return "windows";
  if (/linux|x11|cros/.test(p) && !/android/.test(p)) return "linux";
  return null;
}
const os = system();
const choice = {
  mac: ["Download for Mac", "tonal-macos-arm64.dmg"],
  windows: ["Download for Windows", "tonal-setup-x86_64.exe"],
  linux: ["Download for Linux", "tonal-x86_64.AppImage"],
}[os];
if (choice) {
  const link = document.querySelector("[data-download]");
  link.href = FILES + choice[1];
  link.querySelector("[data-download-label]").textContent = choice[0];
  document.querySelector(`.platform[data-os="${os}"]`)?.classList.add("mine");
}

fetch(`${PROJECT}/releases/permalink/latest`)
  .then((r) => (r.ok ? r.json() : null))
  .then((release) => {
    if (!release) return;
    const version = release.tag_name.replace(/^v/, "");
    const date = new Date(release.released_at).toLocaleDateString(undefined, { month: "long", day: "numeric", year: "numeric" });
    document.querySelector("[data-version]").textContent = `Version ${version}, free and open source`;
    document.querySelector("[data-version-line]").textContent = `Version ${version}, released ${date}. Free and open source.`;
  })
  .catch(() => {});

document.querySelectorAll("[data-copy]").forEach((button) => {
  button.addEventListener("click", async () => {
    await navigator.clipboard.writeText(button.dataset.copy);
    const label = button.querySelector("span");
    label.textContent = "Copied";
    setTimeout(() => (label.textContent = "Copy"), 1600);
  });
});

/* The wave under the hero, the same soft ribbon as the player bar. */
(() => {
  const canvas = document.querySelector(".wave");
  const ctx = canvas.getContext("2d");
  let w = 0, h = 0, visible = true;
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
  new IntersectionObserver(([e]) => (visible = e.isIntersecting)).observe(canvas);
  const layers = [
    { color: "#8b7bff", alpha: 0.18, amp: 0.26, speed: 0.22, freq: 1.3, phase: 1.7 },
    { color: "#2ec4b6", alpha: 0.16, amp: 0.3, speed: 0.31, freq: 1.8, phase: 0.4 },
    { color: "#1ed760", alpha: 0.32, amp: 0.36, speed: 0.4, freq: 2.2, phase: 0 },
  ];
  const draw = (t) => {
    ctx.clearRect(0, 0, w, h);
    for (const l of layers) {
      const y = (x) => {
        const u = x / w;
        const envelope = Math.sin(Math.PI * u) ** 0.6;
        return h * 0.62 - h * l.amp * envelope * (
          Math.sin(u * Math.PI * 2 * l.freq + t * l.speed + l.phase) * 0.6 +
          Math.sin(u * Math.PI * 2 * l.freq * 2.3 - t * l.speed * 1.4 + l.phase) * 0.25 +
          Math.sin(t * l.speed * 0.7 + l.phase) * 0.15);
      };
      ctx.beginPath();
      ctx.moveTo(0, h);
      for (let x = 0; x <= w; x += 6) ctx.lineTo(x, y(x));
      ctx.lineTo(w, h);
      ctx.closePath();
      const fill = ctx.createLinearGradient(0, 0, 0, h);
      fill.addColorStop(0, l.color + Math.round(l.alpha * 255).toString(16).padStart(2, "0"));
      fill.addColorStop(1, l.color + "00");
      ctx.fillStyle = fill;
      ctx.fill();
      ctx.beginPath();
      for (let x = 0; x <= w; x += 6) x ? ctx.lineTo(x, y(x)) : ctx.moveTo(x, y(x));
      ctx.strokeStyle = l.color;
      ctx.globalAlpha = Math.min(1, l.alpha * 2.6);
      ctx.lineWidth = 1.6;
      ctx.stroke();
      ctx.globalAlpha = 1;
    }
  };
  if (still) return draw(0);
  const loop = (ms) => {
    if (visible && !document.hidden) draw(ms / 1000);
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
})();

/* The equalizer: the app's presets, springs and bubble notes (src/app/equalizer.rs, src/sfx.rs). */
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

  const svg = document.querySelector(".eq-graph");
  const line = svg.querySelector(".eq-line");
  const fill = svg.querySelector(".eq-fill");
  const dotsG = svg.querySelector(".eq-dots");
  const ringsG = svg.querySelector(".eq-rings");
  const ns = "http://www.w3.org/2000/svg";
  const dots = xs.map(() => dotsG.appendChild(document.createElementNS(ns, "circle")));
  const rings = xs.map(() => ringsG.appendChild(document.createElementNS(ns, "circle")));

  let shown = [0, 0, 0, 0, 0, 0];
  let pops = [null, null, null, null, null, null];
  let bounce = null;
  let running = false;

  function curve(points) {
    let d = `M ${points[0][0]} ${points[0][1]}`;
    for (let i = 0; i < points.length - 1; i++) {
      const p0 = points[Math.max(0, i - 1)], p1 = points[i], p2 = points[i + 1], p3 = points[Math.min(points.length - 1, i + 2)];
      const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
      const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
      d += ` C ${c1[0]} ${c1[1]} ${c2[0]} ${c2[1]} ${p2[0]} ${p2[1]}`;
    }
    return d;
  }

  function render(now) {
    const points = [[-20, y(shown[0])], ...xs.map((x, i) => [x, y(shown[i])]), [W + 20, y(shown[5])]];
    const d = curve(points);
    line.setAttribute("d", d);
    fill.setAttribute("d", `${d} L ${W + 20} ${H + 1} L -20 ${H + 1} Z`);
    xs.forEach((x, i) => {
      const age = pops[i] == null ? 1 : Math.min(1, (now - pops[i]) / 1000 / POP);
      const bump = age < 1 ? 5 * Math.sin(Math.min(age * 3, 1) * Math.PI) : 0;
      dots[i].setAttribute("cx", x);
      dots[i].setAttribute("cy", y(shown[i]));
      dots[i].setAttribute("r", 6 + bump / 2);
      const grow = 1 - (1 - age) ** 3;
      rings[i].setAttribute("cx", x);
      rings[i].setAttribute("cy", y(shown[i]));
      rings[i].setAttribute("r", age < 1 ? 5 + 18 * grow : 0);
      rings[i].style.opacity = age < 1 ? (1 - age) ** 2 : 0;
    });
  }

  let audio = null;
  function bubble(band, gain, level) {
    audio ??= new AudioContext();
    if (audio.state === "suspended") audio.resume();
    const degree = Math.max(0, Math.min(SCALE.length - 1, band + 2 + Math.round(gain / 3)));
    const freq = 392 * 2 ** (SCALE[degree] / 12);
    const t = audio.currentTime + 0.005;
    const out = audio.createGain();
    out.gain.setValueAtTime(0, t);
    out.gain.linearRampToValueAtTime(level, t + 0.002);
    out.gain.setTargetAtTime(0, t + 0.002, 0.04);
    out.connect(audio.destination);
    for (const [ratio, part] of [[1, 1], [2, 0.18]]) {
      const osc = audio.createOscillator();
      const amp = audio.createGain();
      amp.gain.value = part;
      osc.frequency.setValueAtTime(freq * ratio, t);
      osc.frequency.setTargetAtTime(freq * ratio * 1.9, t, 0.03);
      osc.connect(amp).connect(out);
      osc.start(t);
      osc.stop(t + 0.18);
    }
  }

  function choose(gains) {
    if (still) {
      shown = gains.slice();
      return render(performance.now());
    }
    const now = performance.now();
    bounce = { start: now, last: now, from: shown.slice(), goal: gains, speed: [0, 0, 0, 0, 0, 0], started: [], landed: [] };
    if (!running) {
      running = true;
      requestAnimationFrame(step);
    }
  }

  function step(now) {
    const b = bounce;
    let done = true;
    if (b) {
      const elapsed = (now - b.start) / 1000;
      const dt = Math.min(0.05, (now - b.last) / 1000);
      b.last = now;
      for (let i = 0; i < 6; i++) {
        if (elapsed < i * STAGGER) { done = false; continue; }
        const moves = Math.abs(b.goal[i] - b.from[i]) > 0.05;
        if (!b.started[i]) {
          b.started[i] = true;
          if (!moves) { b.landed[i] = true; b.speed[i] = WIGGLE; }
        }
        const steps = Math.max(1, Math.ceil(dt / 0.004)), h = dt / steps;
        for (let s = 0; s < steps; s++) {
          b.speed[i] += (STIFFNESS * (b.goal[i] - shown[i]) - DAMPING * b.speed[i]) * h;
          shown[i] += b.speed[i] * h;
        }
        if (!b.landed[i] && (b.goal[i] - shown[i]) * Math.sign(b.goal[i] - b.from[i]) <= 0.05) {
          b.landed[i] = true;
          pops[i] = now;
          const lift = Math.abs(b.goal[i] - b.from[i]);
          bubble(i, b.goal[i], 0.18 * (0.75 + 0.25 * Math.min(1, lift / 6)));
        }
        if (Math.abs(b.goal[i] - shown[i]) < 0.02 && Math.abs(b.speed[i]) < 0.2) {
          shown[i] = b.goal[i];
          b.speed[i] = 0;
        } else {
          done = false;
        }
      }
    }
    const popping = pops.some((p) => p != null && (now - p) / 1000 < POP);
    render(now);
    if (done && !popping) {
      bounce = null;
      running = false;
      return;
    }
    requestAnimationFrame(step);
  }

  const chips = document.querySelector(".chips");
  for (const [name, gains] of PRESETS) {
    const chip = document.createElement("button");
    chip.className = "chip";
    chip.type = "button";
    chip.textContent = name;
    chip.setAttribute("aria-pressed", String(name === "Flat"));
    chip.addEventListener("click", () => {
      chips.querySelectorAll(".chip").forEach((c) => c.setAttribute("aria-pressed", String(c === chip)));
      choose(gains);
    });
    chips.appendChild(chip);
  }
  render(performance.now());
})();
