// gen_oscillator_oracle.mjs — audio goldens for the two parts of rudel's
// oscillator module that have a real upstream: the additive (`partials`)
// wavetable builder, and the pink/brown noise colouring filters.
//
//   cd tools/oracle && node gen_oscillator_oracle.mjs
//
// ## additive
//
// The coefficient loop is copied verbatim from `waveformN` in
// strudel/packages/superdough/synth.mjs, which hands its `real`/`imag` arrays to
// `ac.createPeriodicWave(real, imag)`. Web Audio then synthesises
//
//     x(t) = sum_n  real[n]*cos(2*pi*n*t) + imag[n]*sin(2*pi*n*t)
//
// and, because `disableNormalization` is left false, scales the result to a peak
// of 1. That synthesis is written out here from the spec — scalar and in f64 —
// against rudel's eight-lane f32 version, so the two are independent
// implementations of the same definition rather than one copied from the other.
//
// ## noise
//
// The pink and brown recurrences are copied verbatim from `getNoiseBuffer` in
// strudel/packages/superdough/noise.mjs. Upstream drives them with
// `Math.random()`, which cannot be reproduced, so both sides are driven by
// rudel's own xorshift white source instead — ported below, `Math.fround`ed at
// the same points rudel rounds to f32. What is under test is therefore the
// colouring filter, which is the part that came from upstream; the white source
// is rudel's own and is checked separately by the `white` case.
//
// crates/rudel-dsp/src/tests/oscillator.rs replays each case.
// SPDX-License-Identifier: AGPL-3.0-or-later

import { writeJson } from './lib.mjs';

const PI2 = 2 * Math.PI;
const ADDITIVE_SIZE = 2048;
const NOISE_SAMPLES = 1024;

// --- verbatim from synth.mjs `waveformN` ------------------------------------
const terms = {
  sawtooth: (n) => [0, -1 / n],
  square: (n) => [0, n % 2 === 0 ? 0 : 1 / n],
  triangle: (n) => [n % 2 === 0 ? 0 : 1 / (n * n), 0],
  user: (_n) => [0, 1],
};

function coefficients(partials, phases, type) {
  const len = partials.length;
  const real = new Float64Array(len + 1);
  const imag = new Float64Array(len + 1);
  for (let n = 0; n < len; n++) {
    const mag = partials[n];
    const [r, i] = terms[type](n + 1); // n === 0 is the dc offset, skipped
    const phase = phases?.[n] ?? 0;
    let R = r * mag;
    let I = i * mag;
    if (phase !== 0) {
      const c = Math.cos(PI2 * phase);
      const s = Math.sin(PI2 * phase);
      const R0 = R;
      const I0 = I;
      R = c * R0 - s * I0;
      I = s * R0 + c * I0;
    }
    real[n + 1] = R;
    imag[n + 1] = I;
  }
  return { real, imag };
}
// --- end verbatim -----------------------------------------------------------

// Web Audio PeriodicWave synthesis + default normalisation, from the spec.
function buildTable(partials, phases, type) {
  const { real, imag } = coefficients(partials, phases, type);
  const table = new Float64Array(ADDITIVE_SIZE);
  for (let s = 0; s < ADDITIVE_SIZE; s++) {
    const t = s / ADDITIVE_SIZE;
    let acc = 0;
    for (let n = 1; n < real.length; n++) {
      acc += real[n] * Math.cos(PI2 * n * t) + imag[n] * Math.sin(PI2 * n * t);
    }
    table[s] = acc;
  }
  let peak = 0;
  for (const x of table) peak = Math.max(peak, Math.abs(x));
  if (peak > 1e-9) {
    for (let s = 0; s < ADDITIVE_SIZE; s++) table[s] /= peak;
  }
  return Array.from(table);
}

// rudel's xorshift32 white source (crates/rudel-dsp/src/oscillator.rs), kept in
// u32 with `>>> 0` and rounded through f32 where rudel does.
function makeWhite(seed = 0x12345678) {
  let rng = seed >>> 0;
  const U32_MAX = Math.fround(4294967295);
  return () => {
    let x = rng;
    x = (x ^ (x << 13)) >>> 0;
    x = (x ^ (x >>> 17)) >>> 0;
    x = (x ^ (x << 5)) >>> 0;
    rng = x;
    return Math.fround(Math.fround(Math.fround(x) / U32_MAX) * 2 - 1);
  };
}

// --- verbatim from noise.mjs `getNoiseBuffer` -------------------------------
function noise(type, n) {
  const white = makeWhite();
  const out = new Float64Array(n);
  let lastOut = 0;
  let b0, b1, b2, b3, b4, b5, b6;
  b0 = b1 = b2 = b3 = b4 = b5 = b6 = 0.0;
  for (let i = 0; i < n; i++) {
    const w = white();
    if (type === 'white') {
      out[i] = w;
    } else if (type === 'brown') {
      out[i] = (lastOut + 0.02 * w) / 1.02;
      lastOut = out[i];
    } else if (type === 'pink') {
      b0 = 0.99886 * b0 + w * 0.0555179;
      b1 = 0.99332 * b1 + w * 0.0750759;
      b2 = 0.969 * b2 + w * 0.153852;
      b3 = 0.8665 * b3 + w * 0.3104856;
      b4 = 0.55 * b4 + w * 0.5329522;
      b5 = -0.7616 * b5 - w * 0.016898;
      out[i] = b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362;
      out[i] *= 0.11;
      b6 = w * 0.115926;
    }
  }
  return Array.from(out);
}
// --- end verbatim -----------------------------------------------------------

const ones = (n) => Array.from({ length: n }, () => 1);

const additiveCases = [
  { name: 'saw_8_flat_partials', type: 'sawtooth', partials: ones(8) },
  { name: 'square_12_flat_partials', type: 'square', partials: ones(12) },
  { name: 'triangle_6_flat_partials', type: 'triangle', partials: ones(6) },
  // `user` takes its whole spectrum from the magnitudes, so a rolloff here is
  // the only thing distinguishing the harmonics.
  { name: 'user_rolloff', type: 'user', partials: [1, 0.5, 0.25, 0.125, 0.0625] },
  // Per-harmonic phase rotation: the quarter-turn cases swap the sine and
  // cosine halves of each term, which a rotation applied in the wrong
  // direction (or to the wrong component) gets wrong.
  {
    name: 'saw_with_phase_rotation',
    type: 'sawtooth',
    partials: ones(6),
    phases: [0, 0.25, 0.5, 0.75, 0.125, 0.375],
  },
  { name: 'square_odd_magnitudes', type: 'square', partials: [1, 0, 0.7, 0, 0.4, 0, 0.2] },
  { name: 'single_partial_is_a_sine', type: 'sawtooth', partials: [1] },
  // Triangle is the only base with a non-zero *real* term, so it is the only
  // case where scaling by the magnitude and rotating by the phase are visible
  // on both components at once. Flat magnitudes would leave the `r * mag`
  // multiply indistinguishable from a divide.
  {
    name: 'triangle_scaled_with_phase_rotation',
    type: 'triangle',
    partials: [1, 0.8, 0.6, 0.4, 0.2],
    phases: [0.1, 0.2, 0.3, 0.4, 0.5],
  },
];

const out = {
  additive_size: ADDITIVE_SIZE,
  noise_samples: NOISE_SAMPLES,
  additive: additiveCases.map((c) => ({
    ...c,
    phases: c.phases ?? null,
    table: buildTable(c.partials, c.phases, c.type),
  })),
  noise: ['white', 'pink', 'brown'].map((type) => ({
    name: type,
    samples: noise(type, NOISE_SAMPLES),
  })),
};

writeJson('oscillator_golden.json', out);
console.log(
  `wrote oscillator_golden.json: ${out.additive.length} additive tables x ${ADDITIVE_SIZE}, ` +
    `${out.noise.length} noise runs x ${NOISE_SAMPLES}`,
);
