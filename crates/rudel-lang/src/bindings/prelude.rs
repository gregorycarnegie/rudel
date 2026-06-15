use super::pattern::{
    KPattern, arg_to_f64, arg_to_group, arg_to_pattern, arg_to_pattern_weight, arg_to_value,
    arg_to_weighted_pair, arg0, koto_to_value, pick_args,
};
use koto::prelude::*;
use rudel_core::{Frac, Pattern, PickJoin, Value};

macro_rules! register_unary_pattern_fns {
    ($p:expr; $($name:literal => $f:path),* $(,)?) => {
        $(
            $p.add_fn($name, |ctx| {
                Ok(KPattern($f(arg_to_pattern(&arg0(ctx)))).into())
            });
        )*
    };
}

macro_rules! register_pattern_list_fns {
    ($p:expr; $($name:literal => $f:path),* $(,)?) => {
        $(
            $p.add_fn($name, |ctx| {
                let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
                Ok(KPattern($f(&pats)).into())
            });
        )*
    };
}

macro_rules! register_pick_fns {
    ($p:expr; $($name:literal => ($modulo:expr, $join:expr)),* $(,)?) => {
        $(
            $p.add_fn($name, |ctx| {
                Ok(KPattern(pick_args(ctx.args(), $modulo, $join)).into())
            });
        )*
    };
}

/// Register the standalone (curried-style) form of pattern transforms that are
/// also methods, taking the pattern as the *last* argument to mirror Strudel's
/// `register`ed functions (`fast(2, pat)` == `pat.fast(2)`). Each group matches
/// the argument types in `generated.rs`'s `kpattern_methods!`. Koto has no
/// partial application, so only the fully-applied form is provided.
macro_rules! register_pattern_fns {
    ($p:expr;
     pattern1: [$($n_a1:literal => $a1:ident),* $(,)?];
     noarg:    [$($n_a0:literal => $a0:ident),* $(,)?];
     i64_1:    [$($n_b1:literal => $b1:ident),* $(,)?];
     f64_1:    [$($n_h1:literal => $h1:ident),* $(,)?];
     frac1:    [$($n_c1:literal => $c1:ident),* $(,)?];
     f64_2:    [$($n_d2:literal => $d2:ident),* $(,)?];
     frac2:    [$($n_e2:literal => $e2:ident),* $(,)?];
     i64_2:    [$($n_f2:literal => $f2:ident),* $(,)?];
     i64_3:    [$($n_i3:literal => $i3:ident),* $(,)?];
     i64_frac_f64: [$($n_ja:literal => $ja:ident),* $(,)?];
     i64_f64_frac: [$($n_jb:literal => $jb:ident),* $(,)?];
     pat2:     [$($n_g2:literal => $g2:ident),* $(,)?];
    ) => {{
        // The pattern is the last argument; leading arg `i` exists only when
        // `i < last` (otherwise it would be the pattern itself).
        $($p.add_fn($n_a1, |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = arg_to_pattern(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null));
            Ok(KPattern(pat.$a1(x)).into())
        });)*
        $($p.add_fn($n_a0, |ctx| {
            let a = ctx.args();
            let pat = arg_to_pattern(a.last().unwrap_or(&KValue::Null));
            Ok(KPattern(pat.$a0()).into())
        });)*
        $($p.add_fn($n_b1, |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let n = arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null)) as i64;
            Ok(KPattern(pat.$b1(n)).into())
        });)*
        $($p.add_fn($n_h1, |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let n = arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null));
            Ok(KPattern(pat.$h1(n)).into())
        });)*
        $($p.add_fn($n_c1, |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let n = Frac::from_f64(arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null)));
            Ok(KPattern(pat.$c1(n)).into())
        });)*
        $($p.add_fn($n_d2, |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null));
            let y = arg_to_f64(a.get(1).filter(|_| last >= 2).unwrap_or(&KValue::Null));
            Ok(KPattern(pat.$d2(x, y)).into())
        });)*
        $($p.add_fn($n_e2, |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = Frac::from_f64(arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null)));
            let y = Frac::from_f64(arg_to_f64(a.get(1).filter(|_| last >= 2).unwrap_or(&KValue::Null)));
            Ok(KPattern(pat.$e2(x, y)).into())
        });)*
        $($p.add_fn($n_f2, |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null)) as i64;
            let y = arg_to_f64(a.get(1).filter(|_| last >= 2).unwrap_or(&KValue::Null)) as i64;
            Ok(KPattern(pat.$f2(x, y)).into())
        });)*
        $($p.add_fn($n_i3, |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null)) as i64;
            let y = arg_to_f64(a.get(1).filter(|_| last >= 2).unwrap_or(&KValue::Null)) as i64;
            let z = arg_to_f64(a.get(2).filter(|_| last >= 3).unwrap_or(&KValue::Null)) as i64;
            Ok(KPattern(pat.$i3(x, y, z)).into())
        });)*
        $($p.add_fn($n_ja, |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null)) as i64;
            let y = Frac::from_f64(arg_to_f64(a.get(1).filter(|_| last >= 2).unwrap_or(&KValue::Null)));
            let z = arg_to_f64(a.get(2).filter(|_| last >= 3).unwrap_or(&KValue::Null));
            Ok(KPattern(pat.$ja(x, y, z)).into())
        });)*
        $($p.add_fn($n_jb, |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null)) as i64;
            let y = arg_to_f64(a.get(1).filter(|_| last >= 2).unwrap_or(&KValue::Null));
            let z = Frac::from_f64(arg_to_f64(a.get(2).filter(|_| last >= 3).unwrap_or(&KValue::Null)));
            Ok(KPattern(pat.$jb(x, y, z)).into())
        });)*
        $($p.add_fn($n_g2, |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = arg_to_pattern(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null));
            let y = arg_to_pattern(a.get(1).filter(|_| last >= 2).unwrap_or(&KValue::Null));
            Ok(KPattern(pat.$g2(x, y)).into())
        });)*
    }};
}

/// Add the rudel top-level functions to a Koto prelude.
pub(crate) fn register(prelude: &KMap) {
    // Make every rudel-core control available as a KPattern method (a
    // process-wide one-time extension of the generated method map).
    super::pattern::extend_control_entries();
    let math = KMap::new();
    math.add_fn("pow", |ctx| {
        let base = super::pattern::arg_to_f64(&arg0(ctx));
        let exponent = ctx
            .args()
            .get(1)
            .map(super::pattern::arg_to_f64)
            .unwrap_or(0.0);
        Ok(KValue::Number(KNumber::from(base.powf(exponent))))
    });
    prelude.insert("Math", math);

    register_unary_pattern_fns!(prelude;
        "note" => rudel_core::note,
        "n" => rudel_core::n,
        "i" => rudel_core::i,
        "freq" => rudel_core::freq,
        "mpe" => rudel_core::mpe,
        "bendRange" => rudel_core::bend_range,
        "s" => rudel_core::s,
        "sound" => rudel_core::sound,
    );
    prelude.add_fn("getFreq", |ctx| {
        let value = koto_to_value(&arg0(ctx));
        Ok(rudel_core::get_freq(&value).unwrap_or(0.0).into())
    });
    prelude.add_fn("silence", |_| Ok(KPattern(rudel_core::silence()).into()));
    // Strudel-style chord control: `chord("<Am C>").voicing()`.
    prelude.add_fn("chord", |ctx| {
        Ok(KPattern(rudel_core::control_dyn("chord", arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("rudel_label", |ctx| {
        let name = match ctx.args().first() {
            Some(KValue::Str(s)) => s.to_string(),
            _ => String::new(),
        };
        let pat = ctx
            .args()
            .get(1)
            .map(arg_to_pattern)
            .unwrap_or_else(rudel_core::silence);
        Ok(KPattern(pat.ctrl("id", rudel_core::pure(Value::Str(name)))).into())
    });
    register_pattern_list_fns!(prelude;
        "stack" => rudel_core::stack,
        "polyrhythm" => rudel_core::stack, // Strudel alias: polyrhythm = stack
        "pr" => rudel_core::stack,
        "stackLeft" => rudel_core::stack_left,
        "stackRight" => rudel_core::stack_right,
        "stackCentre" => rudel_core::stack_centre,
        "stackCenter" => rudel_core::stack_centre, // US spelling
        "cat" => rudel_core::cat,
        "seq" => rudel_core::fastcat,
        "sequence" => rudel_core::fastcat,
    );
    // `nothing` is an alias for `silence`.
    prelude.add_fn("nothing", |_| Ok(KPattern(rudel_core::silence()).into()));
    // stackBy(mode, ...pats): dispatch to a step-alignment by mode name.
    // (Strudel patternifies `mode`; here it is taken as a constant string.)
    prelude.add_fn("stackBy", |ctx| {
        let a = ctx.args();
        let mode = koto_to_value(a.first().unwrap_or(&KValue::Null));
        let pats: Vec<Pattern> = a
            .get(1..)
            .unwrap_or(&[])
            .iter()
            .map(arg_to_pattern)
            .collect();
        let out = match mode.as_str().unwrap_or("expand") {
            "left" => rudel_core::stack_left(&pats),
            "right" => rudel_core::stack_right(&pats),
            "centre" | "center" => rudel_core::stack_centre(&pats),
            "repeat" => rudel_core::polymeter(&pats),
            _ => rudel_core::stack(&pats), // "expand"
        };
        Ok(KPattern(out).into())
    });

    // -- Factories ---------------------------------------------------------
    // chooseCycles is randcat over reified args.
    register_pattern_list_fns!(prelude;
        "fastcat" => rudel_core::fastcat,
        "slowcat" => rudel_core::slowcat,
        "randcat" => rudel_core::randcat,
        "chooseCycles" => rudel_core::randcat,
    );

    prelude.add_fn("pure", |ctx| {
        Ok(KPattern(rudel_core::pure(arg_to_value(&arg0(ctx)))).into())
    });
    prelude.add_fn("gap", |ctx| {
        let n = super::pattern::arg_to_f64(&arg0(ctx)) as i64;
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
    prelude.add_fn("timeCat", stepcat);
    prelude.add_fn("s_cat", stepcat); // deprecated Strudel alias
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
    prelude.add_fn("s_polymeter", polymeter); // deprecated Strudel alias
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
    let stepalt = |ctx: &mut CallContext| {
        let groups: Vec<Vec<Pattern>> = ctx.args().iter().map(arg_to_group).collect();
        Ok(KPattern(rudel_core::stepalt(&groups)).into())
    };
    prelude.add_fn("stepalt", stepalt);
    prelude.add_fn("s_alt", stepalt); // deprecated Strudel alias
    // The pick family (strudel core/pick.mjs): select patterns from a list
    // (by index) or a map (by name) via a selector pattern. `pickmod*` wraps
    // out-of-range indices instead of clamping; the suffix picks the join.
    // squeeze(pat, xs): pick from a list with wrapping, squeezing the picked
    // pattern into the selecting event (strudel's standalone `squeeze`).
    register_pick_fns!(prelude;
        "pick" => (false, PickJoin::Inner),
        "pickmod" => (true, PickJoin::Inner),
        "pickOut" => (false, PickJoin::Outer),
        "pickmodOut" => (true, PickJoin::Outer),
        "pickReset" => (false, PickJoin::Reset),
        "pickmodReset" => (true, PickJoin::Reset),
        "pickRestart" => (false, PickJoin::Restart),
        "pickmodRestart" => (true, PickJoin::Restart),
        "inhabit" => (false, PickJoin::Squeeze),
        "pickSqueeze" => (false, PickJoin::Squeeze),
        "inhabitmod" => (true, PickJoin::Squeeze),
        "pickmodSqueeze" => (true, PickJoin::Squeeze),
        "squeeze" => (true, PickJoin::Squeeze),
    );
    prelude.add_fn("pat", |ctx| Ok(KPattern(arg_to_pattern(&arg0(ctx))).into()));
    // m(value, offset): mini-notation with a source offset. Emitted by the
    // preprocessor for every string literal so per-hap locations are absolute
    // to the editor source. Numbers/patterns pass through unchanged. The raw
    // source text is remembered so raw-string consumers can recover it.
    prelude.add_fn("m", |ctx| {
        let value = arg0(ctx);
        let offset = ctx
            .args()
            .get(1)
            .map(super::pattern::arg_to_f64)
            .unwrap_or(0.0) as usize;
        match &value {
            KValue::Str(s) => {
                let pat = rudel_mini::parse_with_offset(s, offset)
                    .unwrap_or_else(|_| rudel_core::silence())
                    .with_source(s.as_str());
                Ok(KPattern(pat).into())
            }
            _ => Ok(KPattern(arg_to_pattern(&value)).into()),
        }
    });
    prelude.add_fn("rev", |ctx| {
        Ok(KPattern(arg_to_pattern(&arg0(ctx)).rev()).into())
    });
    // scan: step through growing runs (run(1), run(2), ... run(n)).
    prelude.add_fn("scan", |ctx| {
        Ok(KPattern(rudel_core::scan(
            super::pattern::arg_to_f64(&arg0(ctx)) as i64
        ))
        .into())
    });
    // zip: interleave the steps of the given patterns into one dense cycle.
    let zip = |ctx: &mut CallContext| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        Ok(KPattern(rudel_core::zip(&pats)).into())
    };
    prelude.add_fn("zip", zip);
    prelude.add_fn("s_zip", zip); // deprecated Strudel alias
    // tour(pat, a, b, ...): standalone form of `pat.tour(a, b, ...)`.
    prelude.add_fn("tour", |ctx| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        let Some((head, many)) = pats.split_first() else {
            return Ok(KPattern(rudel_core::silence()).into());
        };
        Ok(KPattern(head.tour(many)).into())
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
        "tri" => rudel_core::tri, "itri" => rudel_core::itri,
        "square" => rudel_core::square,
        "sine2" => rudel_core::sine2, "cosine2" => rudel_core::cosine2,
        "saw2" => rudel_core::saw2, "isaw2" => rudel_core::isaw2,
        "tri2" => rudel_core::tri2, "itri2" => rudel_core::itri2,
        "square2" => rudel_core::square2,
        "rand" => rudel_core::rand, "rand2" => rudel_core::rand2,
        "brand" => rudel_core::brand,
        "time" => rudel_core::time,
        "perlin" => rudel_core::perlin, "berlin" => rudel_core::berlin,
        // Event-duration signals (take structure from the pattern they meet).
        "per" => rudel_core::per, "perCycle" => rudel_core::per,
        "cyclesPer" => rudel_core::cycles_per, "perx" => rudel_core::perx,
    );
    // brandBy(p): a 0/1 signal that is 1 with probability `p`.
    prelude.add_fn("brandBy", |ctx| {
        Ok(KPattern(rudel_core::brand_by(super::pattern::arg_to_f64(&arg0(ctx)))).into())
    });
    // steady(value): a continuous pattern of a single constant value.
    prelude.add_fn("steady", |ctx| {
        Ok(KPattern(rudel_core::steady(arg_to_value(&arg0(ctx)))).into())
    });
    // choose / chooseOut / chooseIn: continuously pick from the given values.
    // `choose`/`chooseOut` take structure from the random chooser; `chooseIn`
    // takes it from the chosen values.
    let choose = |ctx: &mut CallContext| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        Ok(KPattern(rudel_core::choose(&pats)).into())
    };
    prelude.add_fn("choose", choose);
    prelude.add_fn("chooseOut", choose);
    prelude.add_fn("chooseIn", |ctx| {
        let pats: Vec<Pattern> = ctx.args().iter().map(arg_to_pattern).collect();
        Ok(KPattern(rudel_core::choose_in(&pats)).into())
    });
    // Signals taking an integer count.
    prelude.add_fn("irand", |ctx| {
        Ok(KPattern(rudel_core::irand(
            super::pattern::arg_to_f64(&arg0(ctx)) as i64
        ))
        .into())
    });
    // randrun(n): the integers 0..n once each per cycle, in a random order.
    prelude.add_fn("randrun", |ctx| {
        Ok(KPattern(rudel_core::randrun(
            super::pattern::arg_to_f64(&arg0(ctx)) as i64
        ))
        .into())
    });
    prelude.add_fn("run", |ctx| {
        Ok(KPattern(rudel_core::run(
            super::pattern::arg_to_f64(&arg0(ctx)) as i64
        ))
        .into())
    });
    // MIDI input: `ccin(cc)` / `ccin(cc, chan)` is a 0..1 signal of the latest
    // value of an incoming control-change (the input counterpart to `ccn`).
    prelude.add_fn("ccin", |ctx| {
        let cc = super::pattern::arg_to_f64(&arg0(ctx)) as u8;
        let chan = ctx
            .args()
            .get(1)
            .map(|v| super::pattern::arg_to_f64(v) as u8)
            .filter(|c| *c >= 1);
        Ok(KPattern(rudel_core::cc_in(cc, chan)).into())
    });

    // Standalone (curried-style) forms of the transforms, so Strudel code
    // written as `fast(2, pat)` / `jux(rev, pat)` works as well as the method
    // forms, under both snake_case and Strudel's camelCase names. `rev` is
    // registered above. The function-callback combinators are registered
    // separately since their `Callback` plumbing lives in the pattern module.
    super::pattern::register_standalone_callbacks(prelude);

    // euclid morph / tuple-euclid standalone forms (pattern last); their
    // signatures don't fit the `register_pattern_fns!` arg groups.
    let euclidish_fn = |ctx: &mut CallContext| {
        let a = ctx.args();
        let pulses = arg_to_f64(a.first().unwrap_or(&KValue::Null)) as i64;
        let steps = arg_to_f64(a.get(1).unwrap_or(&KValue::Null)) as i64;
        let perc = arg_to_pattern(a.get(2).unwrap_or(&KValue::Null));
        let pat = arg_to_pattern(a.last().unwrap_or(&KValue::Null));
        Ok(KPattern(pat.euclidish(pulses, steps, perc)).into())
    };
    prelude.add_fn("euclidish", euclidish_fn);
    prelude.add_fn("eish", euclidish_fn);
    prelude.add_fn("bjork", |ctx| {
        let a = ctx.args();
        let euc: Vec<i64> = match a.first() {
            Some(KValue::List(l)) => l.data().iter().map(|v| arg_to_f64(v) as i64).collect(),
            Some(KValue::Tuple(t)) => t.data().iter().map(|v| arg_to_f64(v) as i64).collect(),
            Some(other) => vec![arg_to_f64(other) as i64],
            None => vec![],
        };
        let pat = arg_to_pattern(a.last().unwrap_or(&KValue::Null));
        Ok(KPattern(pat.bjork(&euc)).into())
    });

    register_pattern_fns!(prelude;
        pattern1: [
            "fast" => fast, "slow" => slow, "ply" => ply,
            "sparsity" => slow, // Strudel alias (`density` is a control, not fast)
            "segment" => segment, "seg" => seg,
            "add" => add, "sub" => sub, "mul" => mul, "div" => div, "modulo" => modulo,
            "set" => set, "keep" => keep, "keepif" => keepif, "mask" => mask, "bypass" => bypass,
            "early" => early, "late" => late,
            "lt" => lt, "gt" => gt, "lte" => lte, "gte" => gte,
            "eq" => eq, "eqt" => eqt, "ne" => ne, "net" => net,
            "fastGap" => fast_gap, "fast_gap" => fast_gap,
            "transpose" => transpose, "trans" => trans,
            "scaleTranspose" => scale_transpose, "scale_transpose" => scale_transpose,
            "scaleTrans" => strans, "strans" => strans,
        ];
        noarg: [
            "palindrome" => palindrome, "degrade" => degrade, "undegrade" => undegrade,
            "press" => press, "brak" => brak, "ratio" => ratio, "fit" => fit,
            "invert" => invert, "inv" => invert,
        ];
        i64_1: [
            "iter" => iter, "iterBack" => iter_back, "iter_back" => iter_back,
            "repeatCycles" => repeat_cycles, "repeat_cycles" => repeat_cycles,
            "expand" => expand, "extend" => extend, "contract" => contract,
            "shrink" => shrink, "grow" => grow,
            "chop" => chop, "striate" => striate, "take" => take, "drop" => drop,
            "rootNotes" => root_notes, "root_notes" => root_notes,
            "shuffle" => shuffle, "scramble" => scramble, "replicate" => replicate,
        ];
        f64_1: [
            "degradeBy" => degrade_by, "degrade_by" => degrade_by,
            "undegradeBy" => undegrade_by, "undegrade_by" => undegrade_by,
        ];
        frac1: [
            "hurry" => hurry, "swing" => swing,
            "pressBy" => press_by, "press_by" => press_by,
            "loopAt" => loop_at, "loop_at" => loop_at, "loopat" => loop_at,
            "pace" => pace, "seed" => seed, "linger" => linger,
        ];
        f64_2: ["range" => range, "range2" => range2, "rangex" => rangex];
        frac2: [
            "focus" => focus, "compress" => compress, "zoom" => zoom,
            "ribbon" => ribbon, "rib" => rib,
            "swingBy" => swing_by, "swing_by" => swing_by,
        ];
        i64_2: [
            "euclid" => euclid,
            "euclidLegato" => euclid_legato, "euclid_legato" => euclid_legato,
        ];
        i64_3: [
            "euclidRot" => euclid_rot, "euclid_rot" => euclid_rot,
            "euclidLegatoRot" => euclid_legato_rot, "euclid_legato_rot" => euclid_legato_rot,
        ];
        i64_frac_f64: ["echo" => echo];
        i64_f64_frac: ["stut" => stut];
        pat2: ["slice" => slice, "splice" => splice, "bite" => bite];
    );
}
