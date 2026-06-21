use crate::pattern::{Pattern, stack};
use crate::value::{Value, ValueMap};

/// Strudel's plateau cross-fade gain curve: full until the midpoint, then
/// ramps to zero (`fadeGain`).
fn fade_gain(p: f64) -> f64 {
    if p < 0.5 { 1.0 } else { 1.0 - (p - 0.5) / 0.5 }
}

/// Cross-fade between `a` and `b` as `pos` goes 0→1 by scaling each side's
/// `gain` (`xfade`). Pure pattern combinator — no DSP.
pub fn xfade(a: Pattern, pos: Pattern, b: Pattern) -> Pattern {
    let gain_map = |g: f64| Value::Map(ValueMap::from([("gain".to_string(), Value::F64(g))]));
    let gain_a = pos.fmap(move |v| gain_map(fade_gain(v.as_f64().unwrap_or(0.0))));
    let gain_b = pos.fmap(move |v| gain_map(fade_gain(1.0 - v.as_f64().unwrap_or(0.0))));
    stack(&[a.mul(gain_a), b.mul(gain_b)])
}

impl Pattern {
    /// Cross-fade from this pattern to `b` as `pos` goes 0→1, via an
    /// equal-plateau `gain` curve (`xfade`).
    pub fn xfade(
        &self,
        pos: impl crate::transforms::IntoPattern,
        b: impl crate::transforms::IntoPattern,
    ) -> Pattern {
        xfade(self.clone(), pos.into_pattern(), b.into_pattern())
    }
}
