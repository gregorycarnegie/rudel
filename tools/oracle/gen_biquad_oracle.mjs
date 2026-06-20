// gen_biquad_oracle.mjs - WebAudio BiquadFilterNode impulse-response oracle.
//
// This is the first oracle that renders a *real Web Audio graph* (not pure JS
// math) sample-for-sample: it drives a unit impulse through a BiquadFilterNode
// inside an OfflineAudioContext (via node-web-audio-api, a faithful native
// implementation of the Web Audio API) and dumps the impulse response.
//
// We only golden the `bandpass` and `notch` types, whose Q is linear in both
// the WebAudio spec and the RBJ Audio EQ Cookbook, so they match Rudel's
// `Biquad` (rudel-dsp/src/filter.rs) exactly. `lowpass`/`highpass` are skipped
// here because WebAudio interprets their Q in dB, a different convention from
// Rudel's linear-Q filter controls.
//
// Run: node gen_biquad_oracle.mjs  (writes biquad_golden.json)
// SPDX-License-Identifier: AGPL-3.0-or-later

import { OfflineAudioContext } from 'node-web-audio-api';
import { writeFileSync } from 'node:fs';

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
];

const cases = [];
for (const spec of specs) {
  cases.push({ ...spec, samples: await impulseResponse(spec.type, spec.frequency, spec.q) });
}

const out = { sampleRate: SAMPLE_RATE, length: N, cases };
writeFileSync(new URL('./biquad_golden.json', import.meta.url), JSON.stringify(out, null, 2));
console.log(`wrote biquad_golden.json (${cases.length} cases)`);
