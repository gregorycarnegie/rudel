use super::{
    args::{arg, f64_arg, literal_or_pattern_arg, pattern_arg},
    callback::{Callback, static_period_pattern, with_callback},
    convert::{arg_to_f64, arg_to_frac, arg_to_pattern, arg_to_raw_str, to_value, value_to_arg},
    engine::hap_to_arg,
    pick::{is_lookup, lookup_from_arg, pick_from_lookup},
};
use crate::bindings::routing::IO_KEY;
use crate::js::{Arg, Res};
use rudel_core::{Frac, Pattern, PickJoin, Value};
use std::{collections::HashMap, sync::Arc};

/// A stable string key for a chord value, used to memoise `arp_with` callback
/// results so the script only runs at construction time.
pub(super) fn value_sig(v: &Value) -> String {
    match v {
        Value::Null => "_".into(),
        Value::Bool(b) => format!("b{b}"),
        Value::Int(n) => format!("i{n}"),
        Value::F64(x) => format!("f{x}"),
        Value::Frac(f) => format!("r{}/{}", f.numer(), f.denom()),
        Value::Str(s) => format!("s{s}"),
        Value::List(xs) => format!(
            "[{}]",
            xs.iter().map(value_sig).collect::<Vec<_>>().join(",")
        ),
        Value::Map(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{k}={}", value_sig(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Func(_) => "fn".into(),
        Value::Pat(_) => "pat".into(),
    }
}

/// Collect vararg-style arguments (`layer`, `tour`): a single array is
/// expanded into its elements, otherwise the varargs are used as-is.
fn collect_callables(args: &[Arg]) -> Vec<Arg> {
    match args {
        [Arg::List(l)] => l.clone(),
        _ => args.to_vec(),
    }
}

/// `pat.tour(a, b, ...)`: insert the pattern into the list of patterns
/// stepwise, moving backwards one slot per repetition (also accepts a single
/// array of patterns).
pub(super) fn kpattern_tour(pat: &Pattern, a: &[Arg]) -> Res {
    let many: Vec<Pattern> = collect_callables(a).iter().map(arg_to_pattern).collect();
    Ok(pat.tour(&many).into())
}

/// `pat.FX(fx1, fx2, ...)`: put the pattern through a chain of effects, each a
/// pattern of controls. Repeated calls extend the chain rather than replacing
/// it, and the pattern's own controls are heard after all of them.
pub(super) fn kpattern_fx(pat: &Pattern, a: &[Arg]) -> Res {
    let stages: Vec<Pattern> = a.iter().map(arg_to_pattern).collect();
    Ok(pat.fx(&stages).into())
}

/// `pat.loopAtCps(factor, cps)`: like `loopAt` but with an explicit cps
/// (deprecated in Strudel; kept for parity).
pub(super) fn kpattern_loop_at_cps(pat: &Pattern, a: &[Arg]) -> Res {
    let factor = arg_to_frac(arg(a, 0));
    let cps = arg_to_f64(arg(a, 1));
    Ok(pat.loop_at_cps(factor, cps).into())
}

/// `plyWith`'s per-value copies: `f` applied cumulatively (0×, 1×, 2×, …).
pub(super) fn ply_with_parts(x: &Value, cb: &Callback, factor: i64) -> Vec<Pattern> {
    (0..factor)
        .map(|i| {
            let mut p = rudel_core::pure(x.clone());
            for _ in 0..i {
                p = cb.apply(&p);
            }
            p
        })
        .collect()
}

/// `plyForEach`'s per-value copies: the first is untransformed, the rest are
/// `f(copy, i)`.
pub(super) fn ply_for_each_parts(x: &Value, cb: &Callback, factor: i64) -> Vec<Pattern> {
    let mut parts = vec![rudel_core::pure(x.clone())];
    for i in 1..factor {
        parts.push(cb.apply2(&rudel_core::pure(x.clone()), i));
    }
    parts
}

/// Shared core of `plyWith`/`plyForEach`: per value, build a `cat` of `factor`
/// transformed copies, speed it up to one cycle, and squeeze it into the
/// value's span. The script does not run on the query path, so the per-value
/// copies are probed and baked (as in `arp_with`).
pub(super) fn ply_build(
    pat: &Pattern,
    factor: i64,
    cb: &Callback,
    parts: impl Fn(&Value, &Callback, i64) -> Vec<Pattern>,
) -> Pattern {
    const PROBE: i64 = 16;
    let mut table: HashMap<String, Pattern> = HashMap::new();
    if factor > 0 {
        for cycle in 0..PROBE {
            for hap in pat.query_arc(Frac::int(cycle), Frac::int(cycle + 1)) {
                table.entry(value_sig(&hap.value)).or_insert_with(|| {
                    rudel_core::cat(&parts(&hap.value, cb, factor))._fast(Frac::int(factor))
                });
            }
        }
    }
    let table = Arc::new(table);
    let steps = pat.steps.map(|s| s * Frac::int(factor.max(1)));
    pat.fmap(move |v| {
        let inner = table
            .get(&value_sig(&v))
            .cloned()
            .unwrap_or_else(rudel_core::silence);
        Value::Pat(Box::new(inner))
    })
    .squeeze_join()
    .set_steps(steps)
}

/// The euclid family, with counts that may be patterns:
/// `s("bd").euclid("<3 5>", 8)` alternates rhythm by cycle, the way
/// mini-notation's `bd(<3 5>,8)` already did.
///
/// Plain numbers keep the direct path. It produces the same haps as the
/// patternified one, but building the rhythm per cycle through an `inner_join`
/// only to arrive at a constant is work — and the direct call is what every
/// existing golden was recorded against.
///
/// `rotated` says whether the third count is a rotation, and `build` is the
/// integer method the counts end up in, so the four exports share one body.
pub(in crate::bindings) fn euclid_call(
    pat: &Pattern,
    counts: [Option<&Arg>; 3],
    rotated: bool,
    build: fn(&Pattern, i64, i64, i64) -> Pattern,
) -> Pattern {
    let arity = if rotated { 3 } else { 2 };
    let count = |i: usize| counts[i].unwrap_or(&Arg::Null);
    if counts[..arity]
        .iter()
        .all(|c| matches!(c, Some(Arg::Num(_))))
    {
        let n = |i: usize| arg_to_f64(count(i)) as i64;
        return build(pat, n(0), n(1), if rotated { n(2) } else { 0 });
    }
    pat.euclid_pat(
        arg_to_pattern(count(0)),
        arg_to_pattern(count(1)),
        rotated.then(|| arg_to_pattern(count(2))),
        build,
    )
}

/// A stepwise transform whose count may be a pattern: `expand("3 2 1 1 2 3")`
/// is three (then two, then one…) differently-expanded copies laid end to end,
/// not one copy per sixth of a cycle.
///
/// Upstream registers these with `stepJoin` rather than the usual `innerJoin`
/// (`stepRegister`), which is the whole content of "the argument can also be
/// patterned, and will be treated in a stepwise fashion" on the stepwise page.
/// A plain number keeps the direct path, as for the euclid family above.
pub(in crate::bindings) fn stepwise_call(
    pat: &Pattern,
    arg: Option<&Arg>,
    build: fn(&Pattern, i64) -> Pattern,
) -> Pattern {
    match arg {
        Some(Arg::Num(n)) => build(pat, *n as i64),
        Some(other) => pat.stepwise_pat(arg_to_pattern(other), build),
        None => build(pat, 0),
    }
}

fn kpattern_euclid_family(
    pat: &Pattern,
    a: &[Arg],
    rotated: bool,
    build: fn(&Pattern, i64, i64, i64) -> Pattern,
) -> Res {
    let counts = [0, 1, 2].map(|i| a.get(i));
    Ok(euclid_call(pat, counts, rotated, build).into())
}

pub(super) fn kpattern_euclid(pat: &Pattern, a: &[Arg]) -> Res {
    kpattern_euclid_family(pat, a, false, |pat, a, b, _| pat.euclid(a, b))
}

pub(super) fn kpattern_euclid_rot(pat: &Pattern, a: &[Arg]) -> Res {
    kpattern_euclid_family(pat, a, true, Pattern::euclid_rot)
}

pub(super) fn kpattern_euclid_legato(pat: &Pattern, a: &[Arg]) -> Res {
    kpattern_euclid_family(pat, a, false, |pat, a, b, _| pat.euclid_legato(a, b))
}

pub(super) fn kpattern_euclid_legato_rot(pat: &Pattern, a: &[Arg]) -> Res {
    kpattern_euclid_family(pat, a, true, Pattern::euclid_legato_rot)
}

/// `pat.setSteps(n)`: set the step-count metadata without resampling
/// (stepwise.mjs `setSteps`). `null` clears it, which is how a pattern says it
/// has no step count to stack or concatenate by.
pub(super) fn kpattern_set_steps(pat: &Pattern, a: &[Arg]) -> Res {
    let steps = match arg(a, 0) {
        Arg::Null => None,
        other => Some(arg_to_frac(other)),
    };
    Ok(pat.clone().set_steps(steps).into())
}

/// `pat.plyWith(factor, f)`: repeat each event `factor` times, applying `f`
/// cumulatively (`f` 0×, 1×, 2×, … like `applyN`).
pub(super) fn kpattern_ply_with(pat: &Pattern, a: &[Arg]) -> Res {
    let factor = arg_to_f64(arg(a, 0)) as i64;
    with_callback(pat, a, 1, |pat, cb| {
        ply_build(pat, factor, cb, ply_with_parts)
    })
}

/// `pat.plyForEach(factor, f)`: repeat each event `factor` times, applying
/// `f(copy, i)` to each repeat (the first is left untransformed).
pub(super) fn kpattern_ply_for_each(pat: &Pattern, a: &[Arg]) -> Res {
    let factor = arg_to_f64(arg(a, 0)) as i64;
    with_callback(pat, a, 1, |pat, cb| {
        ply_build(pat, factor, cb, ply_for_each_parts)
    })
}

/// A stable key for a ribbon window (`begin`, `duration`).
fn ribbon_key(begin: Frac, dur: Frac) -> String {
    format!(
        "{}/{}:{}/{}",
        begin.numer(),
        begin.denom(),
        dur.numer(),
        dur.denom()
    )
}

/// Core of `into`/`chunkInto`: where `pieces` is truthy, replace the source
/// with `f` applied to a looped subcycle (`ribbon`) covering that piece; where
/// falsy, play the source unchanged. The callback runs per distinct piece
/// window, so the transformed ribbons are probed and baked.
pub(super) fn into_build(pat: &Pattern, pieces: Pattern, cb: &Callback) -> Pattern {
    const PROBE: i64 = 16;
    let mut table: HashMap<String, Pattern> = HashMap::new();
    for cycle in 0..PROBE {
        for hap in pieces.query_arc(Frac::int(cycle), Frac::int(cycle + 1)) {
            if let (true, Some(w)) = (hap.value.truthy(), hap.whole) {
                table
                    .entry(ribbon_key(w.begin, w.duration()))
                    .or_insert_with(|| cb.apply(&pat.ribbon(w.begin, w.duration())));
            }
        }
    }
    let table = Arc::new(table);
    let base = pat.clone();
    pieces
        .with_hap(move |mut hap| {
            let chosen = match (hap.value.truthy(), hap.whole) {
                (true, Some(w)) => table
                    .get(&ribbon_key(w.begin, w.duration()))
                    .cloned()
                    .unwrap_or_else(|| base.clone()),
                _ => base.clone(),
            };
            hap.value = Value::Pat(Box::new(chosen));
            hap
        })
        .inner_join()
}

pub(super) fn chunk_pieces(n: i64) -> Pattern {
    let mut bins = vec![rudel_core::pure(Value::Bool(true))];
    for _ in 1..n {
        bins.push(rudel_core::pure(Value::Bool(false)));
    }
    rudel_core::fastcat(&bins)
}

/// `pat.into(pieces, f)`: break the pattern into looped subcycles per the truthy
/// parts of `pieces`, applying `f` to each.
pub(super) fn kpattern_into(pat: &Pattern, a: &[Arg]) -> Res {
    let pieces = pattern_arg(a, 0);
    with_callback(pat, a, 1, |pat, cb| into_build(pat, pieces, cb))
}

/// `pat.chunkInto(n, f)`: like `chunk`, but `f` is applied to a looped subcycle.
pub(super) fn kpattern_chunk_into(pat: &Pattern, a: &[Arg]) -> Res {
    let n = arg_to_f64(arg(a, 0)) as i64;
    let pieces = chunk_pieces(n).iter_back(n);
    with_callback(pat, a, 1, |pat, cb| into_build(pat, pieces, cb))
}

/// `pat.chunkBackInto(n, f)`: like `chunkInto`, but moves backwards.
pub(super) fn kpattern_chunk_back_into(pat: &Pattern, a: &[Arg]) -> Res {
    let n = arg_to_f64(arg(a, 0)) as i64;
    let pieces = chunk_pieces(n).iter(n)._early(Frac::one());
    with_callback(pat, a, 1, |pat, cb| into_build(pat, pieces, cb))
}

/// `pat.echoWith(times, time, f)` / `stutWith`: stack `times` copies, each
/// delayed by `time*i` and transformed by `f(copy, i)`.
pub(super) fn kpattern_echo_with(pat: &Pattern, a: &[Arg]) -> Res {
    let times = arg_to_f64(arg(a, 0)) as i64;
    let time = arg_to_frac(arg(a, 1));
    with_callback(pat, a, 2, |pat, cb| {
        pat.echo_with(times, time, |p, i| cb.apply2(p, i))
    })
}

/// `pat.applyN(n, f)`: apply the callback `f` to the pattern `n` times.
pub(super) fn kpattern_apply_n(pat: &Pattern, a: &[Arg]) -> Res {
    let n = arg_to_f64(arg(a, 0)) as i64;
    with_callback(pat, a, 1, |pat, cb| {
        let mut result = pat.clone();
        for _ in 0..n.max(0) {
            result = cb.apply(&result);
        }
        result
    })
}

/// `pat.every(n, f)` / `firstOf` (first cycle) and `lastOf` (last cycle), where
/// `n` may be a pattern (`every("<2 4>", f)`). The callback is applied to the
/// whole pattern once (eagerly), then placed by a patternified cycle count.
fn kpattern_every_impl(pat: &Pattern, a: &[Arg], last: bool) -> Res {
    let n = pattern_arg(a, 0);
    with_callback(pat, a, 1, |pat, cb| pat.every_pat(n, cb.apply(pat), last))
}

pub(super) fn kpattern_every(pat: &Pattern, a: &[Arg]) -> Res {
    kpattern_every_impl(pat, a, false)
}

pub(super) fn kpattern_last_of(pat: &Pattern, a: &[Arg]) -> Res {
    kpattern_every_impl(pat, a, true)
}

/// `pat.euclidish(pulses, steps, perc)` / `pat.eish(...)`: euclid morphed from
/// straight euclidean (`perc=0`) to even pulse (`perc=1`). `perc` may be a
/// continuous pattern.
pub(super) fn kpattern_euclidish(pat: &Pattern, a: &[Arg]) -> Res {
    let pulses = arg_to_f64(arg(a, 0)) as i64;
    let steps = arg_to_f64(arg(a, 1)) as i64;
    let perc = pattern_arg(a, 2);
    Ok(pat.euclidish(pulses, steps, perc).into())
}

/// `pat.hsl(h, s, l)`: write a CSS `hsl(...)` colour to the `color` control.
/// `h`/`s`/`l` may be patterns (hue in turns, saturation/lightness in `0..1`).
pub(super) fn kpattern_hsl(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(pat
        .hsl(pattern_arg(a, 0), pattern_arg(a, 1), pattern_arg(a, 2))
        .into())
}

/// `pat.hsla(h, s, l, a)`: like `hsl` with an extra alpha channel (`0..1`).
pub(super) fn kpattern_hsla(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(pat
        .hsla(
            pattern_arg(a, 0),
            pattern_arg(a, 1),
            pattern_arg(a, 2),
            pattern_arg(a, 3),
        )
        .into())
}

/// The `[pulses, steps, rotation]` array `bjork` takes (a lone number means
/// `steps = pulses`, `rotation = 0`).
pub(in crate::bindings) fn bjork_counts(value: &Arg) -> Vec<i64> {
    match value {
        Arg::List(l) => l.iter().map(|v| arg_to_f64(v) as i64).collect(),
        Arg::Null => Vec::new(),
        other => vec![arg_to_f64(other) as i64],
    }
}

/// `pat.bjork([pulses, steps, rotation])`: Tidal-style euclid taking an array.
pub(super) fn kpattern_bjork(pat: &Pattern, a: &[Arg]) -> Res {
    let mut counts = bjork_counts(arg(a, 0));
    if counts.is_empty() {
        counts.push(0);
    }
    Ok(pat.bjork(&counts).into())
}

/// `pat.choose(a, b, ...)` / `pat.choose2(...)`: use this pattern as the 0..1
/// (or, for `choose2`, -1..1) chooser to select continuously from the values.
/// Accepts a single array or bare varargs.
pub(super) fn kpattern_choose(chooser: &Pattern, a: &[Arg], bipolar: bool) -> Res {
    let chooser = if bipolar {
        chooser.from_bipolar()
    } else {
        chooser.clone()
    };
    let pats: Vec<Pattern> = collect_callables(a).iter().map(arg_to_pattern).collect();
    Ok(rudel_core::choose_with(chooser, &pats).into())
}

/// Apply every function in `funcs` to `pat`, stopping at the first error.
fn apply_each(pat: &Pattern, funcs: Vec<Arg>) -> Result<Vec<Pattern>, String> {
    let mut results = Vec::with_capacity(funcs.len());
    let mut first_err = None;
    for func in funcs {
        let cb = Callback::new(func);
        results.push(cb.apply(pat));
        if let Err(e) = cb.finish() {
            first_err.get_or_insert(e);
        }
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(results),
    }
}

/// `pat.layer(f, g, ...)`: stack the results of applying each function to the
/// pattern. Accepts an array of functions, or bare function arguments.
pub(super) fn kpattern_layer(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(rudel_core::stack(&apply_each(pat, collect_callables(a))?).into())
}

/// `pat.superimpose(...funcs)`: the pattern itself stacked with a copy through
/// each function — `stack(this, ...funcs.map(f => f(this)))`.
///
/// Variadic like `layer`, which is the same construct minus the original.
/// Taking only the first function silently dropped every copy after it, so
/// `superimpose(x => x.slow(2).add(12), x => x.slow(4).sub(5))` lost its whole
/// second voice.
pub(super) fn kpattern_superimpose(pat: &Pattern, a: &[Arg]) -> Res {
    let mut results = vec![pat.clone()];
    results.extend(apply_each(pat, collect_callables(a))?);
    Ok(rudel_core::stack(&results).into())
}

/// `pat.fmap(f)` / `pat.withValue(f)`: map every value through `f` when the
/// pattern is queried, as upstream does — one trip to the JS thread per query,
/// however many haps it holds. A value `f` throws on stays as it was, and the
/// error goes to the console, since a query has no caller to report it to.
pub(super) fn kpattern_fmap(pat: &Pattern, a: &[Arg]) -> Res {
    let Some(func) = crate::js::keep(arg(a, 0)) else {
        return Ok(pat.clone().into());
    };
    Ok(pat
        .with_haps(move |haps, _| {
            let values: Vec<Value> = haps.iter().map(|hap| hap.value.clone()).collect();
            let mapped = func.run(move |f| {
                values
                    .into_iter()
                    .map(
                        |v| match crate::js::call(f, vec![value_to_arg(v.clone())]) {
                            Ok(out) => to_value(&out),
                            Err(e) => {
                                rudel_core::log_line(format!("withValue: {e}"));
                                v
                            }
                        },
                    )
                    .collect::<Vec<_>>()
            });
            match mapped {
                Some(values) => haps
                    .into_iter()
                    .zip(values)
                    .map(|(mut hap, v)| {
                        hap.value = v;
                        hap
                    })
                    .collect(),
                None => haps,
            }
        })
        .into())
}

/// `pat.soundfont(name, n)`: play this pattern with preset `n` of a loaded
/// SoundFont. `loadSoundfont` returns the name to pass here, and the presets
/// are registered as ordinary sounds, so this is `.s(name).n(n)` — the shape
/// `sf.presets[n % sf.presets.length]` takes upstream.
pub(super) fn kpattern_soundfont(pat: &Pattern, a: &[Arg]) -> Res {
    let name = arg_to_raw_str(arg(a, 0)).unwrap_or_default();
    Ok(pat.s(Value::Str(name)).n(pattern_arg(a, 1)).into())
}

/// `pat.degradeByWith(withPat, x)`: drop events where `withPat` is not above
/// `x`, so an arbitrary signal (`rand.fast(2)`, `perlin`, …) drives the
/// degradation instead of the built-in `rand`. The engine method is a literal
/// port of Strudel's `degradeByWith`; only the binding was missing.
pub(super) fn kpattern_degrade_by_with(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(pat.degrade_by_with(pattern_arg(a, 0), f64_arg(a, 1)).into())
}

/// `pat.tag(name)`: mark every hap with an identifier, which a later `filter`
/// can select on (`hap.tags.includes(name)`).
pub(super) fn kpattern_tag(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(pat
        .tag(arg_to_raw_str(arg(a, 0)).unwrap_or_default())
        .into())
}

/// A hap as the object a `filter` predicate receives: Strudel's own shape
/// (`whole`, `part`, `value`), plus flattened `begin`/`end`/`wholeBegin`/
/// `wholeEnd` and the tags, so `hap.value.s === 'hh'` and `hap.hasTag(t)` both
/// read as they do upstream.
pub(crate) fn hap_to_filter_arg(hap: &rudel_core::Hap) -> Arg {
    let Arg::Map(mut map) = hap_to_arg(hap) else {
        unreachable!("a hap is an object")
    };
    let (whole_begin, whole_end) = match &hap.whole {
        Some(w) => (w.begin.to_f64(), w.end.to_f64()),
        // An analog hap has no whole; Strudel's `filterWhen` reads
        // `hap.whole.begin`, so fall back to the part it does have.
        None => (hap.part.begin.to_f64(), hap.part.end.to_f64()),
    };
    let tags: Vec<String> = hap.context.tags.iter().map(|t| t.to_string()).collect();
    map.extend([
        ("begin".to_string(), hap.part.begin.to_f64().into()),
        ("end".to_string(), hap.part.end.to_f64().into()),
        ("wholeBegin".to_string(), whole_begin.into()),
        ("wholeEnd".to_string(), whole_end.into()),
        (
            "tags".to_string(),
            Arg::List(tags.iter().map(|t| Arg::Str(t.clone())).collect()),
        ),
        // `hap.hasTag(name)` — Strudel's `Hap.hasTag`, closed over this hap's
        // tags so the predicate reads exactly as it does upstream.
        (
            "hasTag".to_string(),
            Arg::native(move |a| {
                let wanted = a.first().and_then(arg_to_raw_str);
                Ok(wanted.is_some_and(|w| tags.contains(&w)).into())
            }),
        ),
    ]);
    Arg::Map(map)
}

/// Probe-and-bake a per-hap predicate. The script does not run on the query
/// path, so the pattern is queried over `PROBE` cycles, the predicate applied
/// to each hap, and the survivors emitted as a static pattern that repeats
/// with that period — exactly what `fmap` does with its callback.
fn filter_build(pat: &Pattern, cb: &Callback, arg: impl Fn(&rudel_core::Hap) -> Arg) -> Pattern {
    const PROBE: i64 = 16;
    let haps = pat
        .query_arc(Frac::zero(), Frac::int(PROBE))
        .into_iter()
        .filter(|hap| cb.apply_predicate(arg(hap)))
        .collect();
    static_period_pattern(haps, pat.steps, Frac::int(PROBE))
}

/// `pat.setContext(ctx)`: replace every hap's context.
///
/// Upstream a hap's context carries whatever the passing transforms put there;
/// here it is the source locations the editor highlights from, and a script
/// calls this with `{}` to clear them — `.off(.25, x => x.…setContext({}))` so
/// the offset copy stops lighting up the line the original came from. Whatever
/// object is passed, the context that comes out is empty, because rudel's holds
/// nothing a script can name.
pub(super) fn kpattern_set_context(pat: &Pattern, _: &[Arg]) -> Res {
    Ok(pat
        .with_context(|_| rudel_core::hap::Context::default())
        .into())
}

/// `pat.filter(hap => ...)`: keep only the haps the predicate accepts.
pub(super) fn kpattern_filter(pat: &Pattern, a: &[Arg]) -> Res {
    // Handed something that is not a predicate (`.filter(500)`, meaning the
    // cutoff), upstream plays the pattern unfiltered rather than throwing.
    if !arg(a, 0).is_callable() {
        return Ok(pat.clone().into());
    }
    with_callback(pat, a, 0, |pat, cb| {
        filter_build(pat, cb, hap_to_filter_arg)
    })
}

/// `pat.apply(f)`: run a transform over the whole pattern.
///
/// `f` may be a *pattern of functions* as well as a function
/// (`apply(pick({a: x => x.fast(2), b: rev}))`), which is how a script switches
/// arrangement per section. That is the one case where which function to call
/// is not known until the pattern is queried, so the chosen function is called
/// from the query itself — see `convert::fn_to_value`.
pub(super) fn kpattern_apply(pat: &Pattern, a: &[Arg]) -> Res {
    with_callback(pat, a, 0, |pat, cb| cb.apply(pat))
}

/// `pat.filterValues(v => ...)`: like `filter`, but the predicate sees the
/// hap's value rather than the whole hap (core/pattern.mjs `filterValues`).
pub(super) fn kpattern_filter_values(pat: &Pattern, a: &[Arg]) -> Res {
    with_callback(pat, a, 0, |pat, cb| {
        filter_build(pat, cb, |hap| value_to_arg(hap.value.clone()))
    })
}

/// `pat.filterWhen(t => ...)`: keep only the haps whose onset the predicate
/// accepts. The argument is the whole's begin in cycles, as upstream.
pub(super) fn kpattern_filter_when(pat: &Pattern, a: &[Arg]) -> Res {
    with_callback(pat, a, 0, |pat, cb| {
        filter_build(pat, cb, |hap| {
            let t = hap.whole.as_ref().map_or(hap.part.begin, |w| w.begin);
            t.to_f64().into()
        })
    })
}

/// `pat.log()` / `pat.logValues()`, with or without a formatting callback.
///
/// Upstream both go through `Pattern.onTrigger`, running the callback as each
/// event fires. The script does not run in the realtime path, so the message
/// is decided up front — `mode` for the two built-in formats, or a string
/// probed-and-baked per hap when a callback is given — and carried on the hap as
/// the `_log` control. The scheduler's shared event extraction
/// (`rudel_core::query_controls`) consumes it and writes the line as the event
/// is played, which is the trigger time upstream logs at.
fn log_build(pat: &Pattern, a: &[Arg], mode: &str, per_value: bool) -> Res {
    const PROBE: i64 = 16;
    let func = arg(a, 0);
    if func.is_null() {
        // No callback: tag every hap with the built-in format to use.
        return Ok(pat
            .ctrl(
                rudel_core::LOG_KEY,
                rudel_core::pure(Value::Str(mode.into())),
            )
            .into());
    }
    with_callback(pat, a, 0, |pat, cb| {
        let haps = pat
            .query_arc(Frac::zero(), Frac::int(PROBE))
            .into_iter()
            .map(|hap| {
                // `logValues(f)` hands the callback the hap's value; `log(f)`
                // the whole hap, as upstream.
                let called = if per_value {
                    cb.apply_value(hap.value.clone())
                } else {
                    cb.apply_arg(hap_to_filter_arg(&hap))
                };
                let message = rudel_core::host::stringify_values(&called);
                let mut hap = hap;
                hap.value = merge_log_key(hap.value, message);
                hap
            })
            .collect();
        static_period_pattern(haps, pat.steps, Frac::int(PROBE))
    })
}

/// Add the `_log` control (carrying the already-formatted message) to a hap
/// value, coercing a bare value into a control map the way the controls do.
fn merge_log_key(value: Value, message: String) -> Value {
    let mut map = rudel_core::to_control_map(&value);
    map.insert(rudel_core::LOG_KEY.to_string(), Value::Str(message));
    Value::Map(map)
}

pub(super) use crate::triggers::kpattern_on_trigger_time;

pub(super) fn kpattern_log(pat: &Pattern, a: &[Arg]) -> Res {
    log_build(pat, a, "hap", false)
}

pub(super) fn kpattern_log_values(pat: &Pattern, a: &[Arg]) -> Res {
    log_build(pat, a, "values", true)
}

/// `pat.arpWith(chord => ...)`: arpeggiate chords, transforming each chord
/// (presented as a sequence of its notes) with a callback.
///
/// The callback does not run on the query path, so it is evaluated eagerly
/// here: probe the distinct chords over the first `PROBE` cycles, run the
/// callback on each, and bake the results into a lookup the query path
/// consults. Chords first appearing after the probe window fall back to
/// silence.
pub(super) fn arp_with_build(pat: &Pattern, cb: &Callback) -> Pattern {
    const PROBE: i64 = 16;
    let collected = pat.collect();
    let mut table: HashMap<String, Pattern> = HashMap::new();
    for cycle in 0..PROBE {
        for hap in collected.query_arc(Frac::int(cycle), Frac::int(cycle + 1)) {
            if let Value::List(notes) = &hap.value {
                table.entry(value_sig(&hap.value)).or_insert_with(|| {
                    let pats: Vec<Pattern> = notes.iter().cloned().map(rudel_core::pure).collect();
                    cb.apply(&rudel_core::fastcat(&pats))
                });
            }
        }
    }
    let table = Arc::new(table);
    collected.inner_bind(move |value| match &value {
        Value::List(_) => table
            .get(&value_sig(&value))
            .cloned()
            .unwrap_or_else(rudel_core::silence),
        _ => rudel_core::silence(),
    })
}

pub(super) fn kpattern_arp_with(pat: &Pattern, a: &[Arg]) -> Res {
    with_callback(pat, a, 0, arp_with_build)
}

/// `pat.whenKey(names, f)`: apply `f` while every named key is held. Unlike
/// `when`'s plain boolean pattern the condition reads the live keyboard at
/// query time, so it responds without re-evaluating the code.
pub(super) fn kpattern_when_key(pat: &Pattern, a: &[Arg]) -> Res {
    let keys = super::super::prelude::key_down_pattern(arg(a, 0));
    with_callback(pat, a, 1, |pat, cb| pat.when(keys, |p| cb.apply(p)))
}

/// `pat.keyDown()`: map this pattern's values (key names) to whether they are
/// currently held.
pub(super) fn kpattern_key_down(pat: &Pattern, _: &[Arg]) -> Res {
    Ok(super::super::prelude::key_down_pattern(&Arg::Pat(pat.clone())).into())
}

pub(super) fn kpattern_voicings(pat: &Pattern, a: &[Arg]) -> Res {
    let dict = arg_to_raw_str(arg(a, 0)).unwrap_or_else(|| "legacy".to_string());
    Ok(pat.voicings(dict).into())
}

/// `scale(name)`. The argument follows Strudel's quoting rule, which the
/// preprocessor has already applied: a single-quoted string never reached the
/// mini parser and is one literal scale name (`scale('A1 minor')`), while a
/// double-quoted one arrived as a pattern and stays one, so a tune can alternate
/// scales (`scale("<C:major C:mixolydian>")`). Reading the pattern back as raw
/// text would hand `parse_scale` the mini source and quietly return silence.
pub(super) fn kpattern_scale(pat: &Pattern, a: &[Arg]) -> Res {
    let name = match arg(a, 0) {
        Arg::Str(s) => rudel_core::pure(Value::Str(s.clone())),
        other => arg_to_pattern(other),
    };
    Ok(pat.scale(name).into())
}

/// A raw string argument as one value, or anything else as a pattern.
fn raw_or_pattern(value: &Arg) -> Pattern {
    match arg_to_raw_str(value) {
        Some(s) => rudel_core::pure(Value::Str(s)),
        None => arg_to_pattern(value),
    }
}

/// `markcss(css)`: the CSS the editor styles this event's source span with. A
/// declaration list (`'outline: solid 2px #ff0000'`) is one opaque string, not
/// a sequence of mini words, so the argument is taken raw.
pub(super) fn kpattern_markcss(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(pat.markcss(raw_or_pattern(arg(a, 0))).into())
}

/// The EDO definition (`C:LLsLLLs:2:1`) is a raw colon string, not mini.
pub(super) fn kpattern_edo_scale(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(pat.edo_scale(raw_or_pattern(arg(a, 0))).into())
}

pub(super) fn kpattern_i(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(if a.is_empty() {
        pat.wrap_control("i")
    } else {
        pat.i(pattern_arg(a, 0))
    }
    .into())
}

pub(super) fn kpattern_freq(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(if a.is_empty() {
        pat.wrap_control("freq")
    } else {
        pat.freq(pattern_arg(a, 0))
    }
    .into())
}

macro_rules! literal_or_pattern_methods {
    ($($fn_name:ident => $method:ident),* $(,)?) => {
        $(
            pub(super) fn $fn_name(pat: &Pattern, a: &[Arg]) -> Res {
                Ok(pat.$method(literal_or_pattern_arg(arg(a, 0))).into())
            }
        )*
    };
}

literal_or_pattern_methods! {
    kpattern_tune => tune,
    kpattern_xen => xen,
    kpattern_tuning => tuning,
    kpattern_with_base => with_base,
    kpattern_ftrans => ftrans,
}

pub(super) fn kpattern_partials(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(pat
        .ctrl("partials", rudel_core::pure(to_value(arg(a, 0))))
        .into())
}

pub(super) fn kpattern_phases(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(pat
        .ctrl("phases", rudel_core::pure(to_value(arg(a, 0))))
        .into())
}

pub(super) fn kpattern_ctrl(pat: &Pattern, a: &[Arg]) -> Res {
    let Some(name) = arg_to_raw_str(arg(a, 0)) else {
        return Err("ctrl: expected a control name string".to_string());
    };
    Ok(pat.ctrl(name, pattern_arg(a, 1)).into())
}

/// Shared body for the pick family: the instance is the selector pattern and
/// arg 0 is the lookup (array/object of patterns). The variants differ only
/// in index wrapping (`pickmod*`) and which join flattens the result.
pub(super) fn kpattern_pick_join(
    selector: &Pattern,
    a: &[Arg],
    modulo: bool,
    join: PickJoin,
) -> Res {
    let Some(lookup) = lookup_from_arg(arg(a, 0)) else {
        return Ok(rudel_core::silence().into());
    };
    Ok(pick_from_lookup(lookup, selector.clone(), modulo, join).into())
}

/// `pat.pickF(selector, [f, g, ...])` / `pat.pickF(selector, {a: f, ...})`:
/// use a pattern of indices/names to pick which function transforms the
/// pattern. Strudel composes `pat.apply(pick(lookup, selector))`, which
/// reduces to picking among the (eagerly) applied results with an inner join.
pub(super) fn kpattern_pick_f(pat: &Pattern, a: &[Arg], modulo: bool) -> Res {
    let (selector_value, funcs_value) = {
        let (x, y) = (arg(a, 0), arg(a, 1));
        if is_lookup(x) && !is_lookup(y) {
            (y, x)
        } else {
            (x, y)
        }
    };
    let selector = arg_to_pattern(selector_value);
    let apply = |func: &Arg| -> Result<Pattern, String> {
        let cb = Callback::new(func.clone());
        let applied = cb.apply(pat);
        cb.finish()?;
        Ok(applied)
    };
    let picked = match funcs_value {
        Arg::List(l) => {
            let items = l.iter().map(apply).collect::<Result<Vec<_>, _>>()?;
            rudel_core::pick_list(&items, &selector, modulo, PickJoin::Inner)
        }
        Arg::Map(m) => {
            let mut items = HashMap::new();
            for (key, v) in m {
                items.insert(key.clone(), apply(v)?);
            }
            rudel_core::pick_map(&items, &selector, PickJoin::Inner)
        }
        _ => return Err("pickF: expected an array or object of functions".to_string()),
    };
    Ok(picked.into())
}

/// `pat.as("note:clip")` / `pat.as(["note", "clip"])`: map bare positional
/// values into named controls (Strudel's `as`).
pub(super) fn kpattern_as_controls(pat: &Pattern, a: &[Arg]) -> Res {
    let value = arg(a, 0);
    let names: Vec<String> = if let Some(s) = arg_to_raw_str(value) {
        s.split(':').map(str::to_string).collect()
    } else {
        match value {
            Arg::List(items) => items.iter().filter_map(arg_to_raw_str).collect(),
            _ => return Err("as: expected a control-name string or array".to_string()),
        }
    };
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    Ok(pat.as_controls(&refs).into())
}

pub(super) fn kpattern_midi(pat: &Pattern, a: &[Arg]) -> Res {
    let mut p = pat.ctrl(IO_KEY, rudel_core::pure(Value::Str("midi".into())));
    if let Some(port) = arg_to_raw_str(arg(a, 0)) {
        p = p.ctrl("_midiport", rudel_core::pure(Value::Str(port)));
    }
    Ok(p.into())
}

pub(super) fn kpattern_osc(pat: &Pattern, a: &[Arg]) -> Res {
    let target = arg_to_raw_str(arg(a, 0));
    let mut p = pat.ctrl(IO_KEY, rudel_core::pure(Value::Str("osc".into())));
    if let Some((host, port)) = target.as_deref().and_then(|t| t.rsplit_once(':'))
        && let Ok(port) = port.parse::<i64>()
    {
        p = p.ctrl("oschost", rudel_core::pure(Value::Str(host.to_string())));
        p = p.ctrl("oscport", rudel_core::pure(Value::Int(port)));
    }
    Ok(p.into())
}

/// `chord(name)`. Strudel registers `chord` as a plain control and nothing else
/// (`controls.mjs`: `registerControl('chord')`), so a bare `.chord()` promotes
/// the pattern's own values into it — `{chord: "B^7"}` — which is what
/// `.dict(...)` and `.voicing()` read. Expanding the names straight to note
/// stacks here instead (rudel-core's `Pattern::chord`, still reachable in Rust)
/// left `.voicing()` nothing to voice, and a tune that spells its chords
/// `seq(...).chord().dict('lefthand').voicing()` lost that whole layer.
pub(super) fn kpattern_chord(pat: &Pattern, a: &[Arg]) -> Res {
    Ok(if a.is_empty() {
        rudel_core::control_dyn("chord", pat.clone())
    } else {
        pat.set(rudel_core::control_dyn("chord", pattern_arg(a, 0)))
    }
    .into())
}

/// Inline visual widget methods (`._pianoroll(...)`, `._spiral(...)`, ...).
/// Strudel's CodeMirror host tags the source pattern with the generated widget
/// id before registering a canvas. Rudel keeps the same branch identity in hap
/// context so the native editor can draw only the events for that widget.
pub(super) fn kpattern_visual_widget(pat: &Pattern, a: &[Arg]) -> Res {
    let Some(id) = arg_to_raw_str(arg(a, 0)) else {
        return Ok(pat.clone().into());
    };
    // The transpiler passes the call's original option object through as the
    // second argument, so by now the script has evaluated it — a computed
    // `{cycles: n}` is a real number here, where the source scan could only
    // read literals. Record it for the host to merge over the scanned options.
    crate::widgets::record_options(&id, crate::widgets::options_from_arg(arg(a, 1)));
    Ok(pat.tag(id).into())
}
