pub(super) fn line_start_at(text: &str, char_idx: usize) -> usize {
    let mut start = 0;
    for (idx, ch) in text.chars().enumerate().take(char_idx) {
        if ch == '\n' {
            start = idx + 1;
        }
    }
    start
}

pub(super) fn line_end_at(text: &str, line_start: usize) -> usize {
    for (offset, ch) in text.chars().skip(line_start).enumerate() {
        if ch == '\n' {
            return line_start + offset;
        }
    }
    text.chars().count()
}

pub(super) fn next_line_start_after(text: &str, line_start: usize) -> Option<usize> {
    for (offset, ch) in text.chars().skip(line_start).enumerate() {
        if ch == '\n' {
            return Some(line_start + offset + 1);
        }
    }
    None
}

pub(super) fn char_at(text: &str, char_idx: usize) -> Option<char> {
    text.chars().nth(char_idx)
}

pub(super) fn char_slice(text: &str, range: std::ops::Range<usize>) -> &str {
    &text[byte_index_at_char(text, range.start)..byte_index_at_char(text, range.end)]
}

pub(super) fn insert_text_at_char(text: &mut String, char_idx: usize, insert: &str) {
    text.insert_str(byte_index_at_char(text, char_idx), insert);
}

pub(super) fn replace_char_range(text: &mut String, range: std::ops::Range<usize>, insert: &str) {
    let start = byte_index_at_char(text, range.start);
    let end = byte_index_at_char(text, range.end);
    text.replace_range(start..end, insert);
}

pub(super) fn byte_index_at_char(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

pub(super) fn char_index_at_byte(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}
