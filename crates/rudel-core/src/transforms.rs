// transforms.rs - patternified, argument-lifting transforms.
// These wrap the raw `_`-prefixed ops in pattern.rs the way Strudel's
// `register` mechanism does: arguments can themselves be patterns.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::Frac;
use crate::pattern::{Pattern, pure};
use crate::signal::rand;
use crate::value::Value;
use std::sync::Arc;

/// Anything that can be lifted into a pattern argument.
pub trait IntoPattern {
    fn into_pattern(self) -> Pattern;
}

impl IntoPattern for Pattern {
    fn into_pattern(self) -> Pattern {
        self
    }
}
impl IntoPattern for &Pattern {
    fn into_pattern(self) -> Pattern {
        self.clone()
    }
}
impl IntoPattern for Value {
    fn into_pattern(self) -> Pattern {
        crate::pattern::value_to_pattern(self)
    }
}
macro_rules! into_pattern_via {
    ($($t:ty => $variant:expr),* $(,)?) => {
        $(impl IntoPattern for $t {
            fn into_pattern(self) -> Pattern { pure($variant(self)) }
        })*
    };
}
into_pattern_via!(i64 => Value::Int, f64 => Value::F64, bool => Value::Bool, Frac => Value::Frac);
impl IntoPattern for i32 {
    fn into_pattern(self) -> Pattern {
        pure(Value::Int(self as i64))
    }
}
impl IntoPattern for &str {
    fn into_pattern(self) -> Pattern {
        crate::pattern::parse_string(self)
    }
}
impl IntoPattern for String {
    fn into_pattern(self) -> Pattern {
        crate::pattern::parse_string(&self)
    }
}

/// Patternify a single `Frac`-valued argument, applying raw op `f(pat, frac)`.
/// Fast-paths pure arguments (preserving steps), matching Strudel's `register`.
fn patternify_frac<F>(pat: &Pattern, arg: Pattern, f: F) -> Pattern
where
    F: Fn(&Pattern, Frac) -> Pattern + Send + Sync + 'static,
{
    if let Some(v) = &arg.pure_value {
        return f(pat, v.to_frac());
    }
    let pat = pat.clone();
    let f = Arc::new(f);
    arg.fmap(move |v| Value::Pat(Box::new(f(&pat, v.to_frac()))))
        .inner_join()
}

// ---------------------------------------------------------------------------
// Value-level numeric / structural ops (value.mjs `_composeOp` + COMPOSERS).

fn as_map(v: &Value) -> Value {
    match v {
        Value::Map(_) => v.clone(),
        other => {
            let mut m = std::collections::BTreeMap::new();
            m.insert("value".to_string(), other.clone());
            Value::Map(m)
        }
    }
}

/// Combine two values with `op`, unioning structurally when either is a map
/// (`_composeOp`).
fn compose_op(a: &Value, b: &Value, op: &(dyn Fn(&Value, &Value) -> Value + Send + Sync)) -> Value {
    match (a, b) {
        (Value::Map(_), _) | (_, Value::Map(_)) => as_map(a).union_with(&as_map(b), op),
        _ => op(a, b),
    }
}

fn num_add(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Value::Int(x + y),
        _ => Value::F64(a.as_f64().unwrap_or(0.0) + b.as_f64().unwrap_or(0.0)),
    }
}
fn num_sub(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Value::Int(x - y),
        _ => Value::F64(a.as_f64().unwrap_or(0.0) - b.as_f64().unwrap_or(0.0)),
    }
}
fn num_mul(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Value::Int(x * y),
        _ => Value::F64(a.as_f64().unwrap_or(0.0) * b.as_f64().unwrap_or(0.0)),
    }
}
fn num_div(a: &Value, b: &Value) -> Value {
    Value::F64(a.as_f64().unwrap_or(0.0) / b.as_f64().unwrap_or(1.0))
}

impl Pattern {
    /// `_opIn`: structure from the left (this) pattern.
    pub(crate) fn op_in<O>(&self, other: Pattern, op: O) -> Pattern
    where
        O: Fn(&Value, &Value) -> Value + Send + Sync + 'static,
    {
        let op = Arc::new(op);
        self.fmap(move |a| {
            let op = op.clone();
            Value::func(move |b| compose_op(&a, &b, &*op))
        })
        .app_left(&other)
    }

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

    // -- Structure ---------------------------------------------------------

    pub(crate) fn _segment(&self, rate: Frac) -> Pattern {
        self.struct_pat(pure(Value::Bool(true))._fast(rate))
            .set_steps(Some(rate))
    }

    /// Sample a continuous pattern into `n` discrete steps per cycle.
    pub fn segment(&self, n: impl IntoPattern) -> Pattern {
        patternify_frac(self, n.into_pattern(), |p, f| p._segment(f))
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

    // -- Math / value ops --------------------------------------------------

    pub fn add(&self, other: impl IntoPattern) -> Pattern {
        self.op_in(other.into_pattern(), num_add)
    }
    pub fn sub(&self, other: impl IntoPattern) -> Pattern {
        self.op_in(other.into_pattern(), num_sub)
    }
    pub fn mul(&self, other: impl IntoPattern) -> Pattern {
        self.op_in(other.into_pattern(), num_mul)
    }
    pub fn div(&self, other: impl IntoPattern) -> Pattern {
        self.op_in(other.into_pattern(), num_div)
    }
    /// `set`: override this pattern's values (and map keys) with the other's,
    /// keeping this pattern's structure.
    pub fn set(&self, other: impl IntoPattern) -> Pattern {
        self.op_in(other.into_pattern(), |_, b| b.clone())
    }

    /// Scale a unipolar (0..1) signal into the `min..max` range.
    pub fn range(&self, min: f64, max: f64) -> Pattern {
        self.fmap(move |v| Value::F64(v.as_f64().unwrap_or(0.0) * (max - min) + min))
    }

    // -- Higher-order combinators ------------------------------------------

    /// Apply `f` to a layered copy and stack it on top (`superimpose`).
    pub fn superimpose<F>(&self, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.stack_with(&f(self))
    }

    /// Layer copies produced by each function on top of this pattern (`layer`).
    pub fn layer<F>(&self, funcs: &[F]) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        let pats: Vec<Pattern> = funcs.iter().map(|f| f(self)).collect();
        crate::pattern::stack(&pats)
    }

    /// Offset a copy by `time` cycles, transform it with `f`, and stack it
    /// (`off`).
    pub fn off<F>(&self, time: impl IntoPattern, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        let shifted = self.late(time);
        self.stack_with(&f(&shifted))
    }

    /// Apply `f` every `n`th cycle, on the first cycle of each group
    /// (`every`/`firstOf`).
    pub fn every<F>(&self, n: i64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        if n <= 0 {
            return self.clone();
        }
        let mut pats = Vec::with_capacity(n as usize);
        pats.push(f(self));
        for _ in 1..n {
            pats.push(self.clone());
        }
        crate::pattern::slowcat_prime(&pats)
    }

    /// Alias for [`every`](Self::every) (`firstOf`).
    pub fn first_of<F>(&self, n: i64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.every(n, f)
    }

    /// Apply `f` every `n`th cycle, on the *last* cycle of each group
    /// (`lastOf`).
    pub fn last_of<F>(&self, n: i64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        if n <= 0 {
            return self.clone();
        }
        let mut pats: Vec<Pattern> = (0..n - 1).map(|_| self.clone()).collect();
        pats.push(f(self));
        crate::pattern::slowcat_prime(&pats)
    }

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
