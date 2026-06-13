use crate::reference::{CONTROLS, FACTORIES, LANGUAGE_KEYWORDS, SIGNALS};
use eframe::egui;

const CODE_EDITOR_ID: &str = "rudel_code_editor";
const CODE_INDENT: &str = "  ";

pub(crate) fn code_editor(ui: &mut egui::Ui, code: &mut String, active: &[(usize, usize)]) {
    let editor_id = ui.make_persistent_id(egui::Id::new(CODE_EDITOR_ID));
    let bracket_id = editor_id.with("bracket_match");
    let shortcuts = capture_editor_shortcuts(ui, editor_id);
    let typed_text = editor_typed_text(ui);
    let enter_pressed = editor_enter_pressed(ui);
    // Bracket-match spans computed from last frame's cursor (the layouter runs
    // before this frame's cursor is known); recomputed and stored below.
    let brackets: Vec<(usize, usize)> = ui.data(|d| d.get_temp(bracket_id)).unwrap_or_default();
    let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        let job = highlighted_editor_job(text.as_str(), ui, wrap_width, active, &brackets);
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let mut output = egui::TextEdit::multiline(code)
        .id_salt(CODE_EDITOR_ID)
        .code_editor()
        .layouter(&mut layouter)
        .desired_rows(28)
        .desired_width(f32::INFINITY)
        .show(ui);
    if output.response.has_focus()
        && let Some(cursor_range) = output.cursor_range
    {
        let edited = apply_editor_text_edits(
            code,
            cursor_range,
            shortcuts,
            typed_text.as_deref(),
            enter_pressed,
        );
        let cursor = edited
            .map(|r| r.primary.index)
            .unwrap_or(cursor_range.primary.index);
        if let Some(new_range) = edited {
            output.state.cursor.set_char_range(Some(new_range));
            output.state.store(ui.ctx(), output.response.id);
        }
        // Refresh the bracket-match highlight for the (possibly moved) cursor.
        let new_brackets = bracket_match_spans(code, cursor)
            .map(|pair| pair.to_vec())
            .unwrap_or_default();
        if new_brackets != brackets {
            ui.data_mut(|d| d.insert_temp(bracket_id, new_brackets));
            ui.ctx().request_repaint();
        }
    } else if !brackets.is_empty() {
        ui.data_mut(|d| d.insert_temp(bracket_id, Vec::<(usize, usize)>::new()));
        ui.ctx().request_repaint();
    }
}

fn is_highlighted_ident(ident: &str) -> bool {
    LANGUAGE_KEYWORDS.contains(&ident)
        || FACTORIES.contains(&ident)
        || CONTROLS.contains(&ident)
        || SIGNALS
            .iter()
            .any(|s| s.strip_suffix("(n)").unwrap_or(s) == ident)
}

/// Highlight category for a contiguous byte span of editor text. Mirrors the
/// token categories Strudel's CodeMirror grammar distinguishes, including
/// mini-notation tokens inside string literals.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Token {
    /// Plain code (identifiers, whitespace, operators outside strings).
    Normal,
    /// Language keyword, factory, control, or signal name.
    Keyword,
    /// Identifier following a `.` (a method/control call).
    Method,
    /// String delimiters and inert string content (whitespace inside mini).
    Str,
    /// Numeric literal (in code or mini-notation).
    Number,
    /// `//` line comment.
    Comment,
    /// Mini-notation word: sample/synth/note name.
    MiniWord,
    /// Mini-notation operator or grouping: `* / ! @ < > [ ] ( ) { } , . ? : | % -`.
    MiniOp,
    /// Mini-notation rest (`~`).
    MiniRest,
}

/// Two half-open byte ranges overlap.
fn spans_overlap(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

fn highlighted_editor_job(
    code: &str,
    ui: &egui::Ui,
    wrap_width: f32,
    active: &[(usize, usize)],
    brackets: &[(usize, usize)],
) -> egui::text::LayoutJob {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let normal = egui::TextFormat::simple(font_id.clone(), ui.visuals().text_color());
    let keyword = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(106, 153, 205));
    let method = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(220, 220, 170));
    let string = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(206, 145, 120));
    let number = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(181, 206, 168));
    let comment = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(106, 153, 85));
    let mini_op = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(197, 134, 192));
    let mini_word = egui::TextFormat::simple(font_id, egui::Color32::from_rgb(156, 220, 254));

    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;

    // Background flashed under spans of code currently producing a hap, and a
    // distinct one under the bracket pair around the cursor.
    let flash = egui::Color32::from_rgb(74, 68, 38);
    let bracket_flash = egui::Color32::from_rgb(60, 84, 104);

    for (start, end, token) in tokenize(code) {
        let base = match token {
            Token::Normal => &normal,
            Token::Keyword => &keyword,
            Token::Method => &method,
            Token::Str | Token::MiniRest => &string,
            Token::Number => &number,
            Token::Comment => &comment,
            Token::MiniWord => &mini_word,
            Token::MiniOp => &mini_op,
        };
        let mut format = base.clone();
        // Active-event flash wins over bracket matching when they coincide.
        if brackets
            .iter()
            .any(|&span| spans_overlap((start, end), span))
        {
            format.background = bracket_flash;
        }
        if active.iter().any(|&span| spans_overlap((start, end), span)) {
            format.background = flash;
        }
        job.append(&code[start..end], 0.0, format);
    }

    job
}

/// Split `code` into contiguous highlighted spans (byte ranges). String
/// literals are further tokenized as mini-notation so words, numbers, rests,
/// and operators get distinct colors.
fn tokenize(code: &str) -> Vec<(usize, usize, Token)> {
    let mut tokens = Vec::new();
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let c = bytes[i] as char;

        if c == '/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            tokens.push((start, i, Token::Comment));
        } else if c == '"' || c == '\'' {
            let quote = bytes[i];
            i += 1;
            tokens.push((start, i, Token::Str)); // opening quote
            let body_start = i;
            let mut escaped = false;
            while i < bytes.len() {
                let b = bytes[i];
                if escaped {
                    escaped = false;
                    i += 1;
                } else if b == b'\\' {
                    escaped = true;
                    i += 1;
                } else if b == quote {
                    break;
                } else {
                    i += 1;
                }
            }
            tokenize_mini(&code[body_start..i], body_start, &mut tokens);
            if i < bytes.len() && bytes[i] == quote {
                tokens.push((i, i + 1, Token::Str)); // closing quote
                i += 1;
            }
        } else if c.is_ascii_digit() {
            i += 1;
            while i < bytes.len() {
                let b = bytes[i] as char;
                if b.is_ascii_alphanumeric() || matches!(b, '.' | '_' | '/') {
                    i += 1;
                } else {
                    break;
                }
            }
            tokens.push((start, i, Token::Number));
        } else if c.is_ascii_alphabetic() || matches!(c, '_' | '$') {
            i += 1;
            while i < bytes.len() {
                let b = bytes[i] as char;
                if b.is_ascii_alphanumeric() || matches!(b, '_' | '$') {
                    i += 1;
                } else {
                    break;
                }
            }
            let ident = &code[start..i];
            let token = if start > 0 && bytes[start - 1] == b'.' {
                Token::Method
            } else if is_highlighted_ident(ident) {
                Token::Keyword
            } else {
                Token::Normal
            };
            tokens.push((start, i, token));
        } else {
            i += 1;
            tokens.push((start, i, Token::Normal));
        }
    }
    tokens
}

/// Tokenize the body of a string literal as mini-notation, pushing spans
/// offset by `offset` (the byte position of the body within the full source).
fn tokenize_mini(body: &str, offset: usize, tokens: &mut Vec<(usize, usize, Token)>) {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let c = bytes[i] as char;
        let next_is_digit = bytes.get(i + 1).is_some_and(|b| b.is_ascii_digit());

        if c == '~' {
            i += 1;
            tokens.push((offset + start, offset + i, Token::MiniRest));
        } else if c.is_ascii_digit() || (c == '-' && next_is_digit) {
            i += 1;
            while i < bytes.len() && matches!(bytes[i], b'0'..=b'9' | b'.') {
                i += 1;
            }
            tokens.push((offset + start, offset + i, Token::Number));
        } else if c.is_ascii_alphabetic() || c == '_' || c == '#' {
            i += 1;
            while i < bytes.len() {
                let b = bytes[i] as char;
                if b.is_ascii_alphanumeric() || matches!(b, '#' | '_' | '\'') {
                    i += 1;
                } else {
                    break;
                }
            }
            tokens.push((offset + start, offset + i, Token::MiniWord));
        } else if matches!(
            c,
            '*' | '/'
                | '!'
                | '@'
                | '<'
                | '>'
                | '['
                | ']'
                | '('
                | ')'
                | '{'
                | '}'
                | ','
                | '.'
                | '?'
                | ':'
                | '|'
                | '%'
                | '-'
        ) {
            i += 1;
            tokens.push((offset + start, offset + i, Token::MiniOp));
        } else {
            i += 1;
            tokens.push((offset + start, offset + i, Token::Str));
        }
    }
}

#[derive(Clone, Copy, Default)]
struct EditorShortcuts {
    comment_toggle: bool,
    indent: bool,
    outdent: bool,
    jump_next: bool,
    jump_prev: bool,
}

fn capture_editor_shortcuts(ui: &mut egui::Ui, editor_id: egui::Id) -> EditorShortcuts {
    if !ui.memory(|m| m.has_focus(editor_id)) {
        return EditorShortcuts::default();
    }
    ui.input_mut(|i| EditorShortcuts {
        // Ctrl+/ matches Strudel/CodeMirror's toggle-comment; Ctrl+\ is the
        // alias requested in the parity checklist.
        comment_toggle: i.consume_key(egui::Modifiers::CTRL, egui::Key::Slash)
            | i.consume_key(egui::Modifiers::CTRL, egui::Key::Backslash),
        indent: i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
        outdent: i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab),
        // Strudel's REPL jumps the cursor between `$` block markers.
        jump_next: i.consume_key(egui::Modifiers::ALT, egui::Key::W),
        jump_prev: i.consume_key(egui::Modifiers::ALT, egui::Key::Q),
    })
}

fn editor_typed_text(ui: &egui::Ui) -> Option<String> {
    ui.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Text(text) if text.chars().count() == 1 => Some(text.clone()),
                _ => None,
            })
            .last()
    })
}

fn editor_enter_pressed(ui: &egui::Ui) -> bool {
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

fn apply_editor_text_edits(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    shortcuts: EditorShortcuts,
    typed_text: Option<&str>,
    enter_pressed: bool,
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
    typed_text.and_then(|typed| apply_auto_pair(text, cursor_range, typed))
}

#[derive(Clone, Debug)]
struct CharChange {
    pos: usize,
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
    line_starts: &[usize],
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
    index: usize,
    changes: &[CharChange],
    include_insert_at_index: bool,
) -> usize {
    let mut delta = 0isize;
    for change in changes {
        let insert_len = change.insert.chars().count();
        let deleted_end = change.pos + change.delete_len;
        if index < change.pos
            || (!include_insert_at_index && index == change.pos && change.delete_len == 0)
        {
            break;
        }
        if index <= deleted_end {
            return (change.pos as isize + delta + insert_len as isize).max(0) as usize;
        }
        delta += insert_len as isize - change.delete_len as isize;
    }
    (index as isize + delta).max(0) as usize
}

fn apply_auto_pair(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    typed: &str,
) -> Option<egui::text::CCursorRange> {
    let cursor = cursor_range.single()?;
    let typed = typed.chars().next()?;
    let idx = cursor.index;
    if idx == 0 || char_at(text, idx - 1) != Some(typed) {
        return None;
    }

    if is_pair_closer(typed) || is_quote_pair(typed) {
        if char_at(text, idx) == Some(typed) {
            replace_char_range(text, idx - 1..idx, "");
            return Some(egui::text::CCursorRange::one(egui::text::CCursor::new(idx)));
        }
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
    if idx == 0 || char_at(text, idx - 1) != Some('\n') {
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
    let code_lines: Vec<usize> = line_starts
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
                .take_while(|i| char_at(text, line + i) == Some(' '))
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

fn selected_line_starts(text: &str, cursor_range: egui::text::CCursorRange) -> Vec<usize> {
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

fn line_comment_pos(text: &str, line_start: usize) -> Option<usize> {
    let pos = line_start + leading_whitespace_len(text, line_start);
    (char_at(text, pos) == Some('/') && char_at(text, pos + 1) == Some('/')).then_some(pos)
}

fn line_is_blank(text: &str, line_start: usize) -> bool {
    char_slice(text, line_start..line_end_at(text, line_start))
        .trim()
        .is_empty()
}

fn leading_whitespace_len(text: &str, line_start: usize) -> usize {
    text.chars()
        .skip(line_start)
        .take_while(|c| matches!(c, ' ' | '\t'))
        .count()
}

fn line_start_at(text: &str, char_idx: usize) -> usize {
    let mut start = 0;
    for (idx, ch) in text.chars().enumerate().take(char_idx) {
        if ch == '\n' {
            start = idx + 1;
        }
    }
    start
}

fn line_end_at(text: &str, line_start: usize) -> usize {
    for (offset, ch) in text.chars().skip(line_start).enumerate() {
        if ch == '\n' {
            return line_start + offset;
        }
    }
    text.chars().count()
}

fn next_line_start_after(text: &str, line_start: usize) -> Option<usize> {
    for (offset, ch) in text.chars().skip(line_start).enumerate() {
        if ch == '\n' {
            return Some(line_start + offset + 1);
        }
    }
    None
}

fn char_at(text: &str, char_idx: usize) -> Option<char> {
    text.chars().nth(char_idx)
}

fn char_slice(text: &str, range: std::ops::Range<usize>) -> &str {
    &text[byte_index_at_char(text, range.start)..byte_index_at_char(text, range.end)]
}

fn insert_text_at_char(text: &mut String, char_idx: usize, insert: &str) {
    text.insert_str(byte_index_at_char(text, char_idx), insert);
}

fn replace_char_range(text: &mut String, range: std::ops::Range<usize>, insert: &str) {
    let start = byte_index_at_char(text, range.start);
    let end = byte_index_at_char(text, range.end);
    text.replace_range(start..end, insert);
}

fn byte_index_at_char(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn is_pair_closer(ch: char) -> bool {
    matches!(ch, ')' | ']' | '}')
}

fn is_quote_pair(ch: char) -> bool {
    matches!(ch, '"' | '\'' | '`')
}

/// The char index of the nearest `$` block marker after (`forward`) or before
/// the cursor, mirroring Strudel's `Alt+w`/`Alt+q` jump-to-character.
fn jump_to_marker(text: &str, cursor_char: usize, forward: bool) -> Option<usize> {
    let markers = text.chars().enumerate().filter(|(_, c)| *c == '$');
    if forward {
        markers.map(|(i, _)| i).find(|&i| i > cursor_char)
    } else {
        markers.map(|(i, _)| i).filter(|&i| i < cursor_char).last()
    }
}

/// When the cursor sits next to a bracket, the byte spans of that bracket and
/// its match (CodeMirror's `bracketMatching`). The cursor's right-hand char is
/// preferred, then its left-hand char.
fn bracket_match_spans(text: &str, cursor_char: usize) -> Option<[(usize, usize); 2]> {
    let chars: Vec<char> = text.chars().collect();
    let bracket = |i: usize| chars.get(i).copied().filter(|c| is_bracket(*c));
    let pos = if bracket(cursor_char).is_some() {
        cursor_char
    } else if cursor_char > 0 && bracket(cursor_char - 1).is_some() {
        cursor_char - 1
    } else {
        return None;
    };
    let other = matching_bracket_index(&chars, pos)?;
    Some([char_span_bytes(text, pos), char_span_bytes(text, other)])
}

fn is_bracket(ch: char) -> bool {
    matches!(ch, '(' | ')' | '[' | ']' | '{' | '}')
}

/// The index of the bracket matching the one at `pos`, scanning outward and
/// tracking nesting depth of the same bracket family.
fn matching_bracket_index(chars: &[char], pos: usize) -> Option<usize> {
    let (open, close, forward) = match chars[pos] {
        '(' => ('(', ')', true),
        '[' => ('[', ']', true),
        '{' => ('{', '}', true),
        ')' => ('(', ')', false),
        ']' => ('[', ']', false),
        '}' => ('{', '}', false),
        _ => return None,
    };
    let mut depth = 0i32;
    let indices: Vec<usize> = if forward {
        (pos..chars.len()).collect()
    } else {
        (0..=pos).rev().collect()
    };
    for i in indices {
        if chars[i] == open {
            depth += if forward { 1 } else { -1 };
        } else if chars[i] == close {
            depth += if forward { -1 } else { 1 };
        }
        if depth == 0 {
            return Some(i);
        }
    }
    None
}

/// The byte range of the single char at `char_idx`.
fn char_span_bytes(text: &str, char_idx: usize) -> (usize, usize) {
    (
        byte_index_at_char(text, char_idx),
        byte_index_at_char(text, char_idx + 1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor(index: usize) -> egui::text::CCursorRange {
        egui::text::CCursorRange::one(egui::text::CCursor::new(index))
    }

    fn selection(start: usize, end: usize) -> egui::text::CCursorRange {
        egui::text::CCursorRange::two(
            egui::text::CCursor::new(start),
            egui::text::CCursor::new(end),
        )
    }

    /// Collect `(text, token)` pairs so tests can assert on classification
    /// without depending on egui colors.
    fn classify(code: &str) -> Vec<(&str, Token)> {
        tokenize(code)
            .into_iter()
            .map(|(start, end, token)| (&code[start..end], token))
            .collect()
    }

    #[test]
    fn highlights_keywords_methods_and_numbers_in_code() {
        let toks = classify("stack(x).gain(0.9)");
        assert!(toks.contains(&("stack", Token::Keyword)));
        assert!(toks.contains(&("gain", Token::Method)));
        assert!(toks.contains(&("0.9", Token::Number)));
        assert!(toks.contains(&("x", Token::Normal)));
    }

    #[test]
    fn tokenizes_mini_notation_inside_strings() {
        let toks = classify(r#"s("bd*2 ~ [hh hh:3]")"#);
        assert!(toks.contains(&("bd", Token::MiniWord)));
        assert!(toks.contains(&("hh", Token::MiniWord)));
        assert!(toks.contains(&("*", Token::MiniOp)));
        assert!(toks.contains(&("[", Token::MiniOp)));
        assert!(toks.contains(&(":", Token::MiniOp)));
        assert!(toks.contains(&("2", Token::Number)));
        assert!(toks.contains(&("3", Token::Number)));
        assert!(toks.contains(&("~", Token::MiniRest)));
        // Delimiters are still string-colored.
        assert_eq!(toks.first(), Some(&("s", Token::Keyword)));
        assert!(toks.contains(&("\"", Token::Str)));
    }

    #[test]
    fn tokenizes_note_names_and_decimals_in_mini() {
        let toks = classify(r#"note("c#4 -1.5")"#);
        assert!(toks.contains(&("c#4", Token::MiniWord)));
        assert!(toks.contains(&("-1.5", Token::Number)));
    }

    #[test]
    fn jump_moves_between_dollar_markers() {
        let text = "$: s(\"bd\")\n$: s(\"hh\")";
        let second = text
            .char_indices()
            .filter(|(_, c)| *c == '$')
            .nth(1)
            .unwrap()
            .0;
        // forward from the first marker lands on the second
        assert_eq!(jump_to_marker(text, 0, true), Some(second));
        // backward from the end lands on the second, then the first
        assert_eq!(
            jump_to_marker(text, text.chars().count(), false),
            Some(second)
        );
        assert_eq!(jump_to_marker(text, second, false), Some(0));
        // nothing past the last/first marker
        assert_eq!(jump_to_marker(text, second, true), None);
        assert_eq!(jump_to_marker(text, 0, false), None);
    }

    #[test]
    fn bracket_match_finds_the_pair_next_to_the_cursor() {
        let text = "stack(a, [b])";
        // cursor right after the closing `)` matches the opening `(` at 5
        assert_eq!(bracket_match_spans(text, 13), Some([(12, 13), (5, 6)]));
        // cursor on the inner `[` (index 9) matches its `]` at 11
        assert_eq!(bracket_match_spans(text, 9), Some([(9, 10), (11, 12)]));
        // nested `(` at 5 matches the outer `)` at 12, not the inner bracket
        assert_eq!(bracket_match_spans(text, 5), Some([(5, 6), (12, 13)]));
        // not next to a bracket -> nothing
        assert_eq!(bracket_match_spans(text, 7), None);
    }

    #[test]
    fn active_spans_overlap_token_ranges() {
        // A leaf location (3,5) should flash the token covering bytes 3..5.
        assert!(spans_overlap((3, 5), (3, 5)));
        assert!(spans_overlap((3, 5), (4, 9))); // partial overlap
        assert!(!spans_overlap((3, 5), (5, 7))); // adjacent, half-open
        assert!(!spans_overlap((3, 5), (0, 3)));
    }

    #[test]
    fn tokens_cover_the_whole_source_contiguously() {
        let code = r#"n("0 1").s("piano") // hi"#;
        let mut next = 0;
        for (start, end, _) in tokenize(code) {
            assert_eq!(start, next, "gap before {start}");
            assert!(end > start);
            next = end;
        }
        assert_eq!(next, code.len());
    }

    #[test]
    fn editor_auto_pairs_opening_brackets() {
        let mut text = "stack(".to_string();
        let range = apply_auto_pair(&mut text, cursor(6), "(").unwrap();
        assert_eq!(text, "stack()");
        assert_eq!(range.single().unwrap().index, 6);
    }

    #[test]
    fn editor_skips_existing_closing_brackets() {
        let mut text = "())".to_string();
        let range = apply_auto_pair(&mut text, cursor(2), ")").unwrap();
        assert_eq!(text, "()");
        assert_eq!(range.single().unwrap().index, 2);
    }

    #[test]
    fn editor_auto_indent_carries_indent_after_enter() {
        let mut text = "  note(\n".to_string();
        let range = auto_indent_after_enter(&mut text, cursor(8)).unwrap();
        assert_eq!(text, "  note(\n    ");
        assert_eq!(range.single().unwrap().index, 12);
    }

    #[test]
    fn editor_auto_indent_splits_bracket_pairs() {
        let mut text = "(\n)".to_string();
        let range = auto_indent_after_enter(&mut text, cursor(2)).unwrap();
        assert_eq!(text, "(\n  \n)");
        assert_eq!(range.single().unwrap().index, 4);
    }

    #[test]
    fn editor_indents_and_outdents_selected_lines() {
        let mut text = "a\nb".to_string();
        let range = indent_lines(&mut text, selection(0, 3), true);
        assert_eq!(text, "  a\n  b");
        assert_eq!(range.as_sorted_char_range(), 0..7);

        let range = indent_lines(&mut text, range, false);
        assert_eq!(text, "a\nb");
        assert_eq!(range.as_sorted_char_range(), 0..3);
    }

    #[test]
    fn editor_toggles_line_comments() {
        let mut text = "  a\n  b".to_string();
        let range = toggle_line_comments(&mut text, selection(0, 7));
        assert_eq!(text, "  // a\n  // b");

        let range = toggle_line_comments(&mut text, range);
        assert_eq!(text, "  a\n  b");
        assert_eq!(range.as_sorted_char_range(), 0..7);
    }
}
