// modulate.rs - bindings for the `modulate`/`lfo`/`env`/`bmod` modulator
// builders (core/controls.mjs). Each takes a config *object* whose key order is
// significant (it mirrors the JS config object), so the key order is preserved
// into `rudel_core::modulate`.
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    args::{arg, method},
    convert::{arg_to_pattern, arg_to_raw_str},
};
use crate::js::{Arg, Scope};
use rudel_core::{Pattern, Value, modulate, pure};

/// Ordered `(rawKey, valuePattern)` pairs from a config-object argument. A
/// non-object argument (or a missing one) yields an empty config.
fn config_entries(value: &Arg) -> Vec<(String, Pattern)> {
    match value {
        Arg::Map(m) => m
            .iter()
            .map(|(key, v)| (key.clone(), arg_to_pattern(v)))
            .collect(),
        _ => Vec::new(),
    }
}

/// The id pattern from an optional argument (`pure(Null)` when absent).
fn id_pattern(value: &Arg) -> Pattern {
    match value {
        Arg::Null => pure(Value::Null),
        a => arg_to_pattern(a),
    }
}

/// Put the `modulate`/`lfo`/`env`/`bmod` methods on `Pattern.prototype`.
pub(crate) fn insert_modulate_methods(proto: &Scope) {
    // `pat.lfo(config, id)` and friends: a fixed modulator type.
    for ty in ["lfo", "env", "bmod"] {
        method(proto, ty, move |pat, a| {
            Ok(modulate(pat, ty, config_entries(arg(a, 0)), id_pattern(arg(a, 1))).into())
        });
    }
    // The generic `pat.modulate(type, config, id)`.
    method(proto, "modulate", |pat, a| {
        // A string literal arrives wrapped as a pattern; its raw text is the type.
        let mod_type = arg_to_raw_str(arg(a, 0)).unwrap_or_default();
        let config = config_entries(arg(a, 1));
        Ok(modulate(pat, &mod_type, config, id_pattern(arg(a, 2))).into())
    });
}

/// Register the standalone `lfo(config)`/`env(config)`/`bmod(config)` factories,
/// which build the modulator on an empty control map (`pure({}).lfo(...)`).
pub(crate) fn register_modulate_fns(prelude: &Scope) {
    for ty in ["lfo", "env", "bmod"] {
        prelude.func(ty, move |a| {
            let base = pure(Value::Map(rudel_core::ValueMap::new()));
            Ok(modulate(&base, ty, config_entries(arg(a, 0)), id_pattern(arg(a, 1))).into())
        });
    }
}
