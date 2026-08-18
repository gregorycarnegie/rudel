//! `VoiceParams::from_controls` corners that whole-voice rendering cannot see:
//! the note length coming through at all, the additive/partials pairing, and
//! the `pcurve` flag.

use super::common::*;

fn params(pairs: &[(&str, Value)], duration: f32) -> VoiceParams {
    let map: ValueMap = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    VoiceParams::from_controls(&map, duration)
}

#[test]
fn the_note_length_reaches_the_voice() {
    // Everything downstream that ends a note — the envelope's hold, the
    // sampler's window — reads this, and its default is not the hap's length.
    assert_eq!(params(&[], 2.5).duration, 2.5);
    assert_eq!(params(&[], 0.25).duration, 0.25);
}

#[test]
fn partials_only_build_an_additive_wave_when_there_are_any() {
    // `s('saw').partials([...])` replaces the waveform with a Fourier sum;
    // an empty list has no harmonics to sum, so there is nothing to build.
    let with = params(
        &[
            ("s", Value::from("saw")),
            (
                "partials",
                Value::List(vec![Value::F64(1.0), Value::F64(0.5)]),
            ),
        ],
        1.0,
    );
    assert!(with.additive.is_some(), "partials should build a wave");

    let empty = params(
        &[("s", Value::from("saw")), ("partials", Value::List(vec![]))],
        1.0,
    );
    assert!(
        empty.additive.is_none(),
        "an empty partials list has nothing to build from"
    );

    // `s('user')` names no waveform of its own, so without partials it falls
    // back to a triangle rather than to the default.
    let user = params(&[("s", Value::from("user"))], 1.0);
    assert!(user.additive.is_none());
    assert_eq!(user.waveform, Waveform::Triangle);
    // ...while the other additive names keep their own waveform.
    let saw = params(&[("s", Value::from("saw"))], 1.0);
    assert_eq!(saw.waveform, Waveform::Saw);
}

#[test]
fn pcurve_is_a_flag_not_a_value() {
    // superdough: 0 is the linear default, anything else selects the
    // exponential ramp segments.
    assert!(!params(&[], 1.0).pcurve_exp);
    assert!(!params(&[("pcurve", Value::F64(0.0))], 1.0).pcurve_exp);
    assert!(params(&[("pcurve", Value::F64(1.0))], 1.0).pcurve_exp);
    assert!(params(&[("pcurve", Value::F64(3.0))], 1.0).pcurve_exp);
}

#[test]
fn a_gain_curve_rescales_the_voice_gain() {
    // `setGainCurve` is global state the language layer installs; the voice
    // reads it the way superdough's `applyGainCurve` does at every gain-like
    // control. Cleared either side so the rest of the suite is unaffected.
    let params = |g: f64| {
        let mut map = ValueMap::new();
        map.insert("gain".to_string(), Value::F64(g));
        VoiceParams::from_controls(&map, 1.0).gain
    };
    rudel_core::clear_gain_curve();
    let plain = params(0.5);
    assert!(
        (plain - 0.5).abs() < 1e-6,
        "no curve leaves gain alone: {plain}"
    );

    rudel_core::set_gain_curve(|x| x * x);
    let curved = params(0.5);
    rudel_core::clear_gain_curve();
    assert!((curved - 0.25).abs() < 1e-3, "quadratic gain: {curved}");
    assert!((params(0.5) - 0.5).abs() < 1e-6, "cleared again");
}
