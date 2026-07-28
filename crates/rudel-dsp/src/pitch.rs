use crate::envelope::{Adsr, adsr_value};
use rudel_core::{Value, ValueMap};
use std::f32::consts::TAU;

pub fn mtof(note: f64) -> f32 {
    rudel_core::midi_to_freq(note) as f32
}

/// Convert a note value (number, or a name like `c4`/`eb3`/`f#5`) to a
/// frequency. Note names follow the convention a4 = 69 = 440 Hz.
pub fn note_to_freq(value: &Value) -> Option<f32> {
    rudel_core::get_freq(value).map(|freq| freq as f32)
}

/// Parse a note name like `c`, `cs4`, `c#4`, `eb3`, `Gb2` to a MIDI number.
/// Delegates to the canonical implementation in `rudel_core::tonal`.
pub fn note_name_to_midi(s: &str) -> Option<i32> {
    rudel_core::note_to_midi(s)
}

/// Exponential (geometric) interpolation between `a` and `b` over progress `p`,
/// matching Web Audio's `exponentialRampToValueAtTime`. Zeros are nudged off the
/// axis; if the endpoints straddle zero (undefined for an exp ramp) it falls
/// back to linear.
fn geo(a: f32, b: f32, p: f32) -> f32 {
    let nz = |x: f32| if x == 0.0 { 0.001 } else { x };
    let (a, b) = (nz(a), nz(b));
    if a.signum() != b.signum() {
        a + (b - a) * p
    } else {
        a * (b / a).powf(p)
    }
}

/// The pitch-envelope value (in semitones) at time `t`. Linear by default;
/// `exp` switches to exponential ramp segments (`pcurve`).
fn pitch_env_value(adsr: &Adsr, t: f32, hold_end: f32, min: f32, max: f32, exp: bool) -> f32 {
    if !exp {
        return min + adsr_value(adsr, t, hold_end) * (max - min);
    }
    let Adsr {
        attack,
        decay,
        sustain,
        release,
    } = *adsr;
    let sustain_val = min + sustain * (max - min);
    if t < attack {
        geo(min, max, t / attack.max(1e-9))
    } else if t < attack + decay {
        geo(max, sustain_val, (t - attack) / decay.max(1e-9))
    } else if t < hold_end {
        sustain_val
    } else if t < hold_end + release {
        geo(sustain_val, min, (t - hold_end) / release.max(1e-9))
    } else {
        min
    }
}

/// Vibrato (`vib`/`vibmod`) plus the pitch envelope (`penv` and its
/// `p{attack,decay,sustain,release}`/`panchor`/`pcurve`) — together, the detune
/// superdough's `getVibratoOscillator` + `getPitchEnvelope` apply.
///
/// Both the oscillator synth and the sampler use this: upstream wires them onto
/// the source node's `detune` in either case (`synth.mjs` and `sampler.mjs`, and
/// the soundfont loader), so `vib`/`penv` shape a played sample exactly as they
/// shape a synth note.
#[derive(Clone, Copy, Debug, Default)]
pub struct PitchMod {
    /// Vibrato rate in Hz; `None`/0 = off.
    pub vib: Option<f32>,
    /// Vibrato depth in semitones.
    pub vibmod: f32,
    /// The pitch envelope's ADSR and its `(min, max)` semitone range.
    env: Option<(Adsr, f32, f32)>,
    /// `pcurve`: exponential ramp segments rather than linear.
    exp: bool,
}

impl PitchMod {
    /// Build from already-resolved parameters (the oscillator synth reads these
    /// off its `VoiceParams`).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        vib: Option<f32>,
        vibmod: f32,
        penv: Option<f32>,
        pattack: Option<f32>,
        pdecay: Option<f32>,
        psustain: Option<f32>,
        prelease: Option<f32>,
        panchor: Option<f32>,
        exp: bool,
    ) -> PitchMod {
        // superdough's `getPitchEnvelope` engages when any of its controls is
        // present, defaulting the rest.
        let active = penv.is_some()
            || pattack.is_some()
            || pdecay.is_some()
            || psustain.is_some()
            || prelease.is_some();
        let env = active.then(|| {
            let adsr = Adsr {
                attack: pattack.unwrap_or(0.2),
                decay: pdecay.unwrap_or(0.001),
                sustain: psustain.unwrap_or(1.0),
                release: prelease.unwrap_or(0.001),
            };
            let penv = penv.unwrap_or(1.0); // semitones
            let anchor = panchor.unwrap_or(adsr.sustain);
            (adsr, -penv * anchor, penv - penv * anchor)
        });
        PitchMod {
            vib,
            vibmod,
            env,
            exp,
        }
    }

    /// Read the controls directly (the sampler has no resolved param struct for
    /// these).
    pub fn from_controls(map: &ValueMap) -> PitchMod {
        let get = |k: &str| map.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
        PitchMod::new(
            get("vib"),
            // `getVibratoOscillator` destructures `{ vibmod = 0.5 }`, so an
            // unset depth is half a semitone, not none.
            get("vibmod").unwrap_or(0.5),
            get("penv"),
            get("pattack"),
            get("pdecay"),
            get("psustain"),
            get("prelease"),
            get("panchor"),
            get("pcurve").is_some_and(|c| c != 0.0),
        )
    }

    /// True when neither vibrato nor a pitch envelope is engaged, so callers can
    /// skip the per-sample work entirely.
    pub fn is_idle(&self) -> bool {
        self.env.is_none() && !matches!(self.vib, Some(r) if r > 0.0)
    }

    /// Detune in semitones at elapsed time `t`, for a note held to `hold_end`.
    pub fn semitones(&self, t: f32, hold_end: f32) -> f32 {
        let mut semis = 0.0;
        if let Some(rate) = self.vib
            && rate > 0.0
        {
            semis += self.vibmod * (TAU * rate * t).sin();
        }
        if let Some((adsr, min, max)) = self.env {
            semis += pitch_env_value(&adsr, t, hold_end, min, max, self.exp);
        }
        semis
    }

    /// The frequency (or playback-rate) multiplier at time `t`.
    pub fn multiplier(&self, t: f32, hold_end: f32) -> f32 {
        let semis = self.semitones(t, hold_end);
        if semis == 0.0 {
            1.0
        } else {
            2f32.powf(semis / 12.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_pitch_mod_is_a_no_op() {
        let p = PitchMod::from_controls(&ValueMap::default());
        assert!(p.is_idle());
        assert_eq!(p.multiplier(0.0, 1.0), 1.0);
        assert_eq!(p.multiplier(0.5, 1.0), 1.0);
    }

    #[test]
    fn vibrato_depth_defaults_to_half_a_semitone() {
        // superdough's `getVibratoOscillator` destructures `{ vibmod = 0.5 }`.
        let map: ValueMap = [("vib".to_string(), Value::F64(5.0))].into_iter().collect();
        let p = PitchMod::from_controls(&map);
        assert!((p.semitones(0.05, 1.0) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn vibrato_swings_around_unity() {
        let map: ValueMap = [
            ("vib".to_string(), Value::F64(5.0)),
            ("vibmod".to_string(), Value::F64(1.0)),
        ]
        .into_iter()
        .collect();
        let p = PitchMod::from_controls(&map);
        assert!(!p.is_idle());
        // A 5Hz LFO: zero at t=0, +1 semitone a quarter period in, -1 at three
        // quarters.
        assert!((p.semitones(0.0, 1.0)).abs() < 1e-6);
        assert!((p.semitones(0.05, 1.0) - 1.0).abs() < 1e-4);
        assert!((p.semitones(0.15, 1.0) + 1.0).abs() < 1e-4);
        assert!((p.multiplier(0.05, 1.0) - 2f32.powf(1.0 / 12.0)).abs() < 1e-5);
    }

    #[test]
    fn a_pitch_envelope_sweeps_between_its_endpoints() {
        // penv 12 semitones with sustain 0 anchors the envelope at the note's
        // resting pitch, so it starts an octave up and falls back to unity.
        let map: ValueMap = [
            ("penv".to_string(), Value::F64(12.0)),
            ("pattack".to_string(), Value::F64(0.0)),
            ("pdecay".to_string(), Value::F64(0.5)),
            ("psustain".to_string(), Value::F64(0.0)),
        ]
        .into_iter()
        .collect();
        let p = PitchMod::from_controls(&map);
        assert!(!p.is_idle());
        let start = p.semitones(0.001, 1.0);
        assert!(
            (start - 12.0).abs() < 0.1,
            "expected ~12 semitones, got {start}"
        );
        assert!(p.semitones(0.5, 1.0).abs() < 0.1);
    }
}
