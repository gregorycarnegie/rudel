//! rudel-lang - Koto scripting bindings for live-coding Rudel patterns.
//! Exposes the rudel-core builder API to Koto so users can type code that is
//! evaluated at runtime (Koto replaces JS as the live layer).
//! SPDX-License-Identifier: AGPL-3.0-or-later

mod bindings;
mod preprocess;
mod samples;

use koto::prelude::*;
use rudel_core::Pattern;
use std::sync::{Arc, Mutex};

use bindings::register;
use bindings::{function_names, method_names};
use preprocess::preprocess_strudel;
use samples::register_samples;

pub use bindings::{KPattern, filter_output, output_targets};
pub use samples::SampleEffects;

/// The names a user can reach in Rudel scripts, generated from the live runtime
/// (not a hand-maintained list) so it stays in sync with what is actually
/// exposed. Drives the editor's reference panel, highlighting, and (later)
/// autocomplete; mirrors the role of Strudel's `reference` package.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Top-level functions and values (`note`, `stack`, `sine`, `Math`, ...).
    pub functions: Vec<String>,
    /// Methods callable on a pattern (`fast`, `gain`, `every`, ...).
    pub methods: Vec<String>,
    /// Control names from the core registry (`lpf`, `room`, `delay`, ...).
    pub controls: Vec<String>,
}

/// Build the [`Reference`] surface by introspecting the registered runtime.
pub fn reference() -> Reference {
    let prelude = KMap::default();
    register(&prelude);
    let effects = Arc::new(Mutex::new(SampleEffects::default()));
    register_samples(&prelude, effects);

    let mut controls: Vec<String> = rudel_core::control_builders()
        .map(|(name, _)| name.to_string())
        .chain(
            rudel_core::numbered_control_names()
                .into_iter()
                .map(|(name, _)| name),
        )
        .collect();
    controls.sort();
    controls.dedup();

    Reference {
        functions: function_names(&prelude),
        methods: method_names(),
        controls,
    }
}

/// Evaluate a Koto script and extract the resulting pattern.
pub fn eval(script: &str) -> Result<Pattern, String> {
    eval_with_samples(script).map(|(pat, _)| pat)
}

/// Evaluate a Koto script, returning the resulting pattern plus the sample
/// effects (`samples(...)` / `aliasBank(...)`) requested during evaluation. The
/// host applies those effects (e.g. `Engine::samples` / `Engine::alias_bank`)
/// against its own sample bank.
pub fn eval_with_samples(script: &str) -> Result<(Pattern, SampleEffects), String> {
    let effects = Arc::new(Mutex::new(SampleEffects::default()));
    let mut koto = Koto::default();
    register(koto.prelude());
    register_samples(koto.prelude(), effects.clone());
    let script = preprocess_strudel(script);
    let chunk = koto.compile(&script).map_err(|e| e.to_string())?;
    let result = koto.run(chunk).map_err(|e| e.to_string())?;
    let effects = std::mem::take(&mut *effects.lock().unwrap());
    match result {
        KValue::Object(o) if o.is_a::<KPattern>() => {
            Ok((o.cast::<KPattern>().unwrap().0.clone(), effects))
        }
        other => Err(format!("script did not return a pattern (got {other:?})")),
    }
}

#[cfg(test)]
mod tests;
