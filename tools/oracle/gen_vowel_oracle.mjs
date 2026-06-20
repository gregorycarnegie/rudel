// gen_vowel_oracle.mjs - WebAudio vowel-formant filter impulse-response oracle.
//
// Renders superdough's VowelNode graph (vowel.mjs) sample-for-sample inside an
// OfflineAudioContext (node-web-audio-api): input -> 5 parallel bandpass
// BiquadFilterNodes (per-formant freq/Q) -> per-formant gain -> summed into a
// makeup gain of 8. This is exactly the structure Rudel's `Formant`
// (rudel-dsp/src/postfx.rs) ports, so the impulse response matches.
//
// The formant table is the webdirt/superdough `vowelFormant` table, inlined
// here for the five vowels Rudel implements (a/e/i/o/u) so the generator stays
// dependency-free like gen_zzfx_oracle.mjs.
//
// Run: node gen_vowel_oracle.mjs  (writes vowel_golden.json)
// SPDX-License-Identifier: AGPL-3.0-or-later

import { OfflineAudioContext } from 'node-web-audio-api';
import { writeFileSync } from 'node:fs';

const SAMPLE_RATE = 44100;
const N = 64;

// from strudel/packages/superdough/vowel.mjs (a/e/i/o/u)
const vowelFormant = {
  a: { freqs: [660, 1120, 2750, 3000, 3350], gains: [1, 0.5012, 0.0708, 0.0631, 0.0126], qs: [80, 90, 120, 130, 140] },
  e: { freqs: [440, 1800, 2700, 3000, 3300], gains: [1, 0.1995, 0.1259, 0.1, 0.1], qs: [70, 80, 100, 120, 120] },
  i: { freqs: [270, 1850, 2900, 3350, 3590], gains: [1, 0.0631, 0.0631, 0.0158, 0.0158], qs: [40, 90, 100, 120, 120] },
  o: { freqs: [430, 820, 2700, 3000, 3300], gains: [1, 0.3162, 0.0501, 0.0794, 0.01995], qs: [40, 80, 100, 120, 120] },
  u: { freqs: [370, 630, 2750, 3000, 3400], gains: [1, 0.1, 0.0708, 0.0316, 0.01995], qs: [40, 60, 100, 120, 120] },
};

async function impulseResponse(letter) {
  const { freqs, gains, qs } = vowelFormant[letter];
  const ctx = new OfflineAudioContext(1, N, SAMPLE_RATE);
  const buffer = ctx.createBuffer(1, N, SAMPLE_RATE);
  buffer.getChannelData(0)[0] = 1.0; // unit impulse
  const src = ctx.createBufferSource();
  src.buffer = buffer;
  // VowelNode: the node itself is a GainNode (gain 1 = identity input).
  const input = ctx.createGain();
  const makeup = ctx.createGain();
  makeup.gain.value = 8;
  src.connect(input);
  for (let i = 0; i < 5; i++) {
    const filter = ctx.createBiquadFilter();
    filter.type = 'bandpass';
    filter.Q.value = qs[i];
    filter.frequency.value = freqs[i];
    const gain = ctx.createGain();
    gain.gain.value = gains[i];
    input.connect(filter);
    filter.connect(gain);
    gain.connect(makeup);
  }
  makeup.connect(ctx.destination);
  src.start(0);
  const rendered = await ctx.startRendering();
  return Array.from(rendered.getChannelData(0));
}

const cases = [];
for (const letter of Object.keys(vowelFormant)) {
  cases.push({ vowel: letter, samples: await impulseResponse(letter) });
}

const out = { sampleRate: SAMPLE_RATE, length: N, cases };
writeFileSync(new URL('./vowel_golden.json', import.meta.url), JSON.stringify(out, null, 2));
console.log(`wrote vowel_golden.json (${cases.length} cases)`);
