// gen_adsr_oracle.mjs — audio golden for the linear ADSR gain envelope.
//
//   cd tools/oracle && node gen_adsr_oracle.mjs
//
// `getParamADSR`, `getSlope`, and `nanFallback` are copied verbatim from
// strudel/packages/superdough/{helpers,util}.mjs (helpers.mjs can't be imported
// directly because it pulls in the browser AudioContext). superdough drives the
// amplitude envelope with `getParamADSR(node.gain, a, d, s, r, 0, 1, t, holdEnd,
// 'linear')` (superdough/synth.mjs), scheduling Web Audio param automation
// events. We capture those events with a mock param, then sample the resulting
// automation curve using Web Audio's documented linear-ramp/setValue semantics.
// crates/rudel-dsp/tests/adsr_golden.rs samples rudel's `adsr_value` at the same
// times and compares the two curves.
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MockParam, getParamADSR, writeJson } from './lib.mjs';

const SAMPLE_RATE = 44100;


// attack, decay, sustain, release, duration (holdEnd - begin, seconds)
const cases = [
  { name: 'common', a: 0.01, d: 0.1, s: 0.6, r: 0.2, dur: 1.0 },
  { name: 'synth_defaults', a: 0.001, d: 0.05, s: 0.6, r: 0.01, dur: 0.5 },
  { name: 'attack_longer_than_duration', a: 0.5, d: 0.1, s: 0.6, r: 0.2, dur: 0.2 },
  { name: 'attack_plus_decay_exceeds_duration', a: 0.1, d: 0.5, s: 0.6, r: 0.2, dur: 0.3 },
  { name: 'zero_attack', a: 0.0, d: 0.1, s: 0.5, r: 0.1, dur: 0.4 },
  { name: 'zero_sustain', a: 0.01, d: 0.1, s: 0.0, r: 0.1, dur: 0.5 },
  { name: 'full_sustain_no_decay_drop', a: 0.05, d: 0.1, s: 1.0, r: 0.1, dur: 0.6 },
  { name: 'tiny_release', a: 0.01, d: 0.05, s: 0.6, r: 0.001, dur: 0.3 },
  { name: 'attack_equals_duration', a: 0.2, d: 0.1, s: 0.5, r: 0.1, dur: 0.2 },
];

const out = cases.map(({ name, a, d, s, r, dur }) => {
  const param = new MockParam();
  // superdough's gain envelope: min=0, max=1, begin=0, end=duration, linear.
  getParamADSR(param, a, d, s, r, 0, 1, 0, dur, 'linear');

  // sample the whole envelope: attack/decay/sustain region + release tail.
  const total = dur + r + 0.02;
  const n = Math.ceil(total * SAMPLE_RATE);
  const samples = new Array(n);
  for (let i = 0; i < n; i++) {
    samples[i] = param.valueAt(i / SAMPLE_RATE);
  }
  return { name, attack: a, decay: d, sustain: s, release: r, duration: dur, samples };
});

writeJson('./adsr_golden.json', { sampleRate: SAMPLE_RATE, cases: out });
console.log(`wrote adsr_golden.json: ${out.length} cases`);
