// gen_modenv_oracle.mjs — audio golden for the modulation envelope source
// (`env(...)`), the companion to gen_lfo_oracle.mjs.
//
//   cd tools/oracle && node gen_modenv_oracle.mjs
//
// The state machine and `_warp`/`_advance` helpers are copied verbatim from the
// `envelope-processor` AudioWorklet in strudel/packages/superdough/worklets.mjs,
// with `sampleRate` fixed, `begin` at 0 and the a-rate parameter arrays
// collapsed to scalars. Each voice gets a fresh envelope at its own onset, so
// the worklet's begin-change/retrigger bookkeeping never fires and is dropped.
//
// crates/rudel-dsp/tests/modenv_golden.rs rebuilds each with rudel's `ModEnv`
// and compares sample-for-sample. Only every STRIDE-th sample is stored, to keep
// the golden small — the Rust side still ticks every sample, so the comparison
// covers the whole recurrence.

import { writeJson } from './lib.mjs';

const SAMPLE_RATE = 44100;
const clamp = (num, min, max) => Math.min(Math.max(num, min), max);

function renderEnv(cfg, n) {
  const {
    attack,
    decay,
    sustain,
    release,
    attackCurve = 0,
    decayCurve = 0,
    releaseCurve = 0,
    depth = 1,
    min = -1e9,
    max = 1e9,
    susTime,
  } = cfg;

  let val = 0;
  let state = 1; // the worklet enters the attack segment once `begin` passes
  const beginTime = 0;

  const _warp = (phase, curvature, strength = 8) => {
    if (phase === 0 || phase === 1) return phase; // fast exit
    if (curvature > 0) {
      // snappier
      const exp = 1 + strength * curvature;
      return 1 - Math.pow(1 - phase, exp);
    } else {
      // more calm
      const exp = 1 - strength * curvature;
      return Math.pow(phase, exp);
    }
  };

  const out = [];
  for (let i = 0; i < n; i++) {
    const currentTime = i / SAMPLE_RATE;
    const states = [
      { time: Number.POSITIVE_INFINITY, start: 0, target: 0 }, // idle
      { time: attack, start: 0, target: 1, curve: attackCurve },
      { time: attack + decay, start: 1, target: sustain, curve: decayCurve },
      { time: susTime, start: sustain, target: sustain },
      { time: susTime + release, start: sustain, target: 0, curve: releaseCurve },
    ];
    let { time, start, target, curve } = states[state];

    // _advance
    if (time === 0 || start === target) {
      val = target;
    } else {
      const phase = Math.min(1, (currentTime - beginTime) / time);
      val = start + (target - start) * _warp(phase, curve);
    }

    while (currentTime - beginTime >= time) {
      state = (state + 1) % states.length;
      time = states[state].time;
    }
    out.push(clamp(val * depth, min, max));
  }
  return out;
}

// The processor's own parameter defaults, from its parameterDescriptors.
const base = {
  attack: 0.005,
  decay: 0.14,
  sustain: 0,
  release: 0.1,
  susTime: 0.5,
};
const CASES = {
  defaults: { ...base },
  sustained: { ...base, attack: 0.02, decay: 0.05, sustain: 0.7, release: 0.2 },
  snappy: { ...base, attack: 0.05, sustain: 0.5, attackCurve: 0.8, decayCurve: 0.8 },
  calm: { ...base, attack: 0.05, sustain: 0.5, attackCurve: -0.8, decayCurve: -0.8 },
  release_curve: { ...base, sustain: 0.6, susTime: 0.2, release: 0.3, releaseCurve: -0.5 },
  depth: { ...base, sustain: 0.5, depth: 400 },
  clamped: { ...base, sustain: 1, depth: 10, min: -2, max: 2 },
  zero_attack: { ...base, attack: 0, decay: 0.1, sustain: 0.4 },
  long_hold: { ...base, sustain: 0.8, susTime: 1.5 },
};

const N = 44100; // one second, long enough to cross every segment boundary
const STRIDE = 32;
const out = { stride: STRIDE, length: N, cases: {} };
for (const [label, cfg] of Object.entries(CASES)) {
  out.cases[label] = { cfg, samples: renderEnv(cfg, N).filter((_, i) => i % STRIDE === 0) };
}
writeJson('./modenv_golden.json', out);
console.error(`wrote modenv_golden.json (${Object.keys(out.cases).length} cases)`);
