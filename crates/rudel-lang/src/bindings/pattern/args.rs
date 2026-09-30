use super::convert::{arg_to_f64, arg_to_frac, arg_to_pattern, arg_to_raw_str, to_value};
use crate::js::{Arg, NULL, Res, Scope};
use rudel_core::{Frac, Pattern, Value};

/// Argument `i`, or `undefined` when the call was shorter than that.
pub(super) fn arg(args: &[Arg], i: usize) -> &Arg {
    args.get(i).unwrap_or(NULL)
}

/// Register `name` as a pattern method: `f` gets the pattern it was called on.
pub(crate) fn method(
    proto: &Scope,
    name: &str,
    f: impl Fn(&Pattern, &[Arg]) -> Res + Send + Sync + 'static,
) {
    let label = name.to_string();
    proto.method(name, move |this, args| match this {
        Arg::Pat(pat) => f(pat, args),
        _ => Err(format!("{label}: not called on a pattern")),
    });
}

fn looks_like_mini_pattern(s: &str) -> bool {
    s.chars().any(|c| {
        c.is_whitespace() || matches!(c, '<' | '>' | '[' | ']' | ',' | '|' | '*' | '!' | '~')
    })
}

pub(super) fn literal_or_pattern_arg(value: &Arg) -> Pattern {
    match value {
        Arg::List(_) => rudel_core::pure(to_value(value)),
        _ => {
            // A non-mini-looking literal (plain or `m(...)`-wrapped) is kept as a
            // single string value rather than mini-parsed.
            if let Some(s) = arg_to_raw_str(value)
                && !looks_like_mini_pattern(&s)
            {
                return rudel_core::pure(Value::Str(s));
            }
            arg_to_pattern(value)
        }
    }
}

pub(super) fn pattern_arg(args: &[Arg], i: usize) -> Pattern {
    arg_to_pattern(arg(args, i))
}

pub(super) fn f64_arg(args: &[Arg], i: usize) -> f64 {
    arg_to_f64(arg(args, i))
}

pub(super) fn i64_arg(args: &[Arg], i: usize) -> i64 {
    f64_arg(args, i) as i64
}

pub(super) fn frac_arg(args: &[Arg], i: usize) -> Frac {
    arg_to_frac(arg(args, i))
}
