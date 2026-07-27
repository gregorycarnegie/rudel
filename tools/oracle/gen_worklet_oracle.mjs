// gen_worklet_oracle.mjs — audio goldens for the superdough AudioWorklet DSP
// that rudel ports by hand: the Moog `ladder-processor`, the orbit-bus
// `djf-processor`, and the `transient-processor` shaper.
//
//   cd tools/oracle && node gen_worklet_oracle.mjs
//
// Each processor's per-sample loop is copied verbatim from
// strudel/packages/superdough/worklets.mjs, with `sampleRate` fixed and the
// AudioWorklet scaffolding (parameter automation, block plumbing) removed.
// Every case is rendered **mono**, so the result is independent of the 128-frame
// block size and of upstream's per-channel loop ordering — the reference is then
// exactly "run this recurrence over these samples".
//
// crates/rudel-dsp/tests/worklet_golden.rs replays each case through rudel's
// ports and compares.

import { writeJson } from './lib.mjs';

const SAMPLE_RATE = 44100;
const N = 4096;
const PI = Math.PI;
const TWO_PI = 2 * PI;
const INVSR = 1 / SAMPLE_RATE;
const clamp = (num, min, max) => Math.min(Math.max(num, min), max);
const lerp = (a, b, n) => n * (b - a) + a;
const timeToCoeff = (t) => 1 - Math.exp(-INVSR / t);
const dbToLin = (db) => Math.pow(10, db / 20);
const fast_tanh = (x) => {
  const x2 = x ** 2;
  return (x * (27.0 + x2)) / (27.0 + 9.0 * x2);
};

// A deterministic broadband test signal: two sines plus a seeded LCG noise
// floor, so both sides drive the filters with identical input.
function testSignal(n) {
  const out = new Float64Array(n);
  let seed = 12345;
  for (let i = 0; i < n; i++) {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    const noise = (seed / 2147483648) * 2 - 1;
    const t = i * INVSR;
    out[i] = 0.5 * Math.sin(TWO_PI * 220 * t) + 0.3 * Math.sin(TWO_PI * 3300 * t) + 0.2 * noise;
  }
  return Array.from(out);
}

// --- ladder-processor (worklets.mjs) ---------------------------------------
function ladder(input, frequency, q, driveParam) {
  let p0 = 0,
    p1 = 0,
    p2 = 0,
    p3 = 0,
    p32 = 0,
    p33 = 0,
    p34 = 0;

  const resonance = q;
  const drive = clamp(Math.exp(driveParam), 0.1, 2000);

  let cutoff = frequency;
  cutoff = cutoff * TWO_PI * INVSR;
  cutoff = cutoff > 1 ? 1 : cutoff;

  const k = Math.min(8, resonance * 0.13);
  //               drive makeup  * resonance volume loss makeup
  let makeupgain = (1 / drive) * Math.min(1.75, 1 + k);

  const out = [];
  for (let n = 0; n < input.length; n++) {
    const o = p3 * 0.360891 + p32 * 0.41729 + p33 * 0.177896 + p34 * 0.0439725;

    p34 = p33;
    p33 = p32;
    p32 = p3;

    p0 += (fast_tanh(input[n] * drive - k * o) - fast_tanh(p0)) * cutoff;
    p1 += (fast_tanh(p0) - fast_tanh(p1)) * cutoff;
    p2 += (fast_tanh(p1) - fast_tanh(p2)) * cutoff;
    p3 += (fast_tanh(p2) - fast_tanh(p3)) * cutoff;

    out.push(o * makeupgain);
  }
  return out;
}

// --- djf-processor (worklets.mjs), incl. its TwoPoleFilter -----------------
function djf(input, value) {
  let s0 = 0,
    s1 = 0;
  const update = (s, cutoff, resonance = 0) => {
    resonance = clamp(resonance, 0, 1);
    cutoff = clamp(cutoff, 0, SAMPLE_RATE / 2 - 1);
    const c = clamp(2 * Math.sin(cutoff * PI * INVSR), 0, 1.14);
    const r = Math.pow(0.5, 8 * resonance + 1);
    const mrc = 1 - r * c;
    s0 = mrc * s0 - c * s1 + c * s; // bpf
    s1 = mrc * s1 + c * s0; // lpf
    return s1;
  };

  value = clamp(value, 0, 1);
  let filterType = 'none';
  let cutoff;
  let v = 1;
  if (value > 0.51) {
    filterType = 'hipass';
    v = (value - 0.5) * 2;
  } else if (value < 0.49) {
    filterType = 'lopass';
    v = value * 2;
  }
  cutoff = Math.pow(v * 11, 4);

  const out = [];
  for (let n = 0; n < input.length; n++) {
    if (filterType == 'none') {
      out.push(input[n]);
    } else {
      update(input[n], cutoff, 0.1);
      if (filterType === 'lopass') {
        out.push(s1);
      } else {
        out.push(input[n] - s1);
      }
    }
  }
  return out;
}

// --- transient-processor (worklets.mjs) ------------------------------------
// superdough only ever passes `attack` (the `transient` control) and `sustain`
// (`transsustain`); the rest keep the processor's own defaults.
function transient(input, attack, sustain) {
  const attackTime = clamp(0.003, 0.0005, 0.05);
  const sustainTime = clamp(0.08, 0.01, 0.5);
  const attackCoeff = timeToCoeff(attackTime);
  const sustainCoeff = timeToCoeff(sustainTime);
  const attackAmt = clamp(attack, -1, 1);
  const sustainAmt = clamp(sustain, -1, 1);
  const scaling = 0.5 + 5 * clamp(0.1, 0, 1); // sensitivity default 0.1
  const mix = clamp(1, 0, 1);
  const gainCoeff = timeToCoeff(0.2);

  let avgGain = 1;
  let attEnv = 0;
  let susEnv = 0;
  const out = [];
  for (let n = 0; n < input.length; n++) {
    const sample = input[n];
    const x = Math.abs(sample);
    attEnv = lerp(attEnv, x, attackCoeff);
    susEnv = lerp(susEnv, x, sustainCoeff);
    const peakiness = clamp((scaling * (attEnv - susEnv)) / (susEnv + 1e-6), -1.5, 1.5);
    const attScale = peakiness > 0 ? peakiness : 0;
    const susScale = peakiness < 0 ? -peakiness : 0;
    const attackGain = dbToLin(attackAmt * attScale * 18);
    const sustainGain = dbToLin(sustainAmt * susScale * 36);
    const gain = clamp(attackGain * sustainGain, 0, 8);
    avgGain = lerp(avgGain, gain, gainCoeff);
    const makeup = avgGain > 1e-3 ? 1 / avgGain : 1;
    const wet = sample * gain * makeup;
    let y = lerp(sample, wet, mix);
    y /= 1 + Math.abs(y); // soft clip
    out.push(y);
  }
  return out;
}

const input = testSignal(N);
const cases = {};

for (const c of [
  { frequency: 500, q: 1, drive: 0.69 }, // the worklet's own defaults
  { frequency: 200, q: 8, drive: 0.69 },
  { frequency: 4000, q: 0.5, drive: 2.0 },
  { frequency: 1000, q: 20, drive: -1.0 },
]) {
  cases[`ladder_${c.frequency}_${c.q}_${c.drive}`] = { kind: 'ladder', ...c, samples: ladder(input, c.frequency, c.q, c.drive) };
}

for (const value of [0, 0.25, 0.49, 0.5, 0.51, 0.75, 1]) {
  cases[`djf_${value}`] = { kind: 'djf', value, samples: djf(input, value) };
}

for (const c of [
  { attack: 1, sustain: 0 },
  { attack: -1, sustain: 0 },
  { attack: 0, sustain: 1 },
  { attack: 0, sustain: -1 },
  { attack: 0.5, sustain: -0.5 },
]) {
  cases[`transient_${c.attack}_${c.sustain}`] = { kind: 'transient', ...c, samples: transient(input, c.attack, c.sustain) };
}

writeJson('./worklet_golden.json', { sampleRate: SAMPLE_RATE, input, cases }, 0);
console.log(`wrote worklet_golden.json (${Object.keys(cases).length} cases)`);
