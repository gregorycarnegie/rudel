// gen_biquad_oracle.mjs - WebAudio BiquadFilterNode impulse-response oracle.
//
// This is the first oracle that renders a *real Web Audio graph* (not pure JS
// math) sample-for-sample: it drives a unit impulse through a BiquadFilterNode
// inside an OfflineAudioContext (via node-web-audio-api, a faithful native
// implementation of the Web Audio API) and dumps the impulse response.
//
// `bandpass` and `notch` take a linear Q, as the RBJ Audio EQ Cookbook does;
// `lowpass`/`highpass` read theirs in dB (alpha = sin(w0) / (2 * 10^(Q/20))),
// which is what superdough's `lpq`/`hpq` hand them, so the Rust side runs
// those through the voice filter that converts a pattern's resonance.
//
// Run: node gen_biquad_oracle.mjs  (writes biquad_golden.json)
// SPDX-License-Identifier: AGPL-3.0-or-later

import { OfflineAudioContext } from 'node-web-audio-api';
import { writeJson } from './lib.mjs';

const SAMPLE_RATE = 44100;
const N = 64; // impulse-response length to compare

async function impulseResponse(type, frequency, q) {
  const ctx = new OfflineAudioContext(1, N, SAMPLE_RATE);
  const buffer = ctx.createBuffer(1, N, SAMPLE_RATE);
  buffer.getChannelData(0)[0] = 1.0; // unit impulse at sample 0
  const src = ctx.createBufferSource();
  src.buffer = buffer;
  const filter = ctx.createBiquadFilter();
  filter.type = type;
  filter.frequency.value = frequency;
  filter.Q.value = q;
  src.connect(filter).connect(ctx.destination);
  src.start(0);
  const rendered = await ctx.startRendering();
  return Array.from(rendered.getChannelData(0));
}

const specs = [
  { type: 'bandpass', frequency: 1000, q: 1 },
  { type: 'bandpass', frequency: 440, q: 5 },
  { type: 'bandpass', frequency: 5000, q: 0.5 },
  { type: 'notch', frequency: 1000, q: 1 },
  { type: 'notch', frequency: 2500, q: 3 },
  { type: 'lowpass', frequency: 1000, q: 1 },
  { type: 'lowpass', frequency: 300, q: 12 },
  { type: 'lowpass', frequency: 4000, q: 0 },
  { type: 'highpass', frequency: 1000, q: 1 },
  { type: 'highpass', frequency: 200, q: 6 },
];

const cases = [];
for (const spec of specs) {
  cases.push({ ...spec, samples: await impulseResponse(spec.type, spec.frequency, spec.q) });
}

const out = { sampleRate: SAMPLE_RATE, length: N, cases };
writeJson('./biquad_golden.json', out, 2);
console.log(`wrote biquad_golden.json (${cases.length} cases)`);
