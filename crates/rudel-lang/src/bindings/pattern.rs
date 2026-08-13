// Several Koto methods are deliberately named in camelCase to match Strudel's
// public API exactly (e.g. `iterBack`, `euclidLegato`); the koto derive macro
// also generates `__koto_<name>` shims that inherit those names.
#![allow(non_snake_case)]

mod args;
mod callback;
mod convert;
mod engine;
mod generated;
mod methods;
mod modulate;
mod pick;
mod repl;

use koto::{
    derive::*,
    prelude::*,
    runtime::{KotoEntries, KotoObject},
};
use rudel_core::Pattern;
use std::collections::HashSet;

pub(crate) use callback::register_standalone_callbacks;
pub(crate) use convert::{arg_to_f64, arg_to_pattern, arg_to_raw_str, arg0};
pub(super) use convert::{
    arg_to_group, arg_to_pattern_weight, arg_to_value, arg_to_weighted_pair, koto_to_value,
};
pub(crate) use engine::{register_engine_fns, register_span_fns};
pub(crate) use methods::hap_to_koto;
pub(in crate::bindings) use methods::{euclid_call, stepwise_call};
pub(crate) use modulate::register_modulate_fns;
pub(super) use pick::pick_args;
pub(crate) use repl::{apply_pattern_transforms, push_all, register_slot, reset_slots, set_each};

/// A Koto wrapper around a rudel [`Pattern`].
#[derive(Clone, KotoCopy, KotoType)]
pub struct KPattern(pub Pattern);

impl KotoObject for KPattern {}

impl From<KPattern> for KValue {
    fn from(p: KPattern) -> KValue {
        KObject::from(p).into()
    }
}

impl KPattern {
    fn wrap(pat: Pattern) -> KValue {
        KPattern(pat).into()
    }
}

/// Expose every rudel-core control as a `KPattern` method, driven by the
/// `control_builders` registry instead of hand-listed method names.
///
/// The `#[koto_impl]`-generated entries map is a cheap shared handle to a
/// cached map, so inserting here makes the methods visible to every
/// interpreter on this thread. Under koto's default `rc` feature that cache
/// is `thread_local!`, so the extension runs once per thread (not per
/// process). Names that already have generated or bespoke methods (e.g.
/// `sound`, `i`, `freq`, `loop`) are left untouched, so static definitions
/// always win over registry entries.
pub(crate) fn extend_control_entries() {
    use std::cell::Cell;
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|done| done.replace(true)) {
        return;
    }
    {
        let Some(entries) = KPattern(rudel_core::silence()).entries() else {
            return;
        };
        for (name, builder) in rudel_core::control_builders() {
            if entries.get(name).is_some() {
                continue;
            }
            entries.insert(
                name,
                KValue::NativeFunction(KNativeFunction::new(move |ctx| {
                    control_method_call(ctx, builder)
                })),
            );
        }
        // REPL pattern slots (`p`/`q`/`d1`/`p1`/`q1`) registered onto the same
        // shared entries map.
        repl::insert_slot_methods(&entries);
        // Modulator builders (`modulate`/`lfo`/`env`/`bmod`), which take a
        // config map whose key order is significant.
        modulate::insert_modulate_methods(&entries);
        // Numbered FM controls have no Rust builder fns; their names and
        // canonical keys are generated at runtime.
        for (name, key) in rudel_core::numbered_control_names() {
            if entries.get(name.as_str()).is_some() {
                continue;
            }
            entries.insert(
                name.as_str(),
                KValue::NativeFunction(KNativeFunction::new(move |ctx| {
                    let key = key.clone();
                    control_method_call(ctx, move |arg| rudel_core::control_dyn(key.clone(), arg))
                })),
            );
        }
    }
}

/// The names of every method callable on a pattern (generated + bespoke +
/// registry-driven control methods), sorted. Drives the generated reference
/// surface so it can't drift from what is actually exposed.
pub(crate) fn method_names() -> Vec<String> {
    extend_control_entries();
    let Some(entries) = KPattern(rudel_core::silence()).entries() else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .data()
        .iter()
        .filter_map(|(key, _)| match key.value() {
            KValue::Str(s) if !s.starts_with("rudel_widget_") => Some(s.to_string()),
            _ => None,
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Call a control as a `KPattern` method: extract the instance and the value
/// argument the same way the generated `#[koto_method]` wrappers do.
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
fn control_method_call(
    ctx: &mut koto::runtime::CallContext,
    builder: impl Fn(Pattern) -> Pattern,
) -> koto::runtime::Result<KValue> {
    use koto::runtime::{ErrorKind, MethodContext, runtime_error};
    match ctx.instance_and_args(|i| matches!(i, KValue::Object(_)), KPattern::type_static())? {
        (KValue::Object(o), extra_args) => {
            let bare = extra_args.is_empty();
            let mctx = MethodContext::new(o, extra_args, ctx.vm);
            if bare {
                args::with_instance(&mctx, |pat| builder(pat.clone()))
            } else {
                args::with_pattern_arg(&mctx, |pat, arg| pat.set(builder(arg)))
            }
        }
        _ => runtime_error!(ErrorKind::UnexpectedError),
    }
}

/// Bind `name` as a pattern method that calls the Koto function `func` with the
/// method's own arguments followed by the pattern — Strudel's
/// `register(name, (...args, pat) => ...)` convention, where the pattern is
/// always last.
///
/// Songs in the wild lean on this heavily to define helpers (`split`, `gString`,
/// `ati`), so without it a script fails at its first line. The entry goes into
/// the same shared method map the controls use, which means a registration
/// outlives the evaluation that made it — as it does in Strudel, where the
/// method is patched onto `Pattern.prototype`.
///
/// A **built-in method is never replaced.** Scripts in the wild register
/// polyfills for names Rudel already implements (`pickRestart` is the common
/// one, written when Strudel had not shipped it yet), and because the method map
/// outlives the evaluation, one such script would hand its polyfill to every
/// later script in the session — which is exactly what happened when a corpus
/// was run in one process: a song that passed alone failed after another had
/// been evaluated. Registering over an *earlier registration* is still allowed,
/// so re-evaluating a script that defines its own helper picks up the edit.
///
/// ponytail: no arity or type checking — the Koto call reports its own errors.
pub(crate) fn register_pattern_method(name: &str, func: KValue) {
    register_method(name, func, true)
}

/// Bind `name` the way `Pattern.prototype.name = function …` does: the receiver
/// is still the trailing argument, but the arguments are *not* patternified.
///
/// A prototype method upstream gets none of `register`'s wrapping, and a
/// combinator wants the argument pattern whole — `warp(tpat)` reads `tpat`'s
/// haps to number them, which sampling it per cycle would make impossible.
pub(crate) fn register_prototype_method(name: &str, func: KValue) {
    register_method(name, func, false)
}

fn register_method(name: &str, func: KValue, patternify: bool) {
    use std::cell::RefCell;
    thread_local! {
        /// Names this thread bound through `register`, so a later registration
        /// can tell "mine" from a built-in it must not shadow.
        static REGISTERED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    }
    let Some(entries) = KPattern(rudel_core::silence()).entries() else {
        return;
    };
    let mine = REGISTERED.with(|names| names.borrow().contains(name));
    if !mine && entries.get(name).is_some() {
        return;
    }
    REGISTERED.with(|names| names.borrow_mut().insert(name.to_string()));
    entries.insert(
        name,
        KValue::NativeFunction(KNativeFunction::new(move |ctx: &mut CallContext| {
            let (instance, extra) =
                ctx.instance_and_args(|i| matches!(i, KValue::Object(_)), KPattern::type_static())?;
            let mut args: Vec<KValue> = extra.to_vec();
            args.push(instance.clone());
            let vm = std::cell::RefCell::new(ctx.vm.spawn_shared_vm());
            let call = |args: &[KValue]| {
                vm.borrow_mut()
                    .call_function(func.clone(), CallArgs::Separate(args))
            };
            // Upstream's `register` patternifies its arguments: a pattern passed
            // where a value is expected is sampled per cycle rather than handed
            // to the callback whole (`arg.fmap(v => fn(v, pat)).innerJoin()`).
            // A mini-notation literal carries its own source text and is a value
            // here, as it is everywhere else in the bindings, so only a real
            // pattern expression triggers this.
            let patterned = patternify
                .then(|| {
                    args[..args.len() - 1].iter().position(|arg| {
                        matches!(arg, KValue::Object(o) if o.is_a::<KPattern>()
                            && o.cast::<KPattern>().is_ok_and(|p| p.0.source.is_none()))
                    })
                })
                .flatten();
            let Some(at) = patterned else {
                return call(&args);
            };
            let arg = args[at].clone();
            let KValue::Object(object) = &arg else {
                return call(&args);
            };
            let sampled = object.cast::<KPattern>()?.0.clone();
            Ok(KPattern(callback::probe_patternify(sampled, |value| {
                let mut per_value = args.clone();
                per_value[at] = convert::value_to_koto(value.clone());
                match call(&per_value) {
                    Ok(KValue::Object(o)) if o.is_a::<KPattern>() => {
                        o.cast::<KPattern>().unwrap().0.clone()
                    }
                    _ => rudel_core::silence(),
                }
            }))
            .into())
        })),
    );
}
