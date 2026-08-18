use super::{
    highlight::{Token, tokenize},
    text::char_index_at_byte,
};
use crate::reference::{DRUMS, LANGUAGE_KEYWORDS, WAVEFORMS};
use eframe::egui::{
    self,
    text::{ByteIndex, CharIndex},
};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const MAX_COMPLETIONS: usize = 12;
const PITCH_NAMES: &[&str] = &[
    "C", "C#", "Db", "D", "D#", "Eb", "E", "E#", "Fb", "F", "F#", "Gb", "G", "G#", "Ab", "A", "A#",
    "Bb", "B", "B#", "Cb",
];
const MODE_NAMES: &[&str] = &["below", "above", "duck", "root"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CompletionKind {
    Function,
    Method,
    Control,
    Keyword,
    Sound,
    Bank,
    ChordSymbol,
    Scale,
    Mode,
    Pitch,
}

impl CompletionKind {
    fn label(self) -> &'static str {
        match self {
            CompletionKind::Function => "function",
            CompletionKind::Method => "method",
            CompletionKind::Control => "control",
            CompletionKind::Keyword => "keyword",
            CompletionKind::Sound => "sound",
            CompletionKind::Bank => "bank",
            CompletionKind::ChordSymbol => "chord",
            CompletionKind::Scale => "scale",
            CompletionKind::Mode => "mode",
            CompletionKind::Pitch => "pitch",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CompletionItem {
    pub(super) label: String,
    apply: String,
    kind: CompletionKind,
    detail: Option<String>,
}

impl CompletionItem {
    fn new(label: impl Into<String>, kind: CompletionKind) -> Self {
        let label = label.into();
        Self {
            apply: label.clone(),
            label,
            kind,
            detail: None,
        }
    }

    fn with_apply(
        label: impl Into<String>,
        apply: impl Into<String>,
        kind: CompletionKind,
    ) -> Self {
        Self {
            label: label.into(),
            apply: apply.into(),
            kind,
            detail: None,
        }
    }

    fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

pub(super) struct CompletionCatalog<'a> {
    pub(super) idents: &'a HashSet<String>,
    pub(super) reference: &'a rudel_lang::Reference,
    pub(super) sample_names: &'a [String],
}

/// The active autocomplete popup: the byte range of the prefix being replaced,
/// the candidate names, and which one is selected. Stored in egui temp memory
/// between frames.
#[derive(Clone, Default)]
pub(super) struct Completion {
    pub(super) start: ByteIndex,
    pub(super) items: Vec<CompletionItem>,
    pub(super) selected: usize,
}

/// Draw the autocomplete suggestions just below the editor, with the selected
/// row highlighted. Keyboard-driven (Tab/Enter accept, arrows navigate, Esc
/// dismiss); see `code_editor`.
pub(super) fn completion_popup(
    ui: &egui::Ui,
    id: egui::Id,
    response: &egui::Response,
    state: &Completion,
) {
    egui::Area::new(id.with("popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(response.rect.left_bottom())
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(300.0);
                for (i, item) in state.items.iter().enumerate() {
                    let text = format!("{}  {}", item.label, item.kind.label());
                    let response = ui.selectable_label(
                        i == state.selected,
                        egui::RichText::new(text).monospace(),
                    );
                    if let Some(detail) = &item.detail {
                        response.on_hover_text(detail);
                    }
                }
            });
        });
}

pub(super) fn completion_tooltip(
    ui: &egui::Ui,
    id: egui::Id,
    response: &egui::Response,
    item: &CompletionItem,
) {
    egui::Area::new(id.with("tooltip"))
        .order(egui::Order::Tooltip)
        .fixed_pos(response.rect.right_top() + egui::vec2(8.0, 0.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(280.0);
                ui.label(egui::RichText::new(&item.label).monospace().strong());
                ui.weak(item.kind.label());
                if let Some(detail) = &item.detail {
                    ui.separator();
                    ui.label(detail);
                }
            });
        });
}

/// Replace the prefix bytes `start..cursor` with the accepted item, returning
/// the new char cursor index just after the inserted text.
pub(super) fn apply_completion(
    code: &mut String,
    start: ByteIndex,
    cursor: ByteIndex,
    item: &CompletionItem,
) -> CharIndex {
    code.replace_range(start.0..cursor.0, &item.apply);
    char_index_at_byte(code, start + item.apply.len())
}

/// Autocomplete at byte cursor `cursor`, matching Strudel's handler order:
/// sounds/banks/chords/scales/modes inside quoted arguments, then documented
/// runtime identifiers as the fallback.
pub(super) fn completion_at(
    code: &str,
    cursor: ByteIndex,
    catalog: &CompletionCatalog<'_>,
) -> Option<(ByteIndex, ByteIndex, Vec<CompletionItem>)> {
    completion_at_bytes(code, cursor.0, catalog)
        .map(|(start, end, items)| (ByteIndex(start), ByteIndex(end), items))
}

/// Byte-domain implementation of [`completion_at`]; everything below works in
/// plain `usize` byte offsets (no char indices in sight).
fn completion_at_bytes(
    code: &str,
    cursor: usize,
    catalog: &CompletionCatalog<'_>,
) -> Option<(usize, usize, Vec<CompletionItem>)> {
    if cursor > code.len() {
        return None;
    }

    if let Some(result) = sound_completion(code, cursor, catalog) {
        return result;
    }
    if let Some(result) = bank_completion(code, cursor, catalog) {
        return result;
    }
    if let Some(result) = control_completion(code, cursor, catalog) {
        return result;
    }
    if let Some(result) = chord_completion(code, cursor) {
        return result;
    }
    if let Some(result) = scale_completion(code, cursor) {
        return result;
    }
    if let Some(result) = mode_completion(code, cursor) {
        return result;
    }
    fallback_completion(code, cursor, catalog)
}

pub(super) fn reference_tooltip_at(
    code: &str,
    cursor: ByteIndex,
    catalog: &CompletionCatalog<'_>,
) -> Option<CompletionItem> {
    let (_, _, word) = word_at_cursor(code, cursor.0)?;
    item_for_word(&word, catalog)
}

fn sound_completion(
    code: &str,
    cursor: usize,
    catalog: &CompletionCatalog<'_>,
) -> Option<Option<(usize, usize, Vec<CompletionItem>)>> {
    let ctx = quoted_arg_context(code, cursor, &["s", "sound"])?;
    let start = fragment_start(&ctx.inside, |ch| ch.is_ascii_alphanumeric() || ch == '_');
    let fragment = &ctx.inside[start..];
    let items = sound_names(catalog)
        .into_iter()
        .filter(|name| name.contains(fragment))
        .map(sound_item)
        .collect();
    Some(non_empty_result(
        ctx.absolute_inside_start + start,
        cursor,
        items,
    ))
}

fn bank_completion(
    code: &str,
    cursor: usize,
    catalog: &CompletionCatalog<'_>,
) -> Option<Option<(usize, usize, Vec<CompletionItem>)>> {
    let ctx = quoted_arg_context(code, cursor, &["bank"])?;
    let fragment = ctx.inside.as_str();
    let items = bank_names(catalog)
        .into_iter()
        .filter(|name| name.starts_with(fragment))
        .map(|name| {
            CompletionItem::new(name, CompletionKind::Bank)
                .with_detail("sample bank prefix from loaded samples")
        })
        .collect();
    Some(non_empty_result(ctx.absolute_inside_start, cursor, items))
}

fn control_completion(
    code: &str,
    cursor: usize,
    catalog: &CompletionCatalog<'_>,
) -> Option<Option<(usize, usize, Vec<CompletionItem>)>> {
    let ctx = quoted_arg_context(code, cursor, &["ctrl", "as"])?;
    let start = fragment_start(&ctx.inside, |ch| ch.is_ascii_alphanumeric() || ch == '_');
    let fragment = &ctx.inside[start..];
    let items = control_items(catalog)
        .into_iter()
        .filter(|item| item.label.starts_with(fragment))
        .collect();
    Some(non_empty_result(
        ctx.absolute_inside_start + start,
        cursor,
        items,
    ))
}

fn chord_completion(
    code: &str,
    cursor: usize,
) -> Option<Option<(usize, usize, Vec<CompletionItem>)>> {
    let ctx = quoted_arg_context(code, cursor, &["chord"])?;
    let start = fragment_start(&ctx.inside, |ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '#' | 'b' | '+' | '^' | ':' | '-')
    });
    let fragment = &ctx.inside[start..];
    let absolute = ctx.absolute_inside_start + start;

    let (root, symbol_fragment) = chord_root_and_symbol_fragment(fragment);
    let items = if root.is_some() {
        chord_symbol_items(symbol_fragment)
    } else {
        pitch_items(fragment)
    };
    let from = if root.is_some() {
        cursor - symbol_fragment.len()
    } else {
        absolute
    };
    Some(non_empty_result(from, cursor, items))
}

fn scale_completion(
    code: &str,
    cursor: usize,
) -> Option<Option<(usize, usize, Vec<CompletionItem>)>> {
    let ctx = quoted_arg_context(code, cursor, &["scale"])?;
    if let Some(colon) = ctx.inside.rfind(':') {
        let fragment = &ctx.inside[colon + 1..];
        let items = rudel_core::scale_names()
            .iter()
            .copied()
            .filter(|name| name.starts_with(fragment))
            .map(|name| {
                CompletionItem::with_apply(name, name.replace(' ', ":"), CompletionKind::Scale)
            })
            .collect();
        return Some(non_empty_result(
            ctx.absolute_inside_start + colon + 1,
            cursor,
            items,
        ));
    }

    let start = fragment_start(&ctx.inside, |ch| {
        ch.is_ascii_alphabetic() || matches!(ch, '#' | 'b')
    });
    let fragment = &ctx.inside[start..];
    Some(non_empty_result(
        ctx.absolute_inside_start + start,
        cursor,
        pitch_items(fragment),
    ))
}

fn mode_completion(
    code: &str,
    cursor: usize,
) -> Option<Option<(usize, usize, Vec<CompletionItem>)>> {
    let ctx = quoted_arg_context(code, cursor, &["mode"])?;
    if let Some(colon) = ctx.inside.rfind(':') {
        let fragment = &ctx.inside[colon + 1..];
        return Some(non_empty_result(
            ctx.absolute_inside_start + colon + 1,
            cursor,
            pitch_items(fragment),
        ));
    }

    let start = fragment_start(&ctx.inside, |ch| ch.is_ascii_alphanumeric() || ch == ':');
    let fragment = &ctx.inside[start..];
    let items = MODE_NAMES
        .iter()
        .copied()
        .filter(|name| name.starts_with(fragment))
        .map(|name| CompletionItem::new(name, CompletionKind::Mode))
        .collect();
    Some(non_empty_result(
        ctx.absolute_inside_start + start,
        cursor,
        items,
    ))
}

fn fallback_completion(
    code: &str,
    cursor: usize,
    catalog: &CompletionCatalog<'_>,
) -> Option<(usize, usize, Vec<CompletionItem>)> {
    let (start, end, prefix) = word_at_cursor(code, cursor)?;
    if start == end
        || !(prefix
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'_' | b'$')))
    {
        return None;
    }
    if in_string_or_comment(code, start, catalog.idents) {
        return None;
    }

    let mut items: Vec<_> = fallback_items(catalog)
        .into_iter()
        .filter(|item| item.label.len() > prefix.len() && item.label.starts_with(&prefix))
        .collect();
    items.sort_by(|a, b| {
        a.label
            .cmp(&b.label)
            .then(a.kind.label().cmp(b.kind.label()))
    });
    items.truncate(MAX_COMPLETIONS);
    (!items.is_empty()).then_some((start, end, items))
}

fn non_empty_result(
    start: usize,
    end: usize,
    mut items: Vec<CompletionItem>,
) -> Option<(usize, usize, Vec<CompletionItem>)> {
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.dedup_by(|a, b| a.label == b.label && a.apply == b.apply);
    items.truncate(MAX_COMPLETIONS);
    (!items.is_empty()).then_some((start, end, items))
}

struct QuotedArgContext {
    inside: String,
    absolute_inside_start: usize,
}

fn quoted_arg_context(code: &str, cursor: usize, names: &[&str]) -> Option<QuotedArgContext> {
    let before = code.get(..cursor)?;
    let quote = before
        .char_indices()
        .rev()
        .find(|(_, ch)| matches!(ch, '"' | '\''))?;
    let quote_idx = quote.0;
    let quote_ch = quote.1;
    let inside = &before[quote_idx + quote_ch.len_utf8()..];
    if inside.contains(quote_ch) {
        return None;
    }

    let left = before[..quote_idx].trim_end();
    let left = left.strip_suffix('(')?.trim_end();
    let ident_start = fragment_start(left, |ch| {
        ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
    });
    let name = &left[ident_start..];
    names.contains(&name).then(|| QuotedArgContext {
        inside: inside.to_string(),
        absolute_inside_start: quote_idx + quote_ch.len_utf8(),
    })
}

fn fragment_start(text: &str, allowed: impl Fn(char) -> bool) -> usize {
    text.char_indices()
        .rev()
        .find(|(_, ch)| !allowed(*ch))
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0)
}

fn word_at_cursor(code: &str, cursor: usize) -> Option<(usize, usize, String)> {
    if cursor > code.len() {
        return None;
    }
    let bytes = code.as_bytes();
    let mut start = cursor;
    while start > 0 {
        let b = bytes[start - 1];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'$' {
            start -= 1;
        } else {
            break;
        }
    }
    let mut end = cursor;
    while end < bytes.len() {
        let b = bytes[end];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'$' {
            end += 1;
        } else {
            break;
        }
    }
    (start < end).then(|| (start, end, code[start..end].to_string()))
}

fn sound_names(catalog: &CompletionCatalog<'_>) -> Vec<String> {
    let mut names = BTreeSet::new();
    names.extend(WAVEFORMS.iter().copied().map(str::to_string));
    names.extend(DRUMS.iter().copied().map(str::to_string));
    names.extend(catalog.sample_names.iter().cloned());
    names.into_iter().collect()
}

fn bank_names(catalog: &CompletionCatalog<'_>) -> Vec<String> {
    let mut banks = BTreeSet::new();
    for name in catalog.sample_names {
        if let Some((bank, suffix)) = name.split_once('_')
            && !bank.is_empty()
            && !suffix.is_empty()
        {
            banks.insert(bank.to_string());
        }
    }
    banks.into_iter().collect()
}

fn pitch_items(fragment: &str) -> Vec<CompletionItem> {
    let fragment = fragment.to_ascii_lowercase();
    PITCH_NAMES
        .iter()
        .copied()
        .filter(|pitch| pitch.to_ascii_lowercase().starts_with(&fragment))
        .map(|pitch| CompletionItem::new(pitch, CompletionKind::Pitch))
        .collect()
}

fn chord_root_and_symbol_fragment(fragment: &str) -> (Option<&'static str>, &str) {
    for pitch in PITCH_NAMES {
        if fragment
            .to_ascii_lowercase()
            .starts_with(&pitch.to_ascii_lowercase())
        {
            return (Some(*pitch), &fragment[pitch.len()..]);
        }
    }
    (None, fragment)
}

fn chord_symbol_items(fragment: &str) -> Vec<CompletionItem> {
    rudel_core::chord_symbols()
        .iter()
        .copied()
        .filter_map(|symbol| {
            if symbol.is_empty() {
                fragment
                    .is_empty()
                    .then(|| CompletionItem::with_apply("major", "", CompletionKind::ChordSymbol))
            } else {
                symbol
                    .starts_with(fragment)
                    .then(|| CompletionItem::new(symbol, CompletionKind::ChordSymbol))
            }
        })
        .collect()
}

fn fallback_items(catalog: &CompletionCatalog<'_>) -> Vec<CompletionItem> {
    let mut items: BTreeMap<String, CompletionItem> = BTreeMap::new();
    for name in &catalog.reference.functions {
        insert_item(
            &mut items,
            name,
            CompletionKind::Function,
            "runtime function or value",
        );
    }
    for name in &catalog.reference.controls {
        insert_control_item(&mut items, name);
    }
    for name in &catalog.reference.methods {
        insert_item(&mut items, name, CompletionKind::Method, "pattern method");
    }
    for name in LANGUAGE_KEYWORDS {
        insert_item(
            &mut items,
            name,
            CompletionKind::Keyword,
            "Koto language keyword",
        );
    }
    items.into_values().collect()
}

fn insert_item(
    items: &mut BTreeMap<String, CompletionItem>,
    name: &str,
    kind: CompletionKind,
    detail: &str,
) {
    if is_hidden_completion_name(name) {
        return;
    }
    items
        .entry(name.to_string())
        .or_insert_with(|| CompletionItem::new(name, kind).with_detail(detail));
}

fn insert_control_item(items: &mut BTreeMap<String, CompletionItem>, name: &str) {
    if is_hidden_completion_name(name) {
        return;
    }
    items
        .entry(name.to_string())
        .or_insert_with(|| control_item(name));
}

fn control_items(catalog: &CompletionCatalog<'_>) -> Vec<CompletionItem> {
    catalog
        .reference
        .controls
        .iter()
        .filter(|name| !is_hidden_completion_name(name))
        .map(|name| control_item(name))
        .collect()
}

fn control_item(name: &str) -> CompletionItem {
    CompletionItem::new(name, CompletionKind::Control).with_detail(control_detail(name))
}

fn control_detail(name: &str) -> String {
    let key = rudel_core::control_name(name);
    if key != name {
        return format!("alias for `{key}` control");
    }
    match name {
        "s" => "sound selector; supports name:index".to_string(),
        "n" => "sample index / numeric value".to_string(),
        "note" => "pitch name or MIDI note".to_string(),
        "gain" => "amplitude multiplier".to_string(),
        "pan" => "stereo position".to_string(),
        "speed" => "sample playback-rate multiplier".to_string(),
        "bank" => "sample bank prefix".to_string(),
        _ => format!("sets `{name}` control"),
    }
}

fn sound_item(name: impl Into<String>) -> CompletionItem {
    let name = name.into();
    let detail = if WAVEFORMS.contains(&name.as_str()) {
        "built-in synth/noise waveform"
    } else if DRUMS.contains(&name.as_str()) {
        "built-in drum synth"
    } else {
        "loaded sample sound"
    };
    CompletionItem::new(name, CompletionKind::Sound).with_detail(detail)
}

fn item_for_word(word: &str, catalog: &CompletionCatalog<'_>) -> Option<CompletionItem> {
    fallback_items(catalog)
        .into_iter()
        .find(|item| item.label == word)
        .or_else(|| {
            sound_names(catalog)
                .into_iter()
                .find(|name| name == word)
                .map(sound_item)
        })
}

fn is_hidden_completion_name(name: &str) -> bool {
    name.is_empty() || name.starts_with('_')
}

/// True when byte `pos` falls inside a string literal or `//` comment, where
/// identifier completion should not fire (those are mini-notation / prose).
fn in_string_or_comment(code: &str, pos: usize, idents: &HashSet<String>) -> bool {
    tokenize(code, idents)
        .into_iter()
        .any(|(start, end, token)| {
            start <= pos
                && pos < end
                && matches!(
                    token,
                    Token::Str | Token::MiniWord | Token::MiniOp | Token::MiniRest | Token::Comment
                )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_runs_over_identifier_characters_on_both_sides() {
        // The replaced span for a completion. `_` and `$` are identifier
        // characters here, so a word must not stop at either of them.
        assert_eq!(
            word_at_cursor("a_b$c", 3),
            Some((0, 5, "a_b$c".to_string())),
            "grows in both directions"
        );
        assert_eq!(word_at_cursor("x = ab", 6), Some((4, 6, "ab".to_string())));
        assert_eq!(word_at_cursor("ab", 0), Some((0, 2, "ab".to_string())));
        // Not on whitespace, and not past the end of the buffer.
        assert_eq!(word_at_cursor("a  b", 2), None, "between words");
        assert_eq!(word_at_cursor("ab", 99), None, "out of range");
    }

    #[test]
    fn a_fragment_starts_after_the_last_character_it_may_not_contain() {
        // Where the completion's replacement begins: the run of allowed
        // characters ending at the cursor.
        let alpha = |ch: char| char::is_ascii_alphabetic(&ch);
        assert_eq!(fragment_start("c#4x", alpha), 3, "after the digit");
        assert_eq!(fragment_start("abc", alpha), 0, "all of it is allowed");
        assert_eq!(fragment_start("abc#", alpha), 4, "nothing at the end is");
        assert_eq!(fragment_start("", alpha), 0);
        // Counted in bytes, so a multi-byte character before the fragment has
        // to be stepped over whole.
        assert_eq!(fragment_start("é4x", alpha), 3, "'é' is two bytes");
    }

    #[test]
    fn the_result_drops_duplicates_but_keeps_distinct_items() {
        // Two entries collapse only when they would both insert the same text
        // under the same label — the scale list has several names sharing a
        // label with different `apply` spellings.
        let item = |label: &str, apply: &str| {
            CompletionItem::with_apply(label, apply, CompletionKind::Scale)
        };
        let spread = non_empty_result(2, 5, vec![item("a", "x"), item("a", "y"), item("b", "z")])
            .expect("some items");
        assert_eq!(spread.0, 2, "the span is passed through");
        assert_eq!(spread.1, 5);
        assert_eq!(
            spread
                .2
                .iter()
                .map(|i| (i.label.as_str(), i.apply.as_str()))
                .collect::<Vec<_>>(),
            vec![("a", "x"), ("a", "y"), ("b", "z")],
            "only exact duplicates merge"
        );

        let merged =
            non_empty_result(0, 1, vec![item("a", "x"), item("a", "x")]).expect("some items");
        assert_eq!(merged.2.len(), 1, "an exact duplicate is dropped");
        assert!(non_empty_result(0, 1, vec![]).is_none(), "nothing to show");
    }

    #[test]
    fn a_scale_argument_completes_its_type_after_the_colon() {
        //        0123456789
        let code = r#"scale("c:maj")"#;
        let got = scale_completion(code, 12).expect("in a scale argument");
        let (start, end, items) = got.expect("some scale names");
        assert_eq!((start, end), (9, 12), "replaces just the type fragment");
        assert!(
            items.iter().any(|i| i.label == "major"),
            "{:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
        // A two-word scale name is applied with a `:` between the words, which
        // is how mini-notation spells it.
        let got = scale_completion(r#"scale("c:harmonic")"#, 17)
            .expect("in a scale argument")
            .expect("some scale names");
        assert!(
            got.2.iter().any(|i| i.apply == "harmonic:minor"),
            "{:?}",
            got.2.iter().map(|i| &i.apply).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_mode_argument_completes_from_the_last_word() {
        let (start, end, items) = mode_completion(r#"mode("x ro")"#, 10)
            .expect("in a mode argument")
            .expect("some mode names");
        assert_eq!(
            (start, end),
            (8, 10),
            "replaces only the word under the cursor"
        );
        assert!(
            items.iter().any(|i| i.label == "root"),
            "{:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );

        // After a colon the argument takes a note, not another mode name.
        let (start, end, items) = mode_completion(r#"mode("below:c")"#, 13)
            .expect("in a mode argument")
            .expect("some pitches");
        assert_eq!((start, end), (12, 13), "replaces the note fragment");
        assert!(items.iter().all(|i| i.kind != CompletionKind::Mode));
    }

    #[test]
    fn the_common_controls_document_themselves() {
        // The popup's detail line. Each of these has wording of its own; the
        // catch-all is only for controls with nothing better to say.
        for name in ["s", "n", "note", "gain", "pan", "speed", "bank"] {
            let detail = control_detail(name);
            assert!(
                !detail.contains(&format!("sets `{name}` control")),
                "{name} should have its own description, got {detail:?}"
            );
            assert!(!detail.is_empty());
        }
        assert_eq!(
            control_detail("crush"),
            "sets `crush` control",
            "and anything else falls back"
        );
    }

    fn reference(names: &[&str]) -> rudel_lang::Reference {
        rudel_lang::Reference {
            functions: names.iter().map(|name| name.to_string()).collect(),
            methods: vec!["slow".to_string(), "_spiral".to_string()],
            controls: vec!["gain".to_string(), "bank".to_string()],
        }
    }

    fn catalog<'a>(
        reference: &'a rudel_lang::Reference,
        idents: &'a HashSet<String>,
        sample_names: &'a [String],
    ) -> CompletionCatalog<'a> {
        CompletionCatalog {
            idents,
            reference,
            sample_names,
        }
    }

    fn labels(items: Vec<CompletionItem>) -> Vec<String> {
        items.into_iter().map(|item| item.label).collect()
    }

    #[test]
    fn completion_matches_identifier_prefix() {
        let reference = reference(&["note", "n", "stack", "fast"]);
        let idents: HashSet<String> = ["note", "n", "stack", "slow", "fast", "gain"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let sample_names = Vec::new();
        let catalog = catalog(&reference, &idents, &sample_names);

        let (start, end, items) = completion_at_bytes("st", 2, &catalog).unwrap();
        assert_eq!((start, end), (0, 2));
        assert_eq!(labels(items), vec!["stack".to_string()]);

        let (_, _, items) = completion_at_bytes("s", 1, &catalog).unwrap();
        assert_eq!(labels(items), vec!["slow", "stack"]);
        assert_eq!(completion_at_bytes("note", 4, &catalog), None);
        assert_eq!(completion_at_bytes("note(", 5, &catalog), None);

        let (start, end, items) = completion_at_bytes("note(fa", 7, &catalog).unwrap();
        assert_eq!((start, end), (5, 7));
        assert_eq!(labels(items), vec!["false", "fast"]);
    }

    #[test]
    fn accepting_completion_uses_apply_text() {
        let item = CompletionItem::new("fast", CompletionKind::Function);
        let mut code = "note(fa".to_string();
        let cursor = apply_completion(&mut code, ByteIndex(5), ByteIndex(7), &item);
        assert_eq!(code, "note(fast");
        assert_eq!(cursor, CharIndex(9));

        let major = CompletionItem::with_apply("major", "", CompletionKind::ChordSymbol);
        let mut code = r#"chord("C"#.to_string();
        let cursor = apply_completion(&mut code, ByteIndex(8), ByteIndex(8), &major);
        assert_eq!(code, r#"chord("C"#);
        assert_eq!(cursor, CharIndex(8));
    }

    #[test]
    fn fallback_completion_skips_strings_comments_and_hidden_docs() {
        let reference = reference(&["stack"]);
        let idents: HashSet<String> = ["bd", "stack", "_spiral"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let sample_names = Vec::new();
        let catalog = catalog(&reference, &idents, &sample_names);

        assert_eq!(completion_at_bytes("// st", 5, &catalog), None);
        assert_eq!(completion_at_bytes("_sp", 3, &catalog), None);
    }

    #[test]
    fn sound_completion_matches_builtins_and_loaded_samples_inside_s() {
        let reference = reference(&[]);
        let idents = HashSet::new();
        let sample_names = vec!["RolandTR909_bd".to_string(), "tabla".to_string()];
        let catalog = catalog(&reference, &idents, &sample_names);

        let (start, end, items) = completion_at_bytes(r#"s("ta"#, 5, &catalog).unwrap();
        assert_eq!((start, end), (3, 5));
        assert_eq!(labels(items), vec!["tabla"]);

        let (_, _, items) = completion_at_bytes(r#"sound("[b"#, 9, &catalog).unwrap();
        assert!(labels(items).contains(&"bd".to_string()));
    }

    #[test]
    fn sound_and_control_completions_include_useful_hints() {
        let reference = rudel_lang::Reference {
            functions: vec![],
            methods: vec!["gain".to_string()],
            controls: vec!["gain".to_string(), "lpf".to_string(), "clip".to_string()],
        };
        let idents = HashSet::new();
        let sample_names = vec!["tabla".to_string()];
        let catalog = catalog(&reference, &idents, &sample_names);

        let (_, _, items) = completion_at_bytes(r#"s("tab"#, 6, &catalog).unwrap();
        assert_eq!(items[0].label, "tabla");
        assert_eq!(items[0].detail.as_deref(), Some("loaded sample sound"));

        let (_, _, items) = completion_at_bytes(r#"note("c").ctrl("lp"#, 18, &catalog).unwrap();
        assert_eq!(items[0].label, "lpf");
        assert_eq!(
            items[0].detail.as_deref(),
            Some("alias for `cutoff` control")
        );

        let (_, _, items) =
            completion_at_bytes(r#"pat("c:.5").as("note:cl"#, 23, &catalog).unwrap();
        assert_eq!(items[0].label, "clip");
        assert_eq!(items[0].detail.as_deref(), Some("sets `clip` control"));

        let (_, _, items) = completion_at_bytes("ga", 2, &catalog).unwrap();
        assert_eq!(items[0].kind, CompletionKind::Control);
        assert_eq!(items[0].detail.as_deref(), Some("amplitude multiplier"));
    }

    #[test]
    fn bank_completion_derives_bank_names_from_loaded_samples() {
        let reference = reference(&[]);
        let idents = HashSet::new();
        let sample_names = vec!["RolandTR909_bd".to_string(), "tabla".to_string()];
        let catalog = catalog(&reference, &idents, &sample_names);

        let (start, end, items) = completion_at_bytes(r#"bank("Ro"#, 8, &catalog).unwrap();
        assert_eq!((start, end), (6, 8));
        assert_eq!(labels(items), vec!["RolandTR909"]);
    }

    #[test]
    fn scale_mode_and_chord_contexts_follow_strudel_handlers() {
        let reference = reference(&[]);
        let idents = HashSet::new();
        let sample_names = Vec::new();
        let catalog = catalog(&reference, &idents, &sample_names);

        let (_, _, items) = completion_at_bytes(r#"scale("C:har"#, 12, &catalog).unwrap();
        assert_eq!(items[0].label, "harmonic minor");
        assert_eq!(items[0].apply, "harmonic:minor");

        let (_, _, items) = completion_at_bytes(r#"mode("be"#, 8, &catalog).unwrap();
        assert_eq!(labels(items), vec!["below"]);

        let (start, end, items) = completion_at_bytes(r#"chord("Am"#, 9, &catalog).unwrap();
        assert_eq!((start, end), (8, 9));
        assert!(labels(items).contains(&"m".to_string()));
    }

    #[test]
    fn tooltip_finds_reference_items_at_cursor() {
        let reference = reference(&["stack"]);
        let idents: HashSet<String> = ["stack"].into_iter().map(str::to_string).collect();
        let sample_names = Vec::new();
        let catalog = catalog(&reference, &idents, &sample_names);

        let item = reference_tooltip_at("stack(s(\"bd\"))", ByteIndex(2), &catalog).unwrap();
        assert_eq!(item.label, "stack");
        assert_eq!(item.kind, CompletionKind::Function);
    }
    #[test]
    fn each_kind_names_itself_in_the_popup() {
        // The label sits beside every entry; a blank or wrong one is what the
        // user reads to tell a control from a function.
        for (kind, want) in [
            (CompletionKind::Function, "function"),
            (CompletionKind::Method, "method"),
            (CompletionKind::Control, "control"),
            (CompletionKind::Keyword, "keyword"),
            (CompletionKind::Sound, "sound"),
            (CompletionKind::Bank, "bank"),
            (CompletionKind::ChordSymbol, "chord"),
            (CompletionKind::Scale, "scale"),
            (CompletionKind::Mode, "mode"),
            (CompletionKind::Pitch, "pitch"),
        ] {
            assert_eq!(kind.label(), want);
        }
    }

    #[test]
    fn a_completion_replaces_from_the_start_of_the_fragment_it_matched() {
        // The span is what gets replaced when the entry is accepted. Every
        // one of these is `absolute_inside_start + start`, and the tests need
        // a fragment that is *not* at the start of the quotes, or an offset
        // added to nothing looks the same as one subtracted from it.
        let reference = reference(&["note"]);
        let idents = HashSet::new();
        let sample_names = vec!["bd".to_string(), "sd".to_string()];
        let catalog = catalog(&reference, &idents, &sample_names);

        // s("bd sd  — the second word is the fragment, at inside offset 3.
        let code = r#"s("bd sd"#;
        let (start, end, items) = completion_at_bytes(code, code.len(), &catalog).unwrap();
        assert_eq!((start, end), (6, code.len()), "replaces `sd`, not `bd sd`");
        assert!(labels(items).contains(&"sd".to_string()));

        // scale("C:maj — the fragment starts after the colon.
        let code = r#"scale("C:maj"#;
        let (start, end, _) = completion_at_bytes(code, code.len(), &catalog).unwrap();
        assert_eq!((start, end), (9, code.len()), "replaces `maj`");

        // chord("Cmaj — the root stays, the symbol is replaced.
        let code = r#"chord("Cmaj"#;
        let (start, end, _) = completion_at_bytes(code, code.len(), &catalog).unwrap();
        assert_eq!((start, end), (8, code.len()), "replaces `maj`, keeps `C`");
    }

    #[test]
    fn a_cursor_past_the_end_of_the_buffer_completes_nothing() {
        let reference = reference(&["abs"]);
        let idents: HashSet<String> = ["abs"].into_iter().map(str::to_string).collect();
        let sample_names = Vec::new();
        let catalog = catalog(&reference, &idents, &sample_names);
        assert_eq!(completion_at_bytes("ab", 99, &catalog), None);
        assert_eq!(completion_at_bytes("ab", 3, &catalog), None, "one past");
        assert!(
            completion_at_bytes("ab", 2, &catalog).is_some(),
            "at the end"
        );
    }

    #[test]
    fn identifier_completion_stays_out_of_strings_and_comments() {
        let idents: HashSet<String> = ["stack", "slow"].into_iter().map(str::to_string).collect();
        // Inside a mini-notation string the words are sounds, not identifiers.
        assert!(in_string_or_comment(r#"n("st")"#, 4, &idents));
        // And in a comment they are prose.
        assert!(in_string_or_comment("// st", 4, &idents));
        // Plain code is neither...
        assert!(!in_string_or_comment("st", 1, &idents));
        // ...and the position just past a string is outside it.
        let code = r#""a" + st"#;
        assert!(!in_string_or_comment(code, 7, &idents));
        assert!(in_string_or_comment(code, 1, &idents));
    }
    #[test]
    fn the_other_branch_of_each_quoted_completion_replaces_from_the_right_place() {
        // Chord and scale each have two shapes, and only one of them was
        // covered above: a chord *without* a root completes pitches, and a
        // scale *without* a colon completes the tonic.
        let reference = reference(&["note"]);
        let idents = HashSet::new();
        let sample_names = Vec::new();
        let catalog = catalog(&reference, &idents, &sample_names);

        // chord("Am  — nothing typed for the second chord yet, so the whole
        // pitch list, inserted at the cursor rather than over the first chord.
        let code = r#"chord("Am "#;
        let (start, end, items) = completion_at_bytes(code, code.len(), &catalog).unwrap();
        assert_eq!((start, end), (10, code.len()), "inserts, replaces nothing");
        assert!(!items.is_empty());

        // scale("c d — a pattern of tonics; the fragment is the last one.
        let code = r#"scale("c d"#;
        let (start, end, _) = completion_at_bytes(code, code.len(), &catalog).unwrap();
        assert_eq!((start, end), (9, code.len()), "replaces `d` only");

        // ctrl("bank ga — again a fragment that is not the first thing in
        // the quotes, so the offset is added to something.
        let code = r#"ctrl("bank ga"#;
        let (start, end, items) = completion_at_bytes(code, code.len(), &catalog).unwrap();
        assert_eq!((start, end), (11, code.len()), "replaces `ga` only");
        assert_eq!(labels(items), vec!["gain".to_string()]);
    }

    #[test]
    fn completion_at_reports_the_span_in_char_indices() {
        // The byte-domain worker is wrapped for callers that speak char
        // indices; the wrapper is what the editor actually calls.
        let reference = reference(&["stack"]);
        let idents: HashSet<String> = ["stack"].into_iter().map(str::to_string).collect();
        let sample_names = Vec::new();
        let catalog = catalog(&reference, &idents, &sample_names);
        let (start, end, items) = completion_at("st", ByteIndex(2), &catalog).unwrap();
        assert_eq!((start, end), (ByteIndex(0), ByteIndex(2)));
        assert_eq!(labels(items), vec!["stack".to_string()]);
        assert!(completion_at("", ByteIndex(0), &catalog).is_none());
    }

    #[test]
    fn a_cursor_at_the_very_start_still_completes() {
        // The out-of-range guard is one-sided: 0 is a position, not an
        // overrun.
        let reference = reference(&["abs"]);
        let idents: HashSet<String> = ["abs"].into_iter().map(str::to_string).collect();
        let sample_names = Vec::new();
        let catalog = catalog(&reference, &idents, &sample_names);
        assert!(completion_at_bytes("ab", 0, &catalog).is_some());
    }

    #[test]
    fn a_position_at_the_end_of_a_string_is_outside_it() {
        // Tokens are half-open: the closing quote's own position belongs to
        // the string, the one after it does not.
        let idents: HashSet<String> = ["st"].into_iter().map(str::to_string).collect();
        let code = r#""a" + st"#;
        assert!(in_string_or_comment(code, 1, &idents), "inside");
        assert!(!in_string_or_comment(code, 3, &idents), "just past it");
    }
    #[test]
    fn a_tooltip_is_offered_for_a_known_word_only() {
        let reference = reference(&["note"]);
        let idents = HashSet::new();
        let sample_names = vec!["bd".to_string()];
        let catalog = catalog(&reference, &idents, &sample_names);
        let word = |code: &str, at: usize| {
            reference_tooltip_at(code, ByteIndex(at), &catalog).map(|item| item.label)
        };
        assert_eq!(word("note", 2), Some("note".to_string()), "a function");
        assert_eq!(word("gain", 2), Some("gain".to_string()), "a control");
        assert_eq!(word("bd", 1), Some("bd".to_string()), "a loaded sound");
        // A word that is nothing in particular gets no tooltip, rather than
        // the first entry in the list.
        assert_eq!(word("wobble", 3), None);
    }

    #[test]
    fn a_quoted_argument_belongs_to_the_call_that_opens_it() {
        let reference = reference(&["note"]);
        let idents = HashSet::new();
        let sample_names = vec!["bd".to_string()];
        let catalog = catalog(&reference, &idents, &sample_names);
        // The name before the `(` decides; `$` and `_` are part of it, so a
        // call named `my_s` is not `s`.
        assert!(completion_at_bytes(r#"s("b"#, 4, &catalog).is_some());
        assert!(
            completion_at_bytes(r#"my_s("b"#, 7, &catalog).is_none(),
            "a different function's argument is not a sound list"
        );
        assert!(
            completion_at_bytes(r#"x$s("b"#, 6, &catalog).is_none(),
            "nor is `x$s`"
        );
    }
}
