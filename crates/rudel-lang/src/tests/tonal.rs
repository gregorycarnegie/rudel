use super::common::*;

#[test]
fn scale_via_koto() {
    // n("0 2 4").scale("C:major") -> C3 E3 G3 = 48 52 55
    let pat = eval(r#"n("0 2 4").scale("C:major")"#).expect("eval");
    let mut got: Vec<f64> = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .map(|h| match h.value {
            Value::Map(m) => m.get("note").and_then(|v| v.as_f64()).unwrap(),
            other => other.as_f64().unwrap(),
        })
        .collect();
    got.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(got, vec![48.0, 52.0, 55.0]);
}

#[test]
fn transpose_via_koto() {
    let pat = eval(r#"note(60).transpose(7)"#).expect("eval");
    let note = match &pat.query_arc(Frac::zero(), Frac::one())[0].value {
        Value::Map(m) => m.get("note").and_then(|v| v.as_f64()).unwrap(),
        other => other.as_f64().unwrap(),
    };
    assert_eq!(note, 67.0);
}

#[test]
fn transpose_interval_strings_via_koto() {
    let note_at = |src: &str, b: i64, e: i64| -> f64 {
        let pat = eval(src).expect("eval");
        match &pat.query_arc(Frac::int(b), Frac::int(e))[0].value {
            Value::Map(m) => m.get("note").and_then(|v| v.as_f64()).unwrap(),
            other => other.as_f64().unwrap(),
        }
    };
    // a major third up from C4
    assert_eq!(note_at(r#"note(60).transpose("3M")"#, 0, 1), 64.0);
    // a pattern of interval strings (mini-notation) applied per cycle
    assert_eq!(note_at(r#"note(60).transpose("<5P -2M>")"#, 0, 1), 67.0);
    assert_eq!(note_at(r#"note(60).transpose("<5P -2M>")"#, 1, 2), 58.0);
}

#[test]
fn arp_with_via_koto() {
    // the chord is presented to the callback as a sequence of its notes;
    // identity == arpeggiate
    let pat = eval(r#"stack(5, 7, 9).arp_with(|c| c)"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(5), Value::Int(7), Value::Int(9)]
    );
    // reversing the chord sequence per chord (snake, camelCase, and standalone)
    let pat = eval(r#"stack(0, 1, 2).arp_with(|c| c.rev())"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(2), Value::Int(1), Value::Int(0)]
    );
    let camel = eval(r#"stack(0, 1, 2).arpWith(|c| c.rev())"#).expect("eval");
    assert_eq!(values(&camel, 0, 1), values(&pat, 0, 1));
    let standalone = eval(r#"arpWith(|c| c.rev(), stack(0, 1, 2))"#).expect("eval");
    assert_eq!(values(&standalone, 0, 1), values(&pat, 0, 1));
    // works per-cycle across an alternation of different chords (probe
    // window discovers both chords)
    let pat = eval(r#"seq("<[0,1] [2,3]>").arp_with(|c| c.rev())"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(1), Value::Int(0)]);
    assert_eq!(values(&pat, 1, 2), vec![Value::Int(3), Value::Int(2)]);
}

#[test]
fn voicing_via_koto() {
    // a chord-symbol pattern voiced with the default `ireal` dictionary below
    // the c5 anchor: C -> E3 C4 E4 G4 C5.
    // (mini-notation can't spell `^`, so use `maj7`/`m7`-style symbols, or
    // pure("C^7") for the literal form.)
    let pat = eval(r#"pure("C").voicing()"#).expect("eval");
    let mut got = values(&pat, 0, 1);
    got.sort_by_key(|v| v.as_f64().unwrap() as i64);
    assert_eq!(
        got,
        vec![
            Value::F64(52.0),
            Value::F64(60.0),
            Value::F64(64.0),
            Value::F64(67.0),
            Value::F64(72.0)
        ]
    );
    // named dictionary, literal ^ spelling via pure
    let pat = eval(r#"pure("C^7").voicings("lefthand")"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
    // maj7 spelling routes through the same dictionary key
    let pat = eval(r#"pure("Cmaj7").voicings("lefthand")"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
    // rootNotes maps a chord to its root in an octave
    let pat = eval(r#"pure("Am7").root_notes(3)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(57.0)]); // A3
    // chord progressions resolve through mini-notation alternation
    assert!(eval(r#"seq("<Cmaj7 A7 Dm7 G7>").voicing()"#).is_ok());
}

#[test]
fn arp_and_arpeggiate_via_koto() {
    // stack(0,1,2) is a chord; arp("0 1 2") walks up it
    let pat = eval(r#"stack(0, 1, 2).arp("0 1 2")"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(0), Value::Int(1), Value::Int(2)]
    );
    // arpeggiate plays the chord notes in sequence
    let pat = eval(r#"stack(5, 7, 9).arpeggiate()"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(5), Value::Int(7), Value::Int(9)]
    );
    // works on note chords from mini-notation too
    assert!(eval(r#"note("[c,e,g]").arp("0 1 2 1")"#).is_ok());
}

#[test]
fn chord_control_and_voicing_controls_via_koto() {
    // top-level chord(...) plus `.dict()`/`.voicing()` voice a chord symbol.
    // Default `ireal` dictionary: C -> E3 C4 E4 G4 C5.
    let pat = eval(r#"chord("C").voicing()"#).expect("eval");
    let mut got = values(&pat, 0, 1);
    got.sort_by_key(|v| v.as_f64().unwrap() as i64);
    assert_eq!(
        got,
        vec![
            Value::F64(52.0),
            Value::F64(60.0),
            Value::F64(64.0),
            Value::F64(67.0),
            Value::F64(72.0)
        ]
    );
    // `.dict("lefthand")` routes through the named dictionary (mini can't spell
    // `^`, so use the `maj7` symbol, which normalises to `^7`).
    let pat = eval(r#"chord("Cmaj7").dict("lefthand").voicing()"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
    // mini-notation chord tails (`c:maj7`) voice through the list-backed reader.
    assert!(eval(r#"chord("c:maj7").voicing()"#).is_ok());
    // `.chord(value)` as a control on an n-pattern, then voiced.
    assert!(eval(r#"n("0 1 2 3").chord("<Dm Am>").voicing()"#).is_ok());
    // `.chord()` (zero-arg) still expands chord names to note stacks.
    let pat = eval(r#"pure("C").chord()"#).expect("eval");
    let mut got: Vec<i32> = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .map(|h| h.value.as_f64().unwrap() as i32)
        .collect();
    got.sort();
    assert_eq!(got, vec![48, 52, 55]);
}

#[test]
fn mtranspose_ctranspose_fold_into_note() {
    use rudel_core::query_controls;
    // ctranspose is a chromatic (semitone) shift folded into `note`.
    let pat = eval(r#"note(60).ctranspose(7)"#).expect("eval");
    let evs = query_controls(&pat, 1.0, 0.0, 1.0);
    assert_eq!(
        evs[0].controls.get("note").and_then(|v| v.as_f64()),
        Some(67.0)
    );
    // mtranspose is a scale-step shift within the tagged scale.
    let pat = eval(r#"n(0).scale("C:major").mtranspose(2)"#).expect("eval");
    let evs = query_controls(&pat, 1.0, 0.0, 1.0);
    assert_eq!(
        evs[0].controls.get("note").and_then(|v| v.as_f64()),
        Some(52.0)
    );
    assert!(!evs[0].controls.contains_key("mtranspose"));
}

#[test]
fn xen_via_koto_produces_freq_control() {
    let pat = eval(r#"i("0 1").xen("12edo")"#).expect("eval");
    let got = values(&pat, 0, 1);
    match &got[0] {
        Value::Map(m) => assert_eq!(m.get("freq").and_then(Value::as_f64), Some(220.0)),
        other => panic!("expected freq map, got {other:?}"),
    }
    match &got[1] {
        Value::Map(m) => {
            let freq = m.get("freq").and_then(Value::as_f64).unwrap();
            assert!((freq - 220.0 * 2f64.powf(1.0 / 12.0)).abs() < 1e-6);
        }
        other => panic!("expected freq map, got {other:?}"),
    }
}

#[test]
fn tune_mul_freq_chain_via_koto() {
    let pat = eval(r#"i("0 1 2").tune("hexany15").mul(220).freq()"#).expect("eval");
    let got = values(&pat, 0, 1);
    assert_eq!(got.len(), 3);
    match &got[0] {
        Value::Map(m) => assert_eq!(m.get("freq").and_then(Value::as_f64), Some(220.0)),
        other => panic!("expected freq map, got {other:?}"),
    }
    assert!(
        got.iter()
            .all(|v| matches!(v, Value::Map(m) if m.contains_key("freq")))
    );
}

#[test]
fn xen_ratio_array_and_with_base_via_koto() {
    let pat = eval(r#"i("0 1 2").xen([1, 5/4, 3/2]).withBase(440)"#).expect("eval");
    let got: Vec<f64> = values(&pat, 0, 1)
        .into_iter()
        .map(|v| match v {
            Value::Map(m) => m.get("freq").and_then(Value::as_f64).unwrap(),
            other => panic!("expected freq map, got {other:?}"),
        })
        .collect();
    assert_eq!(got, vec![440.0, 550.0, 660.0]);
}

#[test]
fn edo_scale_via_koto() {
    // C:LLsLLLs:2:1 is C major in 12-EDO; bare degrees map to diatonic notes.
    let pat = eval(r#""0 2 4 6".edoScale("C:LLsLLLs:2:1")"#).expect("eval");
    let got: Vec<f64> = values(&pat, 0, 1)
        .into_iter()
        .map(|v| v.as_f64().expect("note number"))
        .collect();
    assert_eq!(got, vec![48.0, 52.0, 55.0, 59.0]);
    // a non-12 EDO produces microtonal (fractional) MIDI notes.
    let pat = eval(r#""0 1 2".edoScale("C:LLsLLL:3:1")"#).expect("eval");
    let got: Vec<f64> = values(&pat, 0, 1)
        .into_iter()
        .map(|v| v.as_f64().expect("note number"))
        .collect();
    assert_eq!(got, vec![48.0, 50.25, 52.5]);
}

#[test]
fn tuning_ratio_array_via_koto() {
    // tuning reads the bare value as the scale index and returns the raw ratio.
    let pat = eval(r#""0 1 2 3".tuning([1, 5/4, 3/2])"#).expect("eval");
    let got: Vec<f64> = values(&pat, 0, 1)
        .into_iter()
        .map(|v| v.as_f64().expect("ratio number"))
        .collect();
    assert_eq!(got, vec![1.0, 1.25, 1.5, 2.0]);
}

#[test]
fn xen_docs_math_pow_and_piano_via_koto() {
    let pat = eval(
        r#"
i("0 1 2").xen([
  Math.pow(2, 0/31),
  Math.pow(2, 8/31),
  Math.pow(2, 18/31),
]).piano()
"#,
    )
    .expect("eval");
    let got = values(&pat, 0, 1);
    assert_eq!(got.len(), 3);
    for value in got {
        match value {
            Value::Map(m) => {
                assert_eq!(m.get("s").and_then(Value::as_str), Some("piano"));
                assert_eq!(m.get("clip").and_then(Value::as_f64), Some(1.0));
                assert_eq!(m.get("release").and_then(Value::as_f64), Some(0.1));
                assert!(m.get("freq").and_then(Value::as_f64).is_some());
            }
            other => panic!("expected piano control map, got {other:?}"),
        }
    }
}

#[test]
fn fmap_get_freq_and_reverb_aliases_via_koto() {
    let pat = eval(r#""<c3 a3>".fmap(getFreq)"#).expect("eval");
    let got: Vec<f64> = values(&pat, 0, 2)
        .into_iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(got.len(), 2);
    assert!((got[0] - rudel_core::get_freq(&Value::Str("c3".into())).unwrap()).abs() < 1e-9);
    assert!((got[1] - rudel_core::get_freq(&Value::Str("a3".into())).unwrap()).abs() < 1e-9);

    let pat = eval(r#"freq(220).room("1:15").rdim(8500).rlp(14000).rfade(8)"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => {
            assert_eq!(m.get("roomdim").and_then(Value::as_f64), Some(8500.0));
            assert_eq!(m.get("roomlp").and_then(Value::as_f64), Some(14000.0));
            assert_eq!(m.get("roomfade").and_then(Value::as_f64), Some(8.0));
        }
        other => panic!("expected reverb control map, got {other:?}"),
    }
}

#[test]
fn get_freq_and_ftrans_aliases_via_koto() {
    let pat = eval(r#"freq(getFreq("c3"))"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => {
            let got = m.get("freq").and_then(Value::as_f64).unwrap();
            let expected = rudel_core::midi_to_freq(rudel_core::note_to_midi("c3").unwrap() as f64);
            assert!((got - expected).abs() < 1e-9);
        }
        other => panic!("expected freq map, got {other:?}"),
    }

    for src in [
        r#"freq(200).fTrans([7, 31])"#,
        r#"freq(200).fTranspose(7)"#,
        r#"freq(200).ftranspose(7)"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }

    let pat = eval(r#"freq(200).fTrans([7, 31])"#).expect("eval");
    let got = values(&pat, 0, 1);
    assert!(!got.is_empty(), "fTrans list aliases should produce haps");

    let pat = eval(r#"freq(220).withBase([440, 220])"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => assert_eq!(m.get("freq").and_then(Value::as_f64), Some(440.0)),
        other => panic!("expected freq map, got {other:?}"),
    }
}

#[test]
fn anchor_scale_stepping_via_koto() {
    // n("0 7").anchor("c5").scale("C:major") -> C5 (72) and C6 (84).
    let pat = eval(r#"n("0 7").anchor("c5").scale("C:major")"#).expect("eval");
    let mut got: Vec<f64> = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .map(|h| match h.value {
            Value::Map(m) => m.get("note").and_then(|v| v.as_f64()).unwrap(),
            other => other.as_f64().unwrap(),
        })
        .collect();
    got.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(got, vec![72.0, 84.0]);
}

#[test]
fn tonal_controls_resolve() {
    for src in [
        r#"note("c3").mtranspose(2)"#,
        r#"note("c3").ctranspose(-3)"#,
        r#"chord("C").anchor("c5").offset(1).octaves(2).voicing()"#,
        r#"chord("C").dictionary("lefthand").voicing()"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }
}
