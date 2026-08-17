//! The FM matrix's own arithmetic. Rendering a whole FM voice only says the
//! output changed; these call `fm_deviation` directly, where the classic
//! `index x modulator frequency = peak deviation` relationship is exact and
//! every factor in it can be moved on its own.

use super::common::*;

const SR: f32 = 44100.0;

/// A one-operator FM voice: operator 1 modulating the carrier at `amt`, with a
/// square operator wave so its output is ±1 rather than something that starts
/// at zero and hides every multiplication behind it.
fn voice(amt: f32, ratio: f32) -> Voice {
    let mut fm = FmSpec::default();
    fm.max_op = 1;
    fm.ops[1] = FmOp {
        ratio,
        wave: Waveform::Square,
        env: None,
    };
    fm.amt[1][0] = amt;
    Voice::new(
        VoiceParams {
            freq: 100.0,
            fm,
            ..Default::default()
        },
        SR,
    )
}

#[test]
fn the_deviation_is_the_index_times_the_operator_frequency() {
    // A square starts at +1, so the first sample's deviation is exactly
    // `amt * (carrier * ratio) * 1`.
    let dev = |amt: f32, ratio: f32, carrier: f32| voice(amt, ratio).fm_deviation(carrier);

    assert_eq!(dev(1.0, 2.0, 100.0), 200.0);
    // Each factor scales it on its own: doubling any one doubles the answer,
    // which addition or division in place of the multiplication does not.
    assert_eq!(dev(2.0, 2.0, 100.0), 400.0);
    assert_eq!(dev(1.0, 4.0, 100.0), 400.0);
    assert_eq!(dev(1.0, 2.0, 200.0), 400.0);
    // No modulation index is no deviation at all.
    assert_eq!(dev(0.0, 2.0, 100.0), 0.0);
}

#[test]
fn an_operator_advances_at_its_own_frequency() {
    // Operator 1 at ratio 1 against a 4-sample-per-cycle carrier: a square
    // flips sign halfway through its own cycle, so the deviation's sign has to
    // follow the operator's phase and not the carrier's.
    let carrier = SR / 4.0;
    let mut v = voice(1.0, 1.0);
    let first = v.fm_deviation(carrier);
    let second = v.fm_deviation(carrier);
    let third = v.fm_deviation(carrier);
    assert!(first > 0.0, "a square starts positive, got {first}");
    assert_eq!(second, first, "a quarter cycle in it has not flipped yet");
    assert_eq!(third, -first, "half a cycle in it has");
}

#[test]
fn a_modulated_operator_uses_the_previous_samples_value() {
    // Operator 2 -> operator 1 -> carrier. Cross-modulation is sampled before
    // any phase advances, so operator 1's frequency picks up operator 2's
    // contribution rather than running at its ratio alone — and the two chain
    // through the same `amt * freq * out` product.
    let mut fm = FmSpec::default();
    fm.max_op = 2;
    fm.ops[1] = FmOp {
        ratio: 1.0,
        wave: Waveform::Square,
        env: None,
    };
    fm.ops[2] = FmOp {
        ratio: 1.0,
        wave: Waveform::Square,
        env: None,
    };
    fm.amt[1][0] = 1.0;
    fm.amt[2][1] = 4.0;
    fn chained(fm: FmSpec) -> Voice {
        Voice::new(
            VoiceParams {
                freq: 100.0,
                fm,
                ..Default::default()
            },
            SR,
        )
    }
    // The carrier deviation on the first sample is operator 1's alone; what
    // operator 2 changes is how fast operator 1 then moves.
    let mut v = chained(fm.clone());
    let carrier = SR / 8.0;
    let first = v.fm_deviation(carrier);
    assert_eq!(first, carrier, "amt 1 x ratio 1 x +1");

    // Without the 2->1 amount, operator 1 flips later than it does with it.
    let mut plain = voice(1.0, 1.0);
    let flip_of = |v: &mut Voice, carrier: f32| {
        let first = v.fm_deviation(carrier);
        (0..64)
            .position(|_| v.fm_deviation(carrier).signum() != first.signum())
            .expect("the operator should flip within 64 samples")
    };
    let with = flip_of(&mut chained(fm), carrier);
    let without = flip_of(&mut plain, carrier);
    assert!(
        with < without,
        "a modulated operator runs faster: flipped at {with} vs {without}"
    );
}
