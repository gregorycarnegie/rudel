mod labels;
mod mini;
mod mondo;
mod scanner;
mod syntax;
mod widgets;

use labels::rewrite_labels;
use mini::annotate_mini_offsets;
pub(crate) use mondo::looks_like_mondo;
use mondo::rewrite_mondo_templates;
use syntax::{
    flatten_non_final_groups, hoist_leading_commas, indent_dot_continuations,
    join_dangling_operators, order_declarations, quote_numeric_map_keys,
    rename_ignored_identifiers, rename_koto_keywords, rewrite_alignment_getters,
    rewrite_arrow_functions, rewrite_block_bodies, rewrite_const_declarations,
    rewrite_leading_dot_numbers, rewrite_length_property, rewrite_logical_operators,
    rewrite_object_spreads, rewrite_prototype_methods, rewrite_spread_calls,
    rewrite_strict_equality, rewrite_string_method_chains, rewrite_tagged_templates,
    rewrite_ternaries, rewrite_typeof, rewrite_value_property, strip_await, strip_comments,
    strip_new, strip_trailing_semicolons, tighten_call_parens, tighten_member_dots,
};
use widgets::rewrite_editor_widgets_with_context;

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
    // Mondo compiles to Koto, so it runs first and everything below sees a
    // script with no mondo left in it.
    let script = rewrite_mondo_templates(script);
    let (script, widgets, anchors) = rewrite_editor_widgets_with_context(&script, node_offset, "");
    // The returned spans are only an assertion handle for `mini`'s own tests;
    // per-hap source locations reach the editor through the `m(...)` calls this
    // pass writes into the script, not through a side table.
    let (script, _spans) = annotate_mini_offsets(&script, node_offset, &anchors);
    let script = strip_comments(&script);
    let script = rename_koto_keywords(&script);
    let script = rename_ignored_identifiers(&script);
    let script = rewrite_tagged_templates(&script);
    let script = rewrite_leading_dot_numbers(&script);
    let script = rewrite_strict_equality(&script);
    let script = rewrite_typeof(&script);
    let script = rewrite_logical_operators(&script);
    let script = rewrite_length_property(&script);
    let script = rewrite_value_property(&script);
    let script = strip_trailing_semicolons(&script);
    let script = join_dangling_operators(&script);
    let script = rewrite_prototype_methods(&script);
    let script = strip_new(&script);
    let script = rewrite_ternaries(&script);
    let script = rewrite_block_bodies(&script);
    let script = rewrite_const_declarations(&script);
    let script = rewrite_object_spreads(&script);
    let script = rewrite_spread_calls(&script);
    let script = quote_numeric_map_keys(&script);
    let script = rewrite_alignment_getters(&script);
    let script = strip_await(&script);
    let script = rewrite_arrow_functions(&script);
    let script = rewrite_string_method_chains(&script);
    let script = tighten_call_parens(&script);
    let script = tighten_member_dots(&script);
    let script = flatten_non_final_groups(&script);
    let script = hoist_leading_commas(&script);
    let script = indent_dot_continuations(&script);
    let script = order_declarations(&script);
    let script = rewrite_labels(&script);
    // Mirror the transpiler's empty-body fallback: an empty (or fully
    // commented-out) script evaluates to silence rather than erroring.
    let source = if script.trim().is_empty() {
        "silence()".to_string()
    } else {
        script
    };
    PreprocessResult { source, widgets }
}
