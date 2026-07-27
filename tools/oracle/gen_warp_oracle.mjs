// gen_warp_oracle.mjs — golden for the wavetable oscillator's phase warping.
//
//   cd tools/oracle && node gen_warp_oracle.mjs
//
// `WarpMode`, the integer helpers, `hash32`/`hash01`/`noise`/`brownian`,
// `bitReverse`, `_mirror`, `_toBits`, `_warpPhase` and `_sampleFrame` are copied
// verbatim from strudel/packages/superdough/worklets.mjs (the
// `wavetable-oscillator-processor` AudioWorklet).
// crates/rudel-dsp/tests/warp_golden.rs rebuilds each with rudel's `warp_phase`
// and compares value-for-value.

import { writeJson } from './lib.mjs';

const TWO_PI = 2 * Math.PI;
const clamp = (num, min, max) => Math.min(Math.max(num, min), max);
const frac = (x) => x - Math.floor(x);
const ffloor = (x) => x | 0;
const fround = (x) => ffloor(x + 0.5);
const ffrac = (x) => x - ffloor(x);

const WarpMode = Object.freeze({
  NONE: 0,
  ASYM: 1,
  MIRROR: 2,
  BENDP: 3,
  BENDM: 4,
  BENDMP: 5,
  SYNC: 6,
  QUANT: 7,
  FOLD: 8,
  PWM: 9,
  ORBIT: 10,
  SPIN: 11,
  CHAOS: 12,
  PRIMES: 13,
  BINARY: 14,
  BROWNIAN: 15,
  RECIPROCAL: 16,
  WORMHOLE: 17,
  LOGISTIC: 18,
  SIGMOID: 19,
  FRACTAL: 20,
  FLIP: 21,
});

function hash32(u) {
  u = u + 0x7ed55d16 + (u << 12);
  u = u ^ 0xc761c23c ^ (u >>> 19);
  u = u + 0x165667b1 + (u << 5);
  u = (u + 0xd3a2646c) ^ (u << 9);
  u = u + 0xfd7046c5 + (u << 3);
  u = u ^ 0xb55a4f09 ^ (u >>> 16);
  return u >>> 0;
}
const hash01 = (i) => (hash32(i) >>> 8) / 0x01000000;

function bitReverse(i, n) {
  let r = 0;
  for (let b = 0; b < n; b++) {
    r = (r << 1) | (i & 1);
    i >>>= 1;
  }
  return r;
}

function noise(x) {
  const i = Math.floor(x),
    f = x - i;
  const a = hash01(i),
    b = hash01(i + 1);
  return a + (b - a) * f;
}

function brownian(x, oct = 4) {
  let amp = 0.5,
    sum = 0,
    norm = 0,
    freq = 1;
  for (let o = 0; o < oct; o++) {
    sum += amp * noise(x * freq);
    norm += amp;
    amp *= 0.5;
    freq *= 2;
  }
  return (sum / norm) * 2 - 1;
}

const _mirror = (x) => 1 - Math.abs(2 * x - 1);
const _toBits = (amt, min = 2, max = 12) => {
  const b = max + (min - max) * amt;
  return { b, n: fround(Math.pow(2, b)) };
};

function _warpPhase(phase, amt, mode) {
  switch (mode) {
    case WarpMode.NONE:
      return phase;
    case WarpMode.ASYM: {
      const a = 0.01 + 0.99 * amt;
      return phase < a ? (0.5 * phase) / a : 0.5 + (0.5 * (phase - a)) / (1 - a);
    }
    case WarpMode.MIRROR:
      return _mirror(_warpPhase(phase, amt, WarpMode.ASYM));
    case WarpMode.BENDP:
      return Math.pow(phase, 1 + 3 * amt);
    case WarpMode.BENDM:
      return Math.pow(phase, 1 / (1 + 3 * amt));
    case WarpMode.BENDMP:
      return amt < 0.5 ? _warpPhase(phase, 1 - 2 * amt, 3) : _warpPhase(phase, 2 * amt - 1, 2);
    case WarpMode.SYNC: {
      const syncRatio = Math.pow(16, amt ** 2);
      return (phase * syncRatio) % 1;
    }
    case WarpMode.QUANT: {
      const { n } = _toBits(amt);
      return ffloor(phase * n) / n;
    }
    case WarpMode.FOLD: {
      const K = 7;
      const k = 1 + Math.max(1, fround(K * amt));
      return Math.abs(ffrac(k * phase) - 0.5) * 2;
    }
    case WarpMode.PWM: {
      const w = clamp(0.5 + 0.49 * (2 * amt - 1), 0, 1);
      if (phase < w) return (phase / w) * 0.5;
      return 0.5 + ((phase - w) / (1 - w)) * 0.5;
    }
    case WarpMode.ORBIT: {
      const depth = 0.5 * amt;
      const n = 3;
      return frac(phase + depth * Math.sin(TWO_PI * n * phase));
    }
    case WarpMode.SPIN: {
      const depth = 0.5 * amt;
      const { n } = _toBits(amt, 1, 6);
      return frac(phase + depth * Math.sin(TWO_PI * n * phase));
    }
    case WarpMode.CHAOS: {
      const r = 3.7 + 0.3 * amt;
      const logistic = r * phase * (1 - phase);
      return clamp((1 - amt) * phase + amt * logistic, 0, 1);
    }
    case WarpMode.PRIMES: {
      const isPrime = (n) => {
        if (n < 2) return false;
        if (n % 2 === 0) return n === 2;
        for (let d = 3; d ** 2 <= n; d += 2) if (n % d === 0) return false;
        return true;
      };
      let { n } = _toBits(amt, 3);
      while (!isPrime(n)) n++;
      return ffloor(phase * n) / n;
    }
    case WarpMode.BINARY: {
      let { b } = _toBits(amt, 3);
      b = fround(b);
      const n = 1 << b;
      const idx = ffloor(phase * n);
      const ridx = bitReverse(idx, b);
      return ridx / n;
    }
    case WarpMode.BROWNIAN: {
      const disp = 0.25 * amt * brownian(64 * phase, 4);
      return frac(phase + disp);
    }
    case WarpMode.RECIPROCAL: {
      const g = 2 + 4 * amt;
      const num = phase * g;
      const den = phase + (1 - phase) * g;
      const y = den > 1e-12 ? num / den : 0;
      return clamp(y, 0, 1);
    }
    case WarpMode.WORMHOLE: {
      const gap = clamp(0.8 * amt, 0, 1);
      const a = 0.5 * (1 - gap);
      const b = 0.5 * (1 + gap);
      if (phase < a) return (phase / a) * 0.5;
      if (phase > b) return 0.5 * (1 + (phase - b) / (1 - b));
      return 0.5;
    }
    case WarpMode.LOGISTIC: {
      let x = phase;
      const r = 3.6 + 0.4 * amt;
      const iters = 1 + fround(2 * amt);
      for (let i = 0; i < iters; i++) x = r * x * (1 - x);
      return clamp(x, 0, 1);
    }
    case WarpMode.SIGMOID: {
      const k = 1 + 10 * amt;
      const x = phase - 0.5;
      const y = 1 / (1 + Math.exp(-k * x));
      const y0 = 1 / (1 + Math.exp(0.5 * k));
      const y1 = 1 / (1 + Math.exp(-0.5 * k));
      return (y - y0) / (y1 - y0);
    }
    case WarpMode.FRACTAL: {
      const d = 0.5 * Math.sin(TWO_PI * phase) * amt;
      return frac(phase + d);
    }
    case WarpMode.FLIP:
      return phase;
    default:
      return phase;
  }
}

// A grid of phases across one cycle, at several warp amounts, per mode.
const PHASES = Array.from({ length: 64 }, (_, i) => i / 64);
const AMOUNTS = [0, 0.1, 0.25, 0.5, 0.75, 0.9, 1];

const out = {};
for (const [name, mode] of Object.entries(WarpMode)) {
  out[name] = {
    mode,
    amounts: AMOUNTS,
    phases: PHASES,
    // Row-major: one row per amount, one value per phase. Math.fround keeps
    // the comparison honest against rudel's f32 arithmetic.
    values: AMOUNTS.map((amt) => PHASES.map((p) => Math.fround(_warpPhase(p, amt, mode)))),
  };
}
writeJson('./warp_golden.json', out);
console.error(`wrote warp_golden.json (${Object.keys(out).length} modes)`);
