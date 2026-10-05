// gen_delay_oracle.mjs - superdough's feedback delay, rendered by Web Audio.
//
// superdough's `FeedbackDelayNode` (feedbackdelay.mjs) is a `DelayNode` whose
// output feeds back into itself through a gain of `feedback`, and goes out
// through a gain of `wet`. This builds the same graph in an OfflineAudioContext
// (node-web-audio-api) and records its impulse response.
//
// Delay times are whole samples: a `DelayNode` interpolates a fractional delay
// where rudel's line reads whole samples, so a fractional time differs by
// that sub-sample interpolation and is not a golden case.
//
//   cd tools/oracle && node gen_delay_oracle.mjs   (writes delay_golden.json)
// SPDX-License-Identifier: AGPL-3.0-or-later

import { OfflineAudioContext } from 'node-web-audio-api';
import { writeJson } from './lib.mjs';

const SAMPLE_RATE = 44100;
const N = 3000;

async function impulseResponse(delaySamples, feedback) {
  const ctx = new OfflineAudioContext(1, N, SAMPLE_RATE);
  const buffer = ctx.createBuffer(1, N, SAMPLE_RATE);
  buffer.getChannelData(0)[0] = 1.0;
  const src = ctx.createBufferSource();
  src.buffer = buffer;
  // feedbackdelay.mjs, with wet = 1.
  const delay = ctx.createDelay(1);
  delay.delayTime.value = delaySamples / SAMPLE_RATE;
  const feedbackGain = ctx.createGain();
  feedbackGain.gain.value = Math.min(Math.abs(feedback), 0.995);
  delay.connect(feedbackGain);
  feedbackGain.connect(delay);
  src.connect(delay);
  delay.connect(ctx.destination);
  src.start(0);
  const rendered = await ctx.startRendering();
  return Array.from(rendered.getChannelData(0));
}

// Delays at least one 128-frame render quantum, the shortest a Web Audio
// feedback cycle allows.
const specs = [
  { delaySamples: 441, feedback: 0.5 },
  { delaySamples: 1000, feedback: 0.9 },
  { delaySamples: 128, feedback: 0.98 },
];

const cases = [];
for (const spec of specs) {
  cases.push({ ...spec, samples: await impulseResponse(spec.delaySamples, spec.feedback) });
}
writeJson('./delay_golden.json', { sampleRate: SAMPLE_RATE, length: N, cases }, 2);
console.log(`wrote delay_golden.json (${cases.length} cases)`);
