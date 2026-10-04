//! rudel-lang - JavaScript scripting bindings for live-coding Rudel patterns.
//! Exposes the rudel-core builder API to JavaScript (run by boa) so users can
//! type Strudel code that is evaluated at runtime.
//! SPDX-License-Identifier: AGPL-3.0-or-later

mod bindings;
mod blocks;
pub use blocks::Blocks;
mod js;
mod preprocess;
mod samples;
mod sliders;
pub mod triggers;
mod widgets;

use js::{Arg, Scope};
use rudel_core::Pattern;
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex, OnceLock},
};

use bindings::{apply_pattern_transforms, method_names, register, reset_registered, reset_slots};
pub mod canvas;
pub mod hydra;
pub mod kabelsalat;

use preprocess::{preprocess_strudel_with_meta, preprocess_strudel_with_meta_in_range};
use samples::register_samples;

pub use bindings::{filter_output, output_targets};
pub use preprocess::CANVAS_OPTION;
pub use samples::SampleEffects;
pub use sliders::{set_slider_value, slider_value};

/// Install mini-notation as the parser for Rust string patterns.
pub fn install_mini() {
    rudel_mini::install();
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct EvalMeta {
    /// Inline editor widgets discovered during preprocessing/evaluation.
    pub widgets: Vec<WidgetConfig>,
    /// What the script sent to hydra's own outputs (`osc().out()`), drawn
    /// behind the code as upstream's full-screen hydra canvas is. A `_hydra`
    /// widget config with no source position.
    pub hydra: Option<WidgetConfig>,
    /// The per-frame arguments every hydra chain of this evaluation reads,
    /// by slot (`H(pattern)`, arrays, functions); the app fills them in.
    pub hydra_params: Vec<hydra::HydraParam>,
    /// What the script draws on Strudel's canvas behind the code, run once a
    /// frame by the app. `None` when it draws nothing.
    pub canvas: Option<canvas::CanvasDriver>,
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

pub struct EvalResult {
    pub pattern: Pattern,
    pub sample_effects: SampleEffects,
    pub meta: EvalMeta,
    /// `onTriggerTime` callbacks this evaluation registered. Empty unless the
    /// script called `onTriggerTime`.
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
    let (functions, methods) = js::on_js_thread(|| {
        let mut ctx = js::new_context();
        js::lend(&mut ctx, || {
            let global = Scope::global();
            register(&global);
            register_samples(&global, Arc::default());
            // `__`-names are rudel's own plumbing (`__drawFrame`), not for scripts.
            let names = global.names().into_iter().filter(|n| !n.starts_with("__"));
            (names.collect(), method_names())
        })
    });
    Reference {
        functions,
        methods,
        controls,
    }
}

/// The global names rudel itself registers, without the language's own
/// (`Math`, `Array`, ...). Computed once.
pub(crate) fn registered_names() -> &'static HashSet<String> {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        js::on_js_thread(|| {
            let mut ctx = js::new_context();
            js::lend(&mut ctx, || {
                let global = Scope::global();
                let builtin: HashSet<String> = global.names().into_iter().collect();
                register(&global);
                register_samples(&global, Arc::default());
                global
                    .names()
                    .into_iter()
                    .filter(|name| !builtin.contains(name))
                    .collect()
            })
        })
    })
}

/// The script as the engine runs it, after the Strudel passes. An engine
/// error's line and column point into this, not into what was typed.
#[doc(hidden)]
pub fn preprocessed(script: &str) -> String {
    preprocess_strudel_with_meta(script).source
}

/// Evaluate a script and extract the resulting pattern.
pub fn eval(script: &str) -> Result<Pattern, String> {
    eval_result(script).map(|result| result.pattern)
}

/// Evaluate a script, returning the resulting pattern plus the sample effects
/// (`samples(...)` / `aliasBank(...)`) requested during evaluation. The host
/// applies those effects (e.g. `Engine::samples` / `Engine::alias_bank`)
/// against its own sample bank.
pub fn eval_with_samples(script: &str) -> Result<(Pattern, SampleEffects), String> {
    eval_result(script).map(|result| (result.pattern, result.sample_effects))
}

/// Evaluate a script, returning the pattern plus all host-facing side effects
/// and editor metadata gathered during preprocessing/evaluation.
pub fn eval_result(script: &str) -> Result<EvalResult, String> {
    eval_result_with_preprocessor(script, None, None, || preprocess_strudel_with_meta(script))
}

/// [`eval_result`] for the editor's whole document: `blocks` starts again
/// from its labelled patterns and declarations, which a later
/// [`eval_result_with_source_range`] of one block keeps playing and can use.
pub fn eval_document(script: &str, blocks: &mut Blocks) -> Result<EvalResult, String> {
    eval_result_with_preprocessor(script, Some(blocks), None, || {
        preprocess_strudel_with_meta(script)
    })
}

/// Evaluate one block of the editor's document, at `range` in it, while the
/// other `blocks` keep playing: Strudel's `evaluateBlock`. The result stacks
/// every block's pattern. Source ranges stay absolute, the counterpart of the
/// block-based transpiler's `range` / `nodeOffset` option.
pub fn eval_result_with_source_range(
    script: &str,
    range: (usize, usize),
    blocks: &mut Blocks,
) -> Result<EvalResult, String> {
    eval_result_with_preprocessor(script, Some(blocks), Some(range), || {
        preprocess_strudel_with_meta_in_range(script, range.0)
    })
}

/// The script could not be parsed. If Mondo Notation can read it, say so: a
/// script pasted from upstream's docs is written in it, and the syntax error
/// for that — at the first `$` or bare word — says nothing about why.
fn mondo_hint(original: &str, err: String) -> String {
    if !err.starts_with("SyntaxError") || !preprocess::looks_like_mondo(original) {
        return err;
    }
    format!(
        "{err}\n\nThis looks like Mondo Notation. Put `// mondo` on the first \
         line to read the whole script as mondo, or wrap a single pattern in \
         mondo`...`."
    )
}

/// One evaluation at a time, process-wide. The REPL slots, trigger hooks and
/// widget options below are registries that an evaluation clears at its start
/// and reads back at its end, so two concurrent evaluations would wipe each
/// other's recordings. Evaluations already queue for the one JS thread; the
/// lock also covers the preprocessing before it and the tests that read the
/// registries directly.
static EVAL_LOCK: Mutex<()> = Mutex::new(());

fn eval_result_with_preprocessor(
    original: &str,
    mut blocks: Option<&mut Blocks>,
    range: Option<(usize, usize)>,
    preprocess: impl FnOnce() -> preprocess::PreprocessResult,
) -> Result<EvalResult, String> {
    // A script that panics mid-evaluation must not wedge every later one.
    let _guard = EVAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let preprocessed = preprocess();
    preprocess::check_nesting(&preprocessed.source)?;
    let original = original.to_string();
    // The engine can panic on a script it should have rejected — boa's
    // `Array.prototype.sort` does on a comparator that is not a total order,
    // the `() => Math.random() - 0.5` shuffle scripts reach for. That is the
    // script's error to report, not a reason to take the editor down.
    // The blocks go to the JS thread with the evaluation and come back.
    let mut lent = blocks.as_deref_mut().map(std::mem::take);
    let (result, lent) = js::on_js_thread(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            evaluate(&original, preprocessed, lent.as_mut(), range)
        }));
        (result, lent)
    });
    if let (Some(blocks), Some(lent)) = (blocks, lent) {
        *blocks = lent;
    }
    result.unwrap_or_else(|panic| Err(engine_failure(panic.as_ref())))
}

/// The message for an evaluation the engine itself gave up on.
fn engine_failure(panic: &(dyn std::any::Any + Send)) -> String {
    let what = panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    let hint = if what.contains("total order") {
        " — a `sort` comparator has to be consistent; to shuffle, use \
         `rand`/`shuffle` rather than `sort(() => Math.random() - 0.5)`"
    } else {
        ""
    };
    format!("the JavaScript engine failed on this script: {what}{hint}")
}

/// A fresh engine with rudel registered in it, and the effects its sample
/// functions record into.
type Prepared = (js::Context, Arc<Mutex<SampleEffects>>);

thread_local! {
    /// The engine the next evaluation will use, made ahead of time: building
    /// one is most of what an evaluation of a short pattern costs.
    static SPARE: std::cell::Cell<Option<Prepared>> = const { std::cell::Cell::new(None) };
}

fn prepare() -> Prepared {
    let effects = Arc::new(Mutex::new(SampleEffects::default()));
    let mut ctx = js::new_context();
    js::lend(&mut ctx, || {
        let global = Scope::global();
        register(&global);
        register_samples(&global, effects.clone());
    });
    (ctx, effects)
}

/// The evaluation itself, on the JS thread.
fn evaluate(
    original: &str,
    preprocessed: preprocess::PreprocessResult,
    blocks: Option<&mut Blocks>,
    range: Option<(usize, usize)>,
) -> Result<EvalResult, String> {
    if range.is_some() {
        blocks::check_labels(&preprocessed.labels)?;
    }
    let (mut ctx, effects) = SPARE.take().unwrap_or_else(prepare);
    // Build the next evaluation's engine now, while the user is listening to
    // this one, so the next keystroke does not wait for it.
    js::post(|| SPARE.set(Some(prepare())));
    let mut meta = EvalMeta {
        widgets: preprocessed.widgets,
        hydra: None,
        hydra_params: Vec::new(),
        canvas: None,
    };
    // Clear any REPL slots (`p`/`d1`/…) registered by a previous evaluation so
    // they don't leak into this one (Strudel calls `hush()` at eval start).
    reset_slots();
    reset_registered();
    triggers::reset_hooks();
    widgets::reset_options();
    bindings::hydra::reset_scene();
    // The kabelsalat arena is append-only while a script builds its graphs, so
    // it has to be dropped between runs or a long REPL session accumulates
    // every node it ever built.
    kabelsalat::reset();
    blocks::reset_cleared();
    if let (Some(blocks), Some(range)) = (&blocks, range) {
        bindings::seed_slots(blocks.others(range));
        blocks.define_carried(&mut ctx);
    }
    let value = js::run(&mut ctx, &preprocessed.source).map_err(|e| mondo_hint(original, e));
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
    let images = bindings::hydra::scene_images();
    for widget in meta
        .widgets
        .iter_mut()
        .filter(|w| w.widget_type == "_hydra")
    {
        widget.options.extend(images.iter().cloned());
    }
    meta.hydra = bindings::hydra::take_scene();
    meta.hydra_params = bindings::hydra::take_params();
    let effects = std::mem::take(&mut *effects.lock().unwrap());
    // Combine the script's pattern with any registered slots/labels and the
    // `each`/`all` transforms, mirroring Strudel's `applyPatternTransforms`:
    // registered slots stack (with soloing and `each`), otherwise the script's
    // own value is used, and every `all` transform runs over the result.
    let combined = value.and_then(|value| {
        let script_pattern = match &value {
            Arg::Pat(p) => Some(p.clone()),
            // A script that ends on a statement (`let cpm = 30;`) has no
            // value; upstream's REPL plays silence for it. Any other
            // non-pattern is still an error, which says more than silence.
            Arg::Null => Some(rudel_core::silence()),
            _ => None,
        };
        js::lend(&mut ctx, || {
            match apply_pattern_transforms(script_pattern) {
                Some(pattern) => Ok(pattern),
                None => Err(format!(
                    "script did not return a pattern (got {})",
                    js::display(&value)
                )),
            }
        })
    });
    let registered = bindings::registered_slots();
    // The transforms are script functions of this context; let them go with it.
    reset_slots();
    let pattern = combined?;
    let declared = blocks
        .is_some()
        .then(|| blocks::read_declared(&mut ctx, &preprocessed.source));
    // Kept before parking, so a script that draws keeps its context alive.
    meta.canvas = match js::run(
        &mut ctx,
        "globalThis.__drawUsed() ? __drawFrame : undefined",
    ) {
        Ok(frame @ Arg::Func(_)) => {
            js::lend(&mut ctx, || js::keep(&frame)).map(canvas::CanvasDriver::new)
        }
        _ => None,
    };
    let session = js::park(ctx);
    if let (Some(blocks), Some(declared)) = (blocks, declared) {
        let (slots, fresh) = registered;
        blocks.record(
            slots,
            &fresh,
            &preprocessed.labels,
            original,
            range.map_or(0, |r| r.0),
            declared,
            session.clone(),
            range.is_some(),
        );
    }
    let trigger_hooks = triggers::TriggerHooks::take(session.clone());
    let pattern = match session {
        Some(session) => keep_alive(pattern, session),
        None => pattern,
    };
    Ok(EvalResult {
        pattern,
        sample_effects: effects,
        meta,
        trigger_hooks,
    })
}

/// `pat`, holding `session` for as long as it lives: the engine a script
/// function inside it runs on.
fn keep_alive(mut pat: Pattern, session: Arc<js::Session>) -> Pattern {
    let inner = pat.clone();
    let mut out = Pattern::new(move |state| {
        let _engine = &session;
        inner.query(state)
    });
    out.steps = pat.steps;
    out.pure_value = pat.pure_value.take();
    out.pure_loc = pat.pure_loc;
    out.source = pat.source.take();
    out
}

#[cfg(test)]
mod tests;
