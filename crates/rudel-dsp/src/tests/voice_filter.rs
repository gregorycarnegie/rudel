//! The per-voice filter slot's modulation path: which offsets a slot answers
//! to, and how a modulator's value reaches the cutoff and the resonance.
//!
//! The filter tests next door drive whole voices, which cannot separate "the
//! modulation was applied" from "it was applied with the right sign" — the
//! output changes either way. These compare a modulated slot against an
//! unmodulated one built at the value the modulation should have produced, so
//! the two have to agree sample for sample.

use crate::filter::{FilterKind, FilterParams, VoiceFilter};
use crate::modulator::ModTarget;

const SR: f32 = 44100.0;

fn slot(kind: FilterKind, freq: f32, q: f32) -> VoiceFilter {
    VoiceFilter::new(
        kind,
        &FilterParams {
            freq: Some(freq),
            q,
            ..Default::default()
        },
        SR,
    )
}

/// A square wave through the slot, so there is high-frequency content for the
/// cutoff to act on.
fn run(mut f: VoiceFilter, freq_mod: f32, q_mod: f32) -> Vec<f32> {
    (0..256)
        .map(|i| {
            let x = if (i / 8) % 2 == 0 { 1.0 } else { -1.0 };
            f.process(x, i as f32 / SR, 1.0, SR, freq_mod, q_mod)
        })
        .collect()
}

#[test]
fn each_slot_answers_to_its_own_pair_of_targets() {
    // A mixed chain looks its offsets up through this, so a wrong arm points a
    // `bandf` modulation at the low-pass.
    assert_eq!(
        slot(FilterKind::Low, 1000.0, 1.0).mod_targets(),
        (ModTarget::Cutoff, ModTarget::Resonance)
    );
    assert_eq!(
        slot(FilterKind::High, 1000.0, 1.0).mod_targets(),
        (ModTarget::Hcutoff, ModTarget::Hresonance)
    );
    assert_eq!(
        slot(FilterKind::Band, 1000.0, 1.0).mod_targets(),
        (ModTarget::Bandf, ModTarget::Bandq)
    );
}

#[test]
fn a_cutoff_offset_lands_on_the_cutoff_it_is_added_to() {
    // 200Hz modulated up by 3000 is the same filter as a static 3200.
    let modulated = run(slot(FilterKind::Low, 200.0, 1.0), 3000.0, 0.0);
    let equivalent = run(slot(FilterKind::Low, 3200.0, 1.0), 0.0, 0.0);
    assert_eq!(modulated, equivalent);
    // ...and not the same as one modulated the other way, or not at all.
    assert_ne!(modulated, run(slot(FilterKind::Low, 200.0, 1.0), 0.0, 0.0));
    assert_ne!(
        modulated,
        run(slot(FilterKind::Low, 3200.0, 1.0), -3000.0, 0.0)
    );
}

#[test]
fn a_resonance_offset_lands_on_the_resonance() {
    let modulated = run(slot(FilterKind::Low, 800.0, 1.0), 0.0, 2.0);
    let equivalent = run(slot(FilterKind::Low, 800.0, 3.0), 0.0, 0.0);
    assert_eq!(modulated, equivalent);
    assert_ne!(modulated, run(slot(FilterKind::Low, 800.0, 1.0), 0.0, 0.0));
}

#[test]
fn an_unmodulated_slot_takes_the_static_path() {
    // Both offsets zero has to mean "leave the coefficients alone" — and
    // either one alone has to mean "recompute", or that modulation is dropped.
    let still = run(slot(FilterKind::Low, 800.0, 1.0), 0.0, 0.0);
    assert_ne!(still, run(slot(FilterKind::Low, 800.0, 1.0), 400.0, 0.0));
    assert_ne!(still, run(slot(FilterKind::Low, 800.0, 1.0), 0.0, 4.0));
}

#[test]
fn an_envelope_sweep_is_the_base_the_offset_rides_on() {
    // With a cutoff envelope the base is the sweep's current value, not the
    // static cutoff — and a modulator adds on top of wherever the sweep is.
    // The sweep starts at `min` and settles at `max` (sustain 1), so the two
    // filters only agree once it has, hence comparing the settled tail.
    let swept = |freq_mod: f32| {
        let f = VoiceFilter::new(
            FilterKind::Low,
            &FilterParams {
                freq: Some(500.0),
                q: 1.0,
                env: Some(2.0),
                attack: Some(0.0),
                decay: Some(0.0),
                sustain: Some(1.0),
                ..Default::default()
            },
            SR,
        );
        run(f, freq_mod, 0.0)
    };
    let settled = |v: &[f32]| v[192..].to_vec();
    let close = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 2e-3);

    // Sustain 1 holds the sweep at its top — two octaves above 500 is 2000.
    let held = swept(0.0);
    let statically = run(slot(FilterKind::Low, 2000.0, 1.0), 0.0, 0.0);
    assert!(
        close(&settled(&held), &settled(&statically)),
        "a settled sweep should be its own top: {:?} vs {:?}",
        &settled(&held)[..4],
        &settled(&statically)[..4]
    );
    // An offset moves it from there, not from the 500 it started at.
    assert!(close(
        &settled(&swept(1000.0)),
        &settled(&run(slot(FilterKind::Low, 3000.0, 1.0), 0.0, 0.0))
    ));
    assert!(!close(
        &settled(&swept(1000.0)),
        &settled(&run(slot(FilterKind::Low, 1500.0, 1.0), 0.0, 0.0))
    ));
}
