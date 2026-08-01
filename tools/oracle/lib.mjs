import { writeFileSync } from 'node:fs';

export function writeJson(filename, data, space) {
  writeFileSync(new URL(filename, import.meta.url), JSON.stringify(data, null, space));
}

export function fracStr(f) {
  return `${f.s < 0 ? '-' : ''}${f.n}/${f.d}`;
}

export function normValue(v) {
  if (v === null || v === undefined) return null;
  if (Array.isArray(v)) return v.map(normValue);
  if (typeof v === 'object') {
    const o = {};
    for (const k of Object.keys(v).sort()) o[k] = normValue(v[k]);
    return o;
  }
  return v;
}

// ============================================================================
// superdough's parameter-automation envelope, shared by the oracles that need
// a value moving over time: the gain ADSR (gen_adsr_oracle) and the pitch
// envelope driving a voice's detune (gen_supersaw_oracle).
// ============================================================================

// --- verbatim from superdough/util.mjs --------------------------------------
export function nanFallback(value, fallback = 0, _silent) {
  if (isNaN(Number(value))) {
    return fallback;
  }
  return value;
}

// --- verbatim from superdough/helpers.mjs -----------------------------------
export const getSlope = (y1, y2, x1, x2) => {
  const denom = x2 - x1;
  if (denom === 0) {
    return 0;
  }
  return (y2 - y1) / (x2 - x1);
};

export const getParamADSR = (
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
export class MockParam {
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
