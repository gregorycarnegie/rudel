//! Char-indexed text helpers for the editor. Positions are typed with egui's
//! [`CharIndex`]/[`ByteIndex`] newtypes so char and byte offsets can't be
//! mixed up; lengths and counts stay plain `usize`.

use eframe::egui::text::{ByteIndex, CharIndex};

pub(super) fn line_start_at(text: &str, char_idx: CharIndex) -> CharIndex {
    let mut start = 0;
    for (idx, ch) in text.chars().enumerate().take(char_idx.0) {
        if ch == '\n' {
            start = idx + 1;
        }
    }
    CharIndex(start)
}

pub(super) fn line_end_at(text: &str, line_start: CharIndex) -> CharIndex {
    for (offset, ch) in text.chars().skip(line_start.0).enumerate() {
        if ch == '\n' {
            return line_start + offset;
        }
    }
    CharIndex(text.chars().count())
}

pub(super) fn next_line_start_after(text: &str, line_start: CharIndex) -> Option<CharIndex> {
    for (offset, ch) in text.chars().skip(line_start.0).enumerate() {
        if ch == '\n' {
            return Some(line_start + offset + 1);
        }
    }
    None
}

pub(super) fn char_at(text: &str, char_idx: CharIndex) -> Option<char> {
    text.chars().nth(char_idx.0)
}

pub(super) fn char_slice(text: &str, range: std::ops::Range<CharIndex>) -> &str {
    &text[byte_index_at_char(text, range.start).0..byte_index_at_char(text, range.end).0]
}

pub(super) fn insert_text_at_char(text: &mut String, char_idx: CharIndex, insert: &str) {
    text.insert_str(byte_index_at_char(text, char_idx).0, insert);
}

pub(super) fn replace_char_range(
    text: &mut String,
    range: std::ops::Range<CharIndex>,
    insert: &str,
) {
    let start = byte_index_at_char(text, range.start).0;
    let end = byte_index_at_char(text, range.end).0;
    text.replace_range(start..end, insert);
}

pub(super) fn byte_index_at_char(text: &str, char_idx: CharIndex) -> ByteIndex {
    ByteIndex(
        text.char_indices()
            .nth(char_idx.0)
            .map(|(idx, _)| idx)
            .unwrap_or(text.len()),
    )
}

pub(super) fn char_index_at_byte(text: &str, byte: ByteIndex) -> CharIndex {
    CharIndex(text[..byte.0.min(text.len())].chars().count())
}
