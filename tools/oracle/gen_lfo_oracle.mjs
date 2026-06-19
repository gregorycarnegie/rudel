// gen_lfo_oracle.mjs — audio golden for the LFO modulator source.
//
//   cd tools/oracle && node gen_lfo_oracle.mjs
//
// The `waveshapes` table and the per-sample loop are copied verbatim from
// strudel/packages/superdough/worklets.mjs (the `lfo-processor` AudioWorklet),
// with `sampleRate` fixed. crates/rudel-dsp/src/tests/... rebuilds each with
// rudel's `Lfo` and compares sample-for-sample.

import { writeFileSync } from 'node:fs';

const SAMPLE_RATE = 44100;
const TWO_PI = 2 * Math.PI;
const INVSR = 1 / SAMPLE_RATE;
const clamp = (num, min, max) => Math.min(Math.max(num, min), max);
const ffloor = (x) => x | 0;
const ffrac = (x) => x - ffloor(x);

function polyBlep(phase, dt) {
  const invdt = 1 / dt;
  if (phase < dt) {
    phase *= invdt;
    return 2 * phase - phase ** 2 - 1;
  } else if (phase > 1 - dt) {
    phase = (phase - 1) * invdt;
    return phase ** 2 + 2 * phase + 1;
  } else {
    return 0;
  }
}

// verbatim from worklets.mjs (order is significant: it indexes `shape`)
const waveshapes = {
  tri(phase, skew = 0.5) {
    const x = 1 - skew;
    if (phase >= skew) {
      return 1 / x - phase / x;
    }
    return phase / skew;
  },
  sine(phase) {
    return Math.sin(TWO_PI * phase) * 0.5 + 0.5;
  },
  ramp(phase) {
    return phase;
  },
  saw(phase) {
    return 1 - phase;
  },
  square(phase, skew = 0.5) {
    if (phase >= skew) {
      return 0;
    }
    return 1;
  },
  custom(phase, values = [0, 1]) {
    const numParts = values.length - 1;
    const currPart = Math.floor(phase * numParts);
    const partLength = 1 / numParts;
    const startVal = clamp(values[currPart], 0, 1);
    const endVal = clamp(values[currPart + 1], 0, 1);
    const slope = (endVal - startVal) / partLength;
    return slope * (phase - partLength * currPart) + startVal;
  },
  sawblep(phase, dt) {
    const v = 2 * phase - 1;
    return v - polyBlep(phase, dt);
  },
};
const waveShapeNames = Object.keys(waveshapes);

// One LFO render of `n` samples (the body of LFOProcessor.process).
function renderLfo(cfg, n) {
  const { shape, frequency, skew, depth, dcoffset, phaseoffset, curve, min, max, time } = cfg;
  let phase = ffrac(time * frequency + phaseoffset);
  const dt = frequency * INVSR;
  const out = [];
  for (let i = 0; i < n; i++) {
    let modval = (waveshapes[waveShapeNames[shape]](phase, skew) + dcoffset) * depth;
    modval = Math.pow(modval, curve);
    out.push(clamp(modval, min, max));
    phase += dt;
    if (phase > 1.0) phase -= 1;
  }
  return out;
}

const BIG = 1e9;
const base = {
  shape: 0,
  frequency: 2,
  skew: 0.5,
  depth: 1,
  dcoffset: -0.5,
  phaseoffset: 0,
  curve: 1,
  min: -BIG,
  max: BIG,
  time: 0,
};
const CASES = {
  tri: { ...base, shape: 0 },
  sine: { ...base, shape: 1, frequency: 3 },
  ramp: { ...base, shape: 2 },
  saw: { ...base, shape: 3 },
  square: { ...base, shape: 4, frequency: 4, skew: 0.3 },
  tri_skew: { ...base, shape: 0, skew: 0.25 },
  depth_dc: { ...base, shape: 1, depth: 0.5, dcoffset: 0 },
  phaseoffset: { ...base, shape: 2, phaseoffset: 0.25 },
  clamp: { ...base, shape: 1, depth: 2, min: -0.5, max: 0.5 },
  curve2: { ...base, shape: 1, dcoffset: 0, curve: 2 },
  sawblep: { ...base, shape: 6, frequency: 2, skew: 0.01 },
};

const N = 256;
const out = {};
for (const [label, cfg] of Object.entries(CASES)) {
  out[label] = { cfg, samples: renderLfo(cfg, N) };
}
writeFileSync(new URL('./lfo_golden.json', import.meta.url), JSON.stringify(out));
console.error(`wrote lfo_golden.json (${Object.keys(out).length} cases)`);
