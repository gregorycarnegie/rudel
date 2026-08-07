use super::{
    settings::EditorSettings,
    text::{
        char_at, char_slice, insert_text_at_char, line_end_at, line_start_at,
        next_line_start_after, replace_char_range,
    },
};
use eframe::egui::{self, text::CharIndex};

const CODE_INDENT: &str = "  ";

#[derive(Clone, Copy, Default)]
pub(super) struct EditorShortcuts {
    pub(super) comment_toggle: bool,
    pub(super) indent: bool,
    pub(super) outdent: bool,
    pub(super) jump_next: bool,
    pub(super) jump_prev: bool,
    pub(super) complete_accept: bool,
    pub(super) complete_next: bool,
    pub(super) complete_prev: bool,
    pub(super) complete_dismiss: bool,
}

pub(super) fn capture_editor_shortcuts(
    ui: &mut egui::Ui,
    editor_id: egui::Id,
    completion_active: bool,
    settings: &EditorSettings,
) -> EditorShortcuts {
    use egui::{Key, Modifiers};
    if !ui.memory(|m| m.has_focus(editor_id)) {
        return EditorShortcuts::default();
    }
    ui.input_mut(|i| {
        // When the completion popup is open, Tab/Enter accept it, the arrows
        // navigate it, and Esc dismisses it, taking priority over the normal
        // Tab-indent / Enter-newline behaviour.
        let (complete_accept, complete_next, complete_prev, complete_dismiss) = if completion_active
        {
            (
                i.consume_key(Modifiers::NONE, Key::Tab)
                    | i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        } else {
            (false, false, false, false)
        };
        EditorShortcuts {
            // Ctrl+/ matches Strudel/CodeMirror's toggle-comment; Ctrl+\ is the
            // alias requested in the parity checklist.
            comment_toggle: i.consume_key(Modifiers::CTRL, Key::Slash)
                | i.consume_key(Modifiers::CTRL, Key::Backslash),
            indent: settings.tab_indentation
                && !completion_active
                && i.consume_key(Modifiers::NONE, Key::Tab),
            outdent: i.consume_key(Modifiers::SHIFT, Key::Tab),
            // Strudel's REPL jumps the cursor between `$` block markers.
            jump_next: i.consume_key(Modifiers::ALT, Key::W),
            jump_prev: i.consume_key(Modifiers::ALT, Key::Q),
            complete_accept,
            complete_next,
            complete_prev,
            complete_dismiss,
        }
    })
}

pub(super) fn editor_typed_text(ui: &egui::Ui) -> Option<String> {
    ui.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Text(text) if text.chars().count() == 1 => Some(text.clone()),
                _ => None,
            })
            .next_back()
    })
}

pub(super) fn editor_enter_pressed(ui: &egui::Ui) -> bool {
    ui.input(|i| {
        i.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.command
            )
        })
    })
}

pub(super) fn apply_editor_text_edits(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    shortcuts: EditorShortcuts,
    typed_text: Option<&str>,
    enter_pressed: bool,
    settings: &EditorSettings,
) -> Option<egui::text::CCursorRange> {
    if shortcuts.jump_next || shortcuts.jump_prev {
        if let Some(idx) = jump_to_marker(text, cursor_range.primary.index, shortcuts.jump_next) {
            return Some(egui::text::CCursorRange::one(egui::text::CCursor::new(idx)));
        }
        return None;
    }
    if shortcuts.comment_toggle {
        return Some(toggle_line_comments(text, cursor_range));
    }
    if shortcuts.outdent {
        return Some(indent_lines(text, cursor_range, false));
    }
    if shortcuts.indent {
        return Some(indent_lines(text, cursor_range, true));
    }
    if enter_pressed && let Some(range) = auto_indent_after_enter(text, cursor_range) {
        return Some(range);
    }
    settings
        .bracket_closing
        .then(|| typed_text.and_then(|typed| apply_auto_pair(text, cursor_range, typed)))
        .flatten()
}

#[derive(Clone, Debug)]
struct CharChange {
    /// Char position of the change; `delete_len`/`insert` are char counts.
    pos: CharIndex,
    delete_len: usize,
    insert: String,
}

fn apply_char_changes(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    changes: Vec<CharChange>,
) -> egui::text::CCursorRange {
    if changes.is_empty() {
        return cursor_range;
    }

    let primary = map_index_after_changes(cursor_range.primary.index, &changes, true);
    let secondary = map_index_after_changes(cursor_range.secondary.index, &changes, true);

    for change in changes.iter().rev() {
        replace_char_range(
            text,
            change.pos..change.pos + change.delete_len,
            &change.insert,
        );
    }

    egui::text::CCursorRange {
        primary: egui::text::CCursor::new(primary),
        secondary: egui::text::CCursor::new(secondary),
        h_pos: None,
    }
}

fn apply_line_changes(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    line_starts: &[CharIndex],
    changes: Vec<CharChange>,
) -> egui::text::CCursorRange {
    if cursor_range.is_empty() || line_starts.is_empty() {
        return apply_char_changes(text, cursor_range, changes);
    }

    let first_line = line_starts[0];
    let last_line = *line_starts.last().unwrap();
    let last_line_end = line_end_at(text, last_line);
    let selection_start = map_index_after_changes(first_line, &changes, false);
    let selection_end = map_index_after_changes(last_line_end, &changes, true);

    for change in changes.iter().rev() {
        replace_char_range(
            text,
            change.pos..change.pos + change.delete_len,
            &change.insert,
        );
    }

    egui::text::CCursorRange::two(
        egui::text::CCursor::new(selection_start),
        egui::text::CCursor::new(selection_end),
    )
}

fn map_index_after_changes(
    index: CharIndex,
    changes: &[CharChange],
    include_insert_at_index: bool,
) -> CharIndex {
    let index = index.0;
    let mut delta = 0isize;
    for change in changes {
        let insert_len = change.insert.chars().count();
        let deleted_end = change.pos.0 + change.delete_len;
        if index < change.pos.0
            || (!include_insert_at_index && index == change.pos.0 && change.delete_len == 0)
        {
            break;
        }
        if index <= deleted_end {
            return CharIndex((change.pos.0 as isize + delta + insert_len as isize).max(0) as usize);
        }
        delta += insert_len as isize - change.delete_len as isize;
    }
    CharIndex((index as isize + delta).max(0) as usize)
}

fn apply_auto_pair(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    typed: &str,
) -> Option<egui::text::CCursorRange> {
    let cursor = cursor_range.single()?;
    let typed = typed.chars().next()?;
    let idx = cursor.index;
    if idx == CharIndex::ZERO || char_at(text, idx - 1) != Some(typed) {
        return None;
    }

    if (is_pair_closer(typed) || is_quote_pair(typed)) && char_at(text, idx) == Some(typed) {
        replace_char_range(text, idx - 1..idx, "");
        return Some(egui::text::CCursorRange::one(egui::text::CCursor::new(idx)));
    }

    let close = match typed {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' | '\'' | '`' => typed,
        _ => return None,
    };
    insert_text_at_char(text, idx, &close.to_string());
    Some(egui::text::CCursorRange::one(egui::text::CCursor::new(idx)))
}

fn auto_indent_after_enter(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
) -> Option<egui::text::CCursorRange> {
    let cursor = cursor_range.single()?;
    let idx = cursor.index;
    if idx == CharIndex::ZERO || char_at(text, idx - 1) != Some('\n') {
        return None;
    }

    let newline_idx = idx - 1;
    let prev_start = line_start_at(text, newline_idx);
    let prev_line = char_slice(text, prev_start..newline_idx);
    let base_indent: String = prev_line
        .chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .collect();
    let prev_trimmed = prev_line.trim_end();
    let extra_indent = if matches!(prev_trimmed.chars().last(), Some('(' | '[' | '{')) {
        CODE_INDENT
    } else {
        ""
    };

    let insert = if matching_close_after_cursor(prev_trimmed, char_at(text, idx)) {
        format!("{base_indent}{extra_indent}\n{base_indent}")
    } else {
        format!("{base_indent}{extra_indent}")
    };
    let cursor_idx = idx + base_indent.chars().count() + extra_indent.chars().count();
    insert_text_at_char(text, idx, &insert);
    Some(egui::text::CCursorRange::one(egui::text::CCursor::new(
        cursor_idx,
    )))
}

fn matching_close_after_cursor(prev_trimmed: &str, next: Option<char>) -> bool {
    matches!(
        (prev_trimmed.chars().last(), next),
        (Some('('), Some(')')) | (Some('['), Some(']')) | (Some('{'), Some('}'))
    )
}

fn toggle_line_comments(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
) -> egui::text::CCursorRange {
    let line_starts = selected_line_starts(text, cursor_range);
    let code_lines: Vec<CharIndex> = line_starts
        .iter()
        .copied()
        .filter(|&line| !line_is_blank(text, line))
        .collect();
    let uncomment = !code_lines.is_empty()
        && code_lines
            .iter()
            .all(|&line| line_comment_pos(text, line).is_some());

    let mut changes = Vec::new();
    for &line in &line_starts {
        let indent = leading_whitespace_len(text, line);
        let pos = line + indent;
        if uncomment {
            if let Some(comment_pos) = line_comment_pos(text, line) {
                let delete_len = if char_at(text, comment_pos + 2) == Some(' ') {
                    3
                } else {
                    2
                };
                changes.push(CharChange {
                    pos: comment_pos,
                    delete_len,
                    insert: String::new(),
                });
            }
        } else {
            changes.push(CharChange {
                pos,
                delete_len: 0,
                insert: "// ".to_string(),
            });
        }
    }

    apply_line_changes(text, cursor_range, &line_starts, changes)
}

fn indent_lines(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    indent: bool,
) -> egui::text::CCursorRange {
    let mut changes = Vec::new();
    let line_starts = selected_line_starts(text, cursor_range);
    for &line in &line_starts {
        if indent {
            changes.push(CharChange {
                pos: line,
                delete_len: 0,
                insert: CODE_INDENT.to_string(),
            });
        } else if char_at(text, line) == Some('\t') {
            changes.push(CharChange {
                pos: line,
                delete_len: 1,
                insert: String::new(),
            });
        } else {
            let spaces = (0..CODE_INDENT.chars().count())
                .take_while(|&i| char_at(text, line + i) == Some(' '))
                .count();
            if spaces > 0 {
                changes.push(CharChange {
                    pos: line,
                    delete_len: spaces,
                    insert: String::new(),
                });
            }
        }
    }

    apply_line_changes(text, cursor_range, &line_starts, changes)
}

fn selected_line_starts(text: &str, cursor_range: egui::text::CCursorRange) -> Vec<CharIndex> {
    let [min, max] = cursor_range.sorted_cursors();
    let start = min.index;
    let mut end = max.index;
    if end > start && char_at(text, end - 1) == Some('\n') {
        end -= 1;
    }

    let mut line = line_start_at(text, start);
    let mut lines = vec![line];
    while let Some(next) = next_line_start_after(text, line) {
        if next > end {
            break;
        }
        line = next;
        lines.push(line);
    }
    lines
}

fn line_comment_pos(text: &str, line_start: CharIndex) -> Option<CharIndex> {
    let pos = line_start + leading_whitespace_len(text, line_start);
    (char_at(text, pos) == Some('/') && char_at(text, pos + 1) == Some('/')).then_some(pos)
}

fn line_is_blank(text: &str, line_start: CharIndex) -> bool {
    char_slice(text, line_start..line_end_at(text, line_start))
        .trim()
        .is_empty()
}

fn leading_whitespace_len(text: &str, line_start: CharIndex) -> usize {
    text.chars()
        .skip(line_start.0)
        .take_while(|c| matches!(c, ' ' | '\t'))
        .count()
}

fn is_pair_closer(ch: char) -> bool {
    matches!(ch, ')' | ']' | '}')
}

fn is_quote_pair(ch: char) -> bool {
    matches!(ch, '"' | '\'' | '`')
}

/// The char index of the nearest `$` block marker after (`forward`) or before
/// the cursor, mirroring Strudel's `Alt+w`/`Alt+q` jump-to-character.
fn jump_to_marker(text: &str, cursor_char: CharIndex, forward: bool) -> Option<CharIndex> {
    let mut markers = text
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == '$')
        .map(|(i, _)| CharIndex(i));
    if forward {
        markers.find(|&i| i > cursor_char)
    } else {
        markers.filter(|&i| i < cursor_char).last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor(index: usize) -> egui::text::CCursorRange {
        egui::text::CCursorRange::one(egui::text::CCursor::new(index))
    }

    fn settings() -> EditorSettings {
        EditorSettings::default()
    }

    fn selection(start: usize, end: usize) -> egui::text::CCursorRange {
        egui::text::CCursorRange::two(
            egui::text::CCursor::new(start),
            egui::text::CCursor::new(end),
        )
    }

    fn change(pos: usize, delete_len: usize, insert: &str) -> CharChange {
        CharChange {
            pos: CharIndex(pos),
            delete_len,
            insert: insert.to_string(),
        }
    }

    #[test]
    fn an_index_moves_with_the_edits_before_it() {
        // Keeping the caret where the user left it across a re-format: an edit
        // ahead of the caret shifts it, an edit behind it does not, and an edit
        // *over* it collapses it to the end of the replacement.
        let edits = [change(2, 3, "xy")];
        let map = |i: usize| map_index_after_changes(CharIndex(i), &edits, true).0;
        assert_eq!(map(0), 0, "before the edit, unmoved");
        assert_eq!(map(2), 4, "at its start, pushed past the insertion");
        assert_eq!(map(5), 4, "inside the deleted run, collapsed to the end");
        assert_eq!(map(7), 6, "after it, shifted by the net -1");
    }

    #[test]
    fn indices_accumulate_across_several_edits() {
        // The running delta from earlier edits has to be carried into the
        // collapse position, not just into the plain shift.
        let edits = [change(0, 1, ""), change(4, 2, "zz")];
        let map = |i: usize| map_index_after_changes(CharIndex(i), &edits, true).0;
        assert_eq!(map(5), 5, "inside the second edit, offset by the first");
        assert_eq!(map(8), 7, "past both, shifted by their net -1");
    }

    #[test]
    fn an_insertion_at_the_caret_can_be_left_behind() {
        // `include_insert_at_index` decides whether text inserted exactly at
        // the caret pushes it along or is typed in front of it — but only for
        // an insertion *at* the caret. One earlier in the buffer still moves it
        // either way.
        let at_caret = [change(2, 0, "ab")];
        assert_eq!(
            map_index_after_changes(CharIndex(2), &at_caret, true).0,
            4,
            "opted in: the caret follows the insertion"
        );
        assert_eq!(
            map_index_after_changes(CharIndex(2), &at_caret, false).0,
            2,
            "opted out: the caret stays put"
        );
        assert_eq!(
            map_index_after_changes(CharIndex(7), &at_caret, false).0,
            9,
            "an insertion before the caret always moves it"
        );
    }

    #[test]
    fn every_opening_bracket_and_quote_gets_its_partner() {
        // The closer is inserted after the caret, which stays between the two.
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            let mut text = open.to_string();
            let moved = apply_auto_pair(&mut text, cursor(1), &open.to_string());
            assert_eq!(text, format!("{open}{close}"), "typing {open}");
            assert_eq!(moved.unwrap().primary.index, CharIndex(1));
        }
        for quote in ['"', '\'', '`'] {
            let mut text = quote.to_string();
            apply_auto_pair(&mut text, cursor(1), &quote.to_string());
            assert_eq!(text, format!("{quote}{quote}"), "typing {quote}");
        }
    }

    #[test]
    fn typing_a_closer_over_its_own_pair_steps_through_it() {
        // Typing `)` where a `)` already sits removes the duplicate and moves
        // past it, rather than stacking a second one.
        let mut text = "())".to_string();
        let moved = apply_auto_pair(&mut text, cursor(2), ")");
        assert_eq!(text, "()", "the just-typed closer is dropped");
        assert_eq!(
            moved.unwrap().primary.index,
            CharIndex(2),
            "and the caret lands past the one that was already there"
        );

        // An *opener* facing a copy of itself is not a step-through: it still
        // gets a partner.
        let mut text = "((".to_string();
        apply_auto_pair(&mut text, cursor(1), "(");
        assert_eq!(text, "()(", "an opener always pairs");
    }

    #[test]
    fn auto_pairing_needs_the_typed_char_to_be_there() {
        // The handler runs after the character has landed, so at the very start
        // of the buffer there is nothing behind the caret to match.
        let mut text = "()".to_string();
        assert!(apply_auto_pair(&mut text, cursor(0), "(").is_none());
        assert_eq!(text, "()", "and nothing is inserted");
        // Nor when the character behind the caret is something else.
        assert!(apply_auto_pair(&mut text, cursor(1), "[").is_none());
    }

    #[test]
    fn a_selection_covers_the_lines_it_touches() {
        // Line-wise commands work on whole lines; a selection ending exactly at
        // the start of the next line must not drag that line in, or commenting
        // a block would comment one line too many.
        let text = "one\ntwo\nthree";
        let starts = |range| {
            selected_line_starts(text, range)
                .into_iter()
                .map(|c| c.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            starts(cursor(0)),
            vec![0],
            "a bare caret takes its own line"
        );
        assert_eq!(starts(cursor(5)), vec![4], "...wherever it sits on it");
        assert_eq!(starts(selection(0, 5)), vec![0, 4], "spanning two lines");
        assert_eq!(
            starts(selection(0, 4)),
            vec![0],
            "ending just past the newline stays on the first line"
        );
        assert_eq!(starts(selection(0, 13)), vec![0, 4, 8], "the whole buffer");
        assert_eq!(starts(selection(5, 9)), vec![4, 8], "starting mid-line");
    }

    #[test]
    fn jump_moves_between_dollar_markers() {
        let text = "$: s(\"bd\")\n$: s(\"hh\")";
        let second = CharIndex(
            text.chars()
                .enumerate()
                .filter(|(_, c)| *c == '$')
                .nth(1)
                .unwrap()
                .0,
        );
        // forward from the first marker lands on the second
        assert_eq!(jump_to_marker(text, CharIndex::ZERO, true), Some(second));
        // backward from the end lands on the second, then the first
        assert_eq!(
            jump_to_marker(text, CharIndex(text.chars().count()), false),
            Some(second)
        );
        assert_eq!(jump_to_marker(text, second, false), Some(CharIndex::ZERO));
        // nothing past the last/first marker
        assert_eq!(jump_to_marker(text, second, true), None);
        assert_eq!(jump_to_marker(text, CharIndex::ZERO, false), None);
    }

    #[test]
    fn editor_auto_pairs_opening_brackets() {
        let mut text = "stack(".to_string();
        let range = apply_auto_pair(&mut text, cursor(6), "(").unwrap();
        assert_eq!(text, "stack()");
        assert_eq!(range.single().unwrap().index, CharIndex(6));
    }

    #[test]
    fn editor_skips_existing_closing_brackets() {
        let mut text = "())".to_string();
        let range = apply_auto_pair(&mut text, cursor(2), ")").unwrap();
        assert_eq!(text, "()");
        assert_eq!(range.single().unwrap().index, CharIndex(2));
    }

    #[test]
    fn editor_auto_indent_carries_indent_after_enter() {
        let mut text = "  note(\n".to_string();
        let range = auto_indent_after_enter(&mut text, cursor(8)).unwrap();
        assert_eq!(text, "  note(\n    ");
        assert_eq!(range.single().unwrap().index, CharIndex(12));
    }

    #[test]
    fn editor_auto_indent_splits_bracket_pairs() {
        let mut text = "(\n)".to_string();
        let range = auto_indent_after_enter(&mut text, cursor(2)).unwrap();
        assert_eq!(text, "(\n  \n)");
        assert_eq!(range.single().unwrap().index, CharIndex(4));
    }

    #[test]
    fn editor_indents_and_outdents_selected_lines() {
        let mut text = "a\nb".to_string();
        let range = indent_lines(&mut text, selection(0, 3), true);
        assert_eq!(text, "  a\n  b");
        assert_eq!(range.as_sorted_char_range(), CharIndex(0)..CharIndex(7));

        let range = indent_lines(&mut text, range, false);
        assert_eq!(text, "a\nb");
        assert_eq!(range.as_sorted_char_range(), CharIndex(0)..CharIndex(3));
    }

    #[test]
    fn editor_toggles_line_comments() {
        let mut text = "  a\n  b".to_string();
        let range = toggle_line_comments(&mut text, selection(0, 7));
        assert_eq!(text, "  // a\n  // b");

        let range = toggle_line_comments(&mut text, range);
        assert_eq!(text, "  a\n  b");
        assert_eq!(range.as_sorted_char_range(), CharIndex(0)..CharIndex(7));
    }

    #[test]
    fn editor_respects_disabled_bracket_closing_setting() {
        let mut text = "(".to_string();
        let mut settings = settings();
        settings.bracket_closing = false;

        let range = apply_editor_text_edits(
            &mut text,
            cursor(1),
            EditorShortcuts::default(),
            Some("("),
            false,
            &settings,
        );

        assert_eq!(text, "(");
        assert!(range.is_none());
    }
}
