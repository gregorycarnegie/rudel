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

impl KPattern {
    fn wrap(pat: Pattern) -> KValue {
        KPattern(pat).into()
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

/// Collect callable arguments for `layer`: a single list/tuple is expanded into
/// its elements, otherwise the varargs are used as-is.
fn collect_callables(args: &[KValue]) -> Vec<KValue> {
    match args {
        [KValue::List(l)] => l.data().iter().cloned().collect(),
        [KValue::Tuple(t)] => t.data().to_vec(),
        _ => args.to_vec(),
    }
}

/// Interpret an argument as a `(weight, pattern)` pair for `stepcat`/`arrange`.
/// A two-element list/tuple `[weight, pat]` sets the weight explicitly;
/// otherwise the pattern's own step count is used (defaulting to `1`).
fn arg_to_weighted_pair(value: &KValue) -> (Frac, Pattern) {
    let explicit = match value {
        KValue::List(l) => {
            let d = l.data();
            (d.len() == 2).then(|| (arg_to_frac(&d[0]), arg_to_pattern(&d[1])))
        }
        KValue::Tuple(t) => {
            let d = t.data();
            (d.len() == 2).then(|| (arg_to_frac(&d[0]), arg_to_pattern(&d[1])))
        }
        _ => None,
    };
    explicit.unwrap_or_else(|| {
        let pat = arg_to_pattern(value);
        let weight = pat.steps.unwrap_or_else(Frac::one);
        (weight, pat)
    })
}

/// Interpret an argument as a `[pattern, weight]` pair for the weighted
/// choosers (`wchoose`/`wrandcat`). A bare pattern defaults to weight `1`.
fn arg_to_pattern_weight(value: &KValue) -> (Pattern, f64) {
    let pair = |slice: &[KValue]| (arg_to_pattern(&slice[0]), arg_to_f64(&slice[1]));
    match value {
        KValue::List(l) if l.data().len() == 2 => pair(&l.data()),
        KValue::Tuple(t) if t.data().len() == 2 => pair(t.data()),
        _ => (arg_to_pattern(value), 1.0),
    }
}

/// Interpret an argument as a group of patterns for `stepalt`. A list/tuple
/// becomes a multi-element group; anything else is a single-element group.
fn arg_to_group(value: &KValue) -> Vec<Pattern> {
    match value {
        KValue::List(l) => l.data().iter().map(arg_to_pattern).collect(),
        KValue::Tuple(t) => t.data().iter().map(arg_to_pattern).collect(),
        _ => vec![arg_to_pattern(value)],
    }
}

fn method_arg(ctx: &MethodContext<KPattern>, i: usize) -> KValue {
    ctx.args.get(i).cloned().unwrap_or(KValue::Null)
}

fn method_pattern_arg(ctx: &MethodContext<KPattern>, i: usize) -> Pattern {
    arg_to_pattern(&method_arg(ctx, i))
}

fn method_f64_arg(ctx: &MethodContext<KPattern>, i: usize) -> f64 {
    arg_to_f64(&method_arg(ctx, i))
}

fn method_i64_arg(ctx: &MethodContext<KPattern>, i: usize) -> i64 {
    method_f64_arg(ctx, i) as i64
}

fn method_frac_arg(ctx: &MethodContext<KPattern>, i: usize) -> Frac {
    arg_to_frac(&method_arg(ctx, i))
}

fn with_instance(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern) -> Pattern,
) -> KotoResult<KValue> {
    let instance = ctx.instance()?;
    Ok(KPattern::wrap(f(&instance.0)))
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
            Ok(KValue::Object(o)) if o.is_a::<KPattern>() => {
                o.cast::<KPattern>().unwrap().0.clone()
            }
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

fn with_callback(
    ctx: &MethodContext<KPattern>,
    callback_arg: usize,
    f: impl FnOnce(Pattern, &Callback) -> Pattern,
) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let cb = Callback::new(ctx, method_arg(ctx, callback_arg));
    let result = f(pat, &cb);
    cb.finish()?;
    Ok(KPattern::wrap(result))
}

macro_rules! kpattern_methods {
    (
        pattern_arg: [$($pattern_arg_method:ident),* $(,)?],
        no_arg: [$($no_arg_method:ident),* $(,)?],
        i64_arg: [$($i64_arg_method:ident),* $(,)?],
        frac_arg: [$($frac_arg_method:ident),* $(,)?],
        pattern_pattern_arg: [$($pattern_pattern_arg_method:ident),* $(,)?],
        frac_frac_arg: [$($frac_frac_arg_method:ident),* $(,)?],
        f64_f64_arg: [$($f64_f64_arg_method:ident),* $(,)?],
        i64_i64_arg: [$($i64_i64_arg_method:ident),* $(,)?],
        i64_i64_i64_arg: [$($i64_i64_i64_arg_method:ident),* $(,)?],
        i64_frac_f64_arg: [$($i64_frac_f64_arg_method:ident),* $(,)?],
        i64_f64_frac_arg: [$($i64_f64_frac_arg_method:ident),* $(,)?],
        fn_arg: [$($fn_arg_method:ident),* $(,)?],
        i64_fn_arg: [$($i64_fn_arg_method:ident),* $(,)?],
        frac_fn_arg: [$($frac_fn_arg_method:ident),* $(,)?],
        f64_fn_arg: [$($f64_fn_arg_method:ident),* $(,)?],
        pattern_fn_arg: [$($pattern_fn_arg_method:ident),* $(,)?],
        frac_frac_fn_arg: [$($frac_frac_fn_arg_method:ident),* $(,)?],
    ) => {
        #[koto_impl]
        impl KPattern {
            $(
                #[koto_method]
                fn $pattern_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let arg = method_pattern_arg(&ctx, 0);
                    with_instance(&ctx, |pat| pat.$pattern_arg_method(arg))
                }
            )*

            $(
                #[koto_method]
                fn $no_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_instance(&ctx, |pat| pat.$no_arg_method())
                }
            )*

            $(
                #[koto_method]
                fn $i64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_i64_arg(&ctx, 0);
                    with_instance(&ctx, |pat| pat.$i64_arg_method(n))
                }
            )*

            $(
                #[koto_method]
                fn $frac_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_frac_arg(&ctx, 0);
                    with_instance(&ctx, |pat| pat.$frac_arg_method(n))
                }
            )*

            $(
                #[koto_method]
                fn $pattern_pattern_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let a = method_pattern_arg(&ctx, 0);
                    let b = method_pattern_arg(&ctx, 1);
                    with_instance(&ctx, |pat| pat.$pattern_pattern_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $frac_frac_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let a = method_frac_arg(&ctx, 0);
                    let b = method_frac_arg(&ctx, 1);
                    with_instance(&ctx, |pat| pat.$frac_frac_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $f64_f64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let a = method_f64_arg(&ctx, 0);
                    let b = method_f64_arg(&ctx, 1);
                    with_instance(&ctx, |pat| pat.$f64_f64_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $i64_i64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let a = method_i64_arg(&ctx, 0);
                    let b = method_i64_arg(&ctx, 1);
                    with_instance(&ctx, |pat| pat.$i64_i64_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $i64_i64_i64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let a = method_i64_arg(&ctx, 0);
                    let b = method_i64_arg(&ctx, 1);
                    let c = method_i64_arg(&ctx, 2);
                    with_instance(&ctx, |pat| pat.$i64_i64_i64_arg_method(a, b, c))
                }
            )*

            $(
                #[koto_method]
                fn $i64_frac_f64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let a = method_i64_arg(&ctx, 0);
                    let b = method_frac_arg(&ctx, 1);
                    let c = method_f64_arg(&ctx, 2);
                    with_instance(&ctx, |pat| pat.$i64_frac_f64_arg_method(a, b, c))
                }
            )*

            $(
                #[koto_method]
                fn $i64_f64_frac_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let a = method_i64_arg(&ctx, 0);
                    let b = method_f64_arg(&ctx, 1);
                    let c = method_frac_arg(&ctx, 2);
                    with_instance(&ctx, |pat| pat.$i64_f64_frac_arg_method(a, b, c))
                }
            )*

            // `pat.method(f)` where `f` is a Koto function `Pattern -> Pattern`.
            $(
                #[koto_method]
                fn $fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_callback(&ctx, 0, |pat, cb| pat.$fn_arg_method(|p| cb.apply(p)))
                }
            )*

            // `pat.method(n, f)` where `n` is an integer and `f` a function.
            $(
                #[koto_method]
                fn $i64_fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_i64_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$i64_fn_arg_method(n, |p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                fn $frac_fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_frac_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$frac_fn_arg_method(n, |p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                fn $f64_fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_f64_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$f64_fn_arg_method(n, |p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                fn $pattern_fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let arg = method_pattern_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$pattern_fn_arg_method(arg, |p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                fn $frac_frac_fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let a = method_frac_arg(&ctx, 0);
                    let b = method_frac_arg(&ctx, 1);
                    with_callback(&ctx, 2, |pat, cb| pat.$frac_frac_fn_arg_method(a, b, |p| cb.apply(p)))
                }
            )*

            // `pat.layer([f, g, ...])`: stack the results of applying each
            // function in the list to the pattern. Accepts a list/tuple of
            // callables, or bare callable args.
            #[koto_method]
            fn layer(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let pat = ctx.instance()?.0.clone();
                let funcs = collect_callables(&ctx.args);
                let mut results = Vec::with_capacity(funcs.len());
                let mut first_err = None;
                for func in funcs {
                    let cb = Callback::new(&ctx, func);
                    results.push(cb.apply(&pat));
                    if let Err(e) = cb.finish() {
                        first_err.get_or_insert(e);
                    }
                }
                if let Some(e) = first_err {
                    return Err(e);
                }
                Ok(KPattern::wrap(rudel_core::stack(&results)))
            }

            // `pat.voicings("lefthand")`: voice chords with a named dictionary.
            #[koto_method]
            fn voicings(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let dict = match method_arg(&ctx, 0) {
                    KValue::Str(s) => s.to_string(),
                    _ => "legacy".to_string(),
                };
                with_instance(&ctx, |pat| pat.voicings(dict.clone()))
            }

            #[koto_method]
            fn scale(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let name = match method_arg(&ctx, 0) {
                    KValue::Str(s) => rudel_core::pure(Value::Str(s.to_string())),
                    other => arg_to_pattern(&other),
                };
                with_instance(&ctx, |pat| pat.scale(name))
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
        hcutoff, hresonance, bandf, bandq,
        // filter envelopes + short aliases
        lpenv, lpattack, lpdecay, lpsustain, lprelease,
        hpenv, hpattack, hpdecay, hpsustain, hprelease,
        bpenv, bpattack, bpdecay, bpsustain, bprelease, fanchor,
        lpe, lpa, lpd, lps, lpr, hpe, hpa, hpd, hps, hpr, bpe, bpa, bpd, bps, bpr,
        // supersaw + FM + ADSR shortcuts
        unison, detune, spread, fm, fmh, fmi, adsr, ad, ar, hold,
        // vibrato + pitch envelope (+ aliases)
        vib, vibmod, penv, pattack, pdecay, psustain, prelease, panchor,
        vibrato, vmod, patt, pdec, psus, prel,
        // post-fx: tremolo + phaser
        tremolo, tremolodepth, phaser, phaserrate, phaserdepth, phasercenter, phasersweep,
        // filter / envelope / misc aliases
        lpf, lp, ctf, lpq, hpf, hp, hpq, bpf, bp, bpq, vel, att, rel, sus, dec,
        delayt, delayfb, o, trans, strans,
        // alignment matrix (`in` is the default plain op; these are the rest)
        add_out, add_mix, add_squeeze, add_squeezeout, add_reset, add_restart,
        sub_out, mul_out, mul_squeeze, div_out,
        set_out, set_mix, set_squeeze, set_squeezeout,
        keep_out, keep_squeeze,
        add_poly, mul_poly, set_poly, keep_poly,
        transpose, scale_transpose,
        overlay, arp,
    ],
    no_arg: [
        rev, revv, palindrome, degrade, undegrade, press, brak, round, floor, ceil,
        to_bipolar, from_bipolar, ratio, fit, chord, arpeggiate, voicing,
    ],
    i64_arg: [iter, iter_back, repeat_cycles, expand, extend, chop, striate, take, drop, root_notes],
    frac_arg: [hurry, press_by, swing, loop_at, pace],
    pattern_pattern_arg: [slice, splice],
    frac_frac_arg: [focus, swing_by, compress, zoom],
    f64_f64_arg: [range, range2, rangex],
    i64_i64_arg: [euclid],
    i64_i64_i64_arg: [euclid_rot],
    i64_frac_f64_arg: [echo],
    i64_f64_frac_arg: [stut],
    fn_arg: [
        superimpose, jux, sometimes, often, rarely, almost_always, almost_never, some_cycles,
    ],
    i64_fn_arg: [every, first_of, last_of, chunk, chunk_back],
    frac_fn_arg: [inside, outside],
    f64_fn_arg: [jux_by, sometimes_by, some_cycles_by],
    pattern_fn_arg: [off, when],
    frac_frac_fn_arg: [within],
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

    // -- Factories ---------------------------------------------------------
    prelude.add_fn("fastcat", |ctx| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        Ok(KPattern(rudel_core::fastcat(&pats)).into())
    });
    prelude.add_fn("slowcat", |ctx| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        Ok(KPattern(rudel_core::slowcat(&pats)).into())
    });
    prelude.add_fn("randcat", |ctx| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        Ok(KPattern(rudel_core::randcat(&pats)).into())
    });
    // chooseCycles is randcat over reified args.
    prelude.add_fn("chooseCycles", |ctx| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        Ok(KPattern(rudel_core::randcat(&pats)).into())
    });
    prelude.add_fn("pure", |ctx| {
        Ok(KPattern(rudel_core::pure(arg_to_value(&arg0(ctx)))).into())
    });
    prelude.add_fn("gap", |ctx| {
        let n = arg_to_f64(&arg0(ctx)) as i64;
        Ok(KPattern(rudel_core::gap(Frac::int(n.max(0)))).into())
    });
    // stepcat / timecat: weighted stepwise concatenation. Each arg is either a
    // pattern (weight = its step count) or a `[weight, pattern]` pair.
    let stepcat = |ctx: &mut CallContext| {
        let pairs: Vec<(Frac, Pattern)> = ctx.args().iter().map(arg_to_weighted_pair).collect();
        Ok(KPattern(rudel_core::timecat(&pairs)).into())
    };
    prelude.add_fn("stepcat", stepcat);
    prelude.add_fn("timecat", stepcat);
    // arrange: each arg is a `[cycles, pattern]` section laid out on a timeline.
    prelude.add_fn("arrange", |ctx| {
        let sections: Vec<(Frac, Pattern)> = ctx.args().iter().map(arg_to_weighted_pair).collect();
        Ok(KPattern(rudel_core::arrange(&sections)).into())
    });
    // polymeter / pm: align patterns to a common (LCM) step count.
    let polymeter = |ctx: &mut CallContext| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        Ok(KPattern(rudel_core::polymeter(&pats)).into())
    };
    prelude.add_fn("polymeter", polymeter);
    prelude.add_fn("pm", polymeter);
    // wchoose: continuously choose from weighted [pattern, weight] pairs.
    prelude.add_fn("wchoose", |ctx| {
        let pairs: Vec<(Pattern, f64)> = ctx.args().iter().map(arg_to_pattern_weight).collect();
        Ok(KPattern(rudel_core::wchoose(&pairs)).into())
    });
    // wchooseCycles / wrandcat: pick one weighted pattern per cycle.
    let wrandcat = |ctx: &mut CallContext| {
        let pairs: Vec<(Pattern, f64)> = ctx.args().iter().map(arg_to_pattern_weight).collect();
        Ok(KPattern(rudel_core::wrandcat(&pairs)).into())
    };
    prelude.add_fn("wchooseCycles", wrandcat);
    prelude.add_fn("wrandcat", wrandcat);
    // stepalt: alternate stepwise between groups of patterns.
    prelude.add_fn("stepalt", |ctx| {
        let groups: Vec<Vec<Pattern>> = ctx.args().iter().map(arg_to_group).collect();
        Ok(KPattern(rudel_core::stepalt(&groups)).into())
    });
    // scan: step through growing runs (run(1), run(2), ... run(n)).
    prelude.add_fn("scan", |ctx| {
        Ok(KPattern(rudel_core::scan(arg_to_f64(&arg0(ctx)) as i64)).into())
    });

    // -- Signals --------------------------------------------------------
    // Continuous signals are exposed as pattern *values* (like Strudel), so
    // `sine.range(0,1)` works without calling `sine()`.
    macro_rules! signal_val {
        ($($name:literal => $f:path),* $(,)?) => {
            $( prelude.insert($name, KPattern($f())); )*
        };
    }
    signal_val!(
        "sine" => rudel_core::sine, "cosine" => rudel_core::cosine,
        "saw" => rudel_core::saw, "isaw" => rudel_core::isaw,
        "tri" => rudel_core::tri, "square" => rudel_core::square,
        "sine2" => rudel_core::sine2, "cosine2" => rudel_core::cosine2,
        "saw2" => rudel_core::saw2, "isaw2" => rudel_core::isaw2,
        "tri2" => rudel_core::tri2, "square2" => rudel_core::square2,
        "rand" => rudel_core::rand, "rand2" => rudel_core::rand2,
        "time" => rudel_core::time, "perlin" => rudel_core::perlin,
    );
    // Signals taking an integer count.
    prelude.add_fn("irand", |ctx| {
        Ok(KPattern(rudel_core::irand(arg_to_f64(&arg0(ctx)) as i64)).into())
    });
    prelude.add_fn("run", |ctx| {
        Ok(KPattern(rudel_core::run(arg_to_f64(&arg0(ctx)) as i64)).into())
    });
}

fn arg0(ctx: &mut CallContext) -> KValue {
    ctx.args().first().cloned().unwrap_or(KValue::Null)
}

/// Convert a Koto value into a literal rudel [`Value`] (no mini-notation
/// parsing — used by `pure`).
fn arg_to_value(value: &KValue) -> Value {
    match value {
        KValue::Number(n) => {
            if n.is_i64() {
                Value::Int(n.into())
            } else {
                Value::F64(n.into())
            }
        }
        KValue::Bool(b) => Value::Bool(*b),
        KValue::Str(s) => Value::Str(s.to_string()),
        KValue::Object(o) if o.is_a::<KPattern>() => {
            // a pattern value (rare); wrap it
            Value::Pat(Box::new(o.cast::<KPattern>().unwrap().0.clone()))
        }
        _ => Value::Null,
    }
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
    fn signals_are_values_and_segment() {
        // sine is a value (no parens) and can be segmented + ranged
        let pat = eval(r#"sine.range(0, 10).segment(4)"#).expect("eval");
        assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
        // run(4) -> 0 1 2 3
        let pat = eval(r#"run(4)"#).expect("eval");
        assert_eq!(
            values(&pat, 0, 1),
            vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)]
        );
        // rand / perlin / saw2 usable bare
        for s in [
            "rand.segment(8)",
            "perlin.segment(8)",
            "saw2.segment(4)",
            "irand(8).segment(4)",
        ] {
            assert!(eval(s).is_ok(), "should eval: {s}");
        }
    }

    #[test]
    fn factories_resolve() {
        // slowcat: one value per cycle
        let pat = eval(r#"slowcat(0, 1, 2)"#).expect("eval");
        assert_eq!(values(&pat, 0, 1)[0], Value::Int(0));
        assert_eq!(values(&pat, 1, 2)[0], Value::Int(1));
        // pure literal, gap silence, fastcat/randcat resolve
        assert_eq!(
            values(&eval("pure(60)").unwrap(), 0, 1),
            vec![Value::Int(60)]
        );
        assert!(
            eval("gap(2)")
                .unwrap()
                .query_arc(Frac::zero(), Frac::one())
                .is_empty()
        );
        for s in ["fastcat(0, 1, 2)", "randcat(0, 1)", "chooseCycles(0, 1)"] {
            assert!(eval(s).is_ok(), "should eval: {s}");
        }
    }

    #[test]
    fn newly_bound_transforms_resolve() {
        for s in [
            r#"note(0).hurry(2)"#,
            r#"seq(0, 1, 2, 3).focus(0, 0.5)"#,
            r#"seq(0, 1).press_by(0.5)"#,
            r#"s("x").euclid_rot(3, 8, 1)"#,
        ] {
            assert!(eval(s).is_ok(), "should eval: {s}");
        }
    }

    #[test]
    fn filter_and_transpose_aliases_resolve() {
        // Previously-missing aliases should now evaluate without error.
        for src in [
            r#"note("c2").lpf(800).lpq(0.5)"#,
            r#"note("c2").hpf(400).bpf("200 800")"#,
            r#"note("c2").trans(7)"#,
            r#"note("c2").s("sawtooth").attack(0.1).decay(0.1).sustain(0.2).release(0.1)"#,
        ] {
            assert!(eval(src).is_ok(), "should eval: {src}");
        }
    }

    #[test]
    fn filter_envelopes_and_noise_resolve() {
        for src in [
            r#"note("c2").s("sawtooth").lpf(200).lpenv(4).lpa(0.1).lpd(0.2)"#,
            r#"note("c2").hpf(2000).hpenv(-3)"#,
            r#"s("white pink brown").lpf(1000)"#,
            r#"note("c2").s("saw").vowel("<a e i o>")"#,
        ] {
            assert!(eval(src).is_ok(), "should eval: {src}");
        }
    }

    #[test]
    fn supersaw_fm_adsr_resolve() {
        for src in [
            r#"note("c2").s("supersaw").unison(7).detune(20).spread(0.4)"#,
            r#"note("c3").s("sine").fm(4).fmh(2)"#,
            r#"s("bd*4").adsr("0.01:0.1:0:0.1")"#,
            r#"note("c3").s("saw").ad("0.01:0.2").hold(0.3)"#,
        ] {
            assert!(eval(src).is_ok(), "should eval: {src}");
        }
    }

    #[test]
    fn vibrato_and_pitch_env_resolve() {
        for src in [
            r#"note("c3").s("sine").vib(6).vibmod(0.5)"#,
            r#"note("c3").s("saw").penv(12).patt(0.2)"#,
            r#"note("c3").vibrato(5).vmod(1)"#,
        ] {
            assert!(eval(src).is_ok(), "should eval: {src}");
        }
    }

    #[test]
    fn tremolo_phaser_controls_resolve() {
        for src in [
            r#"note("c3").s("saw").tremolo(4).tremolodepth(0.6)"#,
            r#"note("c3").s("saw").phaser(0.5).phaserdepth(0.8)"#,
            r#"note("c3").s("saw").phaserrate(1).phasercenter(800).phasersweep(1500)"#,
        ] {
            assert!(eval(src).is_ok(), "should eval: {src}");
        }
        // the control lands on the hap map under its own key
        let pat = eval(r#"note("c3").tremolo(4)"#).expect("eval");
        let has = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .any(|h| match h.value {
                Value::Map(m) => m.get("tremolo").and_then(|v| v.as_f64()) == Some(4.0),
                _ => false,
            });
        assert!(has, "tremolo control should be set on the event map");
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
    fn layer_stacks_callback_results() {
        // layer([|x| x.add(0), |x| x.add(7)]) over a single value -> two haps
        let pat = eval(r#"seq(0).layer([|x| x.add(0), |x| x.add(7)])"#).expect("eval");
        let mut got = values(&pat, 0, 1);
        got.sort_by_key(|v| v.as_f64().unwrap() as i64);
        assert_eq!(got, vec![Value::Int(0), Value::Int(7)]);
    }

    #[test]
    fn factories_stepcat_arrange_polymeter() {
        // stepcat("0 1 2", "3 4") -> 5 evenly-weighted steps
        let pat = eval(r#"stepcat("0 1 2", "3 4")"#).expect("eval");
        assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 5);
        // explicit [weight, pat] pairs: "0"@3 then "1" -> 2 onsets, 0 dominates
        let pat = eval(r#"stepcat([3, "0"], [1, "1"])"#).expect("eval");
        assert_eq!(values(&pat, 0, 1), vec![Value::Int(0), Value::Int(1)]);
        // arrange: "0" for 2 cycles, "1" for 1
        let pat = eval(r#"arrange([2, "0"], [1, "1"])"#).expect("eval");
        assert_eq!(values(&pat, 0, 1)[0], Value::Int(0));
        assert_eq!(values(&pat, 2, 3)[0], Value::Int(1));
        // polymeter / pm align to lcm(3,2)=6 steps -> 12 haps stacked
        let pat = eval(r#"polymeter("0 1 2", "4 5")"#).expect("eval");
        assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 12);
        assert!(eval(r#"pm("0 1", "2 3 4")"#).is_ok());
    }

    #[test]
    fn take_drop_scan_via_koto() {
        // seq(0,1,2,3).take(2) -> "0 1"; drop(1) -> "1 2 3"
        let pat = eval(r#"seq(0, 1, 2, 3).take(2)"#).expect("eval");
        assert_eq!(values(&pat, 0, 1), vec![Value::Int(0), Value::Int(1)]);
        let pat = eval(r#"seq(0, 1, 2, 3).drop(1)"#).expect("eval");
        assert_eq!(
            values(&pat, 0, 1),
            vec![Value::Int(1), Value::Int(2), Value::Int(3)]
        );
        // scan(3): cycle 0 -> [0], cycle 2 -> [0 1 2]
        let pat = eval(r#"scan(3)"#).expect("eval");
        assert_eq!(values(&pat, 0, 1), vec![Value::Int(0)]);
        assert_eq!(
            values(&pat, 2, 3),
            vec![Value::Int(0), Value::Int(1), Value::Int(2)]
        );
    }

    #[test]
    fn weighted_choosers_and_stepalt_via_koto() {
        // wrandcat: heavy weight on 0 dominates, one value per cycle
        let pat = eval(r#"wrandcat([0, 1000], [1, 1])"#).expect("eval");
        let mut zeros = 0;
        for c in 0..12 {
            let v = values(&pat, c, c + 1);
            assert_eq!(v.len(), 1);
            if v[0] == Value::Int(0) {
                zeros += 1;
            }
        }
        assert!(zeros >= 10, "heavy weight should dominate (got {zeros}/12)");
        // wchooseCycles is the same function; wchoose evaluates as continuous
        assert!(eval(r#"wchooseCycles(["a", 2], ["b", 1])"#).is_ok());
        assert!(eval(r#"wchoose([0, 1], [1, 1]).segment(4)"#).is_ok());
        // stepalt(["0 1", "2"], "3") == "0 1 3 2 3"
        let pat = eval(r#"stepalt(["0 1", "2"], "3")"#).expect("eval");
        assert_eq!(
            values(&pat, 0, 1),
            vec![
                Value::Int(0),
                Value::Int(1),
                Value::Int(3),
                Value::Int(2),
                Value::Int(3),
            ]
        );
    }

    #[test]
    fn voicing_via_koto() {
        // a chord-symbol pattern voiced below a4: C triad -> C4 E4 G4.
        // (mini-notation can't spell `^`, so use `maj7`/`m7`-style symbols, or
        // pure("C^7") for the literal form.)
        let pat = eval(r#"pure("C").voicing()"#).expect("eval");
        let mut got = values(&pat, 0, 1);
        got.sort_by_key(|v| v.as_f64().unwrap() as i64);
        assert_eq!(got, vec![Value::F64(60.0), Value::F64(64.0), Value::F64(67.0)]);
        // named dictionary, literal ^ spelling via pure
        let pat = eval(r#"pure("C^7").voicings("lefthand")"#).expect("eval");
        assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
        // maj7 spelling routes through the same dictionary key
        let pat = eval(r#"pure("Cmaj7").voicings("lefthand")"#).expect("eval");
        assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
        // rootNotes maps a chord to its root in an octave
        let pat = eval(r#"pure("Am7").root_notes(3)"#).expect("eval");
        assert_eq!(values(&pat, 0, 1), vec![Value::F64(57.0)]); // A3
        // chord progressions resolve through mini-notation alternation
        assert!(eval(r#"seq("<Cmaj7 A7 Dm7 G7>").voicing()"#).is_ok());
    }

    #[test]
    fn arp_and_arpeggiate_via_koto() {
        // stack(0,1,2) is a chord; arp("0 1 2") walks up it
        let pat = eval(r#"stack(0, 1, 2).arp("0 1 2")"#).expect("eval");
        assert_eq!(
            values(&pat, 0, 1),
            vec![Value::Int(0), Value::Int(1), Value::Int(2)]
        );
        // arpeggiate plays the chord notes in sequence
        let pat = eval(r#"stack(5, 7, 9).arpeggiate()"#).expect("eval");
        assert_eq!(
            values(&pat, 0, 1),
            vec![Value::Int(5), Value::Int(7), Value::Int(9)]
        );
        // works on note chords from mini-notation too
        assert!(eval(r#"note("[c,e,g]").arp("0 1 2 1")"#).is_ok());
    }

    #[test]
    fn overlay_and_pace_via_koto() {
        let pat = eval(r#"seq(0).overlay(7)"#).expect("eval");
        let mut got = values(&pat, 0, 1);
        got.sort_by_key(|v| v.as_f64().unwrap() as i64);
        assert_eq!(got, vec![Value::Int(0), Value::Int(7)]);
        let pat = eval(r#"seq(0, 1, 2).pace(4)"#).expect("eval");
        assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
    }

    #[test]
    fn callback_error_is_surfaced() {
        // Referencing an undefined function inside the callback raises.
        let err = eval(r#"seq(0).every(2, |x| x.nonexistent_method())"#);
        assert!(err.is_err());
    }
}
