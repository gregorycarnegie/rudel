// gen_chrome_oscillator_oracle.mjs - Chrome's OscillatorNode, sample for sample.
//
// superdough plays `sine`/`square`/`sawtooth`/`triangle` through Web Audio's
// OscillatorNode, which the spec defines as a band-limited PeriodicWave of the
// waveform's Fourier series, normalized to peak 1. Chrome builds that as 4096-
// point tables, three per octave, and blends two of them by pitch
// (periodic_wave_handler.cc, oscillator_handler.cc). node-web-audio-api uses
// polyBLEP instead, so this oracle renders in real Chrome: an
// OfflineAudioContext in the installed browser, driven by playwright-core.
//
//   cd tools/oracle && npm install && node gen_chrome_oscillator_oracle.mjs
// writes chrome_oscillator_golden.json; crates/rudel-dsp/src/tests/oscillator.rs
// replays it.
// SPDX-License-Identifier: AGPL-3.0-or-later

import { chromium } from 'playwright-core';
import { writeJson } from './lib.mjs';

const SAMPLE_RATE = 44100;
const N = 2048;

// `sweep` ramps the frequency linearly from `frequency` to `to` across the
// render, crossing range tables per sample (a-rate).
const specs = [];
for (const type of ['sawtooth', 'square', 'triangle']) {
  for (const frequency of [55, 440, 1760, 5000, 11000]) specs.push({ type, frequency });
  specs.push({ type, frequency: 100, to: 9000 });
  // LFO rates: Chrome's 3- and 5-point Lagrange interpolation.
  specs.push({ type, frequency: 2.5 });
  specs.push({ type, frequency: 1 });
}

const browser = await chromium.launch({ channel: 'chrome', headless: true });
const page = await browser.newPage();
const cases = await page.evaluate(
  async ({ specs, SAMPLE_RATE, N }) => {
    const out = [];
    for (const spec of specs) {
      const ctx = new OfflineAudioContext(1, N, SAMPLE_RATE);
      const osc = new OscillatorNode(ctx, { type: spec.type, frequency: spec.frequency });
      if (spec.to !== undefined) {
        osc.frequency.setValueAtTime(spec.frequency, 0);
        osc.frequency.linearRampToValueAtTime(spec.to, N / SAMPLE_RATE);
      }
      osc.connect(ctx.destination);
      osc.start(0);
      const rendered = await ctx.startRendering();
      out.push({ ...spec, samples: Array.from(rendered.getChannelData(0)) });
    }
    return out;
  },
  { specs, SAMPLE_RATE, N },
);
const version = browser.version();
await browser.close();

writeJson('./chrome_oscillator_golden.json', { browser: `chrome ${version}`, sampleRate: SAMPLE_RATE, length: N, cases });
console.log(`wrote chrome_oscillator_golden.json (${cases.length} cases, chrome ${version})`);
