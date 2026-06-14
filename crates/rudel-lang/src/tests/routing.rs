use super::common::*;

#[test]
fn per_pattern_naming_and_mute() {
    // `.p(name)` tags the pattern with an `id`.
    let pat = eval(r#"s("bd").p("drums")"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => assert_eq!(m.get("id").and_then(|v| v.as_str()), Some("drums")),
        other => panic!("expected control map, got {other:?}"),
    }

    // `$:` is an anonymous per-pattern label that stacks into the result.
    let pat = eval(
        r#"
$: s("bd")
$: note("c4")
"#,
    )
    .expect("eval");
    assert!(!pat.query_arc(Frac::zero(), Frac::one()).is_empty());

    // comments-as-mute: a commented label line drops out of the stack.
    let pat = eval(
        r#"
drums: s("bd sd")
// bass: note("c2 c2 c2 c2")
"#,
    )
    .expect("eval");
    let ids: Vec<String> = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .filter_map(|h| match h.value {
            Value::Map(m) => m
                .get("id")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            _ => None,
        })
        .collect();
    assert!(ids.contains(&"drums".to_string()));
    assert!(!ids.contains(&"bass".to_string()));
}

#[test]
fn midi_osc_routing_tags_and_filter() {
    // `.midi()` / `.osc()` tag haps with the `_io` routing control.
    let pat = eval(r#"stack(note("c4").midi(), s("bd").osc(), s("hh"))"#).expect("eval");
    let (midi, osc) = output_targets(&pat);
    assert!(midi && osc, "both midi and osc tags should be detected");

    // The audio slice keeps only the untagged hap (hh), and strips `_io`.
    let audio = filter_output(&pat, "audio", true);
    let audio_vals = audio.query_arc(Frac::zero(), Frac::one());
    assert_eq!(audio_vals.len(), 1);
    for h in &audio_vals {
        if let Value::Map(m) = &h.value {
            assert!(!m.contains_key("_io"), "_io must be stripped");
            assert_eq!(m.get("s").and_then(|v| v.as_str()), Some("hh"));
        }
    }

    // The midi slice keeps only the `.midi()`-tagged hap (note c4).
    let midi_slice = filter_output(&pat, "midi", false);
    let midi_vals = midi_slice.query_arc(Frac::zero(), Frac::one());
    assert_eq!(midi_vals.len(), 1);
    assert!(matches!(&midi_vals[0].value, Value::Map(m) if m.contains_key("note")));

    // The osc slice keeps only the `.osc()`-tagged hap (bd).
    let osc_slice = filter_output(&pat, "osc", false);
    assert_eq!(osc_slice.query_arc(Frac::zero(), Frac::one()).len(), 1);
}

#[test]
fn osc_method_sets_host_and_port() {
    // `.osc("host:port")` also sets the oschost/oscport routing controls.
    let pat = eval(r#"s("bd").osc("10.0.0.2:9000")"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => {
            assert_eq!(m.get("oschost").and_then(|v| v.as_str()), Some("10.0.0.2"));
            assert_eq!(m.get("oscport").and_then(|v| v.as_f64()), Some(9000.0));
        }
        other => panic!("expected control map, got {other:?}"),
    }
}

#[test]
fn midi_method_stores_device_hint() {
    // `.midi("IAC")` records the device hint as `_midiport` (stripped on route).
    let pat = eval(r#"note("c4").midi("IAC")"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => {
            assert_eq!(m.get("_io").and_then(|v| v.as_str()), Some("midi"));
            assert_eq!(m.get("_midiport").and_then(|v| v.as_str()), Some("IAC"));
        }
        other => panic!("expected control map, got {other:?}"),
    }
    // filter_output strips both routing keys.
    let slice = filter_output(&pat, "midi", false);
    if let Value::Map(m) = &values(&slice, 0, 1)[0] {
        assert!(!m.contains_key("_io") && !m.contains_key("_midiport"));
    }
}

#[test]
fn ccin_reads_the_midi_input_bus() {
    // `ccin(cc)` is a live 0..1 signal of the latest incoming control-change.
    rudel_core::clear_cc();
    let pat = eval(r#"ccin(74).segment(4)"#).expect("eval");
    // nothing received yet -> 0
    assert!(values(&pat, 0, 1).iter().all(|v| v.as_f64() == Some(0.0)));
    rudel_core::set_cc(1, 74, 0.5);
    assert!(values(&pat, 0, 1).iter().all(|v| v.as_f64() == Some(0.5)));
    // channel-pinned form + use as a control modulator resolves too
    assert!(eval(r#"note("c3").lpf(ccin(1, 1).range(200, 2000))"#).is_ok());
}
