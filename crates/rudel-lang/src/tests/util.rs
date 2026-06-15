use super::common::*;

// The user-reachable scalar helpers from core/util.mjs. They return numbers,
// so each is wrapped in `pure(...)` to give `eval` a pattern to return.

#[test]
fn midi_to_freq_matches_strudel() {
    // midiToFreq(69) == 440; midiToFreq(57) == 220 (an octave down).
    let pat = eval("pure(midiToFreq(69))").expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(440.0)]);
    let pat = eval("pure(midiToFreq(57))").expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(220.0)]);
}

#[test]
fn freq_to_midi_is_the_inverse() {
    // freqToMidi(440) == 69.
    let pat = eval("pure(freqToMidi(440))").expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::F64(n) => assert!((n - 69.0).abs() < 1e-9, "got {n}"),
        other => panic!("expected a number, got {other:?}"),
    }
}

#[test]
fn note_to_midi_parses_note_names() {
    // a4 -> 69, c4 -> 60 (default octave 3 is only used when none is given).
    let pat = eval(r#"pure(noteToMidi("a4"))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(69.0)]);
    let pat = eval(r#"pure(noteToMidi("c4"))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(60.0)]);
    // Default octave 3: a bare "c" is C3 (48).
    let pat = eval(r#"pure(noteToMidi("c"))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(48.0)]);
}

#[test]
fn note_to_midi_rejects_non_notes() {
    // Strudel throws on a non-note; the binding raises a Koto error.
    assert!(eval(r#"pure(noteToMidi("xyz"))"#).is_err());
}

#[test]
fn clamp_limits_to_the_range() {
    let pat = eval("pure(clamp(5, 0, 1))").expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(1.0)]);
    let pat = eval("pure(clamp(-3, 0, 1))").expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(0.0)]);
    let pat = eval("pure(clamp(0.5, 0, 1))").expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(0.5)]);
}

#[test]
fn converters_compose_in_patterns() {
    // A realistic use: set a note from a frequency round-tripped through midi.
    let pat = eval(r#"note(freqToMidi(440))"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => match m.get("note") {
            Some(Value::F64(n)) => assert!((n - 69.0).abs() < 1e-9, "got {n}"),
            other => panic!("expected note number, got {other:?}"),
        },
        other => panic!("expected a control map, got {other:?}"),
    }
}
