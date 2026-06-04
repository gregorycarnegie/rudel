// rudel-lang - Koto scripting bindings for live-coding Rudel patterns.
// Exposes the rudel-core builder API to Koto so users can type code that is
// evaluated at runtime (Koto replaces JS as the live layer).
// SPDX-License-Identifier: AGPL-3.0-or-later

use koto::derive::*;
use koto::prelude::*;
use koto::runtime::{KotoObject, Result as KotoResult};
use rudel_core::{Pattern, Value};

/// A Koto wrapper around a rudel [`Pattern`].
#[derive(Clone, KotoCopy, KotoType)]
pub struct KPattern(pub Pattern);

impl KotoObject for KPattern {}

impl From<KPattern> for KValue {
    fn from(p: KPattern) -> KValue {
        KObject::from(p).into()
    }
}

/// Convert a Koto argument into a pattern: numbers become `pure` values,
/// strings parse as mini-notation, and patterns pass through.
fn arg_to_pattern(value: &KValue) -> Pattern {
    match value {
        KValue::Number(n) => {
            if n.is_i64() {
                rudel_core::pure(Value::Int(n.into()))
            } else {
                rudel_core::pure(Value::F64(n.into()))
            }
        }
        KValue::Str(s) => rudel_mini::parse(s).unwrap_or_else(|_| rudel_core::silence()),
        KValue::Object(o) if o.is_a::<KPattern>() => o.cast::<KPattern>().unwrap().0.clone(),
        _ => rudel_core::silence(),
    }
}

fn arg_to_f64(value: &KValue) -> f64 {
    match value {
        KValue::Number(n) => n.into(),
        KValue::Str(s) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn first_arg(ctx: &MethodContext<KPattern>) -> KValue {
    ctx.args.first().cloned().unwrap_or(KValue::Null)
}

macro_rules! kpattern_methods {
    (
        pattern_arg: [$($pattern_arg_method:ident),* $(,)?],
        no_arg: [$($no_arg_method:ident),* $(,)?],
        i64_arg: [$($i64_arg_method:ident),* $(,)?],
    ) => {
        #[koto_impl]
        impl KPattern {
            fn wrap(pat: Pattern) -> KValue {
                KPattern(pat).into()
            }

            $(
                #[koto_method]
                fn $pattern_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let arg = arg_to_pattern(&first_arg(&ctx));
                    Ok(Self::wrap(ctx.instance()?.0.$pattern_arg_method(arg)))
                }
            )*

            $(
                #[koto_method]
                fn $no_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    Ok(Self::wrap(ctx.instance()?.0.$no_arg_method()))
                }
            )*

            $(
                #[koto_method]
                fn $i64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = arg_to_f64(&first_arg(&ctx)) as i64;
                    Ok(Self::wrap(ctx.instance()?.0.$i64_arg_method(n)))
                }
            )*

            #[koto_method]
            fn euclid(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let p = arg_to_f64(&ctx.args.first().cloned().unwrap_or(KValue::Null)) as i64;
                let s = arg_to_f64(&ctx.args.get(1).cloned().unwrap_or(KValue::Null)) as i64;
                Ok(Self::wrap(ctx.instance()?.0.euclid(p, s)))
            }
        }
    };
}

kpattern_methods! {
    pattern_arg: [
        fast, slow, ply, segment, add, sub, mul, note, n, s, gain, pan, speed, cutoff, room, delay,
    ],
    no_arg: [rev, palindrome, degrade],
    i64_arg: [iter],
}

/// Add the rudel top-level functions to a Koto prelude.
fn register(prelude: &KMap) {
    prelude.add_fn("note", |ctx| {
        Ok(KPattern(rudel_core::note(arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("n", |ctx| {
        Ok(KPattern(rudel_core::n(arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("s", |ctx| {
        Ok(KPattern(rudel_core::s(arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("sound", |ctx| {
        Ok(KPattern(rudel_core::sound(arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("silence", |_| Ok(KPattern(rudel_core::silence()).into()));
    prelude.add_fn("stack", |ctx| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        Ok(KPattern(rudel_core::stack(&pats)).into())
    });
    prelude.add_fn("cat", |ctx| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        Ok(KPattern(rudel_core::cat(&pats)).into())
    });
    prelude.add_fn("seq", |ctx| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        Ok(KPattern(rudel_core::fastcat(&pats)).into())
    });
}

fn arg0(ctx: &mut CallContext) -> KValue {
    ctx.args().first().cloned().unwrap_or(KValue::Null)
}

/// Evaluate a Koto script and extract the resulting pattern.
pub fn eval(script: &str) -> Result<Pattern, String> {
    let mut koto = Koto::default();
    register(koto.prelude());
    let chunk = koto.compile(script).map_err(|e| e.to_string())?;
    let result = koto.run(chunk).map_err(|e| e.to_string())?;
    match result {
        KValue::Object(o) if o.is_a::<KPattern>() => Ok(o.cast::<KPattern>().unwrap().0.clone()),
        other => Err(format!("script did not return a pattern (got {other:?})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudel_core::Frac;

    #[test]
    fn eval_simple_pattern() {
        let pat = eval(r#"note("c4 e4 g4").fast(2)"#).expect("eval");
        let haps = pat.query_arc(Frac::zero(), Frac::one());
        assert_eq!(haps.len(), 6);
    }

    #[test]
    fn eval_stack_and_controls() {
        let pat = eval(r#"stack(s("bd*2"), note("c4 e4").gain(0.5))"#).expect("eval");
        assert!(!pat.query_arc(Frac::zero(), Frac::one()).is_empty());
    }

    #[test]
    fn non_pattern_result_errors() {
        assert!(eval("1 + 2").is_err());
    }
}
