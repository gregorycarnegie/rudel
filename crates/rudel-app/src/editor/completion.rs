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
        .map(|name| CompletionItem::new(name, CompletionKind::Sound))
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
        .map(|name| CompletionItem::new(name, CompletionKind::Bank))
        .collect();
    Some(non_empty_result(ctx.absolute_inside_start, cursor, items))
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

    let start = suffix_start(&ctx.inside, |ch| {
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
    let ident_start = left
        .char_indices()
        .rev()
        .find(|(_, ch)| !(ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '$'))
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);
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

fn suffix_start(text: &str, allowed: impl Fn(char) -> bool) -> usize {
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
    for name in &catalog.reference.methods {
        insert_item(&mut items, name, CompletionKind::Method, "pattern method");
    }
    for name in &catalog.reference.controls {
        insert_item(&mut items, name, CompletionKind::Control, "control name");
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

fn item_for_word(word: &str, catalog: &CompletionCatalog<'_>) -> Option<CompletionItem> {
    fallback_items(catalog)
        .into_iter()
        .find(|item| item.label == word)
        .or_else(|| {
            sound_names(catalog)
                .into_iter()
                .find(|name| name == word)
                .map(|name| {
                    CompletionItem::new(name, CompletionKind::Sound).with_detail("sound name")
                })
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
}
