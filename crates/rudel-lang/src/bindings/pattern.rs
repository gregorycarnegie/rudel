// Several Koto methods are deliberately named in camelCase to match Strudel's
// public API exactly (e.g. `iterBack`, `euclidLegato`); the koto derive macro
// also generates `__koto_<name>` shims that inherit those names.
#![allow(non_snake_case)]

mod args;
mod callback;
mod convert;
mod generated;
mod methods;
mod pick;
mod repl;

use koto::derive::*;
use koto::prelude::*;
use koto::runtime::{KotoEntries, KotoObject};
use rudel_core::Pattern;

pub(crate) use callback::register_standalone_callbacks;
pub(crate) use convert::{arg_to_f64, arg_to_raw_str, arg0};
pub(super) use convert::{
    arg_to_group, arg_to_pattern, arg_to_pattern_weight, arg_to_value, arg_to_weighted_pair,
    koto_to_value,
};
pub(super) use pick::pick_args;
pub(crate) use repl::{collected_stack, reset_slots};

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
                    control_method_call(ctx, |pat, arg| pat.set(builder(arg)))
                })),
            );
        }
        // REPL pattern slots (`p`/`q`/`d1`/`p1`/`q1`) registered onto the same
        // shared entries map.
        repl::insert_slot_methods(&entries);
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
                    control_method_call(ctx, move |pat, arg| {
                        pat.set(rudel_core::control_dyn(key, arg))
                    })
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

/// Call a control body as a `KPattern` method: extract the instance and the
/// value argument the same way the generated `#[koto_method]` wrappers do.
fn control_method_call(
    ctx: &mut koto::runtime::CallContext,
    body: impl FnOnce(&Pattern, Pattern) -> Pattern,
) -> koto::runtime::Result<KValue> {
    use koto::runtime::{ErrorKind, MethodContext, runtime_error};
    match ctx.instance_and_args(|i| matches!(i, KValue::Object(_)), KPattern::type_static())? {
        (KValue::Object(o), extra_args) => {
            let mctx = MethodContext::new(o, extra_args, ctx.vm);
            args::with_pattern_arg(&mctx, body)
        }
        _ => runtime_error!(ErrorKind::UnexpectedError),
    }
}
