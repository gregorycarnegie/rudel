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
use preprocess::preprocess_strudel;
use samples::register_samples;

pub use bindings::{KPattern, filter_output, output_targets};
pub use samples::SampleEffects;

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
