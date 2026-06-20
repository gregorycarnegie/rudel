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

import { writeFileSync } from 'node:fs';

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
};

const out = {};
for (const [label, params] of Object.entries(CASES)) {
  out[label] = { params, samples: buildSamples(...params) };
}
writeFileSync(new URL('./zzfx_golden.json', import.meta.url), JSON.stringify(out));
console.error(`wrote zzfx_golden.json (${Object.keys(out).length} cases)`);
