use super::KPattern;
use super::args::method_arg;
use super::convert::{arg_to_f64, arg_to_pattern, koto_to_value, value_to_koto};
use koto::prelude::*;
use koto::runtime::{Error as KotoError, Result as KotoResult};
use rudel_core::{Frac, Pattern, Value};
use std::cell::RefCell;

/// Register the standalone (curried-style) forms of the higher-order callback
/// combinators, taking the pattern last (`jux(rev, pat)`, `every(4, f, pat)`).
/// The transform argument must be a function value (`rev`, `|x| x.fast(2)`),
/// since Koto can't partially apply `fast(2)` into a function.
pub(crate) fn register_standalone_callbacks(prelude: &KMap) {
    // The pattern is the last arg and the transform function the one before it;
    // any leading args (count `n`, time `t`, bounds `a`/`b`) come first.
    fn func_and_pat(ctx: &CallContext) -> (KValue, Pattern) {
        let a = ctx.args();
        let func = a
            .len()
            .checked_sub(2)
            .and_then(|i| a.get(i))
            .cloned()
            .unwrap_or(KValue::Null);
        (func, arg_to_pattern(a.last().unwrap_or(&KValue::Null)))
    }
    // Leading arg `i` (before the function and pattern), or Null if absent.
    fn lead<'a>(ctx: &'a CallContext, i: usize) -> &'a KValue {
        let a = ctx.args();
        let present = a.len().checked_sub(2).is_some_and(|leading| i < leading);
        a.get(i).filter(|_| present).unwrap_or(&KValue::Null)
    }

    // Each macro registers a callback combinator group; `$name` is the
    // Strudel-facing name (snake or camelCase) and `$m` the core method.
    macro_rules! cb_only {
        ($($name:literal => $m:ident),* $(,)?) => {$(
            prelude.add_fn($name, |ctx| {
                let (func, pat) = func_and_pat(ctx);
                let cb = Callback::from_call_ctx(ctx, func);
                let out = pat.$m(|p| cb.apply(p));
                cb.finish()?;
                Ok(KPattern(out).into())
            });
        )*};
    }
    macro_rules! cb_i64 {
        ($($name:literal => $m:ident),* $(,)?) => {$(
            prelude.add_fn($name, |ctx| {
                let n = arg_to_f64(lead(ctx, 0)) as i64;
                let (func, pat) = func_and_pat(ctx);
                let cb = Callback::from_call_ctx(ctx, func);
                let out = pat.$m(n, |p| cb.apply(p));
                cb.finish()?;
                Ok(KPattern(out).into())
            });
        )*};
    }
    macro_rules! cb_f64 {
        ($($name:literal => $m:ident),* $(,)?) => {$(
            prelude.add_fn($name, |ctx| {
                let n = arg_to_f64(lead(ctx, 0));
                let (func, pat) = func_and_pat(ctx);
                let cb = Callback::from_call_ctx(ctx, func);
                let out = pat.$m(n, |p| cb.apply(p));
                cb.finish()?;
                Ok(KPattern(out).into())
            });
        )*};
    }
    macro_rules! cb_frac {
        ($($name:literal => $m:ident),* $(,)?) => {$(
            prelude.add_fn($name, |ctx| {
                let n = Frac::from_f64(arg_to_f64(lead(ctx, 0)));
                let (func, pat) = func_and_pat(ctx);
                let cb = Callback::from_call_ctx(ctx, func);
                let out = pat.$m(n, |p| cb.apply(p));
                cb.finish()?;
                Ok(KPattern(out).into())
            });
        )*};
    }
    macro_rules! cb_pat {
        ($($name:literal => $m:ident),* $(,)?) => {$(
            prelude.add_fn($name, |ctx| {
                let x = arg_to_pattern(lead(ctx, 0));
                let (func, pat) = func_and_pat(ctx);
                let cb = Callback::from_call_ctx(ctx, func);
                let out = pat.$m(x, |p| cb.apply(p));
                cb.finish()?;
                Ok(KPattern(out).into())
            });
        )*};
    }
    // `every`/`firstOf`/`lastOf` patternify their cycle count (the callback is
    // applied eagerly to the whole pattern, then placed by a patterned count).
    macro_rules! cb_cycles {
        ($($name:literal => $last:expr),* $(,)?) => {$(
            prelude.add_fn($name, |ctx| {
                let n = arg_to_pattern(lead(ctx, 0));
                let (func, pat) = func_and_pat(ctx);
                let cb = Callback::from_call_ctx(ctx, func);
                let transformed = cb.apply(&pat);
                cb.finish()?;
                Ok(KPattern(pat.every_pat(n, transformed, $last)).into())
            });
        )*};
    }
    macro_rules! cb_frac2 {
        ($($name:literal => $m:ident),* $(,)?) => {$(
            prelude.add_fn($name, |ctx| {
                let a = Frac::from_f64(arg_to_f64(lead(ctx, 0)));
                let b = Frac::from_f64(arg_to_f64(lead(ctx, 1)));
                let (func, pat) = func_and_pat(ctx);
                let cb = Callback::from_call_ctx(ctx, func);
                let out = pat.$m(a, b, |p| cb.apply(p));
                cb.finish()?;
                Ok(KPattern(out).into())
            });
        )*};
    }

    cb_only! {
        "superimpose" => superimpose, "jux" => jux,
        "sometimes" => sometimes, "often" => often, "rarely" => rarely,
        "almostAlways" => almost_always, "almost_always" => almost_always,
        "almostNever" => almost_never, "almost_never" => almost_never,
        "someCycles" => some_cycles, "some_cycles" => some_cycles,
        "apply" => apply, "always" => always, "never" => never,
    }
    cb_i64! {
        "chunk" => chunk,
        "chunkBack" => chunk_back, "chunk_back" => chunk_back,
    }
    cb_cycles! {
        "every" => false,
        "firstOf" => false, "first_of" => false,
        "lastOf" => true, "last_of" => true,
    }
    cb_f64! {
        "juxBy" => jux_by, "jux_by" => jux_by,
        "sometimesBy" => sometimes_by, "sometimes_by" => sometimes_by,
        "someCyclesBy" => some_cycles_by, "some_cycles_by" => some_cycles_by,
    }
    cb_frac! { "inside" => inside, "outside" => outside }
    cb_pat! { "off" => off, "when" => when }
    cb_frac2! { "within" => within }
}

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

    /// Like [`Callback::new`] but built from a free-function call context, for
    /// the standalone (curried-style) forms of the callback combinators.
    pub(crate) fn from_call_ctx(ctx: &CallContext, func: KValue) -> Self {
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
