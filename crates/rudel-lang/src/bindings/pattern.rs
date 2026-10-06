mod args;
mod callback;
mod convert;
mod engine;
mod generated;
mod methods;
mod modulate;
mod pick;
mod repl;

use crate::js::{self, Arg, Res, Scope};
use rudel_core::Pattern;
use std::{cell::RefCell, collections::HashSet};

pub(crate) use args::method;
pub(crate) use callback::register_standalone_callbacks;
pub(crate) use convert::{arg_to_f64, arg_to_pattern, arg_to_raw_str, arg0, fn_to_value};
pub(super) use convert::{
    arg_to_group, arg_to_pattern_weight, arg_to_value, arg_to_weighted_pair, to_value, value_to_arg,
};
pub(crate) use engine::{register_engine_fns, register_span_fns};
pub(crate) use methods::hap_to_filter_arg;
pub(in crate::bindings) use methods::{bjork_counts, euclid_call, stepwise_call};
pub(crate) use modulate::register_modulate_fns;
pub(super) use pick::pick_args;
pub(crate) use repl::{
    apply_pattern_transforms, insert_numbered_slots, push_all, register_slot, registered_slots,
    reset_slots, seed_slots, set_each,
};

/// Fill `Pattern.prototype`: the generated and bespoke methods first, then one
/// method per rudel-core control, driven by the `control_builders` registry
/// instead of hand-listed names. Names that already have a method (e.g.
/// `sound`, `i`, `freq`, `loop`) are left untouched, so the definitions above
/// always win over registry entries.
pub(crate) fn register_methods(proto: &Scope) {
    generated::register_generated(proto);
    engine::register_engine_methods(proto);
    for (name, builder) in rudel_core::control_builders() {
        if !proto.has(name) {
            method(proto, name, move |pat, a| {
                control_method_call(pat, a, &builder)
            });
        }
    }
    // REPL pattern slots (`p`/`q`/`d1`/`p1`/`q1`).
    repl::insert_slot_methods(proto);
    // Modulator builders (`modulate`/`lfo`/`env`/`bmod`), which take a config
    // object whose key order is significant.
    modulate::insert_modulate_methods(proto);
    // Numbered FM controls have no Rust builder fns; their names and canonical
    // keys are generated at runtime.
    for (name, key) in rudel_core::numbered_control_names() {
        if !proto.has(&name) {
            method(proto, &name, move |pat, a| {
                control_method_call(pat, a, &|arg| rudel_core::control_dyn(key.clone(), arg))
            });
        }
    }
}

/// The names of every method callable on a pattern (generated + bespoke +
/// registry-driven control methods), sorted. Drives the generated reference
/// surface so it can't drift from what is actually exposed.
pub(crate) fn method_names() -> Vec<String> {
    Scope::pattern()
        .names()
        .into_iter()
        .filter(|name| !name.starts_with("rudel_widget_") && name != "constructor")
        .collect()
}

/// Call a control as a pattern method.
///
/// With an argument this is `pat.set(builder(arg))`. With none the pattern's own
/// values become the control — Strudel's `createParam`
/// (`if (typeof value === 'undefined') return pat.fmap(withVal)`), and the
/// reason a tune can write `"0 2 4".note()` or `"bd sd".s()`. Setting from a
/// missing argument would set from silence.
///
/// Both paths go through the control's own builder rather than wrapping by
/// name, because only the builder knows a control that spreads over several
/// keys: `"bd:3".s()` has to set `s` *and* `n`, the way `s("bd:3")` does.
fn control_method_call(pat: &Pattern, a: &[Arg], builder: &dyn Fn(Pattern) -> Pattern) -> Res {
    Ok(match a.first() {
        None => builder(pat.clone()),
        Some(arg) => pat.set(builder(arg_to_pattern(arg))),
    }
    .into())
}

thread_local! {
    /// Names this evaluation bound through `register`, so a later registration
    /// can tell "mine" from a built-in it must not shadow.
    static REGISTERED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// Forget what the previous evaluation `register`ed.
pub(crate) fn reset_registered() {
    REGISTERED.with(|names| names.borrow_mut().clear());
}

/// Bind `name` as a pattern method that calls the script function `func` with
/// the method's own arguments followed by the pattern — Strudel's
/// `register(name, (...args, pat) => ...)` convention, where the pattern is
/// always last.
///
/// Songs in the wild lean on this heavily to define helpers (`split`, `gString`,
/// `ati`), so without it a script fails at its first line.
///
/// A **built-in method is never replaced.** Scripts in the wild register
/// polyfills for names Rudel already implements (`pickRestart` is the common
/// one, written when Strudel had not shipped it yet), and a polyfill written
/// against upstream's internals is a worse `pickRestart` than the real one.
/// Registering over an *earlier registration* is still allowed, so a script
/// that defines the same helper twice gets the second.
///
/// `patternify` is `register`'s third argument: a helper that says `false`
/// does its own `reify`/join on its arguments and wants them whole.
///
/// ponytail: no arity or type checking — the call reports its own errors.
pub(crate) fn register_pattern_method(name: &str, func: &Arg, patternify: bool) {
    let proto = Scope::pattern();
    let mine = REGISTERED.with(|names| names.borrow().contains(name));
    if !mine && proto.has(name) {
        return;
    }
    REGISTERED.with(|names| names.borrow_mut().insert(name.to_string()));
    proto.value(
        name,
        js::method_calling(Arg::native(registered_call), func, patternify),
    );
}

/// What a `register`ed method runs: `(fn, patternify, args, pattern)`.
fn registered_call(a: &[Arg]) -> Res {
    let func = args::arg(a, 0);
    let patternify = matches!(args::arg(a, 1), Arg::Bool(true));
    let mut call_args = match args::arg(a, 2) {
        Arg::List(extra) => extra.clone(),
        _ => Vec::new(),
    };
    call_args.push(args::arg(a, 3).clone());
    // Upstream's `register` patternifies its arguments: a pattern passed where a
    // value is expected is sampled per cycle rather than handed to the
    // callback whole (`arg.fmap(v => fn(v, pat)).innerJoin()`). Except for its
    // pure fast path, which hands the leading arguments over as plain values
    // when every one of them is a `pure` — so what decides is whether the
    // argument *has structure*, not whether it was written as a mini literal.
    // `m("c3")` is one steady value and stays a value; `m("<c3 e3>")` is a
    // cycle-alternation and gets sampled, which is what a helper doing
    // `noteToMidi(arg)` needs.
    let patterned = patternify
        .then(|| {
            call_args[..call_args.len() - 1]
                .iter()
                .position(|arg| matches!(arg, Arg::Pat(p) if p.pure_value.is_none()))
        })
        .flatten();
    let Some(at) = patterned else {
        return js::call(func, call_args);
    };
    let Arg::Pat(sampled) = call_args[at].clone() else {
        return js::call(func, call_args);
    };
    // Called per sampled value when a query first meets it, with the other
    // arguments fixed as they were passed.
    let bound = js::bind_at(func, call_args, at)?;
    let callback = callback::Deferred::require("register", &bound)?;
    Ok(callback::patternify_deferred(sampled, callback, |value, cb| cb.pattern_of(value)).into())
}
