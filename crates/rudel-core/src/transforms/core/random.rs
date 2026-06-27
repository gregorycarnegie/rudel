use crate::{pattern::Pattern, signal::rand, value::Value};

impl Pattern {
    // -- Randomness --------------------------------------------------------

    /// `degradeByWith`: keep events where `with_pat` exceeds `x`.
    pub fn degrade_by_with(&self, with_pat: Pattern, x: f64) -> Pattern {
        self.fmap(|a| Value::func(move |_| a.clone()))
            .app_left(&with_pat.filter_values(move |v| v.as_f64().unwrap_or(0.0) > x))
    }

    /// Randomly drop a proportion `x` of events (`degradeBy`).
    pub fn degrade_by(&self, x: f64) -> Pattern {
        self.degrade_by_with(rand(), x)
    }

    /// Randomly drop ~50% of events (`degrade`).
    pub fn degrade(&self) -> Pattern {
        self.degrade_by(0.5)
    }

    /// Inverse of `degradeBy` (`undegradeBy`).
    pub fn undegrade_by(&self, x: f64) -> Pattern {
        self.degrade_by_with(
            rand().fmap(|v| Value::F64(1.0 - v.as_f64().unwrap_or(0.0))),
            x,
        )
    }
}
