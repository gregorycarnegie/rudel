// gen_zzfx_oracle.mjs — audio golden for the ZzFX synth core.
//
//   cd tools/oracle && node gen_zzfx_oracle.mjs
//
// `buildSamples` is copied verbatim from
// strudel/packages/superdough/zzfx_fork.mjs, with `getAudioContext().sampleRate`
// replaced by a fixed SAMPLE_RATE (the only browser dependency). All cases use
// randomness 0 so the `Math.random()` term is a no-op and the output is fully
// deterministic. crates/rudel-dsp/src/tests/zzfx.rs rebuilds each with rudel's
// `build_samples` and compares sample-for-sample.

import { writeJson } from './lib.mjs';

const SAMPLE_RATE = 44100;

// --- verbatim from zzfx_fork.mjs (only the sampleRate line changed) ----------
function buildSamples(
  volume = 1,
  randomness = 0.05,
  frequency = 220,
  attack = 0,
  sustain = 0,
  release = 0.1,
  shape = 0,
  shapeCurve = 1,
  slide = 0,
  deltaSlide = 0,
  pitchJump = 0,
  pitchJumpTime = 0,
  repeatTime = 0,
  noise = 0,
  modulation = 0,
  bitCrush = 0,
  delay = 0,
  sustainVolume = 1,
  decay = 0,
  tremolo = 0,
) {
  let PI2 = Math.PI * 2,
    sampleRate = SAMPLE_RATE,
    sign = (v) => (v > 0 ? 1 : -1),
    startSlide = (slide *= (500 * PI2) / sampleRate / sampleRate),
    startFrequency = (frequency *= ((1 + randomness * 2 * Math.random() - randomness) * PI2) / sampleRate),
    b = [],
    t = 0,
    tm = 0,
    i = 0,
    j = 1,
    r = 0,
    c = 0,
    s = 0,
    f,
    length;

  attack = attack * sampleRate + 9;
  decay *= sampleRate;
  sustain *= sampleRate;
  release *= sampleRate;
  delay *= sampleRate;
  deltaSlide *= (500 * PI2) / sampleRate ** 3;
  modulation *= PI2 / sampleRate;
  pitchJump *= PI2 / sampleRate;
  pitchJumpTime *= sampleRate;
  repeatTime = (repeatTime * sampleRate) | 0;

  for (length = (attack + decay + sustain + release + delay) | 0; i < length; b[i++] = s) {
    if (!(++c % ((bitCrush * 100) | 0))) {
      s = shape
        ? shape > 1
          ? shape > 2
            ? shape > 3
              ? Math.sin((t % PI2) ** 3)
              : Math.max(Math.min(Math.tan(t), 1), -1)
            : 1 - (((((2 * t) / PI2) % 2) + 2) % 2)
          : 1 - 4 * Math.abs(Math.round(t / PI2) - t / PI2)
        : Math.sin(t);

      s =
        (repeatTime ? 1 - tremolo + tremolo * Math.sin((PI2 * i) / repeatTime) : 1) *
        sign(s) *
        Math.abs(s) ** shapeCurve *
        volume *
        1 *
        (i < attack
          ? i / attack
          : i < attack + decay
            ? 1 - ((i - attack) / decay) * (1 - sustainVolume)
            : i < attack + decay + sustain
              ? sustainVolume
              : i < length - delay
                ? ((length - i - delay) / release) * sustainVolume
                : 0);

      s = delay
        ? s / 2 +
          (delay > i ? 0 : ((i < length - delay ? 1 : (length - i) / delay) * b[(i - delay) | 0]) / 2)
        : s;
    }

    f = (frequency += slide += deltaSlide) * Math.cos(modulation * tm++);
    t += f - f * noise * (1 - (((Math.sin(i) + 1) * 1e9) % 2));

    if (j && ++j > pitchJumpTime) {
      frequency += pitchJump;
      startFrequency += pitchJump;
      j = 0;
    }

    if (repeatTime && !(++r % repeatTime)) {
      frequency = startFrequency;
      slide = startSlide;
      j ||= 1;
    }
  }
  return b;
}

// label -> the 20 buildSamples params. randomness (index 1) is 0 everywhere.
// Short envelopes keep the buffers small. Param order:
// volume, randomness, frequency, attack, sustain, release, shape, shapeCurve,
// slide, deltaSlide, pitchJump, pitchJumpTime, repeatTime, noise, modulation,
// bitCrush, delay, sustainVolume, decay, tremolo
const CASES = {
  sine: [0.25, 0, 440, 0.001, 0.003, 0.003, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  triangle: [0.25, 0, 330, 0.001, 0.003, 0.003, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  saw: [0.25, 0, 220, 0.001, 0.003, 0.003, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  tan: [0.25, 0, 110, 0.001, 0.003, 0.003, 3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  noise: [0.25, 0, 440, 0.001, 0.003, 0.003, 4, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  square: [0.25, 0, 220, 0.001, 0.003, 0.003, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  slide: [0.25, 0, 220, 0.001, 0.004, 0.003, 0, 1, 0.5, 0.2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  modulation: [0.25, 0, 330, 0.001, 0.004, 0.003, 0, 1, 0, 0, 0, 0, 0, 0, 20, 0, 0, 1, 0, 0],
  bitcrush: [0.25, 0, 440, 0.001, 0.004, 0.003, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0.5, 0, 1, 0, 0],
  delay: [0.25, 0, 330, 0.001, 0.004, 0.003, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0.002, 1, 0, 0],
  pitchjump: [0.25, 0, 220, 0.001, 0.004, 0.003, 0, 1, 0, 0, 300, 0.002, 0, 0, 0, 0, 0, 1, 0, 0],
  noisefm: [0.25, 0, 220, 0.001, 0.004, 0.003, 0, 1, 0, 0, 0, 0, 0, 0.3, 0, 0, 0, 1, 0, 0],
  tremolo_repeat: [0.25, 0, 330, 0.001, 0.006, 0.003, 0, 1, 0, 0, 0, 0, 0.002, 0, 0, 0, 0, 1, 0, 0.5],
  decay: [0.25, 0, 330, 0.001, 0.002, 0.003, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0.4, 0.003, 0],
  // The cases above pair every parameter with its neighbours, which leaves the
  // arithmetic joining them unchecked: 20 of build_samples' mutants survived
  // them. These vary each one on its own, and at values either side of the
  // defaults rather than only at them.
  shapecurve_2: [0.25, 0, 330, 0.001, 0.004, 0.003, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  shapecurve_half: [0.25, 0, 330, 0.001, 0.004, 0.003, 0, 0.5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  volume_loud: [0.9, 0, 330, 0.001, 0.004, 0.003, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  slide_down: [0.25, 0, 440, 0.001, 0.004, 0.003, 0, 1, -0.5, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  deltaslide_only: [0.25, 0, 220, 0.001, 0.004, 0.003, 0, 1, 0, 0.4, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  tremolo_only: [0.25, 0, 330, 0.001, 0.006, 0.003, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0.8],
  repeat_only: [0.25, 0, 330, 0.001, 0.006, 0.003, 0, 1, 0, 0, 0, 0, 0.0015, 0, 0, 0, 0, 1, 0, 0],
  sustainvol_only: [0.25, 0, 330, 0.001, 0.004, 0.003, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0.3, 0, 0],
  pitchjump_down: [0.25, 0, 440, 0.001, 0.004, 0.003, 0, 1, 0, 0, -200, 0.002, 0, 0, 0, 0, 0, 1, 0, 0],
  bitcrush_hard: [0.25, 0, 440, 0.001, 0.004, 0.003, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0.9, 0, 1, 0, 0],
  noise_heavy: [0.25, 0, 220, 0.001, 0.004, 0.003, 0, 1, 0, 0, 0, 0, 0, 0.9, 0, 0, 0, 1, 0, 0],
  zero_attack: [0.25, 0, 330, 0, 0.004, 0.003, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  zero_release: [0.25, 0, 330, 0.001, 0.004, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
  // Every envelope boundary in the cases above lands between two samples,
  // because `attack*44100 + 9` and friends are not whole numbers — so the four
  // `i < …` comparisons that pick the segment are never evaluated *at* their
  // boundary, and `<` and `<=` agree. These put each boundary on an exact
  // sample index (times as k/44100, attack 0 so `attack` is exactly 9) and
  // give each segment a different level, so crossing one is visible.
  int_segments: [0.25, 0, 330, 0, 200 / 44100, 300 / 44100, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0.3, 100 / 44100, 0],
  int_delay: [0.25, 0, 330, 0, 200 / 44100, 300 / 44100, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 50 / 44100, 0.3, 100 / 44100, 0],
  int_repeat: [0.25, 0, 330, 0, 400 / 44100, 300 / 44100, 0, 1, 0, 0, 0, 0, 150 / 44100, 0, 0, 0, 0, 0.3, 100 / 44100, 0.5],
  int_pitchjump: [0.25, 0, 330, 0, 400 / 44100, 300 / 44100, 0, 1, 0, 0, 300, 100 / 44100, 0, 0, 0, 0, 0, 0.3, 100 / 44100, 0],
  int_bitcrush: [0.25, 0, 330, 0, 400 / 44100, 300 / 44100, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0.5, 0, 0.3, 100 / 44100, 0],
  int_everything: [
    0.4, 0, 330, 0, 400 / 44100, 300 / 44100, 2, 1.5, 0.3, 0.1, 120, 100 / 44100, 150 / 44100, 0, 12,
    0.5, 50 / 44100, 0.6, 100 / 44100, 0.4,
  ],
  everything: [
    0.4, 0, 330, 0.002, 0.008, 0.004, 2, 1.5, 0.3, 0.1, 120, 0.003, 0.003, 0.2, 12, 0.3, 0.001,
    0.6, 0.002, 0.4,
  ],
};

const out = {};
for (const [label, params] of Object.entries(CASES)) {
  out[label] = { params, samples: buildSamples(...params) };
}
writeJson('./zzfx_golden.json', out);
console.error(`wrote zzfx_golden.json (${Object.keys(out).length} cases)`);
