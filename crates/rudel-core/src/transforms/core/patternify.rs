use crate::{fraction::Frac, hap::Context, pattern::Pattern, value::Value};
use std::sync::Arc;

/// Strudel's `register` keeps a bypassed pure argument's source location by
/// appending it to every hap's context.
pub(crate) fn push_loc(result: Pattern, loc: Option<(usize, usize)>) -> Pattern {
    let Some((start, end)) = loc else {
        return result;
    };
    result.with_context(move |context: &Context| {
        let mut context = context.clone();
        context.locations.push((start, end));
        context
    })
}

/// Patternify a single argument, applying raw op `f(pat, value)`. Pure
/// arguments bypass (preserving steps and their source location), patterned
/// ones map to the per-value result and `innerJoin` — Strudel's `register`.
pub(crate) fn patternify_value<F>(pat: &Pattern, arg: Pattern, f: F) -> Pattern
where
    F: Fn(&Pattern, &Value) -> Pattern + Send + Sync + 'static,
{
    if let Some(v) = &arg.pure_value {
        return push_loc(f(pat, v), arg.pure_loc);
    }
    let pat = pat.clone();
    let f = Arc::new(f);
    arg.fmap(move |v| Value::Pat(Box::new(f(&pat, &v))))
        .inner_join()
}

/// Patternify two value arguments the way Strudel's `register` does for an
/// arity-3 transform: `a` is the structural outer (`fmap`), `b` is sampled by
/// `appLeft`, then `innerJoin`. Both-pure bypasses to a direct call.
pub(crate) fn patternify_value2<F>(pat: &Pattern, a: Pattern, b: Pattern, f: F) -> Pattern
where
    F: Fn(&Pattern, &Value, &Value) -> Pattern + Send + Sync + 'static,
{
    if let (Some(av), Some(bv)) = (&a.pure_value, &b.pure_value) {
        let loc = a.pure_loc.or(b.pure_loc);
        return push_loc(f(pat, av, bv), loc);
    }
    let pat = pat.clone();
    let f = Arc::new(f);
    a.fmap(move |av| {
        let pat = pat.clone();
        let f = f.clone();
        Value::func(move |bv| Value::Pat(Box::new(f(&pat, &av, &bv))))
    })
    .app_left(&b)
    .inner_join()
}

/// Patternify a single `Frac`-valued argument, applying raw op `f(pat, frac)`.
pub(super) fn patternify_frac<F>(pat: &Pattern, arg: Pattern, f: F) -> Pattern
where
    F: Fn(&Pattern, Frac) -> Pattern + Send + Sync + 'static,
{
    patternify_value(pat, arg, move |pat, v| f(pat, v.to_frac()))
}

/// Patternify a single `f64`-valued argument, applying raw op `f(pat, x)`.
///
/// The `Frac` variant above would round the argument onto a bounded rational
/// first, which is right for a *time* but not for a probability sampled from a
/// continuous signal.
pub(super) fn patternify_f64<F>(pat: &Pattern, arg: Pattern, f: F) -> Pattern
where
    F: Fn(&Pattern, f64) -> Pattern + Send + Sync + 'static,
{
    patternify_value(pat, arg, move |pat, v| f(pat, v.as_f64().unwrap_or(0.0)))
}
