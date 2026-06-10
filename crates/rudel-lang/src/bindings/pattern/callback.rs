use super::KPattern;
use super::args::method_arg;
use super::convert::{koto_to_value, value_to_koto};
use koto::prelude::*;
use koto::runtime::{Error as KotoError, Result as KotoResult};
use rudel_core::{Frac, Pattern, Value};
use std::cell::RefCell;

/// Marshals a Koto callable into the `Fn(&Pattern) -> Pattern` shape that the
/// engine's higher-order combinators (`every`, `jux`, `sometimes`, ...) expect.
///
/// Those combinators apply their callback *eagerly* at construction time, so we
/// can drive the callback synchronously on a VM spawned from the method's VM
/// (the immutable `MethodContext` VM can't call functions itself). The first
/// error raised by the callback is captured and surfaced once the combinator
/// returns; on error the input pattern is passed through unchanged.
pub(super) struct Callback {
    vm: RefCell<KotoVm>,
    func: KValue,
    err: RefCell<Option<KotoError>>,
}

impl Callback {
    pub(super) fn new(ctx: &MethodContext<KPattern>, func: KValue) -> Self {
        Self {
            vm: RefCell::new(ctx.vm.spawn_shared_vm()),
            func,
            err: RefCell::new(None),
        }
    }

    /// Invoke the Koto function with `p` wrapped as a `KPattern`.
    pub(super) fn apply(&self, p: &Pattern) -> Pattern {
        let arg: KValue = KPattern(p.clone()).into();
        let call = self
            .vm
            .borrow_mut()
            .call_function(self.func.clone(), CallArgs::Single(arg));
        match call {
            Ok(KValue::Object(o)) if o.is_a::<KPattern>() => {
                o.cast::<KPattern>().unwrap().0.clone()
            }
            Ok(_) => p.clone(),
            Err(e) => {
                if self.err.borrow().is_none() {
                    *self.err.borrow_mut() = Some(e);
                }
                p.clone()
            }
        }
    }

    /// Invoke the Koto function with a single Rudel value and convert the
    /// result back into a Rudel value.
    pub(super) fn apply_value(&self, value: Value) -> Value {
        let fallback = value.clone();
        let call = self
            .vm
            .borrow_mut()
            .call_function(self.func.clone(), CallArgs::Single(value_to_koto(value)));
        match call {
            Ok(value) => koto_to_value(&value),
            Err(e) => {
                if self.err.borrow().is_none() {
                    *self.err.borrow_mut() = Some(e);
                }
                fallback
            }
        }
    }

    /// Surface the first callback error (if any) after the combinator has run.
    pub(super) fn finish(self) -> KotoResult<()> {
        match self.err.into_inner() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

pub(super) fn static_period_pattern(
    mut haps: Vec<rudel_core::Hap>,
    steps: Option<Frac>,
    period: Frac,
) -> Pattern {
    haps.sort_by_key(|h| h.part.begin);
    Pattern::new(move |state| {
        let mut out = Vec::new();
        let first_repeat = (state.span.begin / period).floor().numer() as i64;
        let last_repeat = (state.span.end / period).ceil().numer() as i64;
        for repeat in first_repeat..last_repeat {
            let offset = period * Frac::int(repeat);
            for template in &haps {
                let mut hap = template.with_span(|span| span.with_time(|t| t + offset));
                if let Some(part) = hap.part.intersection(&state.span) {
                    hap.part = part;
                    out.push(hap);
                }
            }
        }
        out
    })
    .set_steps(steps)
}

pub(super) fn with_callback(
    ctx: &MethodContext<KPattern>,
    callback_arg: usize,
    f: impl FnOnce(Pattern, &Callback) -> Pattern,
) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let cb = Callback::new(ctx, method_arg(ctx, callback_arg));
    let result = f(pat, &cb);
    cb.finish()?;
    Ok(KPattern::wrap(result))
}
