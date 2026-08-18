use super::value_ops::{ValueOp, compose_op};
use crate::{pattern::Pattern, value::Value};
use std::sync::Arc;

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
    pub(crate) fn op_reset_impl<O>(&self, other: Pattern, op: O, restart: bool) -> Pattern
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
}
