//! rudel-lang - Koto scripting bindings for live-coding Rudel patterns.
//! Exposes the rudel-core builder API to Koto so users can type code that is
//! evaluated at runtime (Koto replaces JS as the live layer).
//! SPDX-License-Identifier: AGPL-3.0-or-later

mod bindings;
mod preprocess;
mod samples;
mod sliders;
pub mod triggers;
mod widgets;

use koto::prelude::*;
use rudel_core::Pattern;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use bindings::{apply_pattern_transforms, function_names, method_names, register, reset_slots};
use preprocess::{preprocess_strudel_with_meta, preprocess_strudel_with_meta_in_range};
use samples::register_samples;

pub use bindings::{KPattern, filter_output, output_targets};
pub use samples::SampleEffects;
pub use sliders::{set_slider_value, slider_value};

/// Install mini-notation as the parser for Rust string patterns.
pub fn install_mini() {
    rudel_mini::install();
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct EvalMeta {
    /// Source byte ranges for mini-notation leaves discovered during
    /// preprocessing, matching Strudel's `meta.miniLocations` role.
    pub mini_locations: Vec<(usize, usize)>,
    /// Inline editor widgets discovered during preprocessing/evaluation.
    pub widgets: Vec<WidgetConfig>,
    /// Block/label metadata for range-aware evaluation.
    pub labels: Vec<LabelMeta>,
    /// Cleanup requested after eval, e.g. when a visual widget was removed.
    pub cleanup: CleanupHints,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct WidgetConfig {
    pub widget_type: String,
    pub id: String,
    pub from: usize,
    pub to: usize,
    pub index: usize,
    pub options: BTreeMap<String, WidgetOption>,
    pub value: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WidgetOption {
    Bool(bool),
    Number(f64),
    String(String),
}

impl WidgetOption {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            WidgetOption::Bool(value) => Some(*value),
            WidgetOption::Number(value) => Some(*value != 0.0),
            WidgetOption::String(value) => match value.as_str() {
                "true" | "1" => Some(true),
                "false" | "0" => Some(false),
                _ => None,
            },
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            WidgetOption::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
            WidgetOption::Number(value) => Some(*value),
            WidgetOption::String(value) => value.parse().ok(),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            WidgetOption::String(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LabelMeta {
    pub name: String,
    pub index: usize,
    pub end: usize,
    pub full_match: String,
    pub active_visualizer: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CleanupHints {
    pub widget_removed: bool,
    pub cleanup_draw_context: bool,
}

pub struct EvalResult {
    pub pattern: Pattern,
    pub sample_effects: SampleEffects,
    pub meta: EvalMeta,
    /// `onTriggerTime` callbacks this evaluation registered, with the VM that
    /// runs them. Empty unless the script called `onTriggerTime`.
    pub trigger_hooks: triggers::TriggerHooks,
}

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
    eval_result(script).map(|result| result.pattern)
}

/// Evaluate a Koto script, returning the resulting pattern plus the sample
/// effects (`samples(...)` / `aliasBank(...)`) requested during evaluation. The
/// host applies those effects (e.g. `Engine::samples` / `Engine::alias_bank`)
/// against its own sample bank.
pub fn eval_with_samples(script: &str) -> Result<(Pattern, SampleEffects), String> {
    eval_result(script).map(|result| (result.pattern, result.sample_effects))
}

/// Evaluate a Koto script, returning the pattern plus all host-facing side
/// effects and editor metadata gathered during preprocessing/evaluation.
pub fn eval_result(script: &str) -> Result<EvalResult, String> {
    eval_result_with_preprocessor(|| preprocess_strudel_with_meta(script))
}

/// Evaluate a source slice while preserving absolute source ranges from the
/// surrounding editor buffer. This is the native counterpart to Strudel's
/// block-based transpiler `range` / `nodeOffset` option.
pub fn eval_result_with_source_range(
    script: &str,
    range: (usize, usize),
) -> Result<EvalResult, String> {
    eval_result_with_preprocessor(|| preprocess_strudel_with_meta_in_range(script, range.0))
}

/// One evaluation at a time, process-wide. The REPL slots, trigger hooks and
/// widget options below are process-global registries that an evaluation clears
/// at its start and reads back at its end, so two concurrent evaluations wipe
/// each other's recordings. The host only ever evaluates on one thread; this
/// makes that a rule instead of an assumption (and keeps the parallel test
/// suite from racing itself).
static EVAL_LOCK: Mutex<()> = Mutex::new(());

fn eval_result_with_preprocessor(
    preprocess: impl FnOnce() -> preprocess::PreprocessResult,
) -> Result<EvalResult, String> {
    // A script that panics mid-evaluation must not wedge every later one.
    let _guard = EVAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let effects = Arc::new(Mutex::new(SampleEffects::default()));
    let mut koto = Koto::default();
    register(koto.prelude());
    register_samples(koto.prelude(), effects.clone());
    let preprocessed = preprocess();
    let script = preprocessed.source;
    let preprocess::PreprocessMeta {
        mini_locations,
        widgets,
    } = preprocessed.meta;
    let mut meta = EvalMeta {
        mini_locations,
        widgets: widgets
            .into_iter()
            .map(|widget| WidgetConfig {
                widget_type: widget.widget_type,
                id: widget.id,
                from: widget.from,
                to: widget.to,
                index: widget.index,
                options: widget.options,
                value: widget.value,
                min: widget.min,
                max: widget.max,
                step: widget.step,
            })
            .collect(),
        ..Default::default()
    };
    // Clear any REPL slots (`p`/`d1`/…) registered by a previous evaluation so
    // they don't leak into this one (Strudel calls `hush()` at eval start).
    reset_slots();
    triggers::reset_hooks();
    widgets::reset_options();
    let chunk = koto.compile(&script).map_err(|e| e.to_string())?;
    let result = koto.run(chunk).map_err(|e| e.to_string())?;
    // Fold in the options the widget calls actually evaluated to. The source
    // scan above could only read literals, so this is what makes a computed
    // option (`.pianoroll({cycles: n})`) reach the painter. Evaluated values
    // win over scanned ones, which agree anyway wherever the option was a
    // literal.
    for widget in &mut meta.widgets {
        if let Some(evaluated) = widgets::recorded_options(&widget.id) {
            widget.options.extend(evaluated);
        }
    }
    let effects = std::mem::take(&mut *effects.lock().unwrap());
    // Combine the script's pattern with any registered slots/labels and the
    // `each`/`all` transforms, mirroring Strudel's `applyPatternTransforms`:
    // registered slots stack (with soloing and `each`), otherwise the script's
    // own return value is used, and every `all` transform runs over the result.
    let script_pattern = match &result {
        KValue::Object(o) if o.is_a::<KPattern>() => Some(o.cast::<KPattern>().unwrap().0.clone()),
        _ => None,
    };
    let pattern = match apply_pattern_transforms(script_pattern) {
        Some(pattern) => pattern,
        None => return Err(format!("script did not return a pattern (got {result:?})")),
    };
    Ok(EvalResult {
        pattern,
        sample_effects: effects,
        meta,
        trigger_hooks: triggers::TriggerHooks::take(koto),
    })
}

#[cfg(test)]
mod tests;
