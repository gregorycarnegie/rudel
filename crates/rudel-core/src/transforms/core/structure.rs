use super::IntoPattern;
use super::patternify::patternify_frac;
use crate::fraction::Frac;
use crate::pattern::{Pattern, pure};
use crate::value::Value;

impl Pattern {
    // -- Structure ---------------------------------------------------------

    pub(crate) fn _segment(&self, rate: Frac) -> Pattern {
        self.struct_pat(pure(Value::Bool(true))._fast(rate))
            .set_steps(Some(rate))
    }

    /// Sample a continuous pattern into `n` discrete steps per cycle.
    pub fn segment(&self, n: impl IntoPattern) -> Pattern {
        patternify_frac(self, n.into_pattern(), |p, f| p._segment(f))
    }

    /// Alias for [`segment`](Self::segment) (`seg`).
    pub fn seg(&self, n: impl IntoPattern) -> Pattern {
        self.segment(n)
    }

    /// Restructure to the onsets of a boolean pattern, keeping this pattern's
    /// values (`struct`). Named `struct_pat` because `struct` is a keyword.
    pub fn struct_pat(&self, bools: impl IntoPattern) -> Pattern {
        self.fmap(|a| Value::func(move |b| if b.truthy() { a.clone() } else { Value::Null }))
            .app_right(&bools.into_pattern())
            .filter_values(|v| !matches!(v, Value::Null))
    }

    /// Silence this pattern wherever the mask pattern is false (`mask`).
    pub fn mask(&self, bools: impl IntoPattern) -> Pattern {
        self.fmap(|a| Value::func(move |b| if b.truthy() { a.clone() } else { Value::Null }))
            .app_left(&bools.into_pattern())
            .filter_values(|v| !matches!(v, Value::Null))
    }
}
