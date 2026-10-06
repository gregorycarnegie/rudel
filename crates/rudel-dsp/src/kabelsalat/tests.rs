// Tests for the kabelsalat back end. Split out of `kabelsalat.rs` because they
// outweigh it; a child module still sees the private ugen structs.
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use rudel_core::ValueMap;

/// A silent voice to sit under a graph that does not use `audioin`.
struct Silence;

impl VoiceLike for Silence {
    fn tick(&mut self) -> (f32, f32) {
        (0.0, 0.0)
    }
    fn is_done(&self) -> bool {
        false
    }
}

/// Build a program the way `rudel-lang`'s compiler does, so these tests
/// pin the wire format as well as the interpreter.
fn program(types: &[&str], values: &[Value], ins: &[&[Value]], outs: &[(i64, i64)]) -> Value {
    let mut map = ValueMap::new();
    map.insert(
        "types".to_string(),
        Value::List(types.iter().map(|t| Value::Str(t.to_string())).collect()),
    );
    map.insert("values".to_string(), Value::List(values.to_vec()));
    map.insert(
        "ins".to_string(),
        Value::List(ins.iter().map(|i| Value::List(i.to_vec())).collect()),
    );
    map.insert(
        "outs".to_string(),
        Value::List(
            outs.iter()
                .map(|(r, c)| Value::List(vec![Value::Int(*r), Value::Int(*c)]))
                .collect(),
        ),
    );
    Value::Map(map)
}

fn voice(program: Value) -> KabelVoice {
    KabelVoice::new(
        KabelProgram::from_value(&program).expect("a well-formed program"),
        Box::new(Silence),
        48000.0,
        440.0,
        Vec::new(),
        1.0,
        1.0,
    )
}

#[test]
fn a_sine_graph_oscillates_at_the_frequency_it_was_given() {
    // sine(4800).mul(0.5).out() — 4800Hz at 48kHz is ten samples a cycle,
    // so a quarter cycle in the phase is two and a half samples.
    let value = program(
        &["n", "sine", "n", "mul"],
        &[
            Value::F64(4800.0),
            Value::Null,
            Value::F64(0.5),
            Value::Null,
        ],
        &[
            &[],
            &[Value::Int(0), Value::F64(0.0), Value::F64(0.0)],
            &[],
            &[Value::Int(1), Value::Int(2)],
        ],
        &[(3, 0), (3, 1)],
    );
    let mut voice = voice(value);
    let samples: Vec<f32> = (0..10).map(|_| voice.tick().0).collect();

    // Upstream reads the phase before advancing it, so the first sample is
    // sin(0) and the peak lands a quarter cycle later.
    assert_eq!(samples[0], 0.0);
    assert!(
        (samples[2] - 0.5 * (0.2 * TAU).sin()).abs() < 1e-6,
        "{samples:?}"
    );
    // Both channels carry it, and the `mul` really did scale.
    assert!(samples.iter().all(|s| s.abs() <= 0.5 + 1e-6), "{samples:?}");
    assert!(samples.iter().any(|s| s.abs() > 0.4), "{samples:?}");
}

#[test]
fn an_unported_node_type_passes_its_input_through() {
    // `scope` has no interpreter here: it posts buffers to a UI there is
    // none of. Upstream's compiler falls back to `thru` for a type it does
    // not know, so a patch using one still plays; this pins that a gap
    // degrades rather than silences.
    let value = program(
        &["n", "scope"],
        &[Value::F64(0.25), Value::Null],
        &[&[], &[Value::Int(0)]],
        &[(1, 0), (1, 1)],
    );
    let mut voice = voice(value);
    assert_eq!(voice.tick(), (0.25, 0.25));
}

#[test]
fn a_missing_inlet_falls_back_to_the_literal_the_compiler_wrote() {
    // `pulse(freq)` with no width: the compiler pads the second inlet with
    // kabelsalat's own default of 0.5, so the wave is square.
    let high = |width: Option<f64>| {
        let mut ins = vec![Value::Int(0)];
        ins.push(width.map_or(Value::F64(0.5), Value::F64));
        let value = program(
            &["n", "pulse"],
            &[Value::F64(4800.0), Value::Null],
            &[&[], &ins],
            &[(1, 0), (1, 1)],
        );
        let mut voice = voice(value);
        let samples: Vec<f32> = (0..200).map(|_| voice.tick().0).collect();
        assert!(samples.iter().all(|s| s.abs() == 1.0), "{samples:?}");
        samples.iter().filter(|s| **s > 0.0).count()
    };
    // Not an exact count: the phase accumulator is never wrapped, here or
    // upstream, so which side of the boundary a sample lands on drifts.
    // What has to hold is that the padded inlet really is 0.5 — half the
    // samples high, and more of them than an explicit quarter gives.
    // `kabelsalat_parity` is what pins the waveform sample for sample.
    let default = high(None);
    assert!((90..=110).contains(&default), "{default} of 200 high");
    assert!(high(Some(0.25)) < default);
}

#[test]
fn a_feedback_loop_reads_the_previous_samples_output() {
    // `src(0)` reads output channel 0 as it stood one sample ago, which is
    // what makes a cyclic graph work. Feeding it back at unity gain, plus a
    // constant 1, counts up by one per sample.
    let value = program(
        &["n", "src", "n", "add"],
        &[Value::F64(0.0), Value::Null, Value::F64(1.0), Value::Null],
        &[&[], &[Value::Int(0)], &[], &[Value::Int(1), Value::Int(2)]],
        &[(3, 0)],
    );
    let mut voice = voice(value);
    let samples: Vec<f32> = (0..4).map(|_| voice.tick().0).collect();
    assert_eq!(samples, vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn audioin_reads_the_voice_the_graph_wraps() {
    struct Dc;
    impl VoiceLike for Dc {
        fn tick(&mut self) -> (f32, f32) {
            (1.0, 1.0)
        }
        fn is_done(&self) -> bool {
            false
        }
    }
    // audioin().mul(0.25).out() — `K(...)` used as an insert effect.
    let value = program(
        &["audioin", "n", "mul"],
        &[Value::Null, Value::F64(0.25), Value::Null],
        &[&[], &[], &[Value::Int(0), Value::Int(1)]],
        &[(2, 0), (2, 1)],
    );
    let mut voice = KabelVoice::new(
        KabelProgram::from_value(&value).expect("a well-formed program"),
        Box::new(Dc),
        48000.0,
        440.0,
        Vec::new(),
        1.0,
        1.0,
    );
    assert_eq!(voice.tick(), (0.25, 0.25));
}

#[test]
fn the_gate_falls_when_the_haps_own_span_ends() {
    // `sGate` is 1 while the note is held and 0 after, which is how an
    // `adsr` inside a graph knows to release.
    let value = program(&["sgate"], &[Value::Null], &[&[]], &[(0, 0)]);
    let mut voice = KabelVoice::new(
        KabelProgram::from_value(&value).expect("a well-formed program"),
        Box::new(Silence),
        4.0,
        440.0,
        Vec::new(),
        0.5,
        1.0,
    );
    let gates: Vec<f32> = (0..4).map(|_| voice.tick().0).collect();
    assert_eq!(gates, vec![1.0, 1.0, 0.0, 0.0]);
    assert!(voice.is_done());
}

#[test]
fn a_pattern_written_inside_the_graph_reaches_it_as_a_value() {
    // `K(sine(S("220 440")).out())` samples the pattern per hap and hands
    // the graph the number, where upstream splices it into the source text.
    let value = program(&["pat"], &[Value::Int(1)], &[&[]], &[(0, 0)]);
    let mut voice = KabelVoice::new(
        KabelProgram::from_value(&value).expect("a well-formed program"),
        Box::new(Silence),
        48000.0,
        440.0,
        vec![110.0, 220.0],
        1.0,
        1.0,
    );
    assert_eq!(voice.tick().0, 220.0);
}

// ---------------------------------------------------------------------------
// A one-node harness
//
// Most ugens are reachable only through `tick`'s match, so the cheapest way to
// drive one is a program holding exactly that node with every inlet a literal.

/// One node of `kind`, every inlet a literal, its output on channel 0.
fn node_at(rate: f32, kind: &str, value: Value, ins: &[f32]) -> KabelVoice {
    let literals: Vec<Value> = ins.iter().map(|v| Value::F64(f64::from(*v))).collect();
    let value = program(&[kind], &[value], &[literals.as_slice()], &[(0, 0)]);
    KabelVoice::new(
        KabelProgram::from_value(&value).expect("a well-formed program"),
        Box::new(Silence),
        rate,
        440.0,
        Vec::new(),
        1.0,
        1.0,
    )
}

/// `n` samples out of a one-node graph at the given rate. A low rate makes the
/// phase steps exact binary fractions, so a boundary comparison can be pinned.
fn run_at(rate: f32, kind: &str, ins: &[f32], n: usize) -> Vec<f32> {
    let mut voice = node_at(rate, kind, Value::Null, ins);
    (0..n).map(|_| voice.tick().0).collect()
}

fn run(kind: &str, ins: &[f32], n: usize) -> Vec<f32> {
    run_at(48000.0, kind, ins, n)
}

/// The first sample out of a one-node graph — enough for a stateless node.
fn once(kind: &str, ins: &[f32]) -> f32 {
    run(kind, ins, 1)[0]
}

/// Assert two floats agree to within a tolerance a mutated operator could not
/// slip through.
#[track_caller]
fn close(got: f32, want: f32) {
    assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
}

// ---------------------------------------------------------------------------
// The pure helpers

#[test]
fn js_round_sends_halves_up_where_rust_sends_them_away_from_zero() {
    assert_eq!(js_round(0.5), 1.0);
    assert_eq!(js_round(2.5), 3.0);
    // The difference: Rust's `f32::round` gives -3.0 here.
    assert_eq!(js_round(-2.5), -2.0);
    assert_eq!(js_round(-0.5), 0.0);
    assert_eq!(js_round(1.4), 1.0);
}

#[test]
fn js_sign_gives_zero_for_zero_where_signum_gives_one() {
    assert_eq!(js_sign(2.0), 1.0);
    assert_eq!(js_sign(-2.0), -1.0);
    assert_eq!(js_sign(0.0), 0.0);
    assert_eq!(once("sign", &[-3.0]), -1.0);
}

#[test]
fn lerp_clamps_at_the_top_but_not_the_bottom() {
    assert_eq!(lerp(0.5, 0.0, 10.0), 5.0);
    assert_eq!(lerp(1.0, 0.0, 10.0), 10.0);
    // Past the end it holds; before the start it keeps extrapolating, which is
    // what `synth.js` does and what an adsr's release depends on.
    assert_eq!(lerp(2.0, 0.0, 10.0), 10.0);
    assert_eq!(lerp(-1.0, 0.0, 10.0), -10.0);
}

#[test]
fn inv_lerp_clamps_at_both_ends() {
    assert_eq!(inv_lerp(0.5, 0.0, 1.0), 0.5);
    assert_eq!(inv_lerp(3.0, 2.0, 6.0), 0.25);
    assert_eq!(inv_lerp(-1.0, 0.0, 1.0), 0.0);
    assert_eq!(inv_lerp(2.0, 0.0, 1.0), 1.0);
}

#[test]
fn distort_curves_harder_as_the_amount_rises() {
    close(distort(0.5, 0.5), 0.745);
    close(distort(0.5, 2.0), 0.995);
    // Clamped at the bottom too, where the -0.01 leaves the curve just under
    // unity gain.
    close(distort(0.5, -1.0), 0.495);
    assert_eq!(distort(0.0, 0.5), 0.0);
}

#[test]
fn fold_leaves_the_input_alone_until_the_rate_pushes_it_over() {
    // rate 0 is unity: the fold's first segment is the identity.
    close(fold(0.5, 0.0), 0.5);
    close(fold(1.0, 0.0), 1.0);
    // rate 1 doubles the input, and 3.0 folds all the way back down past zero.
    close(fold(1.5, 1.0), -1.0);
    // Past the first corner it really folds back: 1.4 comes down to 0.6.
    close(fold(1.4, 0.0), 0.6);
    // A negative rate is clamped to 0, so it is the identity again.
    close(fold(0.5, -5.0), 0.5);
}

#[test]
fn poly_blep_corrects_only_within_one_step_of_the_discontinuity() {
    close(poly_blep(0.05, 0.1) as f32, -0.25);
    close(poly_blep(0.95, 0.1) as f32, 0.25);
    assert_eq!(poly_blep(0.5, 0.1), 0.0);
}

#[test]
fn a_variadic_node_folds_its_inputs_left_to_right() {
    assert_eq!(fold_args(&[], |x, y| x + y), 0.0);
    assert_eq!(fold_args(&[5.0], |x, y| x + y), 5.0);
    assert_eq!(fold_args(&[1.0, 2.0, 3.0], |x, y| x - y), -4.0);
}

#[test]
fn a_missing_inlet_reads_as_zero_or_as_the_ugens_own_default() {
    assert_eq!(at(&[7.0], 0), 7.0);
    assert_eq!(at(&[7.0], 1), 0.0);
    assert_eq!(at2(&[7.0], 0, 500.0), 7.0);
    assert_eq!(at2(&[7.0], 1, 500.0), 500.0);
}

#[test]
fn argmin_and_argmax_keep_the_later_index_on_a_tie() {
    assert_eq!(arg_extreme(&[5.0, 2.0, 9.0], |x, y| x < y), 1.0);
    assert_eq!(arg_extreme(&[5.0, 2.0, 9.0], |x, y| x > y), 2.0);
    // Upstream's reduce yields `b` when the two are equal.
    assert_eq!(arg_extreme(&[3.0, 1.0, 1.0], |x, y| x < y), 2.0);
    assert_eq!(arg_extreme(&[1.0, 3.0, 3.0], |x, y| x > y), 2.0);
}

#[test]
fn pick_wraps_its_index_and_tolerates_a_negative_one() {
    let inputs = [7.0, 8.0, 9.0];
    assert_eq!(pick(0.0, &inputs), 7.0);
    assert_eq!(pick(2.0, &inputs), 9.0);
    assert_eq!(pick(4.0, &inputs), 8.0);
    // The double modulo is what carries these two.
    assert_eq!(pick(-1.0, &inputs), 9.0);
    assert_eq!(pick(-3.0, &inputs), 7.0);
    assert_eq!(pick(1.5, &inputs), 8.0);
    assert_eq!(pick(0.0, &[]), 0.0);
}

// ---------------------------------------------------------------------------
// The ugen state
//
// The noise sources are seeded per node type rather than from `Math.random()`,
// which is the one thing about them a golden test can pin. The numbers below
// are this port's, captured once; `kabelsalat_parity` in rudel-lang is what
// checks the waveforms against upstream.

#[test]
fn the_rng_is_xorshift32_and_repeats_from_its_seed() {
    let mut rng = Rng(1);
    assert_eq!(rng.next(), 6.295019e-5);
    assert_eq!(rng.0, 270369);
    // `bipolar` is `next() * 2 - 1`, so it spans -1..1 where `next` spans 0..1.
    let mut rng = Rng(0x2545_f491);
    assert_eq!(rng.bipolar(), 0.7589328);
    assert_eq!(rng.bipolar(), 0.090651155);
    assert!(Rng(7).next() < 1.0);
}

#[test]
fn a_phase_accumulator_advances_before_it_is_read_and_wraps_at_one() {
    let mut phase = Phase::default();
    assert_eq!(phase.step(1.0, 0.25), 0.25);
    assert_eq!(phase.step(1.0, 0.25), 0.5);
    assert_eq!(phase.step(1.0, 0.25), 0.75);
    // The accumulator itself is never wrapped — only the reading is.
    assert_eq!(phase.step(1.0, 0.25), 0.0);
    assert_eq!(phase.step(1.0, 0.25), 0.25);
    assert_eq!(phase.0, 1.25);
}

#[test]
fn a_sine_reads_its_phase_offset_before_advancing() {
    // 6000Hz at 48kHz is an eighth of a cycle a sample; a quarter-cycle offset
    // puts the first sample at the peak.
    let dt = 1.0 / 48000.0;
    let mut osc = SineOsc::default();
    let got: Vec<f32> = (0..4).map(|_| osc.update(6000.0, 0.0, 0.25, dt)).collect();
    // A quarter turn apart each sample: peak, half-root-two, zero, and back
    // down. The zero is a rounding residue rather than an exact one.
    let half_root_2 = std::f32::consts::FRAC_1_SQRT_2;
    assert_eq!(got[0], 1.0);
    close(got[1], half_root_2);
    assert_eq!(got[2], -7.059922e-8);
    close(got[3], -half_root_2);
}

#[test]
fn a_sine_hard_syncs_on_the_rising_edge_of_its_sync_input() {
    let dt = 1.0 / 48000.0;
    let mut osc = SineOsc::default();
    osc.update(6000.0, 0.0, 0.0, dt);
    osc.update(6000.0, 0.0, 0.0, dt);
    // Two samples in, the phase is a quarter cycle; a rising sync puts it back
    // to zero, so the reading is sin(0).
    assert_eq!(osc.update(6000.0, 1.0, 0.0, dt), 0.0);
    // Held high, it does not resync — the oscillator runs on.
    assert_eq!(
        osc.update(6000.0, 1.0, 0.0, dt),
        std::f32::consts::FRAC_1_SQRT_2
    );
}

#[test]
fn a_saw_is_blep_corrected_at_the_jump_and_wraps_its_phase() {
    let mut osc = SawOsc::default();
    let got: Vec<f32> = (0..8).map(|_| osc.update(8000.0, 48000.0)).collect();
    // Six samples a cycle: the ramp climbs by a third each sample, and the
    // polyBLEP rounds off the sample either side of the discontinuity.
    assert_eq!(
        got,
        vec![
            0.0,
            -0.6666667,
            -0.33333334,
            0.0,
            0.33333334,
            0.6666667,
            1.110223e-15,
            -0.6666667,
        ]
    );
}

#[test]
fn an_impulse_fires_one_sample_high_per_cycle_and_starts_immediately() {
    // Rate 4 and 1Hz make the phase step an exact quarter, so the sample that
    // lands exactly on the cycle boundary is the one that fires.
    assert_eq!(
        run_at(4.0, "impulse", &[1.0], 8),
        vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
    );
    // Two cycles a second is twice as often, which pins the phase step as a
    // product of the rate and the frequency rather than a ratio of them.
    assert_eq!(run_at(4.0, "impulse", &[2.0], 4), vec![1.0, 1.0, 0.0, 1.0]);
}

#[test]
fn a_clock_divider_counts_both_edges_and_flips_at_the_factor() {
    let mut div = ClockDiv::default();
    // Factor 2: two edges — one falling, one rising — per flip.
    let got: Vec<f32> = [-1.0, 1.0, -1.0, 1.0, -1.0, 1.0]
        .iter()
        .map(|clock| div.update(*clock, 2.0))
        .collect();
    assert_eq!(got, vec![1.0, -1.0, -1.0, 1.0, 1.0, -1.0]);
    // Factor 1 flips on every edge instead.
    let mut div = ClockDiv::default();
    let got: Vec<f32> = [-1.0, 1.0, -1.0]
        .iter()
        .map(|clock| div.update(*clock, 1.0))
        .collect();
    assert_eq!(got, vec![-1.0, 1.0, -1.0]);
    // A clock sitting exactly at zero reads as low, so the next rise is an
    // edge the divider counts.
    let mut div = ClockDiv::default();
    assert_eq!(div.update(0.0, 2.0), 1.0);
    assert_eq!(div.update(1.0, 2.0), -1.0);
}

#[test]
fn noise_only_redraws_when_its_input_is_truthy() {
    let mut noise = Noise::default();
    let first = noise.update(0.0);
    assert_eq!(first, 0.7589328);
    // Zero holds — this is what makes `noise` double as a sample-and-hold.
    assert_eq!(noise.update(0.0), first);
    assert_eq!(noise.update(1.0), 0.090651155);
}

#[test]
fn pink_noise_runs_white_through_the_kellett_filter_bank() {
    let mut pink = Pink::default();
    let got: Vec<f32> = (0..3).map(|_| pink.update()).collect();
    assert_eq!(got, vec![-0.06646299, 0.09700737, 0.047560986]);
}

#[test]
fn brown_noise_leaks_towards_zero() {
    let mut brown = Brown::default();
    let got: Vec<f32> = (0..3).map(|_| brown.update()).collect();
    assert_eq!(got, vec![2.218228e-5, -0.004823687, -0.004951498]);
}

#[test]
fn dust_fires_at_random_and_is_scaled_by_the_hardcoded_rate() {
    let mut dust = Dust::default();
    // `ISR` is 1/48000 whatever the context runs at, as upstream — so density
    // 20000 fires on roughly two samples in eight here.
    let got: Vec<f32> = (0..8).map(|_| dust.update(20000.0)).collect();
    assert_eq!(
        got,
        vec![0.0, 0.8356378, 0.0, 0.0, 0.67371345, 0.0, 0.0, 0.0]
    );
    // Density zero never fires.
    let mut dust = Dust::default();
    assert!((0..64).all(|_| dust.update(0.0) == 0.0));
}

#[test]
fn lcg_noise_is_deterministic_and_resettable() {
    let mut lcg = LcgNoise::new(0);
    let first = lcg.update(1.0, 0.0);
    assert_eq!(first, -0.6680714);
    assert_eq!(lcg.update(1.0, 0.0), -0.6623032);
    // A truthy reset winds the generator back to its seed.
    assert_eq!(lcg.update(1.0, 1.0), first);
    // A zero trigger holds the last value instead of drawing.
    assert_eq!(lcg.update(0.0, 0.0), first);
    // The channel scales the seed, so a second node starts elsewhere.
    assert_ne!(LcgNoise::new(1).update(1.0, 0.0), first);
}

#[test]
fn the_two_pole_filter_clamps_its_inputs_and_carries_both_outputs() {
    let mut filter = TwoPole::default();
    filter.apply(1.0, 0.5, 0.25);
    // s0 is the band-pass and s1 the low-pass, one recurrence apart.
    assert_eq!((filter.s0, filter.s1), (0.0625, 0.00390625));
    filter.apply(1.0, 0.5, 0.25);
    assert_eq!((filter.s0, filter.s1), (0.12426758, 0.011642456));
    // An input other than unity, so the term the input is scaled by is a
    // product rather than something a division would agree with.
    let mut filter = TwoPole::default();
    filter.apply(2.0, 0.5, 0.25);
    assert_eq!((filter.s0, filter.s1), (0.125, 0.0078125));
    // Out-of-range cutoff and resonance are clamped rather than producing NaN.
    let mut filter = TwoPole::default();
    filter.apply(1.0, 5.0, -3.0);
    assert_eq!((filter.s0, filter.s1), (1.0, 1.0));
}

#[test]
fn a_delay_reads_the_line_at_the_distance_its_time_input_asks_for() {
    let mut delay = Delay::default();
    let got: Vec<f32> = [1.0, 2.0, 3.0, 4.0, 5.0]
        .iter()
        .map(|input| delay.update(*input, 0.5, 4.0))
        .collect();
    // Half a second at 4Hz is two samples.
    assert_eq!(got, vec![0.0, 0.0, 1.0, 2.0, 3.0]);
    assert_eq!(delay.buffer.len(), (MAX_DELAY_TIME * 4.0) as usize);
    // A zero — or negative — time reads what was just written.
    let mut delay = Delay::default();
    assert_eq!(delay.update(9.0, -1.0, 4.0), 9.0);
    // Past the end of the line the distance clamps to one sample short of the
    // buffer, which keeps the read behind the write rather than in front of it.
    let mut delay = Delay::default();
    assert_eq!(delay.update(5.0, 1000.0, 4.0), 0.0);
    assert_eq!(delay.update(6.0, 1000.0, 4.0), 0.0);
}

#[test]
fn hold_latches_on_the_rising_edge_of_its_trigger() {
    let mut hold = Hold::default();
    assert_eq!(hold.update(1.0, 0.0), 0.0);
    assert_eq!(hold.update(1.0, 1.0), 1.0);
    // Held high, a new input does not get through.
    assert_eq!(hold.update(2.0, 1.0), 1.0);
    assert_eq!(hold.update(2.0, 0.0), 1.0);
    assert_eq!(hold.update(2.0, 1.0), 2.0);
}

#[test]
fn a_sequence_steps_on_each_rising_clock_and_blanks_the_first_sample() {
    let mut seq = Sequence::default();
    let steps = [10.0, 20.0, 30.0];
    // It starts believing the clock is already high, so the first low reading
    // arms it without advancing.
    assert_eq!(seq.update(-1.0, &steps), 10.0);
    // The rising edge advances, and returns zero once so a gate retriggers.
    assert_eq!(seq.update(1.0, &steps), 0.0);
    assert_eq!(seq.update(1.0, &steps), 20.0);
    assert_eq!(seq.update(-1.0, &steps), 20.0);
    assert_eq!(seq.update(1.0, &steps), 0.0);
    assert_eq!(seq.update(1.0, &steps), 30.0);
    // And it wraps.
    seq.update(-1.0, &steps);
    seq.update(1.0, &steps);
    assert_eq!(seq.update(1.0, &steps), 10.0);
    // A clock resting at exactly zero is low, so the rise after it is a real
    // edge rather than one the sequence has already counted.
    let mut seq = Sequence::default();
    let got: Vec<f32> = [-1.0, 0.0, 1.0]
        .iter()
        .map(|clock| seq.update(*clock, &steps))
        .collect();
    assert_eq!(got, vec![10.0, 10.0, 0.0]);
    // Held high it advances once and then holds, however long it stays up.
    let mut seq = Sequence::default();
    seq.update(-1.0, &steps);
    let got: Vec<f32> = (0..3).map(|_| seq.update(1.0, &steps)).collect();
    assert_eq!(got, vec![0.0, 20.0, 20.0]);
    // No steps at all is silence, not a panic.
    assert_eq!(seq.update(1.0, &[]), 0.0);
}

#[test]
fn the_adsr_walks_attack_decay_sustain_and_release() {
    let mut env = AdsrEnv::default();
    // Off until the gate opens.
    assert_eq!(env.eval(0.0, 0.0, 1.0, 1.0, 0.5, 1.0), 0.0);
    assert_eq!(env.state, EnvState::Off);
    // Attack ramps from where it stood to 1 over the attack time.
    assert_eq!(env.eval(0.0, 1.0, 1.0, 1.0, 0.5, 1.0), 0.0);
    assert_eq!(env.state, EnvState::Attack);
    close(env.eval(0.5, 1.0, 1.0, 1.0, 0.5, 1.0), 0.5);
    // Past the attack it hands over to decay at full height.
    assert_eq!(env.eval(1.5, 1.0, 1.0, 1.0, 0.5, 1.0), 1.0);
    assert_eq!(env.state, EnvState::Decay);
    // Decay falls from 1 to the sustain level.
    close(env.eval(2.0, 1.0, 1.0, 1.0, 0.5, 1.0), 0.75);
    assert_eq!(env.eval(3.0, 1.0, 1.0, 1.0, 0.5, 1.0), 0.5);
    assert_eq!(env.state, EnvState::Sustain);
    // Sustain holds until the gate falls.
    assert_eq!(env.eval(4.0, 1.0, 1.0, 1.0, 0.5, 2.0), 0.5);
    assert_eq!(env.eval(5.0, 1.0, 1.0, 1.0, 0.5, 2.0), 0.5);
    assert_eq!(env.state, EnvState::Sustain);
    // The release runs from where sustain left it, over the release time.
    assert_eq!(env.eval(5.0, 0.0, 1.0, 1.0, 0.5, 2.0), 0.5);
    assert_eq!(env.state, EnvState::Release);
    close(env.eval(5.5, 0.0, 1.0, 1.0, 0.5, 2.0), 0.375);
    assert_eq!(env.eval(8.0, 0.0, 1.0, 1.0, 0.5, 2.0), 0.0);
    assert_eq!(env.state, EnvState::Off);
}

#[test]
fn an_envelope_opened_late_measures_from_when_its_gate_arrived() {
    // Every stage times from `start_time`, so an envelope that opens well into
    // the voice has to measure the difference rather than the clock.
    let mut env = AdsrEnv::default();
    assert_eq!(env.eval(10.0, 1.0, 1.0, 1.0, 0.5, 1.0), 0.0);
    close(env.eval(10.5, 1.0, 1.0, 1.0, 0.5, 1.0), 0.5);
    assert_eq!(env.state, EnvState::Attack);
}

#[test]
fn the_adsr_releases_out_of_the_decay_and_retriggers_out_of_the_release() {
    let mut env = AdsrEnv::default();
    env.eval(0.0, 1.0, 1.0, 1.0, 0.5, 1.0);
    env.eval(1.5, 1.0, 1.0, 1.0, 0.5, 1.0);
    // A gate that falls mid-decay releases from the level it had reached, not
    // from the sustain level.
    let value = env.eval(2.0, 0.0, 1.0, 1.0, 0.5, 2.0);
    close(value, 0.75);
    assert_eq!(env.state, EnvState::Release);
    assert_eq!(env.start_value, value);
    // A gate arriving during the release attacks again from where it is.
    let value = env.eval(2.5, 1.0, 1.0, 1.0, 0.5, 2.0);
    close(value, 0.5625);
    assert_eq!(env.state, EnvState::Attack);
    assert_eq!(env.start_value, value);
}

#[test]
fn the_biquad_coefficients_follow_the_cookbook_for_each_kind() {
    let coefficients = |kind| biquad_coefficients(kind, 1000.0, 3.0, 6.0, 48000.0);
    // 0 (and anything unrecognised) is a low-pass, 1 high-pass, 2 band-pass,
    // 3 notch, 4 all-pass, 5 peaking, 6 low-shelf, 7 high-shelf.
    assert_eq!(
        coefficients(0),
        [0.00408865, 0.0081773, 0.00408865, -1.8953207, 0.91167533]
    );
    assert_eq!(coefficients(99), coefficients(0));
    assert_eq!(
        coefficients(1),
        [0.95174897, -1.9034979, 0.95174897, -1.8953207, 0.91167533]
    );
    assert_eq!(
        coefficients(2),
        [0.06238093, 0.0, -0.06238093, -1.8953207, 0.91167533]
    );
    assert_eq!(
        coefficients(3),
        [0.95583767, -1.8953207, 0.95583767, -1.8953207, 0.91167533]
    );
    assert_eq!(
        coefficients(4),
        [0.91167533, -1.8953207, 1.0, -1.8953207, 0.91167533]
    );
    assert_eq!(
        coefficients(5),
        [1.0315231, -1.9200857, 0.905131, -1.9200857, 0.936654]
    );
    assert_eq!(
        coefficients(6),
        [1.0183604, -1.9075866, 0.9125187, -1.9133959, 0.92506975]
    );
    assert_eq!(
        coefficients(7),
        [1.9592891, -3.7488956, 1.812479, -1.873194, 0.8960665]
    );
}

#[test]
fn the_biquad_carries_two_samples_of_input_and_output_history() {
    // A step of 2 rather than 1, so the input is scaled by b0 rather than
    // merely agreeing with it.
    let mut doubled = Biquad::default();
    assert_eq!(
        doubled.update(2.0, 0.0, 1000.0, 3.0, 6.0, 48000.0),
        0.0081773
    );

    let mut filter = Biquad::default();
    let got: Vec<f32> = (0..3)
        .map(|_| filter.update(1.0, 0.0, 1000.0, 3.0, 6.0, 48000.0))
        .collect();
    // A step into a low-pass climbs; the first sample is b0 alone.
    assert_eq!(got, vec![0.00408865, 0.020015253, 0.0505624]);
    assert_eq!((filter.x1, filter.x2), (1.0, 1.0));
    assert_eq!((filter.y1, filter.y2), (0.0505624, 0.020015253));
}

#[test]
fn a_coded_node_reads_its_expression_as_bytes_or_as_a_raw_signal() {
    // `bytebeat`'s `t` is the node's own input, and the result is folded from
    // 0..255 into -1..1.
    let mut coded = Coded::new("t*3", CodedKind::Byte);
    assert_eq!(coded.update(1.0, 0.0, 0.0), -0.9764706);
    // 300 wraps to 44 through the mask.
    assert_eq!(coded.update(100.0, 0.0, 0.0), -0.654902);
    // `raw` reads the node's input as `$input` and passes the value straight
    // out.
    let mut raw = Coded::new("$input*2", CodedKind::Raw);
    assert_eq!(raw.update(0.0, 0.25, 0.0), 0.5);
}

// ---------------------------------------------------------------------------
// The instruction set
//
// One node per test, its inlets literals, so each match arm in `tick` is
// pinned by the value it produces rather than by a whole patch.

#[test]
fn the_variadic_arithmetic_nodes_fold_every_inlet() {
    assert_eq!(once("add", &[1.0, 2.0, 3.0]), 6.0);
    assert_eq!(once("sub", &[1.0, 2.0, 3.0]), -4.0);
    assert_eq!(once("mul", &[2.0, 3.0, 4.0]), 24.0);
    assert_eq!(once("div", &[12.0, 2.0, 3.0]), 2.0);
    assert_eq!(once("mod", &[7.0, 4.0, 2.0]), 1.0);
    assert_eq!(once("min", &[3.0, 1.0, 2.0]), 1.0);
    assert_eq!(once("max", &[3.0, 1.0, 2.0]), 3.0);
    // `mix` and `poly` sum, like `add`.
    assert_eq!(once("mix", &[1.0, 2.0, 3.0]), 6.0);
    assert_eq!(once("poly", &[1.0, 2.0]), 3.0);
    assert_eq!(once("argmin", &[5.0, 2.0, 9.0]), 1.0);
    assert_eq!(once("argmax", &[5.0, 2.0, 9.0]), 2.0);
    // A tie keeps the later index, as upstream's reduce does.
    assert_eq!(once("argmin", &[3.0, 1.0, 1.0]), 2.0);
    assert_eq!(once("argmax", &[1.0, 3.0, 3.0]), 2.0);
}

#[test]
fn the_unary_math_nodes_follow_javascript_rather_than_rust() {
    assert_eq!(once("abs", &[-2.5]), 2.5);
    assert_eq!(once("floor", &[-2.5]), -3.0);
    assert_eq!(once("ceil", &[-2.5]), -2.0);
    // `Math.round` sends halves up.
    assert_eq!(once("round", &[-2.5]), -2.0);
    // Truthiness, so only exact zero is false.
    assert_eq!(once("not", &[0.0]), 1.0);
    assert_eq!(once("not", &[0.5]), 0.0);
    assert_eq!(once("bool", &[0.0]), 0.0);
    assert_eq!(once("bool", &[-0.5]), 1.0);
    close(once("log", &[std::f32::consts::E]), 1.0);
    close(once("exp", &[1.0]), std::f32::consts::E);
    close(once("sin", &[0.0]), 0.0);
    close(once("cos", &[0.0]), 1.0);
    close(once("tan", &[0.0]), 0.0);
    close(once("asin", &[1.0]), std::f32::consts::FRAC_PI_2);
    close(once("acos", &[1.0]), 0.0);
    close(once("atan", &[1.0]), std::f32::consts::FRAC_PI_4);
}

#[test]
fn midinote_converts_a_note_number_to_hertz() {
    assert_eq!(once("midinote", &[69.0]), 440.0);
    // An octave up is a doubling — which pins the divisor as well as the base.
    assert_eq!(once("midinote", &[81.0]), 880.0);
    assert_eq!(once("midinote", &[57.0]), 220.0);
}

#[test]
fn the_binary_and_ternary_nodes_use_javascript_truthiness() {
    assert_eq!(once("pow", &[2.0, 10.0]), 1024.0);
    assert_eq!(once("greater", &[2.0, 1.0]), 1.0);
    assert_eq!(once("greater", &[1.0, 2.0]), 0.0);
    assert_eq!(once("lower", &[1.0, 2.0]), 1.0);
    assert_eq!(once("lower", &[2.0, 1.0]), 0.0);
    // Neither is true of two equal operands.
    assert_eq!(once("greater", &[1.0, 1.0]), 0.0);
    assert_eq!(once("lower", &[1.0, 1.0]), 0.0);
    assert_eq!(once("xor", &[1.0, 1.0]), 0.0);
    assert_eq!(once("xor", &[1.0, 0.0]), 1.0);
    // Both sides have to be non-zero for `and`, either for `or`.
    assert_eq!(once("and", &[2.0, 3.0]), 1.0);
    assert_eq!(once("and", &[2.0, 0.0]), 0.0);
    assert_eq!(once("and", &[0.0, 3.0]), 0.0);
    assert_eq!(once("or", &[0.0, 0.0]), 0.0);
    assert_eq!(once("or", &[0.0, 3.0]), 1.0);
    assert_eq!(once("or", &[2.0, 0.0]), 1.0);
}

#[test]
fn ifelse_tests_for_exactly_one_rather_than_for_truth() {
    assert_eq!(once("ifelse", &[1.0, 7.0, 9.0]), 7.0);
    assert_eq!(once("ifelse", &[0.0, 7.0, 9.0]), 9.0);
    // Truthy but not 1 takes the else branch, as upstream's `=== 1` does.
    assert_eq!(once("ifelse", &[2.0, 7.0, 9.0]), 9.0);
}

#[test]
fn clamp_and_clip_bound_a_signal_either_way_round() {
    assert_eq!(once("clamp", &[5.0, 0.0, 1.0]), 1.0);
    assert_eq!(once("clamp", &[-5.0, 0.0, 1.0]), 0.0);
    assert_eq!(once("clamp", &[0.5, 0.0, 1.0]), 0.5);
    // `clamp` sorts its bounds; `clip` takes them in order.
    assert_eq!(once("clamp", &[0.5, 1.0, 0.0]), 0.5);
    assert_eq!(once("clip", &[5.0, 0.0, 1.0]), 1.0);
    assert_eq!(once("clip", &[-5.0, 0.0, 1.0]), 0.0);
}

#[test]
fn range_scales_a_bipolar_signal_and_takes_a_curve_as_a_fourth_inlet() {
    // -1..1 into 2..4: the midpoint lands in the middle.
    assert_eq!(once("range", &[0.0, 2.0, 4.0]), 3.0);
    assert_eq!(once("range", &[-1.0, 2.0, 4.0]), 2.0);
    assert_eq!(once("range", &[1.0, 2.0, 4.0]), 4.0);
    // A span that is not twice its floor, so the width is a difference.
    assert_eq!(once("range", &[0.0, 2.0, 6.0]), 4.0);
    // A fourth inlet is an exponent applied before the scaling.
    assert_eq!(once("range", &[0.0, 2.0, 4.0, 2.0]), 2.5);
}

#[test]
fn remap_rescales_between_two_ranges() {
    assert_eq!(once("remap", &[3.0, 2.0, 6.0, 0.0, 100.0]), 25.0);
    assert_eq!(once("remap", &[1.0, 2.0, 6.0, 0.0, 100.0]), 0.0);
    assert_eq!(once("remap", &[9.0, 2.0, 6.0, 0.0, 100.0]), 100.0);
}

#[test]
fn the_source_nodes_read_the_voice_around_them() {
    // `time` counts seconds from the start of the voice, one sample at a time.
    assert_eq!(run_at(4.0, "time", &[], 3), vec![0.0, 0.25, 0.5]);
    assert_eq!(once("sfreq", &[]), 440.0);
    assert_eq!(once("sgate", &[]), 1.0);
    assert_eq!(once("thru", &[0.75]), 0.75);
    // An `out` node passes its input on for the output pass to pick up.
    assert_eq!(once("output", &[0.75]), 0.75);
}

#[test]
fn the_phase_oscillators_share_one_accumulator_and_differ_in_shape() {
    // Rate 4 at 1Hz is a quarter cycle a sample, so each shape can be read at
    // its corners.
    assert_eq!(run_at(4.0, "zaw", &[1.0], 4), vec![-0.5, 0.0, 0.5, -1.0]);
    assert_eq!(run_at(4.0, "tri", &[1.0], 4), vec![0.0, 1.0, 0.0, -1.0]);
    // The pulse is high strictly below its width, so the sample sitting
    // exactly on it is low.
    assert_eq!(
        run_at(4.0, "pulse", &[1.0, 0.25], 4),
        vec![-1.0, -1.0, -1.0, 1.0]
    );
    assert_eq!(
        run_at(4.0, "pulse", &[1.0, 0.75], 4),
        vec![1.0, 1.0, -1.0, 1.0]
    );
    // A clock is in pulses per quarter note: 2.5bpm is 1Hz at 24ppq.
    assert_eq!(run_at(4.0, "clock", &[2.5], 4), vec![1.0, -1.0, -1.0, 1.0]);
}

#[test]
fn the_filter_nodes_take_the_low_pass_and_the_band_pass_off_one_recurrence() {
    assert_eq!(once("filter", &[1.0, 0.5, 0.25]), 0.00390625);
    assert_eq!(once("bpf", &[1.0, 0.5, 0.25]), 0.0625);
}

#[test]
fn qf_takes_its_defaults_from_the_ugen_rather_than_the_compiler() {
    // `qf`'s compile step spreads `...vars` untouched, so an omitted inlet
    // falls back to the class default: type 0, 500Hz, Q 1, gain 1.
    let expected = biquad_coefficients(0, 500.0, 1.0, 1.0, 48000.0)[0];
    assert_eq!(once("qf", &[1.0]), expected);
    // And an explicit type reaches the coefficient table.
    let high_pass = biquad_coefficients(1, 500.0, 1.0, 1.0, 48000.0)[0];
    assert_eq!(once("qf", &[1.0, 1.0]), high_pass);
}

#[test]
fn the_shaping_nodes_wire_their_inlets_in_upstreams_order() {
    close(once("distort", &[0.5, 0.5]), 0.745);
    close(once("fold", &[1.5, 1.0]), -1.0);
    assert_eq!(once("pick", &[1.0, 7.0, 8.0, 9.0]), 8.0);
    // A negative index wraps from the end, however far below zero it is.
    assert_eq!(once("pick", &[-5.0, 7.0, 8.0]), 8.0);
    // A sequence's first inlet is its clock; the rest are the steps.
    assert_eq!(once("seq", &[1.0, 10.0, 20.0, 30.0]), 10.0);
    assert_eq!(once("hold", &[5.0, 1.0]), 5.0);
    assert_eq!(once("lcgnoise", &[1.0, 0.0]), -0.6680714);
    assert_eq!(once("noise", &[0.0]), 0.7589328);
    assert_eq!(once("clockdiv", &[-1.0, 2.0]), 1.0);
    assert_eq!(once("delay", &[1.0, 0.0, 0.0]), 1.0);
}

#[test]
fn a_delay_node_reads_the_line_at_its_time_inlet() {
    // Half a second at 4Hz is two samples behind a constant input.
    assert_eq!(
        run_at(4.0, "delay", &[1.0, 0.5], 4),
        vec![0.0, 0.0, 1.0, 1.0]
    );
}

#[test]
fn lag_and_slide_reach_their_target_at_once_when_their_time_is_zero() {
    // A zero time floors the divisor at 1, so the state jumps straight there
    // and stays — which is also what pins the difference term.
    assert_eq!(run("lag", &[5.0, 0.0], 2), vec![5.0, 5.0]);
    assert_eq!(run("slide", &[5.0, 0.0], 2), vec![5.0, 5.0]);
    // A real time smooths instead: 1 second of lag is 4410 samples of it.
    assert!(once("lag", &[5.0, 2.0]) < 0.01);
    assert!(once("slide", &[5.0, 2.0]) < 0.01);
}

#[test]
fn slew_limits_how_fast_a_signal_may_rise_and_fall() {
    // The rates are in units per second against the hardcoded 1/48000, so
    // 48000 is one unit a sample.
    assert_eq!(
        run("slew", &[5.0, 48000.0, 48000.0], 3),
        vec![1.0, 2.0, 3.0]
    );
    // Falling is limited by the third inlet, not the second.
    assert_eq!(
        run("slew", &[-5.0, 48000.0, 48000.0], 3),
        vec![-1.0, -2.0, -3.0]
    );
    assert_eq!(run("slew", &[5.0, 96000.0, 48000.0], 2), vec![2.0, 4.0]);
    // Given room to move it goes to the target and stops there, which is what
    // makes the step a difference from the current value.
    assert_eq!(run("slew", &[5.0, 4.8e9, 48000.0], 2), vec![5.0, 5.0]);
}

#[test]
fn an_adsr_node_reads_its_inlets_as_gate_attack_decay_sustain_release() {
    // A held gate at rate 4: half a second of attack, half of decay, down to a
    // quarter of sustain.
    assert_eq!(
        run_at(4.0, "adsr", &[1.0, 0.5, 0.5, 0.25, 1.0], 5),
        vec![0.0, 0.5, 1.0, 1.0, 0.625]
    );
}

#[test]
fn a_trig_node_fires_once_per_rising_edge_of_its_input() {
    // A constant high fires only on the first sample.
    assert_eq!(run("trig", &[1.0], 3), vec![1.0, 0.0, 0.0]);
    // Driven from an impulse it fires again on every new edge, which is what
    // the falling half of the state machine is for.
    let value = program(
        &["impulse", "trig"],
        &[Value::Null, Value::Null],
        &[&[Value::F64(1.0)], &[Value::Int(0)]],
        &[(1, 0)],
    );
    let mut voice = KabelVoice::new(
        KabelProgram::from_value(&value).expect("a well-formed program"),
        Box::new(Silence),
        4.0,
        440.0,
        Vec::new(),
        4.0,
        4.0,
    );
    let got: Vec<f32> = (0..8).map(|_| voice.tick().0).collect();
    assert_eq!(got, vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn a_coded_node_takes_its_expression_from_the_nodes_value() {
    let coded = |kind: &str, source: &str, input: f32| {
        let value = program(
            &[kind],
            &[Value::Str(source.to_string())],
            &[&[Value::F64(f64::from(input))]],
            &[(0, 0)],
        );
        voice(value).tick().0
    };
    // `bytebeat`'s `t` is the node's own input.
    assert_eq!(coded("bytebeat", "t*3", 1.0), -0.9764706);
    assert_eq!(coded("raw", "$input*2", 0.25), 0.5);
}

// ---------------------------------------------------------------------------
// The output pass

#[test]
fn an_output_channel_past_the_last_one_wraps_round() {
    // Sixteen channels: writing to 16 lands back on the left.
    let value = program(&["n"], &[Value::F64(0.5)], &[&[]], &[(0, 16)]);
    assert_eq!(voice(value).tick(), (0.5, 0.0));
    // And `src` wraps the same way, so a feedback loop addressed past the end
    // still finds its channel.
    let value = program(
        &["n", "src", "n", "add"],
        &[Value::F64(16.0), Value::Null, Value::F64(1.0), Value::Null],
        &[&[], &[Value::Int(0)], &[], &[Value::Int(1), Value::Int(2)]],
        &[(3, 0)],
    );
    let mut voice = voice(value);
    let got: Vec<f32> = (0..3).map(|_| voice.tick().0).collect();
    assert_eq!(got, vec![1.0, 2.0, 3.0]);
}

#[test]
fn two_roots_on_one_channel_sum_rather_than_replace() {
    let value = program(
        &["n", "n"],
        &[Value::F64(0.25), Value::F64(0.5)],
        &[&[], &[]],
        &[(0, 0), (1, 0), (1, 1)],
    );
    assert_eq!(voice(value).tick(), (0.75, 0.5));
}

#[test]
fn a_voice_runs_until_its_end_and_forwards_a_bus_input_to_what_it_wraps() {
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Recorder(Arc<Mutex<Vec<i32>>>);

    impl VoiceLike for Recorder {
        fn tick(&mut self) -> (f32, f32) {
            (0.0, 0.0)
        }
        fn set_bus_input(&mut self, bus: i32, _left: &[f32], _right: &[f32]) {
            self.0.lock().unwrap().push(bus);
        }
        fn is_done(&self) -> bool {
            false
        }
    }

    let seen = Arc::new(Mutex::new(Vec::new()));
    let value = program(&["sgate"], &[Value::Null], &[&[]], &[(0, 0)]);
    let mut voice = KabelVoice::new(
        KabelProgram::from_value(&value).expect("a well-formed program"),
        Box::new(Recorder(Arc::clone(&seen))),
        4.0,
        440.0,
        Vec::new(),
        1.0,
        0.5,
    );
    // A bus reaches the voice underneath, which is where the effect chain is.
    voice.set_bus_input(3, &[1.0], &[1.0]);
    assert_eq!(*seen.lock().unwrap(), vec![3]);
    // Not done until `t` reaches the end the hap asked for.
    assert!(!voice.is_done());
    voice.tick();
    assert!(!voice.is_done());
    voice.tick();
    assert!(voice.is_done());
}

// ---------------------------------------------------------------------------
// Reading a program

#[test]
fn a_value_that_is_not_a_compiled_graph_is_refused() {
    assert!(KabelProgram::from_value(&Value::F64(1.0)).is_none());
    let mut map = ValueMap::new();
    map.insert("types".to_string(), Value::List(Vec::new()));
    // Missing `values`, `ins` and `outs`.
    assert!(KabelProgram::from_value(&Value::Map(map.clone())).is_none());
    map.insert("values".to_string(), Value::List(Vec::new()));
    map.insert("ins".to_string(), Value::List(Vec::new()));
    assert!(KabelProgram::from_value(&Value::Map(map.clone())).is_none());
    map.insert("outs".to_string(), Value::List(Vec::new()));
    assert!(KabelProgram::from_value(&Value::Map(map)).is_some());
}

#[test]
fn the_three_parallel_lists_have_to_be_the_same_length() {
    let one = |key: &str| {
        let mut map = ValueMap::new();
        map.insert(
            "types".to_string(),
            Value::List(vec![Value::Str("n".into())]),
        );
        map.insert("values".to_string(), Value::List(vec![Value::F64(1.0)]));
        map.insert(
            "ins".to_string(),
            Value::List(vec![Value::List(Vec::new())]),
        );
        map.insert("outs".to_string(), Value::List(Vec::new()));
        // Lengthen exactly one of them, which no compiler output ever does.
        map.insert(key.to_string(), Value::List(vec![Value::F64(1.0); 2]));
        KabelProgram::from_value(&Value::Map(map))
    };
    assert!(one("values").is_none());
    assert!(one("ins").is_none());
}

#[test]
fn an_output_naming_a_register_that_does_not_exist_is_dropped() {
    // A root past the end of the program would index out of bounds every
    // sample, so it is filtered out when the program is read.
    let value = program(&["n"], &[Value::F64(0.5)], &[&[]], &[(1, 0), (0, 0)]);
    let mut voice = voice(value);
    assert_eq!(voice.tick(), (0.5, 0.0));
    // An entry that is not a `(register, channel)` pair goes the same way.
    let mut map = ValueMap::new();
    map.insert(
        "types".to_string(),
        Value::List(vec![Value::Str("n".into())]),
    );
    map.insert("values".to_string(), Value::List(vec![Value::F64(0.5)]));
    map.insert(
        "ins".to_string(),
        Value::List(vec![Value::List(Vec::new())]),
    );
    map.insert(
        "outs".to_string(),
        Value::List(vec![Value::List(vec![Value::Int(0)]), Value::F64(0.0)]),
    );
    let program = KabelProgram::from_value(&Value::Map(map)).expect("a well-formed program");
    assert!(program.outs.is_empty());
}

#[test]
fn an_inlet_is_a_wire_when_it_is_an_integer_and_a_literal_otherwise() {
    // The compiler distinguishes the two by type, which is the whole wire
    // format: `mul` here reads register 0 against the literal 3.
    let value = program(
        &["n", "mul"],
        &[Value::F64(0.25), Value::Null],
        &[&[], &[Value::Int(0), Value::F64(3.0)]],
        &[(1, 0)],
    );
    assert_eq!(voice(value).tick().0, 0.75);
    // An inlet list that is not a list at all leaves the node with no inputs.
    let mut map = ValueMap::new();
    map.insert(
        "types".to_string(),
        Value::List(vec![Value::Str("add".into())]),
    );
    map.insert("values".to_string(), Value::List(vec![Value::Null]));
    map.insert("ins".to_string(), Value::List(vec![Value::F64(9.0)]));
    map.insert(
        "outs".to_string(),
        Value::List(vec![Value::List(vec![Value::Int(0), Value::Int(0)])]),
    );
    let program = KabelProgram::from_value(&Value::Map(map)).expect("a well-formed program");
    assert!(program.ins[0].is_empty());
}
