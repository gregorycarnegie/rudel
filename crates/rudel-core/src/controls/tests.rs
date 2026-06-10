use super::*;
use crate::{IntoPattern, Value, seq};

#[test]
fn note_wraps_into_map() {
    let pat = note(seq([0, 4, 7]));
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => assert_eq!(m.get("note"), Some(&Value::Int(0))),
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn s_splits_sample_index() {
    let pat = s("bd:3".into_pattern());
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("s"), Some(&Value::Str("bd".to_string())));
            assert_eq!(m.get("n"), Some(&Value::Int(3)));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn s_preserves_non_numeric_tail() {
    // `s("name:tail")` keeps a non-numeric tail as a string `n`.
    let pat = s("bd:foo".into_pattern());
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("s"), Some(&Value::Str("bd".to_string())));
            assert_eq!(m.get("n"), Some(&Value::Str("foo".to_string())));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn mode_splits_into_mode_and_anchor() {
    // `mode("below:G4")` (a `:`-list) sets both `mode` and `anchor`.
    let pat = mode(Value::List(vec![
        Value::Str("below".into()),
        Value::Str("G4".into()),
    ]));
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("mode"), Some(&Value::Str("below".to_string())));
            assert_eq!(m.get("anchor"), Some(&Value::Str("G4".to_string())));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn alias_controls_write_canonical_keys() {
    // Aliases canonicalize like Strudel's `getControlName`: `ph` writes
    // `phaserrate`, `duck` writes `duckorbit`, `v` writes `vib`.
    let pat = note(seq([0])).ph(2).duck(0.5).v(4);
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("phaserrate"), Some(&Value::Int(2)));
            assert_eq!(m.get("duckorbit"), Some(&Value::F64(0.5)));
            assert_eq!(m.get("vib"), Some(&Value::Int(4)));
            assert!(!m.contains_key("ph"));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn named_controls_write_literal_keys() {
    // Snake-case builder fns write Strudel's camelCase keys.
    let pat = note(seq([0])).compressor_knee(30).fx_release(0.2);
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("compressorKnee"), Some(&Value::Int(30)));
            assert_eq!(m.get("FXrelease"), Some(&Value::F64(0.2)));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn roomsize_aliases_map_to_size() {
    // Rudel's canonical reverb-size key is `size`; Strudel's `roomsize`,
    // `sz`, and `rsize` all land there.
    let pat = note(seq([0])).roomsize(0.8);
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => assert_eq!(m.get("size"), Some(&Value::F64(0.8))),
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn adsr_expands_into_envelope_keys() {
    // `adsr(".1:.2:.5:.3")` (a `:`-list) expands into the four envelope
    // controls, like Strudel's multi-control helper.
    let pat = adsr(Value::List(vec![
        Value::F64(0.1),
        Value::F64(0.2),
        Value::F64(0.5),
        Value::F64(0.3),
    ]));
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("attack"), Some(&Value::F64(0.1)));
            assert_eq!(m.get("decay"), Some(&Value::F64(0.2)));
            assert_eq!(m.get("sustain"), Some(&Value::F64(0.5)));
            assert_eq!(m.get("release"), Some(&Value::F64(0.3)));
        }
        other => panic!("expected map, got {other:?}"),
    }
    // a scalar only sets `attack`
    let pat = adsr(Value::F64(0.1));
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("attack"), Some(&Value::F64(0.1)));
            assert!(!m.contains_key("decay"));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn envelope_helper_defaults_match_strudel() {
    // `ad(x)`: decay defaults to attack; `ar(x)`: release defaults to
    // attack; `ds(x)`: sustain defaults to 0.
    let first = &ad(Value::F64(0.2)).query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("attack"), Some(&Value::F64(0.2)));
            assert_eq!(m.get("decay"), Some(&Value::F64(0.2)));
        }
        other => panic!("expected map, got {other:?}"),
    }
    let first = &ds(Value::F64(0.3)).query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("decay"), Some(&Value::F64(0.3)));
            assert_eq!(m.get("sustain"), Some(&Value::Int(0)));
        }
        other => panic!("expected map, got {other:?}"),
    }
    let first = &ar(Value::F64(0.4)).query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("attack"), Some(&Value::F64(0.4)));
            assert_eq!(m.get("release"), Some(&Value::F64(0.4)));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn control_and_sysex_spread_pairs() {
    let pat = note(seq([0])).control(Value::List(vec![Value::Int(74), Value::Int(64)]));
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("ccn"), Some(&Value::Int(74)));
            assert_eq!(m.get("ccv"), Some(&Value::Int(64)));
        }
        other => panic!("expected map, got {other:?}"),
    }
    let pat = note(seq([0])).sysex(Value::List(vec![Value::Int(7), Value::Int(1)]));
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("sysexid"), Some(&Value::Int(7)));
            assert_eq!(m.get("sysexdata"), Some(&Value::Int(1)));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn control_name_resolves_aliases() {
    // mirrors Strudel's getControlName: aliases resolve to the canonical
    // key they write, unknown names resolve to themselves.
    assert_eq!(control_name("lpf"), "cutoff");
    assert_eq!(control_name("bb"), "byteBeatExpression");
    assert_eq!(control_name("fm23"), "fmi23");
    assert_eq!(control_name("vel"), "velocity");
    assert_eq!(control_name("sound"), "s");
    assert_eq!(control_name("loopb"), "loopBegin");
    assert_eq!(control_name("note"), "note");
    assert_eq!(control_name("not_a_control"), "not_a_control");
}

#[test]
fn as_controls_maps_positional_values() {
    // `"c:.5".as("note:clip")`: list values map positionally, with alias
    // names canonicalized (vel -> velocity).
    let pat = crate::pure(Value::List(vec![Value::Str("c".into()), Value::F64(0.5)]))
        .as_controls(&["note", "vel"]);
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("note"), Some(&Value::Str("c".into())));
            assert_eq!(m.get("velocity"), Some(&Value::F64(0.5)));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn scrub_sets_begin_speed_and_clip() {
    // scrub("0.5:2"): structure from the positions pattern; begin set,
    // speed multiplied, clip forced to 1.
    let positions = crate::pure(Value::List(vec![Value::F64(0.5), Value::Int(2)]));
    let pat = s("amen".into_pattern()).speed(0.5).scrub(positions);
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert_eq!(m.get("begin"), Some(&Value::F64(0.5)));
            assert_eq!(m.get("speed"), Some(&Value::F64(1.0)));
            assert_eq!(m.get("clip"), Some(&Value::Int(1)));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn numbered_fm_names_resolve_to_canonical_keys() {
    // 8 families * 8 ops + 5 short spellings * 8 + fm1-fm8 + the 9x9
    // matrix under both spellings.
    let names = numbered_control_names();
    assert_eq!(names.len(), 8 * 8 + 5 * 8 + 8 + 9 * 9 * 2);
    let key = |name: &str| {
        names
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, k)| k.as_str())
            .unwrap_or_else(|| panic!("{name} missing"))
    };
    // `{name}1` is the bare control; `fmN` aliases the chain `fmiN`;
    // `fm{i}{j}` aliases the matrix edge `fmi{i}{j}`.
    assert_eq!(key("fmh1"), "fmh");
    assert_eq!(key("fm1"), "fm");
    assert_eq!(key("fm3"), "fmi3");
    assert_eq!(key("fmatt5"), "fmattack5");
    assert_eq!(key("fme1"), "fmenv");
    assert_eq!(key("fm23"), "fmi23");
    assert_eq!(key("fmi20"), "fmi20");
}

#[test]
fn gain_method_merges_key() {
    // note(...).gain(0.5) -> { note, gain }
    let pat = note(seq([0, 1])).gain(0.5);
    let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
    match &first.value {
        Value::Map(m) => {
            assert!(m.contains_key("note"));
            assert_eq!(m.get("gain"), Some(&Value::F64(0.5)));
        }
        other => panic!("expected map, got {other:?}"),
    }
}
