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
/// inner patterns is constant, pairing each span with those patterns stacked
/// (upstream's `_slices`/`_fitslice`).
fn slices(haps: &[Hap]) -> Vec<(Frac, Pattern)> {
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
            (edges[1] - edges[0], crate::pattern::stack(&inner))
        })
        .collect()
}

/// Re-weight the slices by the step counts of the patterns in them, so a slice
/// occupying a quarter of the cycle but holding six steps is laid out as six
/// (upstream's `_retime`). A slice whose pattern has no step count has no
/// weight, and [`stepcat_maybe`] decides what to do with it.
fn retime(timed: Vec<(Frac, Pattern)>) -> Vec<(Option<Frac>, Pattern)> {
    timed
        .into_iter()
        .map(|(_dur, pat)| (pat.steps, pat))
        .collect()
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
