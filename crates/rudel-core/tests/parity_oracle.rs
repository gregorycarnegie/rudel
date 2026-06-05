// parity_oracle.rs — bit-for-bit parity checks against Strudel's engine.
//
// The GOLDEN values below are produced by `tools/gen_parity_oracle.mjs`, which
// re-implements Strudel's RNG and signal arithmetic verbatim from
// strudel/packages/core/signal.mjs. Re-run that script and paste its JSON here
// if the reference ever needs regenerating.
//
// Focus is the RNG-driven and continuous signals (rand/perlin/degrade plus the
// analytic oscillators), sampled at the left edge of 8 segments across cycle 0.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::{Frac, Pattern, Value, cosine, isaw, perlin, rand, saw, seq, sine, square};

const N: i64 = 8;
const EPS: f64 = 1e-12;

// --- golden reference (from tools/gen_parity_oracle.mjs) ---------------------
const RAND: [f64; 8] = [
    0.0,
    0.6852155700325966,
    0.36975969187915325,
    0.40139251574873924,
    0.2604806162416935,
    0.1356358677148819,
    0.19582648016512394,
    0.3976310808211565,
];
const PERLIN: [f64; 8] = [
    0.0,
    0.008339818690274114,
    0.05378073193423916,
    0.14298191054922427,
    0.25977108255028725,
    0.3765602545513502,
    0.46576143316633534,
    0.5112023464103004,
];
const SAW: [f64; 8] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875];
const ISAW: [f64; 8] = [1.0, 0.875, 0.75, 0.625, 0.5, 0.375, 0.25, 0.125];
const SINE: [f64; 8] = [
    0.5,
    0.8535533905932737,
    1.0,
    0.8535533905932737,
    0.5000000000000001,
    0.14644660940672627,
    0.0,
    0.14644660940672616,
];
const COSINE: [f64; 8] = [
    1.0,
    0.8535533905932737,
    0.5,
    0.14644660940672627,
    0.0,
    0.14644660940672616,
    0.4999999999999999,
    0.8535533905932737,
];
const SQUARE: [f64; 8] = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
// "0 1 .. 7".degradeBy(0.5) survivors (event indices that remain).
const DEGRADE_SURVIVORS: [i64; 1] = [1];

/// Sample a continuous signal at the left edge of `N` segments across cycle 0.
fn sample(sig: Pattern) -> Vec<f64> {
    let mut haps = sig.segment(N).query_arc(Frac::zero(), Frac::one());
    haps.sort_by_key(|h| h.part.begin);
    haps.into_iter()
        .map(|h| h.value.as_f64().expect("numeric signal value"))
        .collect()
}

fn assert_matches(name: &str, got: &[f64], want: &[f64]) {
    assert_eq!(got.len(), want.len(), "{name}: length mismatch");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(
            (g - w).abs() <= EPS,
            "{name}[{i}]: rudel={g} strudel={w} (diff {})",
            (g - w).abs()
        );
    }
}

#[test]
fn rand_matches_strudel() {
    assert_matches("rand", &sample(rand()), &RAND);
}

#[test]
fn perlin_matches_strudel() {
    assert_matches("perlin", &sample(perlin()), &PERLIN);
}

#[test]
fn analytic_signals_match_strudel() {
    assert_matches("saw", &sample(saw()), &SAW);
    assert_matches("isaw", &sample(isaw()), &ISAW);
    assert_matches("sine", &sample(sine()), &SINE);
    assert_matches("cosine", &sample(cosine()), &COSINE);
    assert_matches("square", &sample(square()), &SQUARE);
}

#[test]
fn degrade_selection_matches_strudel() {
    // seq(0..8).degradeBy(0.5): keep events whose rand value exceeds 0.5.
    let pat = seq(0..N).degrade_by(0.5);
    let mut haps = pat.query_arc(Frac::zero(), Frac::one());
    haps.sort_by_key(|h| h.part.begin);
    let survivors: Vec<i64> = haps
        .into_iter()
        .filter_map(|h| match h.value {
            Value::Int(n) => Some(n),
            _ => None,
        })
        .collect();
    assert_eq!(survivors, DEGRADE_SURVIVORS);
}
