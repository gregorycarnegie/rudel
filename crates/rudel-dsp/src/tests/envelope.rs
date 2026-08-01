//! `getADSRValues` semantics for the gain envelope.
//!
//! The trap is that it does *not* fill in missing stages from the defaults
//! array. That array is used only when all four are unset; naming any one sends
//! the rest to a clamped branch. Filling them in field-wise gives an envelope
//! that still sounds like an envelope, just not upstream's — which is why this
//! went unnoticed until the pitch-envelope oracle caught the same bug.

use super::common::*;
use rudel_core::Value;

fn adsr_from(controls: &[(&str, Value)]) -> Adsr {
    let map: ValueMap = controls
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    VoiceParams::from_controls(&map, 0.25).adsr
}

fn assert_adsr(got: Adsr, want: [f32; 4], what: &str) {
    let g = [got.attack, got.decay, got.sustain, got.release];
    for (i, stage) in ["attack", "decay", "sustain", "release"].iter().enumerate() {
        assert!(
            (g[i] - want[i]).abs() < 1e-6,
            "{what}: {stage} should be {}, got {} (whole envelope {g:?})",
            want[i],
            g[i]
        );
    }
}

#[test]
fn an_unset_envelope_takes_the_synth_defaults() {
    // Only when *nothing* is named does the defaults array apply.
    assert_adsr(adsr_from(&[]), [0.001, 0.05, 0.6, 0.01], "no controls");
}

#[test]
fn naming_one_stage_redefaults_the_others() {
    // The headline case. `.attack(0.1)` is not "the defaults with a longer
    // attack": decay collapses to 1ms and sustain goes to full, so the note
    // holds instead of dropping to 60%.
    assert_adsr(
        adsr_from(&[("attack", Value::F64(0.1))]),
        [0.1, 0.001, 1.0, 0.01],
        "attack only",
    );

    // With decay named too, the unset sustain lands on 0.001 instead of 1 —
    // a percussive envelope rather than a held one.
    assert_adsr(
        adsr_from(&[("attack", Value::F64(0.1)), ("decay", Value::F64(0.2))]),
        [0.1, 0.2, 0.001, 0.01],
        "attack + decay",
    );

    // Naming only the release still re-defaults attack and decay.
    assert_adsr(
        adsr_from(&[("release", Value::F64(0.5))]),
        [0.001, 0.001, 1.0, 0.5],
        "release only",
    );
}

#[test]
fn stages_clamp_to_their_floors_and_ceilings() {
    // Attack and decay floor at 1ms, release at 10ms — note the release floor
    // is ten times the other two — and sustain caps at 1.
    assert_adsr(
        adsr_from(&[
            ("attack", Value::F64(0.0)),
            ("decay", Value::F64(0.0)),
            ("sustain", Value::F64(4.0)),
            ("release", Value::F64(0.0)),
        ]),
        [0.001, 0.001, 1.0, 0.01],
        "all zeroed",
    );

    // A release under the floor is raised to it, not kept.
    assert_adsr(
        adsr_from(&[("release", Value::F64(0.001))]),
        [0.001, 0.001, 1.0, 0.01],
        "release below its floor",
    );
}

#[test]
fn the_list_shortcuts_feed_the_same_four_controls() {
    // `adsr("a:d:s:r")` sets all four outright.
    assert_adsr(
        adsr_from(&[(
            "adsr",
            Value::List(vec![
                Value::F64(0.1),
                Value::F64(0.2),
                Value::F64(0.3),
                Value::F64(0.4),
            ]),
        )]),
        [0.1, 0.2, 0.3, 0.4],
        "adsr list",
    );

    // `ad(t)` is `[attack, decay = attack]`, and sets no sustain — which lands
    // on 0.001 because both attack and decay are then present.
    assert_adsr(
        adsr_from(&[("ad", Value::F64(0.05))]),
        [0.05, 0.05, 0.001, 0.01],
        "ad with one value",
    );
    assert_adsr(
        adsr_from(&[("ad", Value::List(vec![Value::F64(0.05), Value::F64(0.3)]))]),
        [0.05, 0.3, 0.001, 0.01],
        "ad with two values",
    );

    // `ar(t)` is `[attack, release = attack]`, leaving decay unset — so sustain
    // stays full and the note holds until release.
    assert_adsr(
        adsr_from(&[("ar", Value::F64(0.2))]),
        [0.2, 0.001, 1.0, 0.2],
        "ar with one value",
    );
    assert_adsr(
        adsr_from(&[("ar", Value::List(vec![Value::F64(0.2), Value::F64(0.6)]))]),
        [0.2, 0.001, 1.0, 0.6],
        "ar with two values",
    );
}
