use super::{
    args::arg,
    convert::{arg_to_f64, arg_to_pattern, to_value, value_to_arg},
    methods::value_sig,
};
use crate::js::{self, Arg, NULL, Res, Scope};
use rudel_core::{Frac, Pattern, Value};
use std::{cell::RefCell, collections::HashMap, sync::Arc};

/// Patternify a callback combinator's leading argument when it is a pattern
/// rather than a scalar (`chunk("<2 4>", f)`, `inside("<2 3>", f)`). The script
/// is not run on the query path, so the combinator result is built eagerly
/// for each distinct argument value seen over a probe window, then selected per
/// cycle with `innerJoin` — matching Strudel's `register` patternification
/// (`arg.fmap(v => combinator(v, f, pat)).innerJoin()`). Values first appearing
/// after the probe window fall back to silence (same limit as `fmap`/`arpWith`).
pub(super) fn probe_patternify<F>(arg: Pattern, build: F) -> Pattern
where
    F: Fn(&Value) -> Pattern,
{
    const PROBE: i64 = 16;
    let mut table: HashMap<String, Pattern> = HashMap::new();
    for cycle in 0..PROBE {
        for hap in arg.query_arc(Frac::int(cycle), Frac::int(cycle + 1)) {
            table
                .entry(value_sig(&hap.value))
                .or_insert_with(|| build(&hap.value));
        }
    }
    let table = Arc::new(table);
    arg.fmap(move |v| {
        let pat = table
            .get(&value_sig(&v))
            .cloned()
            .unwrap_or_else(rudel_core::silence);
        Value::Pat(Box::new(pat))
    })
    .inner_join()
}

/// `combinator(n, f)` where the leading numeric `n` may be a scalar (fast path)
/// or a pattern (probed). `conv` maps a value to the scalar type the core
/// combinator expects; `build` applies the combinator.
fn with_cb_scalar<T, C, F>(pat: &Pattern, n: &Arg, func: &Arg, conv: C, build: F) -> Res
where
    C: Fn(&Value) -> T,
    F: Fn(&Pattern, T, &Callback) -> Pattern,
{
    let cb = Callback::new(func.clone());
    let result = if let Arg::Num(_) = n {
        build(pat, conv(&to_value(n)), &cb)
    } else {
        probe_patternify(arg_to_pattern(n), |v| build(pat, conv(v), &cb))
    };
    cb.finish()?;
    Ok(result.into())
}

pub(super) fn with_cb_i64<F>(pat: &Pattern, n: &Arg, func: &Arg, build: F) -> Res
where
    F: Fn(&Pattern, i64, &Callback) -> Pattern,
{
    with_cb_scalar(pat, n, func, |v| v.as_f64().unwrap_or(0.0) as i64, build)
}

pub(super) fn with_cb_frac<F>(pat: &Pattern, n: &Arg, func: &Arg, build: F) -> Res
where
    F: Fn(&Pattern, Frac, &Callback) -> Pattern,
{
    with_cb_scalar(pat, n, func, |v| v.to_frac(), build)
}

pub(super) fn with_cb_f64<F>(pat: &Pattern, n: &Arg, func: &Arg, build: F) -> Res
where
    F: Fn(&Pattern, f64, &Callback) -> Pattern,
{
    with_cb_scalar(pat, n, func, |v| v.as_f64().unwrap_or(0.0), build)
}

/// Like [`with_cb_scalar`] but for the two-bound `within(a, b, f)`. When either
/// bound is a pattern, `a` provides the structure and `b` is `appLeft`-sampled
/// (Strudel's order), and the windowed result is probed per distinct `(a, b)`.
pub(super) fn with_cb_frac2<F>(pat: &Pattern, a: &Arg, b: &Arg, func: &Arg, build: F) -> Res
where
    F: Fn(&Pattern, Frac, Frac, &Callback) -> Pattern,
{
    let cb = Callback::new(func.clone());
    let result = if matches!(a, Arg::Num(_)) && matches!(b, Arg::Num(_)) {
        build(pat, to_value(a).to_frac(), to_value(b).to_frac(), &cb)
    } else {
        let paired = arg_to_pattern(a)
            .fmap(|av| Value::func(move |bv| Value::List(vec![av.clone(), bv])))
            .app_left(&arg_to_pattern(b));
        probe_patternify(paired, |pair| match pair {
            Value::List(xy) if xy.len() == 2 => build(pat, xy[0].to_frac(), xy[1].to_frac(), &cb),
            _ => pat.clone(),
        })
    };
    cb.finish()?;
    Ok(result.into())
}

/// Register the standalone (curried-style) forms of the higher-order callback
/// combinators, taking the pattern last (`jux(rev, pat)`, `every(4, f, pat)`).
/// The transform argument must be a function value: `rev`, `x => x.fast(2)`, or
/// a partially applied standalone transform (`fast(2)`, `ply("0")`). The
/// combinators curry too: called without the trailing pattern they return a
/// partial application (`every(4, rev)`).
pub(crate) fn register_standalone_callbacks(prelude: &Scope) {
    // The pattern is the last arg and the transform function the one before it;
    // any leading args (count `n`, time `t`, bounds `a`/`b`) come first.
    fn func_and_pat(a: &[Arg]) -> (&Arg, Pattern) {
        let func = a
            .len()
            .checked_sub(2)
            .and_then(|i| a.get(i))
            .unwrap_or(NULL);
        (func, arg_to_pattern(a.last().unwrap_or(NULL)))
    }
    // Leading arg `i` (before the function and pattern), or null if absent.
    fn lead(a: &[Arg], i: usize) -> &Arg {
        let present = a.len().checked_sub(2).is_some_and(|leading| i < leading);
        a.get(i).filter(|_| present).unwrap_or(NULL)
    }

    // Each macro registers a callback combinator group; `$name` is the
    // Strudel-facing name (snake or camelCase) and `$m` the core method.
    macro_rules! cb_only {
        ($($name:literal => $m:ident),* $(,)?) => {$(
            prelude.curried($name, 2, |a| {
                let (func, pat) = func_and_pat(a);
                let cb = Callback::new(func.clone());
                let out = pat.$m(|p| cb.apply(p));
                cb.finish()?;
                Ok(out.into())
            });
        )*};
    }
    // Standalone leading numeric arg: scalar fast path, else probe-patternify
    // (`chunk("<2 4>", f, pat)`), the same helpers the methods use.
    macro_rules! cb_i64 {
        ($($name:literal => $m:ident),* $(,)?) => {$(
            prelude.curried($name, 3, |a| {
                let (func, pat) = func_and_pat(a);
                with_cb_i64(&pat, lead(a, 0), func, |p, n, cb| p.$m(n, |p| cb.apply(p)))
            });
        )*};
    }
    macro_rules! cb_f64 {
        ($($name:literal => $m:ident),* $(,)?) => {$(
            prelude.curried($name, 3, |a| {
                let (func, pat) = func_and_pat(a);
                with_cb_f64(&pat, lead(a, 0), func, |p, n, cb| p.$m(n, |p| cb.apply(p)))
            });
        )*};
    }
    macro_rules! cb_frac {
        ($($name:literal => $m:ident),* $(,)?) => {$(
            prelude.curried($name, 3, |a| {
                let (func, pat) = func_and_pat(a);
                with_cb_frac(&pat, lead(a, 0), func, |p, n, cb| p.$m(n, |p| cb.apply(p)))
            });
        )*};
    }
    macro_rules! cb_pat {
        ($($name:literal => $m:ident),* $(,)?) => {$(
            prelude.curried($name, 3, |a| {
                let x = arg_to_pattern(lead(a, 0));
                let (func, pat) = func_and_pat(a);
                let cb = Callback::new(func.clone());
                let out = pat.$m(x, |p| cb.apply(p));
                cb.finish()?;
                Ok(out.into())
            });
        )*};
    }
    // `every`/`firstOf`/`lastOf` patternify their cycle count (the callback is
    // applied eagerly to the whole pattern, then placed by a patterned count).
    macro_rules! cb_cycles {
        ($($name:literal => $last:expr),* $(,)?) => {$(
            prelude.curried($name, 3, |a| {
                let n = arg_to_pattern(lead(a, 0));
                let (func, pat) = func_and_pat(a);
                let cb = Callback::new(func.clone());
                let transformed = cb.apply(&pat);
                cb.finish()?;
                Ok(pat.every_pat(n, transformed, $last).into())
            });
        )*};
    }

    cb_only! {
        "superimpose" => superimpose, "jux" => jux,
        "juxFlip" => jux_flip, "juxflip" => jux_flip, "flux" => jux_flip,
        "sometimes" => sometimes, "often" => often, "rarely" => rarely,
        "almostAlways" => almost_always, "almost_always" => almost_always,
        "almostNever" => almost_never, "almost_never" => almost_never,
        "someCycles" => some_cycles, "some_cycles" => some_cycles,
        "apply" => apply, "always" => always, "never" => never,
    }
    cb_i64! {
        "chunk" => chunk, "slowChunk" => chunk, "slowchunk" => chunk,
        "chunkBack" => chunk_back, "chunk_back" => chunk_back, "chunkback" => chunk_back,
        "fastChunk" => fast_chunk, "fastchunk" => fast_chunk, "fast_chunk" => fast_chunk,
    }
    cb_cycles! {
        "every" => false,
        "firstOf" => false, "first_of" => false,
        "lastOf" => true, "last_of" => true,
    }
    cb_f64! {
        "juxBy" => jux_by, "jux_by" => jux_by, "juxby" => jux_by,
        "juxFlipBy" => jux_flip_by, "juxflipby" => jux_flip_by,
        "fluxBy" => jux_flip_by, "fluxby" => jux_flip_by,
        "sometimesBy" => sometimes_by, "sometimes_by" => sometimes_by,
        "someCyclesBy" => some_cycles_by, "some_cycles_by" => some_cycles_by,
    }
    cb_frac! { "inside" => inside, "outside" => outside }
    cb_pat! { "off" => off, "when" => when }
    // whenKey(names, f, pat): `pat.when(keyDown(names), f)`. Unlike `when`'s
    // plain boolean pattern, the condition reads the live keyboard at query
    // time, so holding a key changes what plays without re-evaluating.
    prelude.curried("whenKey", 3, |a| {
        let keys = super::super::prelude::key_down_pattern(lead(a, 0));
        let (func, pat) = func_and_pat(a);
        let cb = Callback::new(func.clone());
        let out = pat.when(keys, |p| cb.apply(p));
        cb.finish()?;
        Ok(out.into())
    });
    prelude.curried("within", 4, |a| {
        let (func, pat) = func_and_pat(a);
        with_cb_frac2(&pat, lead(a, 0), lead(a, 1), func, |p, x, y, cb| {
            p.within(x, y, |p| cb.apply(p))
        })
    });

    // applyN(n, f, pat): apply the callback `n` times.
    prelude.curried("applyN", 3, |a| {
        let n = arg_to_f64(lead(a, 0)) as i64;
        let (func, pat) = func_and_pat(a);
        let cb = Callback::new(func.clone());
        let mut result = pat;
        for _ in 0..n.max(0) {
            result = cb.apply(&result);
        }
        cb.finish()?;
        Ok(result.into())
    });

    // echoWith/stutWith(times, time, func, pat): indexed delayed copies.
    for name in ["echoWith", "echowith", "stutWith", "stutwith"] {
        prelude.curried(name, 4, |a| {
            let times = arg_to_f64(lead(a, 0)) as i64;
            let time = Frac::from_f64(arg_to_f64(lead(a, 1)));
            let (func, pat) = func_and_pat(a);
            let cb = Callback::new(func.clone());
            let out = pat.echo_with(times, time, |p, i| cb.apply2(p, i));
            cb.finish()?;
            Ok(out.into())
        });
    }

    // plyWith/plyForEach(factor, func, pat): repeat each event `factor` times,
    // transforming the copies (probed and baked, like `arp_with`).
    use super::methods::{ply_build, ply_for_each_parts, ply_with_parts};
    type Parts = fn(&Value, &Callback, i64) -> Vec<Pattern>;
    for (names, parts) in [
        (["plyWith", "plywith"], ply_with_parts as Parts),
        (["plyForEach", "plyforeach"], ply_for_each_parts as Parts),
    ] {
        for name in names {
            prelude.curried(name, 3, move |a| {
                let factor = arg_to_f64(lead(a, 0)) as i64;
                let (func, pat) = func_and_pat(a);
                let cb = Callback::new(func.clone());
                let out = ply_build(&pat, factor, &cb, parts);
                cb.finish()?;
                Ok(out.into())
            });
        }
    }

    // into(pieces, func, pat) and chunkInto/chunkBackInto(n, func, pat).
    use super::methods::{chunk_pieces, into_build};
    prelude.curried("into", 3, |a| {
        let pieces = arg_to_pattern(lead(a, 0));
        let (func, pat) = func_and_pat(a);
        let cb = Callback::new(func.clone());
        let out = into_build(&pat, pieces, &cb);
        cb.finish()?;
        Ok(out.into())
    });
    for (names, back) in [
        (["chunkInto", "chunkinto"], false),
        (["chunkBackInto", "chunkbackinto"], true),
    ] {
        for name in names {
            prelude.curried(name, 3, move |a| {
                let n = arg_to_f64(lead(a, 0)) as i64;
                let (func, pat) = func_and_pat(a);
                let cb = Callback::new(func.clone());
                let pieces = if back {
                    chunk_pieces(n).iter(n)._early(Frac::one())
                } else {
                    chunk_pieces(n).iter_back(n)
                };
                let out = into_build(&pat, pieces, &cb);
                cb.finish()?;
                Ok(out.into())
            });
        }
    }

    // arpWith(func, pat): arpeggiate chords, transforming each chord pattern.
    use super::methods::arp_with_build;
    prelude.curried("arpWith", 2, |a| {
        let (func, pat) = func_and_pat(a);
        let cb = Callback::new(func.clone());
        let out = arp_with_build(&pat, &cb);
        cb.finish()?;
        Ok(out.into())
    });
}

/// Marshals a script function into the `Fn(&Pattern) -> Pattern` shape that the
/// engine's higher-order combinators (`every`, `jux`, `sometimes`, ...) expect.
///
/// Those combinators apply their callback *eagerly* at construction time, so
/// the function is called right here, inside the native call that was handed
/// it. The first error raised by the callback is captured and surfaced once the
/// combinator returns; on error the input pattern is passed through unchanged.
pub(crate) struct Callback {
    func: Arg,
    err: RefCell<Option<String>>,
}

impl Callback {
    pub(crate) fn new(func: Arg) -> Self {
        Self {
            func,
            err: RefCell::new(None),
        }
    }

    /// Call the function, keeping the first error for [`Callback::finish`].
    fn call(&self, args: Vec<Arg>) -> Option<Arg> {
        match js::call(&self.func, args) {
            Ok(value) => Some(value),
            Err(e) => {
                self.err.borrow_mut().get_or_insert(e);
                None
            }
        }
    }

    /// Invoke the function with `p`. Anything but a pattern coming back leaves
    /// `p` as it was.
    ///
    /// The "function" may be a *pattern* of functions —
    /// `sometimesBy(.2, choose(x => …, x => …))`, `apply(pick({…}))` — which
    /// upstream's `register` samples per cycle like any other patterned
    /// argument. Which function applies is then only known at query time, so
    /// it is called from the query (see `convert::fn_to_value`). A value that
    /// is not a function leaves the pattern as it was.
    pub(crate) fn apply(&self, p: &Pattern) -> Pattern {
        if let Arg::Pat(functions) = &self.func {
            let p = p.clone();
            return functions
                .fmap(move |f| match f {
                    Value::Func(_) => f.apply(Value::Pat(Box::new(p.clone()))),
                    _ => Value::Pat(Box::new(p.clone())),
                })
                .inner_join();
        }
        match self.call(vec![Arg::Pat(p.clone())]) {
            Some(Arg::Pat(out)) => out,
            _ => p.clone(),
        }
    }

    /// Invoke the function with `(p, i)` for the indexed combinators
    /// (`echoWith`/`stutWith`/`plyForEach`).
    pub(super) fn apply2(&self, p: &Pattern, i: i64) -> Pattern {
        match self.call(vec![Arg::Pat(p.clone()), Arg::Num(i as f64)]) {
            Some(Arg::Pat(out)) => out,
            _ => p.clone(),
        }
    }

    /// Invoke the function with a single rudel value and convert the result
    /// back into one.
    pub(super) fn apply_value(&self, value: Value) -> Value {
        match self.call(vec![value_to_arg(value.clone())]) {
            Some(out) => to_value(&out),
            None => value,
        }
    }

    /// Invoke the function with an already-built argument and convert the
    /// result back into a rudel value. Used by `log`, whose callback is handed
    /// the whole hap and returns the message to write.
    pub(super) fn apply_arg(&self, arg: Arg) -> Value {
        self.call(vec![arg])
            .map_or(Value::Null, |out| to_value(&out))
    }

    /// Invoke the function with an already-built argument and read the result
    /// as a truth value. Used by the predicate combinators (`filter`,
    /// `filterWhen`), which keep a hap when the callback says so; a callback
    /// that errors keeps the hap, so a broken predicate drops nothing.
    pub(super) fn apply_predicate(&self, arg: Arg) -> bool {
        self.call(vec![arg]).is_none_or(|out| truthy(&out))
    }

    /// Surface the first callback error (if any) after the combinator has run.
    pub(crate) fn finish(self) -> Result<(), String> {
        match self.err.into_inner() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// JavaScript's truthiness, for what a predicate returns.
fn truthy(value: &Arg) -> bool {
    match value {
        Arg::Null => false,
        Arg::Bool(b) => *b,
        Arg::Num(n) => *n != 0.0 && !n.is_nan(),
        Arg::Str(s) => !s.is_empty(),
        _ => true,
    }
}

pub(super) fn static_period_pattern(
    mut haps: Vec<rudel_core::Hap>,
    steps: Option<Frac>,
    period: Frac,
) -> Pattern {
    haps.sort_by_key(|h| h.part.begin);
    Pattern::new(move |state| {
        let mut out = Vec::new();
        let first_repeat = (state.span.begin / period).floor().numer() as i64;
        let last_repeat = (state.span.end / period).ceil().numer() as i64;
        for repeat in first_repeat..last_repeat {
            let offset = period * Frac::int(repeat);
            for template in &haps {
                let mut hap = template.clone().with_span(|span| span.with_time(|t| t + offset));
                if let Some(part) = hap.part.intersection(&state.span) {
                    hap.part = part;
                    out.push(hap);
                }
            }
        }
        out
    })
    .set_steps(steps)
}

/// `pat.method(..., f)`: run `body` with a [`Callback`] over argument
/// `callback_arg`, then surface whatever the callback raised.
pub(super) fn with_callback(
    pat: &Pattern,
    args: &[Arg],
    callback_arg: usize,
    body: impl FnOnce(&Pattern, &Callback) -> Pattern,
) -> Res {
    // `pat.rarely()`: upstream's `register` turns a one-argument method's
    // missing argument into `sequence()`, which is silence, so the method
    // plays nothing rather than failing.
    if callback_arg == 0 && args.is_empty() {
        return Ok(rudel_core::silence().into());
    }
    let cb = Callback::new(arg(args, callback_arg).clone());
    let result = body(pat, &cb);
    cb.finish()?;
    Ok(result.into())
}
