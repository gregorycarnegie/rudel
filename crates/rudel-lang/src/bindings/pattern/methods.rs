use super::{
    KPattern,
    args::{
        method_arg, method_f64_arg, method_pattern_arg, with_instance, with_literal_or_pattern_arg,
        with_pattern_arg,
    },
    callback::{Callback, static_period_pattern},
    convert::{arg_to_f64, arg_to_frac, arg_to_pattern, arg_to_raw_str, koto_to_value},
    pick::{is_lookup, lookup_from_koto, pick_from_lookup},
};
use crate::bindings::routing::IO_KEY;
use koto::{prelude::*, runtime::Result as KotoResult};
use rudel_core::{Frac, Pattern, PickJoin, Value};
use std::{collections::HashMap, sync::Arc};

/// A stable string key for a chord value, used to memoise `arp_with` callback
/// results so the (non-`Send`) Koto VM is only touched at construction time.
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

/// Collect vararg-style arguments (`layer`, `tour`): a single list/tuple is
/// expanded into its elements, otherwise the varargs are used as-is.
fn collect_callables(args: &[KValue]) -> Vec<KValue> {
    match args {
        [KValue::List(l)] => l.data().iter().cloned().collect(),
        [KValue::Tuple(t)] => t.data().to_vec(),
        _ => args.to_vec(),
    }
}

macro_rules! literal_or_pattern_methods {
    ($($fn_name:ident => $method:ident),* $(,)?) => {
        $(
            pub(super) fn $fn_name(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
                with_literal_or_pattern_arg(&ctx, |pat, arg| pat.$method(arg))
            }
        )*
    };
}

macro_rules! pattern_arg_methods {
    ($($fn_name:ident => $method:ident),* $(,)?) => {
        $(
            pub(super) fn $fn_name(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
                with_pattern_arg(&ctx, |pat, arg| pat.$method(arg))
            }
        )*
    };
}

/// `pat.tour(a, b, ...)`: insert the pattern into the list of patterns
/// stepwise, moving backwards one slot per repetition (also accepts a single
/// list/tuple of patterns).
pub(super) fn kpattern_tour(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let many: Vec<Pattern> = collect_callables(ctx.args)
        .iter()
        .map(arg_to_pattern)
        .collect();
    Ok(KPattern::wrap(pat.tour(&many)))
}

/// `pat.loopAtCps(factor, cps)`: like `loopAt` but with an explicit cps
/// (deprecated in Strudel; kept for parity).
pub(super) fn kpattern_loop_at_cps(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let factor = arg_to_frac(&method_arg(&ctx, 0));
    let cps = arg_to_f64(&method_arg(&ctx, 1));
    Ok(KPattern::wrap(pat.loop_at_cps(factor, cps)))
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
/// value's span. The Koto VM can't run in the query path, so the per-value
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

/// `pat.plyWith(factor, f)`: repeat each event `factor` times, applying `f`
/// cumulatively (`f` 0×, 1×, 2×, … like `applyN`).
pub(super) fn kpattern_ply_with(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let factor = arg_to_f64(&method_arg(&ctx, 0)) as i64;
    let cb = Callback::new(&ctx, method_arg(&ctx, 1));
    let out = ply_build(&pat, factor, &cb, ply_with_parts);
    cb.finish()?;
    Ok(KPattern::wrap(out))
}

/// `pat.plyForEach(factor, f)`: repeat each event `factor` times, applying
/// `f(copy, i)` to each repeat (the first is left untransformed).
pub(super) fn kpattern_ply_for_each(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let factor = arg_to_f64(&method_arg(&ctx, 0)) as i64;
    let cb = Callback::new(&ctx, method_arg(&ctx, 1));
    let out = ply_build(&pat, factor, &cb, ply_for_each_parts);
    cb.finish()?;
    Ok(KPattern::wrap(out))
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
pub(super) fn kpattern_into(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let pieces = method_pattern_arg(&ctx, 0);
    let cb = Callback::new(&ctx, method_arg(&ctx, 1));
    let out = into_build(&pat, pieces, &cb);
    cb.finish()?;
    Ok(KPattern::wrap(out))
}

/// `pat.chunkInto(n, f)`: like `chunk`, but `f` is applied to a looped subcycle.
pub(super) fn kpattern_chunk_into(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let n = arg_to_f64(&method_arg(&ctx, 0)) as i64;
    let cb = Callback::new(&ctx, method_arg(&ctx, 1));
    let pieces = chunk_pieces(n).iter_back(n);
    let out = into_build(&pat, pieces, &cb);
    cb.finish()?;
    Ok(KPattern::wrap(out))
}

/// `pat.chunkBackInto(n, f)`: like `chunkInto`, but moves backwards.
pub(super) fn kpattern_chunk_back_into(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let n = arg_to_f64(&method_arg(&ctx, 0)) as i64;
    let cb = Callback::new(&ctx, method_arg(&ctx, 1));
    let pieces = chunk_pieces(n).iter(n)._early(Frac::one());
    let out = into_build(&pat, pieces, &cb);
    cb.finish()?;
    Ok(KPattern::wrap(out))
}

/// `pat.echoWith(times, time, f)` / `stutWith`: stack `times` copies, each
/// delayed by `time*i` and transformed by `f(copy, i)`.
pub(super) fn kpattern_echo_with(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let times = arg_to_f64(&method_arg(&ctx, 0)) as i64;
    let time = arg_to_frac(&method_arg(&ctx, 1));
    let cb = Callback::new(&ctx, method_arg(&ctx, 2));
    let out = pat.echo_with(times, time, |p, i| cb.apply2(p, i));
    cb.finish()?;
    Ok(KPattern::wrap(out))
}

/// `pat.applyN(n, f)`: apply the callback `f` to the pattern `n` times.
pub(super) fn kpattern_apply_n(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let n = arg_to_f64(&method_arg(&ctx, 0)) as i64;
    let cb = Callback::new(&ctx, method_arg(&ctx, 1));
    let mut result = pat;
    for _ in 0..n.max(0) {
        result = cb.apply(&result);
    }
    cb.finish()?;
    Ok(KPattern::wrap(result))
}

/// `pat.every(n, f)` / `firstOf` (first cycle) and `lastOf` (last cycle), where
/// `n` may be a pattern (`every("<2 4>", f)`). The callback is applied to the
/// whole pattern once (eagerly), then placed by a patternified cycle count.
fn kpattern_every_impl(ctx: MethodContext<KPattern>, last: bool) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let n = method_pattern_arg(&ctx, 0);
    let cb = Callback::new(&ctx, method_arg(&ctx, 1));
    let transformed = cb.apply(&pat);
    cb.finish()?;
    Ok(KPattern::wrap(pat.every_pat(n, transformed, last)))
}

pub(super) fn kpattern_every(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    kpattern_every_impl(ctx, false)
}

pub(super) fn kpattern_last_of(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    kpattern_every_impl(ctx, true)
}

/// `pat.euclidish(pulses, steps, perc)` / `pat.eish(...)`: euclid morphed from
/// straight euclidean (`perc=0`) to even pulse (`perc=1`). `perc` may be a
/// continuous pattern.
pub(super) fn kpattern_euclidish(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let pulses = arg_to_f64(&method_arg(&ctx, 0)) as i64;
    let steps = arg_to_f64(&method_arg(&ctx, 1)) as i64;
    let perc = arg_to_pattern(&method_arg(&ctx, 2));
    Ok(KPattern::wrap(pat.euclidish(pulses, steps, perc)))
}

/// `pat.hsl(h, s, l)`: write a CSS `hsl(...)` colour to the `color` control.
/// `h`/`s`/`l` may be patterns (hue in turns, saturation/lightness in `0..1`).
pub(super) fn kpattern_hsl(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let h = method_pattern_arg(&ctx, 0);
    let s = method_pattern_arg(&ctx, 1);
    let l = method_pattern_arg(&ctx, 2);
    Ok(KPattern::wrap(pat.hsl(h, s, l)))
}

/// `pat.hsla(h, s, l, a)`: like `hsl` with an extra alpha channel (`0..1`).
pub(super) fn kpattern_hsla(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let h = method_pattern_arg(&ctx, 0);
    let s = method_pattern_arg(&ctx, 1);
    let l = method_pattern_arg(&ctx, 2);
    let a = method_pattern_arg(&ctx, 3);
    Ok(KPattern::wrap(pat.hsla(h, s, l, a)))
}

/// `pat.bjork([pulses, steps, rotation])`: Tidal-style euclid taking a tuple
/// (a lone number means `steps = pulses`, `rotation = 0`).
pub(super) fn kpattern_bjork(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let euc: Vec<i64> = match &method_arg(&ctx, 0) {
        KValue::List(l) => l.data().iter().map(|v| arg_to_f64(v) as i64).collect(),
        KValue::Tuple(t) => t.data().iter().map(|v| arg_to_f64(v) as i64).collect(),
        other => vec![arg_to_f64(other) as i64],
    };
    Ok(KPattern::wrap(pat.bjork(&euc)))
}

/// `pat.choose(a, b, ...)` / `pat.choose2(...)`: use this pattern as the 0..1
/// (or, for `choose2`, -1..1) chooser to select continuously from the values.
/// Accepts a single list/tuple or bare varargs.
pub(super) fn kpattern_choose(ctx: MethodContext<KPattern>, bipolar: bool) -> KotoResult<KValue> {
    let chooser = ctx.instance()?.0.clone();
    let chooser = if bipolar {
        chooser.from_bipolar()
    } else {
        chooser
    };
    let pats: Vec<Pattern> = collect_callables(ctx.args)
        .iter()
        .map(arg_to_pattern)
        .collect();
    Ok(KPattern::wrap(rudel_core::choose_with(chooser, &pats)))
}

/// `pat.layer([f, g, ...])`: stack the results of applying each function in
/// the list to the pattern. Accepts a list/tuple of callables, or bare callable
/// args.
pub(super) fn kpattern_layer(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let funcs = collect_callables(ctx.args);
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

/// `pat.fmap(f)`: Strudel's value-level mapper. The Koto VM isn't Send+Sync,
/// so map one probe window eagerly and repeat that shape.
pub(super) fn kpattern_fmap(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    const PROBE: i64 = 16;
    let pat = ctx.instance()?.0.clone();
    let cb = Callback::new(&ctx, method_arg(&ctx, 0));
    let haps = pat
        .query_arc(Frac::zero(), Frac::int(PROBE))
        .into_iter()
        .map(|hap| hap.with_value(|v| cb.apply_value(v)))
        .collect();
    cb.finish()?;
    Ok(KPattern::wrap(static_period_pattern(
        haps,
        pat.steps,
        Frac::int(PROBE),
    )))
}

/// `pat.soundfont(name, n)`: play this pattern with preset `n` of a loaded
/// SoundFont. `loadSoundfont` returns the name to pass here, and the presets
/// are registered as ordinary sounds, so this is `.s(name).n(n)` — the shape
/// `sf.presets[n % sf.presets.length]` takes upstream.
pub(super) fn kpattern_soundfont(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let name = arg_to_raw_str(&method_arg(&ctx, 0)).unwrap_or_default();
    let n = method_pattern_arg(&ctx, 1);
    with_instance(&ctx, |pat| {
        pat.s(rudel_core::Value::Str(name.clone())).n(n.clone())
    })
}

/// `pat.degradeByWith(withPat, x)`: drop events where `withPat` is not above
/// `x`, so an arbitrary signal (`rand.fast(2)`, `perlin`, …) drives the
/// degradation instead of the built-in `rand`. The engine method is a literal
/// port of Strudel's `degradeByWith`; only the binding was missing.
pub(super) fn kpattern_degrade_by_with(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let with_pat = method_pattern_arg(&ctx, 0);
    let x = method_f64_arg(&ctx, 1);
    with_instance(&ctx, |pat| pat.degrade_by_with(with_pat.clone(), x))
}

/// `pat.tag(name)`: mark every hap with an identifier, which a later `filter`
/// can select on (`hap.tags.contains(name)`).
pub(super) fn kpattern_tag(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let tag = arg_to_raw_str(&method_arg(&ctx, 0)).unwrap_or_default();
    with_instance(&ctx, |pat| pat.tag(tag.clone()))
}

/// Marshal a hap into the Koto map a `filter` predicate receives:
/// `{value, begin, end, wholeBegin, wholeEnd, tags, hasTag}`. Strudel hands over
/// a `Hap` object; this is the same information in Koto's shape, so
/// `hap.value.s == "hh"` and `hap.hasTag(t)` both read as they do upstream.
pub(crate) fn hap_to_koto(hap: &rudel_core::Hap) -> KValue {
    let map = koto::runtime::KMap::new();
    map.insert("value", super::convert::value_to_koto(hap.value.clone()));
    map.insert("begin", hap.part.begin.to_f64());
    map.insert("end", hap.part.end.to_f64());
    let (whole_begin, whole_end) = match &hap.whole {
        Some(w) => (w.begin.to_f64(), w.end.to_f64()),
        // An analog hap has no whole; Strudel's `filterWhen` reads
        // `hap.whole.begin`, so fall back to the part it does have.
        None => (hap.part.begin.to_f64(), hap.part.end.to_f64()),
    };
    map.insert("wholeBegin", whole_begin);
    map.insert("wholeEnd", whole_end);
    map.insert(
        "tags",
        KValue::List(koto::runtime::KList::from_slice(
            &hap.context
                .tags
                .iter()
                .map(|t| KValue::Str(t.as_str().into()))
                .collect::<Vec<_>>(),
        )),
    );
    // `hap.hasTag(name)` — Strudel's `Hap.hasTag`, closed over this hap's tags
    // so the predicate reads exactly as it does upstream.
    let tags: Vec<String> = hap.context.tags.iter().map(|t| t.to_string()).collect();
    map.insert(
        "hasTag",
        KValue::NativeFunction(koto::runtime::KNativeFunction::new(move |ctx| {
            let wanted = ctx.args().first().and_then(arg_to_raw_str);
            Ok(KValue::Bool(wanted.is_some_and(|w| tags.contains(&w))))
        })),
    );
    KValue::Map(map)
}

/// Probe-and-bake a per-hap predicate. The Koto VM is not `Send`, so it cannot
/// run in the query path; the pattern is queried over `PROBE` cycles, the
/// predicate applied to each hap, and the survivors emitted as a static pattern
/// that repeats with that period — exactly what `fmap` does with its callback.
fn filter_build(pat: &Pattern, cb: &Callback, arg: impl Fn(&rudel_core::Hap) -> KValue) -> Pattern {
    const PROBE: i64 = 16;
    let haps = pat
        .query_arc(Frac::zero(), Frac::int(PROBE))
        .into_iter()
        .filter(|hap| cb.apply_predicate(arg(hap)))
        .collect();
    static_period_pattern(haps, pat.steps, Frac::int(PROBE))
}

/// `pat.filter(|hap| ...)`: keep only the haps the predicate accepts.
pub(super) fn kpattern_filter(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let cb = Callback::new(&ctx, method_arg(&ctx, 0));
    let out = filter_build(&pat, &cb, hap_to_koto);
    cb.finish()?;
    Ok(KPattern::wrap(out))
}

/// `pat.filterWhen(|t| ...)`: keep only the haps whose onset the predicate
/// accepts. The argument is the whole's begin in cycles, as upstream.
pub(super) fn kpattern_filter_when(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let cb = Callback::new(&ctx, method_arg(&ctx, 0));
    let out = filter_build(&pat, &cb, |hap| {
        let t = hap.whole.as_ref().map_or(hap.part.begin, |w| w.begin);
        KValue::Number(t.to_f64().into())
    });
    cb.finish()?;
    Ok(KPattern::wrap(out))
}

/// `pat.log()` / `pat.logValues()`, with or without a formatting callback.
///
/// Upstream both go through `Pattern.onTrigger`, running the callback as each
/// event fires. Rudel's Koto VM can't run in the realtime path, so the message
/// is decided up front — `mode` for the two built-in formats, or a string
/// probed-and-baked per hap when a callback is given — and carried on the hap as
/// the `_log` control. The scheduler's shared event extraction
/// (`rudel_core::query_controls`) consumes it and writes the line as the event
/// is played, which is the trigger time upstream logs at.
fn log_build(ctx: &MethodContext<KPattern>, mode: &str, per_value: bool) -> KotoResult<KValue> {
    const PROBE: i64 = 16;
    let pat = ctx.instance()?.0.clone();
    let arg = method_arg(ctx, 0);
    if matches!(arg, KValue::Null) {
        // No callback: tag every hap with the built-in format to use.
        return Ok(KPattern::wrap(pat.ctrl(
            rudel_core::LOG_KEY,
            rudel_core::pure(Value::Str(mode.into())),
        )));
    }
    let cb = Callback::new(ctx, arg);
    let haps = pat
        .query_arc(Frac::zero(), Frac::int(PROBE))
        .into_iter()
        .map(|hap| {
            // `logValues(f)` hands the callback the hap's value; `log(f)` the
            // whole hap, as upstream.
            let called = if per_value {
                cb.apply_value(hap.value.clone())
            } else {
                cb.apply_koto(hap_to_koto(&hap))
            };
            let message = rudel_core::host::stringify_values(&called);
            let mut hap = hap;
            hap.value = merge_log_key(hap.value, message);
            hap
        })
        .collect();
    let out = static_period_pattern(haps, pat.steps, Frac::int(PROBE));
    cb.finish()?;
    Ok(KPattern::wrap(out))
}

/// Add the `_log` control (carrying the already-formatted message) to a hap
/// value, coercing a bare value into a control map the way the controls do.
fn merge_log_key(value: Value, message: String) -> Value {
    let mut map = rudel_core::to_control_map(&value);
    map.insert(rudel_core::LOG_KEY.to_string(), Value::Str(message));
    Value::Map(map)
}

pub(super) use crate::triggers::kpattern_on_trigger_time;

pub(super) fn kpattern_log(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    log_build(&ctx, "hap", false)
}

pub(super) fn kpattern_log_values(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    log_build(&ctx, "values", true)
}

/// `pat.arp_with(|chord| ...)`: arpeggiate chords, transforming each chord
/// (presented as a sequence of its notes) with a callback.
///
/// The callback can't run in the (Send+Sync) query path because the Koto VM
/// isn't Send, so we evaluate it eagerly here: probe the distinct chords over
/// the first `PROBE` cycles, run the callback on each, and bake the results
/// into a lookup the query path consults. Chords first appearing after the
/// probe window fall back to silence.
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

pub(super) fn kpattern_arp_with(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let cb = Callback::new(&ctx, method_arg(&ctx, 0));
    let out = arp_with_build(&pat, &cb);
    cb.finish()?;
    Ok(KPattern::wrap(out))
}

/// `pat.whenKey(names, f)`: apply `f` while every named key is held. Unlike
/// `when`'s plain boolean pattern the condition reads the live keyboard at
/// query time, so it responds without re-evaluating the code.
pub(super) fn kpattern_when_key(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let keys = super::super::prelude::key_down_pattern(&method_arg(&ctx, 0));
    super::callback::with_callback(&ctx, 1, |pat, cb| pat.when(keys.clone(), |p| cb.apply(p)))
}

/// `pat.keyDown()`: map this pattern's values (key names) to whether they are
/// currently held.
pub(super) fn kpattern_key_down(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_instance(&ctx, |pat| {
        super::super::prelude::key_down_pattern(&KPattern(pat.clone()).into())
    })
}

pub(super) fn kpattern_voicings(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let dict = arg_to_raw_str(&method_arg(&ctx, 0)).unwrap_or_else(|| "legacy".to_string());
    with_instance(&ctx, |pat| pat.voicings(dict.clone()))
}

pub(super) fn kpattern_scale(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let arg = method_arg(&ctx, 0);
    let name = match arg_to_raw_str(&arg) {
        Some(s) => rudel_core::pure(Value::Str(s)),
        None => arg_to_pattern(&arg),
    };
    with_instance(&ctx, |pat| pat.scale(name))
}

/// `markcss(css)`: the CSS the editor styles this event's source span with. A
/// declaration list (`'outline: solid 2px #ff0000'`) is one opaque string, not
/// a sequence of mini words, so the argument is taken raw.
pub(super) fn kpattern_markcss(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let arg = method_arg(&ctx, 0);
    let css = match arg_to_raw_str(&arg) {
        Some(s) => rudel_core::pure(Value::Str(s)),
        None => arg_to_pattern(&arg),
    };
    with_instance(&ctx, |pat| pat.markcss(css))
}

pub(super) fn kpattern_edo_scale(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    // The EDO definition (`C:LLsLLLs:2:1`) is a raw colon string, not mini.
    let arg = method_arg(&ctx, 0);
    let def = match arg_to_raw_str(&arg) {
        Some(s) => rudel_core::pure(Value::Str(s)),
        None => arg_to_pattern(&arg),
    };
    with_instance(&ctx, |pat| pat.edo_scale(def))
}

pub(super) fn kpattern_i(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    if ctx.args.is_empty() {
        with_instance(&ctx, |pat| pat.wrap_control("i"))
    } else {
        with_pattern_arg(&ctx, |pat, arg| pat.i(arg))
    }
}

pub(super) fn kpattern_freq(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    if ctx.args.is_empty() {
        with_instance(&ctx, |pat| pat.wrap_control("freq"))
    } else {
        with_pattern_arg(&ctx, |pat, arg| pat.freq(arg))
    }
}

literal_or_pattern_methods! {
    kpattern_tune => tune,
    kpattern_xen => xen,
    kpattern_tuning => tuning,
    kpattern_with_base => with_base,
    kpattern_ftrans => ftrans,
    kpattern_ftranspose => ftrans,
}

pub(super) fn kpattern_partials(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let v = koto_to_value(&method_arg(&ctx, 0));
    with_instance(&ctx, |pat| {
        pat.ctrl("partials", rudel_core::pure(v.clone()))
    })
}

pub(super) fn kpattern_phases(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let v = koto_to_value(&method_arg(&ctx, 0));
    with_instance(&ctx, |pat| pat.ctrl("phases", rudel_core::pure(v.clone())))
}

pub(super) fn kpattern_ctrl(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let name = match arg_to_raw_str(&method_arg(&ctx, 0)) {
        Some(s) => s,
        None => {
            let other = method_arg(&ctx, 0);
            return runtime_error!("ctrl: expected a control name string, got {other:?}");
        }
    };
    let value = method_pattern_arg(&ctx, 1);
    with_instance(&ctx, |pat| pat.ctrl(name.clone(), value.clone()))
}

pattern_arg_methods! {
    kpattern_sound => s,
    kpattern_struct_alias => struct_pat,
}

/// Shared body for the pick family: the instance is the selector pattern and
/// arg 0 is the lookup (list/tuple/map of patterns). The variants differ only
/// in index wrapping (`pickmod*`) and which join flattens the result.
pub(super) fn kpattern_pick_join(
    ctx: MethodContext<KPattern>,
    modulo: bool,
    join: PickJoin,
) -> KotoResult<KValue> {
    let selector = ctx.instance()?.0.clone();
    let Some(lookup) = lookup_from_koto(&method_arg(&ctx, 0)) else {
        return Ok(KPattern::wrap(rudel_core::silence()));
    };
    Ok(KPattern::wrap(pick_from_lookup(
        lookup, selector, modulo, join,
    )))
}

/// `pat.pickF(selector, [f, g, ...])` / `pat.pickF(selector, {a: f, ...})`:
/// use a pattern of indices/names to pick which function transforms the
/// pattern. Strudel composes `pat.apply(pick(lookup, selector))`, which
/// reduces to picking among the (eagerly) applied results with an inner join
/// — eager application is required here because the Koto VM can't be driven
/// from the query path.
pub(super) fn kpattern_pick_f(ctx: MethodContext<KPattern>, modulo: bool) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let (selector_value, funcs_value) = {
        let a = method_arg(&ctx, 0);
        let b = method_arg(&ctx, 1);
        if is_lookup(&a) && !is_lookup(&b) {
            (b, a)
        } else {
            (a, b)
        }
    };
    let selector = arg_to_pattern(&selector_value);
    let apply = |func: &KValue| -> KotoResult<Pattern> {
        let cb = Callback::new(&ctx, func.clone());
        let applied = cb.apply(&pat);
        cb.finish()?;
        Ok(applied)
    };
    let picked = match &funcs_value {
        KValue::List(l) => {
            let items = l.data().iter().map(apply).collect::<KotoResult<Vec<_>>>()?;
            rudel_core::pick_list(&items, &selector, modulo, PickJoin::Inner)
        }
        KValue::Tuple(t) => {
            let items = t.iter().map(apply).collect::<KotoResult<Vec<_>>>()?;
            rudel_core::pick_list(&items, &selector, modulo, PickJoin::Inner)
        }
        KValue::Map(m) => {
            let mut items = HashMap::new();
            for (k, v) in m.data().iter() {
                if let KValue::Str(key) = k.value() {
                    items.insert(key.to_string(), apply(v)?);
                }
            }
            rudel_core::pick_map(&items, &selector, PickJoin::Inner)
        }
        other => {
            return runtime_error!("pickF: expected a list or map of functions, got {other:?}");
        }
    };
    Ok(KPattern::wrap(picked))
}

/// `pat.as("note:clip")` / `pat.as(["note", "clip"])`: map bare positional
/// values into named controls (Strudel's `as`).
pub(super) fn kpattern_as_controls(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let arg = method_arg(&ctx, 0);
    let names: Vec<String> = if let Some(s) = arg_to_raw_str(&arg) {
        s.split(':').map(str::to_string).collect()
    } else {
        match &arg {
            KValue::List(items) => items.data().iter().filter_map(arg_to_raw_str).collect(),
            KValue::Tuple(items) => items.iter().filter_map(arg_to_raw_str).collect(),
            other => {
                return runtime_error!("as: expected a control-name string or list, got {other:?}");
            }
        }
    };
    with_instance(&ctx, |pat| {
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        pat.as_controls(&refs)
    })
}

pattern_arg_methods! {
    kpattern_loop_play => loop_play,
    kpattern_loop_begin => loop_begin,
    kpattern_loop_end => loop_end,
}

pub(super) fn kpattern_p(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let name = match method_arg(&ctx, 0) {
        KValue::Number(n) => n.to_string(),
        other => arg_to_raw_str(&other).unwrap_or_default(),
    };
    with_instance(&ctx, |pat| {
        pat.ctrl("id", rudel_core::pure(Value::Str(name.clone())))
    })
}

pub(super) fn kpattern_midi(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let port = arg_to_raw_str(&method_arg(&ctx, 0));
    with_instance(&ctx, |pat| {
        let mut p = pat.ctrl(IO_KEY, rudel_core::pure(Value::Str("midi".into())));
        if let Some(port) = &port {
            p = p.ctrl("_midiport", rudel_core::pure(Value::Str(port.clone())));
        }
        p
    })
}

pub(super) fn kpattern_osc(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let target = arg_to_raw_str(&method_arg(&ctx, 0));
    with_instance(&ctx, |pat| {
        let mut p = pat.ctrl(IO_KEY, rudel_core::pure(Value::Str("osc".into())));
        if let Some((host, port)) = target.as_deref().and_then(|t| t.rsplit_once(':'))
            && let Ok(port) = port.parse::<i64>()
        {
            p = p.ctrl("oschost", rudel_core::pure(Value::Str(host.to_string())));
            p = p.ctrl("oscport", rudel_core::pure(Value::Int(port)));
        }
        p
    })
}

pub(super) fn kpattern_chord(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    if ctx.args.is_empty() {
        with_instance(&ctx, |pat| pat.chord())
    } else {
        with_pattern_arg(&ctx, |pat, arg| {
            pat.set(rudel_core::control_dyn("chord", arg))
        })
    }
}

/// Inline visual widget methods (`._pianoroll(...)`, `._spiral(...)`, ...).
/// Strudel's CodeMirror host tags the source pattern with the generated widget
/// id before registering a canvas. Rudel keeps the same branch identity in hap
/// context so the native editor can draw only the events for that widget.
pub(super) fn kpattern_visual_widget(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let Some(id) = arg_to_raw_str(&method_arg(&ctx, 0)) else {
        return Ok(KPattern::wrap(pat));
    };
    Ok(KPattern::wrap(pat.tag(id)))
}
