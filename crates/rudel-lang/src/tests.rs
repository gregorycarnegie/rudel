use super::*;
use rudel_core::{Frac, Pattern, Value};

fn values(pat: &Pattern, b: i64, e: i64) -> Vec<Value> {
    let mut haps = pat.query_arc(Frac::int(b), Frac::int(e));
    haps.sort_by_key(|h| h.part.begin);
    haps.into_iter().map(|h| h.value).collect()
}

#[test]
fn eval_simple_pattern() {
    let pat = eval(r#"note("c4 e4 g4").fast(2)"#).expect("eval");
    let haps = pat.query_arc(Frac::zero(), Frac::one());
    assert_eq!(haps.len(), 6);
}

#[test]
fn eval_stack_and_controls() {
    let pat = eval(r#"stack(s("bd*2"), note("c4 e4").gain(0.5))"#).expect("eval");
    assert!(!pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}

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
fn samples_collects_sources_and_keeps_the_pattern() {
    let (pat, effects) = eval_with_samples(
        r#"
samples("github:tidalcycles/dirt-samples")
samples("local:")
s("bd sd")
"#,
    )
    .expect("eval");
    assert_eq!(
        effects.sources,
        vec![
            "github:tidalcycles/dirt-samples".to_string(),
            "local:".to_string()
        ]
    );
    // the trailing pattern is still returned
    assert!(!pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}

#[test]
fn samples_alone_evaluates_to_silence() {
    let (pat, effects) = eval_with_samples(r#"samples("github:x/y")"#).expect("eval");
    assert_eq!(effects.sources, vec!["github:x/y".to_string()]);
    assert!(pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}

#[test]
fn samples_inline_map_collects_json_and_base() {
    let (_pat, effects) = eval_with_samples(
        r#"samples({ bd: "808bd/a.wav", sd: ["s/c.wav", "s/d.wav"] }, "https://x.com/")"#,
    )
    .expect("eval");
    assert!(effects.sources.is_empty());
    assert_eq!(effects.maps.len(), 1);
    let (json, base) = &effects.maps[0];
    assert_eq!(base, "https://x.com/");
    // Round-trip the serialized JSON to check the shape is preserved.
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(parsed["bd"], serde_json::json!("808bd/a.wav"));
    assert_eq!(parsed["sd"], serde_json::json!(["s/c.wav", "s/d.wav"]));
}

#[test]
fn strudel_const_comments_and_urls_preprocess() {
    let (pat, effects) = eval_with_samples(
        r#"
samples("https://example.test/a//b")
const gainnn = ["2", "3"] // this should disappear
pick(gainnn, 0)
"#,
    )
    .expect("eval");
    assert_eq!(
        effects.sources,
        vec!["https://example.test/a//b".to_string()]
    );
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(2)]);
}

#[test]
fn set_cps_collects_tempo_effect() {
    let (pat, effects) = eval_with_samples(
        r#"
setCps(140/60/4)
s("bd")
"#,
    )
    .expect("eval");
    assert_eq!(effects.cps, Some(140.0 / 60.0 / 4.0));
    assert!(!pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}

#[test]
fn set_cpm_alias_collects_tempo_effect() {
    let (_pat, effects) = eval_with_samples(
        r#"
setcpm(120/4)
s("bd")
"#,
    )
    .expect("eval");
    assert_eq!(effects.cps, Some((120.0 / 4.0) / 60.0));
}

#[test]
fn labels_stack_into_the_returned_pattern() {
    let pat = eval(
        r#"
bassline: s("bd")
main_arp: note("c")
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
    assert!(ids.contains(&"bassline".to_string()));
    assert!(ids.contains(&"main_arp".to_string()));
}

#[test]
fn pick_supports_lists_methods_and_string_pattern_chains() {
    let pat = eval(r#"pick(["a", "b"], 1)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Str("b".to_string())]);

    let pat = eval(r#""1".pick(["a", "b"])"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Str("b".to_string())]);

    let pat = eval(
        r#"
xs = ["0", "1"]
pick(xs, "<0 1>".slow(2))
"#,
    )
    .expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0)]);
}

#[test]
fn compact_strudel_performance_script_shape_evaluates() {
    let (pat, effects) = eval_with_samples(
        r#"
setCps(140/60/4)

samples('github:algorave-dave/samples')
samples('github:tidalcycles/dirt-samples')

const gainnn = [
  "2",
  "{0.75 2.5}*4",
]

const Structures = [
  "~",
  "x*4",
]

const gooo = 1
// off/on

bassline: note("[eb1, eb2]!16 [f2, f1]!16")
  .sound("supersaw")
  .postgain(pick(gainnn, gooo))

const arpeggiator = [
  "{d4 bb3 eb3}%16",
  "{c4 bb3 f3}%16",
  "{d4 bb3 g3}%16",
  "{c4 bb3 f3}%16",
]

main_arp: note(pick(arpeggiator, "<0 1 2 3>".slow(2)))//.rev()
  .sound("supersaw")
  .postgain(pick(gainnn, gooo))

drums: stack(
  s("tech:5").postgain(6).struct(pick(Structures, gooo)),
)
"#,
    )
    .expect("eval");
    assert_eq!(effects.cps, Some(140.0 / 60.0 / 4.0));
    assert_eq!(
        effects.sources,
        vec![
            "github:algorave-dave/samples".to_string(),
            "github:tidalcycles/dirt-samples".to_string(),
        ]
    );
    assert!(!pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}

#[test]
fn alias_bank_collects_pairs() {
    let (_pat, effects) =
        eval_with_samples(r#"aliasBank("RolandTR909", "tr909", "909")"#).expect("eval");
    assert_eq!(
        effects.bank_aliases,
        vec![
            ("RolandTR909".to_string(), "tr909".to_string()),
            ("RolandTR909".to_string(), "909".to_string()),
        ]
    );
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
fn non_pattern_result_errors() {
    assert!(eval("1 + 2").is_err());
}

#[test]
fn every_with_koto_callback() {
    // every(2, |x| x.add(10)): cycle 0 -> 10, cycle 1 -> 0
    let pat = eval(r#"seq(0).every(2, |x| x.add(10))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(10));
    assert_eq!(values(&pat, 1, 2)[0], Value::Int(0));
}

#[test]
fn superimpose_with_koto_callback() {
    // superimpose(|x| x.add(7)) over a single value -> two haps
    let pat = eval(r#"seq(0).superimpose(|x| x.add(7))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0), Value::Int(7)]);
}

#[test]
fn jux_with_koto_callback() {
    let pat = eval(r#"note("0 1").jux(|x| x.rev())"#).expect("eval");
    let pans: Vec<f64> = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .filter_map(|h| match h.value {
            Value::Map(m) => m.get("pan").and_then(|v| v.as_f64()),
            _ => None,
        })
        .collect();
    assert!(pans.contains(&0.0) && pans.contains(&1.0));

    let pat = eval(r#"note("0 1").jux(rev)"#).expect("eval");
    assert!(!pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}

#[test]
fn within_with_koto_callback() {
    // apply +10 only to the first 40% of the cycle -> events 0 and 1
    let pat = eval(r#"seq(0, 1, 2, 3).within(0, 0.4, |x| x.add(10))"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(10), Value::Int(11), Value::Int(2), Value::Int(3)]
    );
}

#[test]
fn chunk_with_koto_callback() {
    // chunk(4, +10): first element bumped on cycle 0
    let pat = eval(r#"seq(0, 1, 2, 3).chunk(4, |x| x.add(10))"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(10), Value::Int(1), Value::Int(2), Value::Int(3)]
    );
}

#[test]
fn off_with_koto_callback() {
    // off(0.25, +12) stacks a shifted, transposed copy: two onsets per cycle
    let pat = eval(r#"note(0).off(0.25, |x| x.add(12))"#).expect("eval");
    let onsets = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .filter(|h| h.has_onset())
        .count();
    assert_eq!(onsets, 2);
}

#[test]
fn range_scales_signal() {
    let pat = eval(r#"seq(0, 1).range(10, 20)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(10.0), Value::F64(20.0)]);
}

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
fn signals_are_values_and_segment() {
    // sine is a value (no parens) and can be segmented + ranged
    let pat = eval(r#"sine.range(0, 10).segment(4)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
    // run(4) -> 0 1 2 3
    let pat = eval(r#"run(4)"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)]
    );
    // rand / perlin / saw2 usable bare
    for s in [
        "rand.segment(8)",
        "perlin.segment(8)",
        "saw2.segment(4)",
        "irand(8).segment(4)",
    ] {
        assert!(eval(s).is_ok(), "should eval: {s}");
    }
}

#[test]
fn factories_resolve() {
    // slowcat: one value per cycle
    let pat = eval(r#"slowcat(0, 1, 2)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(0));
    assert_eq!(values(&pat, 1, 2)[0], Value::Int(1));
    // pure literal, gap silence, fastcat/randcat resolve
    assert_eq!(
        values(&eval("pure(60)").unwrap(), 0, 1),
        vec![Value::Int(60)]
    );
    assert!(
        eval("gap(2)")
            .unwrap()
            .query_arc(Frac::zero(), Frac::one())
            .is_empty()
    );
    for s in ["fastcat(0, 1, 2)", "randcat(0, 1)", "chooseCycles(0, 1)"] {
        assert!(eval(s).is_ok(), "should eval: {s}");
    }
}

#[test]
fn newly_bound_transforms_resolve() {
    for s in [
        r#"note(0).hurry(2)"#,
        r#"seq(0, 1, 2, 3).focus(0, 0.5)"#,
        r#"seq(0, 1).press_by(0.5)"#,
        r#"s("x").euclid_rot(3, 8, 1)"#,
    ] {
        assert!(eval(s).is_ok(), "should eval: {s}");
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
fn alignment_via_koto() {
    // add.out takes structure from the right pattern -> 3 onsets
    let pat = eval(r#"seq(0, 1).add_out("10 20 30")"#).expect("eval");
    let onsets = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .filter(|h| h.has_onset())
        .count();
    assert_eq!(onsets, 3);
    // set.squeeze merges the s control into each note event -> 4 haps
    let pat = eval(r#"note("0 1").set_squeeze(s("a b"))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
}

#[test]
fn chop_via_koto() {
    let pat = eval(r#"s("bd").chop(4)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
}

#[test]
fn slice_via_koto() {
    let pat = eval(r#"s("bd").slice(4, "0 2")"#).expect("eval");
    let haps = pat.query_arc(Frac::zero(), Frac::one());
    assert_eq!(haps.len(), 2);
    match &haps[0].value {
        Value::Map(m) => assert_eq!(m.get("begin"), Some(&Value::F64(0.0))),
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn layer_stacks_callback_results() {
    // layer([|x| x.add(0), |x| x.add(7)]) over a single value -> two haps
    let pat = eval(r#"seq(0).layer([|x| x.add(0), |x| x.add(7)])"#).expect("eval");
    let mut got = values(&pat, 0, 1);
    got.sort_by_key(|v| v.as_f64().unwrap() as i64);
    assert_eq!(got, vec![Value::Int(0), Value::Int(7)]);
}

#[test]
fn factories_stepcat_arrange_polymeter() {
    // stepcat("0 1 2", "3 4") -> 5 evenly-weighted steps
    let pat = eval(r#"stepcat("0 1 2", "3 4")"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 5);
    // explicit [weight, pat] pairs: "0"@3 then "1" -> 2 onsets, 0 dominates
    let pat = eval(r#"stepcat([3, "0"], [1, "1"])"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0), Value::Int(1)]);
    // arrange: "0" for 2 cycles, "1" for 1
    let pat = eval(r#"arrange([2, "0"], [1, "1"])"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(0));
    assert_eq!(values(&pat, 2, 3)[0], Value::Int(1));
    // polymeter / pm align to lcm(3,2)=6 steps -> 12 haps stacked
    let pat = eval(r#"polymeter("0 1 2", "4 5")"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 12);
    assert!(eval(r#"pm("0 1", "2 3 4")"#).is_ok());
}

#[test]
fn take_drop_scan_via_koto() {
    // seq(0,1,2,3).take(2) -> "0 1"; drop(1) -> "1 2 3"
    let pat = eval(r#"seq(0, 1, 2, 3).take(2)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0), Value::Int(1)]);
    let pat = eval(r#"seq(0, 1, 2, 3).drop(1)"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(1), Value::Int(2), Value::Int(3)]
    );
    // scan(3): cycle 0 -> [0], cycle 2 -> [0 1 2]
    let pat = eval(r#"scan(3)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0)]);
    assert_eq!(
        values(&pat, 2, 3),
        vec![Value::Int(0), Value::Int(1), Value::Int(2)]
    );
}

#[test]
fn weighted_choosers_and_stepalt_via_koto() {
    // wrandcat: heavy weight on 0 dominates, one value per cycle
    let pat = eval(r#"wrandcat([0, 1000], [1, 1])"#).expect("eval");
    let mut zeros = 0;
    for c in 0..12 {
        let v = values(&pat, c, c + 1);
        assert_eq!(v.len(), 1);
        if v[0] == Value::Int(0) {
            zeros += 1;
        }
    }
    assert!(zeros >= 10, "heavy weight should dominate (got {zeros}/12)");
    // wchooseCycles is the same function; wchoose evaluates as continuous
    assert!(eval(r#"wchooseCycles(["a", 2], ["b", 1])"#).is_ok());
    assert!(eval(r#"wchoose([0, 1], [1, 1]).segment(4)"#).is_ok());
    // stepalt(["0 1", "2"], "3") == "0 1 3 2 3"
    let pat = eval(r#"stepalt(["0 1", "2"], "3")"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(3),
            Value::Int(2),
            Value::Int(3),
        ]
    );
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
    // reversing the chord sequence per chord
    let pat = eval(r#"stack(0, 1, 2).arp_with(|c| c.rev())"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(2), Value::Int(1), Value::Int(0)]
    );
    // works per-cycle across an alternation of different chords (probe
    // window discovers both chords)
    let pat = eval(r#"seq("<[0,1] [2,3]>").arp_with(|c| c.rev())"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(1), Value::Int(0)]);
    assert_eq!(values(&pat, 1, 2), vec![Value::Int(3), Value::Int(2)]);
}

#[test]
fn voicing_via_koto() {
    // a chord-symbol pattern voiced below a4: C triad -> C4 E4 G4.
    // (mini-notation can't spell `^`, so use `maj7`/`m7`-style symbols, or
    // pure("C^7") for the literal form.)
    let pat = eval(r#"pure("C").voicing()"#).expect("eval");
    let mut got = values(&pat, 0, 1);
    got.sort_by_key(|v| v.as_f64().unwrap() as i64);
    assert_eq!(
        got,
        vec![Value::F64(60.0), Value::F64(64.0), Value::F64(67.0)]
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
fn ribbon_and_seg_via_koto() {
    // ribbon loops the window [1,3) of "<0 1 2 3>": cycle 0 -> 1, cycle 2 -> 1
    let pat = eval(r#"n("<0 1 2 3>").ribbon(1, 2)"#).expect("eval");
    let n_at = |c: i64| match &pat.query_arc(Frac::int(c), Frac::int(c + 1))[0].value {
        Value::Map(m) => m.get("n").and_then(|v| v.as_f64()).unwrap(),
        other => other.as_f64().unwrap(),
    };
    assert_eq!(n_at(0), 1.0);
    assert_eq!(n_at(1), 2.0);
    assert_eq!(n_at(2), 1.0); // looped
    // rib alias resolves; seg == segment (8 discrete events)
    assert!(eval(r#"n("<0 1>").rib(0, 1)"#).is_ok());
    let pat = eval(r#"rand.seg(8)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 8);
}

#[test]
fn overlay_and_pace_via_koto() {
    let pat = eval(r#"seq(0).overlay(7)"#).expect("eval");
    let mut got = values(&pat, 0, 1);
    got.sort_by_key(|v| v.as_f64().unwrap() as i64);
    assert_eq!(got, vec![Value::Int(0), Value::Int(7)]);
    let pat = eval(r#"seq(0, 1, 2).pace(4)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
}

#[test]
fn camel_case_aliases_resolve() {
    // Strudel-style camelCase aliases should evaluate without error.
    for src in [
        r#"seq(0, 1, 2).iterBack(2)"#,
        r#"s("bd sd").fastGap(2)"#,
        r#"seq(0, 1).repeatCycles(2)"#,
        r#"seq(0, 1).pressBy(0.5)"#,
        r#"seq(0, 1, 2, 3).swingBy(0.25, 2)"#,
        r#"s("x").euclidRot(3, 8, 1)"#,
        r#"note("c3").euclidLegato(3, 8)"#,
        r#"note("c3").euclidLegatoRot(3, 5, 2)"#,
        r#"n("0").scale("C:major").scaleTranspose(2)"#,
        r#"n("0").scale("C:major").scaleTrans(2)"#,
        r#"pure("Am7").rootNotes(3)"#,
        r#"s("bd").loopAt(2)"#,
        r#"sine.toBipolar()"#,
        r#"sine.fromBipolar()"#,
        r#"seq(0, 1).firstOf(2, |x| x.add(10))"#,
        r#"seq(0, 1).lastOf(2, |x| x.add(10))"#,
        r#"seq(0, 1, 2, 3).chunkBack(2, |x| x.add(10))"#,
        r#"note("0 1").juxBy(0.5, rev)"#,
        r#"seq(0, 1).sometimesBy(0.5, |x| x.add(7))"#,
        r#"seq(0, 1).someCycles(|x| x.add(7))"#,
        r#"seq(0, 1).someCyclesBy(0.5, |x| x.add(7))"#,
        r#"seq(0, 1).almostAlways(|x| x.add(7))"#,
        r#"seq(0, 1).almostNever(|x| x.add(7))"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }
}

#[test]
fn apply_always_never_via_koto() {
    // apply/always run the callback; never leaves the pattern unchanged.
    let pat = eval(r#"seq(0).apply(|x| x.add(5))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(5)]);
    let pat = eval(r#"seq(0).always(|x| x.add(5))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(5)]);
    let pat = eval(r#"seq(0).never(|x| x.add(5))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0)]);
}

#[test]
fn step_count_transforms_via_koto() {
    // contract halves the step count; shrink/grow concatenate shrinking views.
    let pat = eval(r#"seq(0, 1, 2, 3).contract(2)"#).expect("eval");
    assert_eq!(pat.steps, Some(Frac::int(2)));
    let pat = eval(r#"seq(0, 1, 2, 3).shrink(1)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 10);
    let pat = eval(r#"seq(0, 1, 2, 3).grow(1)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(0));
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 10);
}

#[test]
fn chord_control_and_voicing_controls_via_koto() {
    // top-level chord(...) plus `.dict()`/`.voicing()` voice a chord symbol.
    let pat = eval(r#"chord("C").voicing()"#).expect("eval");
    let mut got = values(&pat, 0, 1);
    got.sort_by_key(|v| v.as_f64().unwrap() as i64);
    assert_eq!(
        got,
        vec![Value::F64(60.0), Value::F64(64.0), Value::F64(67.0)]
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

#[test]
fn callback_error_is_surfaced() {
    // Referencing an undefined function inside the callback raises.
    let err = eval(r#"seq(0).every(2, |x| x.nonexistent_method())"#);
    assert!(err.is_err());
}
