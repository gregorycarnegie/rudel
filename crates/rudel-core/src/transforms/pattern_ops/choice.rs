use crate::fraction::Frac;
use crate::pattern::{Pattern, silence, value_to_pattern};
use crate::signal::rand;
use crate::value::Value;
use std::sync::Arc;

/// Pick one of the patterns at random each cycle (`randcat`/`chooseCycles`).
pub fn randcat(pats: &[Pattern]) -> Pattern {
    if pats.is_empty() {
        return silence();
    }
    let pats: Vec<Pattern> = pats.to_vec();
    let len = pats.len();
    let chooser = rand().segment(1);
    let pats = Arc::new(pats);
    chooser
        .fmap(move |v| {
            let idx = ((v.as_f64().unwrap_or(0.0) * len as f64) as usize).min(len - 1);
            Value::Pat(Box::new(pats[idx].clone()))
        })
        .inner_join()
}

/// Shared core of `choose`/`chooseIn` (`__chooseWith`): scale the 0..1 chooser
/// into `0..len`, then map each draw to the pattern at the clamped index. The
/// result is a pattern-of-patterns, joined by the callers below.
fn choose_pats(chooser: Pattern, pats: &[Pattern]) -> Pattern {
    let pats = Arc::new(pats.to_vec());
    let len = pats.len();
    chooser.range(0.0, len as f64).fmap(move |v| {
        let key = (v.as_f64().unwrap_or(0.0).floor().max(0.0) as usize).min(len - 1);
        Value::Pat(Box::new(pats[key].clone()))
    })
}

/// `chooseWith`: choose from the list using an arbitrary 0..1 `chooser` pattern,
/// taking structure from the chooser (`outerJoin`). Used by `Pattern.choose`.
pub fn choose_with(chooser: Pattern, pats: &[Pattern]) -> Pattern {
    if pats.is_empty() {
        return silence();
    }
    choose_pats(chooser, pats).outer_join()
}

/// `choose`/`chooseOut`: continuously choose from the list, with the structure
/// coming from the random chooser (`outerJoin`).
pub fn choose(pats: &[Pattern]) -> Pattern {
    choose_with(rand(), pats)
}

/// `chooseIn`: like [`choose`], but the structure comes from the chosen values
/// (`innerJoin`).
pub fn choose_in(pats: &[Pattern]) -> Pattern {
    if pats.is_empty() {
        return silence();
    }
    choose_pats(rand(), pats).inner_join()
}

/// Shared core of the weighted choosers. `chooser` is a 0..1 signal; each pair
/// is `(pattern, weight)`. Returns a pattern-of-patterns ready to be joined.
fn wchoose_with(chooser: Pattern, pairs: &[(Pattern, f64)]) -> Pattern {
    let pats: Vec<Pattern> = pairs.iter().map(|(p, _)| p.clone()).collect();
    // Running cumulative weights, so a uniform draw maps to a weighted index.
    let mut total = 0.0;
    let cumulative: Vec<f64> = pairs
        .iter()
        .map(|(_, w)| {
            total += w.max(0.0);
            total
        })
        .collect();
    if total <= 0.0 {
        return silence();
    }
    let pats = Arc::new(pats);
    let cumulative = Arc::new(cumulative);
    chooser.fmap(move |v| {
        let target = v.as_f64().unwrap_or(0.0) * total;
        let idx = cumulative
            .iter()
            .position(|&c| c > target)
            .unwrap_or(pats.len() - 1);
        Value::Pat(Box::new(pats[idx].clone()))
    })
}

/// `wchoose`: continuously choose from weighted `(pattern, weight)` pairs.
pub fn wchoose(pairs: &[(Pattern, f64)]) -> Pattern {
    if pairs.is_empty() {
        return silence();
    }
    wchoose_with(rand(), pairs).outer_join()
}

/// `wchooseCycles`/`wrandcat`: pick one weighted pattern per cycle.
pub fn wrandcat(pairs: &[(Pattern, f64)]) -> Pattern {
    if pairs.is_empty() {
        return silence();
    }
    wchoose_with(rand().segment(Frac::one()), pairs).inner_join()
}

/// Build a pattern that cycles randomly through values each cycle. Convenience
/// wrapper over [`randcat`].
pub fn choose_cycles<I, T>(items: I) -> Pattern
where
    I: IntoIterator<Item = T>,
    T: Into<Value>,
{
    let pats: Vec<Pattern> = items
        .into_iter()
        .map(|v| value_to_pattern(v.into()))
        .collect();
    randcat(&pats)
}
