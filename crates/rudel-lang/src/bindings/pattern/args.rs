use super::KPattern;
use super::convert::{arg_to_f64, arg_to_frac, arg_to_pattern, koto_to_value};
use koto::prelude::*;
use koto::runtime::Result as KotoResult;
use rudel_core::{Frac, Pattern, Value};

pub(super) fn method_arg(ctx: &MethodContext<KPattern>, i: usize) -> KValue {
    ctx.args.get(i).cloned().unwrap_or(KValue::Null)
}

pub(super) fn method_pattern_arg(ctx: &MethodContext<KPattern>, i: usize) -> Pattern {
    arg_to_pattern(&method_arg(ctx, i))
}

fn looks_like_mini_pattern(s: &str) -> bool {
    s.chars().any(|c| {
        c.is_whitespace() || matches!(c, '<' | '>' | '[' | ']' | ',' | '|' | '*' | '!' | '~')
    })
}

fn literal_or_pattern_arg(value: &KValue) -> Pattern {
    match value {
        KValue::List(_) | KValue::Tuple(_) => rudel_core::pure(koto_to_value(value)),
        KValue::Str(s) if !looks_like_mini_pattern(s) => {
            rudel_core::pure(Value::Str(s.to_string()))
        }
        _ => arg_to_pattern(value),
    }
}

pub(super) fn method_literal_or_pattern_arg(ctx: &MethodContext<KPattern>, i: usize) -> Pattern {
    literal_or_pattern_arg(&method_arg(ctx, i))
}

pub(super) fn method_f64_arg(ctx: &MethodContext<KPattern>, i: usize) -> f64 {
    arg_to_f64(&method_arg(ctx, i))
}

pub(super) fn method_i64_arg(ctx: &MethodContext<KPattern>, i: usize) -> i64 {
    method_f64_arg(ctx, i) as i64
}

pub(super) fn method_frac_arg(ctx: &MethodContext<KPattern>, i: usize) -> Frac {
    arg_to_frac(&method_arg(ctx, i))
}

pub(super) fn with_instance(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern) -> Pattern,
) -> KotoResult<KValue> {
    let instance = ctx.instance()?;
    Ok(KPattern::wrap(f(&instance.0)))
}

pub(super) fn with_pattern_arg(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, Pattern) -> Pattern,
) -> KotoResult<KValue> {
    let arg = method_pattern_arg(ctx, 0);
    with_instance(ctx, |pat| f(pat, arg))
}

pub(super) fn with_literal_or_pattern_arg(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, Pattern) -> Pattern,
) -> KotoResult<KValue> {
    let arg = method_literal_or_pattern_arg(ctx, 0);
    with_instance(ctx, |pat| f(pat, arg))
}

pub(super) fn with_i64_arg(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, i64) -> Pattern,
) -> KotoResult<KValue> {
    let n = method_i64_arg(ctx, 0);
    with_instance(ctx, |pat| f(pat, n))
}

pub(super) fn with_frac_arg(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, Frac) -> Pattern,
) -> KotoResult<KValue> {
    let n = method_frac_arg(ctx, 0);
    with_instance(ctx, |pat| f(pat, n))
}

pub(super) fn with_f64_arg(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, f64) -> Pattern,
) -> KotoResult<KValue> {
    let n = method_f64_arg(ctx, 0);
    with_instance(ctx, |pat| f(pat, n))
}

pub(super) fn with_pattern_pattern_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, Pattern, Pattern) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_pattern_arg(ctx, 0);
    let b = method_pattern_arg(ctx, 1);
    with_instance(ctx, |pat| f(pat, a, b))
}

pub(super) fn with_frac_frac_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, Frac, Frac) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_frac_arg(ctx, 0);
    let b = method_frac_arg(ctx, 1);
    with_instance(ctx, |pat| f(pat, a, b))
}

pub(super) fn with_f64_f64_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, f64, f64) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_f64_arg(ctx, 0);
    let b = method_f64_arg(ctx, 1);
    with_instance(ctx, |pat| f(pat, a, b))
}

pub(super) fn with_i64_i64_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, i64, i64) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_i64_arg(ctx, 0);
    let b = method_i64_arg(ctx, 1);
    with_instance(ctx, |pat| f(pat, a, b))
}

pub(super) fn with_i64_i64_i64_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, i64, i64, i64) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_i64_arg(ctx, 0);
    let b = method_i64_arg(ctx, 1);
    let c = method_i64_arg(ctx, 2);
    with_instance(ctx, |pat| f(pat, a, b, c))
}

pub(super) fn with_i64_frac_f64_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, i64, Frac, f64) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_i64_arg(ctx, 0);
    let b = method_frac_arg(ctx, 1);
    let c = method_f64_arg(ctx, 2);
    with_instance(ctx, |pat| f(pat, a, b, c))
}

pub(super) fn with_i64_f64_frac_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, i64, f64, Frac) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_i64_arg(ctx, 0);
    let b = method_f64_arg(ctx, 1);
    let c = method_frac_arg(ctx, 2);
    with_instance(ctx, |pat| f(pat, a, b, c))
}
