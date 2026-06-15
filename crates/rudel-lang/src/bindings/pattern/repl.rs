// repl.rs - REPL pattern slots (`p`/`d1`/`p1`/`q`), ported from the
// user-visible parts of strudel/packages/core/repl.mjs. `p(id)` registers a
// pattern into a per-evaluation registry; the evaluator stacks the registered
// patterns, mirroring Strudel's `pPatterns` + `applyPatternTransforms`.
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::KPattern;
use super::args::{method_arg, with_instance};
use super::convert::koto_to_value;
use koto::prelude::*;
use koto::runtime::{CallContext, ErrorKind, MethodContext, Result as KotoResult, runtime_error};
use rudel_core::{Pattern, Value, pure, silence, stack};
use std::cell::{Cell, RefCell};

thread_local! {
    /// Patterns registered via `p`/`d1`/`p1` during the current evaluation, in
    /// registration order. Reset per eval, like Strudel's `pPatterns`.
    static P_SLOTS: RefCell<Vec<Pattern>> = const { RefCell::new(Vec::new()) };
    /// Counter for anonymous (`$`) slots, matching Strudel's `anonymousIndex`.
    static ANON: Cell<usize> = const { Cell::new(0) };
}

/// Clear the slot registry. Called at the start of every evaluation (and by
/// `hush`), so slots from a previous eval don't leak into the next.
pub(crate) fn reset_slots() {
    P_SLOTS.with(|s| s.borrow_mut().clear());
    ANON.with(|a| a.set(0));
}

/// The stack of all registered slots, or `None` if none were registered (in
/// which case the evaluator keeps the script's own return value). Mirrors
/// `applyPatternTransforms`: when any slot is set, the result is their stack.
pub(crate) fn collected_stack() -> Option<Pattern> {
    P_SLOTS.with(|s| {
        let slots = s.borrow();
        (!slots.is_empty()).then(|| stack(&slots))
    })
}

/// Register `pat` under slot `id` (`Pattern.prototype.p`). A `_x`/`x_` id mutes
/// (returns silence without registering); a `$` id gets a per-eval anonymous
/// suffix. The pattern is tagged with its id (like Strudel's
/// `withState(setControls({id}))`) and recorded for stacking.
fn register_slot(id: &str, pat: Pattern) -> Pattern {
    if id.starts_with('_') || id.ends_with('_') {
        return silence();
    }
    let key = if id.contains('$') {
        let n = ANON.with(|a| {
            let v = a.get();
            a.set(v + 1);
            v
        });
        format!("{id}{n}")
    } else {
        id.to_string()
    };
    let tagged = pat.ctrl("id", pure(Value::Str(key)));
    P_SLOTS.with(|s| s.borrow_mut().push(tagged.clone()));
    tagged
}

/// Turn a slot-id argument into its registry key: numbers render without a
/// decimal point (`1` -> `"1"`), strings pass through.
fn slot_id_string(value: &KValue) -> String {
    match koto_to_value(value) {
        Value::Str(s) => s,
        Value::Int(n) => n.to_string(),
        Value::F64(n) if n.fract() == 0.0 => (n as i64).to_string(),
        Value::F64(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.as_f64().map(|n| n.to_string()).unwrap_or_default(),
    }
}

/// Run `body` with the method's instance pattern (the koto-method call shape
/// used by the dynamically-inserted slot methods).
fn with_slot_instance(
    ctx: &mut CallContext,
    body: impl FnOnce(&MethodContext<KPattern>, &Pattern) -> Pattern,
) -> KotoResult<KValue> {
    match ctx.instance_and_args(|i| matches!(i, KValue::Object(_)), KPattern::type_static())? {
        (KValue::Object(o), extra_args) => {
            let mctx = MethodContext::new(o, extra_args, ctx.vm);
            with_instance(&mctx, |pat| body(&mctx, pat))
        }
        _ => runtime_error!(ErrorKind::UnexpectedError),
    }
}

/// Insert the REPL slot methods (`p`, `q`, `d1`-`d9`, `p1`-`p9`, `q1`-`q9`)
/// onto the shared `KPattern` entries map, alongside the control methods.
pub(crate) fn insert_slot_methods(entries: &koto::runtime::KMap) {
    // p(id): register under the given id.
    entries.insert(
        "p",
        KValue::NativeFunction(KNativeFunction::new(|ctx| {
            with_slot_instance(ctx, |mctx, pat| {
                let id = slot_id_string(&method_arg(mctx, 0));
                register_slot(&id, pat.clone())
            })
        })),
    );
    // q(id): a silent (queued/muted) slot.
    entries.insert(
        "q",
        KValue::NativeFunction(KNativeFunction::new(|ctx| {
            with_slot_instance(ctx, |_, _| silence())
        })),
    );
    for i in 1..=9 {
        // d<i> and p<i> are fixed-id slots: shorthand for p(i).
        for prefix in ["d", "p"] {
            let id = i.to_string();
            entries.insert(
                format!("{prefix}{i}").as_str(),
                KValue::NativeFunction(KNativeFunction::new(move |ctx| {
                    let id = id.clone();
                    with_slot_instance(ctx, move |_, pat| register_slot(&id, pat.clone()))
                })),
            );
        }
        // q<i>: a silent slot.
        entries.insert(
            format!("q{i}").as_str(),
            KValue::NativeFunction(KNativeFunction::new(|ctx| {
                with_slot_instance(ctx, |_, _| silence())
            })),
        );
    }
}
