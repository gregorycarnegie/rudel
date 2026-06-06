use crate::envelope::{Adsr, adsr_value};
use crate::filter::{FilterKind, VoiceFilter};
use crate::oscillator::NoiseGen;
use crate::params::VoiceParams;
use crate::voice::VoiceLike;
use std::f32::consts::PI;

pub struct Voice {
    params: VoiceParams,
    sample_rate: f32,
    phase: f32,
    t: f32, // elapsed seconds
    left_gain: f32,
    right_gain: f32,
    hold_end: f32,
    /// Filter chain (low/high/band-pass), applied in order to the oscillator.
    filters: Vec<VoiceFilter>,
    noise: NoiseGen,
    /// FM modulator phase.
    mod_phase: f32,
    /// Per-voice phases for the super-saw source.
    super_phases: Vec<f32>,
    /// Pitch envelope as `(adsr, min_semitones, max_semitones)`.
    pitch_env: Option<(Adsr, f32, f32)>,
    done: bool,
}

impl Voice {
    pub fn new(params: VoiceParams, sample_rate: f32) -> Voice {
        let pan = params.pan.clamp(0.0, 1.0);
        // equal-power panning
        let left_gain = (pan * PI / 2.0).cos();
        let right_gain = (pan * PI / 2.0).sin();
        let hold_end = (params.duration + params.hold).max(params.adsr.attack);
        let mut filters = Vec::new();
        if params.lp.freq.is_some() {
            filters.push(VoiceFilter::new(FilterKind::Low, &params.lp, sample_rate));
        }
        if params.hp.freq.is_some() {
            filters.push(VoiceFilter::new(FilterKind::High, &params.hp, sample_rate));
        }
        if params.bp.freq.is_some() {
            filters.push(VoiceFilter::new(FilterKind::Band, &params.bp, sample_rate));
        }
        // Spread the super-saw voices' initial phases for a fuller sound.
        let super_phases = if params.supersaw {
            (0..params.unison.max(1))
                .map(|n| n as f32 / params.unison.max(1) as f32)
                .collect()
        } else {
            Vec::new()
        };
        // Pitch envelope (superdough getPitchEnvelope): sweep detune in cents.
        let pitch_active = params.penv.is_some()
            || params.pattack.is_some()
            || params.pdecay.is_some()
            || params.psustain.is_some()
            || params.prelease.is_some();
        let pitch_env = if pitch_active {
            let adsr = Adsr {
                attack: params.pattack.unwrap_or(0.2),
                decay: params.pdecay.unwrap_or(0.001),
                sustain: params.psustain.unwrap_or(1.0),
                release: params.prelease.unwrap_or(0.001),
            };
            let penv = params.penv.unwrap_or(1.0); // semitones
            let anchor = params.panchor.unwrap_or(adsr.sustain);
            let min = -penv * anchor;
            let max = penv - penv * anchor;
            Some((adsr, min, max))
        } else {
            None
        };
        Voice {
            params,
            sample_rate,
            phase: 0.0,
            t: 0.0,
            left_gain,
            right_gain,
            hold_end,
            filters,
            noise: NoiseGen::new(),
            mod_phase: 0.0,
            super_phases,
            pitch_env,
            done: false,
        }
    }

    /// Reverb send amount for this voice (`room`).
    pub fn room(&self) -> f32 {
        self.params.room
    }
    /// Delay send amount for this voice (`delay`).
    pub fn delay_send(&self) -> f32 {
        self.params.delay
    }

    fn envelope(&self) -> f32 {
        adsr_value(&self.params.adsr, self.t, self.hold_end)
    }

    /// Pitch multiplier from vibrato + pitch envelope (applied to the carrier).
    fn pitch_mult(&self) -> f32 {
        let mut semis = 0.0;
        if let Some(rate) = self.params.vib
            && rate > 0.0
        {
            semis += self.params.vibmod * (2.0 * PI * rate * self.t).sin();
        }
        if let Some((adsr, min, max)) = self.pitch_env {
            let shape = adsr_value(&adsr, self.t, self.hold_end);
            semis += min + shape * (max - min);
        }
        if semis == 0.0 {
            1.0
        } else {
            2f32.powf(semis / 12.0)
        }
    }

    /// Produce the next source sample and advance the oscillator phase(s).
    fn next_source(&mut self) -> f32 {
        let sr = self.sample_rate;
        let pitch = self.pitch_mult();
        if self.params.supersaw {
            let voices = self.params.unison.max(1);
            // main detune (cents -> semitones)
            let base = self.params.freq * pitch * 2f32.powf((self.params.detune / 100.0) / 12.0);
            let scale = if voices > 1 {
                self.params.spread / (voices as f32 - 1.0)
            } else {
                0.0
            };
            let center = self.params.spread * 0.5;
            let mut sum = 0.0;
            for (n, ph) in self.super_phases.iter_mut().enumerate() {
                let d = n as f32 * scale - center; // semitone detune for this voice
                let f = base * 2f32.powf(d / 12.0);
                sum += 2.0 * *ph - 1.0; // naive saw
                *ph = (*ph + f / sr).rem_euclid(1.0);
            }
            return sum / (voices as f32).sqrt();
        }
        if let Some(kind) = self.params.noise {
            return self.noise.next(kind);
        }
        // Oscillator, optionally frequency-modulated.
        let carrier = self.params.freq * pitch;
        let s = self.params.waveform.sample(self.phase);
        let inc = if let Some(index) = self.params.fm {
            let modfreq = carrier * self.params.fmh;
            let modv = (2.0 * PI * self.mod_phase).sin();
            self.mod_phase = (self.mod_phase + modfreq / sr).rem_euclid(1.0);
            (carrier + index * modfreq * modv) / sr
        } else {
            carrier / sr
        };
        self.phase = (self.phase + inc).rem_euclid(1.0);
        s
    }

    /// Render the next stereo sample `(left, right)`.
    pub fn tick(&mut self) -> (f32, f32) {
        if self.done {
            return (0.0, 0.0);
        }
        let env = self.envelope();
        let mut osc = self.next_source();
        let (t, hold_end, sr) = (self.t, self.hold_end, self.sample_rate);
        for f in &mut self.filters {
            osc = f.process(osc, t, hold_end, sr);
        }
        // 0.3 matches Strudel's synth turn-down (gainNode(0.3)).
        let s = osc * env * self.params.gain * 0.3;

        self.t += 1.0 / self.sample_rate;
        if self.t >= self.hold_end + self.params.adsr.release {
            self.done = true;
        }
        (s * self.left_gain, s * self.right_gain)
    }

    pub fn is_done(&self) -> bool {
        self.done
    }
}

impl VoiceLike for Voice {
    fn tick(&mut self) -> (f32, f32) {
        Voice::tick(self)
    }
    fn is_done(&self) -> bool {
        self.done
    }
    fn room(&self) -> f32 {
        self.params.room
    }
    fn delay_send(&self) -> f32 {
        self.params.delay
    }
}
