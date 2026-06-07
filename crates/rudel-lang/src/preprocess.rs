fn strip_line_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut quote = None;
    let mut escaped = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
            out.push(c);
            i += 1;
        } else if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            if i < chars.len() {
                out.push('\n');
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn rewrite_const_declarations(src: &str) -> String {
    src.lines()
        .map(|line| {
            let indent_len = line.len() - line.trim_start().len();
            let (indent, rest) = line.split_at(indent_len);
            if let Some(stripped) = rest.strip_prefix("const ") {
                format!("{indent}{stripped}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_string_literal(literal: &str) -> String {
    let Some(quote) = literal.chars().next() else {
        return literal.to_string();
    };
    if quote != '"' && quote != '\'' {
        return literal.to_string();
    }
    let content = &literal[1..literal.len().saturating_sub(1)];
    if !content.contains('{') && !content.contains('}') {
        return literal.to_string();
    }
    let mut hashes = "#".to_string();
    while content.contains(&format!("'{}", hashes)) {
        hashes.push('#');
    }
    format!("r{hashes}'{content}'{hashes}")
}

fn rewrite_string_method_chains(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '"' && c != '\'' {
            out.push(c);
            i += 1;
            continue;
        }

        let start = i;
        let quote = c;
        let mut escaped = false;
        i += 1;
        while i < chars.len() {
            let c = chars[i];
            i += 1;
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                break;
            }
        }
        let literal = normalize_string_literal(&chars[start..i].iter().collect::<String>());
        let mut j = i;
        while j < chars.len() && chars[j].is_whitespace() && chars[j] != '\n' {
            j += 1;
        }
        let method_chain =
            j + 1 < chars.len() && chars[j] == '.' && chars[j + 1].is_ascii_alphabetic();
        if method_chain {
            out.push_str("pat(");
            out.push_str(&literal);
            out.push(')');
        } else {
            out.push_str(&literal);
        }
    }
    out
}

fn delimiter_delta(line: &str) -> i64 {
    let mut delta = 0;
    let mut quote = None;
    let mut escaped = false;
    for c in line.chars() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
        } else if matches!(c, '(' | '[' | '{') {
            delta += 1;
        } else if matches!(c, ')' | ']' | '}') {
            delta -= 1;
        }
    }
    delta
}

fn label_at_line(line: &str) -> Option<(String, String)> {
    if line.chars().next().is_some_and(char::is_whitespace) {
        return None;
    }
    let mut end = 0;
    for (i, c) in line.char_indices() {
        let ok = if i == 0 {
            c.is_ascii_alphabetic() || c == '_' || c == '$'
        } else {
            c.is_ascii_alphanumeric() || c == '_' || c == '$'
        };
        if ok {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    let rest = &line[end..];
    rest.strip_prefix(':')
        .map(|expr| (line[..end].to_string(), expr.trim_start().to_string()))
}

fn top_level_boundary(line: &str) -> bool {
    !line.chars().next().is_some_and(char::is_whitespace) && !line.trim_start().starts_with('.')
}

fn sanitize_label(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        out.push_str("anon");
    }
    out
}

fn rewrite_labels(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut labels = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((name, rest)) = label_at_line(lines[i]) else {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        };

        let mut expr_lines = vec![rest];
        let mut depth = delimiter_delta(expr_lines[0].as_str());
        i += 1;
        while i < lines.len() {
            let line = lines[i];
            if depth <= 0 {
                if line.trim().is_empty() {
                    i += 1;
                    break;
                }
                if label_at_line(line).is_some() || top_level_boundary(line) {
                    break;
                }
            }
            expr_lines.push(line.to_string());
            depth += delimiter_delta(line);
            i += 1;
        }

        let var = format!("rudel_label_{}_{}", labels.len(), sanitize_label(&name));
        let expr = expr_lines.join("\n").trim().to_string();
        out.push(format!("{var} = rudel_label({name:?}, {expr})"));
        labels.push(var);
    }

    if !labels.is_empty() {
        out.push(format!("stack({})", labels.join(", ")));
    }
    out.join("\n")
}

pub(crate) fn preprocess_strudel(script: &str) -> String {
    let script = strip_line_comments(script);
    let script = rewrite_const_declarations(&script);
    let script = rewrite_string_method_chains(&script);
    rewrite_labels(&script)
}
