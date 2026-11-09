// "First Light": a short waltz for harp and ocarina made in code (no recording, so no rights to clear),
// played through Tonal's six equalizer bands, plus the app's interface sounds
// from src/sfx.rs. Nothing makes a sound until the visitor clicks something.

const BPM = 92;
const STEP = 60 / BPM / 4;
const BAR = 12;
const BARS = 20;
const STEPS = BARS * BAR;
export const DURATION = STEPS * STEP;

const D = [50, 4], G = [55, 4], A = [45, 4], Bm = [47, 3], Em = [52, 3], Fsm = [54, 3];
const CHORDS = [D, G, D, A, D, G, Bm, A, D, G, Em, A, G, A, Fsm, Bm, G, A, D, D];
const MELODY = [
  [], [], [], [],
  [[0, 74, 6], [6, 78, 2], [8, 81, 4]],
  [[0, 83, 8], [8, 81, 2], [10, 79, 2]],
  [[0, 78, 6], [6, 74, 2], [8, 76, 4]],
  [[0, 73, 4], [4, 76, 8]],
  [[0, 74, 6], [6, 78, 2], [8, 81, 4]],
  [[0, 86, 8], [8, 83, 2], [10, 81, 2]],
  [[0, 79, 4], [4, 78, 4], [8, 76, 4]],
  [[0, 81, 12]],
  [[0, 83, 4], [4, 81, 2], [6, 79, 2], [8, 83, 4]],
  [[0, 85, 8], [8, 83, 2], [10, 81, 2]],
  [[0, 81, 4], [4, 78, 4], [8, 73, 4]],
  [[0, 74, 8], [8, 76, 2], [10, 78, 2]],
  [[0, 79, 6], [6, 83, 2], [8, 86, 4]],
  [[0, 88, 6], [6, 86, 2], [8, 85, 4]],
  [[0, 86, 12]],
  [],
];

/// The song has no words, so the synced lines follow what joins and leaves the mix.
export const LYRICS = [
  [0, "♪ A harp, softly"],
  [4, "♪ The ocarina comes in"],
  [12, "♪ Strings and drums join"],
  [18, "♪ And back to the harp"],
].map(([bar, text]) => ({ text, time: bar * BAR * STEP }));

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
  let ctx = null, out, musicBus, eq = [], preamp, analyser, data, noise, verb, echo;
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
    room();
    applyEq();
  }

  function env(gainNode, t, peak, attack, decay) {
    gainNode.gain.setValueAtTime(0, t);
    gainNode.gain.linearRampToValueAtTime(peak, t + attack);
    gainNode.gain.setTargetAtTime(0, t + attack, decay);
  }

  function room() {
    const length = ctx.sampleRate * 2.4;
    const impulse = ctx.createBuffer(2, length, ctx.sampleRate);
    for (let c = 0; c < 2; c++) {
      const d = impulse.getChannelData(c);
      for (let i = 0; i < length; i++) d[i] = (Math.random() * 2 - 1) * (1 - i / length) ** 3;
    }
    verb = ctx.createConvolver();
    verb.buffer = impulse;
    const back = ctx.createGain();
    back.gain.value = 0.32;
    verb.connect(back).connect(musicBus);
    echo = ctx.createDelay(1);
    echo.delayTime.value = STEP * 3;
    const feedback = ctx.createGain();
    feedback.gain.value = 0.32;
    const tone = ctx.createBiquadFilter();
    tone.type = "lowpass";
    tone.frequency.value = 2400;
    echo.connect(tone).connect(feedback).connect(echo);
    const wet = ctx.createGain();
    wet.gain.value = 0.35;
    tone.connect(wet).connect(musicBus);
  }

  function send(node, dry, wet, delayed = 0) {
    const d = ctx.createGain();
    d.gain.value = dry;
    node.connect(d).connect(musicBus);
    const w = ctx.createGain();
    w.gain.value = wet;
    node.connect(w).connect(verb);
    if (delayed) {
      const e = ctx.createGain();
      e.gain.value = delayed;
      node.connect(e).connect(echo);
    }
  }

  function harp(note, t, level) {
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(level, t + 0.004);
    g.gain.setTargetAtTime(0, t + 0.004, 0.55);
    const lp = ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.setValueAtTime(4200, t);
    lp.frequency.setTargetAtTime(1400, t, 0.25);
    g.connect(lp);
    send(lp, 0.8, 0.6);
    for (const [type, mul, ratio] of [["triangle", 1, 1], ["sine", 0.3, 2], ["sine", 0.08, 3]]) {
      const o = ctx.createOscillator();
      const m = ctx.createGain();
      m.gain.value = mul;
      o.type = type;
      o.frequency.value = hz(note) * ratio;
      o.connect(m).connect(g);
      o.start(t);
      o.stop(t + 3.5);
    }
  }

  function ocarina(note, t, length) {
    const f = hz(note);
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(0.13, t + 0.05);
    g.gain.setTargetAtTime(0.1, t + 0.05, 0.25);
    g.gain.setTargetAtTime(0, t + length, 0.07);
    send(g, 0.85, 0.75, 0.35);
    const vibrato = ctx.createOscillator();
    vibrato.frequency.value = 5;
    const amount = ctx.createGain();
    amount.gain.setValueAtTime(0, t);
    amount.gain.linearRampToValueAtTime(0, t + 0.25);
    amount.gain.linearRampToValueAtTime(length > 0.6 ? 14 : 5, t + 0.6);
    vibrato.connect(amount);
    for (const [mul, ratio] of [[1, 1], [0.07, 2], [0.025, 3]]) {
      const o = ctx.createOscillator();
      const m = ctx.createGain();
      m.gain.value = mul;
      o.frequency.value = f * ratio;
      amount.connect(o.detune);
      o.connect(m).connect(g);
      o.start(t);
      o.stop(t + length + 0.5);
    }
    vibrato.start(t);
    vibrato.stop(t + length + 0.5);
    const s = ctx.createBufferSource();
    s.buffer = noise;
    const bp = ctx.createBiquadFilter();
    bp.type = "bandpass";
    bp.frequency.value = f * 2;
    bp.Q.value = 3;
    const breath = ctx.createGain();
    breath.gain.setValueAtTime(0, t);
    breath.gain.linearRampToValueAtTime(0.03, t + 0.02);
    breath.gain.setTargetAtTime(0.006, t + 0.02, 0.08);
    breath.gain.setTargetAtTime(0, t + length, 0.05);
    s.connect(bp).connect(breath).connect(g);
    s.start(t, Math.random() * 0.5);
    s.stop(t + length + 0.4);
  }

  function strings(chord, t, level, length) {
    const [root, third] = chord;
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(level, t + 0.7);
    g.gain.setTargetAtTime(0, t + length, 0.35);
    const lp = ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.value = 1100;
    g.connect(lp);
    send(lp, 0.6, 0.8);
    for (const note of [root + 12, root + 12 + third, root + 19]) {
      for (const detune of [-7, 7]) {
        const o = ctx.createOscillator();
        o.type = "sawtooth";
        o.frequency.value = hz(note);
        o.detune.value = detune;
        o.connect(g);
        o.start(t);
        o.stop(t + length + 1.5);
      }
    }
  }

  function low(note, t, length) {
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(0.22, t + 0.03);
    g.gain.setTargetAtTime(0, t + length * 0.6, 0.3);
    g.connect(musicBus);
    const o = ctx.createOscillator();
    o.frequency.value = hz(note);
    o.connect(g);
    o.start(t);
    o.stop(t + length + 1.5);
  }

  function drum(t, level) {
    const o = ctx.createOscillator();
    const g = ctx.createGain();
    o.frequency.setValueAtTime(110, t);
    o.frequency.exponentialRampToValueAtTime(62, t + 0.25);
    env(g, t, level, 0.004, 0.22);
    o.connect(g);
    send(g, 1, 0.5);
    o.start(t);
    o.stop(t + 1.2);
  }

  function hiss(t, type, f, q, peak, decay, wet = 0) {
    const s = ctx.createBufferSource();
    s.buffer = noise;
    const filter = ctx.createBiquadFilter();
    filter.type = type;
    filter.frequency.value = f;
    filter.Q.value = q;
    const g = ctx.createGain();
    env(g, t, peak, 0.002, decay);
    s.connect(filter).connect(g);
    send(g, 1, wet);
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
    const bar = Math.floor(s / BAR) % BARS, at = s % BAR, chord = CHORDS[bar];
    const [root, third] = chord;
    const melody = bar >= 4 && bar < 19;
    const big = bar >= 12 && bar < 19;
    const arp = [root, root + 7, root + 12, root + 12 + third, root + 19, root + 12];
    if (at % 2 === 0) harp(arp[at / 2], t, melody ? 0.07 : 0.09);
    if (at === 0 && melody) low(root - 12, t, BAR * STEP);
    if (at === 0 && bar >= 8 && bar < 19) strings(chord, t, big ? 0.03 : 0.016, BAR * STEP);
    if (big && at === 0) drum(t, 0.35);
    if (big && at === 8 && bar % 2) drum(t, 0.18);
    if (big && at % 2 === 0) hiss(t, "highpass", 7000, 0.7, at % 4 ? 0.008 : 0.014, 0.03);
    if (melody) for (const [when, note, length] of MELODY[bar]) if (when === at) ocarina(note, t, length * STEP);
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
