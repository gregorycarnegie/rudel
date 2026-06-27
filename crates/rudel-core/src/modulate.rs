// modulate.rs - the generic modulation-descriptor builders (`modulate`/`lfo`/
// `env`/`bmod`), ported from `strudel/packages/core/controls.mjs`. Each call
// folds a nested modulator descriptor into the hap's control map:
//
//   s("saw").lpf(500).lfo({ rate: 2 })
//     -> { s:"saw", lpf:500, lfo: { __ids:[0], "0": { control:"lpf", rate:2 } } }
//
// The "default to the control applied just before this in the chain" rule reads
// the *last-inserted* key of the control map, which is why `Value::Map` is an
// insertion-ordered `ValueMap` (see `value.rs`).
//
// Note: this only builds the descriptor in the hap value (parity with Strudel's
// hap output). Binding a modulation source to a node parameter at render time
// (superdough's `connectLFO`/`connectEnvelope`/`connectBusModulator`) is a
// separate audio-graph concern with no current rudel-dsp analog.
// Copyright (C) 2022 Strudel contributors; 2026 Rudel contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    controls::control_name,
    pattern::{Pattern, pure},
    value::{Value, ValueMap},
};

const MODULATOR_KEYS: [&str; 3] = ["lfo", "env", "bmod"];

/// Resolve a config alias to its canonical sub-control name for the given
/// modulator type (`registerSubControls` in controls.mjs). Unknown keys pass
/// through unchanged. Matched case-insensitively, like `getMainSubcontrolName`.
fn main_subcontrol_name(mod_type: &str, raw: &str) -> String {
    // (canonical, [aliases...]) per modulator type.
    let table: &[(&str, &[&str])] = match mod_type {
        "lfo" => &[
            ("control", &["c"]),
            ("subControl", &["sc"]),
            ("rate", &["r"]),
            ("depth", &["dep", "dr"]),
            ("depthabs", &["da"]),
            ("dcoffset", &["dc"]),
            ("shape", &["sh"]),
            ("skew", &["sk"]),
            ("curve", &["cu"]),
            ("sync", &["s"]),
            ("retrig", &["rt"]),
            ("fxi", &[]),
        ],
        "env" => &[
            ("control", &["c"]),
            ("subControl", &["sc"]),
            ("attack", &["att", "a"]),
            ("decay", &["dec", "d"]),
            ("sustain", &["sus", "s"]),
            ("release", &["rel", "r"]),
            ("depth", &["dep", "dr"]),
            ("depthabs", &["da"]),
            ("acurve", &["ac"]),
            ("dcurve", &["dc"]),
            ("rcurve", &["rc"]),
            ("fxi", &[]),
        ],
        "bmod" => &[
            ("bus", &["b"]),
            ("control", &["c"]),
            ("subControl", &["sc"]),
            ("depth", &["dep", "dr"]),
            ("depthabs", &["da"]),
            ("dc", &[]),
            ("fxi", &[]),
        ],
        _ => &[],
    };
    let lower = raw.to_ascii_lowercase();
    for (canonical, aliases) in table {
        if lower == canonical.to_ascii_lowercase() || aliases.iter().any(|a| lower == *a) {
            return canonical.to_string();
        }
    }
    raw.to_string()
}

/// String key an id value maps to (JS object keys are strings; numeric ids
/// render without a decimal point).
fn id_key(id: &Value) -> String {
    match id {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::F64(n) if n.fract() == 0.0 => (*n as i64).to_string(),
        Value::F64(n) => n.to_string(),
        other => other.as_f64().map(|n| n.to_string()).unwrap_or_default(),
    }
}

/// Compute the default control for a freshly-created modulator id: the control
/// applied just before in the chain (the last-inserted key of `vm`). If that is
/// itself a modulator, target its most-recent id (`lfo_0`). `Null` when the map
/// is empty.
fn default_control(vm: &ValueMap) -> Value {
    let Some(last_key) = vm.keys().last() else {
        return Value::Null;
    };
    let control = control_name(last_key);
    if MODULATOR_KEYS.contains(&control.as_str())
        && let Some(Value::Map(sub)) = vm.get(&control)
        && let Some(Value::List(ids)) = sub.get("__ids")
        && let Some(last_id) = ids.last()
    {
        return Value::Str(format!("{control}_{}", id_key(last_id)));
    }
    Value::Str(control)
}

/// One config-key fold: thread the `[v, id]` state, ensuring `v[type][id]`
/// exists and writing `sub_key` from the sampled config value `c`. `is_first`
/// marks the leading `control` entry, which establishes the default control and
/// resolves the id. Mirrors the per-key closure in `Pattern.prototype.modulate`.
fn modulate_step(state: &Value, mod_type: &str, sub_key: &str, is_first: bool, c: &Value) -> Value {
    let Value::List(parts) = state else {
        return state.clone();
    };
    let (v, mut id) = match parts.as_slice() {
        [v, id] => (v.clone(), id.clone()),
        _ => return state.clone(),
    };
    let Value::Map(mut vm) = v else {
        // Not a control map: nothing to modulate, pass the state through.
        return Value::List(vec![v, id]);
    };

    // The default control must be read before `v[type]` is (re)ensured, and is
    // only needed when creating the id's entry (which happens on the first key).
    let default = if is_first {
        default_control(&vm)
    } else {
        Value::Null
    };

    // Ensure `v[type]` exists (a fresh `{ __ids: [] }`). Keep `type` at the end
    // of `vm` so a later modulator's default-control lookup sees it last.
    if !vm.contains_key(mod_type) {
        let mut sub = ValueMap::new();
        sub.insert("__ids".to_string(), Value::List(Vec::new()));
        vm.insert(mod_type.to_string(), Value::Map(sub));
    }
    let mut t = match vm.shift_remove(mod_type) {
        Some(Value::Map(t)) => t,
        _ => ValueMap::new(),
    };

    // Resolve the id: a fresh modulator (`id` unset) takes the next index.
    let ids_len = match t.get("__ids") {
        Some(Value::List(l)) => l.len(),
        _ => 0,
    };
    if id.is_nothing() {
        id = Value::Int(ids_len as i64);
    }
    let key = id_key(&id);

    // Create the id's entry, defaulting its `control`.
    if !t.contains_key(&key) {
        let mut entry = ValueMap::new();
        entry.insert("control".to_string(), default);
        t.insert(key.clone(), Value::Map(entry));
    }
    // Track insertion order (Set semantics: no duplicates).
    if let Some(Value::List(ids)) = t.get_mut("__ids")
        && !ids.iter().any(|x| x == &id)
    {
        ids.push(id.clone());
    }

    // Write the sub-control value (control/subControl names are canonicalised).
    if !c.is_nothing()
        && let Some(Value::Map(entry)) = t.get_mut(&key)
    {
        let value = if (sub_key == "control" || sub_key == "subControl")
            && let Value::Str(s) = c
        {
            Value::Str(control_name(s))
        } else {
            c.clone()
        };
        entry.insert(sub_key.to_string(), value);
    }

    vm.insert(mod_type.to_string(), Value::Map(t));
    Value::List(vec![Value::Map(vm), id])
}

/// Build the modulator descriptor (`Pattern.prototype.modulate`). `config` is
/// the ordered list of `(rawKey, valuePattern)` from the user's config object;
/// `id_pat` carries the optional modulator id (`pure(Null)` when unset). An
/// unknown `mod_type` returns the pattern unchanged.
pub fn modulate(
    pat: &Pattern,
    mod_type: &str,
    config: Vec<(String, Pattern)>,
    id_pat: Pattern,
) -> Pattern {
    if !MODULATOR_KEYS.contains(&mod_type) {
        return pat.clone();
    }

    // `config = { control: undefined, ...config }`: a leading `control` entry is
    // always present and processed first (establishing the default control).
    // Only the literal `control` key merges into it; aliases (`c`) append.
    let mut entries: Vec<(String, Pattern)> = vec![("control".to_string(), pure(Value::Null))];
    for (raw, value_pat) in config {
        if raw == "control" {
            entries[0].1 = value_pat;
        } else {
            entries.push((raw, value_pat));
        }
    }

    // Bind the id into the threaded `[v, id]` state.
    let mut output = pat
        .fmap(|v| Value::func(move |id| Value::List(vec![v.clone(), id])))
        .app_left(&id_pat);

    // Fold each config key, sampling its value with `appLeft`.
    for (i, (raw, value_pat)) in entries.into_iter().enumerate() {
        let sub_key = main_subcontrol_name(mod_type, &raw);
        let mod_type = mod_type.to_string();
        let is_first = i == 0;
        output = output
            .fmap(move |state| {
                let mod_type = mod_type.clone();
                let sub_key = sub_key.clone();
                Value::func(move |c| modulate_step(&state, &mod_type, &sub_key, is_first, &c))
            })
            .app_left(&value_pat);
    }

    // Discard the id, leaving the modified control map.
    output.fmap(|state| match state {
        Value::List(parts) if !parts.is_empty() => parts.into_iter().next().unwrap(),
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Frac;

    /// A control-map pattern with the given keys in insertion order, mirroring a
    /// chain like `s("saw").lpf(500)` -> `{ s:"saw", cutoff:500 }`.
    fn cmap(pairs: &[(&str, Value)]) -> Pattern {
        let mut m = ValueMap::new();
        for (k, v) in pairs {
            m.insert(k.to_string(), v.clone());
        }
        pure(Value::Map(m))
    }

    /// First hap's control map over cycle 0.
    fn first_map(pat: &Pattern) -> ValueMap {
        match pat
            .query_arc(Frac::zero(), Frac::one())
            .first()
            .map(|h| h.value.clone())
        {
            Some(Value::Map(m)) => m,
            other => panic!("expected a control map, got {other:?}"),
        }
    }

    /// The modulator submap for `type`, asserting it exists.
    fn modulator<'a>(m: &'a ValueMap, ty: &str) -> &'a ValueMap {
        match m.get(ty) {
            Some(Value::Map(t)) => t,
            other => panic!("expected `{ty}` modulator map, got {other:?}"),
        }
    }

    fn entry<'a>(t: &'a ValueMap, id: &str) -> &'a ValueMap {
        match t.get(id) {
            Some(Value::Map(e)) => e,
            other => panic!("expected entry `{id}`, got {other:?}"),
        }
    }

    #[test]
    fn lfo_defaults_to_the_previous_control() {
        // `s("saw").lpf(500).lfo({ rate: 2 })` modulates the control set just
        // before — `cutoff` (the key `lpf` writes) — with an id of 0.
        let pat = modulate(
            &cmap(&[("s", Value::Str("saw".into())), ("cutoff", Value::Int(500))]),
            "lfo",
            vec![("rate".to_string(), pure(Value::Int(2)))],
            pure(Value::Null),
        );
        let m = first_map(&pat);
        let t = modulator(&m, "lfo");
        assert_eq!(
            t.get("__ids").cloned(),
            Some(Value::List(vec![Value::Int(0)]))
        );
        let e = entry(t, "0");
        assert_eq!(
            e.get("control").cloned(),
            Some(Value::Str("cutoff".to_string()))
        );
        assert_eq!(e.get("rate").cloned(), Some(Value::Int(2)));
    }

    #[test]
    fn explicit_control_alias_overrides_default() {
        // `lfo({ c: "lpf", depth: 4 })`: `c` targets a control explicitly and is
        // canonicalised (`lpf` -> `cutoff`); `dep`/`dr`/`depth` -> `depth`.
        let pat = modulate(
            &cmap(&[("s", Value::Str("saw".into()))]),
            "lfo",
            vec![
                ("c".to_string(), pure(Value::Str("lpf".to_string()))),
                ("dep".to_string(), pure(Value::Int(4))),
            ],
            pure(Value::Null),
        );
        let m = first_map(&pat);
        let e = entry(modulator(&m, "lfo"), "0");
        assert_eq!(
            e.get("control").cloned(),
            Some(Value::Str("cutoff".to_string()))
        );
        assert_eq!(e.get("depth").cloned(), Some(Value::Int(4)));
    }

    #[test]
    fn chained_lfos_get_incrementing_ids_and_chain_control() {
        // `s("saw").lpf(500).lfo().lfo()`: the first lfo (id 0) modulates cutoff;
        // the second lfo (id 1) defaults to the first lfo's most recent id.
        let first = modulate(
            &cmap(&[("cutoff", Value::Int(500))]),
            "lfo",
            vec![],
            pure(Value::Null),
        );
        let pat = modulate(&first, "lfo", vec![], pure(Value::Null));
        let m = first_map(&pat);
        let t = modulator(&m, "lfo");
        assert_eq!(
            t.get("__ids").cloned(),
            Some(Value::List(vec![Value::Int(0), Value::Int(1)]))
        );
        assert_eq!(
            entry(t, "0").get("control").cloned(),
            Some(Value::Str("cutoff".into()))
        );
        assert_eq!(
            entry(t, "1").get("control").cloned(),
            Some(Value::Str("lfo_0".into()))
        );
    }

    #[test]
    fn env_uses_its_own_alias_table() {
        // `env({ a: 1, d: 0.5 })`: `a`->attack, `d`->decay (env-specific aliases,
        // where in the lfo table `d` would not apply).
        let pat = modulate(
            &cmap(&[("cutoff", Value::Int(500))]),
            "env",
            vec![
                ("a".to_string(), pure(Value::Int(1))),
                ("d".to_string(), pure(Value::F64(0.5))),
            ],
            pure(Value::Null),
        );
        let m = first_map(&pat);
        let e = entry(modulator(&m, "env"), "0");
        assert_eq!(e.get("attack").cloned(), Some(Value::Int(1)));
        assert_eq!(e.get("decay").cloned(), Some(Value::F64(0.5)));
        assert_eq!(e.get("control").cloned(), Some(Value::Str("cutoff".into())));
    }

    #[test]
    fn named_id_is_used_as_the_entry_key() {
        // A string id keys the entry, so a later `sometimes` can update it.
        let pat = modulate(
            &cmap(&[("cutoff", Value::Int(500))]),
            "lfo",
            vec![("depth".to_string(), pure(Value::Int(4)))],
            pure(Value::Str("lpf_mod".to_string())),
        );
        let m = first_map(&pat);
        let t = modulator(&m, "lfo");
        assert_eq!(
            t.get("__ids").cloned(),
            Some(Value::List(vec![Value::Str("lpf_mod".to_string())]))
        );
        assert_eq!(
            entry(t, "lpf_mod").get("depth").cloned(),
            Some(Value::Int(4))
        );
    }

    #[test]
    fn unknown_modulator_type_is_a_noop() {
        let pat = modulate(
            &cmap(&[("s", Value::Str("saw".into()))]),
            "wobble",
            vec![],
            pure(Value::Null),
        );
        let m = first_map(&pat);
        assert!(!m.contains_key("wobble"));
        assert_eq!(m.get("s").cloned(), Some(Value::Str("saw".to_string())));
    }
}
