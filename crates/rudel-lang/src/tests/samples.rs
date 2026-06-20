use super::common::*;

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
fn eval_result_collects_sample_effects() {
    let result = eval_result(r#"samples("github:x/y")"#).expect("eval");
    assert_eq!(result.sample_effects.sources, vec!["github:x/y"]);
    assert!(result.meta.widgets.is_empty());
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
