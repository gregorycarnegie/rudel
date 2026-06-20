use super::IntoPattern;
use super::patternify::patternify_frac;
use crate::pattern::Pattern;

impl Pattern {
    // -- Time transforms (patternified) ------------------------------------

    pub fn fast(&self, n: impl IntoPattern) -> Pattern {
        patternify_frac(self, n.into_pattern(), |p, f| p._fast(f))
    }
    pub fn slow(&self, n: impl IntoPattern) -> Pattern {
        patternify_frac(self, n.into_pattern(), |p, f| p._slow(f))
    }
    pub fn early(&self, n: impl IntoPattern) -> Pattern {
        patternify_frac(self, n.into_pattern(), |p, f| p._early(f))
    }
    pub fn late(&self, n: impl IntoPattern) -> Pattern {
        patternify_frac(self, n.into_pattern(), |p, f| p._late(f))
    }
    pub fn ply(&self, n: impl IntoPattern) -> Pattern {
        patternify_frac(self, n.into_pattern(), |p, f| p._ply(f))
    }
    pub fn fast_gap(&self, n: impl IntoPattern) -> Pattern {
        patternify_frac(self, n.into_pattern(), |p, f| p._fast_gap(f))
    }
}
