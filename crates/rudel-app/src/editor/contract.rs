//! End-to-end tests for the native StrudelMirror contract.
//!
//! The per-module tests (`decorations`, `sliders`, `widgets`, `blocks`,
//! `highlight`, `edit`) each check one piece in isolation. These drive the
//! whole sequence a live-coding session actually performs — evaluate, decorate,
//! drag a slider, edit the text around it, re-evaluate a block, remove a widget,
//! flash the active haps — against *real* `rudel_lang` evaluation output rather
//! than hand-built metadata, so a change that breaks the seam between two
//! modules fails here even when both modules' own tests still pass.
//!
//! What these deliberately do not cover: pixel output and pointer input, which
//! need a live `egui` context and a window. Those remain manual.

use super::{
    blocks::{block_at_byte, block_regions},
    decorations::{EditorDecorationState, SourceRange, TextChange},
    highlight::tokenize,
    sliders::slider_reservations,
    widgets::WidgetHostState,
};
use rudel_core::Frac;
use std::collections::HashSet;

/// Evaluate `src` the way the app does, returning the metadata the editor
/// decorates from.
fn eval_meta(src: &str) -> rudel_lang::EvalMeta {
    rudel_lang::eval_result(src)
        .unwrap_or_else(|e| panic!("eval {src:?}: {e}"))
        .meta
}

/// Evaluate one block, the way `Ctrl+Shift+Enter` does: only the block's own
/// text is compiled, but `range.from` is passed as the node offset so every id
/// and location stays absolute to the whole buffer.
fn eval_meta_range(src: &str, range: SourceRange) -> rudel_lang::EvalMeta {
    let block = &src[range.from..range.to];
    rudel_lang::eval_result_with_source_range(block, (range.from, range.to))
        .unwrap_or_else(|e| panic!("eval range {block:?}: {e}"))
        .meta
}

#[test]
fn slider_rewrite_decoration_and_live_value_round_trip() {
    // `slider(...)` is scanned out of the source, given a stable `from:to` id,
    // and rewritten to `slider_with_id`; the editor then anchors a control at
    // the literal and writes back through the live registry.
    let src = r#"s("bd*4").gain(slider(0.5, 0, 1))"#;
    let mut state = EditorDecorationState::default();
    state.replace_all(&eval_meta(src));

    let sliders = state.sliders();
    assert_eq!(sliders.len(), 1, "one slider decoration: {sliders:?}");
    let id = sliders[0].id.clone();
    // The id is the first argument's source range, as `plugin-widgets.mjs`
    // builds it, and the decoration points at that literal.
    let (from, to) = (sliders[0].range.from, sliders[0].range.to);
    assert_eq!(&src[from..to], "0.5", "the decoration covers the literal");
    assert_eq!(id, format!("{from}:{to}"));

    // The reservation the renderer uses to make room for the inline control is
    // anchored at the same place.
    let reservations = slider_reservations(state.sliders());
    assert_eq!(reservations.len(), 1);
    assert_eq!((reservations[0].0, reservations[0].1), (from, to));

    // Dragging: the live registry updates without a re-eval, and the source
    // literal is replaced in step.
    assert!(
        rudel_lang::set_slider_value(&id, 0.75),
        "the registry must know the id the scanner produced"
    );
    assert!(state.set_slider_literal(&id, "0.75".to_string()));
    assert_eq!(state.sliders()[0].value.as_deref(), Some("0.75"));

    // An unknown id is refused rather than silently registered, like upstream's
    // message handler.
    assert!(!rudel_lang::set_slider_value("nope:nope", 0.1));
}

#[test]
fn decorations_survive_edits_elsewhere_in_the_document() {
    // Typing above a slider must move its decoration, not invalidate it — this
    // is the seam between `TextChange::from_texts` and `map_change`.
    let before = r#"s("bd*4").gain(slider(0.5, 0, 1))"#;
    let mut state = EditorDecorationState::default();
    state.replace_all(&eval_meta(before));
    let original = state.sliders()[0].range;

    let after = format!("// a new comment line\n{before}");
    let change = TextChange::from_texts(before, &after).expect("an insertion is a text change");
    state.map_change(change);

    let moved = state.sliders()[0].range;
    let shift = after.len() - before.len();
    assert_eq!(moved.from, original.from + shift);
    assert_eq!(moved.to, original.to + shift);
    assert_eq!(
        &after[moved.from..moved.to],
        "0.5",
        "the remapped range still covers the literal"
    );
}

#[test]
fn block_eval_preserves_decorations_outside_the_evaluated_range() {
    // Strudel's block eval replaces only the decorations inside the range; a
    // widget in another block must survive. The ids stay absolute to the whole
    // buffer, which is what `nodeOffset` is for.
    let src = "s(\"bd*4\")._pianoroll()\n\ns(\"hh*8\").gain(slider(0.5, 0, 1))";
    let mut state = EditorDecorationState::default();
    state.replace_all(&eval_meta(src));
    assert_eq!(state.widgets().len(), 1, "{:?}", state.widgets());
    assert_eq!(state.sliders().len(), 1);
    let widget_id = state.widgets()[0].id.clone();

    // Evaluate only the second block.
    let blocks = block_regions(src);
    assert_eq!(blocks.len(), 2, "blank line separates the two blocks");
    let second = SourceRange::new(blocks[1].from, blocks[1].to);
    let cursor = block_at_byte(src, second.from).expect("cursor lands in a block");
    assert_eq!(cursor.from, second.from);

    state.replace_range(&eval_meta_range(src, second), second);

    assert_eq!(
        state.widgets().len(),
        1,
        "the pianoroll outside the range must survive block eval"
    );
    assert_eq!(state.widgets()[0].id, widget_id, "and keep its id");
    assert_eq!(state.sliders().len(), 1, "the in-range slider is re-scanned");
    let slider = &state.sliders()[0];
    assert_eq!(
        &src[slider.range.from..slider.range.to],
        "0.5",
        "block-eval ids/ranges stay absolute to the full buffer"
    );
}

#[test]
fn widget_surfaces_are_reused_by_identity_and_cleaned_up_when_removed() {
    let two = "s(\"bd*4\")._pianoroll()\n\ns(\"hh*8\")._spiral()";
    let mut state = EditorDecorationState::default();
    let mut host = WidgetHostState::default();

    state.replace_all(&eval_meta(two));
    let first = host.sync(state.widgets());
    assert_eq!(first.created.len(), 2, "{first:?}");
    assert!(first.removed.is_empty());

    // Re-evaluating identical source must not recreate the surfaces — a
    // repaint would otherwise reset every widget on each keystroke.
    state.replace_all(&eval_meta(two));
    let again = host.sync(state.widgets());
    assert!(again.created.is_empty(), "{again:?}");
    assert!(again.removed.is_empty(), "{again:?}");

    // Deleting one widget's call removes exactly its surface.
    let one = "s(\"bd*4\")._pianoroll()";
    state.replace_all(&eval_meta(one));
    let shrunk = host.sync(state.widgets());
    assert!(shrunk.created.is_empty(), "{shrunk:?}");
    assert_eq!(shrunk.removed.len(), 1, "{shrunk:?}");

    // And removing the last one clears the host.
    state.replace_all(&eval_meta("s(\"bd*4\")"));
    let empty = host.sync(state.widgets());
    assert_eq!(empty.removed.len(), 1, "{empty:?}");
    assert!(host.sync(state.widgets()).removed.is_empty(), "idempotent");
}

#[test]
fn active_haps_flash_the_source_ranges_that_produced_them() {
    // The whole point of the mini-location plumbing: a playing hap points back
    // at the exact bytes it came from.
    let src = r#"s("bd sd")"#;
    let result = rudel_lang::eval_result(src).expect("eval");
    let mut state = EditorDecorationState::default();
    state.replace_all(&result.meta);

    let haps = result
        .pattern
        .query_arc(Frac::new(0, 1), Frac::new(1, 2))
        .into_iter()
        .filter(|h| h.has_onset())
        .collect::<Vec<_>>();
    assert_eq!(haps.len(), 1, "one onset in the first half cycle");

    let spans: Vec<(usize, usize, Option<u32>)> = haps[0]
        .context
        .locations
        .iter()
        .map(|(from, to)| (*from, *to, None))
        .collect();
    assert!(!spans.is_empty(), "the hap carries a source location");
    state.set_flash_ranges_from_eval(&spans);

    let flashes = state.flash_ranges();
    assert!(!flashes.is_empty(), "flash ranges reach the editor");
    for (from, to, _) in &flashes {
        assert!(*to <= src.len(), "flash range {from}..{to} is in bounds");
        assert_eq!(
            &src[*from..*to],
            "bd",
            "the first hap flashes the `bd` leaf, not the whole string"
        );
    }

    // Flash ranges are remapped across edits like every other decoration, so a
    // highlight does not smear onto the wrong text after a keystroke.
    let edited = format!("// x\n{src}");
    state.map_change(TextChange::from_texts(src, &edited).unwrap());
    for (from, to, _) in state.flash_ranges() {
        assert_eq!(&edited[from..to], "bd");
    }
}

#[test]
fn highlighting_covers_the_buffer_and_survives_a_live_update() {
    // The tokenizer is what paints the editor every frame; it must tile the
    // input exactly (no dropped or duplicated bytes) for any buffer, including
    // one mid-edit.
    for src in [
        r#"s("bd sd").gain(0.5) // a comment"#,
        "note(\"c a f e\")\n  .lpf(sine.range(200, 2000))",
        r#"s("bd*<2 3>").room(.4)"#,
        "s(\"bd", // unterminated, as it is while you type
        "",
    ] {
        let tokens = tokenize(src, &HashSet::new());
        let mut cursor = 0usize;
        for (from, to, _) in &tokens {
            assert_eq!(*from, cursor, "gap or overlap in {src:?}");
            assert!(*to <= src.len(), "out of bounds in {src:?}");
            cursor = *to;
        }
        assert_eq!(cursor, src.len(), "tokens must cover all of {src:?}");
    }
}
