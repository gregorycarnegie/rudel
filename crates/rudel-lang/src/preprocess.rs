//! The transpiler: what Strudel's `transpiler.mjs` does to a script before it
//! runs, as text passes over the source. The engine reads JavaScript itself;
//! these passes are the Strudel on top of it — mini-notation literals, inline
//! widgets, `name:` labels, Mondo Notation and the kabelsalat scope.

mod kabelsalat;
mod labels;
mod mini;
mod mondo;
mod scanner;
mod syntax;
mod widgets;

use kabelsalat::scope_kabelsalat_calls;
use labels::rewrite_labels;
use mini::annotate_mini_offsets;
pub(crate) use mondo::looks_like_mondo;
use mondo::rewrite_mondo_templates;
use syntax::{rewrite_alignment_getters, rewrite_tagged_templates, strip_await, strip_comments};
use widgets::rewrite_editor_widgets_with_context;

/// How deep a script's brackets may nest. boa's parser recurses once per
/// level with large frames, and on the engine thread's stack it overflows —
/// aborting the process, where no error can be caught — a few hundred levels
/// in. Real scripts nest a dozen deep.
const MAX_NESTING: usize = 128;

/// Refuse a script nested deeper than the engine can parse without crashing.
pub(crate) fn check_nesting(source: &str) -> Result<(), String> {
    match scanner::bracket_depth(source) {
        depth if depth > MAX_NESTING => Err(format!(
            "SyntaxError: brackets nested {depth} levels deep (max {MAX_NESTING})"
        )),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreprocessResult {
    pub source: String,
    pub widgets: Vec<crate::WidgetConfig>,
}

#[cfg(test)]
pub(crate) fn preprocess_strudel(script: &str) -> String {
    preprocess_strudel_with_meta(script).source
}

pub(crate) fn preprocess_strudel_with_meta(script: &str) -> PreprocessResult {
    preprocess_strudel_with_meta_in_range(script, 0)
}

pub(crate) fn preprocess_strudel_with_meta_in_range(
    script: &str,
    node_offset: usize,
) -> PreprocessResult {
    // Mondo compiles to JavaScript, so it runs first and everything below sees
    // a script with no mondo left in it.
    let script = rewrite_mondo_templates(script);
    let (script, widgets, anchors) = rewrite_editor_widgets_with_context(&script, node_offset, "");
    // The returned spans are only an assertion handle for `mini`'s own tests;
    // per-hap source locations reach the editor through the `m(...)` calls this
    // pass writes into the script, not through a side table.
    let (script, _spans) = annotate_mini_offsets(&script, node_offset, &anchors);
    let script = strip_comments(&script);
    let script = scope_kabelsalat_calls(&script);
    let script = rewrite_tagged_templates(&script);
    let script = rewrite_alignment_getters(&script);
    let script = strip_await(&script);
    let script = rewrite_labels(&script);
    // Mirror the transpiler's empty-body fallback: an empty (or fully
    // commented-out) script evaluates to silence rather than erroring.
    let source = if script.trim().is_empty() {
        "silence".to_string()
    } else {
        script
    };
    PreprocessResult { source, widgets }
}
