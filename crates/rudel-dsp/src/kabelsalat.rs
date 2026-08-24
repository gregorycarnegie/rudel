// kabelsalat.rs - the `K(...)` / `worklet` voice: a kabelsalat graph, run as a
// register program.
//
// `rudel-lang` builds and topologically sorts the graph (see
// `rudel-lang/src/kabelsalat.rs`); this module is the back end. Upstream has
// two of those already — `lib/src/lang/js.js` emits JavaScript that superdough
// hands to `new Function` and calls once per sample, and `lang/c.js` emits C —
// so this is a third in the same slot, emitting nothing and interpreting the
// instruction list directly.
//
// Three arrays hold the state, named as upstream names them:
//
//   r  one register per node, in topological order
//   o  the output channels, cleared before every sample
//   s  the *previous* sample's outputs, which `src` reads
//
// `s` is what closes a feedback loop. A cyclic graph — `x => x.delay(.2)` —
// stops the topological sort at the node it has already visited, so the loop
// reads a register from one sample ago. That one-sample delay is the whole
// mechanism, upstream and here.
//
// The ugens are ported from `@kabelsalat/lib` 0.4.1 (`src/ugens.js` and the
// helpers in `src/synth.js`). Unported types fall back to passing their first
// input through, which is what upstream's compiler does for a node type it
// does not recognise (`fallbackType = "thru"`), so a patch using one still
// plays rather than failing.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::voice::VoiceLike;
use rudel_core::Value;
use std::f32::consts::TAU;

/// `ISR` in `ugens.js` — hardcoded to 48kHz upstream, and used by `dust` and
/// `slew` regardless of the context's actual rate. Kept wrong on purpose: it
/// changes their tuning, and parity is the point.
const ISR: f32 = 1.0 / 48000.0;

/// `CLOCK_PPQ`: clock pulses per quarter note.
const CLOCK_PPQ: f32 = 24.0;

/// The longest delay a `delay` node can hold, as upstream's `MAX_DELAY_TIME`.
const MAX_DELAY_TIME: f32 = 10.0;

// ---------------------------------------------------------------------------
// Program

/// One inlet: either a wire to an earlier register, or a literal the compiler
/// filled in for an argument the patch left off.
#[derive(Clone, Copy)]
enum In {
    Reg(usize),
    Const(f32),
}

/// A compiled kabelsalat graph: one instruction per register, in the order
/// they run.
pub struct KabelProgram {
    ops: Vec<Op>,
    ins: Vec<Vec<In>>,
    /// `(register, channel)` — where the graph's audio leaves.
    outs: Vec<(usize, usize)>,
}

impl KabelProgram {
    /// Read the program out of a `worklet` control value. Returns `None` when
    /// the value is not a compiled graph, so a hap carrying nonsense plays as
    /// an ordinary voice rather than failing.
    pub fn from_value(value: &Value) -> Option<KabelProgram> {
        let Value::Map(map) = value else {
            return None;
        };
        let list = |key: &str| match map.get(key) {
            Some(Value::List(items)) => Some(items.clone()),
            _ => None,
        };
        let types = list("types")?;
        let values = list("values")?;
        let raw_ins = list("ins")?;
        if values.len() != types.len() || raw_ins.len() != types.len() {
            return None;
        }

        let ops = types
            .iter()
            .zip(&values)
            .map(|(ty, value)| Op::new(ty.as_str().unwrap_or("thru"), value))
            .collect();
        let ins = raw_ins
            .iter()
            .map(|entry| match entry {
                Value::List(items) => items.iter().map(In::new).collect(),
                _ => Vec::new(),
            })
            .collect();
        let outs = list("outs")?
            .iter()
            .filter_map(|entry| match entry {
                Value::List(pair) if pair.len() == 2 => {
                    let reg = pair[0].as_f64()? as usize;
                    let channel = pair[1].as_f64()? as usize;
                    (reg < types.len()).then_some((reg, channel))
                }
                _ => None,
            })
            .collect();
        Some(KabelProgram { ops, ins, outs })
    }
}

impl In {
    fn new(value: &Value) -> In {
        match value {
            // The compiler distinguishes the two by type: an `Int` indexes a
            // register, an `F64` is a literal default.
            Value::Int(reg) => In::Reg(*reg as usize),
            other => In::Const(other.as_f64().unwrap_or(0.0) as f32),
        }
    }
}

// ---------------------------------------------------------------------------
// Instructions

/// What a register computes. The stateless ones carry nothing; the stateful
/// ones carry the ugen's own state, one instance per register, exactly as
/// upstream builds one class instance per node.
enum Op {
    /// A constant — kabelsalat's `n` node.
    Const(f32),
    /// Seconds since the voice started, upstream's `time`.
    Time,
    /// Pass the first input through. Also the fallback for a node type this
    /// back end has not ported.
    Thru,
    /// The voice this graph is wrapped around, upstream's `audioin`.
    AudioIn,
    /// The hap's own frequency and gate, which upstream splices into the
    /// source text as `sFreq` and `sGate` before compiling.
    Freq,
    Gate,
    /// A value sampled from a pattern written inside `K(...)`, by index.
    Pat(usize),

    // Variadic arithmetic.
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Min,
    Max,
    ArgMin,
    ArgMax,
    Mix,

    // Unary math.
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Log,
    Exp,
    Abs,
    Round,
    Floor,
    Ceil,
    Sign,
    Not,
    Bool,
    MidiNote,

    // Binary and ternary math.
    Pow,
    Greater,
    Lower,
    Xor,
    And,
    Or,
    IfElse,
    Clamp,
    Range,

    /// Read the previous sample's output channel — the far side of a feedback
    /// loop.
    Src,
    /// Add into an output channel, and publish it for `src`.
    Output,

    // Stateful ugens.
    Adsr(AdsrEnv),
    Sine(SineOsc),
    Saw(SawOsc),
    Zaw(Phase),
    Tri(Phase),
    Pulse(Phase),
    Impulse(Impulse),
    Clock(Phase),
    ClockDiv(ClockDiv),
    Noise(Noise),
    Dust(Dust),
    Brown(Brown),
    Pink(Pink),
    Filter(TwoPole),
    Bpf(TwoPole),
    Delay(Delay),
    Hold(Hold),
    Distort,
    Fold,
    Lag(f32),
    Slew(f32),
    Slide(f32),
    Seq(Sequence),
    Pick,
    Remap,
    Clip,
    Trig(bool),
    Lcg(LcgNoise),
    Qf(Biquad),
    /// `bytebeat`, `floatbeat` and `raw`: a JavaScript expression per sample.
    Coded(Coded),
}

impl Op {
    fn new(kind: &str, value: &Value) -> Op {
        let num = || value.as_f64().unwrap_or(0.0) as f32;
        let text = || value.as_str().unwrap_or("0").to_string();
        match kind {
            "n" => Op::Const(num()),
            "time" => Op::Time,
            "audioin" => Op::AudioIn,
            "sfreq" => Op::Freq,
            "sgate" => Op::Gate,
            "pat" => Op::Pat(num() as usize),

            "add" => Op::Add,
            "sub" => Op::Sub,
            "mul" => Op::Mul,
            "div" => Op::Div,
            "mod" => Op::Mod,
            "min" => Op::Min,
            "max" => Op::Max,
            "argmin" => Op::ArgMin,
            "argmax" => Op::ArgMax,
            "mix" | "poly" => Op::Mix,

            "sin" => Op::Sin,
            "cos" => Op::Cos,
            "tan" => Op::Tan,
            "asin" => Op::Asin,
            "acos" => Op::Acos,
            "atan" => Op::Atan,
            "log" => Op::Log,
            "exp" => Op::Exp,
            "abs" => Op::Abs,
            "round" => Op::Round,
            "floor" => Op::Floor,
            "ceil" => Op::Ceil,
            "sign" => Op::Sign,
            "not" => Op::Not,
            "bool" => Op::Bool,
            "midinote" => Op::MidiNote,

            "pow" => Op::Pow,
            "greater" => Op::Greater,
            "lower" => Op::Lower,
            "xor" => Op::Xor,
            "and" => Op::And,
            "or" => Op::Or,
            "ifelse" => Op::IfElse,
            "clamp" => Op::Clamp,
            "range" => Op::Range,

            "src" => Op::Src,
            "output" => Op::Output,

            "adsr" => Op::Adsr(AdsrEnv::default()),
            "sine" => Op::Sine(SineOsc::default()),
            "saw" => Op::Saw(SawOsc::default()),
            "zaw" => Op::Zaw(Phase::default()),
            "tri" => Op::Tri(Phase::default()),
            "pulse" => Op::Pulse(Phase::default()),
            "impulse" => Op::Impulse(Impulse::default()),
            "clock" => Op::Clock(Phase::default()),
            "clockdiv" => Op::ClockDiv(ClockDiv::default()),
            "noise" => Op::Noise(Noise::default()),
            "dust" => Op::Dust(Dust::default()),
            "brown" => Op::Brown(Brown::default()),
            "pink" => Op::Pink(Pink::default()),
            "filter" => Op::Filter(TwoPole::default()),
            "bpf" => Op::Bpf(TwoPole::default()),
            "delay" => Op::Delay(Delay::default()),
            "hold" => Op::Hold(Hold::default()),
            "distort" => Op::Distort,
            "fold" => Op::Fold,
            "lag" => Op::Lag(0.0),
            "slew" => Op::Slew(0.0),
            "slide" => Op::Slide(0.0),
            "seq" => Op::Seq(Sequence::default()),
            "pick" => Op::Pick,
            "remap" => Op::Remap,
            "clip" => Op::Clip,
            "trig" => Op::Trig(false),
            // Upstream seeds this from a counter of every `lcgnoise` ever
            // constructed on the page; per graph is the reproducible reading.
            "lcgnoise" => Op::Lcg(LcgNoise::new(0)),
            "qf" => Op::Qf(Biquad::default()),
            "bytebeat" => Op::Coded(Coded::new(&text(), CodedKind::Byte)),
            "raw" => Op::Coded(Coded::new(&text(), CodedKind::Raw)),
            _ => Op::Thru,
        }
    }
}

// ---------------------------------------------------------------------------
// Ugen state
//
// One struct per stateful node type, holding what upstream's class holds. The
// `update` bodies are `ugens.js`'s, with the helpers they call folded in from
// `synth.js`.

/// xorshift32, standing in for the `Math.random()` upstream's noise sources
/// call. Seeded per node type, so a patch sounds the same every time it is
/// played — upstream's does not, which is what makes this the one place a
/// golden test can pin.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32
    }

    /// `Math.random() * 2 - 1`.
    fn bipolar(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

/// A bare phase accumulator, shared by the oscillators that are just
/// `phase += dt` and a waveshape: `zaw`, `tri`, `pulse` and `clock`.
///
/// The accumulator is `f64` because upstream's is a JavaScript number, and it
/// is never wrapped — so by the end of a long note it holds a large value whose
/// fractional part is the phase. In `f32` that fraction loses a bit a second
/// and the pitch drifts.
#[derive(Default)]
struct Phase(f64);

impl Phase {
    /// Advance and return the position within the cycle. Upstream advances
    /// *before* reading.
    fn step(&mut self, freq: f32, dt: f32) -> f32 {
        self.0 += f64::from(dt) * f64::from(freq);
        (self.0 % 1.0) as f32
    }
}

/// `SineOsc`: a phase accumulator with a hard-sync input and a phase offset.
#[derive(Default)]
struct SineOsc {
    phase: f64,
    sync_high: bool,
}

impl SineOsc {
    fn update(&mut self, freq: f32, sync: f32, offset: f32, dt: f32) -> f32 {
        if !self.sync_high && sync > 0.0 {
            self.phase = 0.0;
        }
        self.sync_high = sync > 0.0;
        // The offset applies to the phase *before* it advances, so the first
        // sample of a synced oscillator is `sin(offset)`.
        let pos = (self.phase + f64::from(offset)) % 1.0;
        self.phase += f64::from(dt) * f64::from(freq);
        (pos * f64::from(TAU)).sin() as f32
    }
}

/// `SawOsc`: a band-limited saw, polyBLEP-corrected at the discontinuity.
/// Upstream starts it at a random phase; a fixed start keeps a patch
/// reproducible, at the cost of every saw in a graph beginning in step.
#[derive(Default)]
struct SawOsc {
    phase: f64,
}

impl SawOsc {
    fn update(&mut self, freq: f32, sample_rate: f32) -> f32 {
        let dt = f64::from(freq) / f64::from(sample_rate);
        let s = 2.0 * self.phase - 1.0 - poly_blep(self.phase, dt);
        self.phase += dt;
        // This one *is* wrapped upstream, which is why its accumulator stays
        // small and its polyBLEP window stays meaningful.
        if self.phase > 1.0 {
            self.phase -= 1.0;
        }
        s as f32
    }
}

/// `polyBlep` from `utils.js`: the correction that rounds off a saw's jump.
fn poly_blep(phase: f64, dt: f64) -> f64 {
    if phase < dt {
        let p = phase / dt;
        p + p - p * p - 1.0
    } else if phase > 1.0 - dt {
        let p = (phase - 1.0) / dt;
        p * p + p + p + 1.0
    } else {
        0.0
    }
}

/// `ImpulseOsc`: one sample high per cycle. Starts at phase 1 so it fires
/// immediately.
struct Impulse {
    phase: f64,
}

impl Default for Impulse {
    fn default() -> Impulse {
        Impulse { phase: 1.0 }
    }
}

impl Impulse {
    fn update(&mut self, freq: f32, dt: f32) -> f32 {
        self.phase += f64::from(dt) * f64::from(freq);
        let v = if self.phase >= 1.0 { 1.0 } else { 0.0 };
        self.phase %= 1.0;
        v
    }
}

/// `ClockDiv`: flips its output every `factor` edges of the input clock.
struct ClockDiv {
    in_high: bool,
    out_high: bool,
    count: f32,
}

impl Default for ClockDiv {
    fn default() -> ClockDiv {
        // Both start high so the divider, like the clock, triggers at once.
        ClockDiv {
            in_high: true,
            out_high: true,
            count: 0.0,
        }
    }
}

impl ClockDiv {
    fn update(&mut self, clock: f32, factor: f32) -> f32 {
        let high = clock > 0.0;
        if self.in_high != high {
            // Both edges count, rising and falling.
            self.count += 1.0;
            if self.count >= factor {
                self.count = 0.0;
                self.out_high = !self.out_high;
            }
        }
        self.in_high = high;
        if self.out_high { 1.0 } else { -1.0 }
    }
}

/// `NoiseOsc`: white noise that only redraws when its input is truthy, so it
/// doubles as a sample-and-hold noise source.
struct Noise {
    rng: Rng,
    value: f32,
}

impl Default for Noise {
    fn default() -> Noise {
        let mut rng = Rng(0x2545_f491);
        let value = rng.bipolar();
        Noise { rng, value }
    }
}

impl Noise {
    fn update(&mut self, next: f32) -> f32 {
        if next == 0.0 {
            return self.value;
        }
        self.value = self.rng.bipolar();
        self.value
    }
}

/// `PinkNoise`: Paul Kellett's filter bank over white noise.
struct Pink {
    rng: Rng,
    b: [f32; 7],
}

impl Default for Pink {
    fn default() -> Pink {
        Pink {
            rng: Rng(0x9e37_79b9),
            b: [0.0; 7],
        }
    }
}

impl Pink {
    fn update(&mut self) -> f32 {
        let white = self.rng.bipolar();
        self.b[0] = 0.99886 * self.b[0] + white * 0.0555179;
        self.b[1] = 0.99332 * self.b[1] + white * 0.0750759;
        self.b[2] = 0.969 * self.b[2] + white * 0.153852;
        self.b[3] = 0.8665 * self.b[3] + white * 0.3104856;
        self.b[4] = 0.55 * self.b[4] + white * 0.5329522;
        self.b[5] = -0.7616 * self.b[5] - white * 0.016898;
        let pink: f32 = self.b.iter().sum::<f32>() + white * 0.5362;
        self.b[6] = white * 0.115926;
        pink * 0.11
    }
}

/// `synth.js`'s `TwoPoleFilter`: one recurrence giving a band-pass in `s0` and
/// a low-pass in `s1`. superdough carries its own version of this (see
/// `bus.rs`), but takes cutoff in Hz where kabelsalat takes it normalised.
#[derive(Default)]
struct TwoPole {
    s0: f32,
    s1: f32,
}

impl TwoPole {
    fn apply(&mut self, s: f32, cutoff: f32, resonance: f32) {
        // Out-of-bound values can produce NaNs (upstream's comment).
        let cutoff = cutoff.min(1.0);
        let resonance = resonance.max(0.0);
        let c = 0.5f32.powf((1.0 - cutoff) / 0.125);
        let r = 0.5f32.powf((resonance + 0.125) / 0.125);
        let mrc = 1.0 - r * c;
        self.s0 = mrc * self.s0 - c * self.s1 + c * s;
        self.s1 = mrc * self.s1 + c * self.s0;
    }
}

/// `synth.js`'s `Delay`: a fixed 10-second line, read at a distance the time
/// input sets. The buffer is allocated on first use, so a graph carrying an
/// unused delay node does not cost ten seconds of audio for nothing.
#[derive(Default)]
struct Delay {
    buffer: Vec<f32>,
    write: usize,
    read: usize,
}

impl Delay {
    fn update(&mut self, input: f32, time: f32, sample_rate: f32) -> f32 {
        if self.buffer.is_empty() {
            self.buffer = vec![0.0; (MAX_DELAY_TIME * sample_rate) as usize];
        }
        self.write = (self.write + 1) % self.buffer.len();
        self.buffer[self.write] = input;
        let samples = ((sample_rate * time).max(0.0) as usize).min(self.buffer.len() - 1);
        self.read = (self.write + self.buffer.len() - samples) % self.buffer.len();
        self.buffer[self.read]
    }
}

/// `Hold`: sample and hold, latching on the rising edge of its trigger.
#[derive(Default)]
struct Hold {
    value: f32,
    trig_high: bool,
}

impl Hold {
    fn update(&mut self, input: f32, trig: f32) -> f32 {
        if !self.trig_high && trig > 0.0 {
            self.value = input;
        }
        self.trig_high = trig > 0.0;
        self.value
    }
}

/// `Sequence`: steps through its inputs on each rising clock edge.
struct Sequence {
    clock_high: bool,
    step: usize,
}

impl Default for Sequence {
    fn default() -> Sequence {
        Sequence {
            clock_high: true,
            step: 0,
        }
    }
}

impl Sequence {
    fn update(&mut self, clock: f32, steps: &[f32]) -> f32 {
        if steps.is_empty() {
            return 0.0;
        }
        if !self.clock_high && clock > 0.0 {
            self.step = (self.step + 1) % steps.len();
            self.clock_high = true;
            // Upstream returns zero for the first sample of a new step, so a
            // gate driven from the sequence retriggers.
            return 0.0;
        }
        self.clock_high = clock > 0.0;
        steps[self.step]
    }
}

/// The state machine behind `adsr`, from `synth.js`'s `ADSREnv`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum EnvState {
    #[default]
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Default)]
struct AdsrEnv {
    state: EnvState,
    start_time: f32,
    start_value: f32,
}

impl AdsrEnv {
    fn eval(
        &mut self,
        now: f32,
        gate: f32,
        attack: f32,
        decay: f32,
        sustain: f32,
        release: f32,
    ) -> f32 {
        match self.state {
            EnvState::Off => {
                if gate > 0.0 {
                    self.state = EnvState::Attack;
                    self.start_time = now;
                    self.start_value = 0.0;
                }
                0.0
            }
            EnvState::Attack => {
                let time = now - self.start_time;
                if time > attack {
                    self.state = EnvState::Decay;
                    self.start_time = now;
                    return 1.0;
                }
                lerp(time / attack, self.start_value, 1.0)
            }
            EnvState::Decay => {
                let time = now - self.start_time;
                let value = lerp(time / decay, 1.0, sustain);
                if gate <= 0.0 {
                    self.state = EnvState::Release;
                    self.start_time = now;
                    self.start_value = value;
                    return value;
                }
                if time > decay {
                    self.state = EnvState::Sustain;
                    self.start_time = now;
                    return sustain;
                }
                value
            }
            EnvState::Sustain => {
                if gate <= 0.0 {
                    self.state = EnvState::Release;
                    self.start_time = now;
                    self.start_value = sustain;
                }
                sustain
            }
            EnvState::Release => {
                let time = now - self.start_time;
                if time > release {
                    self.state = EnvState::Off;
                    return 0.0;
                }
                let value = lerp(time / release, self.start_value, 0.0);
                if gate > 0.0 {
                    self.state = EnvState::Attack;
                    self.start_time = now;
                    self.start_value = value;
                }
                value
            }
        }
    }
}

/// `synth.js`'s `lerp`, which clamps at the top but not the bottom.
fn lerp(x: f32, y0: f32, y1: f32) -> f32 {
    if x >= 1.0 {
        return y1;
    }
    y0 + x * (y1 - y0)
}

/// `utils.js`'s `invLerp`, clamped at both ends.
fn inv_lerp(x: f32, y0: f32, y1: f32) -> f32 {
    if x <= y0 {
        return 0.0;
    }
    if x >= y1 {
        return 1.0;
    }
    if y1 == y0 {
        return 0.0;
    }
    (x - y0) / (y1 - y0)
}

/// `synth.js`'s overdrive curve.
fn distort(x: f32, amount: f32) -> f32 {
    let amount = amount.clamp(0.0, 1.0) - 0.01;
    let k = (2.0 * amount) / (1.0 - amount);
    ((1.0 + k) * x) / (1.0 + k * x.abs())
}

/// `Fold`: the wavefolder. `rate` 0 leaves the input alone.
fn fold(input: f32, rate: f32) -> f32 {
    let rate = rate.max(0.0) + 1.0;
    let x = input * rate;
    4.0 * ((0.25 * x + 0.25 - js_round(0.25 * x + 0.25)).abs() - 0.25)
}

/// JavaScript's `Math.round`: halves go *up*, not away from zero, which is
/// where Rust's `f32::round` differs.
fn js_round(x: f32) -> f32 {
    (x + 0.5).floor()
}

/// `DustOsc`: random impulses at roughly `density` per second.
struct Dust {
    rng: Rng,
}

impl Default for Dust {
    fn default() -> Dust {
        Dust {
            rng: Rng(0x1234_5679),
        }
    }
}

impl Dust {
    fn update(&mut self, density: f32) -> f32 {
        // `ISR` and not the real sample period, as upstream.
        if self.rng.next() < density * ISR {
            self.rng.next()
        } else {
            0.0
        }
    }
}

/// `BrownNoiseOsc`: a leaky integrator over white noise.
struct Brown {
    rng: Rng,
    out: f32,
}

impl Default for Brown {
    fn default() -> Brown {
        Brown {
            rng: Rng(0x8bad_f00d),
            out: 0.0,
        }
    }
}

impl Brown {
    fn update(&mut self) -> f32 {
        let white = self.rng.bipolar();
        self.out = (self.out + 0.02 * white) / 1.02;
        self.out
    }
}

/// `LcgNoise`: a linear congruential generator, "modeled after melimelo
/// noise". Unlike the other noise sources this one is deterministic upstream
/// too, so it can be checked sample for sample.
struct LcgNoise {
    start_seed: u32,
    state: u32,
    value: f32,
}

impl LcgNoise {
    /// `NOISE_SEED * (channel + 1)`. Upstream counts channels in a module
    /// global that is never reset, so the seed depends on how many `lcgnoise`
    /// nodes have ever been constructed in the page; here it is per node, and
    /// the first one agrees.
    fn new(channel: u32) -> LcgNoise {
        let start_seed = 340u32.wrapping_mul(channel + 1);
        LcgNoise {
            start_seed,
            state: start_seed,
            value: 0.0,
        }
    }

    fn update(&mut self, next: f32, reset: f32) -> f32 {
        if next == 0.0 {
            return self.value;
        }
        if reset != 0.0 {
            self.state = self.start_seed;
        }
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let y = (self.state & 0x00ff_ffff) as f32 / (1u32 << 24) as f32;
        self.value = y * 2.0 - 1.0;
        self.value
    }
}

/// Which curve `qf` computes, by upstream's numbering.
fn biquad_coefficients(kind: i32, freq: f32, q: f32, gain: f32, sample_rate: f32) -> [f32; 5] {
    let omega = TAU * freq / sample_rate;
    let (sin_omega, cos_omega) = (omega.sin(), omega.cos());
    // Upstream reads `Q` in decibels — "like web audio?" — before using it.
    let q = 10.0f32.powf(q / 20.0);
    let alpha = sin_omega / (2.0 * q);
    // Shelving and peaking share the amplitude term.
    let a = 10.0f32.powf(gain / 40.0);
    let (b0, b1, b2, a0, a1, a2) = match kind {
        1 => {
            let b0 = (1.0 + cos_omega) / 2.0;
            (
                b0,
                -(1.0 + cos_omega),
                b0,
                1.0 + alpha,
                -2.0 * cos_omega,
                1.0 - alpha,
            )
        }
        2 => {
            let b0 = sin_omega / 2.0;
            (b0, 0.0, -b0, 1.0 + alpha, -2.0 * cos_omega, 1.0 - alpha)
        }
        3 => (
            1.0,
            -2.0 * cos_omega,
            1.0,
            1.0 + alpha,
            -2.0 * cos_omega,
            1.0 - alpha,
        ),
        4 => (
            1.0 - alpha,
            -2.0 * cos_omega,
            1.0 + alpha,
            1.0 + alpha,
            -2.0 * cos_omega,
            1.0 - alpha,
        ),
        5 => (
            1.0 + alpha * a,
            -2.0 * cos_omega,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos_omega,
            1.0 - alpha / a,
        ),
        6 | 7 => {
            let sqrt2aa = 2.0 * a.sqrt() * alpha;
            let am1w0 = (a - 1.0) * cos_omega;
            let ap1w0 = (a + 1.0) * cos_omega;
            if kind == 6 {
                (
                    a * (a + 1.0 - am1w0 + sqrt2aa),
                    2.0 * a * (a - 1.0 - ap1w0),
                    a * (a + 1.0 - am1w0 - sqrt2aa),
                    a + 1.0 + am1w0 + sqrt2aa,
                    -2.0 * (a - 1.0 + ap1w0),
                    a + 1.0 + am1w0 - sqrt2aa,
                )
            } else {
                (
                    a * (a + 1.0 + am1w0 + sqrt2aa),
                    -2.0 * a * (a - 1.0 + ap1w0),
                    a * (a + 1.0 + am1w0 - sqrt2aa),
                    a + 1.0 - am1w0 + sqrt2aa,
                    2.0 * (a - 1.0 - ap1w0),
                    a + 1.0 - am1w0 - sqrt2aa,
                )
            }
        }
        // 0, and anything upstream's `if` chain leaves untouched, is a
        // low-pass — including a type it does not recognise, which keeps the
        // coefficients from the previous sample. Recomputing them as a
        // low-pass is the closest a stateless mapping gets.
        _ => {
            let b1 = 1.0 - cos_omega;
            (
                b1 / 2.0,
                b1,
                b1 / 2.0,
                1.0 + alpha,
                -2.0 * cos_omega,
                1.0 - alpha,
            )
        }
    };
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

/// `BiquadFilter` (`qf`): the Audio EQ Cookbook forms, coefficients recomputed
/// every sample so every input can be modulated.
#[derive(Default)]
struct Biquad {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn update(
        &mut self,
        input: f32,
        kind: f32,
        freq: f32,
        q: f32,
        gain: f32,
        sample_rate: f32,
    ) -> f32 {
        let [b0, b1, b2, a1, a2] = biquad_coefficients(kind as i32, freq, q, gain, sample_rate);
        let output = b0 * input + b1 * self.x1 + b2 * self.x2 - a1 * self.y1 - a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

/// `bytebeat`, `floatbeat` and `raw`: nodes whose body is a JavaScript
/// expression evaluated per sample.
///
/// This is the one place a kabelsalat graph really does run arbitrary code, and
/// the reason `dough` could not be ported. It works here because rudel already
/// carries an evaluator for exactly this dialect — the integer-expression
/// parser behind the `bytebeat` synth — so the three nodes reuse it rather than
/// needing a JavaScript engine.
struct Coded {
    expr: crate::bytebeat::ByteBeatExpr,
    kind: CodedKind,
}

/// How a coded node's result is read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CodedKind {
    /// `bytebeat`: 0..255, folded to -1..1 by `(x & 255) / 127.5 - 1`.
    ///
    /// `floatbeat` lands here too. Upstream builds it as a node of type
    /// `"bytebeat"` (`new Node("bytebeat", code)`) while registering its
    /// float-reading `compile` under the name `floatbeat`, and the compiler
    /// looks the schema up by *type* — so the float reading is never reached
    /// and a floatbeat is heard as a bytebeat. Mirrored, because parity is the
    /// point; if upstream fixes it, `floatbeat` needs a type of its own.
    Byte,
    /// `raw`: -1..1, with the node's input available as `$input`.
    Raw,
}

impl Coded {
    fn new(source: &str, kind: CodedKind) -> Coded {
        // `$` is not an operator in the bytebeat dialect, so `raw`'s input
        // variable is spelled as a plain identifier before parsing.
        let source = match kind {
            CodedKind::Raw => source.replace("$input", "input"),
            _ => source.to_string(),
        };
        Coded {
            expr: crate::bytebeat::ByteBeatExpr::parse(&source),
            kind,
        }
    }

    fn update(&mut self, t: f64, input: f32, time: f32) -> f32 {
        let value = self
            .expr
            .eval_with(t, &[("input", f64::from(input)), ("time", f64::from(time))]);
        match self.kind {
            CodedKind::Byte => ((value as i64 & 255) as f32) / 127.5 - 1.0,
            CodedKind::Raw => value as f32,
        }
    }
}

// ---------------------------------------------------------------------------
// The voice

/// How many output channels the graph can address, as upstream's
/// `GenericProcessor`. Only the first two reach the speakers; the rest exist so
/// a patch can route a signal back to itself through `src`.
const CHANNELS: usize = 16;

/// A kabelsalat graph, playing as one voice.
///
/// The graph wraps whatever the hap would otherwise have played, which is how
/// upstream wires it (`chain.connect(workletNode)`): `audioin()` reads that
/// voice, so `K(...)` is an effect when the patch uses it and a synth when it
/// does not.
pub struct KabelVoice {
    program: KabelProgram,
    /// The voice under the graph, read by `audioin`.
    inner: Box<dyn VoiceLike>,
    /// `r`: one slot per instruction.
    regs: Vec<f32>,
    /// `o`: output channels, cleared every sample.
    outs: [f32; CHANNELS],
    /// `s`: the previous sample's outputs, which `src` reads.
    srcs: [f32; CHANNELS],
    /// Values of the patterns written inside `K(...)`, sampled once per hap.
    pats: Vec<f32>,
    /// The hap's frequency, upstream's `sFreq`.
    freq: f32,
    sample_rate: f32,
    /// `playPos`: seconds since this voice started.
    t: f32,
    /// When the gate falls — the hap's own end, before any release tail.
    gate_end: f32,
    end: f32,
}

impl KabelVoice {
    pub fn new(
        program: KabelProgram,
        inner: Box<dyn VoiceLike>,
        sample_rate: f32,
        freq: f32,
        pats: Vec<f32>,
        gate_end: f32,
        end: f32,
    ) -> KabelVoice {
        let regs = vec![0.0; program.ops.len()];
        KabelVoice {
            program,
            inner,
            regs,
            outs: [0.0; CHANNELS],
            srcs: [0.0; CHANNELS],
            pats,
            freq,
            sample_rate,
            t: 0.0,
            gate_end,
            end,
        }
    }

    /// Read one inlet.
    fn arg(&self, ins: &[In], index: usize) -> f32 {
        match ins.get(index) {
            Some(In::Reg(reg)) => self.regs[*reg],
            Some(In::Const(value)) => *value,
            None => 0.0,
        }
    }
}

/// One inlet of an already-resolved argument list; a missing one is `0`, as
/// reading past the end of `vars` is in the generated JavaScript.
fn at(values: &[f32], index: usize) -> f32 {
    values.get(index).copied().unwrap_or(0.0)
}

/// One inlet with the ugen's own default, for the nodes whose compile step
/// passes `vars` through and leaves the defaults to the class.
fn at2(values: &[f32], index: usize, default: f32) -> f32 {
    values.get(index).copied().unwrap_or(default)
}

/// Fold a variadic node's inputs, as upstream's `vars.join(" op ")` does — and
/// with the same answer for no inputs at all, which is `0`.
fn fold_args(values: &[f32], f: impl Fn(f32, f32) -> f32) -> f32 {
    let mut iter = values.iter().copied();
    let Some(first) = iter.next() else {
        return 0.0;
    };
    iter.fold(first, f)
}

impl VoiceLike for KabelVoice {
    fn tick(&mut self) -> (f32, f32) {
        let (in_l, in_r) = self.inner.tick();
        // The graph has one audio input where the browser has one channel;
        // upstream's `inputs[0][0]` is the left one, so a stereo voice under a
        // `K(...)` is heard in mono by `audioin`.
        let input = 0.5 * (in_l + in_r);
        let dt = 1.0 / self.sample_rate;
        let gate = if self.t < self.gate_end { 1.0 } else { 0.0 };

        self.outs = [0.0; CHANNELS];

        // The instruction list is already topologically sorted, so one pass in
        // order resolves every wire but the feedback ones, which read `srcs`.
        for i in 0..self.program.ops.len() {
            // Split the borrow: the op is mutated in place while the registers
            // it reads stay available.
            let ins = std::mem::take(&mut self.program.ins[i]);
            let a = self.arg(&ins, 0);
            let b = self.arg(&ins, 1);
            let c = self.arg(&ins, 2);
            let all: Vec<f32> = (0..ins.len()).map(|k| self.arg(&ins, k)).collect();
            let now = self.t;
            let rate = self.sample_rate;

            let value = match &mut self.program.ops[i] {
                Op::Const(v) => *v,
                Op::Time => now,
                Op::Thru => a,
                Op::AudioIn => input,
                Op::Freq => self.freq,
                Op::Gate => gate,
                Op::Pat(index) => self.pats.get(*index).copied().unwrap_or(0.0),

                Op::Add => fold_args(&all, |x, y| x + y),
                Op::Sub => fold_args(&all, |x, y| x - y),
                Op::Mul => fold_args(&all, |x, y| x * y),
                Op::Div => fold_args(&all, |x, y| x / y),
                Op::Mod => fold_args(&all, |x, y| x % y),
                Op::Min => fold_args(&all, f32::min),
                Op::Max => fold_args(&all, f32::max),
                Op::Mix => fold_args(&all, |x, y| x + y),
                // The index of the smallest (largest) input, which upstream
                // gets by reducing a list of (value, index) pairs.
                Op::ArgMin => arg_extreme(&all, |x, y| x < y),
                Op::ArgMax => arg_extreme(&all, |x, y| x > y),

                Op::Sin => a.sin(),
                Op::Cos => a.cos(),
                Op::Tan => a.tan(),
                Op::Asin => a.asin(),
                Op::Acos => a.acos(),
                Op::Atan => a.atan(),
                // `Math.log` is the natural log, whatever the docs say.
                Op::Log => a.ln(),
                Op::Exp => a.exp(),
                Op::Abs => a.abs(),
                Op::Round => js_round(a),
                Op::Floor => a.floor(),
                Op::Ceil => a.ceil(),
                Op::Sign => js_sign(a),
                Op::Not => f32::from(a == 0.0),
                Op::Bool => f32::from(a != 0.0),
                Op::MidiNote => 2.0f32.powf((a - 69.0) / 12.0) * 440.0,

                Op::Pow => a.powf(b),
                Op::Greater => f32::from(a > b),
                Op::Lower => f32::from(a < b),
                Op::Xor => f32::from(a != b),
                // JavaScript truthiness, so only exact zero is false.
                Op::And => f32::from(a != 0.0 && b != 0.0),
                Op::Or => f32::from(a != 0.0 || b != 0.0),
                // Upstream tests `control === 1`, not truthiness.
                Op::IfElse => {
                    if a == 1.0 {
                        b
                    } else {
                        c
                    }
                }
                Op::Clamp => a.max(b.min(c)).min(b.max(c)),
                Op::Range => {
                    let unipolar = (a + 1.0) * 0.5;
                    // A fourth input is a curve, applied before the scaling.
                    let shaped = match all.len() {
                        0..=3 => unipolar,
                        _ => unipolar.powf(at(&all, 3)),
                    };
                    shaped * (c - b) + b
                }

                Op::Src => self.srcs[(a as usize) % CHANNELS],
                // Outputs are applied after the loop, from `program.outs`.
                Op::Output => a,

                Op::Adsr(env) => env.eval(now, a, at(&all, 1), c, at(&all, 3), at(&all, 4)),
                Op::Sine(osc) => osc.update(a, b, c, dt),
                Op::Saw(osc) => osc.update(a, rate),
                Op::Zaw(phase) => phase.step(a, dt) * 2.0 - 1.0,
                Op::Tri(phase) => {
                    let pos = phase.step(a, dt);
                    let norm = if pos < 0.5 {
                        2.0 * pos
                    } else {
                        1.0 - 2.0 * (pos - 0.5)
                    };
                    norm * 2.0 - 1.0
                }
                Op::Pulse(phase) => {
                    if phase.step(a, dt) < b {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Op::Impulse(osc) => osc.update(a, dt),
                // The clock starts high so it triggers straight away.
                Op::Clock(phase) => {
                    if phase.step(CLOCK_PPQ * a / 60.0, dt) < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Op::ClockDiv(div) => div.update(a, b),
                Op::Noise(noise) => noise.update(a),
                Op::Dust(dust) => dust.update(a),
                Op::Brown(brown) => brown.update(),
                Op::Pink(pink) => pink.update(),
                Op::Filter(filter) => {
                    filter.apply(a, b, c);
                    filter.s1
                }
                Op::Bpf(filter) => {
                    filter.apply(a, b, c);
                    filter.s0
                }
                Op::Delay(delay) => delay.update(a, b, rate),
                Op::Hold(hold) => hold.update(a, b),
                Op::Distort => distort(a, b),
                Op::Fold => fold(a, b),
                Op::Lag(state) => {
                    // 60dB per second, near enough (upstream's comment).
                    let rate = (b * 4410.0).max(1.0);
                    *state += (1.0 / rate) * (a - *state);
                    *state
                }
                Op::Slew(last) => {
                    let up = b * ISR;
                    let down = c * ISR;
                    *last += (a - *last).clamp(-down, up);
                    *last
                }
                Op::Slide(state) => {
                    let rate = (b * 1000.0).max(1.0);
                    *state += (1.0 / rate) * (a - *state);
                    *state
                }
                // The first input is the clock; the rest are the steps.
                Op::Seq(seq) => seq.update(a, &all[1..]),
                Op::Pick => pick(a, &all[1..]),
                Op::Remap => {
                    let norm = inv_lerp(a, b, c);
                    lerp(norm, at(&all, 3), at(&all, 4))
                }
                Op::Clip => a.max(b).min(c),
                Op::Lcg(noise) => noise.update(a, b),
                // `qf`'s defaults live in the ugen upstream, not the compiler,
                // because its compile step spreads `...vars` untouched.
                Op::Qf(filter) => filter.update(
                    a,
                    at2(&all, 1, 0.0),
                    at2(&all, 2, 500.0),
                    at2(&all, 3, 1.0),
                    at2(&all, 4, 1.0),
                    rate,
                ),
                // `bytebeat`'s `t` is the node's own input, not the sample
                // counter — upstream generates `let t = <input>` ahead of the
                // expression. `raw` reads the same input as `$input`, plus the
                // elapsed time the graph's `time` node also reads.
                Op::Coded(coded) => coded.update(f64::from(a), a, now),
                Op::Trig(high) => {
                    if !*high && a > 0.0 {
                        *high = true;
                        1.0
                    } else {
                        if *high && a <= 0.0 {
                            *high = false;
                        }
                        0.0
                    }
                }
            };
            self.regs[i] = value;
            self.program.ins[i] = ins;
        }

        // Sum the graph's roots into their channels, then publish them for the
        // next sample's `src` reads.
        for &(reg, channel) in &self.program.outs {
            let channel = channel % CHANNELS;
            self.outs[channel] += self.regs[reg];
            self.srcs[channel] = self.outs[channel];
        }

        self.t += dt;
        (self.outs[0], self.outs[1])
    }

    fn set_bus_input(&mut self, bus: i32, left: &[f32], right: &[f32]) {
        self.inner.set_bus_input(bus, left, right);
    }

    fn is_done(&self) -> bool {
        self.t >= self.end
    }
}

/// `argmin`/`argmax`: the index of the winning input.
///
/// Upstream reduces a list of `(value, index)` pairs left to right with
/// `a.value < b.value ? a : b`, which yields `b` when the two are equal — so a
/// tie keeps the *later* index, not the earlier one. Comparing the standing
/// best against each candidate and replacing unless it strictly wins gives the
/// same answer.
fn arg_extreme(values: &[f32], better: impl Fn(f32, f32) -> bool) -> f32 {
    let mut best = 0;
    for (i, &v) in values.iter().enumerate().skip(1) {
        if !better(values[best], v) {
            best = i;
        }
    }
    best as f32
}

/// `Pick`: index into the inputs, wrapping, and tolerating a negative index —
/// which is the whole reason upstream does the modulo twice.
fn pick(index: f32, inputs: &[f32]) -> f32 {
    if inputs.is_empty() {
        return 0.0;
    }
    let len = inputs.len() as f32;
    let i = (index % len) + len;
    inputs[(i.floor() as usize) % inputs.len()]
}

/// `Math.sign`, which gives 0 for 0 where Rust's `signum` gives 1.
fn js_sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}
#[cfg(test)]
mod tests;
