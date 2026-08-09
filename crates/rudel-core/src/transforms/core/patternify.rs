use crate::{fraction::Frac, pattern::Pattern, value::Value};
use std::sync::Arc;

/// Patternify a single `Frac`-valued argument, applying raw op `f(pat, frac)`.
/// Fast-paths pure arguments (preserving steps), matching Strudel's `register`.
pub(super) fn patternify_frac<F>(pat: &Pattern, arg: Pattern, f: F) -> Pattern
where
    F: Fn(&Pattern, Frac) -> Pattern + Send + Sync + 'static,
{
    if let Some(v) = &arg.pure_value {
        let result = f(pat, v.to_frac());
        // Strudel's register keeps the bypassed pure argument's source
        // location by appending it to every hap's context.
        if let Some((start, end)) = arg.pure_loc {
            return result.with_context(move |context| {
                let mut context = context.clone();
                context.locations.push((start, end));
                context
            });
        }
        return result;
    }
    let pat = pat.clone();
    let f = Arc::new(f);
    arg.fmap(move |v| Value::Pat(Box::new(f(&pat, v.to_frac()))))
        .inner_join()
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
    if let Some(v) = &arg.pure_value {
        let result = f(pat, v.as_f64().unwrap_or(0.0));
        if let Some((start, end)) = arg.pure_loc {
            return result.with_context(move |context| {
                let mut context = context.clone();
                context.locations.push((start, end));
                context
            });
        }
        return result;
    }
    let pat = pat.clone();
    let f = Arc::new(f);
    arg.fmap(move |v| Value::Pat(Box::new(f(&pat, v.as_f64().unwrap_or(0.0)))))
        .inner_join()
}
