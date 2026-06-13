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

use rudel_core::{
    Frac, Pattern, Value, berlin, choose, cosine, cycles_per, isaw, itri, per, perlin, perx, rand,
    saw, seq, sine, square, tri,
};

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
const TRI: [f64; 8] = [0.0, 0.25, 0.5, 0.75, 1.0, 0.75, 0.5, 0.25];
const ITRI: [f64; 8] = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75];
const BERLIN: [f64; 8] = [
    0.0,
    0.032471385318785906,
    0.06494277063757181,
    0.09741415595635772,
    0.12988554127514362,
    0.16235692659392953,
    0.19482831191271544,
    0.22729969723150134,
];
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
fn berlin_matches_strudel() {
    assert_matches("berlin", &sample(berlin()), &BERLIN);
}

#[test]
fn analytic_signals_match_strudel() {
    assert_matches("saw", &sample(saw()), &SAW);
    assert_matches("isaw", &sample(isaw()), &ISAW);
    assert_matches("sine", &sample(sine()), &SINE);
    assert_matches("cosine", &sample(cosine()), &COSINE);
    assert_matches("square", &sample(square()), &SQUARE);
    // tri = fastcat(saw, isaw) rises to 1 at the half cycle; itri is its mirror.
    assert_matches("tri", &sample(tri()), &TRI);
    assert_matches("itri", &sample(itri()), &ITRI);
}

#[test]
fn per_signals_take_structure_from_partner() {
    // per/cyclesPer have no structure of their own; struct'd over "1 1 [1 1] 1"
    // the events have durations 1/4, 1/4, [1/8, 1/8], 1/4 of a cycle.
    // cyclesPer reports those durations; per reports their reciprocals.
    let struct_pat = seq([true, true]); // simpler: two half-cycle events
    let cps: Vec<f64> = sample_struct(cycles_per(), &struct_pat);
    assert_eq!(cps, vec![0.5, 0.5]);
    let p: Vec<f64> = sample_struct(per(), &struct_pat);
    assert_eq!(p, vec![2.0, 2.0]);
    // perx: halving the duration adds one. A half-cycle event => log2(2)+1 = 2.
    let px: Vec<f64> = sample_struct(perx(), &struct_pat);
    assert_eq!(px, vec![2.0, 2.0]);
}

fn sample_struct(sig: Pattern, structure: &Pattern) -> Vec<f64> {
    // The signal provides values; the bool pattern provides structure.
    let mut haps = sig
        .struct_pat(structure.clone())
        .query_arc(Frac::zero(), Frac::one());
    haps.sort_by_key(|h| h.part.begin);
    haps.into_iter()
        .map(|h| h.value.as_f64().expect("numeric value"))
        .collect()
}

#[test]
fn choose_is_deterministic_and_in_set() {
    // choose picks continuously from the list using `rand`; sampled per segment,
    // every value must be one of the inputs and the stream is deterministic.
    let pat = choose(&[
        rudel_core::pure(Value::Int(10)),
        rudel_core::pure(Value::Int(20)),
        rudel_core::pure(Value::Int(30)),
    ]);
    let a = sample(pat.clone());
    let b = sample(pat);
    assert_eq!(a, b, "choose must be deterministic");
    for v in &a {
        assert!(
            [10.0, 20.0, 30.0].contains(v),
            "unexpected choose value {v}"
        );
    }
}

#[test]
fn seed_shifts_the_random_stream() {
    // seed(n) sets randSeed, which offsets `rand` in time, so degrade keeps a
    // different set of events than the default seed.
    let base = seq(0..N).degrade_by(0.5);
    let seeded = seq(0..N).degrade_by(0.5).seed(Frac::int(1));
    let collect = |pat: &Pattern| -> Vec<i64> {
        let mut haps = pat.query_arc(Frac::zero(), Frac::one());
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter()
            .filter_map(|h| match h.value {
                Value::Int(n) => Some(n),
                _ => None,
            })
            .collect()
    };
    assert_ne!(collect(&base), collect(&seeded));
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
