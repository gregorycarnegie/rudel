// widgets.rs - evaluated inline-widget options.
// The source scan in `preprocess/widgets.rs` can only read option *literals*
// (`.pianoroll({cycles: 4})`), because it runs before the script does. The
// rewrite passes the original argument through to the widget method, though, so
// by the time that method runs Koto has evaluated the map — `{cycles: n}` or
// `{cycles: bars * 2}` included. This registry carries those evaluated options
// back out of the run so the host can merge them over the scanned ones.
//
// Same shape as the slider registry next door: a process-global map keyed by
// widget id, cleared at the start of each evaluation.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::WidgetOption;
use koto::prelude::*;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{LazyLock, RwLock},
};

type OptionMap = BTreeMap<String, WidgetOption>;

static WIDGET_OPTIONS: LazyLock<RwLock<HashMap<String, OptionMap>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Convert one evaluated Koto value into a widget option. Mirrors the literal
/// forms the source scan accepts, so a computed value and a written-out one
/// land as the same `WidgetOption`.
fn option_from_koto(value: &KValue) -> Option<WidgetOption> {
    match value {
        KValue::Bool(b) => Some(WidgetOption::Bool(*b)),
        KValue::Number(n) => Some(WidgetOption::Number(f64::from(n))),
        KValue::Str(s) => Some(WidgetOption::String(s.to_string())),
        // A hydra chain arrives as an object and leaves as the WGSL it
        // compiles to, so the shader is generated once per evaluation rather
        // than once per frame, and the widget host needs to know nothing about
        // hydra beyond "this option is a shader".
        KValue::Object(o) => o
            .cast::<crate::bindings::hydra::KHydra>()
            .ok()
            .map(|h| WidgetOption::String(crate::hydra::compile(&h.0))),
        _ => None,
    }
}

/// Read an evaluated `{key: value}` widget-option map. Non-string keys and
/// values that are not a bool/number/string (a nested map, a function) are
/// skipped rather than failing the evaluation — the scanned literal, or the
/// painter's default, still stands for them.
pub(crate) fn options_from_koto(value: &KValue) -> OptionMap {
    let KValue::Map(map) = value else {
        return OptionMap::new();
    };
    map.data()
        .iter()
        .filter_map(|(k, v)| match k.value() {
            KValue::Str(key) => Some((key.to_string(), option_from_koto(v)?)),
            _ => None,
        })
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
    fn koto_values_map_onto_the_literal_option_forms() {
        let map = KMap::new();
        map.insert("cycles", 4);
        map.insert("vertical", true);
        map.insert("mode", "polygon");
        // A value shape no option takes is skipped, not an error.
        map.insert("nested", KMap::new());

        let options = options_from_koto(&KValue::Map(map));
        assert_eq!(options.get("cycles"), Some(&WidgetOption::Number(4.0)));
        assert_eq!(options.get("vertical"), Some(&WidgetOption::Bool(true)));
        assert_eq!(
            options.get("mode"),
            Some(&WidgetOption::String("polygon".to_string()))
        );
        assert!(!options.contains_key("nested"));

        // A non-map argument yields nothing rather than panicking.
        assert!(options_from_koto(&KValue::Null).is_empty());
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
