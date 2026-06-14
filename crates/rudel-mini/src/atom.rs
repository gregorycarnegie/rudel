use rudel_core::Value;

/// Classify one step token like mini.mjs does: `~`/`-` are rests (None),
/// strings JavaScript's `Number()` accepts become numbers, the rest stay
/// strings.
pub(crate) fn atom_value(s: &str) -> Option<Value> {
    if s == "~" || s == "-" {
        return None;
    }
    Some(match js_number(s) {
        Some(x) => num_value(x),
        None => Value::Str(s.to_string()),
    })
}

pub(crate) fn num_value(x: f64) -> Value {
    if x.fract() == 0.0 && x.is_finite() && x.abs() < 9.007199254740992e15 {
        Value::Int(x as i64)
    } else {
        Value::F64(x)
    }
}

/// JavaScript `Number()` semantics for the strings the step rule can produce
/// (no `+`, no whitespace): decimal literals with optional exponent,
/// `0x`/`0o`/`0b` radix literals (unsigned only), and `Infinity`.
fn js_number(s: &str) -> Option<f64> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let radix = |prefixes: [&str; 2], radix: u32| {
        let digits = prefixes.iter().find_map(|p| body.strip_prefix(p))?;
        if neg {
            return None; // JS rejects signed radix literals
        }
        u64::from_str_radix(digits, radix).ok().map(|n| n as f64)
    };
    let val = if body == "Infinity" {
        Some(f64::INFINITY)
    } else if body.starts_with("0x") || body.starts_with("0X") {
        radix(["0x", "0X"], 16)
    } else if body.starts_with("0b") || body.starts_with("0B") {
        radix(["0b", "0B"], 2)
    } else if body.starts_with("0o") || body.starts_with("0O") {
        radix(["0o", "0O"], 8)
    } else if is_js_decimal(body) {
        body.parse::<f64>().ok()
    } else {
        None
    };
    val.map(|v| if neg { -v } else { v })
}

/// Validate a JS decimal literal: `digits[.digits]` or `.digits`, with an
/// optional exponent. Rust's f64 parser is more permissive (`inf`, `nan`),
/// so validation must happen before parsing.
fn is_js_decimal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
            i += 1;
        }
        let mut exp_digits = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            exp_digits += 1;
        }
        if exp_digits == 0 {
            return false;
        }
    }
    i == b.len()
}
