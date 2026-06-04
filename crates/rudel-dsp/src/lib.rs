// rudel-dsp - synthesis voices for Rudel.
// Phase-3 voices are hand-rolled (oscillator + ADSR + pan) so they're
// deterministic and testable offline; fundsp powers effects in a later phase.
// Param model mirrors strudel/packages/superdough/synth.mjs.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::Value;
use std::collections::BTreeMap;
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

impl Waveform {
    pub fn from_name(name: &str) -> Option<Waveform> {
        Some(match name {
            "sine" | "sin" => Waveform::Sine,
            "saw" | "sawtooth" => Waveform::Saw,
            "square" | "sqr" => Waveform::Square,
            "triangle" | "tri" => Waveform::Triangle,
            _ => return None,
        })
    }

    fn sample(self, phase: f32) -> f32 {
        let p = phase.rem_euclid(1.0);
        match self {
            Waveform::Sine => (2.0 * PI * p).sin(),
            Waveform::Saw => 2.0 * p - 1.0,
            Waveform::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Waveform::Triangle => 4.0 * (if p < 0.5 { p } else { 1.0 - p }) - 1.0,
        }
    }
}

/// Attack/decay/sustain/release envelope (seconds; sustain is a 0..1 level).
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

/// Everything needed to render one note.
#[derive(Clone, Copy, Debug)]
pub struct VoiceParams {
    pub waveform: Waveform,
    pub freq: f32,
    pub gain: f32,
    /// 0.0 = hard left, 1.0 = hard right.
    pub pan: f32,
    pub adsr: Adsr,
    /// Hold time in seconds (the note's `whole` duration), before release.
    pub duration: f32,
}

impl Default for VoiceParams {
    fn default() -> Self {
        VoiceParams {
            waveform: Waveform::Sine,
            freq: 440.0,
            gain: 1.0,
            pan: 0.5,
            adsr: Adsr::default(),
            duration: 0.25,
        }
    }
}

impl VoiceParams {
    /// Build params from a control map and the note duration in seconds.
    pub fn from_controls(map: &BTreeMap<String, Value>, duration: f32) -> VoiceParams {
        let mut p = VoiceParams {
            duration,
            ..Default::default()
        };
        if let Some(name) = map.get("s").and_then(|v| v.as_str())
            && let Some(w) = Waveform::from_name(name)
        {
            p.waveform = w;
        }
        if let Some(freq) = map.get("freq").and_then(|v| v.as_f64()) {
            p.freq = freq as f32;
        } else if let Some(n) = map.get("note") {
            p.freq = note_to_freq(n).unwrap_or(p.freq);
        } else if let Some(n) = map.get("n") {
            // bare numbers as note numbers when no note/freq given
            if let Some(f) = note_to_freq(n) {
                p.freq = f;
            }
        }
        if let Some(g) = map.get("gain").and_then(|v| v.as_f64()) {
            p.gain = g as f32;
        }
        if let Some(pan) = map.get("pan").and_then(|v| v.as_f64()) {
            p.pan = pan as f32;
        }
        if let Some(a) = map.get("attack").and_then(|v| v.as_f64()) {
            p.adsr.attack = a as f32;
        }
        if let Some(d) = map.get("decay").and_then(|v| v.as_f64()) {
            p.adsr.decay = d as f32;
        }
        if let Some(s) = map.get("sustain").and_then(|v| v.as_f64()) {
            p.adsr.sustain = s as f32;
        }
        if let Some(r) = map.get("release").and_then(|v| v.as_f64()) {
            p.adsr.release = r as f32;
        }
        p
    }
}

/// MIDI note number to frequency in Hz.
pub fn mtof(note: f64) -> f32 {
    (440.0 * 2f64.powf((note - 69.0) / 12.0)) as f32
}

/// Convert a note value (number, or a name like `c4`/`eb3`/`f#5`) to a
/// frequency. Note names follow the convention a4 = 69 = 440 Hz.
pub fn note_to_freq(value: &Value) -> Option<f32> {
    if let Some(n) = value.as_f64() {
        // numeric: treat as a MIDI note number
        if matches!(value, Value::Int(_) | Value::F64(_)) {
            return Some(mtof(n));
        }
    }
    let s = value.as_str()?;
    note_name_to_midi(s).map(|m| mtof(m as f64))
}

/// Parse a note name like `c`, `cs4`, `c#4`, `eb3`, `Gb2` to a MIDI number.
pub fn note_name_to_midi(s: &str) -> Option<i32> {
    let mut chars = s.chars().peekable();
    let letter = chars.next()?.to_ascii_lowercase();
    let base = match letter {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let mut semis = base;
    let mut octave: i32 = 3; // default octave (Strudel uses 3 when omitted)
    let mut octave_seen = false;
    let mut octave_str = String::new();
    while let Some(&c) = chars.peek() {
        match c {
            's' | '#' => {
                semis += 1;
                chars.next();
            }
            'b' => {
                semis -= 1;
                chars.next();
            }
            '-' | '0'..='9' => {
                octave_str.push(c);
                octave_seen = true;
                chars.next();
            }
            _ => return None,
        }
    }
    if octave_seen {
        octave = octave_str.parse().ok()?;
    }
    // a4 = 69; midi = (octave + 1) * 12 + semis
    Some((octave + 1) * 12 + semis)
}

/// A single sounding note.
pub struct Voice {
    params: VoiceParams,
    sample_rate: f32,
    phase: f32,
    t: f32, // elapsed seconds
    left_gain: f32,
    right_gain: f32,
    hold_end: f32,
    done: bool,
}

impl Voice {
    pub fn new(params: VoiceParams, sample_rate: f32) -> Voice {
        let pan = params.pan.clamp(0.0, 1.0);
        // equal-power panning
        let left_gain = (pan * PI / 2.0).cos();
        let right_gain = (pan * PI / 2.0).sin();
        let hold_end = params.duration.max(params.adsr.attack);
        Voice {
            params,
            sample_rate,
            phase: 0.0,
            t: 0.0,
            left_gain,
            right_gain,
            hold_end,
            done: false,
        }
    }

    fn envelope(&self) -> f32 {
        let Adsr {
            attack,
            decay,
            sustain,
            release,
        } = self.params.adsr;
        let t = self.t;
        if t < attack {
            t / attack.max(1e-9)
        } else if t < attack + decay {
            1.0 - (1.0 - sustain) * ((t - attack) / decay.max(1e-9))
        } else if t < self.hold_end {
            sustain
        } else if t < self.hold_end + release {
            sustain * (1.0 - (t - self.hold_end) / release.max(1e-9))
        } else {
            0.0
        }
    }

    /// Render the next stereo sample `(left, right)`.
    pub fn tick(&mut self) -> (f32, f32) {
        if self.done {
            return (0.0, 0.0);
        }
        let env = self.envelope();
        // 0.3 matches Strudel's synth turn-down (gainNode(0.3)).
        let s = self.params.waveform.sample(self.phase) * env * self.params.gain * 0.3;

        self.phase = (self.phase + self.params.freq / self.sample_rate).rem_euclid(1.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names() {
        assert_eq!(note_name_to_midi("a4"), Some(69));
        assert_eq!(note_name_to_midi("c4"), Some(60));
        assert_eq!(note_name_to_midi("c3"), Some(48));
        assert_eq!(note_name_to_midi("c#4"), Some(61));
        assert_eq!(note_name_to_midi("eb3"), Some(51));
        assert_eq!(note_name_to_midi("c"), Some(48)); // default octave 3
    }

    #[test]
    fn mtof_a4() {
        assert!((mtof(69.0) - 440.0).abs() < 0.001);
    }

    #[test]
    fn voice_produces_sound_then_finishes() {
        let p = VoiceParams {
            duration: 0.01,
            ..Default::default()
        };
        let mut v = Voice::new(p, 44100.0);
        let mut peak = 0.0f32;
        for _ in 0..44100 {
            let (l, _r) = v.tick();
            peak = peak.max(l.abs());
            if v.is_done() {
                break;
            }
        }
        assert!(peak > 0.0, "voice should produce non-silent output");
        assert!(v.is_done(), "voice should finish after its envelope");
    }

    #[test]
    fn pan_hard_left_silences_right() {
        let p = VoiceParams {
            pan: 0.0,
            ..Default::default()
        };
        let mut v = Voice::new(p, 44100.0);
        // skip the very start so the envelope has opened
        for _ in 0..100 {
            v.tick();
        }
        let (l, r) = v.tick();
        assert!(l.abs() > 0.0);
        assert!(r.abs() < 1e-6);
    }
}
