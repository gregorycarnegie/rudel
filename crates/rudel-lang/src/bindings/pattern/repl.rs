// repl.rs - REPL pattern slots (`p`/`d1`/`p1`/`q`), ported from the
// user-visible parts of strudel/packages/core/repl.mjs. `p(id)` registers a
// pattern into a per-evaluation registry; the evaluator stacks the registered
// patterns, mirroring Strudel's `pPatterns` + `applyPatternTransforms`.
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::KPattern;
use super::args::{method_arg, with_instance};
use super::callback::Callback;
use super::convert::koto_to_value;
use koto::prelude::*;
use koto::runtime::{CallContext, ErrorKind, MethodContext, Result as KotoResult, runtime_error};
use rudel_core::{Pattern, Value, pure, silence, stack};
use std::cell::{Cell, RefCell};

thread_local! {
    /// Patterns registered via `p`/`d1`/`p1`/`$:` during the current evaluation,
    /// in registration order, each with its slot `key`. Reset per eval, like
    /// Strudel's `pPatterns`. The key drives solo detection (an `S`-prefixed key
    /// longer than one char solos, mirroring `S$:`).
    static P_SLOTS: RefCell<Vec<(String, Pattern)>> = const { RefCell::new(Vec::new()) };
    /// Counter for anonymous (`$`) slots, matching Strudel's `anonymousIndex`.
    static ANON: Cell<usize> = const { Cell::new(0) };
    /// Transform set by `each(f)`: applied to every registered pattern (or the
    /// script's own pattern when none are registered). Strudel's `eachTransform`.
    static EACH: RefCell<Option<Callback>> = const { RefCell::new(None) };
    /// Transforms pushed by `all(f)`: applied in order to the final stacked
    /// pattern. Strudel's `allTransforms`.
    static ALL: RefCell<Vec<Callback>> = const { RefCell::new(Vec::new()) };
}

/// Clear the slot registry and the `each`/`all` transforms. Called at the start
/// of every evaluation (and by `hush`), so state from a previous eval doesn't
/// leak into the next. Mirrors `hush()` in `core/repl.mjs`.
pub(crate) fn reset_slots() {
    P_SLOTS.with(|s| s.borrow_mut().clear());
    ANON.with(|a| a.set(0));
    EACH.with(|e| *e.borrow_mut() = None);
    ALL.with(|a| a.borrow_mut().clear());
}

/// Store the `each(f)` transform (the last call wins, matching Strudel).
pub(crate) fn set_each(ctx: &CallContext, func: KValue) {
    let cb = Callback::from_call_ctx(ctx, func);
    EACH.with(|e| *e.borrow_mut() = Some(cb));
}

/// Append an `all(f)` transform (applied in registration order).
pub(crate) fn push_all(ctx: &CallContext, func: KValue) {
    let cb = Callback::from_call_ctx(ctx, func);
    ALL.with(|a| a.borrow_mut().push(cb));
}

/// Combine the evaluated patterns the way Strudel's `applyPatternTransforms`
/// does: when slots/labels were registered, stack them (honouring `S`-prefixed
/// soloing and the per-pattern `each` transform); otherwise fall back to the
/// script's own `pattern`, still applying `each`. Finally run every `all`
/// transform over the result. Returns `None` only when there is nothing to play
/// (no slots, no script pattern, no transforms) so the caller can report the
/// "script did not return a pattern" error.
pub(crate) fn apply_pattern_transforms(script: Option<Pattern>) -> Option<Pattern> {
    let slots = P_SLOTS.with(|s| s.borrow().clone());

    let mut pattern = if !slots.is_empty() {
        // Soloing: once an `S`-prefixed key (longer than one char) appears, drop
        // every previously collected pattern and keep only soloed ones.
        let mut patterns: Vec<Pattern> = Vec::new();
        let mut solo_active = false;
        for (key, pat) in &slots {
            let is_solod = key.len() > 1 && key.starts_with('S');
            if is_solod && !solo_active {
                patterns.clear();
                solo_active = true;
            }
            if !solo_active || is_solod {
                patterns.push(pat.clone());
            }
        }
        EACH.with(|e| {
            if let Some(cb) = e.borrow().as_ref() {
                patterns = patterns.iter().map(|p| cb.apply(p)).collect();
            }
        });
        stack(&patterns)
    } else {
        match script {
            Some(p) => EACH.with(|e| match e.borrow().as_ref() {
                Some(cb) => cb.apply(&p),
                None => p,
            }),
            // No slots and no script pattern: only meaningful if `all` was used
            // (it then transforms silence); otherwise there is nothing to play.
            None if ALL.with(|a| a.borrow().is_empty()) => return None,
            None => silence(),
        }
    };

    ALL.with(|a| {
        for cb in a.borrow().iter() {
            pattern = cb.apply(&pattern);
        }
    });
    Some(pattern)
}

/// Register `pat` under slot `id` (`Pattern.prototype.p` and the `$:` labels). A
/// `_x`/`x_` id mutes (returns silence without registering); a `$` id gets a
/// per-eval anonymous suffix. The pattern is tagged with its id (like Strudel's
/// `withState(setControls({id}))`) and recorded with its key for stacking and
/// solo detection.
pub(crate) fn register_slot(id: &str, pat: Pattern) -> Pattern {
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
    let tagged = pat.ctrl("id", pure(Value::Str(key.clone())));
    P_SLOTS.with(|s| s.borrow_mut().push((key, tagged.clone())));
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
