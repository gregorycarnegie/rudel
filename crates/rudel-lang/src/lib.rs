// rudel-lang - Koto scripting bindings for live-coding Rudel patterns.
// Exposes the rudel-core builder API to Koto so users can type code that is
// evaluated at runtime (Koto replaces JS as the live layer).
// SPDX-License-Identifier: AGPL-3.0-or-later

use koto::derive::*;
use koto::prelude::*;
use koto::runtime::{Error as KotoError, KotoObject, Result as KotoResult};
use rudel_core::{Frac, Pattern, Value};
use std::cell::RefCell;

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
        // Allow `"1/3"` style ratios in string arguments.
        KValue::Str(s) => match s.split_once('/') {
            Some((a, b)) => {
                let (a, b) = (a.trim().parse::<f64>(), b.trim().parse::<f64>());
                match (a, b) {
                    (Ok(a), Ok(b)) if b != 0.0 => a / b,
                    _ => 0.0,
                }
            }
            None => s.parse().unwrap_or(0.0),
        },
        _ => 0.0,
    }
}

fn arg_to_frac(value: &KValue) -> Frac {
    Frac::from_f64(arg_to_f64(value))
}

fn first_arg(ctx: &MethodContext<KPattern>) -> KValue {
    ctx.args.first().cloned().unwrap_or(KValue::Null)
}

fn nth_arg(ctx: &MethodContext<KPattern>, i: usize) -> KValue {
    ctx.args.get(i).cloned().unwrap_or(KValue::Null)
}

/// Marshals a Koto callable into the `Fn(&Pattern) -> Pattern` shape that the
/// engine's higher-order combinators (`every`, `jux`, `sometimes`, ...) expect.
///
/// Those combinators apply their callback *eagerly* at construction time, so we
/// can drive the callback synchronously on a VM spawned from the method's VM
/// (the immutable `MethodContext` VM can't call functions itself). The first
/// error raised by the callback is captured and surfaced once the combinator
/// returns; on error the input pattern is passed through unchanged.
struct Callback {
    vm: RefCell<KotoVm>,
    func: KValue,
    err: RefCell<Option<KotoError>>,
}

impl Callback {
    fn new(ctx: &MethodContext<KPattern>, func: KValue) -> Self {
        Self {
            vm: RefCell::new(ctx.vm.spawn_shared_vm()),
            func,
            err: RefCell::new(None),
        }
    }

    /// Invoke the Koto function with `p` wrapped as a `KPattern`.
    fn apply(&self, p: &Pattern) -> Pattern {
        let arg: KValue = KPattern(p.clone()).into();
        let call = self
            .vm
            .borrow_mut()
            .call_function(self.func.clone(), CallArgs::Single(arg));
        match call {
            Ok(KValue::Object(o)) if o.is_a::<KPattern>() => o.cast::<KPattern>().unwrap().0.clone(),
            Ok(_) => p.clone(),
            Err(e) => {
                if self.err.borrow().is_none() {
                    *self.err.borrow_mut() = Some(e);
                }
                p.clone()
            }
        }
    }

    /// Surface the first callback error (if any) after the combinator has run.
    fn finish(self) -> KotoResult<()> {
        match self.err.into_inner() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

macro_rules! kpattern_methods {
    (
        pattern_arg: [$($pattern_arg_method:ident),* $(,)?],
        no_arg: [$($no_arg_method:ident),* $(,)?],
        i64_arg: [$($i64_arg_method:ident),* $(,)?],
        fn_arg: [$($fn_arg_method:ident),* $(,)?],
        i64_fn_arg: [$($i64_fn_arg_method:ident),* $(,)?],
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

            // `pat.method(f)` where `f` is a Koto function `Pattern -> Pattern`.
            $(
                #[koto_method]
                fn $fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let pat = ctx.instance()?.0.clone();
                    let cb = Callback::new(&ctx, first_arg(&ctx));
                    let result = pat.$fn_arg_method(|p| cb.apply(p));
                    cb.finish()?;
                    Ok(Self::wrap(result))
                }
            )*

            // `pat.method(n, f)` where `n` is an integer and `f` a function.
            $(
                #[koto_method]
                fn $i64_fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = arg_to_f64(&first_arg(&ctx)) as i64;
                    let pat = ctx.instance()?.0.clone();
                    let cb = Callback::new(&ctx, nth_arg(&ctx, 1));
                    let result = pat.$i64_fn_arg_method(n, |p| cb.apply(p));
                    cb.finish()?;
                    Ok(Self::wrap(result))
                }
            )*

            #[koto_method]
            fn euclid(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let p = arg_to_f64(&first_arg(&ctx)) as i64;
                let s = arg_to_f64(&nth_arg(&ctx, 1)) as i64;
                Ok(Self::wrap(ctx.instance()?.0.euclid(p, s)))
            }

            // -- Higher-order combinators with non-uniform signatures ---------

            /// `off(time, f)`: stack a transformed copy shifted later in time.
            #[koto_method]
            fn off(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let time = arg_to_pattern(&first_arg(&ctx));
                let pat = ctx.instance()?.0.clone();
                let cb = Callback::new(&ctx, nth_arg(&ctx, 1));
                let result = pat.off(time, |p| cb.apply(p));
                cb.finish()?;
                Ok(Self::wrap(result))
            }

            /// `within(a, b, f)`: apply `f` only to the `[a, b]` slice of a cycle.
            #[koto_method]
            fn within(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let a = arg_to_frac(&first_arg(&ctx));
                let b = arg_to_frac(&nth_arg(&ctx, 1));
                let pat = ctx.instance()?.0.clone();
                let cb = Callback::new(&ctx, nth_arg(&ctx, 2));
                let result = pat.within(a, b, |p| cb.apply(p));
                cb.finish()?;
                Ok(Self::wrap(result))
            }

            /// `inside(n, f)`: apply `f` to a slowed view, then speed back up.
            #[koto_method]
            fn inside(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = arg_to_frac(&first_arg(&ctx));
                let pat = ctx.instance()?.0.clone();
                let cb = Callback::new(&ctx, nth_arg(&ctx, 1));
                let result = pat.inside(n, |p| cb.apply(p));
                cb.finish()?;
                Ok(Self::wrap(result))
            }

            /// `outside(n, f)`: apply `f` to a sped-up view, then slow back down.
            #[koto_method]
            fn outside(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = arg_to_frac(&first_arg(&ctx));
                let pat = ctx.instance()?.0.clone();
                let cb = Callback::new(&ctx, nth_arg(&ctx, 1));
                let result = pat.outside(n, |p| cb.apply(p));
                cb.finish()?;
                Ok(Self::wrap(result))
            }

            /// `jux_by(amount, f)`: pan-split copies and transform the right one.
            #[koto_method]
            fn jux_by(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let by = arg_to_f64(&first_arg(&ctx));
                let pat = ctx.instance()?.0.clone();
                let cb = Callback::new(&ctx, nth_arg(&ctx, 1));
                let result = pat.jux_by(by, |p| cb.apply(p));
                cb.finish()?;
                Ok(Self::wrap(result))
            }

            /// `sometimes_by(prob, f)`: apply `f` to a `prob` fraction of events.
            #[koto_method]
            fn sometimes_by(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let prob = arg_to_f64(&first_arg(&ctx));
                let pat = ctx.instance()?.0.clone();
                let cb = Callback::new(&ctx, nth_arg(&ctx, 1));
                let result = pat.sometimes_by(prob, |p| cb.apply(p));
                cb.finish()?;
                Ok(Self::wrap(result))
            }

            /// `some_cycles_by(prob, f)`: apply `f` on a `prob` fraction of cycles.
            #[koto_method]
            fn some_cycles_by(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let prob = arg_to_f64(&first_arg(&ctx));
                let pat = ctx.instance()?.0.clone();
                let cb = Callback::new(&ctx, nth_arg(&ctx, 1));
                let result = pat.some_cycles_by(prob, |p| cb.apply(p));
                cb.finish()?;
                Ok(Self::wrap(result))
            }

            /// `when(bools, f)`: apply `f` where the boolean pattern is true.
            #[koto_method]
            fn when(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let bools = arg_to_pattern(&first_arg(&ctx));
                let pat = ctx.instance()?.0.clone();
                let cb = Callback::new(&ctx, nth_arg(&ctx, 1));
                let result = pat.when(bools, |p| cb.apply(p));
                cb.finish()?;
                Ok(Self::wrap(result))
            }

            // -- Scalar transforms exposed from the engine --------------------

            /// `range(min, max)`: scale a unipolar (0..1) signal into `min..max`.
            #[koto_method]
            fn range(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let min = arg_to_f64(&first_arg(&ctx));
                let max = arg_to_f64(&nth_arg(&ctx, 1));
                Ok(Self::wrap(ctx.instance()?.0.range(min, max)))
            }

            /// `range2(min, max)`: scale a bipolar (-1..1) signal into `min..max`.
            #[koto_method]
            fn range2(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let min = arg_to_f64(&first_arg(&ctx));
                let max = arg_to_f64(&nth_arg(&ctx, 1));
                Ok(Self::wrap(ctx.instance()?.0.range2(min, max)))
            }

            /// `rangex(min, max)`: exponential range scaling.
            #[koto_method]
            fn rangex(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let min = arg_to_f64(&first_arg(&ctx));
                let max = arg_to_f64(&nth_arg(&ctx, 1));
                Ok(Self::wrap(ctx.instance()?.0.rangex(min, max)))
            }

            /// `swing(n)`: delay the off-beats of `n` slices per cycle.
            #[koto_method]
            fn swing(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = arg_to_frac(&first_arg(&ctx));
                Ok(Self::wrap(ctx.instance()?.0.swing(n)))
            }

            /// `swing_by(amount, n)`: like `swing`, with an explicit shift amount.
            #[koto_method]
            fn swing_by(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let amount = arg_to_frac(&first_arg(&ctx));
                let n = arg_to_frac(&nth_arg(&ctx, 1));
                Ok(Self::wrap(ctx.instance()?.0.swing_by(amount, n)))
            }

            /// `echo(times, time, feedback)`: stack decaying delayed copies.
            #[koto_method]
            fn echo(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let times = arg_to_f64(&first_arg(&ctx)) as i64;
                let time = arg_to_frac(&nth_arg(&ctx, 1));
                let feedback = arg_to_f64(&nth_arg(&ctx, 2));
                Ok(Self::wrap(ctx.instance()?.0.echo(times, time, feedback)))
            }

            /// `stut(times, feedback, time)`: `echo` with the legacy arg order.
            #[koto_method]
            fn stut(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let times = arg_to_f64(&first_arg(&ctx)) as i64;
                let feedback = arg_to_f64(&nth_arg(&ctx, 1));
                let time = arg_to_frac(&nth_arg(&ctx, 2));
                Ok(Self::wrap(ctx.instance()?.0.stut(times, feedback, time)))
            }

            /// `compress(b, e)`: squeeze each cycle into the `[b, e]` window.
            #[koto_method]
            fn compress(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let b = arg_to_frac(&first_arg(&ctx));
                let e = arg_to_frac(&nth_arg(&ctx, 1));
                Ok(Self::wrap(ctx.instance()?.0.compress(b, e)))
            }

            /// `zoom(s, e)`: play the `[s, e]` slice of a cycle over the full cycle.
            #[koto_method]
            fn zoom(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let s = arg_to_frac(&first_arg(&ctx));
                let e = arg_to_frac(&nth_arg(&ctx, 1));
                Ok(Self::wrap(ctx.instance()?.0.zoom(s, e)))
            }

            // -- Sample manipulation ------------------------------------------

            /// `chop(n)`: slice each sample into `n` pieces played in order.
            #[koto_method]
            fn chop(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = arg_to_f64(&first_arg(&ctx)) as i64;
                Ok(Self::wrap(ctx.instance()?.0.chop(n)))
            }

            /// `striate(n)`: interleave `n` sample slices across the cycle.
            #[koto_method]
            fn striate(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = arg_to_f64(&first_arg(&ctx)) as i64;
                Ok(Self::wrap(ctx.instance()?.0.striate(n)))
            }

            /// `slice(n, i)`: trigger slice `i` of `n` sample pieces.
            #[koto_method]
            fn slice(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = arg_to_pattern(&first_arg(&ctx));
                let i = arg_to_pattern(&nth_arg(&ctx, 1));
                Ok(Self::wrap(ctx.instance()?.0.slice(n, i)))
            }

            /// `splice(n, i)`: like `slice`, time-stretching each slice to its step.
            #[koto_method]
            fn splice(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = arg_to_pattern(&first_arg(&ctx));
                let i = arg_to_pattern(&nth_arg(&ctx, 1));
                Ok(Self::wrap(ctx.instance()?.0.splice(n, i)))
            }

            /// `loop_at(cycles)`: stretch a sample to span `cycles` cycles.
            #[koto_method]
            fn loop_at(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let cycles = arg_to_frac(&first_arg(&ctx));
                Ok(Self::wrap(ctx.instance()?.0.loop_at(cycles)))
            }

            /// `fit()`: stretch each sample to fill its own event duration.
            #[koto_method]
            fn fit(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                Ok(Self::wrap(ctx.instance()?.0.fit()))
            }

            // -- Tonal: scales, transpose, chords -----------------------------

            /// `scale(name)`: map scale-degree numbers to notes in `name`
            /// (e.g. `"C:major"`). The name is taken literally rather than as
            /// mini-notation, so `:` separates root from scale type.
            #[koto_method]
            fn scale(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let name = match first_arg(&ctx) {
                    KValue::Str(s) => rudel_core::pure(Value::Str(s.to_string())),
                    other => arg_to_pattern(&other),
                };
                Ok(Self::wrap(ctx.instance()?.0.scale(name)))
            }

            /// `transpose(semitones)`: shift each note by a number of semitones.
            #[koto_method]
            fn transpose(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let semis = arg_to_pattern(&first_arg(&ctx));
                Ok(Self::wrap(ctx.instance()?.0.transpose(semis)))
            }

            /// `scale_transpose(offset)`: transpose within the tagged scale.
            #[koto_method]
            fn scale_transpose(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let offset = arg_to_pattern(&first_arg(&ctx));
                Ok(Self::wrap(ctx.instance()?.0.scale_transpose(offset)))
            }

            /// `chord()`: expand chord names into stacks of simultaneous notes.
            #[koto_method]
            fn chord(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                Ok(Self::wrap(ctx.instance()?.0.chord()))
            }
        }
    };
}

kpattern_methods! {
    pattern_arg: [
        fast, slow, ply, segment, add, sub, mul, div, modulo, pow, set, keep, mask, struct_pat,
        early, late, fast_gap,
        note, n, s, gain, pan, speed, cutoff, resonance, room, size, shape, crush, delay,
        delaytime, delayfeedback, attack, decay, sustain, release, vowel, accelerate, coarse,
        orbit, velocity, begin, end, legato, clip,
        // alignment matrix (`in` is the default plain op; these are the rest)
        add_out, add_mix, add_squeeze, add_squeezeout, add_reset, add_restart,
        sub_out, mul_out, mul_squeeze, div_out,
        set_out, set_mix, set_squeeze, set_squeezeout,
        keep_out, keep_squeeze,
    ],
    no_arg: [
        rev, revv, palindrome, degrade, undegrade, press, brak, round, floor, ceil,
        to_bipolar, from_bipolar, ratio,
    ],
    i64_arg: [iter, iter_back, repeat_cycles],
    fn_arg: [
        superimpose, jux, sometimes, often, rarely, almost_always, almost_never, some_cycles,
    ],
    i64_fn_arg: [every, first_of, last_of, chunk, chunk_back],
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
    prelude.add_fn("perlin", |_| Ok(KPattern(rudel_core::perlin()).into()));
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

    fn values(pat: &Pattern, b: i64, e: i64) -> Vec<Value> {
        let mut haps = pat.query_arc(Frac::int(b), Frac::int(e));
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter().map(|h| h.value).collect()
    }

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

    #[test]
    fn every_with_koto_callback() {
        // every(2, |x| x.add(10)): cycle 0 -> 10, cycle 1 -> 0
        let pat = eval(r#"seq(0).every(2, |x| x.add(10))"#).expect("eval");
        assert_eq!(values(&pat, 0, 1)[0], Value::Int(10));
        assert_eq!(values(&pat, 1, 2)[0], Value::Int(0));
    }

    #[test]
    fn superimpose_with_koto_callback() {
        // superimpose(|x| x.add(7)) over a single value -> two haps
        let pat = eval(r#"seq(0).superimpose(|x| x.add(7))"#).expect("eval");
        assert_eq!(values(&pat, 0, 1), vec![Value::Int(0), Value::Int(7)]);
    }

    #[test]
    fn jux_with_koto_callback() {
        let pat = eval(r#"note("0 1").jux(|x| x.rev())"#).expect("eval");
        let pans: Vec<f64> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .filter_map(|h| match h.value {
                Value::Map(m) => m.get("pan").and_then(|v| v.as_f64()),
                _ => None,
            })
            .collect();
        assert!(pans.contains(&0.0) && pans.contains(&1.0));
    }

    #[test]
    fn within_with_koto_callback() {
        // apply +10 only to the first 40% of the cycle -> events 0 and 1
        let pat = eval(r#"seq(0, 1, 2, 3).within(0, 0.4, |x| x.add(10))"#).expect("eval");
        assert_eq!(
            values(&pat, 0, 1),
            vec![Value::Int(10), Value::Int(11), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn chunk_with_koto_callback() {
        // chunk(4, +10): first element bumped on cycle 0
        let pat = eval(r#"seq(0, 1, 2, 3).chunk(4, |x| x.add(10))"#).expect("eval");
        assert_eq!(
            values(&pat, 0, 1),
            vec![Value::Int(10), Value::Int(1), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn off_with_koto_callback() {
        // off(0.25, +12) stacks a shifted, transposed copy: two onsets per cycle
        let pat = eval(r#"note(0).off(0.25, |x| x.add(12))"#).expect("eval");
        let onsets = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .filter(|h| h.has_onset())
            .count();
        assert_eq!(onsets, 2);
    }

    #[test]
    fn range_scales_signal() {
        let pat = eval(r#"seq(0, 1).range(10, 20)"#).expect("eval");
        assert_eq!(values(&pat, 0, 1), vec![Value::F64(10.0), Value::F64(20.0)]);
    }

    #[test]
    fn scale_via_koto() {
        // n("0 2 4").scale("C:major") -> C3 E3 G3 = 48 52 55
        let pat = eval(r#"n("0 2 4").scale("C:major")"#).expect("eval");
        let mut got: Vec<f64> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| match h.value {
                Value::Map(m) => m.get("note").and_then(|v| v.as_f64()).unwrap(),
                other => other.as_f64().unwrap(),
            })
            .collect();
        got.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(got, vec![48.0, 52.0, 55.0]);
    }

    #[test]
    fn transpose_via_koto() {
        let pat = eval(r#"note(60).transpose(7)"#).expect("eval");
        let note = match &pat.query_arc(Frac::zero(), Frac::one())[0].value {
            Value::Map(m) => m.get("note").and_then(|v| v.as_f64()).unwrap(),
            other => other.as_f64().unwrap(),
        };
        assert_eq!(note, 67.0);
    }

    #[test]
    fn alignment_via_koto() {
        // add.out takes structure from the right pattern -> 3 onsets
        let pat = eval(r#"seq(0, 1).add_out("10 20 30")"#).expect("eval");
        let onsets = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .filter(|h| h.has_onset())
            .count();
        assert_eq!(onsets, 3);
        // set.squeeze merges the s control into each note event -> 4 haps
        let pat = eval(r#"note("0 1").set_squeeze(s("a b"))"#).expect("eval");
        assert_eq!(values(&pat, 0, 1).len(), 4);
    }

    #[test]
    fn chop_via_koto() {
        let pat = eval(r#"s("bd").chop(4)"#).expect("eval");
        assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
    }

    #[test]
    fn slice_via_koto() {
        let pat = eval(r#"s("bd").slice(4, "0 2")"#).expect("eval");
        let haps = pat.query_arc(Frac::zero(), Frac::one());
        assert_eq!(haps.len(), 2);
        match &haps[0].value {
            Value::Map(m) => assert_eq!(m.get("begin"), Some(&Value::F64(0.0))),
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn callback_error_is_surfaced() {
        // Referencing an undefined function inside the callback raises.
        let err = eval(r#"seq(0).every(2, |x| x.nonexistent_method())"#);
        assert!(err.is_err());
    }
}
