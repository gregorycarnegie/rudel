// transforms/core.rs - patternified, argument-lifting transforms.
// These wrap the raw `_`-prefixed ops in pattern.rs the way Strudel's
// `register` mechanism does: arguments can themselves be patterns.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::Frac;
use crate::pattern::{Pattern, pure};
use crate::signal::rand;
use crate::value::Value;
use std::sync::Arc;

/// A shared two-argument value combiner (the per-element op behind `add`, `set`,
/// ... before map-structural composition).
type ValueOp = Arc<dyn Fn(&Value, &Value) -> Value + Send + Sync>;

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

/// The eight pattern alignments Strudel exposes on each operator
/// (`.add.out`, `.set.squeeze`, ...). `Poly` is not yet ported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Structure from the left (this) pattern. The default.
    In,
    /// Structure from the right (other) pattern.
    Out,
    /// Structure from the intersection of both.
    Mix,
    /// Squeeze one cycle of `other` into each event of this pattern.
    Squeeze,
    /// Squeeze one cycle of this pattern into each event of `other`.
    SqueezeOut,
    /// Retrigger this pattern at each onset of `other`, aligned to cycle pos.
    Reset,
    /// Retrigger this pattern at each onset of `other`, aligned to cycle zero.
    Restart,
    /// Polymetric: align step counts via `extend`, then outer-join.
    Poly,
}

/// Generate the six non-default alignment methods for one operator, e.g.
/// `add_out`, `add_squeeze`, ... The default (`in`) variant stays as the plain
/// `add`/`sub`/... method.
macro_rules! aligned_variants {
    ($op:expr; $out:ident $mix:ident $sq:ident $sqo:ident $reset:ident $restart:ident $poly:ident) => {
        #[doc = "Polymetric alignment (`poly`)."]
        pub fn $poly(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Poly, $op)
        }
        #[doc = "Structure from the right (`out` alignment)."]
        pub fn $out(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Out, $op)
        }
        #[doc = "Structure from the intersection of both (`mix` alignment)."]
        pub fn $mix(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Mix, $op)
        }
        #[doc = "Squeeze one cycle of `other` into each event (`squeeze`)."]
        pub fn $sq(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Squeeze, $op)
        }
        #[doc = "Squeeze one cycle of this into each event of `other` (`squeezeOut`)."]
        pub fn $sqo(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::SqueezeOut, $op)
        }
        #[doc = "Retrigger this pattern at each onset of `other` (`reset`)."]
        pub fn $reset(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Reset, $op)
        }
        #[doc = "Retrigger from cycle zero at each onset of `other` (`restart`)."]
        pub fn $restart(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Restart, $op)
        }
    };
}

impl Pattern {
    /// Lift a value combiner into the curried, map-structural form the
    /// applicative ops apply (`a => b => _composeOp(a, b, op)`).
    fn compose_curry(op: ValueOp) -> impl Fn(Value) -> Value + Send + Sync + 'static {
        move |a| {
            let op = op.clone();
            Value::func(move |b| compose_op(&a, &b, &*op))
        }
    }

    /// `_opIn`: structure from the left (this) pattern.
    pub(crate) fn op_in<O>(&self, other: Pattern, op: O) -> Pattern
    where
        O: Fn(&Value, &Value) -> Value + Send + Sync + 'static,
    {
        self.fmap(Self::compose_curry(Arc::new(op)))
            .app_left(&other)
    }

    /// `_opOut`: structure from the right (other) pattern.
    pub(crate) fn op_out<O>(&self, other: Pattern, op: O) -> Pattern
    where
        O: Fn(&Value, &Value) -> Value + Send + Sync + 'static,
    {
        self.fmap(Self::compose_curry(Arc::new(op)))
            .app_right(&other)
    }

    /// `_opMix`: structure from both (intersection of wholes).
    pub(crate) fn op_mix<O>(&self, other: Pattern, op: O) -> Pattern
    where
        O: Fn(&Value, &Value) -> Value + Send + Sync + 'static,
    {
        self.fmap(Self::compose_curry(Arc::new(op)))
            .app_both(&other)
    }

    /// `_opSqueeze`: squeeze one cycle of `other` into each of this pattern's
    /// events.
    pub(crate) fn op_squeeze<O>(&self, other: Pattern, op: O) -> Pattern
    where
        O: Fn(&Value, &Value) -> Value + Send + Sync + 'static,
    {
        let op = Arc::new(op);
        self.fmap(move |a| {
            let op = op.clone();
            let other = other.clone();
            Value::Pat(Box::new(other.fmap(move |b| compose_op(&a, &b, &*op))))
        })
        .squeeze_join()
    }

    /// `_opSqueezeOut`: squeeze one cycle of this pattern into each of `other`'s
    /// events (this pattern keeps the value orientation: `compose_op(this, other)`).
    pub(crate) fn op_squeeze_out<O>(&self, other: Pattern, op: O) -> Pattern
    where
        O: Fn(&Value, &Value) -> Value + Send + Sync + 'static,
    {
        let op = Arc::new(op);
        let this = self.clone();
        other
            .fmap(move |a| {
                let op = op.clone();
                let this = this.clone();
                Value::Pat(Box::new(this.fmap(move |b| compose_op(&b, &a, &*op))))
            })
            .squeeze_join()
    }

    /// `_opReset`/`_opRestart`: retrigger this pattern at each onset of `other`.
    fn op_reset_impl<O>(&self, other: Pattern, op: O, restart: bool) -> Pattern
    where
        O: Fn(&Value, &Value) -> Value + Send + Sync + 'static,
    {
        let op = Arc::new(op);
        let this = self.clone();
        let joined = other.fmap(move |b| {
            let op = op.clone();
            let this = this.clone();
            Value::Pat(Box::new(this.fmap(move |a| compose_op(&a, &b, &*op))))
        });
        if restart {
            joined.restart_join()
        } else {
            joined.reset_join()
        }
    }

    /// `_opPoly`: combine polymetrically. Note the orientation matches Strudel
    /// (`compose_op(other, this)`): `this` provides the outer structure.
    pub(crate) fn op_poly<O>(&self, other: Pattern, op: O) -> Pattern
    where
        O: Fn(&Value, &Value) -> Value + Send + Sync + 'static,
    {
        let op = Arc::new(op);
        self.fmap(move |b| {
            let op = op.clone();
            let other = other.clone();
            Value::Pat(Box::new(other.fmap(move |a| compose_op(&a, &b, &*op))))
        })
        .poly_join()
    }

    /// Combine this pattern with `other` using value-combiner `op` under the
    /// given [`Align`]ment.
    pub(crate) fn op_align<O>(&self, other: Pattern, align: Align, op: O) -> Pattern
    where
        O: Fn(&Value, &Value) -> Value + Send + Sync + 'static,
    {
        match align {
            Align::In => self.op_in(other, op),
            Align::Out => self.op_out(other, op),
            Align::Mix => self.op_mix(other, op),
            Align::Squeeze => self.op_squeeze(other, op),
            Align::SqueezeOut => self.op_squeeze_out(other, op),
            Align::Reset => self.op_reset_impl(other, op, false),
            Align::Restart => self.op_reset_impl(other, op, true),
            Align::Poly => self.op_poly(other, op),
        }
    }

    // -- Alignment matrix --------------------------------------------------
    // Each operator's default (`in`) variant is the plain method (`add`, `set`,
    // ...); these generate the remaining alignments (`add_out`, `set_squeeze`, ...).

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

    aligned_variants!(num_add; add_out add_mix add_squeeze add_squeezeout add_reset add_restart add_poly);
    aligned_variants!(num_sub; sub_out sub_mix sub_squeeze sub_squeezeout sub_reset sub_restart sub_poly);
    aligned_variants!(num_mul; mul_out mul_mix mul_squeeze mul_squeezeout mul_reset mul_restart mul_poly);
    aligned_variants!(num_div; div_out div_mix div_squeeze div_squeezeout div_reset div_restart div_poly);
    aligned_variants!(|_a: &Value, b: &Value| b.clone();
        set_out set_mix set_squeeze set_squeezeout set_reset set_restart set_poly);
    aligned_variants!(|a: &Value, _b: &Value| a.clone();
        keep_out keep_mix keep_squeeze keep_squeezeout keep_reset keep_restart keep_poly);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::{fastcat, pure};
    use std::collections::BTreeMap;

    fn vals(pat: &Pattern) -> Vec<Value> {
        let mut haps = pat.query_arc(Frac::zero(), Frac::one());
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter().map(|h| h.value).collect()
    }

    fn seq(items: &[i64]) -> Pattern {
        fastcat(
            &items
                .iter()
                .map(|&n| pure(Value::Int(n)))
                .collect::<Vec<_>>(),
        )
    }

    fn onsets(pat: &Pattern) -> usize {
        pat.query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .filter(|h| h.has_onset())
            .count()
    }

    #[test]
    fn add_in_takes_left_structure() {
        // "0 1".add("10 20 30") -> 2 onsets (structure from left)
        assert_eq!(onsets(&seq(&[0, 1]).add(seq(&[10, 20, 30]))), 2);
    }

    #[test]
    fn add_out_takes_right_structure() {
        // "0 1".add.out("10 20 30") -> 3 onsets (structure from right)
        assert_eq!(onsets(&seq(&[0, 1]).add_out(seq(&[10, 20, 30]))), 3);
    }

    #[test]
    fn add_squeeze_fits_other_per_event() {
        // each of the 2 events gets a full cycle of "10 20" squeezed in -> 4 haps
        let pat = seq(&[0, 1]).add_squeeze(seq(&[10, 20]));
        assert_eq!(
            vals(&pat),
            vec![
                Value::Int(10),
                Value::Int(20),
                Value::Int(11),
                Value::Int(21)
            ]
        );
    }

    #[test]
    fn set_squeeze_merges_maps() {
        // {note:0} set.squeeze {s:a}{s:b} -> per note event, two {note,s} haps
        let note = pure(Value::Map(BTreeMap::from([("note".into(), Value::Int(0))])));
        let s = fastcat(&[
            pure(Value::Map(BTreeMap::from([(
                "s".into(),
                Value::Str("a".into()),
            )]))),
            pure(Value::Map(BTreeMap::from([(
                "s".into(),
                Value::Str("b".into()),
            )]))),
        ]);
        let pat = note.set_squeeze(s);
        let got = vals(&pat);
        assert_eq!(got.len(), 2);
        match &got[0] {
            Value::Map(m) => {
                assert_eq!(m.get("note"), Some(&Value::Int(0)));
                assert_eq!(m.get("s"), Some(&Value::Str("a".into())));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn expand_scales_step_count_only() {
        // "0 1" has 2 steps; expand(3) -> 6 steps, same timing (2 onsets/cycle)
        let pat = seq(&[0, 1]).expand(3);
        assert_eq!(pat.steps, Some(Frac::int(6)));
        assert_eq!(onsets(&pat), 2);
    }

    #[test]
    fn extend_is_fast_plus_expand() {
        // extend(2) of "0 1" -> fast(2) (4 onsets/cycle) and steps 2*2 = 4
        let pat = seq(&[0, 1]).extend(2);
        assert_eq!(pat.steps, Some(Frac::int(4)));
        assert_eq!(onsets(&pat), 4);
    }

    #[test]
    fn contract_divides_step_count_only() {
        // "0 1 2 3" has 4 steps; contract(2) -> 2 steps, same timing (4 onsets).
        let pat = seq(&[0, 1, 2, 3]).contract(2);
        assert_eq!(pat.steps, Some(Frac::int(2)));
        assert_eq!(onsets(&pat), 4);
    }

    #[test]
    fn shrink_progressively_drops_steps() {
        // "0 1 2 3".shrink(1) == "0 1 2 3 1 2 3 2 3 3" (10 steps).
        let pat = seq(&[0, 1, 2, 3]).shrink(1);
        assert_eq!(pat.steps, Some(Frac::int(10)));
        assert_eq!(
            vals(&pat),
            [0, 1, 2, 3, 1, 2, 3, 2, 3, 3]
                .into_iter()
                .map(Value::Int)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn grow_progressively_reveals_steps() {
        // "0 1 2 3".grow(1) == "0 0 1 0 1 2 0 1 2 3" (10 steps).
        let pat = seq(&[0, 1, 2, 3]).grow(1);
        assert_eq!(pat.steps, Some(Frac::int(10)));
        assert_eq!(
            vals(&pat),
            [0, 0, 1, 0, 1, 2, 0, 1, 2, 3]
                .into_iter()
                .map(Value::Int)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn shrink_grow_need_step_metadata() {
        // a continuous signal has no step count -> silence.
        assert!(
            rand()
                .shrink(1)
                .query_arc(Frac::zero(), Frac::one())
                .is_empty()
        );
    }

    #[test]
    fn add_poly_aligns_step_counts() {
        // "0 1 2" (3 steps) add.poly "10 20" (2 steps): outer 3 steps drive it,
        // the other is extended to 3 steps -> 3 onsets, first value 0+10.
        let pat = seq(&[0, 1, 2]).add_poly(seq(&[10, 20]));
        assert_eq!(onsets(&pat), 3);
        assert_eq!(vals(&pat)[0], Value::Int(10));
    }

    #[test]
    fn keep_prefers_left_value() {
        // {s:bd} keep {s:sd, n:1} -> keeps s:bd, gains n:1
        let a = pure(Value::Map(BTreeMap::from([(
            "s".into(),
            Value::Str("bd".into()),
        )])));
        let b = pure(Value::Map(BTreeMap::from([
            ("s".into(), Value::Str("sd".into())),
            ("n".into(), Value::Int(1)),
        ])));
        match &vals(&a.keep(b))[0] {
            Value::Map(m) => {
                assert_eq!(m.get("s"), Some(&Value::Str("bd".into())));
                assert_eq!(m.get("n"), Some(&Value::Int(1)));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }
}
