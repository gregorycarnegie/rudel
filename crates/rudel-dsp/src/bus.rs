//! Buses: the orbit-bus effects superdough runs on a whole orbit rather than on
//! a single voice, plus the signal buses `bus`/`bmod`/`s("bus")` route through.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    envelope::{Adsr, adsr_value},
    filter::{FilterSet, VoiceFilters},
    modulator::{ModBank, ModSpec},
    sampler::Sample,
    voice::VoiceLike,
};
use rudel_core::{Value, ValueMap};
use std::{
    f32::consts::{FRAC_PI_2, PI},
    sync::Arc,
};

/// Per-orbit reverb settings. These are superdough's `createReverb` arguments
/// (`roomsize`/`roomfade`/`roomlp`/`roomdim`), which it deliberately leaves
/// `undefined` until the node is built so the defaults live in one place.
#[derive(Clone)]
pub struct ReverbConfig {
    /// `size`/`roomsize`: the -60dB decay time in seconds.
    pub size: f32,
    /// `roomfade`: fade-in time of the impulse response, in seconds.
    pub fade: f32,
    /// `roomlp`: the IR's lowpass frequency at the start of the decay.
    pub lp: f32,
    /// `roomdim`: the IR's lowpass frequency at the end of the decay.
    pub dim: f32,
    /// `ir`/`iresponse`: a loaded sample to convolve against instead of the
    /// generated noise tail. Resolved against the sample bank by the audio
    /// layer, which owns it; `None` uses the generated IR.
    pub ir: Option<Arc<Sample>>,
    /// `irspeed`: playback rate when reading the loaded impulse response.
    pub irspeed: f32,
    /// `irbegin`: where in the loaded impulse response to start, 0..1.
    pub irbegin: f32,
}

/// Hand-written so the impulse response shows as its length rather than
/// dumping every sample (`Sample` is deliberately not `Debug`).
impl std::fmt::Debug for ReverbConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReverbConfig")
            .field("size", &self.size)
            .field("fade", &self.fade)
            .field("lp", &self.lp)
            .field("dim", &self.dim)
            .field("ir", &self.ir.as_ref().map(|s| s.data.len()))
            .field("irspeed", &self.irspeed)
            .field("irbegin", &self.irbegin)
            .finish()
    }
}

/// Two configs are the same reverb when their numbers match and they point at
/// the same impulse response — `Arc` identity, since a loaded sample is
/// registered once and shared.
impl PartialEq for ReverbConfig {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size
            && self.fade == other.fade
            && self.lp == other.lp
            && self.dim == other.dim
            && self.irspeed == other.irspeed
            && self.irbegin == other.irbegin
            && match (&self.ir, &other.ir) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}

impl Default for ReverbConfig {
    /// `convolver.generate(d = 2, fade = 0.1, lp = 15000, dim = 1000)`, plus
    /// superdough's `irspeed`/`irbegin` defaults.
    fn default() -> ReverbConfig {
        ReverbConfig {
            size: 2.0,
            fade: 0.1,
            lp: 15000.0,
            dim: 1000.0,
            ir: None,
            irspeed: 1.0,
            irbegin: 0.0,
        }
    }
}

/// Per-orbit delay settings (`delaytime`/`delaysync`/`delayfeedback`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelayConfig {
    /// Delay time in seconds.
    pub time: f32,
    /// Feedback, clamped to superdough's 0..0.98 ear-saver.
    pub feedback: f32,
}

impl Default for DelayConfig {
    fn default() -> Self {
        DelayConfig {
            // `delaysync` defaults to 3/16 of a cycle; at the default 1 cps that
            // is 0.1875s. `from_controls` recomputes it against the live cps.
            time: 3.0 / 16.0,
            feedback: 0.5,
        }
    }
}

/// How one voice routes into its orbit, plus the orbit settings it carries.
///
/// superdough reads all of these off the same control map as the voice and then
/// applies them to the orbit's shared nodes, so the most recent event to hit an
/// orbit configures it (`getReverb`/`getDelay` only rebuild when a value
/// actually changed). Rudel does the same, at voice onset.
#[derive(Clone, Debug)]
pub struct OrbitSend {
    /// Which orbit bus this voice feeds (`orbit`, default 1).
    pub orbit: i32,
    /// Direct-signal level (`dry`), 0..1. The reverb/delay sends are taken
    /// pre-dry, so `dry(0)` leaves only the wet signal.
    pub dry: f32,
    /// Reverb send amount (`room`).
    pub room: f32,
    /// Delay send amount (`delay`).
    pub delay: f32,
    /// Reverb settings for the orbit.
    pub reverb: ReverbConfig,
    /// Delay settings for the orbit.
    pub delay_cfg: DelayConfig,
    /// `djf` knob for the orbit; `None` leaves it as it was.
    pub djf: Option<f32>,
    /// `bus`: also send this voice's post-fx output to a numbered signal bus,
    /// where another pattern's `bmod` can read it. Additional to the orbit
    /// routing, so `dry(0)` makes a voice a pure modulation source.
    pub bus: Option<i32>,
    /// `busgain`: level of that send.
    pub busgain: f32,
}

impl Default for OrbitSend {
    fn default() -> Self {
        OrbitSend {
            orbit: 1,
            dry: 1.0,
            room: 0.0,
            delay: 0.0,
            reverb: ReverbConfig::default(),
            delay_cfg: DelayConfig::default(),
            djf: None,
            bus: None,
            busgain: 1.0,
        }
    }
}

impl OrbitSend {
    /// Read the routing controls from a hap's control map. `cps` converts
    /// `delaysync` (a fraction of a cycle) into seconds, as superdough's
    /// `delaytime = delaytime ?? cycleToSeconds(delaysync, cps)` does.
    pub fn from_controls(map: &ValueMap, cps: f64) -> OrbitSend {
        let get = |k: &str| map.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
        let d = OrbitSend::default();
        let delaysync = get("delaysync").unwrap_or(3.0 / 16.0);
        OrbitSend {
            orbit: get("orbit").map(|o| o as i32).unwrap_or(d.orbit),
            dry: get("dry").unwrap_or(d.dry),
            room: get("room").unwrap_or(d.room),
            delay: get("delay").unwrap_or(d.delay),
            reverb: ReverbConfig {
                size: get("size").unwrap_or(d.reverb.size),
                fade: get("roomfade").unwrap_or(d.reverb.fade),
                lp: get("roomlp").unwrap_or(d.reverb.lp),
                dim: get("roomdim").unwrap_or(d.reverb.dim),
                // The impulse response itself is a sample name, resolved by the
                // audio layer (which owns the bank) after this call.
                ir: None,
                irspeed: get("irspeed").unwrap_or(d.reverb.irspeed),
                irbegin: get("irbegin").unwrap_or(d.reverb.irbegin),
            },
            delay_cfg: DelayConfig {
                time: get("delaytime").unwrap_or((delaysync as f64 / cps.max(1e-9)) as f32),
                feedback: get("delayfeedback")
                    .unwrap_or(d.delay_cfg.feedback)
                    .clamp(0.0, 0.98),
            },
            djf: get("djf"),
            bus: get("bus").map(|b| b as i32),
            busgain: get("busgain").unwrap_or(d.busgain),
        }
    }
}

/// One orbit to sidechain-duck when a voice starts (`duckorbit` and friends).
///
/// A voice can name several targets with a `:`-list (`duckorbit("2:3")`), and
/// `duckonset`/`duckattack`/`duckdepth` may be `:`-lists too — read per target,
/// falling back to the first entry, exactly as superdough's
/// `SuperdoughAudioController.duck` does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Duck {
    /// The orbit whose output gain gets ducked.
    pub orbit: i32,
    /// `duckonset`: seconds to dip down over.
    pub onset: f32,
    /// `duckattack`: seconds to recover over (at least 0.002).
    pub attack: f32,
    /// `duckdepth`: 0..1, how far down to dip.
    pub depth: f32,
}

impl Duck {
    /// Read `duckorbit`/`duckonset`/`duckattack`/`duckdepth` from a control
    /// map. Empty when the voice does not duck anything.
    pub fn from_controls(map: &ValueMap) -> Vec<Duck> {
        // A `:`-list arrives as a `Value::List`; a bare number as itself.
        let list = |k: &str| -> Vec<f32> {
            match map.get(k) {
                Some(Value::List(items)) => items
                    .iter()
                    .filter_map(|v| v.as_f64())
                    .map(|x| x as f32)
                    .collect(),
                Some(v) => v.as_f64().map(|x| vec![x as f32]).unwrap_or_default(),
                None => Vec::new(),
            }
        };
        let targets = list("duckorbit");
        if targets.is_empty() {
            return Vec::new();
        }
        let (onsets, attacks, depths) = (list("duckonset"), list("duckattack"), list("duckdepth"));
        // Per target, take entry `i` or fall back to entry 0, then the default.
        let at = |v: &[f32], i: usize, default: f32| {
            v.get(i).or_else(|| v.first()).copied().unwrap_or(default)
        };
        targets
            .iter()
            .enumerate()
            .map(|(i, &orbit)| Duck {
                orbit: orbit as i32,
                onset: at(&onsets, i, 0.0),
                attack: at(&attacks, i, 0.1).max(0.002),
                depth: at(&depths, i, 1.0),
            })
            .collect()
    }
}

/// The sidechain duck envelope on an orbit's output gain: an exponential dip to
/// `1 - sqrt(depth)` over `duckonset`, then an exponential recovery to unity
/// over `duckattack`.
///
/// Ported from superdough's `Orbit.duck`, which schedules a pair of
/// `exponentialRampToValueAtTime`s. An exponential ramp is geometric, so each
/// segment is just a constant per-sample multiplier — no `powf` in the loop.
/// Re-triggering ramps from the *current* gain rather than from unity, matching
/// the `cancelScheduledValues` + `setValueAtTime(currVal)` pair upstream uses to
/// emulate `cancelAndHoldAtTime`.
#[derive(Clone, Copy, Debug)]
pub struct DuckEnv {
    /// Current gain, 1.0 when not ducking.
    gain: f32,
    /// Samples remaining in the dip, then in the recovery.
    dip_left: u32,
    rise_left: u32,
    /// Per-sample multipliers for each segment.
    dip_mul: f32,
    rise_mul: f32,
    /// The gain at the bottom of the dip.
    floor: f32,
}

impl Default for DuckEnv {
    fn default() -> Self {
        DuckEnv {
            gain: 1.0,
            dip_left: 0,
            rise_left: 0,
            dip_mul: 1.0,
            rise_mul: 1.0,
            floor: 1.0,
        }
    }
}

impl DuckEnv {
    /// Start (or restart) a duck.
    pub fn trigger(&mut self, sample_rate: f32, duck: &Duck) {
        // superdough: `clamp(1 - sqrt(depth), 0.01, currVal)`. The 0.01 floor is
        // what keeps the exponential ramps away from zero, where they are
        // undefined.
        let floor = (1.0 - duck.depth.max(0.0).sqrt()).clamp(0.01, self.gain.max(0.01));
        let dip = (sample_rate * duck.onset.max(0.0)) as u32;
        let rise = ((sample_rate * duck.attack.max(0.002)) as u32).max(1);
        self.dip_mul = if dip > 0 {
            (floor / self.gain).powf(1.0 / dip as f32)
        } else {
            // A zero onset is an immediate drop (upstream's clicky default).
            self.gain = floor;
            1.0
        };
        self.rise_mul = (1.0 / floor).powf(1.0 / rise as f32);
        self.floor = floor;
        self.dip_left = dip;
        self.rise_left = rise;
    }

    /// Advance one sample and return the gain to apply.
    pub fn next_gain(&mut self) -> f32 {
        if self.dip_left > 0 {
            self.dip_left -= 1;
            self.gain *= self.dip_mul;
            if self.dip_left == 0 {
                self.gain = self.floor;
            }
        } else if self.rise_left > 0 {
            self.rise_left -= 1;
            self.gain *= self.rise_mul;
            if self.rise_left == 0 {
                self.gain = 1.0;
            }
        }
        self.gain
    }

    /// True when the envelope is at unity with nothing scheduled, so the whole
    /// stage can be skipped.
    pub fn is_idle(&self) -> bool {
        self.dip_left == 0 && self.rise_left == 0 && self.gain == 1.0
    }
}

/// `s("bus")`: play a signal bus back as a sound source, so a second pattern
/// can run whatever was sent to it through its own effects.
///
/// Ported from superdough's `registerSound('bus')`, which connects the bus node
/// through a gain node holding a linear ADSR for the hap's duration. The rest of
/// the voice chain then treats it like any other source.
#[derive(Clone, Copy, Debug)]
pub struct BusParams {
    /// Which bus to read — `value.n ?? 0`, so `s("bus").n(1)`.
    pub bus: i32,
    /// The gain envelope. `registerSound('bus')` defaults it to
    /// `[0.001, 0.05, 1, 0.01]`: sustain 1, unlike the synth voices' 0.6.
    pub adsr: Adsr,
    /// The hap's length in seconds — how long the envelope holds.
    pub duration: f32,
    pub gain: f32,
    pub pan: f32,
    /// `lpf`/`hpf`/`bpf` on the returned signal, which is most of the reason to
    /// route a bus through a second pattern in the first place.
    pub filters: FilterSet,
}

impl BusParams {
    pub fn from_controls(map: &ValueMap, duration: f32) -> BusParams {
        let num = |k: &str| map.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
        BusParams {
            bus: num("n").unwrap_or(0.0) as i32,
            adsr: Adsr {
                attack: num("attack").unwrap_or(0.001),
                decay: num("decay").unwrap_or(0.05),
                sustain: num("sustain").unwrap_or(1.0),
                release: num("release").unwrap_or(0.01),
            },
            duration,
            gain: num("gain").unwrap_or(1.0),
            pan: num("pan").unwrap_or(0.5),
            filters: FilterSet::from_controls(map),
        }
    }
}

/// A playing [`BusParams`]. It has no signal of its own: the mixer refills its
/// input before every block ([`VoiceLike::set_bus_input`]) and renders the
/// voices *sending* to a bus first, so what a source reads is the block those
/// senders just wrote.
pub struct BusVoice {
    params: BusParams,
    filters: VoiceFilters,
    mods: ModBank,
    sample_rate: f32,
    left: Vec<f32>,
    right: Vec<f32>,
    pos: usize,
    /// Elapsed seconds, driving the envelope.
    t: f32,
    dt: f32,
    end: f32,
    left_gain: f32,
    right_gain: f32,
}

impl BusVoice {
    pub fn new(params: BusParams, sample_rate: f32) -> BusVoice {
        BusVoice::with_mods(params, sample_rate, &[])
    }

    pub fn with_mods(params: BusParams, sample_rate: f32, mods: &[ModSpec]) -> BusVoice {
        let pan = params.pan.clamp(0.0, 1.0);
        // `end = begin + duration + release + 0.01`, as the sound's timeout uses.
        let end = params.duration + params.adsr.release + 0.01;
        BusVoice {
            // The bus is stereo, and a filter is stateful and mono, so each
            // channel needs its own bank.
            filters: VoiceFilters::new(&params.filters, sample_rate, true),
            mods: ModBank::new(mods, sample_rate as f64),
            sample_rate,
            left: Vec::new(),
            right: Vec::new(),
            pos: 0,
            t: 0.0,
            dt: 1.0 / sample_rate,
            end,
            left_gain: (pan * FRAC_PI_2).cos(),
            right_gain: (pan * FRAC_PI_2).sin(),
            params,
        }
    }
}

impl VoiceLike for BusVoice {
    fn tick(&mut self) -> (f32, f32) {
        if self.is_done() {
            return (0.0, 0.0);
        }
        let env = adsr_value(&self.params.adsr, self.t, self.params.duration) * self.params.gain;
        // Reading past the block the mixer supplied is silence, not a panic.
        let l = self.left.get(self.pos).copied().unwrap_or(0.0);
        let r = self.right.get(self.pos).copied().unwrap_or(0.0);
        self.mods.tick();
        let (l, r) = self.filters.process_stereo(
            l,
            r,
            self.t,
            self.params.duration,
            self.sample_rate,
            &self.mods,
        );
        self.pos += 1;
        self.t += self.dt;
        (l * env * self.left_gain, r * env * self.right_gain)
    }

    fn set_bus_input(&mut self, bus: i32, left: &[f32], right: &[f32]) {
        if bus != self.params.bus {
            return;
        }
        self.left.clear();
        self.left.extend_from_slice(left);
        self.right.clear();
        self.right.extend_from_slice(right);
        self.pos = 0;
    }

    fn is_done(&self) -> bool {
        self.t >= self.end
    }
}

/// superdough's `TwoPoleFilter` (`worklets.mjs`): a state-variable pair giving a
/// band-pass (`s0`) and a low-pass (`s1`) from the same recurrence.
#[derive(Clone, Copy, Default)]
struct TwoPole {
    s0: f32,
    s1: f32,
}

impl TwoPole {
    /// Advance by one sample, returning the low-pass output.
    fn update(&mut self, s: f32, sample_rate: f32, cutoff: f32, resonance: f32) -> f32 {
        // Out-of-bound values can produce NaNs (upstream's comment).
        let resonance = resonance.clamp(0.0, 1.0);
        let cutoff = cutoff.clamp(0.0, sample_rate / 2.0 - 1.0);
        let c = (2.0 * (cutoff * PI / sample_rate).sin()).clamp(0.0, 1.14);
        let r = 0.5f32.powf(8.0 * resonance + 1.0);
        let mrc = 1.0 - r * c;
        self.s0 = mrc * self.s0 - c * self.s1 + c * s;
        self.s1 = mrc * self.s1 + c * self.s0;
        self.s1
    }
}

/// Which side of centre the DJ filter is on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DjfMode {
    /// `value` in the 0.49..=0.51 dead zone — the signal passes through.
    Bypass,
    Lowpass,
    Highpass,
}

/// The DJ filter (`djf`), ported from superdough's `djf-processor` worklet: one
/// knob sweeping a low-pass below centre and a high-pass above it, with a dead
/// zone around 0.5. Runs on the orbit bus, so it colours that orbit's dry signal
/// and its reverb/delay returns alike — as upstream, where the node sits between
/// the orbit's summing node and its output.
#[derive(Clone)]
pub struct Djf {
    filter: TwoPole,
    sample_rate: f32,
    mode: DjfMode,
    cutoff: f32,
}

impl Djf {
    pub fn new(sample_rate: f32, value: f32) -> Djf {
        let mut d = Djf {
            filter: TwoPole::default(),
            sample_rate,
            mode: DjfMode::Bypass,
            cutoff: 0.0,
        };
        d.set_value(value);
        d
    }

    /// Retune the knob, keeping the filter state so a swept `djf` is continuous.
    pub fn set_value(&mut self, value: f32) {
        let value = value.clamp(0.0, 1.0);
        let (mode, v) = if value > 0.51 {
            (DjfMode::Highpass, (value - 0.5) * 2.0)
        } else if value < 0.49 {
            (DjfMode::Lowpass, value * 2.0)
        } else {
            (DjfMode::Bypass, 1.0)
        };
        self.mode = mode;
        self.cutoff = (v * 11.0).powi(4);
    }

    /// True when the knob sits in the dead zone and the filter is a no-op.
    pub fn is_bypass(&self) -> bool {
        self.mode == DjfMode::Bypass
    }

    pub fn process(&mut self, x: f32) -> f32 {
        match self.mode {
            DjfMode::Bypass => x,
            mode => {
                let lp = self.filter.update(x, self.sample_rate, self.cutoff, 0.1);
                if mode == DjfMode::Lowpass { lp } else { x - lp }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn djf_centre_is_a_bypass() {
        let mut d = Djf::new(44100.0, 0.5);
        assert!(d.is_bypass());
        for x in [-1.0, 0.0, 0.3, 1.0] {
            assert_eq!(d.process(x), x);
        }
    }

    #[test]
    fn djf_sides_select_low_and_high_pass() {
        // A high-frequency input survives the high-pass side and is killed by
        // the low-pass side (whose cutoff collapses towards 0 as value -> 0).
        let sr = 44100.0;
        let tone: Vec<f32> = (0..2000)
            .map(|i| (std::f32::consts::TAU * 8000.0 * i as f32 / sr).sin())
            .collect();
        let peak = |mut f: Djf| {
            tone.iter()
                .map(|&x| f.process(x).abs())
                .skip(1000)
                .fold(0.0f32, f32::max)
        };
        assert!(
            peak(Djf::new(sr, 0.1)) < 0.05,
            "low-pass side should reject 8kHz"
        );
        assert!(
            peak(Djf::new(sr, 0.9)) > 0.5,
            "high-pass side should pass 8kHz"
        );
    }

    /// The mixer rebuilds its convolver whenever the reverb config changes, so
    /// every field has to take part in the comparison — an `eq` that ignores
    /// one leaves the old impulse response in place.
    #[test]
    fn reverb_configs_compare_on_every_field_and_ir_identity() {
        let base = ReverbConfig::default();
        assert_eq!(base, ReverbConfig::default());

        type Vary = (&'static str, fn(&mut ReverbConfig));
        let vary: [Vary; 6] = [
            ("size", |c| c.size += 1.0),
            ("fade", |c| c.fade += 1.0),
            ("lp", |c| c.lp += 1.0),
            ("dim", |c| c.dim += 1.0),
            ("irspeed", |c| c.irspeed += 1.0),
            ("irbegin", |c| c.irbegin += 1.0),
        ];
        for (name, change) in vary {
            let mut other = base.clone();
            change(&mut other);
            assert_ne!(base, other, "{name} should make the configs differ");
        }

        // A loaded impulse response compares by `Arc` identity: the same
        // sample is the same reverb, an equal-but-separate one is not.
        let sample = || {
            std::sync::Arc::new(crate::Sample {
                data: vec![1.0, 0.0],
                sample_rate: 44100.0,
            })
        };
        let a = sample();
        let with_ir = ReverbConfig {
            ir: Some(a.clone()),
            ..base.clone()
        };
        assert_eq!(
            with_ir,
            ReverbConfig {
                ir: Some(a),
                ..base.clone()
            }
        );
        assert_ne!(
            with_ir,
            ReverbConfig {
                ir: Some(sample()),
                ..base.clone()
            }
        );
        assert_ne!(with_ir, base);
    }

    /// `delaysync` is a fraction of a cycle, so the delay time it implies
    /// depends on the tempo — and its default is superdough's 3/16.
    #[test]
    fn delaysync_converts_a_cycle_fraction_to_seconds() {
        let send = |pairs: &[(&str, f64)], cps: f64| {
            let mut m = ValueMap::new();
            for (k, v) in pairs {
                m.insert(k.to_string(), Value::F64(*v));
            }
            OrbitSend::from_controls(&m, cps).delay_cfg.time
        };
        // No delaytime, no delaysync: 3/16 of a cycle, at this tempo.
        assert!((send(&[], 1.0) - 3.0 / 16.0).abs() < 1e-6);
        assert!((send(&[], 0.5) - 3.0 / 8.0).abs() < 1e-6, "half cps, twice as long");
        assert!((send(&[], 2.0) - 3.0 / 32.0).abs() < 1e-6);
        // An explicit delaysync overrides the default.
        assert!((send(&[("delaysync", 0.5)], 2.0) - 0.25).abs() < 1e-6);
        // An explicit delaytime wins outright, tempo-independent.
        assert!((send(&[("delaytime", 0.4), ("delaysync", 0.5)], 2.0) - 0.4).abs() < 1e-6);
    }

    /// The ladder's coefficient goes complex past Nyquist, so `TwoPole` clamps
    /// the cutoff just below it — everything above behaves as that clamp.
    #[test]
    fn the_two_pole_cutoff_is_clamped_below_nyquist() {
        let sr = 44100.0;
        let run = |cutoff: f32| -> Vec<f32> {
            let mut f = TwoPole { s0: 0.0, s1: 0.0 };
            (0..64)
                .map(|i| f.update((i as f32 * 0.3).sin(), sr, cutoff, 0.5))
                .collect()
        };
        let at_limit = run(sr / 2.0 - 1.0);
        for above in [sr / 2.0, sr, sr * 4.0] {
            assert_eq!(run(above), at_limit, "cutoff {above} should clamp");
        }
        assert_ne!(run(1000.0), at_limit);
        // Every output stays finite: an unclamped cutoff produces NaNs.
        assert!(at_limit.iter().all(|x| x.is_finite()));
    }
}

