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

#[cfg(test)]
mod tests {
    use super::*;

    fn onsets(p: &Pattern, cycle: i64) -> Vec<(Frac, Frac)> {
        let mut v: Vec<(Frac, Frac)> = p
            .query_arc(Frac::int(cycle), Frac::int(cycle + 1))
            .into_iter()
            .map(|h| {
                let w = h.whole.expect("morph events are discrete");
                (w.begin, w.end)
            })
            .collect();
        v.sort();
        v
    }

    fn list(bits: &[i64]) -> Vec<Value> {
        bits.iter().map(|b| Value::Int(*b)).collect()
    }

    #[test]
    fn morph_interpolates_each_onset_between_its_two_positions() {
        // Onsets at 0 and 1/2 morphing to 0 and 1/4. Each event's *duration*
        // is one step of the `from` rhythm (1/4), whatever `by` is.
        let from = list(&[1, 0, 1, 0]);
        let to = list(&[1, 1, 0, 0]);
        let at = |by: Frac| onsets(&morph_inner(&from, &to, by), 0);

        assert_eq!(
            at(Frac::zero()),
            vec![
                (Frac::zero(), Frac::new(1, 4)),
                (Frac::new(1, 2), Frac::new(3, 4)),
            ],
            "by = 0 is the `from` rhythm"
        );
        assert_eq!(
            at(Frac::one()),
            vec![
                (Frac::zero(), Frac::new(1, 4)),
                (Frac::new(1, 4), Frac::new(1, 2)),
            ],
            "by = 1 is the `to` rhythm"
        );
        // Halfway is the midpoint of the two positions, not either end.
        assert_eq!(
            at(Frac::new(1, 2)),
            vec![
                (Frac::zero(), Frac::new(1, 4)),
                (Frac::new(3, 8), Frac::new(5, 8)),
            ]
        );
    }

    #[test]
    fn morph_repeats_in_every_cycle_at_the_same_offsets() {
        // The arcs are built once in cycle-relative terms and shifted by the
        // queried cycle, so cycle 2 looks like cycle 0 moved along by 2.
        let p = morph_inner(&list(&[1, 0, 1, 0]), &list(&[1, 1, 0, 0]), Frac::zero());
        assert_eq!(
            onsets(&p, 2),
            vec![
                (Frac::int(2), Frac::new(9, 4)),
                (Frac::new(5, 2), Frac::new(11, 4)),
            ]
        );
        // A query that lands *inside* a cycle clips the part but keeps the
        // whole, and both carry the same cycle offset.
        // (2 + 1/8 .. 2 + 1/4 clips the first event, which runs 2 .. 2 + 1/4.)
        let clipped = p.query_arc(Frac::new(17, 8), Frac::new(9, 4));
        assert_eq!(clipped.len(), 1);
        assert_eq!(clipped[0].part.begin, Frac::new(17, 8));
        assert_eq!(clipped[0].whole.unwrap().begin, Frac::int(2));

        // An empty rhythm has nothing to morph.
        assert!(
            morph_inner(&[], &list(&[1]), Frac::zero())
                .query_arc(Frac::zero(), Frac::one())
                .is_empty()
        );
    }
}
