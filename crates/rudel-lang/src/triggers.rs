// triggers.rs - `onTriggerTime`: user callbacks fired as events play.
//
// Every other callback in Rudel is applied eagerly at build time, which is
// cheaper and keeps a callback's errors attached to the evaluation that made
// them. `onTriggerTime` is the one that genuinely has to run *later*: it exists
// to make something happen at event time. So the function is kept past `eval`
// (see `js::SendFn`), the haps are tagged with the hook id, and the host fires
// the callbacks from its frame loop as the playhead passes each event.
//
// Upstream (`core/pattern.mjs`) implements this with `onTrigger` plus a
// `window.setTimeout`, and its own docs call that "innacurate for audio tasks".
// Rudel's frame-loop firing has the same character: it is for driving UI and
// side effects, not for sample-accurate audio.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::js::{self, Arg, Res, SendFn, Session};
use rudel_core::{Hap, Pattern, Value};
use std::{cell::RefCell, collections::HashMap, sync::Arc};

/// The control an `onTriggerTime`-tagged hap carries: the id of the callback to
/// fire. The scheduler's event extraction strips it, like `rudel_core::LOG_KEY`.
pub use rudel_core::TRIGGER_KEY;

thread_local! {
    /// Callbacks registered by the evaluation currently running, keyed by id.
    /// Drained into a [`TriggerHooks`] when the evaluation finishes.
    static PENDING: RefCell<Vec<SendFn>> = const { RefCell::new(Vec::new()) };
}

/// Forget any callbacks a previous evaluation left behind. Called at the start
/// of every evaluation, next to `reset_slots`.
pub(crate) fn reset_hooks() {
    PENDING.with(|p| p.borrow_mut().clear());
}

/// `pat.onTriggerTime(f)`: keep `f` and tag the pattern with its id.
pub(crate) fn kpattern_on_trigger_time(pat: &Pattern, a: &[Arg]) -> Res {
    let Some(func) = a.first().and_then(js::keep) else {
        return Err("onTriggerTime: expected a function".to_string());
    };
    let id = PENDING.with(|p| {
        let mut p = p.borrow_mut();
        p.push(func);
        p.len() as i64 - 1
    });
    Ok(pat
        .ctrl(TRIGGER_KEY, rudel_core::pure(Value::Int(id)))
        .into())
}

/// The callbacks an evaluation registered. Holding one keeps the evaluation's
/// engine alive to run them.
#[derive(Default)]
pub struct TriggerHooks {
    hooks: HashMap<i64, SendFn>,
    _session: Option<Arc<Session>>,
}

impl TriggerHooks {
    /// Take whatever the just-finished evaluation registered.
    pub(crate) fn take(session: Option<Arc<Session>>) -> TriggerHooks {
        let hooks: HashMap<i64, SendFn> = PENDING.with(|p| {
            p.borrow_mut()
                .drain(..)
                .enumerate()
                .map(|(i, f)| (i as i64, f))
                .collect()
        });
        TriggerHooks {
            _session: (!hooks.is_empty()).then_some(session).flatten(),
            hooks,
        }
    }

    /// True when no `onTriggerTime` callback was registered, so the host can
    /// skip its per-frame scan entirely.
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// Fire the callback `hap` is tagged for, passing the hap as the object a
    /// `filter` predicate sees. Returns the callback's error message, if it
    /// raised one, so the host can surface it.
    pub fn fire(&mut self, hap: &Hap) -> Option<String> {
        let id = trigger_id(&hap.value)?;
        let func = *self.hooks.get(&id)?;
        let hap = hap.clone();
        func.run(move |f| js::call(f, vec![crate::bindings::hap_to_filter_arg(&hap)]).err())
            .flatten()
    }
}

/// The hook id a hap carries, if it is `onTriggerTime`-tagged.
pub fn trigger_id(value: &Value) -> Option<i64> {
    match value {
        Value::Map(m) => m.get(TRIGGER_KEY).and_then(Value::as_f64).map(|n| n as i64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use rudel_core::Frac;

    #[test]
    fn the_hooks_keep_the_engine_alive_and_dropping_them_releases_it() {
        let result =
            crate::eval_result(r#"s("bd").onTriggerTime(hap => { throw 'boom' })"#).expect("eval");
        let mut hooks = result.trigger_hooks;
        let hap = result
            .pattern
            .query_arc(Frac::zero(), Frac::one())
            .remove(0);
        // The pattern goes first: the hooks alone must keep the evaluation's
        // engine parked, or the host's later firing finds nothing to call.
        drop(result.pattern);
        let err = hooks.fire(&hap).expect("the callback still runs");
        assert!(err.contains("boom"), "{err}");
        // Once nothing holds the engine it is released, and the function with it.
        let func = *hooks.hooks.values().next().expect("one hook");
        drop(hooks);
        assert!(
            func.run(|_| ()).is_none(),
            "the engine outlived its last holder"
        );
    }
}
