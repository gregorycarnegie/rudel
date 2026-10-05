// gen_pulse_oracle.mjs - superdough's `pulse` oscillator, sample for sample.
//
// `s("pulse")` is the `pulse-oscillator` AudioWorklet: two half-Tomisawa
// feedback oscillators, the second offset by the pulse width. This runs that
// processor from strudel/packages/superdough/worklets.mjs (via
// worklet_harness.mjs) and records its output.
//
//   cd tools/oracle && node gen_pulse_oracle.mjs   (writes pulse_golden.json)
// SPDX-License-Identifier: AGPL-3.0-or-later

import { loadWorklets } from './worklet_harness.mjs';
import { writeJson } from './lib.mjs';

const SAMPLE_RATE = 44100;
const N = 1024;
const { render } = loadWorklets(SAMPLE_RATE);

const specs = [
  { frequency: 110, pulsewidth: 0.5 },
  { frequency: 440, pulsewidth: 0.5 },
  { frequency: 440, pulsewidth: 0.1 },
  { frequency: 440, pulsewidth: 0.9 },
  { frequency: 3000, pulsewidth: 0.3 },
  { frequency: 220, pulsewidth: 1.5 }, // clamped to 0.99
  { frequency: 220, pulsewidth: 0.5, detune: 700 },
  // a-rate width: what a modulated `pw` drives.
  { frequency: 220, pulsewidth: 'sweep' },
];
const cases = specs.map((spec) => {
  const params = { ...spec };
  if (spec.pulsewidth === 'sweep') params.pulsewidth = (i) => 0.5 + 0.4 * Math.sin((2 * Math.PI * i) / 512);
  return { ...spec, samples: render('pulse-oscillator', params, N) };
});
writeJson('./pulse_golden.json', { sampleRate: SAMPLE_RATE, length: N, cases }, 2);
console.log(`wrote pulse_golden.json (${cases.length} cases)`);
