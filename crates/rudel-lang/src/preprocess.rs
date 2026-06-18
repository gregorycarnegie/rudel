mod labels;
mod mini;
mod scanner;
mod syntax;
mod widgets;

use crate::WidgetOption;
use labels::rewrite_labels;
use mini::annotate_mini_offsets;
use std::collections::BTreeMap;
use syntax::{
    indent_dot_continuations, rewrite_arrow_functions, rewrite_const_declarations,
    rewrite_string_method_chains, strip_line_comments,
};
use widgets::rewrite_editor_widgets_with_context;

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct PreprocessMeta {
    pub mini_locations: Vec<(usize, usize)>,
    pub widgets: Vec<PreprocessWidget>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct PreprocessWidget {
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
pub(crate) struct PreprocessResult {
    pub source: String,
    pub meta: PreprocessMeta,
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
    let (script, widgets, anchors) = rewrite_editor_widgets_with_context(script, node_offset, "");
    let (script, mini_locations) = annotate_mini_offsets(&script, node_offset, &anchors);
    let script = strip_line_comments(&script);
    let script = rewrite_arrow_functions(&script);
    let script = rewrite_const_declarations(&script);
    let script = rewrite_string_method_chains(&script);
    let script = indent_dot_continuations(&script);
    let script = rewrite_labels(&script);
    // Mirror the transpiler's empty-body fallback: an empty (or fully
    // commented-out) script evaluates to silence rather than erroring.
    let source = if script.trim().is_empty() {
        "silence()".to_string()
    } else {
        script
    };
    PreprocessResult {
        source,
        meta: PreprocessMeta {
            mini_locations,
            widgets,
        },
    }
}
