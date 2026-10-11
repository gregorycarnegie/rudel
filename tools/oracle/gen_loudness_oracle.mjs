// gen_loudness_oracle.mjs — golden for hydra's `a`: Meyda's `loudness`.
//
//   cd tools/oracle && npm install && node gen_loudness_oracle.mjs
//
// Upstream hydra-synth reads `loudness.specific` from a Meyda analyser with its
// default 512-sample buffer and Hann window. This runs the real Meyda over a
// few signals at two sample rates; crates/rudel-app/src/app/hydra_audio.rs
// feeds the same samples to its port and compares the 24 Bark bands.

import Meyda from 'meyda';
import { writeJson } from './lib.mjs';

const N = 512;

// mulberry32: noise that is the same on every run.
function noise(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296 * 2 - 1;
  };
}

const signals = (sr) => {
  const tone = (freq, amp) => Array.from({ length: N }, (_, i) => amp * Math.sin(2 * Math.PI * freq * i / sr));
  const rand = noise(7);
  return {
    tone_200: tone(200, 1),
    tone_1000: tone(1000, 0.5),
    tone_4000: tone(4000, 0.25),
    tone_12000: tone(12000, 0.8),
    noise: Array.from({ length: N }, () => 0.3 * rand()),
    // DC and a ramp: energy in the lowest band, and a slope across the window.
    ramp: Array.from({ length: N }, (_, i) => 0.2 + i / N),
  };
};

const cases = {};
for (const sr of [44100, 48000]) {
  Meyda.bufferSize = N;
  Meyda.sampleRate = sr;
  // `extract` caches the Bark scale by length alone, so a new rate needs it cleared.
  Meyda.barkScale = undefined;
  for (const [name, samples] of Object.entries(signals(sr))) {
    const input = Float32Array.from(samples);
    const { specific } = Meyda.extract('loudness', input);
    cases[`${name}_${sr}`] = { sampleRate: sr, input: Array.from(input), specific: Array.from(specific) };
  }
}

writeJson('loudness_golden.json', { bufferSize: N, cases });
console.log(`wrote ${Object.keys(cases).length} cases`);
