use crate::{
    fraction::Frac,
    hap::Hap,
    pattern::{Pattern, silence},
    timespan::TimeSpan,
    transforms::IntoPattern,
    value::Value,
};

/// Truthy entries of a binary rhythm list as `(position, value)`, where the
/// position is the index normalized to `[0, 1)`.
fn morph_positions(list: &[Value]) -> Vec<(Frac, Value)> {
    let len = list.len().max(1) as i64;
    list.iter()
        .enumerate()
        .filter(|(_, v)| v.truthy())
        .map(|(i, v)| (Frac::int(i as i64) / Frac::int(len), v.clone()))
        .collect()
}

/// Morph between two binary rhythms (`from`/`to`, lists of 1s and 0s with the
/// same number of true values) by `by` in 0→1 (`_morph`). Produces a boolean
/// structure pattern with each onset interpolated between its `from` and `to`
/// position.
fn morph_inner(from: &[Value], to: &[Value], by: Frac) -> Pattern {
    if from.is_empty() {
        return silence();
    }
    let dur = Frac::one() / Frac::int(from.len() as i64);
    let from_pos = morph_positions(from);
    let to_pos = morph_positions(to);
    let arcs: Vec<TimeSpan> = from_pos
        .iter()
        .zip(to_pos.iter())
        .map(|((pa, _), (pb, _))| {
            let b = by * (*pb - *pa) + *pa;
            TimeSpan::new(b, b + dur)
        })
        .collect();
    Pattern::new(move |state| {
        let cycle = state.span.begin.sam();
        let cyc_arc = state.span.cycle_arc();
        let mut out = Vec::new();
        for whole in &arcs {
            if let Some(part) = whole.intersection(&cyc_arc) {
                out.push(Hap::new(
                    Some(whole.with_time(|x| x + cycle)),
                    part.with_time(|x| x + cycle),
                    Value::Bool(true),
                ));
            }
        }
        out
    })
    .split_queries()
}

/// `morph(from, to, by)`: morph between two binary rhythms by a 0→1 pattern.
/// `from`/`to` are list-valued patterns; `by` is sampled per cycle.
pub fn morph(from: impl IntoPattern, to: impl IntoPattern, by: impl IntoPattern) -> Pattern {
    let to_pat = to.into_pattern();
    let by_pat = by.into_pattern();
    from.into_pattern().inner_bind(move |fv| {
        let by_pat = by_pat.clone();
        let from_list = as_list(&fv);
        to_pat.inner_bind(move |tv| {
            let from_list = from_list.clone();
            let to_list = as_list(&tv);
            by_pat.inner_bind(move |bv| morph_inner(&from_list, &to_list, bv.to_frac()))
        })
    })
}

/// View a value as a list of positional items (a list yields its items, a
/// scalar is a one-item list).
fn as_list(v: &Value) -> Vec<Value> {
    match v {
        Value::List(items) => items.clone(),
        other => vec![other.clone()],
    }
}
