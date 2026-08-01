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

/// The `adsr`/`ad`/`ds`/`ar` shortcuts never reach this layer as themselves —
/// like upstream they are control setters, and `rudel_core::controls::multi`
/// expands them into plain stages first (rudel-lang's `controls` tests cover
/// that expansion, including that the `adsr` key does not survive it). What is
/// worth pinning here is the envelope each expansion resolves to, because that
/// is where the surprises are: two of these four leave a stage unnamed, and
/// which one decides whether the note holds or dies.
#[test]
fn the_envelopes_the_shortcuts_resolve_to() {
    // `adsr("0.1:0.2:0.3:0.4")` names all four, so nothing is re-defaulted.
    assert_adsr(
        adsr_from(&[
            ("attack", Value::F64(0.1)),
            ("decay", Value::F64(0.2)),
            ("sustain", Value::F64(0.3)),
            ("release", Value::F64(0.4)),
        ]),
        [0.1, 0.2, 0.3, 0.4],
        "adsr(0.1:0.2:0.3:0.4)",
    );

    // `ad("0.05")` expands to attack *and* decay (core defaults the second to
    // the first). Both present means the unset sustain lands on 0.001 — so `ad`
    // is percussive without ever naming a sustain.
    assert_adsr(
        adsr_from(&[("attack", Value::F64(0.05)), ("decay", Value::F64(0.05))]),
        [0.05, 0.05, 0.001, 0.01],
        "ad(0.05)",
    );

    // `ds("0.2:0.4")` names decay and sustain, leaving attack at its floor.
    assert_adsr(
        adsr_from(&[("decay", Value::F64(0.2)), ("sustain", Value::F64(0.4))]),
        [0.001, 0.2, 0.4, 0.01],
        "ds(0.2:0.4)",
    );

    // `ar("0.2")` expands to attack and release, leaving decay unnamed — so
    // sustain stays full and the note holds until the release, the opposite of
    // `ad`.
    assert_adsr(
        adsr_from(&[("attack", Value::F64(0.2)), ("release", Value::F64(0.2))]),
        [0.2, 0.001, 1.0, 0.2],
        "ar(0.2)",
    );
}
