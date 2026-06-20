// gen_adsr_oracle.mjs — audio golden for the linear ADSR gain envelope.
//
//   cd tools/oracle && node gen_adsr_oracle.mjs
//
// `getParamADSR`, `getSlope`, and `nanFallback` are copied verbatim from
// strudel/packages/superdough/{helpers,util}.mjs (helpers.mjs can't be imported
// directly because it pulls in the browser AudioContext). superdough drives the
// amplitude envelope with `getParamADSR(node.gain, a, d, s, r, 0, 1, t, holdEnd,
// 'linear')` (superdough/synth.mjs), scheduling Web Audio param automation
// events. We capture those events with a mock param, then sample the resulting
// automation curve using Web Audio's documented linear-ramp/setValue semantics.
// crates/rudel-dsp/tests/adsr_golden.rs samples rudel's `adsr_value` at the same
// times and compares the two curves.
// SPDX-License-Identifier: AGPL-3.0-or-later

import { writeFileSync } from 'node:fs';

const SAMPLE_RATE = 44100;

// --- verbatim from superdough/util.mjs --------------------------------------
function nanFallback(value, fallback = 0, _silent) {
  if (isNaN(Number(value))) {
    return fallback;
  }
  return value;
}

// --- verbatim from superdough/helpers.mjs -----------------------------------
const getSlope = (y1, y2, x1, x2) => {
  const denom = x2 - x1;
  if (denom === 0) {
    return 0;
  }
  return (y2 - y1) / (x2 - x1);
};

const getParamADSR = (
  param,
  attack,
  decay,
  sustain,
  release,
  // min = value at start of attack, max = value at end of attack; it is possible that max < min
  min,
  max,
  begin,
  end,
  //exponential works better for frequency modulations (such as filter cutoff) due to human ear perception
  curve = 'exponential',
) => {
  attack = nanFallback(attack);
  decay = nanFallback(decay);
  sustain = nanFallback(sustain);
  release = nanFallback(release);
  const ramp = curve === 'exponential' ? 'exponentialRampToValueAtTime' : 'linearRampToValueAtTime';
  if (curve === 'exponential') {
    min = min === 0 ? 0.001 : min;
    max = max === 0 ? 0.001 : max;
  }
  const range = max - min;
  const sustainVal = min + sustain * range;
  const duration = end - begin;

  const envValAtTime = (time) => {
    let val;
    if (attack > time) {
      val = time * getSlope(min, max, 0, attack) + min;
    } else {
      val = (time - attack) * getSlope(max, sustainVal, 0, decay) + max;
    }
    if (curve === 'exponential') {
      val = val || 0.001;
    }
    return val;
  };

  param.setValueAtTime(min, begin);
  if (attack > duration) {
    //attack
    param[ramp](envValAtTime(duration), end);
  } else if (attack + decay > duration) {
    //attack
    param[ramp](envValAtTime(attack), begin + attack);
    //decay
    param[ramp](envValAtTime(duration), end);
  } else {
    //attack
    param[ramp](envValAtTime(attack), begin + attack);
    //decay
    param[ramp](envValAtTime(attack + decay), begin + attack + decay);
    //sustain
    param.setValueAtTime(sustainVal, end);
  }
  //release
  param[ramp](min, end + release);
};

// --- mock Web Audio param + automation-curve sampler ------------------------
// Records scheduled events, then reconstructs the value at an arbitrary time
// using Web Audio's documented behaviour: setValueAtTime steps/holds, and
// linearRampToValueAtTime interpolates linearly from the previous event.
class MockParam {
  constructor() {
    this.events = [];
  }
  setValueAtTime(value, time) {
    this.events.push({ type: 'set', value, time });
  }
  linearRampToValueAtTime(value, time) {
    this.events.push({ type: 'lin', value, time });
  }
  exponentialRampToValueAtTime(value, time) {
    this.events.push({ type: 'exp', value, time });
  }
  valueAt(t) {
    const ev = this.events;
    if (t <= ev[0].time) {
      return ev[0].value;
    }
    let prev = ev[0];
    for (let i = 1; i < ev.length; i++) {
      const e = ev[i];
      if (t >= e.time) {
        prev = e;
        continue;
      }
      // t is between prev.time and e.time
      if (e.type === 'lin') {
        const frac = (t - prev.time) / (e.time - prev.time);
        return prev.value + (e.value - prev.value) * frac;
      }
      if (e.type === 'exp') {
        const frac = (t - prev.time) / (e.time - prev.time);
        return prev.value * Math.pow(e.value / prev.value, frac);
      }
      // 'set' holds the previous value until the step
      return prev.value;
    }
    return prev.value;
  }
}

// attack, decay, sustain, release, duration (holdEnd - begin, seconds)
const cases = [
  { name: 'common', a: 0.01, d: 0.1, s: 0.6, r: 0.2, dur: 1.0 },
  { name: 'synth_defaults', a: 0.001, d: 0.05, s: 0.6, r: 0.01, dur: 0.5 },
  { name: 'attack_longer_than_duration', a: 0.5, d: 0.1, s: 0.6, r: 0.2, dur: 0.2 },
  { name: 'attack_plus_decay_exceeds_duration', a: 0.1, d: 0.5, s: 0.6, r: 0.2, dur: 0.3 },
  { name: 'zero_attack', a: 0.0, d: 0.1, s: 0.5, r: 0.1, dur: 0.4 },
  { name: 'zero_sustain', a: 0.01, d: 0.1, s: 0.0, r: 0.1, dur: 0.5 },
  { name: 'full_sustain_no_decay_drop', a: 0.05, d: 0.1, s: 1.0, r: 0.1, dur: 0.6 },
  { name: 'tiny_release', a: 0.01, d: 0.05, s: 0.6, r: 0.001, dur: 0.3 },
  { name: 'attack_equals_duration', a: 0.2, d: 0.1, s: 0.5, r: 0.1, dur: 0.2 },
];

const out = cases.map(({ name, a, d, s, r, dur }) => {
  const param = new MockParam();
  // superdough's gain envelope: min=0, max=1, begin=0, end=duration, linear.
  getParamADSR(param, a, d, s, r, 0, 1, 0, dur, 'linear');

  // sample the whole envelope: attack/decay/sustain region + release tail.
  const total = dur + r + 0.02;
  const n = Math.ceil(total * SAMPLE_RATE);
  const samples = new Array(n);
  for (let i = 0; i < n; i++) {
    samples[i] = param.valueAt(i / SAMPLE_RATE);
  }
  return { name, attack: a, decay: d, sustain: s, release: r, duration: dur, samples };
});

writeFileSync(
  new URL('./adsr_golden.json', import.meta.url),
  JSON.stringify({ sampleRate: SAMPLE_RATE, cases: out }),
);
console.log(`wrote adsr_golden.json: ${out.length} cases`);
