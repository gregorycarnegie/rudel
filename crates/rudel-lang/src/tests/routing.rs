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
    // The bus is process-global and these tests run in parallel, so this uses a
    // CC number no other test touches rather than clearing the whole bus.
    let pat = eval(r#"ccin(74).segment(4)"#).expect("eval");
    // nothing received yet -> 0
    assert!(values(&pat, 0, 1).iter().all(|v| v.as_f64() == Some(0.0)));
    rudel_core::set_cc(1, 74, 0.5);
    assert!(values(&pat, 0, 1).iter().all(|v| v.as_f64() == Some(0.5)));
    // channel-pinned form + use as a control modulator resolves too
    assert!(eval(r#"note("c3").lpf(ccin(1, 1).range(200, 2000))"#).is_ok());
}

#[test]
fn midin_and_midikeys_read_their_own_device() {
    use rudel_core::{Frac, State, TimeSpan, Value, ValueMap};

    // The CC bus is process-global and these tests run in parallel, so this
    // uses a CC number no other test touches rather than clearing the bus.
    rudel_core::clear_midi_notes();
    // `midin(name)` returns a `(cc[, chan]) -> pattern` factory, and asks the
    // host to open that port.
    let (pat, effects) = crate::eval_with_samples(
        "let cc = midin('keystep')\nnote(\"c3\").lpf(cc(91).range(200, 2000))",
    )
    .expect("eval");
    assert_eq!(effects.midi_inputs, vec!["keystep".to_string()]);
    // A CC from another device doesn't move it; one from `keystep` does.
    rudel_core::set_cc_from("other", 1, 91, 1.0);
    let lpf = |p: &rudel_core::Pattern| match &values(p, 0, 1)[0] {
        Value::Map(m) => m.get("cutoff").and_then(Value::as_f64).unwrap(),
        other => panic!("expected a control map, got {other:?}"),
    };
    assert_eq!(lpf(&pat), 200.0);
    rudel_core::set_cc_from("keystep", 1, 91, 1.0);
    assert_eq!(lpf(&pat), 2000.0);

    // `midikeys(name)` returns a `(noteLength?) -> pattern` factory of the
    // notes played on that port. Notes only surface on a scheduler query.
    let (keys, effects) =
        crate::eval_with_samples("let kb = midikeys('keystep')\nkb(0.25).s(\"tri\")")
            .expect("eval");
    assert_eq!(effects.midi_inputs, vec!["keystep".to_string()]);
    rudel_core::push_midi_note("keystep", 60, 0.5);
    // A plain (visualiser) query leaves the queue alone...
    assert!(keys.query_arc(Frac::zero(), Frac::one()).is_empty());
    // ...a scheduler query picks the note up, for the requested length.
    let controls = ValueMap::from([("cyclist".to_string(), Value::Str("cyclist".to_string()))]);
    let haps = keys.query(&State::with_controls(
        TimeSpan::new(Frac::zero(), Frac::new(1, 4)),
        controls,
    ));
    assert_eq!(haps.len(), 1);
    assert_eq!(haps[0].whole.unwrap().end, Frac::new(1, 4));
    match &haps[0].value {
        Value::Map(m) => {
            assert_eq!(m.get("note").and_then(Value::as_f64), Some(60.0));
            assert_eq!(m.get("velocity").and_then(Value::as_f64), Some(0.5));
            assert_eq!(m.get("s").and_then(Value::as_str), Some("tri"));
        }
        other => panic!("expected a control map, got {other:?}"),
    }
    // The note is consumed, so it doesn't repeat on the next block.
    assert!(keys.query_arc(Frac::zero(), Frac::one()).is_empty());
}

#[test]
fn mouse_signals_read_the_pointer_bus() {
    // `mousex`/`mousey` are pattern *values* like `sine`, not calls.
    rudel_core::set_pointer(0.25, 0.75);
    let x = eval(r#"mousex.segment(2)"#).expect("eval");
    let y = eval(r#"mouseY.segment(2)"#).expect("eval");
    assert!(values(&x, 0, 1).iter().all(|v| v.as_f64() == Some(0.25)));
    assert!(values(&y, 0, 1).iter().all(|v| v.as_f64() == Some(0.75)));
    // and compose like any other signal
    assert!(eval(r#"n(mousex.segment(4).range(0,7)).scale("C:minor")"#).is_ok());
}

#[test]
fn key_down_and_when_key_read_the_live_keyboard() {
    rudel_core::clear_keys();
    // A `:`-list is a combination: every key must be held.
    let down = eval(r#"keyDown("ctrl:j")"#).expect("eval");
    let held = |p: &rudel_core::Pattern| values(p, 0, 1)[0].truthy();
    assert!(!held(&down));
    rudel_core::set_keys_held(["Control"]);
    assert!(!held(&down), "one of the pair is not enough");
    rudel_core::set_keys_held(["Control", "j"]);
    assert!(held(&down));

    // `whenKey` applies its callback while the keys are held, and the check is
    // live: the same pattern responds without being re-evaluated.
    let pat = eval(r#"note("c e").whenKey("ctrl:j", |p| p.fast(2))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4, "held -> fast(2)");
    rudel_core::clear_keys();
    assert_eq!(values(&pat, 0, 1).len(), 2, "released -> untransformed");
}

#[test]
fn log_and_log_values_write_lines_as_events_play() {
    use rudel_core::{drain_log, query_controls};

    // `logValues()` prints the event's controls; the `_log` key never reaches
    // the back-ends.
    drain_log();
    let pat = eval(r#"s("bd sd").logValues()"#).expect("eval");
    let events = query_controls(&pat, 1.0, 0.0, 1.0);
    assert_eq!(events.len(), 2);
    assert!(
        events.iter().all(|e| !e.controls.contains_key("_log")),
        "the log key is consumed by the scheduler"
    );
    assert_eq!(
        drain_log(),
        vec!["[hap] s:bd".to_string(), "[hap] s:sd".to_string()]
    );

    // `log()` adds the whole's span, and nothing is logged until the events
    // are actually scheduled.
    let pat = eval(r#"s("bd").log()"#).expect("eval");
    assert!(drain_log().is_empty(), "building the pattern logs nothing");
    query_controls(&pat, 1.0, 0.0, 1.0);
    assert_eq!(drain_log(), vec!["[hap] 0/1 → 1/1: s:bd".to_string()]);

    // A formatting callback replaces the message.
    let pat = eval(r#"s("bd sd").logValues(|v| 'saw ' + v.s)"#).expect("eval");
    query_controls(&pat, 1.0, 0.0, 1.0);
    assert_eq!(
        drain_log(),
        vec!["saw bd".to_string(), "saw sd".to_string()]
    );
}

#[test]
fn on_trigger_time_fires_its_callback_per_event() {
    // The callback survives evaluation (with the VM that runs it) and is fired
    // by the host as each event's onset passes; the tag never leaks to a
    // back-end.
    let result = crate::eval_result(
        "let seen = []\nexport seen = seen\ns(\"bd sd\").onTriggerTime(|hap| seen.push(hap.value.s))",
    )
    .expect("eval");
    let mut hooks = result.trigger_hooks;
    assert!(!hooks.is_empty());

    let events = rudel_core::query_controls(&result.pattern, 1.0, 0.0, 1.0);
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|e| !e.controls.contains_key(rudel_core::TRIGGER_KEY)),
        "the trigger key is consumed by the scheduler"
    );

    // Firing runs the Koto callback without error.
    for hap in result.pattern.query_arc(Frac::zero(), Frac::one()) {
        assert_eq!(hooks.fire(&hap), None, "callback should not raise");
    }

    // A script with no hook carries none, so the host skips the scan.
    let plain = crate::eval_result(r#"s("bd")"#).expect("eval");
    assert!(plain.trigger_hooks.is_empty());
}

#[test]
fn midimaps_register_control_to_cc_tables() {
    use rudel_core::{ValueMap, midimap_ccs};

    // `midimaps({ name: { control: ccn | {ccn, min, max, exp} } })` writes the
    // process-global registry `rudel-midi` reads at schedule time. These tests
    // run in parallel, so this uses map names no other test touches.
    eval(
        r#"midimaps({
             lang_test_map: { lpf: 74 },
             lang_test_ranged: { room: { ccn: 91, min: 0, max: 2, exp: 0.5 } },
           })"#,
    )
    .expect("eval");

    let controls = |pairs: &[(&str, f64)]| -> ValueMap {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), Value::F64(*v)))
            .collect()
    };
    // A bare number is `{ ccn }` over 0..1.
    assert_eq!(
        midimap_ccs("lang_test_map", &controls(&[("cutoff", 0.25)])),
        [(74, 0.25)]
    );
    // The table form carries the range and curve: 0.5/2 = 0.25, ^0.5 = 0.5.
    assert_eq!(
        midimap_ccs("lang_test_ranged", &controls(&[("room", 0.5)])),
        [(91, 0.5)]
    );

    // `.midimap(name)` is a control, so it rides along on the hap that selects
    // the table; the standalone factory form works too.
    let pat = eval(r#"note("c3").lpf(500).midimap("lang_test_map")"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => assert_eq!(
            m.get("midimap").and_then(Value::as_str),
            Some("lang_test_map")
        ),
        other => panic!("expected control map, got {other:?}"),
    }
    assert!(eval(r#"defaultmidimap({ lpf: 74 })"#).is_ok());

    // The string form names a JSON source the host fetches, recorded as an
    // effect like `samples(...)` rather than registered during eval.
    let (_, effects) = crate::eval_with_samples(r#"midimaps("github:user/repo")"#).expect("eval");
    assert_eq!(effects.midimaps, vec!["github:user/repo".to_string()]);
    // The inline form is applied in-process, so it records no host effect.
    let (_, effects) =
        crate::eval_with_samples(r#"midimaps({ lang_inline_map: { lpf: 74 } })"#).expect("eval");
    assert!(effects.midimaps.is_empty());
    assert_eq!(
        midimap_ccs("lang_inline_map", &controls(&[("cutoff", 1.0)])),
        [(74, 1.0)]
    );
}
