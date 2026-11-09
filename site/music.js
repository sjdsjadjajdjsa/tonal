// "First Light": a short loop made in code (no recording, so no rights to clear),
// played through Tonal's six equalizer bands, plus the app's interface sounds
// from src/sfx.rs. Nothing makes a sound until the visitor clicks something.

const BPM = 90;
const STEP = 60 / BPM / 4;
const BARS = 16;
const STEPS = BARS * 16;
export const DURATION = STEPS * STEP;

const CHORDS = [[53, 57, 60, 64], [53, 57, 59, 64], [52, 55, 59, 62], [55, 59, 60, 64]];
const ROOTS = [38, 43, 36, 45];
const LEAD = [
  [[[2, 74, 2], [4, 76, 2], [6, 77, 4], [10, 76, 2], [12, 74, 4]],
   [[2, 71, 2], [4, 74, 2], [6, 76, 6], [12, 74, 2], [14, 71, 2]],
   [[0, 72, 4], [4, 71, 2], [6, 67, 6], [12, 69, 2], [14, 71, 2]],
   [[0, 72, 6], [8, 76, 4], [12, 79, 4]]],
  [[[2, 77, 2], [4, 79, 2], [6, 81, 4], [10, 79, 2], [12, 77, 4]],
   [[2, 74, 2], [4, 77, 2], [6, 79, 6], [12, 77, 2], [14, 74, 2]],
   [[0, 76, 4], [4, 74, 2], [6, 71, 6], [12, 72, 2], [14, 74, 2]],
   [[0, 76, 8], [8, 72, 4], [12, 69, 4]]],
];

/// The song has no words, so the synced lines follow what joins and leaves the mix.
export const LYRICS = [
  [0, "♪ Keys and a soft hi-hat"],
  [4, "♪ Drums and bass come in"],
  [8, "♪ The melody"],
  [12, "♪ The melody, higher"],
].map(([bar, text]) => ({ text, time: bar * 16 * STEP }));

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

  function piano(note, t, level, length) {
    const f = hz(note);
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(level, t + 0.006);
    g.gain.setTargetAtTime(level * 0.35, t + 0.006, 0.35);
    g.gain.setTargetAtTime(0, t + length, 0.18);
    const lp = ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.setValueAtTime(3200, t);
    lp.frequency.setTargetAtTime(1300, t, 0.4);
    g.connect(lp);
    send(lp, 0.9, 0.45);
    const carrier = ctx.createOscillator();
    carrier.frequency.value = f;
    const mod = ctx.createOscillator();
    mod.frequency.value = f;
    const depth = ctx.createGain();
    depth.gain.setValueAtTime(f * 1.4, t);
    depth.gain.setTargetAtTime(f * 0.15, t, 0.12);
    mod.connect(depth).connect(carrier.frequency);
    carrier.connect(g);
    for (const o of [carrier, mod]) {
      o.start(t);
      o.stop(t + length + 1);
    }
  }

  function keys(chord, t, level, length) {
    chord.forEach((note, i) => piano(note, t + i * 0.008, level, length));
  }

  function bass(note, t, length) {
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(0.3, t + 0.012);
    g.gain.setTargetAtTime(0.2, t + 0.012, 0.15);
    g.gain.setTargetAtTime(0, t + length, 0.05);
    const lp = ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.setValueAtTime(700, t);
    lp.frequency.setTargetAtTime(260, t, 0.08);
    g.connect(lp).connect(musicBus);
    for (const [type, mul] of [["sine", 1], ["triangle", 0.35]]) {
      const o = ctx.createOscillator();
      const m = ctx.createGain();
      m.gain.value = mul;
      o.type = type;
      o.frequency.value = hz(note);
      o.connect(m).connect(g);
      o.start(t);
      o.stop(t + length + 0.4);
    }
  }

  function lead(note, t, length) {
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(0.075, t + 0.03);
    g.gain.setTargetAtTime(0.055, t + 0.03, 0.2);
    g.gain.setTargetAtTime(0, t + length, 0.08);
    const lp = ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.value = 2600;
    g.connect(lp);
    send(lp, 1, 0.5, 0.6);
    const vibrato = ctx.createOscillator();
    vibrato.frequency.value = 5.2;
    const amount = ctx.createGain();
    amount.gain.setValueAtTime(0, t);
    amount.gain.linearRampToValueAtTime(9, t + 0.4);
    vibrato.connect(amount);
    for (const [type, mul, detune] of [["triangle", 1, 0], ["sine", 0.5, 1200], ["sawtooth", 0.06, 4]]) {
      const o = ctx.createOscillator();
      const m = ctx.createGain();
      m.gain.value = mul;
      o.type = type;
      o.frequency.value = hz(note);
      o.detune.value = detune;
      amount.connect(o.detune);
      o.connect(m).connect(g);
      o.start(t);
      o.stop(t + length + 0.6);
    }
    vibrato.start(t);
    vibrato.stop(t + length + 0.6);
  }

  function kick(t, level = 0.6) {
    const o = ctx.createOscillator();
    const g = ctx.createGain();
    o.frequency.setValueAtTime(150, t);
    o.frequency.exponentialRampToValueAtTime(48, t + 0.09);
    env(g, t, level, 0.002, 0.13);
    o.connect(g).connect(musicBus);
    o.start(t);
    o.stop(t + 0.6);
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

  function snare(t) {
    hiss(t, "bandpass", 2200, 0.8, 0.2, 0.06, 0.5);
    const o = ctx.createOscillator();
    const g = ctx.createGain();
    o.frequency.setValueAtTime(220, t);
    o.frequency.exponentialRampToValueAtTime(160, t + 0.05);
    env(g, t, 0.18, 0.001, 0.04);
    o.connect(g).connect(musicBus);
    o.start(t);
    o.stop(t + 0.3);
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
    const bar = Math.floor(s / 16) % BARS, beat = s % 16, chord = bar % 4;
    const swing = beat % 2 ? STEP * 0.16 : 0;
    const drums = bar >= 4;
    if (beat === 0) keys(CHORDS[chord], t, 0.06, STEP * 5);
    if (beat === 6) keys(CHORDS[chord], t + swing, 0.04, STEP * 3);
    if (beat === 10) keys(CHORDS[chord].slice(1), t, 0.035, STEP * 5);
    if (beat % 2 === 0) hiss(t, "highpass", 8000, 0.7, beat % 4 ? 0.03 : 0.045, 0.022);
    if (beat % 2 && drums) hiss(t + swing, "highpass", 9000, 0.7, 0.012, 0.015);
    if (beat === 14 && drums) hiss(t, "highpass", 7000, 0.7, 0.04, 0.09);
    if (drums) {
      if (beat === 0 || beat === 10 || (beat === 7 && bar % 2)) kick(t + swing, beat === 7 ? 0.4 : 0.6);
      if (beat === 4 || beat === 12) snare(t);
      if (beat === 15 && bar % 4 === 3) snare(t + swing);
      if (beat === 0) bass(ROOTS[chord], t, STEP * 5);
      if (beat === 6) bass(ROOTS[chord] + 12, t + swing, STEP * 1.5);
      if (beat === 10) bass(ROOTS[chord] + 7, t, STEP * 3);
      if (beat === 14) bass(ROOTS[chord] - 2, t, STEP * 1.5);
    }
    if (bar >= 8 && bar < 16) {
      for (const [at, note, length] of LEAD[bar < 12 ? 0 : 1][chord]) if (at === beat) lead(note, t, length * STEP);
    }
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
