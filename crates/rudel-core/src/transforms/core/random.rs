use super::patternify::patternify_f64;
use crate::{pattern::Pattern, signal::rand, transforms::IntoPattern, value::Value};

impl Pattern {
    // -- Randomness --------------------------------------------------------

    /// `degradeByWith`: keep events where `with_pat` exceeds `x`.
    pub fn degrade_by_with(&self, with_pat: Pattern, x: f64) -> Pattern {
        self.fmap(|a| Value::func(move |_| a.clone()))
            .app_left(&with_pat.filter_values(move |v| v.as_f64().unwrap_or(0.0) > x))
    }

    /// Randomly drop a proportion `x` of events, for a fixed `x`.
    pub fn _degrade_by(&self, x: f64) -> Pattern {
        self.degrade_by_with(rand(), x)
    }

    /// Randomly drop a proportion `x` of events (`degradeBy`).
    ///
    /// The amount is patternified, as upstream's `register('degradeBy', …,
    /// true, true)` makes it: a signal or mini pattern is sampled per cycle and
    /// the raw op applied to each value. Taking a single number instead meant a
    /// patterned amount — `degradeBy(sine.range(0,.5).slow(32))`, which is how
    /// tunes make the density breathe — collapsed to one arbitrary value and
    /// kept events upstream drops.
    pub fn degrade_by(&self, x: impl IntoPattern) -> Pattern {
        patternify_f64(self, x.into_pattern(), |p, x| p._degrade_by(x))
    }

    /// Randomly drop ~50% of events (`degrade`).
    pub fn degrade(&self) -> Pattern {
        self._degrade_by(0.5)
    }

    /// Inverse of `degradeBy` for a fixed `x`.
    pub fn _undegrade_by(&self, x: f64) -> Pattern {
        self.degrade_by_with(
            rand().fmap(|v| Value::F64(1.0 - v.as_f64().unwrap_or(0.0))),
            x,
        )
    }

    /// Inverse of `degradeBy` (`undegradeBy`), with the same patternified
    /// amount as [`degrade_by`](Self::degrade_by).
    pub fn undegrade_by(&self, x: impl IntoPattern) -> Pattern {
        patternify_f64(self, x.into_pattern(), |p, x| p._undegrade_by(x))
    }
}
