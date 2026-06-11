use super::convert::arg_to_pattern;
use koto::prelude::*;
use rudel_core::{Pattern, PickJoin};
use std::collections::HashMap;

pub(super) enum PatternLookup {
    List(Vec<Pattern>),
    Map(HashMap<String, Pattern>),
}

pub(super) fn lookup_from_koto(value: &KValue) -> Option<PatternLookup> {
    match value {
        KValue::List(l) => Some(PatternLookup::List(
            l.data().iter().map(arg_to_pattern).collect(),
        )),
        KValue::Tuple(t) => Some(PatternLookup::List(
            t.data().iter().map(arg_to_pattern).collect(),
        )),
        KValue::Map(m) => {
            let mut out = HashMap::new();
            for (k, v) in m.data().iter() {
                if let KValue::Str(key) = k.value() {
                    out.insert(key.to_string(), arg_to_pattern(v));
                }
            }
            Some(PatternLookup::Map(out))
        }
        _ => None,
    }
}

pub(super) fn is_lookup(value: &KValue) -> bool {
    matches!(value, KValue::List(_) | KValue::Tuple(_) | KValue::Map(_))
}

pub(super) fn pick_from_lookup(
    lookup: PatternLookup,
    selector: Pattern,
    modulo: bool,
    join: PickJoin,
) -> Pattern {
    match lookup {
        PatternLookup::List(items) => rudel_core::pick_list(&items, &selector, modulo, join),
        PatternLookup::Map(items) => rudel_core::pick_map(&items, &selector, join),
    }
}

pub(in crate::bindings) fn pick_args(args: &[KValue], modulo: bool, join: PickJoin) -> Pattern {
    let Some(first) = args.first() else {
        return rudel_core::silence();
    };
    let Some(second) = args.get(1) else {
        return rudel_core::silence();
    };
    let (lookup_value, selector_value) = if is_lookup(second) && !is_lookup(first) {
        (second, first)
    } else {
        (first, second)
    };
    let Some(lookup) = lookup_from_koto(lookup_value) else {
        return rudel_core::silence();
    };
    pick_from_lookup(lookup, arg_to_pattern(selector_value), modulo, join)
}
