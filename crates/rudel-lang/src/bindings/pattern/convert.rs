use super::KPattern;
use koto::prelude::*;
use rudel_core::{Frac, Pattern, Value, ValueMap};
use std::sync::{Arc, Mutex};

/// Wrap a Koto callable as a rudel [`Value::Func`], so a script's own function
/// can travel *inside* a pattern and be called when that pattern is queried.
///
/// This is the one place the Koto VM is reachable from the query path. It is
/// possible at all because rudel builds Koto with its `arc` feature: values are
/// `Arc`-backed and the VM is `Send`, so a VM behind a mutex satisfies the
/// `Send + Sync` bound on a pattern's query closure (see
/// `tests/send_sync.rs`). Everywhere else callbacks are still applied eagerly
/// at construction, which is cheaper and keeps errors attached to the
/// evaluation that caused them; this path exists for `apply(patternOfFunctions)`,
/// where *which* function to call is not known until the pattern is queried.
///
/// A pattern argument arrives as `Value::Pat` and comes back the same way, so
/// the wrapped function reads as a pattern transform; anything else is passed
/// through as an ordinary value. A call that fails yields `Value::Null` rather
/// than unwinding through the scheduler.
pub(super) fn koto_fn_to_value(func: KValue, vm: &KotoVm) -> Value {
    let vm = Mutex::new(vm.spawn_shared_vm());
    let vm = Arc::new(vm);
    Value::func(move |arg| {
        let koto_arg = match arg {
            Value::Pat(pat) => KPattern(*pat).into(),
            other => value_to_koto(other),
        };
        let Ok(mut vm) = vm.lock() else {
            return Value::Null;
        };
        match vm.call_function(func.clone(), CallArgs::Single(koto_arg)) {
            Ok(KValue::Object(o)) if o.is_a::<KPattern>() => {
                Value::Pat(Box::new(o.cast::<KPattern>().unwrap().0.clone()))
            }
            Ok(other) => koto_to_value(&other),
            Err(_) => Value::Null,
        }
    })
}

/// Convert a Koto argument into a pattern: numbers and strings become `pure`
/// values, and patterns pass through.
///
/// A bare string is *not* mini-notation. Upstream only parses a string as mini
/// when the transpiler wrapped it — which it does for double quotes and
/// backticks, never for single quotes — and `reify` leaves plain strings alone
/// because `setStringParser` is only installed by `miniAllStrings()`, which
/// nothing calls. By the time an argument reaches here the preprocessor has
/// already applied the same rule, so a double-quoted literal arrives as an
/// `m(...)` pattern and a single-quoted one as this `KValue::Str`.
///
/// Parsing it anyway split every literal on whitespace:
/// `cat('C3 dorian', 'Bb2 major')` became a sequence of four words, so
/// `.scale(...)` was handed `"C3"` and `"dorian"` as scale names on alternating
/// cycles and quietly produced notes belonging to no scale at all.
pub(crate) fn arg_to_pattern(value: &KValue) -> Pattern {
    match value {
        KValue::Number(n) => {
            if n.is_i64() {
                rudel_core::pure(Value::Int(n.into()))
            } else {
                rudel_core::pure(Value::F64(n.into()))
            }
        }
        KValue::Bool(b) => rudel_core::pure(Value::Bool(*b)),
        KValue::Str(s) => rudel_core::pure(Value::Str(s.to_string())),
        KValue::Object(o) if o.is_a::<KPattern>() => o.cast::<KPattern>().unwrap().0.clone(),
        // A list is a sequence, as Strudel's `reify` makes it: `seq([a, b])`
        // and `stack([a, b])` both lay `a` and `b` out across one cycle.
        KValue::List(l) => {
            rudel_core::fastcat(&l.data().iter().map(arg_to_pattern).collect::<Vec<_>>())
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::pattern::engine::KFrac;

    fn num(n: i64) -> KValue {
        KValue::Number(KNumber::from(n))
    }

    fn list(items: Vec<KValue>) -> KValue {
        KValue::List(KList::with_data(items.into()))
    }

    fn tuple(items: Vec<KValue>) -> KValue {
        KValue::Tuple(KTuple::from(items))
    }

    /// A Koto object that is *not* a `KPattern` — `Fraction(1)`, as a script
    /// would write it. Every `is_a::<KPattern>()` guard has to reject it, or
    /// the `cast(...).unwrap()` behind the guard panics.
    fn not_a_pattern() -> KValue {
        KFrac(Frac::one()).into()
    }

    fn first_value(pat: &Pattern) -> Option<Value> {
        pat.query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .next()
            .map(|h| h.value)
    }

    fn pat_arg(n: i64) -> KValue {
        KPattern(rudel_core::pure(Value::Int(n))).into()
    }

    #[test]
    fn a_foreign_object_is_never_cast_to_a_pattern() {
        assert!(first_value(&arg_to_pattern(&not_a_pattern())).is_none());
        assert_eq!(arg_to_raw_str(&not_a_pattern()), None);
        assert_eq!(koto_to_value(&not_a_pattern()), Value::Null);
        assert_eq!(arg_to_value(&not_a_pattern()), Value::Null);
    }

    #[test]
    fn ratio_strings_divide() {
        assert_eq!(arg_to_f64(&num(3)), 3.0);
        assert_eq!(arg_to_f64(&KValue::Str("1/2".into())), 0.5);
        assert_eq!(arg_to_f64(&KValue::Str("1.5".into())), 1.5);
        // A zero denominator is refused rather than yielding an infinity.
        assert_eq!(arg_to_f64(&KValue::Str("1/0".into())), 0.0);
        assert_eq!(arg_to_f64(&KValue::Null), 0.0);
    }

    #[test]
    fn literals_convert_without_becoming_patterns() {
        assert_eq!(arg_to_value(&KValue::Bool(true)), Value::Bool(true));
        assert_eq!(arg_to_value(&num(2)), Value::Int(2));
        assert_eq!(koto_to_value(&KValue::Bool(false)), Value::Bool(false));
        // Lists and tuples both recurse; a map keeps its keys.
        let want = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(koto_to_value(&list(vec![num(1), num(2)])), want);
        assert_eq!(koto_to_value(&tuple(vec![num(1), num(2)])), want);
        let m = KMap::new();
        m.insert("n", num(4));
        let Value::Map(got) = koto_to_value(&KValue::Map(m)) else {
            panic!("expected a map");
        };
        assert_eq!(got.get("n"), Some(&Value::Int(4)));
    }

    #[test]
    fn pairs_are_accepted_as_lists_or_tuples() {
        // `stepcat([3, pat])`: an explicit weight, either spelling.
        for pair in [
            list(vec![num(3), pat_arg(7)]),
            tuple(vec![num(3), pat_arg(7)]),
        ] {
            let (weight, pat) = arg_to_weighted_pair(&pair);
            assert_eq!(weight, Frac::int(3));
            assert_eq!(first_value(&pat), Some(Value::Int(7)));
        }
        // `wchoose([pat, 2])`: the weight is second here.
        for pair in [
            list(vec![pat_arg(7), num(2)]),
            tuple(vec![pat_arg(7), num(2)]),
        ] {
            let (pat, weight) = arg_to_pattern_weight(&pair);
            assert_eq!(weight, 2.0);
            assert_eq!(first_value(&pat), Some(Value::Int(7)));
        }
        // Anything that is not a two-element pair keeps the default weight
        // instead of indexing past the end.
        for other in [
            list(vec![num(1)]),
            tuple(vec![num(1)]),
            list(vec![num(1), num(2), num(3)]),
            pat_arg(7),
        ] {
            assert_eq!(arg_to_pattern_weight(&other).1, 1.0);
        }
        // `stepalt` groups: a list or tuple is many patterns, anything else one.
        assert_eq!(arg_to_group(&list(vec![num(1), num(2)])).len(), 2);
        assert_eq!(arg_to_group(&tuple(vec![num(1), num(2)])).len(), 2);
        assert_eq!(arg_to_group(&num(1)).len(), 1);
    }
}
