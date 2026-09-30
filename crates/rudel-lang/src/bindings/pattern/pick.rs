use super::convert::arg_to_pattern;
use crate::js::Arg;
use rudel_core::{Pattern, PickJoin};
use std::collections::HashMap;

pub(super) enum PatternLookup {
    List(Vec<Pattern>),
    Map(HashMap<String, Pattern>),
}

pub(super) fn lookup_from_arg(value: &Arg) -> Option<PatternLookup> {
    match value {
        Arg::List(l) => Some(PatternLookup::List(l.iter().map(arg_to_pattern).collect())),
        Arg::Map(m) => Some(PatternLookup::Map(
            m.iter()
                .map(|(k, v)| (k.clone(), arg_to_pattern(v)))
                .collect(),
        )),
        _ => None,
    }
}

pub(super) fn is_lookup(value: &Arg) -> bool {
    matches!(value, Arg::List(_) | Arg::Map(_))
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

pub(in crate::bindings) fn pick_args(args: &[Arg], modulo: bool, join: PickJoin) -> Pattern {
    let (Some(first), Some(second)) = (args.first(), args.get(1)) else {
        return rudel_core::silence();
    };
    let (lookup_value, selector_value) = if is_lookup(second) && !is_lookup(first) {
        (second, first)
    } else {
        (first, second)
    };
    let Some(lookup) = lookup_from_arg(lookup_value) else {
        return rudel_core::silence();
    };
    pick_from_lookup(lookup, arg_to_pattern(selector_value), modulo, join)
}
