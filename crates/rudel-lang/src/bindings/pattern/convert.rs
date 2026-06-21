use super::KPattern;
use koto::prelude::*;
use rudel_core::{Frac, Pattern, Value, ValueMap};

/// Convert a Koto argument into a pattern: numbers become `pure` values,
/// strings parse as mini-notation, and patterns pass through.
pub(in crate::bindings) fn arg_to_pattern(value: &KValue) -> Pattern {
    match value {
        KValue::Number(n) => {
            if n.is_i64() {
                rudel_core::pure(Value::Int(n.into()))
            } else {
                rudel_core::pure(Value::F64(n.into()))
            }
        }
        KValue::Bool(b) => rudel_core::pure(Value::Bool(*b)),
        KValue::Str(s) => rudel_mini::parse(s).unwrap_or_else(|_| rudel_core::silence()),
        KValue::Object(o) if o.is_a::<KPattern>() => o.cast::<KPattern>().unwrap().0.clone(),
        _ => rudel_core::silence(),
    }
}

/// Recover a raw string argument: a plain string, or the original source text
/// of an `m("...", offset)`-wrapped mini literal. The preprocessor wraps every
/// string literal for source-location tracking, so functions that want the
/// literal text (sample names, scale/chord names, device hints, ratios) must
/// read through the wrapper.
pub(crate) fn arg_to_raw_str(value: &KValue) -> Option<String> {
    match value {
        KValue::Str(s) => Some(s.to_string()),
        KValue::Object(o) if o.is_a::<KPattern>() => o
            .cast::<KPattern>()
            .unwrap()
            .0
            .source
            .as_deref()
            .map(|s| s.to_string()),
        _ => None,
    }
}

pub(crate) fn arg_to_f64(value: &KValue) -> f64 {
    if let KValue::Number(n) = value {
        return n.into();
    }
    // Allow `"1/3"` style ratios in string (or wrapped-string) arguments.
    match arg_to_raw_str(value) {
        Some(s) => match s.split_once('/') {
            Some((a, b)) => {
                let (a, b) = (a.trim().parse::<f64>(), b.trim().parse::<f64>());
                match (a, b) {
                    (Ok(a), Ok(b)) if b != 0.0 => a / b,
                    _ => 0.0,
                }
            }
            None => s.parse().unwrap_or(0.0),
        },
        None => 0.0,
    }
}

pub(super) fn arg_to_frac(value: &KValue) -> Frac {
    Frac::from_f64(arg_to_f64(value))
}

/// Interpret an argument as a `(weight, pattern)` pair for `stepcat`/`arrange`.
/// A two-element list/tuple `[weight, pat]` sets the weight explicitly;
/// otherwise the pattern's own step count is used (defaulting to `1`).
pub(in crate::bindings) fn arg_to_weighted_pair(value: &KValue) -> (Frac, Pattern) {
    let explicit = match value {
        KValue::List(l) => {
            let d = l.data();
            (d.len() == 2).then(|| (arg_to_frac(&d[0]), arg_to_pattern(&d[1])))
        }
        KValue::Tuple(t) => {
            let d = t.data();
            (d.len() == 2).then(|| (arg_to_frac(&d[0]), arg_to_pattern(&d[1])))
        }
        _ => None,
    };
    explicit.unwrap_or_else(|| {
        let pat = arg_to_pattern(value);
        let weight = pat.steps.unwrap_or_else(Frac::one);
        (weight, pat)
    })
}

/// Interpret an argument as a `[pattern, weight]` pair for the weighted
/// choosers (`wchoose`/`wrandcat`). A bare pattern defaults to weight `1`.
pub(in crate::bindings) fn arg_to_pattern_weight(value: &KValue) -> (Pattern, f64) {
    let pair = |slice: &[KValue]| (arg_to_pattern(&slice[0]), arg_to_f64(&slice[1]));
    match value {
        KValue::List(l) if l.data().len() == 2 => pair(&l.data()),
        KValue::Tuple(t) if t.data().len() == 2 => pair(t.data()),
        _ => (arg_to_pattern(value), 1.0),
    }
}

/// Interpret an argument as a group of patterns for `stepalt`. A list/tuple
/// becomes a multi-element group; anything else is a single-element group.
pub(in crate::bindings) fn arg_to_group(value: &KValue) -> Vec<Pattern> {
    match value {
        KValue::List(l) => l.data().iter().map(arg_to_pattern).collect(),
        KValue::Tuple(t) => t.data().iter().map(arg_to_pattern).collect(),
        _ => vec![arg_to_pattern(value)],
    }
}

pub(crate) fn arg0(ctx: &mut CallContext) -> KValue {
    ctx.args().first().cloned().unwrap_or(KValue::Null)
}

/// Convert a Koto value into a literal rudel [`Value`], recursing into
/// lists/tuples. Used by list-valued controls like `partials`/`phases`.
pub(in crate::bindings) fn koto_to_value(value: &KValue) -> Value {
    match value {
        KValue::Number(n) => {
            if n.is_i64() {
                Value::Int(n.into())
            } else {
                Value::F64(n.into())
            }
        }
        KValue::Bool(b) => Value::Bool(*b),
        KValue::Str(s) => Value::Str(s.to_string()),
        KValue::Object(o) if o.is_a::<KPattern>() => {
            // A wrapped string literal contributes its raw text as a literal.
            match o.cast::<KPattern>().unwrap().0.source.as_deref() {
                Some(s) => Value::Str(s.to_string()),
                None => Value::Null,
            }
        }
        KValue::List(l) => Value::List(l.data().iter().map(koto_to_value).collect()),
        KValue::Tuple(t) => Value::List(t.data().iter().map(koto_to_value).collect()),
        KValue::Map(m) => {
            // Preserve the Koto map's insertion order (it mirrors JS object key
            // order, which Strudel-faithful behaviour like `modulate` relies on).
            let mut out = ValueMap::new();
            for (k, v) in m.data().iter() {
                if let KValue::Str(key) = k.value() {
                    out.insert(key.to_string(), koto_to_value(v));
                }
            }
            Value::Map(out)
        }
        _ => Value::Null,
    }
}

pub(super) fn value_to_koto(value: Value) -> KValue {
    match value {
        Value::Null => KValue::Null,
        Value::Bool(b) => KValue::Bool(b),
        Value::Int(n) => KValue::Number(KNumber::from(n)),
        Value::F64(n) => KValue::Number(KNumber::from(n)),
        Value::Frac(f) => KValue::Number(KNumber::from(f.to_f64())),
        Value::Str(s) => KValue::Str(s.into()),
        Value::List(items) => {
            KList::with_data(items.into_iter().map(value_to_koto).collect()).into()
        }
        Value::Map(items) => {
            let map = KMap::new();
            for (key, value) in items {
                map.insert(key.as_str(), value_to_koto(value));
            }
            map.into()
        }
        Value::Func(_) => KValue::Null,
        Value::Pat(p) => KPattern(*p).into(),
    }
}

/// Convert a Koto value into a literal rudel [`Value`] (no mini-notation
/// parsing — used by `pure`).
pub(in crate::bindings) fn arg_to_value(value: &KValue) -> Value {
    match value {
        KValue::Number(n) => {
            if n.is_i64() {
                Value::Int(n.into())
            } else {
                Value::F64(n.into())
            }
        }
        KValue::Bool(b) => Value::Bool(*b),
        KValue::Str(s) => Value::Str(s.to_string()),
        KValue::Object(o) if o.is_a::<KPattern>() => {
            let pat = o.cast::<KPattern>().unwrap().0.clone();
            // A wrapped string literal (`m("x", n)`) is a literal value here,
            // not a pattern — `pure("x")` should hold the string, not its haps.
            match pat.source.as_deref() {
                Some(s) => Value::Str(s.to_string()),
                None => Value::Pat(Box::new(pat)),
            }
        }
        _ => Value::Null,
    }
}
