// widgets.rs - evaluated inline-widget options.
// The source scan in `preprocess/widgets.rs` can only read option *literals*
// (`.pianoroll({cycles: 4})`), because it runs before the script does. The
// rewrite passes the original argument through to the widget method, though, so
// by the time that method runs the script has evaluated the object —
// `{cycles: n}` or `{cycles: bars * 2}` included. This registry carries those
// evaluated options back out of the run so the host can merge them over the
// scanned ones.
//
// Same shape as the slider registry next door: a process-global map keyed by
// widget id, cleared at the start of each evaluation.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{WidgetOption, js::Arg};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{LazyLock, RwLock},
};

type OptionMap = BTreeMap<String, WidgetOption>;

static WIDGET_OPTIONS: LazyLock<RwLock<HashMap<String, OptionMap>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Convert one evaluated value into a widget option. Mirrors the literal forms
/// the source scan accepts, so a computed value and a written-out one land as
/// the same `WidgetOption`.
fn option_from_arg(key: &str, value: &Arg) -> Option<WidgetOption> {
    match value {
        Arg::Bool(b) => Some(WidgetOption::Bool(*b)),
        Arg::Num(n) => Some(WidgetOption::Number(*n)),
        Arg::Str(s) => Some(WidgetOption::String(s.clone())),
        // A hydra chain arrives as an object and leaves as the WGSL it
        // compiles to, so the shader is generated once per evaluation rather
        // than once per frame, and the widget host needs to know nothing about
        // hydra beyond "this option is a shader".
        //
        // The key decides which output buffer the chain is bound to, because
        // that is what `prev()` inside it reads. `chain` is `o0` under its
        // single-output name.
        Arg::Hydra(chain) => Some(WidgetOption::String(crate::hydra::compile(
            chain,
            hydra_output(key),
        ))),
        _ => None,
    }
}

/// The output buffer a widget option name binds its chain to.
fn hydra_output(key: &str) -> usize {
    match key {
        "o1" => 1,
        "o2" => 2,
        "o3" => 3,
        _ => 0,
    }
}

/// Read an evaluated `{key: value}` widget-option object. Values that are not a
/// bool/number/string/chain (a nested object, a function) are skipped rather
/// than failing the evaluation — the scanned literal, or the painter's
/// default, still stands for them.
pub(crate) fn options_from_arg(value: &Arg) -> OptionMap {
    let Arg::Map(map) = value else {
        return OptionMap::new();
    };
    map.iter()
        .filter_map(|(key, v)| Some((key.clone(), option_from_arg(key, v)?)))
        .collect()
}

/// Record the options a widget call evaluated to (called from the widget
/// method during the run).
pub(crate) fn record_options(id: &str, options: OptionMap) {
    if options.is_empty() {
        return;
    }
    WIDGET_OPTIONS
        .write()
        .unwrap()
        .insert(id.to_string(), options);
}

/// Drop every recorded option map, so one evaluation cannot see the previous
/// one's widgets (the ids are position-derived and do get reused).
pub(crate) fn reset_options() {
    WIDGET_OPTIONS.write().unwrap().clear();
}

/// The options recorded for `id` during the run, if any.
pub(crate) fn recorded_options(id: &str) -> Option<OptionMap> {
    WIDGET_OPTIONS.read().unwrap().get(id).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluated_values_map_onto_the_literal_option_forms() {
        let map = Arg::Map(vec![
            ("cycles".to_string(), Arg::Num(4.0)),
            ("vertical".to_string(), Arg::Bool(true)),
            ("mode".to_string(), Arg::Str("polygon".to_string())),
            // A value shape no option takes is skipped, not an error.
            ("nested".to_string(), Arg::Map(Vec::new())),
        ]);

        let options = options_from_arg(&map);
        assert_eq!(options.get("cycles"), Some(&WidgetOption::Number(4.0)));
        assert_eq!(options.get("vertical"), Some(&WidgetOption::Bool(true)));
        assert_eq!(
            options.get("mode"),
            Some(&WidgetOption::String("polygon".to_string()))
        );
        assert!(!options.contains_key("nested"));

        // A non-object argument yields nothing rather than panicking.
        assert!(options_from_arg(&Arg::Null).is_empty());
    }

    #[test]
    fn a_hydra_chain_compiles_bound_to_the_buffer_its_key_names() {
        // `prev()` reads the buffer the chain draws into, so it is where the
        // binding shows in the shader.
        let prev = crate::hydra::lookup("prev").expect("prev is in the table");
        for (key, index) in [("chain", 0), ("o0", 0), ("o1", 1), ("o2", 2), ("o3", 3)] {
            let chain = crate::hydra::Chain::source(prev, Vec::new());
            let options = options_from_arg(&Arg::Map(vec![(key.to_string(), Arg::Hydra(chain))]));
            let Some(WidgetOption::String(wgsl)) = options.get(key) else {
                panic!("{key}: {options:?}");
            };
            assert!(wgsl.contains(&format!("h_src(st, {index}.0)")), "{key}");
        }
    }

    #[test]
    fn an_evaluation_does_not_see_the_previous_one_s_options() {
        // The id is the call's source range, so the two calls are the same
        // length; the second evaluates to no option (`null` is skipped) and
        // must not inherit the first one's.
        let options = |src: &str| {
            crate::eval_result(src)
                .expect("eval")
                .meta
                .widgets
                .into_iter()
                .find(|w| w.widget_type == "_pianoroll")
                .expect("a pianoroll")
                .options
        };
        assert_eq!(
            options(r#"s("bd").pianoroll({cycles: 2 * 2})"#).get("cycles"),
            Some(&WidgetOption::Number(4.0))
        );
        assert_eq!(
            options(r#"s("bd").pianoroll({cycles:  null})"#).get("cycles"),
            None
        );
    }

    #[test]
    fn recording_is_per_id_and_resettable() {
        // Same registry an evaluation uses, so take the same lock.
        let _guard = crate::EVAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_options();
        record_options(
            "w-test-1",
            [("cycles".to_string(), WidgetOption::Number(8.0))]
                .into_iter()
                .collect(),
        );
        assert_eq!(
            recorded_options("w-test-1").and_then(|o| o.get("cycles").cloned()),
            Some(WidgetOption::Number(8.0))
        );
        assert!(recorded_options("w-test-missing").is_none());
        // An empty map records nothing, so it cannot mask a scanned option.
        record_options("w-test-2", OptionMap::new());
        assert!(recorded_options("w-test-2").is_none());
    }
}
