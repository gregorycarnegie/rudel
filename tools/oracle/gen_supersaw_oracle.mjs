// gen_supersaw_oracle.mjs — audio goldens for the superdough `supersaw-oscillator`
// worklet that rudel ports to SIMD in `Voice::next_supersaw`.
//
//   cd tools/oracle && node gen_supersaw_oracle.mjs
//
// The per-sample loop below is copied verbatim from
// strudel/packages/superdough/worklets.mjs (`SuperSawOscillatorProcessor.process`
// plus the `polyBlep`/`sawblep`/`getDetuner`/`applySemitoneDetuneToFrequency`
// helpers it calls), with the AudioWorklet scaffolding removed. Two deliberate
// substitutions:
//
//   * `this.phase[n] ?? Math.random()` becomes an explicit phase array, emitted
//     with each case, so the golden is reproducible. rudel seeds the same field
//     from `rand_phase()`, and the test overwrites it with these values.
//   * the output is scaled by `gainAdjustment = 1 / Math.sqrt(voices)` from
//     synth.mjs:187, because rudel folds that compensation into
//     `next_supersaw` itself rather than into a downstream gain node.
//
// crates/rudel-dsp/src/tests/supersaw.rs replays each case through rudel and
// compares. That test lives inside the crate rather than in tests/ because it
// drives the private `next_supersaw` directly, isolating the oscillator from
// the envelope and filter stages.
// SPDX-License-Identifier: AGPL-3.0-or-later

import { writeJson } from './lib.mjs';

const SAMPLE_RATE = 44100;
const INVSR = 1 / SAMPLE_RATE;
const N = 512;

// --- verbatim from worklets.mjs --------------------------------------------
const frac = (x) => x - Math.floor(x);

const getDetuner = (unison, detune) => {
  if (unison < 2) {
    return (_voiceIdx) => 0;
  }
  const scale = detune / (unison - 1);
  const center = detune * 0.5;
  return (voiceIdx) => voiceIdx * scale - center;
};

const applySemitoneDetuneToFrequency = (frequency, detune) => {
  return frequency * Math.pow(2, detune / 12);
};

function polyBlep(phase, dt) {
  dt = Math.min(dt, 1 - dt);
  const invdt = 1 / dt;
  if (phase < dt) {
    phase *= invdt;
    return 2 * phase - phase ** 2 - 1;
  } else if (phase > 1 - dt) {
    phase = (phase - 1) * invdt;
    return phase ** 2 + 2 * phase + 1;
  } else {
    return 0;
  }
}

const sawblep = (phase, dt) => {
  const v = 2 * phase - 1;
  return v - polyBlep(phase, dt);
};
// --- end verbatim ----------------------------------------------------------

// SuperSawOscillatorProcessor.process, unrolled over `n` samples. `detune` is
// the worklet's own AudioParam (driven upstream by the pitch envelope and
// vibrato, 0 here); `freqspread` is what synth.mjs passes as the per-voice
// spread in semitones.
function render(n, { frequency, voices, freqspread, panspread, detune, phases }) {
  const left = new Float64Array(n);
  const right = new Float64Array(n);
  const phase = phases.slice();
  const spread = panspread * 0.5 + 0.5;
  const gainAdjustment = 1 / Math.sqrt(voices);
  // The phase at the top of every sample, plus the state left after the last
  // one. rudel accumulates this in f32 and upstream in f64, so a free-running
  // comparison eventually lands the two on opposite sides of a cycle wrap and
  // diverges by a whole saw period. The Rust side plants each row instead, which
  // compares the oscillator maths per sample and the phase advance step by step,
  // without letting rounding accumulate across either.
  const trace = [];

  for (let i = 0; i < n; i++) {
    trace.push(phase.slice());
    let gainL = Math.sqrt(1 - spread);
    let gainR = Math.sqrt(spread);
    let freq = applySemitoneDetuneToFrequency(frequency, detune / 100);
    const detuner = getDetuner(voices, freqspread);
    for (let v = 0; v < voices; v++) {
      const freqVoice = applySemitoneDetuneToFrequency(freq, detuner(v));
      const dt = frac(freqVoice * INVSR);
      const s = sawblep(phase[v], dt);

      left[i] += s * gainL;
      right[i] += s * gainR;

      let pn = phase[v] + dt;
      if (pn >= 1.0) pn -= 1.0;
      phase[v] = pn;

      const tmp = gainL;
      gainL = gainR;
      gainR = tmp;
    }
    left[i] *= gainAdjustment;
    right[i] *= gainAdjustment;
  }
  trace.push(phase.slice());
  return { left: Array.from(left), right: Array.from(right), phase_trace: trace };
}

// Deterministic initial phases, standing in for the worklet's Math.random().
// A plain LCG so the values are reproducible and spread across [0, 1).
function phasesFor(voices, seed) {
  const out = [];
  let s = seed;
  for (let i = 0; i < voices; i++) {
    s = (s * 1103515245 + 12345) % 2147483648;
    out.push(s / 2147483648);
  }
  return out;
}

// synth.mjs forces panspread to 0 for a single voice and clamps unison to
// 1..=100; the cases below stay inside that, and cover a voice count above the
// 8-wide SIMD block so rudel's padding lanes are exercised.
const cases = [
  { name: 'default_5_voices', frequency: 440, voices: 5, freqspread: 0.18, panspread: 0.6 },
  { name: 'single_voice_no_spread', frequency: 440, voices: 1, freqspread: 0.18, panspread: 0 },
  { name: 'wide_detune_7_voices', frequency: 110, voices: 7, freqspread: 1.2, panspread: 1 },
  { name: 'nine_voices_crosses_simd_block', frequency: 880, voices: 9, freqspread: 0.5, panspread: 0.3 },
  // A high fundamental widens dt, so the polyBLEP windows at both cycle edges
  // fire on most samples rather than rarely.
  { name: 'high_freq_wide_blep_window', frequency: 8000, voices: 3, freqspread: 0.4, panspread: 0.5 },
  { name: 'two_voices_zero_spread', frequency: 220, voices: 2, freqspread: 0, panspread: 0.5 },
  // Above half the sample rate `dt` exceeds 0.5, so polyBlep's `min(dt, 1 - dt)`
  // starts picking its other arm. Not a musical pitch, but it is the only way
  // that branch is ever taken.
  { name: 'above_half_nyquist_flips_blep_window', frequency: 30000, voices: 2, freqspread: 0.3, panspread: 0.4 },
];

const out = {
  sample_rate: SAMPLE_RATE,
  samples: N,
  cases: cases.map((c, i) => {
    const phases = phasesFor(c.voices, 12345 + i * 7919);
    const { left, right, phase_trace } = render(N, { ...c, detune: 0, phases });
    return { ...c, left, right, phase_trace };
  }),
};

writeJson('supersaw_golden.json', out);
console.log(`wrote supersaw_golden.json: ${out.cases.length} cases x ${N} samples`);
