// gen_vocoder_oracle.mjs — audio goldens for superdough's `phase-vocoder-processor`,
// the worklet behind `stretch`, which crates/rudel-dsp/src/vocoder.rs ports by hand.
//
//   cd tools/oracle && node gen_vocoder_oracle.mjs
//
// Nothing here is transcribed. The `PhaseVocoderProcessor` class is *sliced out
// of worklets.mjs as text* and evaluated against the vendored `OLAProcessor`
// and `fft.js`, with only the AudioWorklet globals stubbed. That matters more
// than usual for this one: the upstream phaze project this is derived from has
// a different `applyHannWindow` constant and a different `shiftPeaks`, so an
// oracle written from the original repo "disagrees" with a port that correctly
// follows strudel's fork — and would send you off fixing working code. Slicing
// the vendored file cannot drift that way.
//
// The processor is driven exactly as the browser would: 128-frame blocks in,
// 128-frame blocks out, stereo, with the OLA buffering left to run itself.
//
// crates/rudel-dsp/tests/vocoder_golden.rs replays each case through rudel's
// `PhaseVocoder` and compares.
// SPDX-License-Identifier: AGPL-3.0-or-later

import { readFileSync } from 'node:fs';
import { writeJson } from './lib.mjs';

const SAMPLE_RATE = 44100;
const HOP = 128;
// Enough blocks to fill the 2048-sample analysis frame several times over, so
// the comparison covers steady-state overlap-add and not just the priming.
const BLOCKS = 48;

// --- AudioWorklet scaffolding ----------------------------------------------
// `ola-processor.js` is written against the worklet global scope; provide the
// globals it touches before importing it.
globalThis.sampleRate = SAMPLE_RATE;
globalThis.currentTime = 0;
globalThis.AudioWorkletProcessor = class {
  constructor() {
    this.port = { onmessage: null, postMessage() {} };
  }
};

// --- lift the real class out of worklets.mjs --------------------------------
const worklets = readFileSync(new URL('../../strudel/packages/superdough/worklets.mjs', import.meta.url), 'utf8');

function slice(from, to, what) {
  const start = worklets.indexOf(from);
  const end = worklets.indexOf(to, start);
  if (start < 0 || end < 0) {
    throw new Error(`could not find ${what} in worklets.mjs — has it been restructured?`);
  }
  return worklets.slice(start, end);
}

// The integer helpers the vocoder uses (`fround`, `fceil`) and the phase
// vocoder section itself, both verbatim.
const helpers = slice('// Fast integer ops for non-negative values', 'const fast_tanh', 'the integer helpers');
const vocoder = slice(
  '// Phase Vocoder sourced from',
  "registerProcessor('phase-vocoder-processor'",
  'the phase vocoder',
);

const source = `
import OLAProcessor from '${new URL('../../strudel/packages/superdough/ola-processor.js', import.meta.url).href}';
import FFT from '${new URL('../../strudel/packages/superdough/fft.js', import.meta.url).href}';
const PI = Math.PI;
const TWO_PI = 2 * PI;
${helpers}
${vocoder}
export default PhaseVocoderProcessor;
`;

const PhaseVocoderProcessor = (await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`))
  .default;

// --- driving it -------------------------------------------------------------

// A deterministic broadband stereo signal, matching the other worklet goldens:
// two sines plus a seeded LCG noise floor, with the right channel given its own
// tones so a swapped-channel port would show up.
function testSignal(n) {
  const left = new Float32Array(n);
  const right = new Float32Array(n);
  let seed = 12345;
  for (let i = 0; i < n; i++) {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    const noise = (seed / 2147483648) * 2 - 1;
    const t = i / SAMPLE_RATE;
    left[i] = 0.5 * Math.sin(TWO_PI * 220 * t) + 0.3 * Math.sin(TWO_PI * 3300 * t) + 0.2 * noise;
    right[i] = 0.4 * Math.sin(TWO_PI * 330 * t) + 0.25 * Math.sin(TWO_PI * 1700 * t) - 0.2 * noise;
  }
  return [left, right];
}

const TWO_PI = 2 * Math.PI;

function run(stretch, left, right, zeroAboveNyquist = false) {
  const proc = new PhaseVocoderProcessor({
    numberOfInputs: 1,
    numberOfOutputs: 1,
    processorOptions: {},
  });
  if (zeroAboveNyquist) {
    // `fft.js`'s `realTransform` fills only bins 0..N/2; above that it leaves
    // butterfly scratch from its own recursion, and `createComplexArray` is
    // reused frame to frame so nothing ever clears it. `shiftPeaks` then reads
    // those bins whenever a region of influence runs off the top of the
    // spectrum — which happens only when the pitch factor is below 1, i.e. for
    // a negative `stretch`.
    //
    // Those values are an artefact of fft.js's internal layout, not a design
    // decision, and no independent FFT can reproduce them. Zeroing them gives
    // the reference upstream would produce without the defect, which is what
    // rudel targets. `upstreamRaw` below keeps the undoctored output so the size
    // and the location of the difference stay on the record.
    const real = proc.fft.realTransform.bind(proc.fft);
    proc.fft.realTransform = (out, data) => {
      real(out, data);
      for (let k = (proc.fftSize / 2 + 1) * 2; k < out.length; k++) out[k] = 0;
    };
  }
  const params = { pitchFactor: [stretch] };
  const outL = [];
  const outR = [];
  for (let b = 0; b * HOP < left.length; b++) {
    const inL = left.slice(b * HOP, (b + 1) * HOP);
    const inR = right.slice(b * HOP, (b + 1) * HOP);
    const oL = new Float32Array(HOP);
    const oR = new Float32Array(HOP);
    proc.process([[inL, inR]], [[oL, oR]], params);
    outL.push(...oL);
    outR.push(...oR);
  }
  return { left: outL, right: outR };
}

const n = BLOCKS * HOP;
const [left, right] = testSignal(n);

// `stretch` values spanning the branch in `processOLA`: negative (scaled by
// 0.25 before the +1), zero (unity), and positive both below and above the
// point where shifted peaks start falling off the end of the spectrum.
const STRETCHES = [-0.5, 0, 0.5, 1, 2];

// The reference: upstream's own code, with the read of uninitialised FFT
// scratch above Nyquist neutralised. rudel matches this sample for sample.
const cases = STRETCHES.map((stretch) => ({
  stretch,
  ...run(stretch, left, right, true),
}));

// Upstream exactly as it runs in the browser. Identical to `cases` for every
// pitch factor at or above 1 — the scratch is simply never reached there — and
// different below it. The Rust side asserts both halves of that, so the claim
// "the only divergence is the scratch read" is checked rather than asserted.
const upstreamRaw = STRETCHES.map((stretch) => ({
  stretch,
  ...run(stretch, left, right, false),
}));

writeJson('vocoder_golden.json', {
  sampleRate: SAMPLE_RATE,
  hopSize: HOP,
  blocks: BLOCKS,
  input: { left: Array.from(left), right: Array.from(right) },
  cases,
  upstreamRaw,
});
