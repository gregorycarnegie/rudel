use crate::js::{self, Arg, NULL};
use rudel_core::{Frac, Pattern, Value, ValueMap};

/// Wrap a script function as a rudel [`Value::Func`], so it can travel *inside*
/// a pattern and be called when that pattern is queried.
///
/// This is one of the two places a script runs on the query path (the other is
/// `engine::pattern_from_fn`). Everywhere else callbacks are applied eagerly at
/// construction, which is cheaper and keeps errors attached to the evaluation
/// that caused them; this path exists for `apply(patternOfFunctions)`, where
/// *which* function to call is not known until the pattern is queried. The
/// engine lives on one thread, so the call is handed to it — see `js::SendFn`.
///
/// A pattern argument arrives as `Value::Pat` and comes back the same way, so
/// the wrapped function reads as a pattern transform; anything else is passed
/// through as an ordinary value. A call that fails yields `Value::Null` rather
/// than unwinding through the scheduler.
pub(crate) fn fn_to_value(func: &Arg) -> Value {
    let Some(func) = js::keep(func) else {
        return Value::Null;
    };
    Value::func(move |arg| {
        func.run(move |f| match js::call(f, vec![value_to_arg(arg)]) {
            Ok(Arg::Pat(pat)) => Value::Pat(Box::new(pat)),
            Ok(other) => to_value(&other),
            Err(_) => Value::Null,
        })
        .unwrap_or(Value::Null)
    })
}

/// A script number as a rudel value: an integer where it is one, as Strudel
/// snapshots `n("0 1 2")`. JavaScript has the one number type, so `2` and
/// `2.0` are the same value here too.
pub(super) fn number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9e15 {
        Value::Int(n as i64)
    } else {
        Value::F64(n)
    }
}

/// Convert an argument into a pattern: numbers and strings become `pure`
/// values, and patterns pass through.
///
/// A bare string is *not* mini-notation. Upstream only parses a string as mini
/// when the transpiler wrapped it — which it does for double quotes and
/// backticks, never for single quotes — and `reify` leaves plain strings alone
/// because `setStringParser` is only installed by `miniAllStrings()`, which
/// nothing calls. By the time an argument reaches here the preprocessor has
/// already applied the same rule, so a double-quoted literal arrives as an
/// `m(...)` pattern and a single-quoted one as this `Arg::Str`.
///
/// Parsing it anyway split every literal on whitespace:
/// `cat('C3 dorian', 'Bb2 major')` became a sequence of four words, so
/// `.scale(...)` was handed `"C3"` and `"dorian"` as scale names on alternating
/// cycles and quietly produced notes belonging to no scale at all.
pub(crate) fn arg_to_pattern(value: &Arg) -> Pattern {
    match value {
        Arg::Num(n) => rudel_core::pure(number(*n)),
        Arg::Bool(b) => rudel_core::pure(Value::Bool(*b)),
        Arg::Str(s) => rudel_core::pure(Value::Str(s.clone())),
        Arg::Pat(p) => p.clone(),
        Arg::Frac(f) => rudel_core::pure(Value::Frac(*f)),
        // A function becomes a pattern *of* that function, as `reify` makes it,
        // which is how `choose(x => …, x => …)` builds a pattern of transforms.
        Arg::Func(_) => rudel_core::pure(fn_to_value(value)),
        // A list is a sequence, as Strudel's `reify` makes it: `seq([a, b])`
        // and `stack([a, b])` both lay `a` and `b` out across one cycle.
        Arg::List(l) => rudel_core::fastcat(&l.iter().map(arg_to_pattern).collect::<Vec<_>>()),
        _ => rudel_core::silence(),
    }
}

/// Recover a raw string argument: a plain string, or the original source text
/// of an `m("...", offset)`-wrapped mini literal. The preprocessor wraps every
/// double-quoted literal for source-location tracking, so functions that want
/// the literal text (sample names, scale/chord names, device hints, ratios)
/// must read through the wrapper.
pub(crate) fn arg_to_raw_str(value: &Arg) -> Option<String> {
    match value {
        Arg::Str(s) => Some(s.clone()),
        Arg::Pat(p) => p.source.as_deref().cloned(),
        _ => None,
    }
}

pub(crate) fn arg_to_f64(value: &Arg) -> f64 {
    match value {
        Arg::Num(n) => return *n,
        Arg::Frac(f) => return f.to_f64(),
        _ => {}
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

pub(super) fn arg_to_frac(value: &Arg) -> Frac {
    match value {
        Arg::Frac(f) => *f,
        other => Frac::from_f64(arg_to_f64(other)),
    }
}

/// Interpret an argument as a `(weight, pattern)` pair for `stepcat`/`arrange`.
/// A two-element array `[weight, pat]` sets the weight explicitly; otherwise
/// the pattern's own step count is used (defaulting to `1`).
pub(in crate::bindings) fn arg_to_weighted_pair(value: &Arg) -> (Frac, Pattern) {
    match value {
        Arg::List(d) if d.len() == 2 => (arg_to_frac(&d[0]), arg_to_pattern(&d[1])),
        _ => {
            let pat = arg_to_pattern(value);
            let weight = pat.steps.unwrap_or_else(Frac::one);
            (weight, pat)
        }
    }
}

/// Interpret an argument as a `[pattern, weight]` pair for the weighted
/// choosers (`wchoose`/`wrandcat`). A bare pattern defaults to weight `1`.
pub(in crate::bindings) fn arg_to_pattern_weight(value: &Arg) -> (Pattern, f64) {
    match value {
        Arg::List(l) if l.len() == 2 => (arg_to_pattern(&l[0]), arg_to_f64(&l[1])),
        _ => (arg_to_pattern(value), 1.0),
    }
}

/// Interpret an argument as a group of patterns for `stepalt`. An array becomes
/// a multi-element group; anything else is a single-element group.
pub(in crate::bindings) fn arg_to_group(value: &Arg) -> Vec<Pattern> {
    match value {
        Arg::List(l) => l.iter().map(arg_to_pattern).collect(),
        _ => vec![arg_to_pattern(value)],
    }
}

pub(crate) fn arg0(args: &[Arg]) -> &Arg {
    args.first().unwrap_or(NULL)
}

/// Convert a script value into a literal rudel [`Value`], recursing into
/// arrays and objects. Used by list-valued controls like `partials`/`phases`,
/// and for what a callback returns.
pub(in crate::bindings) fn to_value(value: &Arg) -> Value {
    match value {
        Arg::Num(n) => number(*n),
        Arg::Bool(b) => Value::Bool(*b),
        Arg::Str(s) => Value::Str(s.clone()),
        Arg::Frac(f) => Value::Frac(*f),
        // A wrapped string literal contributes its raw text as a literal — the
        // mini pass put the wrapper there, and a callback returning `"c3"`
        // means the note name. A pattern built any other way is a pattern
        // *value*, which one of the joins then flattens; that is how a
        // `register`ed helper written as `fmap(v => …).squeezeJoin()` gets its
        // inner pattern back out.
        Arg::Pat(pattern) => match pattern.source.as_deref() {
            Some(s) => Value::Str(s.clone()),
            None => Value::Pat(Box::new(pattern.clone())),
        },
        Arg::List(l) => Value::List(l.iter().map(to_value).collect()),
        // Key order is the script's, which Strudel-faithful behaviour like
        // `modulate` relies on.
        Arg::Map(m) => {
            let mut out = ValueMap::new();
            for (k, v) in m {
                out.insert(k.clone(), to_value(v));
            }
            Value::Map(out)
        }
        _ => Value::Null,
    }
}

pub(super) fn value_to_arg(value: Value) -> Arg {
    match value {
        Value::Null => Arg::Null,
        Value::Bool(b) => Arg::Bool(b),
        Value::Int(n) => Arg::Num(n as f64),
        Value::F64(n) => Arg::Num(n),
        Value::Frac(f) => Arg::Num(f.to_f64()),
        Value::Str(s) => Arg::Str(s),
        Value::List(items) => Arg::List(items.into_iter().map(value_to_arg).collect()),
        Value::Map(items) => Arg::Map(
            items
                .into_iter()
                .map(|(key, value)| (key, value_to_arg(value)))
                .collect(),
        ),
        Value::Func(_) => Arg::Null,
        Value::Pat(p) => Arg::Pat(*p),
    }
}

/// Convert a script value into a literal rudel [`Value`] (no mini-notation
/// parsing — used by `pure`).
pub(in crate::bindings) fn arg_to_value(value: &Arg) -> Value {
    match value {
        Arg::Num(n) => number(*n),
        Arg::Bool(b) => Value::Bool(*b),
        Arg::Str(s) => Value::Str(s.clone()),
        // A wrapped string literal (`m("x", n)`) is a literal value here, not
        // a pattern — `pure("x")` should hold the string, not its haps.
        Arg::Pat(pat) => match pat.source.as_deref() {
            Some(s) => Value::Str(s.clone()),
            None => Value::Pat(Box::new(pat.clone())),
        },
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(n: i64) -> Arg {
        Arg::Num(n as f64)
    }

    /// One of rudel's objects that is *not* a pattern — `Hydra.osc()`, as a
    /// script would write it. Nothing here may read it as one.
    fn not_a_pattern() -> Arg {
        Arg::Kabel(0)
    }

    fn first_value(pat: &Pattern) -> Option<Value> {
        pat.query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .next()
            .map(|h| h.value)
    }

    fn pat_arg(n: i64) -> Arg {
        Arg::Pat(rudel_core::pure(Value::Int(n)))
    }

    #[test]
    fn a_foreign_object_is_never_read_as_a_pattern() {
        assert!(first_value(&arg_to_pattern(&not_a_pattern())).is_none());
        assert_eq!(arg_to_raw_str(&not_a_pattern()), None);
        assert_eq!(to_value(&not_a_pattern()), Value::Null);
        assert_eq!(arg_to_value(&not_a_pattern()), Value::Null);
    }

    #[test]
    fn ratio_strings_divide() {
        assert_eq!(arg_to_f64(&num(3)), 3.0);
        assert_eq!(arg_to_f64(&Arg::Str("1/2".into())), 0.5);
        assert_eq!(arg_to_f64(&Arg::Str("1.5".into())), 1.5);
        // A zero denominator is refused rather than yielding an infinity.
        assert_eq!(arg_to_f64(&Arg::Str("1/0".into())), 0.0);
        assert_eq!(arg_to_f64(&Arg::Null), 0.0);
    }

    #[test]
    fn a_whole_number_is_an_integer_and_a_fraction_is_not() {
        // JavaScript has one number type; the split is made here, so `n(2)`
        // snapshots as `2` rather than `2.0`.
        assert!(matches!(number(2.0), Value::Int(2)));
        assert!(matches!(number(-3.0), Value::Int(-3)));
        assert!(matches!(number(0.5), Value::F64(_)));
        assert!(matches!(number(f64::NAN), Value::F64(_)));
        assert!(matches!(number(f64::INFINITY), Value::F64(_)));
    }

    #[test]
    fn literals_convert_without_becoming_patterns() {
        assert_eq!(arg_to_value(&Arg::Bool(true)), Value::Bool(true));
        assert_eq!(arg_to_value(&num(2)), Value::Int(2));
        assert_eq!(to_value(&Arg::Bool(false)), Value::Bool(false));
        // Arrays recurse; an object keeps its keys.
        let want = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(to_value(&Arg::List(vec![num(1), num(2)])), want);
        let Value::Map(got) = to_value(&Arg::Map(vec![("n".to_string(), num(4))])) else {
            panic!("expected a map");
        };
        assert_eq!(got.get("n"), Some(&Value::Int(4)));
    }

    #[test]
    fn pairs_are_two_element_arrays() {
        // `stepcat([3, pat])`: an explicit weight.
        let (weight, pat) = arg_to_weighted_pair(&Arg::List(vec![num(3), pat_arg(7)]));
        assert_eq!(weight, Frac::int(3));
        assert_eq!(first_value(&pat), Some(Value::Int(7)));
        // `wchoose([pat, 2])`: the weight is second here.
        let (pat, weight) = arg_to_pattern_weight(&Arg::List(vec![pat_arg(7), num(2)]));
        assert_eq!(weight, 2.0);
        assert_eq!(first_value(&pat), Some(Value::Int(7)));
        // Anything that is not a two-element pair keeps the default weight
        // instead of indexing past the end.
        for other in [
            Arg::List(vec![num(1)]),
            Arg::List(vec![num(1), num(2), num(3)]),
            pat_arg(7),
        ] {
            assert_eq!(arg_to_pattern_weight(&other).1, 1.0);
        }
        // `stepalt` groups: an array is many patterns, anything else one.
        assert_eq!(arg_to_group(&Arg::List(vec![num(1), num(2)])).len(), 2);
        assert_eq!(arg_to_group(&num(1)).len(), 1);
    }
}
