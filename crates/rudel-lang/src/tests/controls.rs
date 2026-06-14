use super::common::*;

#[test]
fn bank_control_sets_the_bank_key() {
    let pat = eval(r#"s("bd").bank("RolandTR909")"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => {
            assert_eq!(m.get("s").and_then(|v| v.as_str()), Some("bd"));
            assert_eq!(m.get("bank").and_then(|v| v.as_str()), Some("RolandTR909"));
        }
        other => panic!("expected control map, got {other:?}"),
    }
}

#[test]
fn loop_controls_set_their_keys() {
    // `loop` is a Koto keyword but is a valid method name after `.`.
    let pat = eval(r#"s("break").loop(1).loopBegin(0.25).loopEnd(0.75)"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => {
            assert_eq!(m.get("loop").and_then(|v| v.as_f64()), Some(1.0));
            assert_eq!(m.get("loopBegin").and_then(|v| v.as_f64()), Some(0.25));
            assert_eq!(m.get("loopEnd").and_then(|v| v.as_f64()), Some(0.75));
        }
        other => panic!("expected control map, got {other:?}"),
    }
}

#[test]
fn partials_sets_a_list_control() {
    let pat = eval(r#"note("c3").s("sawtooth").partials([1, 0.5, 0.25]).phases([0, 0.25])"#)
        .expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => {
            match m.get("partials") {
                Some(Value::List(items)) => assert_eq!(items.len(), 3),
                other => panic!("expected a partials list, got {other:?}"),
            }
            assert!(matches!(m.get("phases"), Some(Value::List(_))));
        }
        other => panic!("expected control map, got {other:?}"),
    }
}

#[test]
fn ctrl_sets_an_arbitrary_control_key() {
    let pat = eval(r#"s("sine").ctrl("fmi20", 3).ctrl("fmh3", 1.5)"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => {
            assert_eq!(m.get("fmi20").and_then(|v| v.as_f64()), Some(3.0));
            assert_eq!(m.get("fmh3").and_then(|v| v.as_f64()), Some(1.5));
        }
        other => panic!("expected control map, got {other:?}"),
    }
}

#[test]
fn filter_and_transpose_aliases_resolve() {
    // Previously-missing aliases should now evaluate without error.
    for src in [
        r#"note("c2").lpf(800).lpq(0.5)"#,
        r#"note("c2").hpf(400).bpf("200 800")"#,
        r#"note("c2").trans(7)"#,
        r#"note("c2").s("sawtooth").attack(0.1).decay(0.1).sustain(0.2).release(0.1)"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }
}

#[test]
fn filter_envelopes_and_noise_resolve() {
    for src in [
        r#"note("c2").s("sawtooth").lpf(200).lpenv(4).lpa(0.1).lpd(0.2)"#,
        r#"note("c2").hpf(2000).hpenv(-3)"#,
        r#"s("white pink brown").lpf(1000)"#,
        r#"note("c2").s("saw").vowel("<a e i o>")"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }
}

#[test]
fn supersaw_fm_adsr_resolve() {
    for src in [
        r#"note("c2").s("supersaw").unison(7).detune(20).spread(0.4)"#,
        r#"note("c3").s("sine").fm(4).fmh(2)"#,
        r#"note("c3").s("sine").fm(8).fmh(3).fmwave("square").fmattack(0.2).fmdecay(0.1).fmsustain(0.3).fmrelease(0.2)"#,
        // two-operator FM chain via named op-2 controls
        r#"note("c3").s("sine").fm(4).fmh(2).fmi2(5).fmh2(3).fmwave2("triangle")"#,
        // arbitrary matrix edge / higher operator via the generic ctrl
        r#"note("c3").s("sine").fm(4).ctrl("fmi20", 3).ctrl("fmh3", 1.5)"#,
        r#"note("c3").s("pulse").pw("<0.1 0.5 0.9>")"#,
        r#"note("c3").s("saw").noise(0.3).penv(12).pattack(0.2).pcurve(1)"#,
        r#"s("bd*4").adsr("0.01:0.1:0:0.1")"#,
        r#"note("c3").s("saw").ad("0.01:0.2").hold(0.3)"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }
}

#[test]
fn vibrato_and_pitch_env_resolve() {
    for src in [
        r#"note("c3").s("sine").vib(6).vibmod(0.5)"#,
        r#"note("c3").s("saw").penv(12).patt(0.2)"#,
        r#"note("c3").vibrato(5).vmod(1)"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }
}

#[test]
fn tremolo_phaser_controls_resolve() {
    for src in [
        r#"note("c3").s("saw").tremolo(4).tremolodepth(0.6)"#,
        r#"note("c3").s("saw").phaser(0.5).phaserdepth(0.8)"#,
        r#"note("c3").s("saw").phaserrate(1).phasercenter(800).phasersweep(1500)"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }
    // the control lands on the hap map under its own key
    let pat = eval(r#"note("c3").tremolo(4)"#).expect("eval");
    let has = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .any(|h| match h.value {
            Value::Map(m) => m.get("tremolo").and_then(|v| v.as_f64()) == Some(4.0),
            _ => false,
        });
    assert!(has, "tremolo control should be set on the event map");
}

#[test]
fn extended_strudel_controls_resolve() {
    // A sampling of the wider Strudel control surface: wavetable/warp,
    // ducking, byte-beat, compressor, ZZFX, MIDI, and short aliases.
    for src in [
        r#"note("c3").s("saw").wt(0.5).wtenv(1).warp(0.2).warpmode("sync")"#,
        r#"s("bd").duck(1).duckdepth(0.5).duckattack(0.1)"#,
        r#"s("bd").bb("t*128").bbst(2)"#,
        r#"note("c3").compressor(-20).compressorRatio(4).compressorAttack(0.01)"#,
        r#"note("c3").zrand(0.1).zcrush(4).zzfx(1)"#,
        r#"note("c3").midichan(2).ccn(74).ccv(64).progNum(5)"#,
        r#"note("c3").s("saw").ph(2).trem(4).dt(0.25).dfb(0.5).djf(0.3)"#,
        r#"note("c3").amp(0.8).dur(0.5).gate(0.9).octave(5).oct(4)"#,
        r#"note("c3").distort(2).dist(1).squiz(2).chorus(0.5).drive(0.7)"#,
        r#"s("bd").fadeTime(1).fadeOutTime(2).FXrelease(0.3).fxr(0.3)"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }
    // aliases canonicalize: `duck` writes Strudel's `duckorbit` key, and the
    // camelCase method writes the camelCase key.
    let pat = eval(r#"s("bd").duck(1).compressorKnee(30)"#).expect("eval");
    let has = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .any(|h| match h.value {
            Value::Map(m) => {
                m.get("duckorbit").and_then(|v| v.as_f64()) == Some(1.0)
                    && m.get("compressorKnee").and_then(|v| v.as_f64()) == Some(30.0)
            }
            _ => false,
        });
    assert!(has, "duck/compressorKnee should land on the event map");
}

#[test]
fn envelope_and_midi_helpers_via_koto() {
    // adsr expands a `:`-list into the four envelope controls
    let pat = eval(r#"note("c3").adsr("0.1:0.2:0.5:0.3")"#).expect("eval");
    let has = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .any(|h| match h.value {
            Value::Map(m) => {
                m.get("attack").and_then(|v| v.as_f64()) == Some(0.1)
                    && m.get("release").and_then(|v| v.as_f64()) == Some(0.3)
                    && !m.contains_key("adsr")
            }
            _ => false,
        });
    assert!(has, "adsr should expand into attack/decay/sustain/release");
    // ds sets decay/sustain; control sets ccn/ccv
    let pat = eval(r#"note("c3").ds("0.2:0.4").control("74:64")"#).expect("eval");
    let has = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .any(|h| match h.value {
            Value::Map(m) => {
                m.get("decay").and_then(|v| v.as_f64()) == Some(0.2)
                    && m.get("sustain").and_then(|v| v.as_f64()) == Some(0.4)
                    && m.get("ccn").and_then(|v| v.as_f64()) == Some(74.0)
                    && m.get("ccv").and_then(|v| v.as_f64()) == Some(64.0)
            }
            _ => false,
        });
    assert!(has, "ds/control should expand into their control pairs");
}

#[test]
fn as_and_scrub_via_koto() {
    // `as` maps positional values into named controls
    let pat = eval(r#"pat("c:0.5").as("note:clip")"#).expect("eval");
    let has = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .any(|h| match h.value {
            Value::Map(m) => {
                m.get("note") == Some(&Value::Str("c".into()))
                    && m.get("clip").and_then(|v| v.as_f64()) == Some(0.5)
            }
            _ => false,
        });
    assert!(has, "as should map values into note/clip");
    // scrub takes structure from the positions pattern and sets begin/clip
    let pat = eval(r#"s("amen").scrub("0.25 0.5")"#).expect("eval");
    let haps = pat.query_arc(Frac::zero(), Frac::one());
    assert_eq!(haps.len(), 2, "scrub structure comes from positions");
    let has = haps.into_iter().any(|h| match h.value {
        Value::Map(m) => {
            m.get("begin").and_then(|v| v.as_f64()) == Some(0.25)
                && m.get("clip").and_then(|v| v.as_f64()) == Some(1.0)
        }
        _ => false,
    });
    assert!(has, "scrub should set begin and clip");
}

#[test]
fn numbered_fm_controls_via_koto() {
    for src in [
        r#"note("c3").s("sine").fm(4).fm2(2).fm3(1).fmh3(2.01).fmwave4("square")"#,
        r#"note("c3").fmattack5(0.1).fmdec6(0.2).fmsus7(0.5).fmrel8(0.3)"#,
        r#"note("c3").fmenv2("lin").fme3("exp")"#,
        r#"note("c3").fmi13(0.5).fm20(3).fmi81(0.1)"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }
    // matrix alias fm23 writes the canonical fmi23 key
    let pat = eval(r#"note("c3").fm23(0.5)"#).expect("eval");
    let has = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .any(|h| match h.value {
            Value::Map(m) => m.get("fmi23").and_then(|v| v.as_f64()) == Some(0.5),
            _ => false,
        });
    assert!(has, "fm23 should write fmi23");
}

#[test]
fn mode_control_sets_mode_and_anchor() {
    // `mode("below:G4")` sets both `mode` and `anchor` on the event map.
    let pat = eval(r#"note("c4").mode("below:G4")"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => {
            assert_eq!(m.get("mode").and_then(|v| v.as_str()), Some("below"));
            assert_eq!(m.get("anchor").and_then(|v| v.as_str()), Some("G4"));
        }
        other => panic!("expected control map, got {other:?}"),
    }
}

#[test]
fn dry_control_sets_its_key() {
    let pat = eval(r#"note("c3").room(0.8).dry(0.3)"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => assert_eq!(m.get("dry").and_then(|v| v.as_f64()), Some(0.3)),
        other => panic!("expected control map, got {other:?}"),
    }
}

#[test]
fn ftype_control_sets_its_key() {
    let pat = eval(r#"note("c3").lpf(800).ftype(2)"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => assert_eq!(m.get("ftype").and_then(|v| v.as_f64()), Some(2.0)),
        other => panic!("expected control map, got {other:?}"),
    }
}

#[test]
fn arithmetic_on_a_control_and_a_bare_scalar_is_a_no_op() {
    // value.mjs `unionWithObj` issue #1026 guard: a control map combined with a
    // bare scalar (wrapped to `{value: x}`) is refused — the control is returned
    // unchanged. Verified against current Strudel: `n("0 2 4").add(7)` keeps
    // `{n:0},{n:2},{n:4}` (Strudel also logs a warning we have no logger for).
    for src in [
        r#"n("0 2 4").add(7)"#,
        r#"n("0 2 4").add("7")"#,
        r#"n("0 2 4").mul(2)"#,
    ] {
        let pat = eval(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        for v in values(&pat, 0, 1) {
            match v {
                Value::Map(m) => {
                    assert_eq!(m.len(), 1, "{src}: scalar leaked into control: {m:?}");
                    assert!(m.contains_key("n"), "{src}: expected only `n`: {m:?}");
                }
                other => panic!("{src}: expected control map, got {other:?}"),
            }
        }
    }
}

#[test]
fn arithmetic_between_two_controls_combines_on_shared_keys() {
    // Control-to-control arithmetic still applies the op on shared keys (the
    // guard only fires for a `{value: x}` scalar right operand).
    let pat = eval(r#"n("0 2 4").add(n("7"))"#).expect("eval");
    let ns: Vec<f64> = values(&pat, 0, 1)
        .iter()
        .map(|v| match v {
            Value::Map(m) => m.get("n").and_then(|x| x.as_f64()).expect("n key"),
            other => panic!("expected control map, got {other:?}"),
        })
        .collect();
    assert_eq!(ns, vec![7.0, 9.0, 11.0]);

    // A scalar on the *left* keeps its wrapped `value` and unions the control
    // (the guard checks the right operand only).
    let pat = eval(r#"add(n("10"), "0 2")"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => {
            assert_eq!(m.get("value").and_then(|v| v.as_f64()), Some(0.0));
            assert_eq!(m.get("n").and_then(|v| v.as_f64()), Some(10.0));
        }
        other => panic!("expected merged map, got {other:?}"),
    }
}
