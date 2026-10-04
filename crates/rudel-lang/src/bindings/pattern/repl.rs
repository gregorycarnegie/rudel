// repl.rs - REPL pattern slots (`p`/`d1`/`p1`/`q`), ported from the
// user-visible parts of strudel/packages/core/repl.mjs. `p(id)` registers a
// pattern into a per-evaluation registry; the evaluator stacks the registered
// patterns, mirroring Strudel's `pPatterns` + `applyPatternTransforms`.
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    args::{arg, method},
    callback::Callback,
    convert::to_value,
};
use crate::js::{Arg, Scope};
use rudel_core::{Pattern, Value, pure, silence, stack};
use std::cell::{Cell, RefCell};

thread_local! {
    /// Patterns registered via `p`/`d1`/`p1`/`$:` during the current evaluation,
    /// in registration order, each with its slot `key`. Reset per eval, like
    /// Strudel's `pPatterns`. The key drives solo detection (an `S`-prefixed key
    /// longer than one char solos, mirroring `S$:`).
    static P_SLOTS: RefCell<Vec<(String, Pattern)>> = const { RefCell::new(Vec::new()) };
    /// The keys this evaluation registered itself, as opposed to the other
    /// blocks' patterns a block evaluation was seeded with.
    static FRESH: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
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
    FRESH.with(|f| f.borrow_mut().clear());
    ANON.with(|a| a.set(0));
    EACH.with(|e| *e.borrow_mut() = None);
    ALL.with(|a| a.borrow_mut().clear());
}

/// Start the registry from the other blocks' patterns (a block evaluation).
pub(crate) fn seed_slots(slots: Vec<(String, Pattern)>) {
    P_SLOTS.with(|s| *s.borrow_mut() = slots);
}

/// The registry as it stands, and which of its keys this evaluation
/// registered.
pub(crate) fn registered_slots() -> (Vec<(String, Pattern)>, Vec<String>) {
    (
        P_SLOTS.with(|s| s.borrow().clone()),
        FRESH.with(|f| f.borrow().clone()),
    )
}

/// Store the `each(f)` transform (the last call wins, matching Strudel).
pub(crate) fn set_each(func: Arg) {
    EACH.with(|e| *e.borrow_mut() = Some(Callback::new(func)));
}

/// Append an `all(f)` transform (applied in registration order).
pub(crate) fn push_all(func: Arg) {
    ALL.with(|a| a.borrow_mut().push(Callback::new(func)));
}

/// Combine the evaluated patterns the way Strudel's `applyPatternTransforms`
/// does: when slots/labels were registered, stack them (honouring `S`-prefixed
/// soloing and the per-pattern `each` transform); otherwise fall back to the
/// script's own `pattern`, still applying `each`. Finally run every `all`
/// transform over the result. Returns `None` only when there is nothing to play
/// (no slots, no script pattern, no transforms) so the caller can report the
/// "script did not return a pattern" error.
///
/// The transforms are script functions, so this runs with the evaluation's
/// context lent.
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
/// solo detection. A key registered again keeps its place and takes the new
/// pattern, as upstream's `pPatterns[id] = this`.
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
    P_SLOTS.with(|s| {
        let mut slots = s.borrow_mut();
        match slots.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = tagged.clone(),
            None => slots.push((key.clone(), tagged.clone())),
        }
    });
    FRESH.with(|f| f.borrow_mut().push(key));
    tagged
}

/// Turn a slot-id argument into its registry key: numbers render without a
/// decimal point (`1` -> `"1"`), strings pass through.
fn slot_id_string(value: &Arg) -> String {
    match to_value(value) {
        Value::Str(s) => s,
        Value::Int(n) => n.to_string(),
        Value::F64(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.as_f64().map(|n| n.to_string()).unwrap_or_default(),
    }
}

/// Put the REPL slot methods `p` and `q` on `Pattern.prototype`, alongside
/// the control methods. The numbered `d1`/`p1`/`q1` slots are properties,
/// added last by [`insert_numbered_slots`].
pub(crate) fn insert_slot_methods(proto: &Scope) {
    // p(id): register under the given id.
    method(proto, "p", |pat, a| {
        Ok(register_slot(&slot_id_string(arg(a, 0)), pat.clone()).into())
    });
    // q(id): a silent (queued/muted) slot.
    method(proto, "q", |_, _| Ok(silence().into()));
}

/// The numbered slots upstream's REPL puts on patterns (repl.mjs): `pat.d1`
/// and `pat.p1` are getters that register the pattern as slot 1, and `pat.q1`
/// is silence. Defined after every method, over any of the same name, as
/// upstream's are.
pub(crate) fn insert_numbered_slots(proto: &Scope) {
    for i in 1..10 {
        let id = slot_id_string(&Arg::Num(f64::from(i)));
        for name in [format!("d{i}"), format!("p{i}")] {
            let (id, label) = (id.clone(), name.clone());
            proto.getter(&name, move |this| match this {
                Arg::Pat(pat) => Ok(register_slot(&id, pat.clone()).into()),
                _ => Err(format!("{label}: not read from a pattern")),
            });
        }
        proto.value(&format!("q{i}"), silence());
    }
}
