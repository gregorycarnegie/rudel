use super::pattern::{
    KPattern, arg_to_f64, arg_to_group, arg_to_pattern, arg_to_pattern_weight, arg_to_value,
    arg_to_weighted_pair, arg0, koto_to_value, pick_args,
};
use koto::prelude::*;
use rudel_core::{Frac, Pattern, PickJoin, Value};

/// Register the standalone (curried-style) form of pattern transforms that are
/// also methods, taking the pattern as the *last* argument to mirror Strudel's
/// `register`ed functions (`fast(2, pat)` == `pat.fast(2)`). Each group matches
/// the argument types in `generated.rs`'s `kpattern_methods!`. Koto has no
/// partial application, so only the fully-applied form is provided.
macro_rules! register_pattern_fns {
    ($p:expr;
     pattern1: [$($a1:ident),* $(,)?];
     noarg:    [$($a0:ident),* $(,)?];
     i64_1:    [$($b1:ident),* $(,)?];
     frac1:    [$($c1:ident),* $(,)?];
     f64_2:    [$($d2:ident),* $(,)?];
     frac2:    [$($e2:ident),* $(,)?];
     i64_2:    [$($f2:ident),* $(,)?];
     pat2:     [$($g2:ident),* $(,)?];
    ) => {{
        // The pattern is the last argument; a leading arg only exists when
        // there is something before it (otherwise it would be the pattern).
        $($p.add_fn(stringify!($a1), |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = arg_to_pattern(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null));
            Ok(KPattern(pat.$a1(x)).into())
        });)*
        $($p.add_fn(stringify!($a0), |ctx| {
            let a = ctx.args();
            let pat = arg_to_pattern(a.last().unwrap_or(&KValue::Null));
            Ok(KPattern(pat.$a0()).into())
        });)*
        $($p.add_fn(stringify!($b1), |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let n = arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null)) as i64;
            Ok(KPattern(pat.$b1(n)).into())
        });)*
        $($p.add_fn(stringify!($c1), |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let n = Frac::from_f64(arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null)));
            Ok(KPattern(pat.$c1(n)).into())
        });)*
        $($p.add_fn(stringify!($d2), |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null));
            let y = arg_to_f64(a.get(1).filter(|_| last >= 2).unwrap_or(&KValue::Null));
            Ok(KPattern(pat.$d2(x, y)).into())
        });)*
        $($p.add_fn(stringify!($e2), |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = Frac::from_f64(arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null)));
            let y = Frac::from_f64(arg_to_f64(a.get(1).filter(|_| last >= 2).unwrap_or(&KValue::Null)));
            Ok(KPattern(pat.$e2(x, y)).into())
        });)*
        $($p.add_fn(stringify!($f2), |ctx| {
            let a = ctx.args();
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
            let x = arg_to_f64(a.first().filter(|_| last >= 1).unwrap_or(&KValue::Null)) as i64;
            let y = arg_to_f64(a.get(1).filter(|_| last >= 2).unwrap_or(&KValue::Null)) as i64;
            Ok(KPattern(pat.$f2(x, y)).into())
        });)*
        $($p.add_fn(stringify!($g2), |ctx| {
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

    prelude.add_fn("note", |ctx| {
        Ok(KPattern(rudel_core::note(arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("n", |ctx| {
        Ok(KPattern(rudel_core::n(arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("i", |ctx| {
        Ok(KPattern(rudel_core::i(arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("freq", |ctx| {
        Ok(KPattern(rudel_core::freq(arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("mpe", |ctx| {
        Ok(KPattern(rudel_core::mpe(arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("bendRange", |ctx| {
        Ok(KPattern(rudel_core::bend_range(arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("getFreq", |ctx| {
        let value = koto_to_value(&arg0(ctx));
        Ok(rudel_core::get_freq(&value).unwrap_or(0.0).into())
    });
    prelude.add_fn("s", |ctx| {
        Ok(KPattern(rudel_core::s(arg_to_pattern(&arg0(ctx)))).into())
    });
    prelude.add_fn("sound", |ctx| {
        Ok(KPattern(rudel_core::sound(arg_to_pattern(&arg0(ctx)))).into())
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
    prelude.add_fn("pick", |ctx| {
        Ok(KPattern(pick_args(ctx.args(), false, PickJoin::Inner)).into())
    });
    prelude.add_fn("pickmod", |ctx| {
        Ok(KPattern(pick_args(ctx.args(), true, PickJoin::Inner)).into())
    });
    prelude.add_fn("pickOut", |ctx| {
        Ok(KPattern(pick_args(ctx.args(), false, PickJoin::Outer)).into())
    });
    prelude.add_fn("pickmodOut", |ctx| {
        Ok(KPattern(pick_args(ctx.args(), true, PickJoin::Outer)).into())
    });
    prelude.add_fn("pickReset", |ctx| {
        Ok(KPattern(pick_args(ctx.args(), false, PickJoin::Reset)).into())
    });
    prelude.add_fn("pickmodReset", |ctx| {
        Ok(KPattern(pick_args(ctx.args(), true, PickJoin::Reset)).into())
    });
    prelude.add_fn("pickRestart", |ctx| {
        Ok(KPattern(pick_args(ctx.args(), false, PickJoin::Restart)).into())
    });
    prelude.add_fn("pickmodRestart", |ctx| {
        Ok(KPattern(pick_args(ctx.args(), true, PickJoin::Restart)).into())
    });
    let inhabit = |ctx: &mut CallContext| {
        Ok(KPattern(pick_args(ctx.args(), false, PickJoin::Squeeze)).into())
    };
    prelude.add_fn("inhabit", inhabit);
    prelude.add_fn("pickSqueeze", inhabit);
    let inhabitmod =
        |ctx: &mut CallContext| Ok(KPattern(pick_args(ctx.args(), true, PickJoin::Squeeze)).into());
    prelude.add_fn("inhabitmod", inhabitmod);
    prelude.add_fn("pickmodSqueeze", inhabitmod);
    // squeeze(pat, xs): pick from a list with wrapping, squeezing the picked
    // pattern into the selecting event (strudel's standalone `squeeze`).
    prelude.add_fn("squeeze", inhabitmod);
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

    // Standalone (curried-style) forms of the common transforms, so Strudel
    // code written as `fast(2, pat)` works as well as `pat.fast(2)`. `rev` is
    // registered above. Function-callback transforms (`every`/`jux`/...) are
    // not yet exposed standalone (they would need partial application, which
    // Koto lacks).
    register_pattern_fns!(prelude;
        pattern1: [fast, slow, ply, segment, seg, add, sub, mul, div, early, late];
        noarg:    [palindrome, degrade, press, brak];
        i64_1:    [iter, chop, striate, take, drop, shuffle, scramble];
        frac1:    [hurry, swing];
        f64_2:    [range];
        frac2:    [compress, zoom];
        i64_2:    [euclid];
        pat2:     [slice, splice];
    );
}
