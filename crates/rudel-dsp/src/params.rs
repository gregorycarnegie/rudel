use crate::{
    envelope::{Adsr, adsr_values},
    filter::{FilterParams, FilterSet},
    fm::FmSpec,
    oscillator::{AdditiveType, NoiseKind, Waveform, build_additive},
    pitch::note_to_freq,
    wavetable::{ParamMod, WarpMode, WaveTable},
};
use rudel_core::{Value, ValueMap};

pub struct VoiceParams {
    pub waveform: Waveform,
    /// When set, the source is noise rather than the oscillator.
    pub noise: Option<NoiseKind>,
    /// Pulse-wave duty cycle (`pw`, 0..1) for `s("pulse")`.
    pub pw: f32,
    /// Pink-noise mix amount (`noise`, 0..1) blended into the oscillator.
    pub noise_mix: f32,
    /// Precomputed additive wavetable (`partials`); overrides `waveform`.
    pub additive: Option<Vec<f32>>,
    /// When true, the source is a detuned super-saw.
    pub supersaw: bool,
    /// Super-saw voice count (`unison`, clamped 1..=100 like superdough).
    pub unison: usize,
    /// Super-saw per-voice frequency spread in semitones (`detune`, falling
    /// back to `n`, like superdough's `detune ?? n ?? 0.18`).
    pub freqspread: f32,
    /// Super-saw stereo width (`spread`, 0..1): voices alternate between an
    /// L-weighted and R-weighted equal-power gain pair.
    pub panspread: f32,
    /// Multi-operator FM matrix (`fm`/`fmi`/`fmh`/`fmwave`/`fm{adsr}` + the
    /// `fmiIJ` routing and per-operator `*N` variants).
    pub fm: FmSpec,
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
    /// Wavetable source (`tables(...)` + `s("name")`); overrides the oscillator.
    pub wavetable: Option<WaveTable>,
    /// `wt` table position with its envelope + LFO.
    pub wt: ParamMod,
    /// `warp` amount with its envelope + LFO.
    pub warp: ParamMod,
    /// `warpmode`: which phase distortion `warp` applies.
    pub warpmode: WarpMode,
    /// `wtphaserand`: how much random initial phase each unison voice gets.
    /// Defaults to 1 when `unison > 1`, as `onTriggerSynth` does.
    pub wtphaserand: Option<f32>,
}

impl Default for VoiceParams {
    fn default() -> Self {
        VoiceParams {
            // A pattern that sets no `s` plays a triangle, which is
            // superdough's `defaultDefaultValues.s`. A sine is the one waveform
            // with no harmonics at all, so defaulting to it made every tune
            // that just writes `note(...)` come out soft and flute-like where
            // upstream is bright and square-ish.
            waveform: Waveform::Triangle,
            noise: None,
            pw: 0.5,
            noise_mix: 0.0,
            additive: None,
            supersaw: false,
            unison: 5,
            freqspread: 0.18,
            panspread: 0.6,
            fm: FmSpec::default(),
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
            lp: FilterSet::default().lp,
            hp: FilterSet::default().hp,
            bp: FilterSet::default().bp,
            wavetable: None,
            wt: ParamMod::default(),
            warp: ParamMod::default(),
            warpmode: WarpMode::None,
            wtphaserand: None,
        }
    }
}

impl VoiceParams {
    /// Build params from a control map and the note duration in seconds.
    pub fn from_controls(map: &ValueMap, duration: f32) -> VoiceParams {
        VoiceParams::from_controls_at(map, duration, 0.5, 0.0)
    }

    /// Like [`from_controls`](Self::from_controls) but with the pattern clock,
    /// which the wavetable `wt`/`warp` LFOs need (`wtsync` is in cycles, and an
    /// LFO's phase is locked to cycle time).
    pub fn from_controls_at(map: &ValueMap, duration: f32, cps: f64, cycle: f64) -> VoiceParams {
        let mut p = VoiceParams {
            duration,
            ..Default::default()
        };
        let s_name = map.get("s").and_then(|v| v.as_str());
        if let Some(name) = s_name {
            if name == "supersaw" {
                p.supersaw = true;
            } else if let Some(w) = Waveform::from_name(name) {
                p.waveform = w;
            } else if let Some(nk) = NoiseKind::from_name(name) {
                p.noise = Some(nk);
            }
        }
        // Additive synthesis (`partials`): build a custom wavetable over the base
        // series named by `s` (sawtooth/square/triangle/user). `s("user")` with
        // no partials falls back to a plain triangle, matching superdough.
        let float_list = |items: &[Value]| -> Vec<f32> {
            items
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect()
        };
        // `partials`: a list of harmonic magnitudes, or a count N (= N ones).
        let partials: Option<Vec<f32>> = match map.get("partials") {
            Some(Value::List(items)) => Some(float_list(items)),
            Some(v) => v.as_f64().map(|n| vec![1.0; (n as usize).max(1)]),
            None => None,
        };
        // `phases`: a list of per-harmonic phase offsets (in turns).
        let phases: Option<Vec<f32>> = match map.get("phases") {
            Some(Value::List(items)) => Some(float_list(items)),
            Some(v) => v.as_f64().map(|x| vec![x as f32]),
            None => None,
        };
        match (s_name.and_then(AdditiveType::from_name), &partials) {
            (Some(base), Some(parts)) if !parts.is_empty() => {
                p.additive = Some(build_additive(parts, phases.as_deref(), base));
            }
            (Some(AdditiveType::User), _) => p.waveform = Waveform::Triangle,
            _ => {}
        }
        if let Some(u) = map.get("unison").and_then(|v| v.as_f64()) {
            p.unison = (u as usize).clamp(1, 100);
        }
        if let Some(d) = map
            .get("detune")
            .or_else(|| map.get("n"))
            .and_then(|v| v.as_f64())
        {
            p.freqspread = d as f32;
        }
        if let Some(s) = map.get("spread").and_then(|v| v.as_f64()) {
            p.panspread = (s as f32).clamp(0.0, 1.0);
        }
        // Pulse-wave duty cycle and oscillator noise-mix amount.
        if let Some(w) = map.get("pw").and_then(|v| v.as_f64()) {
            p.pw = (w as f32).clamp(0.0, 1.0);
        }
        if let Some(n) = map.get("noise").and_then(|v| v.as_f64()) {
            p.noise_mix = (n as f32).clamp(0.0, 1.0);
        }
        // Wavetable oscillator (`wt`/`warp`/`warpmode`/`wtphaserand` + their
        // envelopes and LFOs). The table itself is attached by the audio layer,
        // which owns the loaded collections; the modulation is read here.
        let time = cycle / cps.max(1e-9);
        p.wt = ParamMod::from_controls(map, "wt", cps, time);
        p.warp = ParamMod::from_controls(map, "warp", cps, time);
        p.warpmode = match map.get("warpmode") {
            Some(Value::Str(name)) => WarpMode::from_name(name).unwrap_or(WarpMode::None),
            Some(v) => v.as_f64().map_or(WarpMode::None, |n| {
                WarpMode::from_index(n.clamp(0.0, 255.0) as u8)
            }),
            None => WarpMode::None,
        };
        p.wtphaserand = map
            .get("wtphaserand")
            .and_then(|v| v.as_f64())
            .map(|x| x as f32);
        // Multi-operator FM matrix (fm/fmi/fmh/fmwave/fm{adsr} + fmiIJ + *N).
        p.fm = FmSpec::from_controls(map);
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
        // The four envelope stages stay `Option` until they are all collected,
        // because `getADSRValues` resolves them together: naming any one changes
        // what the others default to. Filling them in field-wise over
        // `Adsr::default()` instead made `.attack(0.1)` keep decay 0.05 /
        // sustain 0.6 where upstream gives decay 0.001 / sustain 1.0.
        //
        // The `adsr`/`ad`/`ds`/`ar` shortcuts need no handling here: like
        // upstream, they are control setters that expand into these same four
        // keys in `rudel_core::controls::multi` long before the DSP layer sees
        // them, so `adsr("0.1:0.2:0.3:0.4")` arrives as plain attack/decay/
        // sustain/release.
        let num = |k: &str| map.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
        p.adsr = adsr_values(
            num("attack"),
            num("decay"),
            num("sustain"),
            num("release"),
            Adsr::default(),
        );
        if let Some(h) = map.get("hold").and_then(|v| v.as_f64()) {
            p.hold = h as f32;
        }
        // The three filter slots, parsed once for every voice type.
        let filters = FilterSet::from_controls(map);
        p.lp = filters.lp;
        p.hp = filters.hp;
        p.bp = filters.bp;
        p
    }
}
