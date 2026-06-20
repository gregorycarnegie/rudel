#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for Adsr {
    // Strudel synth defaults: getADSRValues(..., [0.001, 0.05, 0.6, 0.01])
    fn default() -> Self {
        Adsr {
            attack: 0.001,
            decay: 0.05,
            sustain: 0.6,
            release: 0.01,
        }
    }
}

/// Normalized (min=0, max=1) ADSR envelope value at relative time `t`, with the
/// note held until `hold_end` (the note duration). Ports superdough's
/// `getParamADSR(param, a, d, s, r, 0, 1, begin, holdEnd, 'linear')` schedule
/// (helpers.mjs): the attack and decay are cut short at `hold_end` when they
/// would overrun it, and the release always ramps from whatever value the
/// envelope had reached at `hold_end`. The common case (attack + decay ≤
/// duration) is the familiar rise / decay-to-sustain / hold / release.
pub fn adsr_value(adsr: &Adsr, t: f32, hold_end: f32) -> f32 {
    let Adsr {
        attack,
        decay,
        sustain,
        release,
    } = *adsr;
    let duration = hold_end;

    // Every schedule opens with `setValueAtTime(min, begin)`, so the value at the
    // note start is always `min` (0) — even for a zero-length attack, whose ramp
    // to `max` only takes effect just after `begin`.
    if t <= 0.0 {
        return 0.0;
    }

    // superdough's `envValAtTime` for the normalized min=0/max=1 gain envelope:
    // a linear rise of slope 1/attack, then a linear decay toward `sustain`.
    let env_at = |time: f32| -> f32 {
        if attack > time {
            time / attack.max(1e-9)
        } else {
            (time - attack) * (sustain - 1.0) / decay.max(1e-9) + 1.0
        }
    };

    if attack > duration {
        // Attack overruns the note: ramp 0 → env_at(duration) over the hold,
        // then release from there. (0 → env_at(duration) is just t/attack.)
        let peak = env_at(duration);
        if t < duration {
            lerp(0.0, peak, t / duration.max(1e-9))
        } else if t < duration + release {
            lerp(peak, 0.0, (t - duration) / release.max(1e-9))
        } else {
            0.0
        }
    } else if attack + decay > duration {
        // Decay overruns the note: full attack, then decay is cut at `duration`.
        let cut = env_at(duration);
        if t < attack {
            t / attack.max(1e-9)
        } else if t < duration {
            lerp(1.0, cut, (t - attack) / (duration - attack).max(1e-9))
        } else if t < duration + release {
            lerp(cut, 0.0, (t - duration) / release.max(1e-9))
        } else {
            0.0
        }
    } else if t < attack {
        t / attack.max(1e-9)
    } else if t < attack + decay {
        1.0 - (1.0 - sustain) * ((t - attack) / decay.max(1e-9))
    } else if t < duration {
        sustain
    } else if t < duration + release {
        sustain * (1.0 - (t - duration) / release.max(1e-9))
    } else {
        0.0
    }
}

fn lerp(a: f32, b: f32, frac: f32) -> f32 {
    a + (b - a) * frac
}
