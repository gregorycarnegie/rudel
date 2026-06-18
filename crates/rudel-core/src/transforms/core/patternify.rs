use crate::fraction::Frac;
use crate::pattern::Pattern;
use crate::value::Value;
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
