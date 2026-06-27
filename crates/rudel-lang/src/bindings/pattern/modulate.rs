// modulate.rs - bindings for the `modulate`/`lfo`/`env`/`bmod` modulator
// builders (core/controls.mjs). Each takes a config *map* whose key order is
// significant (it mirrors the JS config object), so the Koto map's insertion
// order is preserved into `rudel_core::modulate`.
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    KPattern,
    args::{method_arg, with_instance},
    convert::arg_to_pattern,
};
use koto::{
    prelude::*,
    runtime::{CallContext, ErrorKind, MethodContext, Result as KotoResult, runtime_error},
};
use rudel_core::{Pattern, Value, modulate, pure};

/// Ordered `(rawKey, valuePattern)` pairs from a Koto config-map argument. A
/// non-map argument (or a missing one) yields an empty config.
fn config_entries(arg: &KValue) -> Vec<(String, Pattern)> {
    match arg {
        KValue::Map(m) => m
            .data()
            .iter()
            .filter_map(|(k, v)| match k.value() {
                KValue::Str(key) => Some((key.to_string(), arg_to_pattern(v))),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The id pattern from the optional second argument (`pure(Null)` when absent).
fn id_pattern(arg: Option<KValue>) -> Pattern {
    match arg {
        Some(KValue::Null) | None => pure(Value::Null),
        Some(a) => arg_to_pattern(&a),
    }
}

/// Run `body` with the method's instance pattern and its extra args.
fn with_modulate_instance(
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

/// Build the `pat.modulate(type, config, id)` method for a fixed modulator type
/// (`lfo`/`env`/`bmod`): config is arg 0, the optional id is arg 1.
fn modulate_typed(ctx: &mut CallContext, mod_type: &'static str) -> KotoResult<KValue> {
    with_modulate_instance(ctx, |mctx, pat| {
        let config = config_entries(&method_arg(mctx, 0));
        let id = id_pattern(mctx.args.get(1).cloned());
        modulate(pat, mod_type, config, id)
    })
}

/// Insert the `modulate`/`lfo`/`env`/`bmod` methods onto the shared `KPattern`
/// entries map.
pub(crate) fn insert_modulate_methods(entries: &koto::runtime::KMap) {
    for ty in ["lfo", "env", "bmod"] {
        entries.insert(
            ty,
            KValue::NativeFunction(KNativeFunction::new(move |ctx| modulate_typed(ctx, ty))),
        );
    }
    // The generic `pat.modulate(type, config, id)`: type is arg 0 (a string),
    // config arg 1, id arg 2.
    entries.insert(
        "modulate",
        KValue::NativeFunction(KNativeFunction::new(|ctx| {
            with_modulate_instance(ctx, |mctx, pat| {
                let mod_type = match method_arg(mctx, 0) {
                    KValue::Str(s) => s.to_string(),
                    _ => String::new(),
                };
                let config = config_entries(&method_arg(mctx, 1));
                let id = id_pattern(mctx.args.get(2).cloned());
                modulate(pat, &mod_type, config, id)
            })
        })),
    );
}

/// Register the standalone `lfo(config)`/`env(config)`/`bmod(config)` factories,
/// which build the modulator on an empty control map (`pure({}).lfo(...)`).
pub(crate) fn register_modulate_fns(prelude: &KMap) {
    for ty in ["lfo", "env", "bmod"] {
        prelude.add_fn(ty, move |ctx| {
            let args = ctx.args();
            let config = config_entries(args.first().unwrap_or(&KValue::Null));
            let id = id_pattern(args.get(1).cloned());
            let base = pure(Value::Map(rudel_core::ValueMap::new()));
            Ok(KPattern(modulate(&base, ty, config, id)).into())
        });
    }
}
