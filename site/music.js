// "First Light": a short loop made in code (no recording, so no rights to clear),
// played through Tonal's six equalizer bands, plus the app's interface sounds
// from src/sfx.rs. Nothing makes a sound until the visitor clicks something.

const BPM = 84;
const STEP = 60 / BPM / 4;
const BARS = 8;
const STEPS = BARS * 16;
export const DURATION = STEPS * STEP;

const CHORDS = [
  [53, 57, 60, 64, 67], [52, 55, 59, 62, 66], [50, 53, 57, 60, 64], [48, 52, 55, 59, 62],
  [53, 57, 60, 64, 67], [52, 55, 59, 62, 66], [50, 53, 57, 60, 64], [55, 59, 62, 65, 69],
];
const ROOTS = [41, 40, 38, 36, 41, 40, 38, 43];
const MELODY = [
  [[6, 76], [10, 79]], [[2, 74], [12, 71]], [[6, 72], [9, 74], [12, 77]], [[4, 76]],
  [[6, 79], [10, 81]], [[2, 79], [8, 76], [14, 74]], [[6, 72], [12, 74]], [[2, 71], [8, 74]],
];

/// Synced lyrics, one line a bar, like a song's LRC file.
export const LYRICS = [
  "Turn the lights down low",
  "let the room fill up with sound",
  "every band in its own place",
  "every beat comes back around",
  "lift the bass a little higher",
  "soften treble late at night",
  "the waves know where you're going",
  "and the colours follow light",
].map((text, i) => ({ text, time: i * 16 * STEP }));

const BANDS = [
  ["lowshelf", 60],
  ["peaking", 150],
  ["peaking", 400],
  ["peaking", 1000],
  ["peaking", 3000],
  ["highshelf", 8000],
];

const hz = (midi) => 440 * 2 ** ((midi - 69) / 12);

export function createMusic() {
  let ctx = null, out, musicBus, eq = [], preamp, analyser, data, noise;
  let playing = false, step = 0, nextAt = 0, timer = null, startedAt = 0, startStep = 0;
  let volume = 0.7, unlocked = false;
  let gains = [0, 0, 0, 0, 0, 0];

  function setup() {
    if (ctx) return;
    ctx = new AudioContext();
    out = ctx.createGain();
    out.gain.value = volume;
    out.connect(ctx.destination);
    analyser = ctx.createAnalyser();
    analyser.fftSize = 256;
    analyser.smoothingTimeConstant = 0.82;
    data = new Uint8Array(analyser.frequencyBinCount);
    const limiter = ctx.createDynamicsCompressor();
    limiter.threshold.value = -1.5;
    limiter.knee.value = 0;
    limiter.ratio.value = 20;
    limiter.attack.value = 0.003;
    limiter.release.value = 0.12;
    preamp = ctx.createGain();
    musicBus = ctx.createGain();
    musicBus.gain.value = 0;
    let node = musicBus;
    eq = BANDS.map(([type, f], i) => {
      const filter = ctx.createBiquadFilter();
      filter.type = type;
      filter.frequency.value = f;
      filter.Q.value = type === "peaking" ? 1.41 : 0.71;
      filter.gain.value = gains[i];
      node.connect(filter);
      node = filter;
      return filter;
    });
    node.connect(preamp).connect(limiter).connect(analyser).connect(out);
    noise = ctx.createBuffer(1, ctx.sampleRate, ctx.sampleRate);
    const d = noise.getChannelData(0);
    for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
    applyEq();
  }

  function env(gainNode, t, peak, attack, decay) {
    gainNode.gain.setValueAtTime(0, t);
    gainNode.gain.linearRampToValueAtTime(peak, t + attack);
    gainNode.gain.setTargetAtTime(0, t + attack, decay);
  }

  function keys(chord, t) {
    for (const note of chord) {
      const g = ctx.createGain();
      const lp = ctx.createBiquadFilter();
      lp.type = "lowpass";
      lp.frequency.value = 1800;
      env(g, t, 0.05, 0.03, 0.9);
      g.connect(lp).connect(musicBus);
      for (const detune of [-6, 6]) {
        const o = ctx.createOscillator();
        o.type = "triangle";
        o.frequency.value = hz(note);
        o.detune.value = detune;
        o.connect(g);
        o.start(t);
        o.stop(t + 16 * STEP + 0.4);
      }
    }
  }

  function bass(note, t, length) {
    const g = ctx.createGain();
    const lp = ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.value = 420;
    env(g, t, 0.28, 0.01, length * 0.6);
    g.connect(lp).connect(musicBus);
    for (const [type, mul] of [["sine", 1], ["sawtooth", 0.25]]) {
      const o = ctx.createOscillator();
      const m = ctx.createGain();
      m.gain.value = mul;
      o.type = type;
      o.frequency.value = hz(note);
      o.connect(m).connect(g);
      o.start(t);
      o.stop(t + length + 0.3);
    }
  }

  function kick(t) {
    const o = ctx.createOscillator();
    const g = ctx.createGain();
    o.frequency.setValueAtTime(120, t);
    o.frequency.exponentialRampToValueAtTime(44, t + 0.12);
    env(g, t, 0.55, 0.002, 0.11);
    o.connect(g).connect(musicBus);
    o.start(t);
    o.stop(t + 0.5);
  }

  function hiss(t, type, f, q, peak, decay) {
    const s = ctx.createBufferSource();
    s.buffer = noise;
    const filter = ctx.createBiquadFilter();
    filter.type = type;
    filter.frequency.value = f;
    filter.Q.value = q;
    const g = ctx.createGain();
    env(g, t, peak, 0.002, decay);
    s.connect(filter).connect(g).connect(musicBus);
    s.start(t, Math.random() * 0.5);
    s.stop(t + decay * 6);
  }

  function bell(freq, t, level, bus) {
    const g = ctx.createGain();
    env(g, t, level, 0.004, 0.14);
    g.connect(bus);
    for (const [ratio, part] of [[1, 1], [2.76, 0.22]]) {
      const o = ctx.createOscillator();
      const m = ctx.createGain();
      m.gain.value = part;
      o.frequency.value = freq * ratio;
      o.connect(m).connect(g);
      o.start(t);
      o.stop(t + 0.7);
    }
  }

  function schedule(s, t) {
    const bar = Math.floor(s / 16) % BARS, beat = s % 16;
    if (beat === 0) keys(CHORDS[bar], t);
    if (beat === 0) bass(ROOTS[bar], t, 6 * STEP);
    if (beat === 7) bass(ROOTS[bar] + 12, t, 2 * STEP);
    if (beat === 10) bass(ROOTS[bar], t, 5 * STEP);
    if (beat === 0 || beat === 10 || (beat === 7 && bar % 2)) kick(t);
    if (beat === 4 || beat === 12) {
      hiss(t, "bandpass", 1900, 0.9, 0.22, 0.07);
      hiss(t, "lowpass", 400, 0.7, 0.08, 0.05);
    }
    if (beat % 2 === 0) hiss(t + (beat % 4 === 2 ? STEP * 0.18 : 0), "highpass", 7500, 0.7, beat % 4 ? 0.035 : 0.05, 0.025);
    for (const [at, note] of MELODY[bar]) if (at === beat) bell(hz(note), t, 0.07, musicBus);
  }

  function tick() {
    while (nextAt < ctx.currentTime + 0.12) {
      schedule(step, nextAt);
      step = (step + 1) % STEPS;
      nextAt += STEP;
    }
  }

  function applyEq() {
    if (!ctx) return;
    const top = Math.max(0, ...gains);
    eq.forEach((f, i) => f.gain.setTargetAtTime(gains[i], ctx.currentTime, 0.04));
    preamp.gain.setTargetAtTime(10 ** (-top / 2 / 20), ctx.currentTime, 0.04);
  }

  return {
    get playing() { return playing; },
    get unlocked() { return unlocked; },
    play() {
      setup();
      unlocked = true;
      if (ctx.state === "suspended") ctx.resume();
      if (playing) return;
      playing = true;
      nextAt = ctx.currentTime + 0.06;
      startedAt = nextAt;
      startStep = step;
      musicBus.gain.cancelScheduledValues(ctx.currentTime);
      musicBus.gain.setTargetAtTime(1, ctx.currentTime, 0.02);
      tick();
      timer = setInterval(tick, 25);
    },
    pause() {
      if (!playing) return;
      playing = false;
      clearInterval(timer);
      step = Math.floor(this.position() / STEP) % STEPS;
      musicBus.gain.cancelScheduledValues(ctx.currentTime);
      musicBus.gain.setTargetAtTime(0, ctx.currentTime, 0.03);
    },
    position() {
      if (!ctx || !playing) return step * STEP;
      return ((startStep * STEP + Math.max(0, ctx.currentTime - startedAt)) % DURATION + DURATION) % DURATION;
    },
    seek(seconds) {
      const target = Math.floor(Math.max(0, Math.min(DURATION - 0.01, seconds)) / STEP);
      if (!playing) {
        step = target;
        return;
      }
      this.pause();
      step = target;
      this.play();
    },
    /// Loudness of the low, middle and high third of the spectrum, 0 to 1.
    levels() {
      if (!ctx || !playing) return [0, 0, 0];
      analyser.getByteFrequencyData(data);
      const third = data.length / 3;
      return [0, 1, 2].map((k) => {
        let sum = 0;
        for (let i = Math.floor(k * third); i < (k + 1) * third; i++) sum += data[i];
        return Math.min(1, sum / third / 160);
      });
    },
    setEq(next) {
      gains = next.slice();
      applyEq();
    },
    setVolume(v) {
      volume = v;
      if (ctx) out.gain.setTargetAtTime(v, ctx.currentTime, 0.02);
    },
    /// The app's interface sounds. Each one comes from a click, which lets the browser
    /// start audio, so they play from the first click on.
    sound(kind, extra = 0) {
      setup();
      unlocked = true;
      if (ctx.state === "suspended") ctx.resume();
      const t = ctx.currentTime + 0.005;
      if (kind === "bubble") {
        const g = ctx.createGain();
        env(g, t, 0.2 * (0.75 + 0.25 * extra.lift), 0.002, 0.04);
        g.connect(out);
        for (const [ratio, part] of [[1, 1], [2, 0.18]]) {
          const o = ctx.createOscillator();
          const m = ctx.createGain();
          m.gain.value = part;
          o.frequency.setValueAtTime(extra.freq * ratio, t);
          o.frequency.setTargetAtTime(extra.freq * ratio * 1.9, t, 0.03);
          o.connect(m).connect(g);
          o.start(t);
          o.stop(t + 0.18);
        }
      } else if (kind === "like") {
        bell(1318.5, t, 0.16, out);
        bell(1975.5, t + 0.07, 0.14, out);
      } else if (kind === "unlike") {
        bell(987.8, t, 0.12, out);
      } else if (kind === "tap") {
        const o = ctx.createOscillator();
        const g = ctx.createGain();
        o.frequency.setValueAtTime(740 * 1.6, t);
        o.frequency.setTargetAtTime(740, t, 0.008);
        env(g, t, 0.07, 0.001, 0.022);
        o.connect(g).connect(out);
        o.start(t);
        o.stop(t + 0.12);
      }
    },
  };
}
