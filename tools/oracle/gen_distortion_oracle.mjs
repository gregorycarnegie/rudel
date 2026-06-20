// gen_distortion_oracle.mjs — audio golden for the waveshaping distortion algos.
//
//   cd tools/oracle && node gen_distortion_oracle.mjs
//
// `distortionAlgorithms` (and the `clamp`/`_mod`/`__squash` helpers) are copied
// verbatim from strudel/packages/superdough/{helpers,util}.mjs — helpers.mjs
// can't be imported directly because it pulls in the browser AudioContext. Each
// algorithm `shape(x, k)` is the exact closed-form waveshaper superdough's
// distort-processor runs (k = e^distort - 1). crates/rudel-dsp/tests/
// distortion_golden.rs evaluates rudel's `DistortAlgo::shape` over the same
// (algorithm, x, k) grid and compares sample-for-sample.
// SPDX-License-Identifier: AGPL-3.0-or-later

import { writeFileSync } from 'node:fs';

// --- verbatim from superdough/util.mjs --------------------------------------
const clamp = (num, min, max) => Math.min(Math.max(num, min), max);

// --- verbatim from superdough/helpers.mjs (saturation curves) ---------------
const __squash = (x) => x / (1 + x); // [0, inf) to [0, 1)
const _mod = (n, m) => ((n % m) + m) % m;

const _scurve = (x, k) => ((1 + k) * x) / (1 + k * Math.abs(x));
const _soft = (x, k) => Math.tanh(x * (1 + k));
const _hard = (x, k) => clamp((1 + k) * x, -1, 1);

const _fold = (x, k) => {
  // Closed form folding for audio rate
  let y = (1 + 0.5 * k) * x;
  const window = _mod(y + 1, 4);
  return 1 - Math.abs(window - 2);
};

const _sineFold = (x, k) => Math.sin((Math.PI / 2) * _fold(x, k));

const _cubic = (x, k) => {
  const t = __squash(Math.log1p(k));
  const cubic = (x - (t / 3) * x * x * x) / (1 - t / 3); // normalized to go from (-1, 1)
  return _soft(cubic, k);
};

const _diode = (x, k, asym = false) => {
  const g = 1 + 2 * k; // gain
  const t = __squash(Math.log1p(k));
  const bias = 0.07 * t;
  const pos = _soft(x + bias, 2 * k);
  const neg = _soft(asym ? bias : -x + bias, 2 * k);
  const y = pos - neg;
  const sech = 1 / Math.cosh(g * bias);
  const sech2 = sech * sech; // derivative of soft (i.e. tanh) is sech^2
  const denom = Math.max(1e-8, (asym ? 1 : 2) * g * sech2); // g from chain rule; 2 if both pos/neg have x
  return _soft(y / denom, k);
};

const _asym = (x, k) => _diode(x, k, true);

const _chebyshev = (x, k) => {
  const kl = 10 * Math.log1p(k);
  let tnm1 = 1;
  let tnm2 = x;
  let tn;
  let y = 0;
  for (let i = 1; i < 64; i++) {
    if (i < 2) {
      y += i == 0 ? tnm1 : tnm2;
      continue;
    }
    tn = 2 * x * tnm1 - tnm2;
    tnm2 = tnm1;
    tnm1 = tn;
    if (i % 2 === 0) {
      y += Math.min((1.3 * kl) / i, 2) * tn;
    }
  }
  return _soft(y, kl / 20);
};

const distortionAlgorithms = {
  scurve: _scurve,
  soft: _soft,
  hard: _hard,
  cubic: _cubic,
  diode: _diode,
  asym: _asym,
  fold: _fold,
  sinefold: _sineFold,
  chebyshev: _chebyshev,
};

// Input sweep and drive values. The drives are k = e^distort - 1 for distort in
// {0, 0.5, 1, 2, 4} — the worklet's `shape` after the exp mapping.
const xs = [-1.5, -1.0, -0.6, -0.3, -0.05, 0.0, 0.05, 0.3, 0.6, 1.0, 1.5];
const ks = [0, 0.5, 1, 2, 4].map((d) => Math.exp(d) - 1);

const cases = Object.keys(distortionAlgorithms).map((name) => {
  const fn = distortionAlgorithms[name];
  const samples = [];
  for (const k of ks) {
    for (const x of xs) {
      samples.push(fn(x, k));
    }
  }
  return { name, samples };
});

writeFileSync(
  new URL('./distortion_golden.json', import.meta.url),
  JSON.stringify({ xs, ks, cases }),
);
console.log(`wrote distortion_golden.json: ${cases.length} algorithms, ${xs.length * ks.length} samples each`);
