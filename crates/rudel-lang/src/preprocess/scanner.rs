pub(super) struct CallInfo {
    pub close_char: usize,
    pub first_arg: Option<(usize, usize)>,
    pub args: Vec<(usize, usize)>,
}

pub(super) fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

pub(super) fn next_byte(chars: &[(usize, char)], i: usize, len: usize) -> usize {
    chars.get(i + 1).map(|x| x.0).unwrap_or(len)
}

pub(super) fn previous_non_ws(chars: &[(usize, char)], i: usize) -> Option<char> {
    chars[..i]
        .iter()
        .rev()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(_, c)| *c)
}

pub(super) fn trim_range(src: &str, mut start: usize, mut end: usize) -> (usize, usize) {
    while start < end {
        let Some(c) = src[start..end].chars().next() else {
            break;
        };
        if !c.is_whitespace() {
            break;
        }
        start += c.len_utf8();
    }
    while start < end {
        let Some(c) = src[start..end].chars().next_back() else {
            break;
        };
        if !c.is_whitespace() {
            break;
        }
        end -= c.len_utf8();
    }
    (start, end)
}

pub(super) fn skip_string(chars: &[(usize, char)], mut i: usize, quote: char) -> usize {
    let mut escaped = false;
    i += 1;
    while i < chars.len() {
        let ch = chars[i].1;
        i += 1;
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            break;
        }
    }
    i
}

pub(super) fn skip_line_comment(chars: &[(usize, char)], mut i: usize) -> usize {
    while i < chars.len() && chars[i].1 != '\n' {
        i += 1;
    }
    i
}

pub(super) fn skip_block_comment(chars: &[(usize, char)], mut i: usize) -> usize {
    i += 2;
    while i + 1 < chars.len() {
        if chars[i].1 == '*' && chars[i + 1].1 == '/' {
            return i + 2;
        }
        i += 1;
    }
    chars.len()
}

pub(super) fn parse_call(src: &str, chars: &[(usize, char)], open: usize) -> Option<CallInfo> {
    let mut i = open + 1;
    let mut depth = 0i32;
    let mut arg_start = next_byte(chars, open, src.len());
    let mut args = Vec::new();
    while i < chars.len() {
        let (byte, c) = chars[i];
        if (c == '"' || c == '\'') && depth >= 0 {
            i = skip_string(chars, i, c);
            continue;
        }
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('/') {
            i = skip_line_comment(chars, i);
            continue;
        }
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('*') {
            i = skip_block_comment(chars, i);
            continue;
        }
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => {
                let range = trim_range(src, arg_start, byte);
                if range.0 < range.1 {
                    args.push(range);
                }
                let first_arg = args.first().copied();
                return Some(CallInfo {
                    close_char: i,
                    first_arg,
                    args,
                });
            }
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                let range = trim_range(src, arg_start, byte);
                if range.0 < range.1 {
                    args.push(range);
                }
                arg_start = next_byte(chars, i, src.len());
            }
            _ => {}
        }
        i += 1;
    }
    None
}

pub(super) fn top_level_ranges(text: &str, delimiter: char) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        let (byte, ch) = chars[i];
        if ch == '"' || ch == '\'' {
            i = skip_string(&chars, i, ch);
            continue;
        }
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if ch == delimiter && depth == 0 => {
                if start < byte {
                    ranges.push((start, byte));
                }
                start = next_byte(&chars, i, text.len());
            }
            _ => {}
        }
        i += 1;
    }
    if start < text.len() {
        ranges.push((start, text.len()));
    }
    ranges
}

pub(super) fn top_level_split(text: &str, delimiter: char) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        let (byte, ch) = chars[i];
        if ch == '"' || ch == '\'' {
            i = skip_string(&chars, i, ch);
            continue;
        }
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if ch == delimiter && depth == 0 => return Some(byte),
            _ => {}
        }
        i += 1;
    }
    None
}
