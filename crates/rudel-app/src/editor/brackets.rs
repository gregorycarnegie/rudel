use super::text::byte_index_at_char;
use eframe::egui::text::CharIndex;

/// When the cursor sits next to a bracket, the byte spans of that bracket and
/// its match (CodeMirror's `bracketMatching`). The cursor's right-hand char is
/// preferred, then its left-hand char.
pub(super) fn bracket_match_spans(
    text: &str,
    cursor_char: CharIndex,
) -> Option<[(usize, usize); 2]> {
    let cursor_char = cursor_char.0;
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
        byte_index_at_char(text, CharIndex(char_idx)).0,
        byte_index_at_char(text, CharIndex(char_idx + 1)).0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_match_finds_the_pair_next_to_the_cursor() {
        let text = "stack(a, [b])";
        // cursor right after the closing `)` matches the opening `(` at 5
        assert_eq!(
            bracket_match_spans(text, CharIndex(13)),
            Some([(12, 13), (5, 6)])
        );
        // cursor on the inner `[` (index 9) matches its `]` at 11
        assert_eq!(
            bracket_match_spans(text, CharIndex(9)),
            Some([(9, 10), (11, 12)])
        );
        // nested `(` at 5 matches the outer `)` at 12, not the inner bracket
        assert_eq!(
            bracket_match_spans(text, CharIndex(5)),
            Some([(5, 6), (12, 13)])
        );
        // not next to a bracket -> nothing
        assert_eq!(bracket_match_spans(text, CharIndex(7)), None);
    }
}
