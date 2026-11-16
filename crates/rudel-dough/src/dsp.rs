// dsp.rs - dough's building blocks: its approximate maths, oscillators,
// envelope, filters, effects and reverb, ported from dough.h (dough-synth
// 0.4.1, the version the browser comparison pins).
//
// The arithmetic follows C's promotions exactly. Where dough.h mixes a `float`
// with a `double` literal (`0.99886 * b0`), C computes in double and rounds
// once on assignment; the port writes that out as `(0.99886 * b0 as f64) as
// f32`. A `float`-only expression stays `f32`, in the same order. That is what
// keeps the output bit-identical to the C, and so to the browser's WebAssembly
// build: tests/wasm_golden.rs checks it frame for frame.
// SPDX-License-Identifier: AGPL-3.0-or-later

/// `fPI`, `(float)M_PI`.
pub(crate) const FPI: f32 = std::f32::consts::PI;

/// The engine-wide rates every node reads (`engine.sr`, `engine.isr`,
/// `engine.lag_unit`).
#[derive(Clone, Copy)]
pub(crate) struct Rates {
    pub(crate) sr: f32,
    pub(crate) isr: f32,
    pub(crate) lag_unit: i32,
    /// `1 / engine->sr` in float, the step a voice's clock advances by. dough
    /// divides every sample; the quotient is the same each time.
    pub(crate) time_step: f32,
}

// --- maths ------------------------------------------------------------------

/// `floorf`, exactly, in a few plain float operations. The x86-64 baseline
/// has no rounding instruction, so `f32::floor` calls the C runtime's
/// `floorf`, and dough floors several times per voice per sample.
///
/// Below 2^23 in magnitude, adding and taking away 2^23 (taking and adding it
/// for a negative `x`, so the sum keeps a whole-number spacing) rounds to the
/// nearest whole number, and stepping down when that overshot is the floor.
/// Anything else is already whole: a zero (returned as it is, keeping its
/// sign), a large value, an infinity or a NaN. The sign test is a branch, not
/// arithmetic: at any one call site dough's argument nearly always has the
/// same sign, and the arithmetic version was slower.
#[inline]
pub(crate) fn floorf(x: f32) -> f32 {
    const TWO_23: f32 = 8_388_608.0;
    if x > 0.0 && x < TWO_23 {
        let nearest = (x + TWO_23) - TWO_23;
        nearest - f32::from(nearest > x)
    } else if x < 0.0 && x > -TWO_23 {
        let nearest = (x - TWO_23) + TWO_23;
        nearest - f32::from(nearest > x)
    } else {
        x
    }
}

#[cfg(test)]
#[test]
fn split_position_matches_flooring_through_an_integer() {
    let mut x = 1.0_f32;
    while x <= 2046.0 {
        let pos = x.floor() as i32;
        assert_eq!(split_position(x), (pos, x - pos as f32), "split({x})");
        x = f32::from_bits(x.to_bits() + 97);
    }
}

#[cfg(test)]
#[test]
fn floorf_is_floor_for_every_kind_of_float() {
    let mut cases = vec![
        0.0,
        -0.0,
        0.5,
        -0.5,
        1.0,
        -1.0,
        1.5,
        -1.5,
        2.999_999_8,
        -2.999_999_8,
        8_388_607.5,
        -8_388_607.5,
        8_388_608.0,
        -8_388_608.0,
        1e20,
        -1e20,
        f32::MIN_POSITIVE,
        -f32::MIN_POSITIVE,
        1e-40,
        -1e-40,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MAX,
        f32::MIN,
    ];
    // and a sweep of bit patterns across the whole range
    cases.extend((0..1_000_000u32).map(|i| f32::from_bits(i.wrapping_mul(4_294_967_291) ^ i)));
    for x in cases {
        if x.is_nan() {
            // The runtime quiets a signalling NaN; arithmetic never makes one.
            assert!(floorf(x).is_nan());
            continue;
        }
        if x.is_sign_negative() && x > -1.0 && x != 0.0 {
            assert_eq!(floorf(x), -1.0, "floor({x:e})");
        }
        assert_eq!(floorf(x).to_bits(), x.floor().to_bits(), "floor({x:e})");
    }
    assert!(floorf(f32::NAN).is_nan());
}

/// `((int)floorf(x), x - (int)floorf(x))` for a delay position in samples, as
/// the flanger and chorus split one, without converting to an integer and
/// back: adding 2^23 leaves `x` rounded to a whole number in the low bits of
/// the sum, and stepping down when that rounded up gives the floor. Exact for
/// `0 <= x < 2^22`; dough clamps its positions to at least 1 and at most the
/// ring's length, and that clamp turns even a NaN into the length.
#[inline]
fn split_position(x: f32) -> (i32, f32) {
    const TWO_23: f32 = 8_388_608.0;
    debug_assert!((0.0..4_194_304.0).contains(&x), "position {x}");
    let sum = x + TWO_23;
    let nearest = sum - TWO_23;
    let rounded_up = nearest > x;
    let whole = (sum.to_bits() & 0x007f_ffff) as i32 - i32::from(rounded_up);
    (whole, x - (nearest - f32::from(rounded_up)))
}

/// `our_log2f`: the exponent bits as a first guess, bent by a rational term.
pub(crate) fn log2f(x: f32) -> f32 {
    let bits = x.to_bits() as i32;
    let m = f32::from_bits(((bits & ((1 << 23) - 1)) | ((0x7f - 1) << 23)) as u32);
    let mut y = bits as f32;
    y *= 1.0 / (1u32 << 23) as f32;
    y - 124.225_45_f32 - 1.498_030_3_f32 * m - 1.725_88_f32 / (0.352_088_72_f32 + m)
}

/// `our_exp2f`. Inline, as dough's is: the phaser calls it every sample.
#[inline]
pub(crate) fn exp2f(x: f32) -> f32 {
    const C0: f32 = std::f32::consts::SQRT_2;
    const C1: f32 = 0.980_258_17;
    const C2: f32 = 0.339_731_57;
    const C3: f32 = 0.078_494_66;
    const C4: f32 = 0.013_602_088;
    const M0: f32 = 0.999_944_3;
    const M1: f32 = 1.000_031_2;
    let xf = floorf(x);
    let ystep = f32::from_bits(127i32.wrapping_add(xf as i32).wrapping_shl(23) as u32);
    let x1 = x - xf;
    let xt = x1 - 0.5;
    let ytaylor = C0 + xt * (C1 + xt * (C2 + xt * (C3 + xt * C4)));
    ystep * ytaylor * (M0 + (M1 - M0) * x1)
}

/// `our_powf`.
pub(crate) fn powf(x: f32, y: f32) -> f32 {
    if x < 0.0 {
        return f32::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if y == 0.0 {
        return 1.0;
    }
    exp2f(y * log2f(x))
}

/// `our_expm1f`.
pub(crate) fn expm1f(x: f32) -> f32 {
    exp2f(x * std::f32::consts::LOG2_E) - 1.0
}

/// `pow10`.
pub(crate) fn pow10(x: f32) -> f32 {
    exp2f(x * std::f32::consts::LOG2_10)
}

/// `modpi`: wraps into [-pi, pi].
fn modpi(mut x: f32) -> f32 {
    x += FPI;
    x *= 0.5 / FPI;
    x -= floorf(x);
    x *= 2.0 * FPI;
    x -= FPI;
    x
}

/// `par_sinf`: a sine made from a parabola.
pub(crate) fn par_sinf(x: f32) -> f32 {
    let x = modpi(x);
    0.405_284_73_f32 * x * (FPI - x.abs())
}

/// `par_cosf`.
pub(crate) fn par_cosf(x: f32) -> f32 {
    par_sinf(x + 0.5 * FPI)
}

/// `our_sinf`: a Padé approximant over a folded argument.
pub(crate) fn sinf(x: f32) -> f32 {
    const C0: f32 = 1.0;
    const C1: f32 = 1.0;
    const C2: f32 = 445.0 / 12122.0;
    const C3: f32 = -(2363.0 / 18183.0);
    const C4: f32 = 601.0 / 872784.0;
    const C5: f32 = 12671.0 / 4363920.0;
    const C6: f32 = 121.0 / 16662240.0;
    let x = (4.0 * (x * (0.5 / FPI) - floorf(x * (0.5 / FPI) + 3.0 / 4.0) + 1.0 / 4.0).abs() - 1.0)
        * (FPI / 2.0);
    let xx = x * x;
    let num = x * (C1 + xx * (C3 + xx * C5));
    let denom = C0 + xx * (C2 + xx * (C4 + xx * C6));
    num / denom
}

/// The WebAssembly build's `fmaxf`: a plain comparison (src/dough.c gives the
/// C build the same).
pub(crate) fn fmaxf(x: f32, y: f32) -> f32 {
    if x > y { x } else { y }
}

/// The WebAssembly build's `fminf`.
pub(crate) fn fminf(x: f32, y: f32) -> f32 {
    if x < y { x } else { y }
}

/// The WebAssembly build's `roundf`: halves round up.
fn roundf(x: f32) -> f32 {
    floorf(x + 0.5)
}

/// dough's LCG (`lcg_rand`), seeded as dough seeds it. One per engine.
pub(crate) struct Rng(u32);

impl Default for Rng {
    fn default() -> Rng {
        Rng(123_456_789)
    }
}

impl Rng {
    /// `random_float`: in [0, 1].
    pub(crate) fn float(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12345);
        ((self.0 >> 16) & 0x7fff) as f32 / 32767.0
    }
}

/// `lerp`, eased by `exponent` (0 linear, > 0 in, < 0 out).
fn lerp(x: f32, y0: f32, y1: f32, exponent: f32) -> f32 {
    if x <= 0.0 {
        return y0;
    }
    if x >= 1.0 {
        return y1;
    }
    let curved = if exponent == 0.0 {
        x
    } else if exponent > 0.0 {
        powf(x, exponent)
    } else {
        1.0 - powf(1.0 - x, -exponent)
    };
    y0 + (y1 - y0) * curved
}

/// `clamp`.
pub(crate) fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// `nan_fallback`.
pub(crate) fn nan_fallback(value: &mut f32, fallback: f32) {
    if value.is_nan() {
        *value = fallback;
    }
}

/// `ftz`: flush small values to zero.
pub(crate) fn ftz(x: f32, limit: f32) -> f32 {
    if x < limit && x > -limit { 0.0 } else { x }
}

fn poly_blep(mut t: f32, dt: f32) -> f32 {
    if t < dt {
        t /= dt;
        return t + t - t * t - 1.0;
    }
    if t > 1.0 - dt {
        t = (t - 1.0) / dt;
        return t * t + t + t + 1.0;
    }
    0.0
}

/// `update_phasor`: true when the phase wrapped this sample.
fn update_phasor(phase: &mut f32, freq: f32, isr: f32) -> bool {
    *phase += freq * isr;
    if *phase >= 1.0 {
        *phase -= 1.0;
        true
    } else if *phase < 0.0 {
        *phase += 1.0;
        true
    } else {
        false
    }
}

// --- oscillators ------------------------------------------------------------

pub(crate) fn tri(phase: &mut f32, freq: f32, isr: f32) -> f32 {
    let s = if *phase < 0.5 {
        4.0 * *phase - 1.0
    } else {
        3.0 - 4.0 * *phase
    };
    update_phasor(phase, freq, isr);
    s
}

pub(crate) fn sine(phase: &mut f32, freq: f32, isr: f32) -> f32 {
    let s = sinf((*phase as f64 * 2.0 * FPI as f64) as f32);
    update_phasor(phase, freq, isr);
    s
}

pub(crate) fn zaw(phase: &mut f32, freq: f32, isr: f32) -> f32 {
    let s = *phase * 2.0 - 1.0;
    update_phasor(phase, freq, isr);
    s
}

pub(crate) fn saw(phase: &mut f32, freq: f32, isr: f32) -> f32 {
    let p = poly_blep(*phase, freq * isr);
    let s = *phase * 2.0 - 1.0 - p;
    update_phasor(phase, freq, isr);
    s
}

pub(crate) fn pulse(phase: &mut f32, freq: f32, pw: f32, isr: f32) -> f32 {
    let dt = freq * isr;
    let mut phi = *phase + pw;
    if phi >= 1.0 {
        phi -= 1.0;
    }
    let p1 = poly_blep(phi, dt);
    let p2 = poly_blep(*phase, dt);
    let pulse = 2.0 * (*phase - phi) - p2 + p1;
    update_phasor(phase, freq, isr);
    pulse + pw * 2.0 - 1.0
}

pub(crate) fn pulze(phase: &mut f32, freq: f32, duty: f32, isr: f32) -> f32 {
    let s = if *phase < duty { 1.0 } else { -1.0 };
    update_phasor(phase, freq, isr);
    s
}

/// A registered sample as an event sees it (`FileSource`), without the PCM,
/// which the engine keeps.
#[derive(Clone, Copy)]
pub(crate) struct FileSource {
    pub(crate) index: usize,
    pub(crate) channels: i32,
    pub(crate) frames: i32,
    pub(crate) freq: f32,
}

/// `FileSource_update`: the nearest frame of the slice `(begin, end)`, into
/// both channels.
pub(crate) fn file(
    phase: &mut f32,
    freq: f32,
    isr: f32,
    source: &FileSource,
    pcm: &[f32],
    (begin, end): (f32, f32),
    out: &mut [f32; 2],
) -> bool {
    let begin_frame = (begin * source.frames as f32) as i32;
    let end_frame = (end * source.frames as f32) as i32;
    let slice_frames = end_frame - begin_frame;
    if slice_frames <= 0 {
        *out = [0.0; 2];
        return false;
    }
    let mut current = begin_frame + floorf(*phase * slice_frames as f32) as i32;
    if current >= end_frame {
        current = end_frame - 1;
    }
    for (c, sample) in out.iter_mut().enumerate() {
        let channel = c as i32 % source.channels;
        let at = current.wrapping_mul(source.channels).wrapping_add(channel);
        *sample = usize::try_from(at)
            .ok()
            .and_then(|at| pcm.get(at))
            .copied()
            .unwrap_or(0.0);
    }
    update_phasor(phase, freq, isr)
}

// --- noise ------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub(crate) struct PinkNoise {
    b: [f32; 7],
}

impl PinkNoise {
    pub(crate) fn update(&mut self, rng: &mut Rng) -> f32 {
        let white = rng.float() * 2.0 - 1.0;
        let w = white as f64;
        let b = &mut self.b;
        b[0] = (0.99886 * b[0] as f64 + w * 0.0555179) as f32;
        b[1] = (0.99332 * b[1] as f64 + w * 0.0750759) as f32;
        b[2] = (0.969 * b[2] as f64 + w * 0.153852) as f32;
        b[3] = (0.8665 * b[3] as f64 + w * 0.3104856) as f32;
        b[4] = (0.55 * b[4] as f64 + w * 0.5329522) as f32;
        b[5] = (-0.7616 * b[5] as f64 - w * 0.016898) as f32;
        let sum = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6];
        let pink = (sum as f64 + w * 0.5362) as f32;
        b[6] = (w * 0.115926) as f32;
        (pink as f64 * 0.11) as f32
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct BrownNoise {
    out: f32,
}

impl BrownNoise {
    pub(crate) fn update(&mut self, rng: &mut Rng) -> f32 {
        let white = (rng.float() as f64 * 2.0 - 1.0) as f32;
        self.out = ((self.out as f64 + 0.02 * white as f64) / 1.02) as f32;
        self.out
    }
}

// --- envelope ---------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AdsrState {
    #[default]
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy)]
pub(crate) struct Adsr {
    pub(crate) state: AdsrState,
    pub(crate) start_time: f32,
    start_val: f32,
    attack_curve: f32,
    decay_curve: f32,
}

impl Default for Adsr {
    fn default() -> Adsr {
        Adsr {
            state: AdsrState::Off,
            start_time: 0.0,
            start_val: 0.0,
            attack_curve: 2.0,
            decay_curve: 2.0,
        }
    }
}

impl Adsr {
    /// `ADSRNode_update`.
    pub(crate) fn update(
        &mut self,
        cur_time: f32,
        gate: f32,
        attack: f32,
        decay: f32,
        sus: f32,
        release: f32,
    ) -> f32 {
        match self.state {
            AdsrState::Off => {
                if gate > 0.0 {
                    self.state = AdsrState::Attack;
                    self.start_time = cur_time;
                    self.start_val = 0.0;
                }
                0.0
            }
            AdsrState::Attack => {
                let time = cur_time - self.start_time;
                if time > attack {
                    self.state = AdsrState::Decay;
                    self.start_time = cur_time;
                    return 1.0;
                }
                lerp(time / attack, self.start_val, 1.0, self.attack_curve)
            }
            AdsrState::Decay => {
                let time = cur_time - self.start_time;
                let cur_val = lerp(time / decay, 1.0, sus, -self.decay_curve);
                if gate <= 0.0 {
                    self.state = AdsrState::Release;
                    self.start_time = cur_time;
                    self.start_val = cur_val;
                    return cur_val;
                }
                if time > decay {
                    self.state = AdsrState::Sustain;
                    self.start_time = cur_time;
                    return sus;
                }
                cur_val
            }
            AdsrState::Sustain => {
                if gate <= 0.0 {
                    self.state = AdsrState::Release;
                    self.start_time = cur_time;
                    self.start_val = sus;
                }
                sus
            }
            AdsrState::Release => {
                let time = cur_time - self.start_time;
                if time > release {
                    self.state = AdsrState::Off;
                    return 0.0;
                }
                let cur_val = lerp(time / release, self.start_val, 0.0, -self.decay_curve);
                if gate > 0.0 {
                    self.state = AdsrState::Attack;
                    self.start_time = cur_time;
                    self.start_val = cur_val;
                }
                cur_val
            }
        }
    }
}

/// `init_envelope`: defaults for an envelope any of whose values is set;
/// returns whether it is active.
pub(crate) fn init_envelope(
    env: &mut f32,
    att: &mut f32,
    dec: &mut f32,
    sus: &mut f32,
    rel: &mut f32,
) -> bool {
    if env.is_nan() && att.is_nan() && dec.is_nan() && sus.is_nan() && rel.is_nan() {
        return false;
    }
    if sus.is_nan() && !att.is_nan() && dec.is_nan() {
        *sus = 1.0;
    } else if sus.is_nan() && !dec.is_nan() {
        *sus = 0.0;
    } else if !sus.is_nan() {
        *sus = fminf(*sus, 1.0);
    }
    nan_fallback(env, 1.0);
    nan_fallback(att, 0.001);
    nan_fallback(dec, 0.0);
    nan_fallback(sus, 1.0);
    nan_fallback(rel, 0.005);
    true
}

// --- filters ----------------------------------------------------------------

/// The filter types dough uses (`BiquadFilter_update`'s `type`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilterType {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
}

/// A biquad from the Audio EQ Cookbook. dough recomputes its coefficients
/// every sample; they depend only on the type, cutoff and Q, so here they are
/// kept, and recomputed when one of those differs from the last sample's by so
/// much as a bit. A fixed filter then costs five multiplies a sample instead of
/// a sine, a cosine and seven divisions, with the very same coefficients.
#[derive(Clone, Copy)]
pub(crate) struct Biquad {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    /// `(type, cutoff bits, Q bits)` the coefficients are for; `None` before
    /// the first sample.
    key: Option<(FilterType, u32, u32)>,
    /// `b0, b1, b2, a1, a2`, already divided by `a0`.
    coefficients: [f32; 5],
}

impl Default for Biquad {
    fn default() -> Biquad {
        Biquad {
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            key: None,
            coefficients: [0.0; 5],
        }
    }
}

/// `BiquadFilter_update`'s coefficients, normalised. Its peaking and shelving
/// types are unreachable in dough, so they are not ported.
#[inline(always)]
fn biquad_coefficients(kind: FilterType, freq: f32, mut q: f32, rates: Rates) -> [f32; 5] {
    let omega = (2.0 * FPI as f64 * freq as f64 / rates.sr as f64) as f32;
    let sin_omega = par_sinf(omega);
    if matches!(kind, FilterType::Lowpass | FilterType::Highpass) {
        q = pow10(q / 20.0);
    }
    let alpha = sin_omega / (2.0 * q);
    let cos_omega = par_cosf(omega);
    let a0 = 1.0 + alpha;
    let a1 = (-2.0 * cos_omega) / a0;
    let a2 = (1.0 - alpha) / a0;
    // dough divides all five by `a0`. Equal numerators give equal quotients,
    // and a negated one the negated quotient, so each distinct division is
    // done once: b2 is b0 for the low-, high-pass and notch and -b0 for the
    // band-pass, and the notch's b1 is a1.
    match kind {
        FilterType::Lowpass => {
            let b1 = 1.0 - cos_omega;
            let b0 = (b1 / 2.0) / a0;
            [b0, b1 / a0, b0, a1, a2]
        }
        FilterType::Highpass => {
            let b0 = ((1.0 + cos_omega) / 2.0) / a0;
            [b0, -(1.0 + cos_omega) / a0, b0, a1, a2]
        }
        FilterType::Bandpass => {
            let b0 = (sin_omega / 2.0) / a0;
            [b0, 0.0 / a0, -b0, a1, a2]
        }
        FilterType::Notch => {
            let b0 = 1.0 / a0;
            [b0, a1, b0, a1, a2]
        }
    }
}

impl Biquad {
    /// `BiquadFilter_update`.
    pub(crate) fn update(
        &mut self,
        input: f32,
        kind: FilterType,
        freq: f32,
        q: f32,
        rates: Rates,
    ) -> f32 {
        let key = Some((kind, freq.to_bits(), q.to_bits()));
        if self.key != key {
            self.key = key;
            self.coefficients = biquad_coefficients(kind, freq, q, rates);
        }
        self.step(input, self.coefficients)
    }

    /// `BiquadFilter_update` for a cutoff that moves every sample, as the
    /// phaser's does: the kept coefficients would never be reused.
    pub(crate) fn update_moving(
        &mut self,
        input: f32,
        kind: FilterType,
        freq: f32,
        q: f32,
        rates: Rates,
    ) -> f32 {
        self.step(input, biquad_coefficients(kind, freq, q, rates))
    }

    fn step(&mut self, input: f32, [b0, b1, b2, a1, a2]: [f32; 5]) -> f32 {
        let output = b0 * input + b1 * self.x1 + b2 * self.x2 - a1 * self.y1 - a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

/// `Lag`: a one-pole smoother (glide).
#[derive(Clone, Copy, Default)]
pub(crate) struct Lag {
    pub(crate) s: f32,
}

impl Lag {
    pub(crate) fn update(&mut self, input: f32, rate: f32, lag_unit: i32) -> f32 {
        let mut rate = rate * lag_unit as f32;
        if rate < 1.0 {
            rate = 1.0;
        }
        self.s += (1.0 / rate) * (input - self.s);
        self.s
    }
}

/// `Coarse`: a sample-rate reducer.
#[derive(Clone, Copy, Default)]
pub(crate) struct Coarse {
    hold: f32,
    t: i32,
}

impl Coarse {
    /// `Coarse_update`. A `coarse` under 1 divides by zero in dough (the
    /// browser's worklet traps); here it passes the input through instead.
    pub(crate) fn update(&mut self, input: f32, coarse: i32) -> f32 {
        if coarse == 0 || self.t.wrapping_rem(coarse) == 0 {
            self.t = 0;
            self.hold = input;
        }
        self.t = self.t.wrapping_add(1);
        self.hold
    }
}

/// `crush`: bit depth reduction.
pub(crate) fn crush(input: f32, crush: f32) -> f32 {
    let crush = fmaxf(1.0, crush);
    let x = exp2f(crush - 1.0);
    roundf(input * x) / x
}

/// `distort`.
pub(crate) fn distort(input: f32, amount: f32, postgain: f32) -> f32 {
    let k = expm1f(amount);
    (((1.0 + k) * input) / (1.0 + k * input.abs())) * postgain
}

// --- modulation effects -----------------------------------------------------

#[derive(Clone, Copy, Default)]
pub(crate) struct Phaser {
    filter1: Biquad,
    filter2: Biquad,
    lfo: f32,
}

impl Phaser {
    pub(crate) fn update(
        &mut self,
        input: f32,
        rate: f32,
        depth: f32,
        center: f32,
        sweep: f32,
        rates: Rates,
    ) -> f32 {
        let lfo = sine(&mut self.lfo, rate, rates.isr);
        let q = 2.0 - fminf(fmaxf(depth * 2.0, 0.0), 1.9);
        let detune_cents = lfo * sweep;
        let multiplier = exp2f(detune_cents / 1200.0);
        let mut freq1 = center * multiplier;
        let mut freq2 = (center + 282.0) * multiplier;
        freq1 = fmaxf(freq1, 20.0);
        freq1 = fminf(freq1, rates.sr * 0.45);
        freq2 = fmaxf(freq2, 20.0);
        freq2 = fminf(freq2, rates.sr * 0.45);
        let out = self
            .filter1
            .update_moving(input, FilterType::Notch, freq1, q, rates);
        self.filter2
            .update_moving(out, FilterType::Notch, freq2, q, rates)
    }
}

const FLANGER_BUFFER: usize = 512;

#[derive(Clone, Copy)]
pub(crate) struct Flanger {
    buffer: [f32; FLANGER_BUFFER],
    write_pos: i32,
    lfo: f32,
    feedback_buffer: f32,
}

impl Default for Flanger {
    fn default() -> Flanger {
        Flanger {
            buffer: [0.0; FLANGER_BUFFER],
            write_pos: 0,
            lfo: 0.0,
            feedback_buffer: 0.0,
        }
    }
}

impl Flanger {
    #[inline(never)]
    pub(crate) fn update(
        &mut self,
        input: f32,
        rate: f32,
        depth: f32,
        feedback: f32,
        rates: Rates,
    ) -> f32 {
        const SIZE: i32 = FLANGER_BUFFER as i32;
        let lfo = sine(&mut self.lfo, rate, rates.isr);
        let min_delay_ms = 0.5_f32;
        let delay_range = 10.0_f32 - min_delay_ms;
        let depth_curve = depth * depth;
        let delay_ms = min_delay_ms + depth_curve * delay_range * (lfo * 0.5 + 0.5);
        let mut delay_samples = delay_ms * rates.sr * 0.001;
        delay_samples = fmaxf(1.0, fminf(delay_samples, SIZE as f32 - 2.0));
        let (read_pos, frac) = split_position(delay_samples);
        // `% SIZE` of a power of two, on values dough keeps positive.
        let mask = FLANGER_BUFFER - 1;
        let index1 = (self.write_pos - read_pos + SIZE) as usize & mask;
        let index2 = (self.write_pos - read_pos - 1 + SIZE) as usize & mask;
        let delayed1 = self.buffer[index1];
        let delayed2 = self.buffer[index2];
        let delayed = delayed1 + frac * (delayed2 - delayed1);
        let feedback = fmaxf(0.0, fminf(feedback, 0.95));
        self.buffer[self.write_pos as usize & mask] = input + self.feedback_buffer * feedback;
        // `% SIZE`, as a mask: `write_pos` stays in 0..SIZE.
        self.write_pos = (self.write_pos + 1) & (SIZE - 1);
        self.feedback_buffer = delayed;
        input * 0.5 + delayed * 0.5
    }
}

const CHORUS_BUFFER: usize = 2048;
const CHORUS_VOICES: usize = 3;

#[derive(Clone, Copy)]
pub(crate) struct Chorus {
    buffer: [f32; CHORUS_BUFFER],
    write_pos: i32,
    lfo: [f32; CHORUS_VOICES],
}

impl Default for Chorus {
    fn default() -> Chorus {
        let mut lfo = [0.0; CHORUS_VOICES];
        for (i, phase) in lfo.iter_mut().enumerate() {
            *phase = i as f32 / CHORUS_VOICES as f32;
        }
        Chorus {
            buffer: [0.0; CHORUS_BUFFER],
            write_pos: 0,
            lfo,
        }
    }
}

impl Chorus {
    /// One tap of `Chorus_update`, `delay` ms behind the write position.
    fn tap(&self, delay_ms: f32, min_ms: f32, max_ms: f32, sr: f32) -> f32 {
        const SIZE: i32 = CHORUS_BUFFER as i32;
        let delay_ms = fmaxf(min_ms, fminf(delay_ms, max_ms));
        let samples = fmaxf(1.0, fminf(delay_ms * sr * 0.001, SIZE as f32 - 2.0));
        let (pos, frac) = split_position(samples);
        // `% SIZE` of a power of two, on values dough keeps positive.
        let mask = CHORUS_BUFFER - 1;
        let i0 = (self.write_pos - pos + SIZE) as usize & mask;
        let i1 = (self.write_pos - pos - 1 + SIZE) as usize & mask;
        self.buffer[i0] + frac * (self.buffer[i1] - self.buffer[i0])
    }

    #[inline(never)]
    pub(crate) fn update(
        &mut self,
        left: &mut f32,
        right: &mut f32,
        rate: f32,
        depth: f32,
        delay_ms: f32,
        rates: Rates,
    ) {
        let min_ms = 1.5_f32;
        let max_ms = fminf(delay_ms * 2.0, 50.0);
        let range = max_ms - min_ms;
        let depth = fmaxf(0.0, fminf(depth, 1.0));
        let mono = (*left + *right) * 0.5;
        self.buffer[self.write_pos as usize & (CHORUS_BUFFER - 1)] = mono;
        // The three LFOs first, then their taps, then the sums in dough's
        // order: three independent chains the CPU can overlap, where a loop
        // over the voices ran them one after another.
        let lfo = std::array::from_fn::<f32, CHORUS_VOICES, _>(|v| {
            sine(&mut self.lfo[v], rate, rates.isr)
        });
        let taps = lfo.map(|lfo| {
            let delay_l = delay_ms + depth * range * lfo * 0.5;
            let delay_r = delay_ms - depth * range * lfo * 0.5;
            (
                self.tap(delay_l, min_ms, max_ms, rates.sr),
                self.tap(delay_r, min_ms, max_ms, rates.sr),
            )
        });
        let (mut out_l, mut out_r) = (0.0_f32, 0.0_f32);
        for (tap_l, tap_r) in taps {
            out_l += tap_l;
            out_r += tap_r;
        }
        self.write_pos = (self.write_pos + 1) & (CHORUS_BUFFER as i32 - 1);
        out_l /= CHORUS_VOICES as f32;
        out_r /= CHORUS_VOICES as f32;
        let mix = 0.5_f32.sqrt();
        *left = mono * mix + out_l * mix;
        *right = mono * mix + out_r * mix;
    }
}

// --- orbit effects ----------------------------------------------------------

/// `Delay`: a plain delay line, one per orbit channel.
pub(crate) struct Delay {
    write: i32,
    buffer: Box<[f32]>,
}

impl Delay {
    pub(crate) fn new(max_samples: usize) -> Delay {
        Delay {
            write: 0,
            buffer: vec![0.0; max_samples].into_boxed_slice(),
        }
    }

    /// `Delay_update`.
    pub(crate) fn update(&mut self, input: f32, time: f32, sr: f32) -> f32 {
        let max = self.buffer.len() as i32;
        // `(write + 1) % max`, without the division: `write` is below `max`.
        self.write += 1;
        if self.write >= max {
            self.write = 0;
        }
        self.buffer[self.write as usize] = input;
        // `MIN(floor(engine.sr * time), max_delay_samples - 1)`, in double. For
        // a delay time that is not negative, truncating is the floor; a NaN
        // compares false and takes the cap, as in C.
        let wanted = (sr * time) as f64;
        let cap = max - 1;
        let samples = if wanted >= 0.0 {
            (wanted as i64).min(i64::from(cap)) as i32
        } else if wanted.is_nan() {
            cap
        } else {
            wanted.floor() as i32
        };
        let mut read = self.write.wrapping_sub(samples);
        if read < 0 {
            read = read.wrapping_add(max);
        }
        // In range for any delay time dough accepts; kept in range regardless.
        let read = if (0..max).contains(&read) {
            read
        } else {
            read.rem_euclid(max)
        };
        self.buffer[read as usize]
    }
}

/// A Dattorro delay line (`VerbDelayBuffer`): a ring of `N` samples, a power
/// of two, with up to four read taps, indexed in 16 bits as dough does.
///
/// dough sizes each ring to the next power of two above its delay. A read at
/// `(t + N - delay) & (N - 1)` is the sample written `delay` steps ago in any
/// power-of-two ring longer than the delay (`N` divides the 16-bit counter's
/// period), so lines of one role can share the largest of their sizes, and be
/// arrays, without changing what they play.
struct VerbLine<const N: usize> {
    buffer: Box<[f32; N]>,
    offset: [u16; 4],
}

const TAP_MAIN: usize = 0;
const TAP_OUT1: usize = 1;
const TAP_OUT2: usize = 2;
const TAP_OUT3: usize = 3;

impl<const N: usize> VerbLine<N> {
    /// `VerbDelayBuffer_init`.
    fn new(delay: u16) -> VerbLine<N> {
        debug_assert!(N.is_power_of_two() && usize::from(delay) < N && N <= 1 << 16);
        let mut line = VerbLine {
            buffer: Box::new([0.0; N]),
            offset: [0; 4],
        };
        line.set_delay(TAP_MAIN, delay);
        line
    }

    fn set_delay(&mut self, tap: usize, delay: u16) {
        self.offset[tap] = (N as i32 - i32::from(delay)) as u16;
    }

    fn write(&mut self, t: u16, input: f32) {
        self.buffer[usize::from(t) & (N - 1)] = input;
    }

    fn read(&self, tap: usize, t: u16) -> f32 {
        self.buffer[(usize::from(t) + usize::from(self.offset[tap])) & (N - 1)]
    }

    fn process(&mut self, t: u16, input: f32) -> f32 {
        self.write(t, input);
        self.read(TAP_MAIN, t)
    }

    fn all_pass(&mut self, t: u16, gain: f32, mut input: f32) -> f32 {
        let delayed = self.read(TAP_MAIN, t);
        input += delayed * -gain;
        self.write(t, input);
        delayed + input * gain
    }
}

fn low_pass(state: &mut f32, freq: f32, input: f32) -> f32 {
    *state += (input - *state) * freq;
    *state
}

/// The longest pre-delay, in samples (`VERB_MAX_PREDELAY`).
const VERB_MAX_PREDELAY: u16 = 4800;

/// Jon Dattorro's plate reverb, as dough adapts el-visio's. Each group of
/// lines shares the largest of dough's ring sizes for it (see [`VerbLine`]).
pub(crate) struct Verb {
    pre_delay: VerbLine<8192>,
    in_diffusion: [VerbLine<512>; 4],
    decay_diffusion1: [VerbLine<1024>; 2],
    pre_damping: [VerbLine<8192>; 2],
    decay_diffusion2: [VerbLine<4096>; 2],
    post_damping: [VerbLine<4096>; 2],
    pre_filter: f32,
    damping: [f32; 2],
    pre_filter_amount: f32,
    input_diffusion1: f32,
    input_diffusion2: f32,
    decay_diffusion1_amount: f32,
    damping_amount: f32,
    decay: f32,
    decay_diffusion2_amount: f32,
    t: u16,
}

impl Verb {
    /// `DattorroVerb_init`.
    pub(crate) fn new() -> Verb {
        fn tapped<const N: usize>(delay: u16, taps: &[(usize, u16)]) -> VerbLine<N> {
            let mut line = VerbLine::new(delay);
            for &(tap, at) in taps {
                line.set_delay(tap, at);
            }
            line
        }
        let mut verb = Verb {
            pre_delay: VerbLine::new(VERB_MAX_PREDELAY),
            in_diffusion: [
                VerbLine::new(142),
                VerbLine::new(107),
                VerbLine::new(379),
                VerbLine::new(277),
            ],
            decay_diffusion1: [VerbLine::new(672), VerbLine::new(908)],
            pre_damping: [
                tapped(4453, &[(TAP_OUT1, 353), (TAP_OUT2, 3627), (TAP_OUT3, 1990)]),
                tapped(4217, &[(TAP_OUT1, 266), (TAP_OUT2, 2974), (TAP_OUT3, 2111)]),
            ],
            decay_diffusion2: [
                tapped(1800, &[(TAP_OUT1, 187), (TAP_OUT2, 1228)]),
                tapped(2656, &[(TAP_OUT1, 335), (TAP_OUT2, 1913)]),
            ],
            post_damping: [
                tapped(3720, &[(TAP_OUT1, 1066), (TAP_OUT2, 2673)]),
                tapped(3163, &[(TAP_OUT1, 121), (TAP_OUT2, 1996)]),
            ],
            pre_filter: 0.0,
            damping: [0.0; 2],
            pre_filter_amount: 0.0,
            input_diffusion1: 0.0,
            input_diffusion2: 0.0,
            decay_diffusion1_amount: 0.0,
            damping_amount: 0.0,
            decay: 0.0,
            decay_diffusion2_amount: 0.0,
            t: 0,
        };
        verb.set_pre_delay(0.1);
        verb.pre_filter_amount = 0.85;
        verb.input_diffusion1 = 0.75;
        verb.input_diffusion2 = 0.625;
        verb.set_decay(0.75);
        verb.decay_diffusion1_amount = 0.70;
        verb.damping_amount = 0.95;
        verb
    }

    pub(crate) fn set_pre_delay(&mut self, value: f32) {
        self.pre_delay
            .set_delay(TAP_MAIN, (value * f32::from(VERB_MAX_PREDELAY)) as u16);
    }

    pub(crate) fn set_decay(&mut self, value: f32) {
        self.decay = clamp(value, 0.0, 0.99);
        self.decay_diffusion2_amount = clamp(self.decay + 0.15, 0.25, 0.50);
    }

    pub(crate) fn set_damping(&mut self, value: f32) {
        self.damping_amount = value;
    }

    pub(crate) fn set_input_diffusion(&mut self, value: f32) {
        self.input_diffusion1 = value;
        self.input_diffusion2 = value;
    }

    /// `DattorroVerb_process`.
    pub(crate) fn process(&mut self, input: f32) {
        let t = self.t;
        if t & 0x07ff == 0 {
            for line in &mut self.decay_diffusion1 {
                line.offset[TAP_MAIN] = if t < (1 << 15) {
                    line.offset[TAP_MAIN].wrapping_sub(1)
                } else {
                    line.offset[TAP_MAIN].wrapping_add(1)
                };
            }
        }
        let mut x = self.pre_delay.process(t, input);
        x = low_pass(&mut self.pre_filter, self.pre_filter_amount, x);
        x = self.in_diffusion[0].all_pass(t, self.input_diffusion1, x);
        x = self.in_diffusion[1].all_pass(t, self.input_diffusion1, x);
        x = self.in_diffusion[2].all_pass(t, self.input_diffusion2, x);
        x = self.in_diffusion[3].all_pass(t, self.input_diffusion2, x);
        for i in 0..2 {
            let mut x1 = x + self.post_damping[1 - i].read(TAP_MAIN, t) * self.decay;
            x1 = self.decay_diffusion1[i].all_pass(t, -self.decay_diffusion1_amount, x1);
            x1 = self.pre_damping[i].process(t, x1);
            x1 = low_pass(&mut self.damping[i], self.damping_amount, x1);
            x1 *= self.decay;
            x1 = self.decay_diffusion2[i].all_pass(t, self.decay_diffusion2_amount, x1);
            self.post_damping[i].write(t, x1);
        }
        self.t = t.wrapping_add(1);
    }

    pub(crate) fn left(&self) -> f32 {
        let t = self.t;
        let mut a = 0.0_f32;
        a += self.pre_damping[1].read(TAP_OUT1, t);
        a += self.pre_damping[1].read(TAP_OUT2, t);
        a -= self.decay_diffusion2[1].read(TAP_OUT2, t);
        a += self.post_damping[1].read(TAP_OUT2, t);
        a -= self.pre_damping[0].read(TAP_OUT3, t);
        a -= self.decay_diffusion2[0].read(TAP_OUT1, t);
        a += self.post_damping[0].read(TAP_OUT1, t);
        a
    }

    pub(crate) fn right(&self) -> f32 {
        let t = self.t;
        let mut a = 0.0_f32;
        a += self.pre_damping[0].read(TAP_OUT1, t);
        a += self.pre_damping[0].read(TAP_OUT2, t);
        a -= self.decay_diffusion2[0].read(TAP_OUT2, t);
        a += self.post_damping[0].read(TAP_OUT2, t);
        a -= self.pre_damping[1].read(TAP_OUT3, t);
        a -= self.decay_diffusion2[1].read(TAP_OUT1, t);
        a += self.post_damping[1].read(TAP_OUT1, t);
        a
    }
}
