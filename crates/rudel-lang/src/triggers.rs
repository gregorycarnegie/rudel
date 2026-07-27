// triggers.rs - `onTriggerTime`: user callbacks fired as events play.
//
// Every other Koto callback in Rudel is applied eagerly at build time, because
// the Koto VM is not `Send` and the audio/query path is. `onTriggerTime` is the
// one that genuinely has to run *later*: it exists to make something happen at
// event time. So the evaluation's VM is kept alive past `eval` inside a
// [`TriggerHooks`], the haps are tagged with the hook id, and the host — which
// owns the VM's thread — fires the callbacks from its frame loop as the
// playhead passes each event.
//
// Upstream (`core/pattern.mjs`) implements this with `onTrigger` plus a
// `window.setTimeout`, and its own docs call that "innacurate for audio tasks".
// Rudel's frame-loop firing has the same character: it is for driving UI and
// side effects, not for sample-accurate audio.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::bindings::KPattern;
use koto::{prelude::*, runtime::Result as KotoResult};
use rudel_core::{Hap, Value};
use std::{cell::RefCell, collections::HashMap};

/// The control an `onTriggerTime`-tagged hap carries: the id of the callback to
/// fire. The scheduler's event extraction strips it, like `rudel_core::LOG_KEY`.
pub use rudel_core::TRIGGER_KEY;

thread_local! {
    /// Callbacks registered by the evaluation currently running, keyed by id.
    /// Drained into a [`TriggerHooks`] when the evaluation finishes.
    static PENDING: RefCell<Vec<KValue>> = const { RefCell::new(Vec::new()) };
}

/// Forget any callbacks a previous evaluation left behind. Called at the start
/// of every evaluation, next to `reset_slots`.
pub(crate) fn reset_hooks() {
    PENDING.with(|p| p.borrow_mut().clear());
}

/// `pat.onTriggerTime(f)`: register `f` and tag the pattern with its id.
pub(crate) fn kpattern_on_trigger_time(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let func = ctx.args.first().cloned().unwrap_or(KValue::Null);
    let id = PENDING.with(|p| {
        let mut p = p.borrow_mut();
        p.push(func);
        p.len() as i64 - 1
    });
    let pat = ctx.instance()?.0.clone();
    Ok(KPattern(pat.ctrl(TRIGGER_KEY, rudel_core::pure(Value::Int(id)))).into())
}

/// The callbacks an evaluation registered, together with the VM that can run
/// them. Not `Send` — it lives on the thread that evaluated the script, which
/// is the same thread the host's frame loop runs on.
#[derive(Default)]
pub struct TriggerHooks {
    /// `None` when the script registered no hooks, so the common case drops
    /// the interpreter at the end of evaluation as it always did.
    koto: Option<Koto>,
    hooks: HashMap<i64, KValue>,
}

impl TriggerHooks {
    /// Take whatever the just-finished evaluation registered.
    pub(crate) fn take(koto: Koto) -> TriggerHooks {
        let hooks: HashMap<i64, KValue> = PENDING.with(|p| {
            p.borrow_mut()
                .drain(..)
                .enumerate()
                .map(|(i, f)| (i as i64, f))
                .collect()
        });
        TriggerHooks {
            koto: (!hooks.is_empty()).then_some(koto),
            hooks,
        }
    }

    /// True when no `onTriggerTime` callback was registered, so the host can
    /// skip its per-frame scan entirely.
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// Fire the callback `hap` is tagged for, passing the hap as a map (the
    /// same shape `filter` sees). Returns the callback's error message, if it
    /// raised one, so the host can surface it.
    pub fn fire(&mut self, hap: &Hap) -> Option<String> {
        let id = trigger_id(&hap.value)?;
        let func = self.hooks.get(&id)?.clone();
        let koto = self.koto.as_mut()?;
        let arg = crate::bindings::hap_to_koto(hap);
        koto.call_function(func, CallArgs::Single(arg))
            .err()
            .map(|e| e.to_string())
    }
}

/// The hook id a hap carries, if it is `onTriggerTime`-tagged.
pub fn trigger_id(value: &Value) -> Option<i64> {
    match value {
        Value::Map(m) => m.get(TRIGGER_KEY).and_then(Value::as_f64).map(|n| n as i64),
        _ => None,
    }
}
