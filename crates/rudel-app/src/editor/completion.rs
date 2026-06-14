use super::{
    highlight::{Token, tokenize},
    text::char_index_at_byte,
};
use eframe::egui;
use std::collections::HashSet;

const MAX_COMPLETIONS: usize = 12;

/// The active autocomplete popup: the byte range of the prefix being replaced,
/// the candidate names, and which one is selected. Stored in egui temp memory
/// between frames.
#[derive(Clone, Default)]
pub(super) struct Completion {
    pub(super) start: usize,
    pub(super) items: Vec<String>,
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
                ui.set_max_width(220.0);
                for (i, item) in state.items.iter().enumerate() {
                    let _ = ui.selectable_label(
                        i == state.selected,
                        egui::RichText::new(item).monospace(),
                    );
                }
            });
        });
}

/// Replace the prefix bytes `start..cursor` with the accepted `item`, returning
/// the new char cursor index just after the inserted word.
pub(super) fn apply_completion(
    code: &mut String,
    start: usize,
    cursor: usize,
    item: &str,
) -> usize {
    code.replace_range(start..cursor, item);
    char_index_at_byte(code, start + item.len())
}

/// Autocomplete at byte cursor `cursor`: the byte range of the identifier
/// prefix being typed and the matching names from `idents`, or `None` when
/// there is no code identifier to complete (empty prefix, inside a string or
/// comment, or no longer/other match).
pub(super) fn completion_at(
    code: &str,
    cursor: usize,
    idents: &HashSet<String>,
) -> Option<(usize, usize, Vec<String>)> {
    let bytes = code.as_bytes();
    if cursor > bytes.len() {
        return None;
    }
    let mut start = cursor;
    while start > 0 {
        let b = bytes[start - 1];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'$' {
            start -= 1;
        } else {
            break;
        }
    }
    // Must be a non-empty prefix that begins like an identifier (not a number).
    if start == cursor
        || !(bytes[start].is_ascii_alphabetic() || matches!(bytes[start], b'_' | b'$'))
    {
        return None;
    }
    if in_string_or_comment(code, start, idents) {
        return None;
    }
    let prefix = &code[start..cursor];
    let mut items: Vec<String> = idents
        .iter()
        .filter(|name| name.len() > prefix.len() && name.starts_with(prefix))
        .cloned()
        .collect();
    if items.is_empty() {
        return None;
    }
    items.sort();
    items.truncate(MAX_COMPLETIONS);
    Some((start, cursor, items))
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
    fn completion_matches_identifier_prefix() {
        let idents: HashSet<String> = ["note", "n", "stack", "slow", "fast"]
            .into_iter()
            .map(str::to_string)
            .collect();
        // `s` + "to" -> stack/slow (sorted), replacing bytes 0..2
        let (start, end, items) = completion_at("st", 2, &idents).unwrap();
        assert_eq!((start, end), (0, 2));
        assert_eq!(items, vec!["stack".to_string()]);
        // `sl`/`st` distinguish; `s` matches several
        let (_, _, items) = completion_at("s", 1, &idents).unwrap();
        assert_eq!(items, vec!["slow", "stack"]);
        // exact full word offers nothing more
        assert_eq!(completion_at("note", 4, &idents), None);
        // empty prefix / not on an identifier
        assert_eq!(completion_at("note(", 5, &idents), None);
        // cursor mid-expression completes the word under it
        let (start, end, items) = completion_at("note(fa", 7, &idents).unwrap();
        assert_eq!((start, end), (5, 7));
        assert_eq!(items, vec!["fast".to_string()]);
    }

    #[test]
    fn accepting_completion_replaces_the_prefix() {
        let mut code = "note(fa".to_string();
        let cursor = apply_completion(&mut code, 5, 7, "fast");
        assert_eq!(code, "note(fast");
        assert_eq!(cursor, 9);
        // mid-buffer replacement keeps the tail
        let mut code = "x st y".to_string();
        let cursor = apply_completion(&mut code, 2, 4, "stack");
        assert_eq!(code, "x stack y");
        assert_eq!(cursor, 7);
    }

    #[test]
    fn completion_skips_strings_and_comments() {
        let idents: HashSet<String> = ["bd", "stack"].into_iter().map(str::to_string).collect();
        // inside a mini-notation string: no code completion
        assert_eq!(completion_at(r#"s("bd"#, 5, &idents), None);
        // inside a comment
        assert_eq!(completion_at("// st", 5, &idents), None);
    }
}
