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

pub(crate) fn adsr_value(adsr: &Adsr, t: f32, hold_end: f32) -> f32 {
    let Adsr {
        attack,
        decay,
        sustain,
        release,
    } = *adsr;
    if t < attack {
        t / attack.max(1e-9)
    } else if t < attack + decay {
        1.0 - (1.0 - sustain) * ((t - attack) / decay.max(1e-9))
    } else if t < hold_end {
        sustain
    } else if t < hold_end + release {
        sustain * (1.0 - (t - hold_end) / release.max(1e-9))
    } else {
        0.0
    }
}
