use super::IntoPattern;
use crate::{fraction::Frac, hap::Hap, pattern::Pattern, timespan::TimeSpan, value::Value};

/// `stepcat` over weights that may be unknown, which is what a pattern with no
/// step count contributes.
///
/// Upstream's `stepcat` fills an unknown weight with the *average* of the known
/// ones, falls back to `fastcat` when none are known, and refuses the case where
/// a weight is explicitly `undefined` for every entry. `Pattern::stepcat`
/// defaults an unknown count to `1` instead, which is right when the caller
/// passed patterns and wrong when it passed weights, so the join below builds
/// its slices through this.
fn stepcat_maybe(timed: &[(Option<Frac>, Pattern)]) -> Pattern {
    let known: Vec<Frac> = timed.iter().filter_map(|(w, _)| *w).collect();
    if known.is_empty() {
        let pats: Vec<Pattern> = timed.iter().map(|(_, p)| p.clone()).collect();
        return crate::pattern::fastcat(&pats);
    }
    let sum = known.iter().fold(Frac::zero(), |a, b| a + *b);
    let average = sum / Frac::int(known.len() as i64);
    let pairs: Vec<(Frac, Pattern)> = timed
        .iter()
        .map(|(w, p)| (w.unwrap_or(average), p.clone()))
        .collect();
    crate::pattern::timecat(&pairs)
}

/// Cut one cycle of pattern-of-pattern haps into the spans where the set of
/// inner patterns is constant, and stack the patterns covering each span
/// (upstream's `_slices`/`_fitslice`).
///
/// No span *duration* comes back with them. Upstream's `_retime` looks as
/// though it weighs a stepless slice by `dur * (occupied_steps /
/// occupied_perc)`, but the filter it derives `occupied_perc` from is written
/// `.filter((t, pat) => pat.hasSteps)` — `Array.filter` passes `(element,
/// index)`, so `pat` is a number, `pat.hasSteps` is undefined, and the filter
/// keeps nothing. `occupied_perc` is therefore always 0, `total_steps` always
/// `undefined`, and `dur.mulmaybe(undefined)` gives back `undefined`: the
/// duration never reaches the output. Checked against real Strudel, whose
/// layout for slices of 3/4 + 1/4 is identical to its layout for 1/2 + 1/2.
fn slices(haps: &[Hap]) -> Vec<Pattern> {
    let mut points = vec![Frac::zero(), Frac::one()];
    for hap in haps {
        points.push(hap.part.begin);
        points.push(hap.part.end);
    }
    points.sort();
    points.dedup();
    points
        .windows(2)
        .map(|edges| {
            let span = TimeSpan::new(edges[0], edges[1]);
            let inner: Vec<Pattern> = haps
                .iter()
                .filter(|hap| span.intersection(&hap.part).is_some())
                .map(|hap| match &hap.value {
                    Value::Pat(pat) => (**pat).clone(),
                    other => crate::pattern::pure(other.clone()),
                })
                .collect();
            crate::pattern::stack(&inner)
        })
        .collect()
}

/// Weigh the slices by the step counts of the patterns in them, so a slice
/// occupying a quarter of the cycle but holding six steps is laid out as six
/// (upstream's `_retime`). A slice whose pattern has no step count has no
/// weight, and [`stepcat_maybe`] decides what to do with it.
fn retime(pats: Vec<Pattern>) -> Vec<(Option<Frac>, Pattern)> {
    pats.into_iter().map(|pat| (pat.steps, pat)).collect()
}

impl Pattern {
    /// `stepJoin`: flatten a pattern *of patterns* by laying the inner patterns
    /// out across the cycle in proportion to their step counts, rather than to
    /// the spans they arrived in.
    ///
    /// This is what makes a *patterned* argument to a stepwise function mean
    /// what <https://strudel.cc/learn/stepwise/> says it means: "the patterns
    /// from the changing values in the argument will be `stepcat`ted together".
    /// `expand("3 2 1")` is three differently-expanded copies laid end to end by
    /// their own step counts — an `inner_join` would instead give each a third
    /// of the cycle and lose the point.
    pub fn step_join(&self) -> Pattern {
        let laid_out = |pat: &Pattern, cycle: Frac| {
            let haps = pat._early(cycle).query_arc(Frac::zero(), Frac::one());
            stepcat_maybe(&retime(slices(&haps)))
        };
        // The step count of the joined pattern is the one the first cycle lays
        // out, which is what upstream carries as the new pattern's `_steps`.
        let steps = laid_out(self, Frac::zero()).steps;
        let inner = self.clone();
        Pattern::new(move |state| laid_out(&inner, state.span.begin.sam()).query(state))
            .split_queries()
            .set_steps(steps)
    }

    /// Apply a stepwise transform whose argument is a pattern, the way
    /// upstream's `stepRegister` does: build one result per argument value, then
    /// [`step_join`](Self::step_join) them.
    ///
    /// `build` is the plain-integer form, so every stepwise function that takes
    /// a count shares this.
    pub fn stepwise_pat(
        &self,
        arg: impl IntoPattern,
        build: fn(&Pattern, i64) -> Pattern,
    ) -> Pattern {
        let pat = self.clone();
        arg.into_pattern()
            .fmap(move |v| {
                let n = v.as_f64().unwrap_or(0.0) as i64;
                Value::Pat(Box::new(build(&pat, n)))
            })
            .step_join()
    }

    /// `keep`: keep this pattern's values, taking only keys from `other` that
    /// are not already set here (the inverse of [`set`](Self::set)).
    pub fn keep(&self, other: impl IntoPattern) -> Pattern {
        self.op_in(other.into_pattern(), |a: &Value, _b: &Value| a.clone())
    }

    /// `expand`: multiply the step count by `factor`, leaving timing unchanged.
    pub fn expand(&self, factor: impl Into<Frac>) -> Pattern {
        let f = factor.into();
        let mut p = self.clone();
        p.steps = p.steps.map(|s| s * f);
        p
    }

    /// `extend`: like `fast`, but also scales the step count (`fast` + `expand`).
    pub fn extend(&self, factor: impl Into<Frac>) -> Pattern {
        let f = factor.into();
        self._fast(f).expand(f)
    }

    /// `contract`: divide the step count by `factor`, leaving timing unchanged
    /// (the inverse of [`expand`](Self::expand)).
    pub fn contract(&self, factor: impl Into<Frac>) -> Pattern {
        let f = factor.into();
        let mut p = self.clone();
        if f != Frac::zero() {
            p.steps = p.steps.map(|s| s / f);
        }
        p
    }

    /// Build the progressively-zoomed slices used by [`shrink`](Self::shrink)
    /// and [`grow`](Self::grow). A positive `amount` drops steps from the start,
    /// a negative one from the end; the number of slices defaults to the step
    /// count (`shrinklist`).
    fn shrink_list(&self, amount: i64) -> Vec<Pattern> {
        let Some(steps) = self.steps else {
            return vec![self.clone()];
        };
        if amount == 0 || steps <= Frac::zero() {
            return vec![self.clone()];
        }
        let times = steps.to_f64().round() as i64;
        let from_start = amount > 0;
        let seg = Frac::int(amount.abs()) / steps;
        let mut out = Vec::new();
        for i in 0..times {
            let (s, e) = if from_start {
                let s = seg * Frac::int(i);
                if s > Frac::one() {
                    break;
                }
                (s, Frac::one())
            } else {
                let e = Frac::one() - seg * Frac::int(i);
                if e < Frac::zero() {
                    break;
                }
                (Frac::zero(), e)
            };
            let d = e - s;
            if d <= Frac::zero() {
                continue;
            }
            out.push(self.zoom(s, e).set_steps(Some(steps * d)));
        }
        out
    }

    /// `shrink`: progressively drop `amount` steps each repetition (from the
    /// start, or the end for a negative `amount`), concatenating the shrinking
    /// views stepwise.
    pub fn shrink(&self, amount: i64) -> Pattern {
        if self.steps.is_none() {
            return crate::pattern::silence();
        }
        crate::pattern::stepcat(&self.shrink_list(amount))
    }

    /// `grow`: the reverse of [`shrink`](Self::shrink) — progressively reveal
    /// more of the pattern each repetition.
    pub fn grow(&self, amount: i64) -> Pattern {
        if self.steps.is_none() {
            return crate::pattern::silence();
        }
        let mut list = self.shrink_list(-amount);
        list.reverse();
        crate::pattern::stepcat(&list)
    }

    /// `take`: keep the first `i` steps of a stepwise pattern, dropping the
    /// rest (a negative `i` takes from the end). Patterns without a step count
    /// become silence.
    fn _take(&self, i: Frac) -> Pattern {
        let Some(steps) = self.steps else {
            return crate::pattern::silence();
        };
        if steps <= Frac::zero() || i == Frac::zero() {
            return crate::pattern::silence();
        }
        let flip = i < Frac::zero();
        let i = if flip { -i } else { i };
        let frac = i / steps;
        if frac <= Frac::zero() {
            return crate::pattern::silence();
        }
        if frac >= Frac::one() {
            return self.clone();
        }
        let taken = if flip {
            self.zoom(Frac::one() - frac, Frac::one())
        } else {
            self.zoom(Frac::zero(), frac)
        };
        taken.set_steps(Some(i))
    }

    /// `take`: keep the first `n` steps (negative `n` takes from the end).
    pub fn take(&self, n: i64) -> Pattern {
        self._take(Frac::int(n))
    }

    /// `drop`: discard the first `n` steps of a stepwise pattern (negative `n`
    /// drops from the end). The inverse of [`take`](Self::take).
    pub fn drop(&self, n: i64) -> Pattern {
        let Some(steps) = self.steps else {
            return crate::pattern::silence();
        };
        let i = Frac::int(n);
        if i < Frac::zero() {
            self._take(steps + i)
        } else {
            self._take(-(steps - i))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::{fastcat, pure, silence};

    fn four() -> Pattern {
        fastcat(&[
            pure(Value::Int(0)),
            pure(Value::Int(1)),
            pure(Value::Int(2)),
            pure(Value::Int(3)),
        ])
    }

    /// `(begin, value)` per hap, which is enough to tell these apart and reads
    /// like the upstream output they were checked against.
    fn shape(p: &Pattern) -> Vec<(Frac, i64)> {
        let mut v: Vec<(Frac, i64)> = p
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| (h.part.begin, h.value.as_f64().unwrap() as i64))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn take_keeps_steps_from_whichever_end_the_sign_names() {
        // Values from real Strudel: `sequence(0,1,2,3).take(2)` / `.take(-2)`.
        assert_eq!(
            shape(&four().take(2)),
            vec![(Frac::zero(), 0), (Frac::new(1, 2), 1)]
        );
        assert_eq!(
            shape(&four().take(-2)),
            vec![(Frac::zero(), 2), (Frac::new(1, 2), 3)]
        );
        // Taking everything (or more) is the pattern itself; taking none is
        // silence, and so is a pattern with no step count to take from.
        assert_eq!(shape(&four().take(4)), shape(&four()));
        assert_eq!(shape(&four().take(8)), shape(&four()));
        assert!(
            four()
                .take(0)
                .query_arc(Frac::zero(), Frac::one())
                .is_empty()
        );
        assert!(
            silence()
                .take(2)
                .query_arc(Frac::zero(), Frac::one())
                .is_empty()
        );
    }

    #[test]
    fn drop_is_take_from_the_other_side() {
        // `drop(1)` discards the first step, `drop(-1)` the last.
        assert_eq!(
            shape(&four().drop(1)),
            vec![
                (Frac::zero(), 1),
                (Frac::new(1, 3), 2),
                (Frac::new(2, 3), 3),
            ]
        );
        assert_eq!(
            shape(&four().drop(-1)),
            vec![
                (Frac::zero(), 0),
                (Frac::new(1, 3), 1),
                (Frac::new(2, 3), 2),
            ]
        );
        // Dropping nothing keeps everything.
        assert_eq!(shape(&four().drop(0)), shape(&four()));
    }

    #[test]
    fn shrink_and_grow_walk_the_pattern_in_from_each_end() {
        // Upstream's `sequence(0,1,2,3).shrink(1)`: 4 steps, then 3, 2, 1 —
        // ten steps in all, each 1/10 of the cycle.
        assert_eq!(
            shape(&four().shrink(1))
                .into_iter()
                .map(|(_, v)| v)
                .collect::<Vec<i64>>(),
            vec![0, 1, 2, 3, 1, 2, 3, 2, 3, 3]
        );
        // `grow` is the same list reversed: 1 step, then 2, 3, 4.
        assert_eq!(
            shape(&four().grow(1))
                .into_iter()
                .map(|(_, v)| v)
                .collect::<Vec<i64>>(),
            vec![0, 0, 1, 0, 1, 2, 0, 1, 2, 3]
        );
        // No step count, nothing to walk.
        assert!(
            silence()
                .shrink(1)
                .query_arc(Frac::zero(), Frac::one())
                .is_empty()
        );
    }

    #[test]
    fn shrinking_by_nothing_leaves_the_pattern_alone() {
        // Zero is the "do nothing" amount: each repetition would drop no steps,
        // so the list is the pattern itself rather than `steps` copies of it.
        assert_eq!(shape(&four().shrink(0)), shape(&four()));
        assert_eq!(shape(&four().grow(0)), shape(&four()));
    }

    #[test]
    fn a_zero_step_pattern_is_silence_rather_than_a_division_by_zero() {
        // `take`/`drop` divide by the step count. Zero steps is reachable —
        // `setSteps(0)` is a public API — and there is nothing to take from it.
        let none = four().set_steps(Some(Frac::zero()));
        assert!(none.take(2).query_arc(Frac::zero(), Frac::one()).is_empty());
        assert!(none.drop(1).query_arc(Frac::zero(), Frac::one()).is_empty());
        // A negative count is no more takeable than a zero one.
        let negative = four().set_steps(Some(Frac::int(-2)));
        assert!(
            negative
                .take(2)
                .query_arc(Frac::zero(), Frac::one())
                .is_empty()
        );
    }

    #[test]
    fn step_join_lays_inner_patterns_out_by_their_own_step_counts() {
        // The point of `stepJoin` over `innerJoin`: a 3-step pattern and a
        // 2-step one share the cycle 3:2, not 1:1. Upstream's `expand("3 2")`
        // shows the same split (0-0.6, 0.6-1 with 5 steps in all).
        let three = fastcat(&[
            pure(Value::Int(0)),
            pure(Value::Int(1)),
            pure(Value::Int(2)),
        ]);
        let two = fastcat(&[pure(Value::Int(3)), pure(Value::Int(4))]);
        let outer = fastcat(&[
            pure(Value::Pat(Box::new(three))),
            pure(Value::Pat(Box::new(two))),
        ]);
        let joined = outer.step_join();
        assert_eq!(
            shape(&joined),
            vec![
                (Frac::zero(), 0),
                (Frac::new(1, 5), 1),
                (Frac::new(2, 5), 2),
                (Frac::new(3, 5), 3),
                (Frac::new(4, 5), 4),
            ]
        );
        // ...and the joined pattern carries the total.
        assert_eq!(joined.steps, Some(Frac::int(5)));
    }

    #[test]
    fn a_slice_with_no_step_count_is_weighed_at_the_average_of_those_that_have_one() {
        // Weights 3 and 1 are known, so the stepless slice is laid out as 2 —
        // six steps in all, split 3 : 1 : 2.
        let three = fastcat(&[
            pure(Value::Int(0)),
            pure(Value::Int(1)),
            pure(Value::Int(2)),
        ]);
        let one = pure(Value::Int(7));
        let stepless = pure(Value::Int(9)).set_steps(None);
        let outer = fastcat(&[
            pure(Value::Pat(Box::new(three))),
            pure(Value::Pat(Box::new(one))),
            pure(Value::Pat(Box::new(stepless))),
        ]);
        assert_eq!(
            shape(&outer.step_join()),
            vec![
                (Frac::zero(), 0),
                (Frac::new(1, 6), 1),
                (Frac::new(1, 3), 2),
                (Frac::new(1, 2), 7),
                (Frac::new(2, 3), 9),
            ]
        );
    }

    #[test]
    fn step_join_falls_back_to_equal_slices_when_no_step_count_is_known() {
        // With nothing to weigh them by, the inner patterns divide the cycle
        // evenly — `stepcat_maybe`'s "no known weights" branch.
        let a = pure(Value::Int(0));
        let b = pure(Value::Int(1));
        let outer = fastcat(&[pure(Value::Pat(Box::new(a))), pure(Value::Pat(Box::new(b)))]);
        let joined = outer.step_join();
        assert_eq!(
            shape(&joined),
            vec![(Frac::zero(), 0), (Frac::new(1, 2), 1)]
        );
    }
}
