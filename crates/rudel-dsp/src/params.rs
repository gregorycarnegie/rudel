use crate::envelope::Adsr;
use crate::filter::FilterParams;
use crate::oscillator::{NoiseKind, Waveform};
use crate::pitch::note_to_freq;
use rudel_core::Value;
use std::collections::BTreeMap;

pub struct VoiceParams {
    pub waveform: Waveform,
    /// When set, the source is noise rather than the oscillator.
    pub noise: Option<NoiseKind>,
    /// Pulse-wave duty cycle (`pw`, 0..1) for `s("pulse")`.
    pub pw: f32,
    /// Pink-noise mix amount (`noise`, 0..1) blended into the oscillator.
    pub noise_mix: f32,
    /// When true, the source is a detuned super-saw.
    pub supersaw: bool,
    /// Super-saw voice count (`unison`).
    pub unison: usize,
    /// Super-saw detune in cents (`detune`).
    pub detune: f32,
    /// Super-saw frequency spread in semitones (`spread`).
    pub spread: f32,
    /// FM modulation index (`fm`/`fmi`); `None` = no FM.
    pub fm: Option<f32>,
    /// FM harmonicity ratio (`fmh`), modulator freq / carrier freq.
    pub fmh: f32,
    /// FM modulator waveform (`fmwave`); defaults to sine.
    pub fmwave: Waveform,
    /// FM modulation-index envelope (`fmattack`/`fmdecay`/`fmsustain`/
    /// `fmrelease`); `None` = constant index. Scales the index 0..1.
    pub fm_env: Option<Adsr>,
    /// Vibrato rate in Hz (`vib`); `None`/0 = off.
    pub vib: Option<f32>,
    /// Vibrato depth in semitones (`vibmod`).
    pub vibmod: f32,
    /// Pitch-envelope amount in semitones (`penv`).
    pub penv: Option<f32>,
    pub pattack: Option<f32>,
    pub pdecay: Option<f32>,
    pub psustain: Option<f32>,
    pub prelease: Option<f32>,
    /// Pitch-envelope anchor (`panchor`); defaults to the pitch sustain.
    pub panchor: Option<f32>,
    /// Pitch-envelope curve (`pcurve`): `false` = linear (default), `true` =
    /// exponential ramp segments.
    pub pcurve_exp: bool,
    pub freq: f32,
    pub gain: f32,
    /// 0.0 = hard left, 1.0 = hard right.
    pub pan: f32,
    pub adsr: Adsr,
    /// Hold time in seconds (the note's `whole` duration), before release.
    pub duration: f32,
    /// Extra sustain hold beyond the note duration (`hold`), in seconds.
    pub hold: f32,
    /// Low-pass filter (`cutoff`/`lpf` + `lpenv`/`lpattack`/...).
    pub lp: FilterParams,
    /// High-pass filter (`hcutoff`/`hpf` + `hpenv`/...).
    pub hp: FilterParams,
    /// Band-pass filter (`bandf`/`bpf` + `bpenv`/...).
    pub bp: FilterParams,
    /// Reverb send amount (`room`), 0..1.
    pub room: f32,
    /// Delay send amount (`delay`), 0..1.
    pub delay: f32,
}

impl Default for VoiceParams {
    fn default() -> Self {
        VoiceParams {
            waveform: Waveform::Sine,
            noise: None,
            pw: 0.5,
            noise_mix: 0.0,
            supersaw: false,
            unison: 5,
            detune: 0.0,
            spread: 0.2,
            fm: None,
            fmh: 1.0,
            fmwave: Waveform::Sine,
            fm_env: None,
            vib: None,
            vibmod: 0.5,
            penv: None,
            pattack: None,
            pdecay: None,
            psustain: None,
            prelease: None,
            panchor: None,
            pcurve_exp: false,
            freq: 440.0,
            gain: 1.0,
            pan: 0.5,
            adsr: Adsr::default(),
            duration: 0.25,
            hold: 0.0,
            lp: FilterParams::default(),
            hp: FilterParams::default(),
            bp: FilterParams {
                q: 1.0,
                ..FilterParams::default()
            },
            room: 0.0,
            delay: 0.0,
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
        if let Some(name) = map.get("s").and_then(|v| v.as_str()) {
            if name == "supersaw" {
                p.supersaw = true;
            } else if let Some(w) = Waveform::from_name(name) {
                p.waveform = w;
            } else if let Some(nk) = NoiseKind::from_name(name) {
                p.noise = Some(nk);
            }
        }
        if let Some(u) = map.get("unison").and_then(|v| v.as_f64()) {
            p.unison = (u as usize).max(1);
        }
        if let Some(d) = map.get("detune").and_then(|v| v.as_f64()) {
            p.detune = d as f32;
        }
        if let Some(s) = map.get("spread").and_then(|v| v.as_f64()) {
            p.spread = s as f32;
        }
        // Pulse-wave duty cycle and oscillator noise-mix amount.
        if let Some(w) = map.get("pw").and_then(|v| v.as_f64()) {
            p.pw = (w as f32).clamp(0.0, 1.0);
        }
        if let Some(n) = map.get("noise").and_then(|v| v.as_f64()) {
            p.noise_mix = (n as f32).clamp(0.0, 1.0);
        }
        // FM: `fm`/`fmi` modulation index, `fmh` harmonicity ratio.
        if let Some(i) = map
            .get("fm")
            .or_else(|| map.get("fmi"))
            .and_then(|v| v.as_f64())
        {
            p.fm = Some(i as f32);
        }
        if let Some(h) = map.get("fmh").and_then(|v| v.as_f64()) {
            p.fmh = h as f32;
        }
        if let Some(w) = map.get("fmwave").and_then(|v| v.as_str())
            && let Some(wave) = Waveform::from_name(w)
        {
            p.fmwave = wave;
        }
        // FM modulation-index envelope: active if any of the fm{a,d,s,r} are set.
        // Sustain defaults to full (1.0) when only attack/decay are given, like
        // superdough's getADSRValues.
        let fmv = |k: &str| map.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
        let (fa, fd, fs, fr) = (
            fmv("fmattack"),
            fmv("fmdecay"),
            fmv("fmsustain"),
            fmv("fmrelease"),
        );
        if fa.is_some() || fd.is_some() || fs.is_some() || fr.is_some() {
            let sustain = fs.unwrap_or(1.0);
            p.fm_env = Some(Adsr {
                attack: fa.unwrap_or(0.001).max(0.001),
                decay: fd.unwrap_or(0.001).max(0.001),
                sustain: sustain.clamp(0.0, 1.0),
                release: fr.unwrap_or(0.01).max(0.01),
            });
        }
        // Vibrato (`vib` rate Hz, `vibmod` depth semitones).
        if let Some(r) = map.get("vib").and_then(|v| v.as_f64()) {
            p.vib = Some(r as f32);
        }
        if let Some(d) = map.get("vibmod").and_then(|v| v.as_f64()) {
            p.vibmod = d as f32;
        }
        // Pitch envelope (`penv` semitones + `p{attack,decay,sustain,release}`).
        p.penv = map.get("penv").and_then(|v| v.as_f64()).map(|x| x as f32);
        p.pattack = map
            .get("pattack")
            .and_then(|v| v.as_f64())
            .map(|x| x as f32);
        p.pdecay = map.get("pdecay").and_then(|v| v.as_f64()).map(|x| x as f32);
        p.psustain = map
            .get("psustain")
            .and_then(|v| v.as_f64())
            .map(|x| x as f32);
        p.prelease = map
            .get("prelease")
            .and_then(|v| v.as_f64())
            .map(|x| x as f32);
        p.panchor = map
            .get("panchor")
            .and_then(|v| v.as_f64())
            .map(|x| x as f32);
        // `pcurve`: 0 = linear (default), nonzero = exponential ramp segments.
        if let Some(c) = map.get("pcurve").and_then(|v| v.as_f64()) {
            p.pcurve_exp = c != 0.0;
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
        // ADSR shortcut controls accept a `:`-list, e.g. `adsr("0.1:0.1:0.5:0.2")`.
        let list = |k: &str| -> Option<Vec<f32>> {
            map.get(k).map(|v| match v {
                Value::List(items) => items
                    .iter()
                    .filter_map(|x| x.as_f64().map(|f| f as f32))
                    .collect(),
                other => other.as_f64().map(|f| f as f32).into_iter().collect(),
            })
        };
        if let Some(v) = list("adsr") {
            if let Some(a) = v.first() {
                p.adsr.attack = *a;
            }
            if let Some(d) = v.get(1) {
                p.adsr.decay = *d;
            }
            if let Some(s) = v.get(2) {
                p.adsr.sustain = *s;
            }
            if let Some(r) = v.get(3) {
                p.adsr.release = *r;
            }
        }
        if let Some(v) = list("ad") {
            // attack/decay with no sustain (percussive)
            if let Some(a) = v.first() {
                p.adsr.attack = *a;
            }
            if let Some(d) = v.get(1) {
                p.adsr.decay = *d;
            }
            p.adsr.sustain = 0.0;
        }
        if let Some(v) = list("ar") {
            if let Some(a) = v.first() {
                p.adsr.attack = *a;
            }
            if let Some(r) = v.get(1) {
                p.adsr.release = *r;
            }
        }
        if let Some(h) = map.get("hold").and_then(|v| v.as_f64()) {
            p.hold = h as f32;
        }
        let get = |k: &str| map.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
        // Low-pass (cutoff/lpf) + its envelope.
        p.lp.freq = get("cutoff");
        if let Some(q) = get("resonance") {
            p.lp.q = q.max(0.1);
        }
        p.lp.env = get("lpenv");
        p.lp.attack = get("lpattack");
        p.lp.decay = get("lpdecay");
        p.lp.sustain = get("lpsustain");
        p.lp.release = get("lprelease");
        // High-pass (hcutoff/hpf) + its envelope.
        p.hp.freq = get("hcutoff");
        if let Some(q) = get("hresonance") {
            p.hp.q = q.max(0.1);
        }
        p.hp.env = get("hpenv");
        p.hp.attack = get("hpattack");
        p.hp.decay = get("hpdecay");
        p.hp.sustain = get("hpsustain");
        p.hp.release = get("hprelease");
        // Band-pass (bandf/bpf) + its envelope.
        p.bp.freq = get("bandf");
        if let Some(q) = get("bandq") {
            p.bp.q = q.max(0.1);
        }
        p.bp.env = get("bpenv");
        p.bp.attack = get("bpattack");
        p.bp.decay = get("bpdecay");
        p.bp.sustain = get("bpsustain");
        p.bp.release = get("bprelease");
        // Shared filter-envelope anchor (`fanchor`).
        if let Some(a) = get("fanchor") {
            p.lp.anchor = a;
            p.hp.anchor = a;
            p.bp.anchor = a;
        }
        if let Some(room) = map.get("room").and_then(|v| v.as_f64()) {
            p.room = room as f32;
        }
        if let Some(d) = map.get("delay").and_then(|v| v.as_f64()) {
            p.delay = d as f32;
        }
        p
    }
}
