// signal.rs - continuous signals and random generators.
// Ported from strudel/packages/core/signal.mjs.
// Copyright (C) 2024 Strudel contributors; 2026 Rudel contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::Frac;
use crate::hap::Hap;
use crate::pattern::{Pattern, fastcat, pure};
use crate::value::Value;
use std::f64::consts::TAU;

/// A continuous pattern sampling `f` at the start of the query span.
pub fn signal<F>(f: F) -> Pattern
where
    F: Fn(Frac) -> Value + Send + Sync + 'static,
{
    Pattern::new(move |state| vec![Hap::new(None, state.span, f(state.span.begin))])
}

fn signal_f64<F>(f: F) -> Pattern
where
    F: Fn(f64) -> f64 + Send + Sync + 'static,
{
    signal(move |t| Value::F64(f(t.to_f64())))
}

/// Cycle time as a continuous signal.
pub fn time() -> Pattern {
    signal(|t| Value::F64(t.to_f64()))
}

/// Sawtooth 0..1.
pub fn saw() -> Pattern {
    signal_f64(|t| t.rem_euclid(1.0))
}
/// Inverted sawtooth 1..0.
pub fn isaw() -> Pattern {
    signal_f64(|t| 1.0 - t.rem_euclid(1.0))
}
/// Sine 0..1.
pub fn sine() -> Pattern {
    signal_f64(|t| (((TAU * t).sin()) + 1.0) / 2.0)
}
/// Bipolar sine -1..1.
pub fn sine2() -> Pattern {
    signal_f64(|t| (TAU * t).sin())
}
/// Cosine 0..1.
pub fn cosine() -> Pattern {
    signal_f64(|t| (((TAU * t).cos()) + 1.0) / 2.0)
}
/// Square 0..1.
pub fn square() -> Pattern {
    signal_f64(|t| ((t * 2.0).floor()).rem_euclid(2.0))
}
/// Triangle 0..1 (rises then falls), `fastcat(saw, isaw)`.
pub fn tri() -> Pattern {
    fastcat(&[saw(), isaw()])
}
/// Inverted triangle 1..0 (falls then rises), `fastcat(isaw, saw)`.
pub fn itri() -> Pattern {
    fastcat(&[isaw(), saw()])
}

// Bipolar (-1..1) variants (`saw2`/`cosine2`/...). `sine2`/`rand2` are above.
/// Bipolar sawtooth -1..1.
pub fn saw2() -> Pattern {
    signal_f64(|t| t.rem_euclid(1.0) * 2.0 - 1.0)
}
/// Bipolar inverted sawtooth 1..-1.
pub fn isaw2() -> Pattern {
    signal_f64(|t| (1.0 - t.rem_euclid(1.0)) * 2.0 - 1.0)
}
/// Bipolar cosine -1..1.
pub fn cosine2() -> Pattern {
    signal_f64(|t| (TAU * t).cos())
}
/// Bipolar square -1..1.
pub fn square2() -> Pattern {
    signal_f64(|t| ((t * 2.0).floor()).rem_euclid(2.0) * 2.0 - 1.0)
}
/// Bipolar triangle -1..1, `fastcat(saw2, isaw2)`.
pub fn tri2() -> Pattern {
    fastcat(&[saw2(), isaw2()])
}
/// Bipolar inverted triangle 1..-1, `fastcat(isaw2, saw2)`.
pub fn itri2() -> Pattern {
    fastcat(&[isaw2(), saw2()])
}

/// A continuous pattern of a single constant value (`steady`).
pub fn steady(value: Value) -> Pattern {
    Pattern::new(move |state| vec![Hap::new(None, state.span, value.clone())])
}

// ---------------------------------------------------------------------------
// Legacy RNG (Strudel's default). Ported verbatim from signal.mjs so that
// `rand`/`irand`/`degrade` snapshots match bit-for-bit. JS bitwise ops act on
// int32, which maps directly onto Rust `i32` wrapping arithmetic.

fn xorwise(x: i32) -> i32 {
    let a = x.wrapping_shl(13) ^ x;
    let b = (a >> 17) ^ a;
    b.wrapping_shl(5) ^ b
}

fn time_to_int_seed(x: f64) -> i32 {
    let frac = (x / 300.0).fract(); // __frac: x - trunc(x)
    (frac * 536_870_912.0).trunc() as i32
}

fn int_seed_to_rand(x: i32) -> f64 {
    (x % 536_870_912) as f64 / 536_870_912.0
}

/// One pseudo-random number in [0,1) for cycle time `t` (legacy generator).
pub fn time_to_rand(t: f64) -> f64 {
    int_seed_to_rand(xorwise(time_to_int_seed(t))).abs()
}

/// `n` pseudo-random numbers at time `t` (legacy generator). Values keep
/// their sign, matching Strudel's `__timeToRandsPrime` for `n > 1` (only the
/// scalar `n == 1` path — [`time_to_rand`] — takes the absolute value).
pub fn time_to_rands(t: f64, n: usize) -> Vec<f64> {
    // JS folds the first `xorwise` into `__timeToIntSeed`; Rudel keeps
    // `time_to_int_seed` raw and applies it here (as `time_to_rand` does).
    let mut seed = xorwise(time_to_int_seed(t));
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(int_seed_to_rand(seed));
        seed = xorwise(seed);
    }
    out
}

/// Build a continuous random signal that maps `time_to_rand` through `f`, while
/// honoring an optional `randSeed` control. Strudel's legacy
/// `getRandsAtTime(t, 1, seed)` is `time_to_rand(t + seed)`, so `seed`/`withSeed`
/// (which set `randSeed`) shift the random stream in time.
fn rand_signal<F>(f: F) -> Pattern
where
    F: Fn(f64) -> Value + Send + Sync + 'static,
{
    Pattern::new(move |state| {
        let seed = state
            .controls
            .get("randSeed")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let t = state.span.begin.to_f64();
        vec![Hap::new(None, state.span, f(time_to_rand(t + seed)))]
    })
}

/// Continuous random signal in [0,1).
pub fn rand() -> Pattern {
    rand_signal(Value::F64)
}

/// Continuous random signal in [-1,1).
pub fn rand2() -> Pattern {
    rand_signal(|r| Value::F64(r * 2.0 - 1.0))
}

/// Continuous random integers in 0..n.
pub fn irand(n: i64) -> Pattern {
    rand_signal(move |r| Value::Int((r * n as f64).trunc() as i64))
}

/// `brandBy(p)`: a continuous 0/1 signal that is 1 with probability `p`.
pub fn brand_by(p: f64) -> Pattern {
    rand_signal(move |r| Value::Bool(r < p))
}

/// `brand`: a continuous 0/1 signal, 1 half the time (`brandBy(0.5)`).
pub fn brand() -> Pattern {
    brand_by(0.5)
}

/// Discrete pattern of numbers 0..n-1, one per step.
pub fn run(n: i64) -> Pattern {
    let pats: Vec<Pattern> = (0..n).map(|i| pure(Value::Int(i))).collect();
    fastcat(&pats)
}

/// Bit length of `n` (`floor(log2(n)) + 1`), at least 1.
fn nbits_for(n: i64) -> i64 {
    if n <= 0 {
        1
    } else {
        (n as f64).log2().floor() as i64 + 1
    }
}

/// `binaryN(n, nBits)`: a `nBits`-step pattern of the bits of `n`, MSB first
/// (handy as a `struct`). Ported from Strudel's
/// `n.segment(nBits).brshift(bitPos).band(1)`, so a patterned `n` is sampled per
/// step.
pub fn binary_n(n: impl crate::transforms::IntoPattern, nbits: i64) -> Pattern {
    let nbits = nbits.max(1);
    // bitPos per step i: -i + (nBits - 1) = nBits-1-i (MSB on the left).
    let bit_pos = run(nbits).mul(-1).add(nbits - 1);
    n.into_pattern()
        .segment(Frac::int(nbits))
        .brshift(bit_pos)
        .band(pure(Value::Int(1)))
}

/// `binary(n)`: like [`binary_n`] with `nBits = floor(log2(n)) + 1`.
pub fn binary(n: i64) -> Pattern {
    binary_n(pure(Value::Int(n)), nbits_for(n))
}

/// `binaryNL(n, nBits)`: each value becomes the *list* of its `nBits` bits, MSB
/// first (for `partials`/`phases`). A patterned `n` yields a list per value.
pub fn binary_nl(n: impl crate::transforms::IntoPattern, nbits: i64) -> Pattern {
    let nbits = nbits.max(0);
    n.into_pattern().fmap(move |v| {
        let num = v.as_f64().unwrap_or(0.0) as i64 as i32;
        let bits = (0..nbits)
            .rev()
            .map(|i| Value::Int(((num >> i) & 1) as i64))
            .collect();
        Value::List(bits)
    })
}

/// `binaryL(n)`: like [`binary_nl`] with `nBits = floor(log2(value)) + 1` per
/// value (so each value's list is exactly as long as its bit length).
pub fn binary_l(n: impl crate::transforms::IntoPattern) -> Pattern {
    n.into_pattern().fmap(|v| {
        let num = (v.as_f64().unwrap_or(0.0) as i64 as i32).max(0);
        let nbits = nbits_for(num as i64);
        let bits = (0..nbits)
            .rev()
            .map(|i| Value::Int(((num >> i) & 1) as i64))
            .collect();
        Value::List(bits)
    })
}

/// `randL(n)`: a continuous signal whose value is a list of `n` random numbers
/// in `[0, 1)` (the legacy RNG, abs as in Strudel). Used to drive `partials`.
pub fn rand_l(n: i64) -> Pattern {
    signal(move |t| {
        let rands = time_to_rands(t.to_f64(), n.max(0) as usize)
            .into_iter()
            .map(|x| Value::F64(x.abs()))
            .collect();
        Value::List(rands)
    })
}

/// `scan`: step through growing runs, one per cycle — cycle 0 plays `run(1)`,
/// cycle 1 plays `run(2)`, …, up to `run(n)`, then loops.
pub fn scan(n: i64) -> Pattern {
    let runs: Vec<Pattern> = (1..=n.max(0)).map(run).collect();
    crate::pattern::slowcat(&runs)
}

/// `randrun(n)`: each cycle plays the integers `0..n` once each, in an order
/// shuffled per cycle. Reads an optional `randSeed` control. Used by `shuffle`.
pub fn randrun(n: i64) -> Pattern {
    if n <= 0 {
        return crate::pattern::silence();
    }
    Pattern::new(move |state| {
        let seed = state
            .controls
            .get("randSeed")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let t = state.span.begin;
        // Without adding 0.5, the first cycle is always 0,1,2,3,...
        let rands = time_to_rands(t.floor().to_f64() + 0.5 + seed, n as usize);
        let mut order: Vec<usize> = (0..n as usize).collect();
        order.sort_by(|&a, &b| {
            rands[a]
                .partial_cmp(&rands[b])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let i = ((t.cycle_pos() * Frac::int(n)).floor().to_f64() as i64).rem_euclid(n) as usize;
        vec![Hap::new(None, state.span, Value::Int(order[i] as i64))]
    })
    ._segment(Frac::int(n))
}

// ---------------------------------------------------------------------------
// Perlin noise (signal.mjs `_perlin`/`perlin`).

/// Quintic smoothstep `6x^5 - 15x^4 + 10x^3`, giving zero 1st/2nd derivatives
/// at the endpoints (Ken Perlin's "smootherstep").
fn smoother_step(x: f64) -> f64 {
    6.0 * x.powi(5) - 15.0 * x.powi(4) + 10.0 * x.powi(3)
}

/// Perlin-style value noise at cycle time `t`: smoothly interpolate between the
/// random values at the two surrounding integer times.
pub fn perlin_at(t: f64, seed: f64) -> f64 {
    let ta = t.floor();
    let tb = ta + 1.0;
    // getRandsAtTime(_, 1, seed) (legacy) == time_to_rand(time + seed).
    let ra = time_to_rand(ta + seed);
    let rb = time_to_rand(tb + seed);
    ra + smoother_step(t - ta) * (rb - ra)
}

/// Continuous Perlin-noise signal in 0..1. Reads an optional `randSeed` control
/// from the query state (defaulting to 0), mirroring Strudel.
pub fn perlin() -> Pattern {
    Pattern::new(|state| {
        let seed = state
            .controls
            .get("randSeed")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let t = state.span.begin.to_f64();
        vec![Hap::new(None, state.span, Value::F64(perlin_at(t, seed)))]
    })
}

/// Berlin-noise value at cycle time `t`: like Perlin, but linearly ramps from
/// each integer's random "bottom" to a "top" raised by the next integer's
/// random height, then halved into 0..1 (signal.mjs `_berlin`).
pub fn berlin_at(t: f64, seed: f64) -> f64 {
    let prev = t.floor();
    let next = prev + 1.0;
    let bottom = time_to_rand(prev + seed);
    let height = time_to_rand(next + seed);
    let top = bottom + height;
    let pct = t - prev; // (t - prev) / (next - prev), and next - prev == 1
    (bottom + pct * (top - bottom)) / 2.0
}

/// Continuous Berlin-noise signal in 0..1. Reads an optional `randSeed` control.
pub fn berlin() -> Pattern {
    Pattern::new(|state| {
        let seed = state
            .controls
            .get("randSeed")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let t = state.span.begin.to_f64();
        vec![Hap::new(None, state.span, Value::F64(berlin_at(t, seed)))]
    })
}

// ---------------------------------------------------------------------------
// Event-duration signals (signal.mjs `cyclesPer`/`per`/`perx`). Unlike the
// oscillators these have no structure of their own: they report the duration of
// the query span, so they take their structure (and hence event durations) from
// whatever pattern they are combined with.

/// `cyclesPer`: the duration of each event, in cycles per event.
pub fn cycles_per() -> Pattern {
    Pattern::new(|state| {
        vec![Hap::new(
            None,
            state.span,
            Value::Frac(state.span.duration()),
        )]
    })
}

/// `per`/`perCycle`: the 'shortness' of each event, in events per cycle (the
/// reciprocal of `cyclesPer`).
pub fn per() -> Pattern {
    Pattern::new(|state| {
        let d = state.span.duration();
        vec![Hap::new(None, state.span, Value::Frac(Frac::one() / d))]
    })
}

/// `perx`: like `per`, but on an exponential (log2) curve — halving the event
/// duration raises the value by one.
pub fn perx() -> Pattern {
    Pattern::new(|state| {
        let n = Frac::one() / state.span.duration();
        vec![Hap::new(
            None,
            state.span,
            Value::F64(n.to_f64().log2() + 1.0),
        )]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rand_is_deterministic_and_in_range() {
        let r = rand();
        // sample at the midpoint of cycle 0 via segment-like query
        let v = r.query_arc(Frac::zero(), Frac::one())[0]
            .value
            .as_f64()
            .unwrap();
        assert!((0.0..1.0).contains(&v));
        // determinism: same time, same value
        let v2 = rand().query_arc(Frac::zero(), Frac::one())[0]
            .value
            .as_f64()
            .unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn perlin_in_range_and_interpolates() {
        let p = perlin();
        // At integer times, perlin equals the underlying random value.
        let at0 = p.query_arc(Frac::zero(), Frac::one())[0]
            .value
            .as_f64()
            .unwrap();
        assert_eq!(at0, time_to_rand(0.0));
        // Sampled across a cycle it stays within [0, 1) and is deterministic.
        for k in 0..16 {
            let t = Frac::new(k, 16);
            let v = perlin_at(t.to_f64(), 0.0);
            assert!((0.0..1.0).contains(&v), "perlin out of range: {v}");
        }
        // Smootherstep endpoints: f(0)=0, f(1)=1.
        assert_eq!(smoother_step(0.0), 0.0);
        assert_eq!(smoother_step(1.0), 1.0);
    }

    #[test]
    fn perlin_seed_changes_stream() {
        // A different randSeed yields a different value at the same time.
        let a = perlin_at(0.5, 0.0);
        let b = perlin_at(0.5, 7.0);
        assert_ne!(a, b);
    }

    #[test]
    fn run_counts_up() {
        let values: Vec<i64> = run(4)
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| match h.value {
                Value::Int(n) => n,
                _ => -1,
            })
            .collect();
        assert_eq!(values, vec![0, 1, 2, 3]);
    }

    #[test]
    fn scan_grows_runs_per_cycle() {
        let ints = |pat: &Pattern, c: i64| -> Vec<i64> {
            pat.query_arc(Frac::int(c), Frac::int(c + 1))
                .into_iter()
                .filter_map(|h| match h.value {
                    Value::Int(n) => Some(n),
                    _ => None,
                })
                .collect()
        };
        let pat = scan(3);
        assert_eq!(ints(&pat, 0), vec![0]); // run(1)
        assert_eq!(ints(&pat, 1), vec![0, 1]); // run(2)
        assert_eq!(ints(&pat, 2), vec![0, 1, 2]); // run(3)
        assert_eq!(ints(&pat, 3), vec![0]); // loops back to run(1)
    }

    #[test]
    fn steady_is_constant() {
        let pat = steady(Value::Int(7)).segment(Frac::int(4));
        let vals: Vec<i64> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .filter_map(|h| match h.value {
                Value::Int(n) => Some(n),
                _ => None,
            })
            .collect();
        assert_eq!(vals, vec![7, 7, 7, 7]);
    }

    fn ints(pat: &Pattern) -> Vec<i64> {
        let mut haps = pat.query_arc(Frac::zero(), Frac::one());
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter().map(|h| h.value.as_f64().unwrap() as i64).collect()
    }

    #[test]
    fn bitwise_ops_match_int32_semantics() {
        assert_eq!(ints(&pure(Value::Int(6)).band(3)), vec![2]); // 0b110 & 0b011
        assert_eq!(ints(&pure(Value::Int(5)).bor(2)), vec![7]); // 0b101 | 0b010
        assert_eq!(ints(&pure(Value::Int(5)).bxor(1)), vec![4]); // 0b101 ^ 0b001
        assert_eq!(ints(&pure(Value::Int(1)).blshift(3)), vec![8]);
        assert_eq!(ints(&pure(Value::Int(16)).brshift(2)), vec![4]);
    }

    #[test]
    fn binary_produces_msb_first_bits() {
        // binary(5): 0b101 -> "1 0 1" (nBits = floor(log2 5)+1 = 3).
        assert_eq!(ints(&binary(5)), vec![1, 0, 1]);
    }

    #[test]
    fn binary_n_matches_strudel_example() {
        // binaryN(55532, 16) == "1 1 0 1 1 0 0 0 1 1 1 0 1 1 0 0".
        assert_eq!(
            ints(&binary_n(pure(Value::Int(55532)), 16)),
            vec![1, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 0]
        );
    }

    fn list_ints(pat: &Pattern) -> Vec<i64> {
        match &pat.query_arc(Frac::zero(), Frac::one())[0].value {
            Value::List(items) => items.iter().map(|v| v.as_f64().unwrap() as i64).collect(),
            other => panic!("expected a list value, got {other:?}"),
        }
    }

    #[test]
    fn binary_list_forms_pack_bits_into_a_list() {
        // binaryNL(5, 3) and binaryL(5) both -> [1, 0, 1].
        assert_eq!(list_ints(&binary_nl(pure(Value::Int(5)), 3)), vec![1, 0, 1]);
        assert_eq!(list_ints(&binary_l(pure(Value::Int(5)))), vec![1, 0, 1]);
        // padded form keeps leading zeros.
        assert_eq!(
            list_ints(&binary_nl(pure(Value::Int(5)), 5)),
            vec![0, 0, 1, 0, 1]
        );
    }

    #[test]
    fn rand_l_is_a_list_of_n_values_in_range() {
        let haps = rand_l(4).query_arc(Frac::zero(), Frac::one());
        match &haps[0].value {
            Value::List(items) => {
                assert_eq!(items.len(), 4);
                assert!(
                    items.iter().all(|v| (0.0..1.0).contains(&v.as_f64().unwrap())),
                    "all randL values are in [0, 1)"
                );
            }
            other => panic!("expected a list value, got {other:?}"),
        }
    }

    #[test]
    fn brand_is_binary() {
        // Every sampled value is 0 or 1, and the stream is deterministic.
        let sample = || -> Vec<f64> {
            brand()
                .segment(Frac::int(8))
                .query_arc(Frac::zero(), Frac::one())
                .into_iter()
                .map(|h| h.value.as_f64().unwrap())
                .collect()
        };
        let a = sample();
        assert_eq!(a, sample());
        assert!(a.iter().all(|v| *v == 0.0 || *v == 1.0));
    }

    #[test]
    fn per_is_reciprocal_of_cycles_per() {
        // Over a half-cycle event: cyclesPer = 1/2, per = 2.
        let struct_pat = fastcat(&[pure(Value::Bool(true)), pure(Value::Bool(true))]);
        let val = |sig: Pattern| -> f64 {
            sig.struct_pat(struct_pat.clone())
                .query_arc(Frac::zero(), Frac::one())[0]
                .value
                .as_f64()
                .unwrap()
        };
        assert_eq!(val(cycles_per()), 0.5);
        assert_eq!(val(per()), 2.0);
    }
}
