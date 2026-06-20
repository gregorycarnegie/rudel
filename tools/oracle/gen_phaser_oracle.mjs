// gen_phaser_oracle.mjs - WebAudio phaser (swept-notch) impulse-response oracle.
//
// Renders superdough's `getPhaser` filter sample-for-sample inside an
// OfflineAudioContext (node-web-audio-api): a single `notch` BiquadFilterNode at
// `phasercenter + 282`, with its `detune` AudioParam driven by superdough's LFO
// signal. The LFO is reproduced here exactly from superdough's `getLfo` +
// `waveshapes.tri` (the default shape 0): a unipolar triangle offset by
// dcoffset -0.5 and scaled by depth = sweep*2, i.e. detune sweeps ±sweep cents.
// Driving the real BiquadFilterNode with that detune signal makes this a true
// WebAudio golden of the swept-notch rendering, which Rudel's `PostFxVoice`
// phaser (rudel-dsp/src/postfx.rs) must match sample-for-sample.
//
// Run: node gen_phaser_oracle.mjs  (writes phaser_golden.json)
// SPDX-License-Identifier: AGPL-3.0-or-later

import { OfflineAudioContext } from 'node-web-audio-api';
import { writeFileSync } from 'node:fs';

const SAMPLE_RATE = 44100;
const N = 128;

// superdough/worklets.mjs waveshapes.tri (skew 0.5 -> symmetric triangle [0,1]).
function tri(phase, skew = 0.5) {
  const x = 1 - skew;
  return phase >= skew ? 1 / x - phase / x : phase / skew;
}

// superdough getLfo output for the phaser: shape 0 (tri), dcoffset -0.5,
// depth = sweep*2, curve 1, phase = frac(rate * t) for an onset at cycle 0.
function detuneAt(n, rate, sweep) {
  const phase = (rate * (n / SAMPLE_RATE)) % 1;
  const depth = sweep * 2;
  return (tri(phase, 0.5) - 0.5) * depth; // ranges [-sweep, +sweep]
}

async function render(rate, depth, center, sweep) {
  const ctx = new OfflineAudioContext(1, N, SAMPLE_RATE);
  const buf = ctx.createBuffer(1, N, SAMPLE_RATE);
  buf.getChannelData(0)[0] = 1.0; // unit impulse
  const src = ctx.createBufferSource();
  src.buffer = buf;

  const dbuf = ctx.createBuffer(1, N, SAMPLE_RATE);
  const d = dbuf.getChannelData(0);
  for (let n = 0; n < N; n++) d[n] = detuneAt(n, rate, sweep);
  const dsrc = ctx.createBufferSource();
  dsrc.buffer = dbuf;

  const filter = ctx.createBiquadFilter();
  filter.type = 'notch';
  filter.frequency.value = center + 282;
  filter.Q.value = 2 - Math.min(Math.max(depth * 2, 0), 1.9);
  dsrc.connect(filter.detune);

  src.connect(filter);
  filter.connect(ctx.destination);
  src.start(0);
  dsrc.start(0);
  return Array.from((await ctx.startRendering()).getChannelData(0));
}

const specs = [
  { rate: 1, depth: 0.75, center: 1000, sweep: 2000 }, // defaults
  { rate: 4, depth: 0.5, center: 800, sweep: 1500 },
  { rate: 0.5, depth: 0.9, center: 1500, sweep: 2500 },
];

const cases = [];
for (const s of specs) {
  cases.push({ ...s, samples: await render(s.rate, s.depth, s.center, s.sweep) });
}

const out = { sampleRate: SAMPLE_RATE, length: N, cases };
writeFileSync(new URL('./phaser_golden.json', import.meta.url), JSON.stringify(out, null, 2));
console.log(`wrote phaser_golden.json (${cases.length} cases)`);
