use super::convert::arg_to_pattern;
use koto::prelude::*;
use rudel_core::{Pattern, Value};
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

fn is_lookup(value: &KValue) -> bool {
    matches!(value, KValue::List(_) | KValue::Tuple(_) | KValue::Map(_))
}

pub(super) fn pick_from_lookup(lookup: PatternLookup, selector: Pattern, modulo: bool) -> Pattern {
    match lookup {
        PatternLookup::List(items) => {
            if items.is_empty() {
                return rudel_core::silence();
            }
            selector
                .fmap(move |v| {
                    let raw = v.as_f64().unwrap_or(0.0).round() as i64;
                    let idx = if modulo {
                        raw.rem_euclid(items.len() as i64)
                    } else {
                        raw.clamp(0, items.len() as i64 - 1)
                    } as usize;
                    Value::Pat(Box::new(items[idx].clone()))
                })
                .inner_join()
        }
        PatternLookup::Map(items) => {
            if items.is_empty() {
                return rudel_core::silence();
            }
            selector
                .fmap(move |v| {
                    let key = match v {
                        Value::Str(s) => s,
                        Value::Int(n) => n.to_string(),
                        Value::F64(x) => {
                            let s = format!("{x:.0}");
                            s
                        }
                        _ => String::new(),
                    };
                    items
                        .get(&key)
                        .cloned()
                        .map(|p| Value::Pat(Box::new(p)))
                        .unwrap_or(Value::Null)
                })
                .filter_values(|v| !matches!(v, Value::Null))
                .inner_join()
        }
    }
}

pub(in crate::bindings) fn pick_args(args: &[KValue], modulo: bool) -> Pattern {
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
    pick_from_lookup(lookup, arg_to_pattern(selector_value), modulo)
}
