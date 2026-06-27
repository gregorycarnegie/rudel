use super::IntoPattern;
use crate::{fraction::Frac, pattern::Pattern, value::Value};

impl Pattern {
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
