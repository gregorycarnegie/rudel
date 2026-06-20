use super::value_ops::{ValueOp, compose_op};
use crate::pattern::Pattern;
use crate::value::Value;
use std::sync::Arc;

/// The eight pattern alignments Strudel exposes on each operator
/// (`.add.out`, `.set.squeeze`, ...), bound in Koto as `add_out`/`set_squeeze`.
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
}
