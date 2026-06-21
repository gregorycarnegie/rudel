use super::common::*;
use std::collections::BTreeMap;

// `modulate`/`lfo`/`env`/`bmod` build a nested modulator descriptor in the hap
// value. These tests pin the Koto-level surface; the descriptor shape itself is
// unit-tested in rudel-core's `modulate` module.

/// The first hap's control map over cycle 0.
fn ctrl_map(pat: &Pattern) -> BTreeMap<String, Value> {
    match values(pat, 0, 1).into_iter().next() {
        Some(Value::Map(m)) => m.into_iter().collect(),
        other => panic!("expected a control map, got {other:?}"),
    }
}

/// Navigate `map[type][id]` (the modulator entry) as a sorted map.
fn entry(map: &BTreeMap<String, Value>, ty: &str, id: &str) -> BTreeMap<String, Value> {
    let t = match map.get(ty) {
        Some(Value::Map(t)) => t,
        other => panic!("expected `{ty}` modulator map, got {other:?}"),
    };
    match t.get(id) {
        Some(Value::Map(e)) => e.clone().into_iter().collect(),
        other => panic!("expected entry `{ty}[{id}]`, got {other:?}"),
    }
}

#[test]
fn lfo_method_defaults_to_the_previous_control() {
    // `.lpf(500).lfo({rate: 2})` modulates `cutoff` (what lpf writes).
    let pat = eval(r#"s("saw").lpf(500).lfo({rate: 2})"#).expect("eval");
    let e = entry(&ctrl_map(&pat), "lfo", "0");
    assert_eq!(e.get("control").and_then(Value::as_str), Some("cutoff"));
    assert_eq!(e.get("rate").and_then(Value::as_f64), Some(2.0));
}

#[test]
fn lfo_explicit_control_via_alias() {
    // `lfo({c: "lpf", depth: 4})`: `c` aliases control and is canonicalised to
    // `cutoff`; `depth` is set.
    let pat = eval(r#"s("saw").lfo({c: "lpf", depth: 4})"#).expect("eval");
    let e = entry(&ctrl_map(&pat), "lfo", "0");
    assert_eq!(e.get("control").and_then(Value::as_str), Some("cutoff"));
    assert_eq!(e.get("depth").and_then(Value::as_f64), Some(4.0));
}

#[test]
fn env_method_uses_env_aliases() {
    // `.env({a: 1, d: 0.5})`: env-specific `a`->attack, `d`->decay.
    let pat = eval(r#"s("saw").lpf(500).env({a: 1, d: 0.5})"#).expect("eval");
    let e = entry(&ctrl_map(&pat), "env", "0");
    assert_eq!(e.get("attack").and_then(Value::as_f64), Some(1.0));
    assert_eq!(e.get("decay").and_then(Value::as_f64), Some(0.5));
}

#[test]
fn standalone_lfo_builds_on_empty_map() {
    // The standalone factory `lfo(config)` == `pure({}).lfo(config)`.
    let pat = eval(r#"lfo({c: "lpf", rate: 3})"#).expect("eval");
    let e = entry(&ctrl_map(&pat), "lfo", "0");
    assert_eq!(e.get("control").and_then(Value::as_str), Some("cutoff"));
    assert_eq!(e.get("rate").and_then(Value::as_f64), Some(3.0));
}

#[test]
fn named_id_keys_the_modulator_entry() {
    // A string id is used as the entry key (so a later transform can target it).
    let pat = eval(r#"s("saw").lpf(500).lfo({depth: 4}, "lpf_mod")"#).expect("eval");
    let e = entry(&ctrl_map(&pat), "lfo", "lpf_mod");
    assert_eq!(e.get("depth").and_then(Value::as_f64), Some(4.0));
}

#[test]
fn chained_lfos_increment_ids() {
    // Two `.lfo()` calls produce ids 0 and 1, the second chaining to the first.
    let pat = eval(r#"s("saw").lpf(500).lfo().lfo()"#).expect("eval");
    let map = ctrl_map(&pat);
    assert_eq!(
        entry(&map, "lfo", "0")
            .get("control")
            .and_then(Value::as_str),
        Some("cutoff")
    );
    assert_eq!(
        entry(&map, "lfo", "1")
            .get("control")
            .and_then(Value::as_str),
        Some("lfo_0")
    );
}
