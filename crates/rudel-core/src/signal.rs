// signal.rs - continuous signals and random generators.
// Ported from strudel/packages/core/signal.mjs.
// Copyright (C) 2024 Strudel contributors; 2026 Rudel contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::Frac;
use crate::hap::Hap;
use crate::pattern::{Pattern, fastcat, pure};
use crate::value::Value;
use std::f64::consts::PI;

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
    signal_f64(|t| (((2.0 * PI * t).sin()) + 1.0) / 2.0)
}
/// Bipolar sine -1..1.
pub fn sine2() -> Pattern {
    signal_f64(|t| (2.0 * PI * t).sin())
}
/// Cosine 0..1.
pub fn cosine() -> Pattern {
    signal_f64(|t| (((2.0 * PI * t).cos()) + 1.0) / 2.0)
}
/// Square 0..1.
pub fn square() -> Pattern {
    signal_f64(|t| ((t * 2.0).floor()).rem_euclid(2.0))
}
/// Triangle 0..1.
pub fn tri() -> Pattern {
    // fastcat(isaw, saw) gives the unipolar triangle
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
    signal_f64(|t| (2.0 * PI * t).cos())
}
/// Bipolar square -1..1.
pub fn square2() -> Pattern {
    signal_f64(|t| ((t * 2.0).floor()).rem_euclid(2.0) * 2.0 - 1.0)
}
/// Bipolar triangle -1..1.
pub fn tri2() -> Pattern {
    fastcat(&[isaw2(), saw2()])
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

/// `n` pseudo-random numbers at time `t` (legacy generator).
pub fn time_to_rands(t: f64, n: usize) -> Vec<f64> {
    let mut seed = time_to_int_seed(t);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(int_seed_to_rand(seed).abs());
        seed = xorwise(seed);
    }
    out
}

/// Continuous random signal in [0,1).
pub fn rand() -> Pattern {
    signal_f64(time_to_rand)
}

/// Continuous random signal in [-1,1).
pub fn rand2() -> Pattern {
    signal_f64(|t| time_to_rand(t) * 2.0 - 1.0)
}

/// Continuous random integers in 0..n.
pub fn irand(n: i64) -> Pattern {
    signal(move |t| Value::Int((time_to_rand(t.to_f64()) * n as f64).trunc() as i64))
}

/// Discrete pattern of numbers 0..n-1, one per step.
pub fn run(n: i64) -> Pattern {
    let pats: Vec<Pattern> = (0..n).map(|i| pure(Value::Int(i))).collect();
    fastcat(&pats)
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
}
