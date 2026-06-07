// Several Koto methods are deliberately named in camelCase to match Strudel's
// public API exactly (e.g. `iterBack`, `euclidLegato`); the koto derive macro
// also generates `__koto_<name>` shims that inherit those names.
#![allow(non_snake_case)]

use koto::derive::*;
use koto::prelude::*;
use koto::runtime::{Error as KotoError, KotoObject, Result as KotoResult};
use rudel_core::{Frac, Pattern, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// The control key marking which output a hap is routed to (`.midi()`/`.osc()`).
const IO_KEY: &str = "_io";

/// Keep haps routed to `target` via the `_io` control, plus untagged haps when
/// `include_untagged` (the default output). The routing keys (`_io`/`_midiport`)
/// are stripped from kept haps so they don't leak into the back-end.
pub fn filter_output(pat: &Pattern, target: &str, include_untagged: bool) -> Pattern {
    let target = target.to_string();
    pat.filter_values(move |v| match v {
        Value::Map(m) => match m.get(IO_KEY).and_then(|x| x.as_str()) {
            Some(io) => io == target,
            None => include_untagged,
        },
        _ => include_untagged,
    })
    .fmap(|v| match v {
        Value::Map(mut m) => {
            m.remove(IO_KEY);
            m.remove("_midiport");
            Value::Map(m)
        }
        other => other,
    })
}

/// Which tagged outputs (`midi`, `osc`) the pattern routes any haps to over the
/// first cycle. The app uses this to decide which back-ends to start.
pub fn output_targets(pat: &Pattern) -> (bool, bool) {
    let (mut midi, mut osc) = (false, false);
    for hap in pat.query_arc(Frac::zero(), Frac::one()) {
        if let Value::Map(m) = &hap.value
            && let Some(io) = m.get(IO_KEY).and_then(|x| x.as_str())
        {
            match io {
                "midi" => midi = true,
                "osc" => osc = true,
                _ => {}
            }
        }
    }
    (midi, osc)
}

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

pub(crate) fn arg_to_f64(value: &KValue) -> f64 {
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

/// A stable string key for a chord value, used to memoise `arp_with` callback
/// results so the (non-`Send`) Koto VM is only touched at construction time.
fn value_sig(v: &Value) -> String {
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

enum PatternLookup {
    List(Vec<Pattern>),
    Map(HashMap<String, Pattern>),
}

fn lookup_from_koto(value: &KValue) -> Option<PatternLookup> {
    match value {
        KValue::List(l) => Some(PatternLookup::List(
            l.data().iter().map(arg_to_pattern).collect(),
        )),
        KValue::Tuple(t) => Some(PatternLookup::List(
            t.data().iter().map(arg_to_pattern).collect(),
        )),
        KValue::Map(m) => {
            let mut out = HashMap::new();
            for (k, v) in m.data().iter() {
                if let KValue::Str(key) = k.value() {
                    out.insert(key.to_string(), arg_to_pattern(v));
                }
            }
            Some(PatternLookup::Map(out))
        }
        _ => None,
    }
}

fn is_lookup(value: &KValue) -> bool {
    matches!(value, KValue::List(_) | KValue::Tuple(_) | KValue::Map(_))
}

fn pick_from_lookup(lookup: PatternLookup, selector: Pattern, modulo: bool) -> Pattern {
    match lookup {
        PatternLookup::List(items) => {
            if items.is_empty() {
                return rudel_core::silence();
            }
            selector
                .fmap(move |v| {
                    let raw = v.as_f64().unwrap_or(0.0).round() as i64;
                    let idx = if modulo {
                        raw.rem_euclid(items.len() as i64)
                    } else {
                        raw.clamp(0, items.len() as i64 - 1)
                    } as usize;
                    Value::Pat(Box::new(items[idx].clone()))
                })
                .inner_join()
        }
        PatternLookup::Map(items) => {
            if items.is_empty() {
                return rudel_core::silence();
            }
            selector
                .fmap(move |v| {
                    let key = match v {
                        Value::Str(s) => s,
                        Value::Int(n) => n.to_string(),
                        Value::F64(x) => {
                            let s = format!("{x:.0}");
                            s
                        }
                        _ => String::new(),
                    };
                    items
                        .get(&key)
                        .cloned()
                        .map(|p| Value::Pat(Box::new(p)))
                        .unwrap_or(Value::Null)
                })
                .filter_values(|v| !matches!(v, Value::Null))
                .inner_join()
        }
    }
}

fn pick_args(args: &[KValue], modulo: bool) -> Pattern {
    let Some(first) = args.first() else {
        return rudel_core::silence();
    };
    let Some(second) = args.get(1) else {
        return rudel_core::silence();
    };
    let (lookup_value, selector_value) = if is_lookup(second) && !is_lookup(first) {
        (second, first)
    } else {
        (first, second)
    };
    let Some(lookup) = lookup_from_koto(lookup_value) else {
        return rudel_core::silence();
    };
    pick_from_lookup(lookup, arg_to_pattern(selector_value), modulo)
}

fn method_arg(ctx: &MethodContext<KPattern>, i: usize) -> KValue {
    ctx.args.get(i).cloned().unwrap_or(KValue::Null)
}

fn method_pattern_arg(ctx: &MethodContext<KPattern>, i: usize) -> Pattern {
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

fn method_literal_or_pattern_arg(ctx: &MethodContext<KPattern>, i: usize) -> Pattern {
    literal_or_pattern_arg(&method_arg(ctx, i))
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

            // `pat.arp_with(|chord| ...)`: arpeggiate chords, transforming each
            // chord (presented as a sequence of its notes) with a callback.
            //
            // The callback can't run in the (Send+Sync) query path because the
            // Koto VM isn't Send, so we evaluate it eagerly here: probe the
            // distinct chords over the first `PROBE` cycles, run the callback on
            // each, and bake the results into a lookup the query path consults.
            // Chords first appearing after the probe window fall back to silence.
            #[koto_method]
            fn arp_with(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                const PROBE: i64 = 16;
                let collected = ctx.instance()?.0.collect();
                let cb = Callback::new(&ctx, method_arg(&ctx, 0));
                let mut table: HashMap<String, Pattern> = HashMap::new();
                for cycle in 0..PROBE {
                    for hap in collected.query_arc(Frac::int(cycle), Frac::int(cycle + 1)) {
                        if let Value::List(notes) = &hap.value {
                            let sig = value_sig(&hap.value);
                            if !table.contains_key(&sig) {
                                let pats: Vec<Pattern> =
                                    notes.iter().cloned().map(rudel_core::pure).collect();
                                let chord = rudel_core::fastcat(&pats);
                                table.insert(sig, cb.apply(&chord));
                            }
                        }
                    }
                }
                cb.finish()?;
                let table = Arc::new(table);
                let result = collected.inner_bind(move |value| match &value {
                    Value::List(_) => table
                        .get(&value_sig(&value))
                        .cloned()
                        .unwrap_or_else(rudel_core::silence),
                    _ => rudel_core::silence(),
                });
                Ok(KPattern::wrap(result))
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

            #[koto_method]
            fn i(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                if ctx.args.is_empty() {
                    with_instance(&ctx, |pat| pat.wrap_control("i"))
                } else {
                    let arg = method_pattern_arg(&ctx, 0);
                    with_instance(&ctx, |pat| pat.i(arg.clone()))
                }
            }

            #[koto_method]
            fn freq(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                if ctx.args.is_empty() {
                    with_instance(&ctx, |pat| pat.wrap_control("freq"))
                } else {
                    let arg = method_pattern_arg(&ctx, 0);
                    with_instance(&ctx, |pat| pat.freq(arg.clone()))
                }
            }

            #[koto_method]
            fn tune(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let scale = method_literal_or_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.tune(scale.clone()))
            }

            #[koto_method]
            fn xen(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let scale = method_literal_or_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.xen(scale.clone()))
            }

            #[koto_method]
            fn with_base(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let base = method_literal_or_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.with_base(base.clone()))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn withBase(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let base = method_literal_or_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.with_base(base.clone()))
            }

            #[koto_method]
            fn ftrans(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let amount = method_literal_or_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.ftrans(amount.clone()))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn fTrans(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let amount = method_literal_or_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.ftrans(amount.clone()))
            }

            #[koto_method]
            fn ftranspose(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let amount = method_literal_or_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.ftrans(amount.clone()))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn fTranspose(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let amount = method_literal_or_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.ftrans(amount.clone()))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn bendRange(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let arg = method_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.bend_range(arg.clone()))
            }

            // Sample looping. `loop` is a Koto keyword but is allowed after `.`,
            // so these expose the Strudel names (`loop`/`loopBegin`/`loopEnd`)
            // as aliases of the keyword-safe Rust method names.
            // `pat.partials([1, 0.5, 0.3])` / `pat.partials(8)`: additive
            // harmonic magnitudes (or a count). `pat.phases([...])`: per-harmonic
            // phase offsets. The value is the whole list, applied to every event.
            #[koto_method]
            fn partials(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let v = koto_to_value(&method_arg(&ctx, 0));
                with_instance(&ctx, |pat| pat.ctrl("partials", rudel_core::pure(v.clone())))
            }

            #[koto_method]
            fn phases(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let v = koto_to_value(&method_arg(&ctx, 0));
                with_instance(&ctx, |pat| pat.ctrl("phases", rudel_core::pure(v.clone())))
            }

            // `pat.ctrl("fmi20", 3)`: set an arbitrary named control. The escape
            // hatch for FM-matrix edges / higher operators without a method.
            #[koto_method]
            fn ctrl(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let name = match method_arg(&ctx, 0) {
                    KValue::Str(s) => s.to_string(),
                    other => return runtime_error!("ctrl: expected a control name string, got {other:?}"),
                };
                let value = method_pattern_arg(&ctx, 1);
                with_instance(&ctx, |pat| pat.ctrl(name.clone(), value.clone()))
            }

            #[koto_method]
            fn sound(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let arg = method_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.s(arg.clone()))
            }

            #[koto_method(alias = "struct")]
            fn struct_alias(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let arg = method_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.struct_pat(arg.clone()))
            }

            #[koto_method]
            fn pick(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let selector = ctx.instance()?.0.clone();
                let Some(lookup) = lookup_from_koto(&method_arg(&ctx, 0)) else {
                    return Ok(KPattern::wrap(rudel_core::silence()));
                };
                Ok(KPattern::wrap(pick_from_lookup(lookup, selector, false)))
            }

            #[koto_method]
            fn pickmod(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let selector = ctx.instance()?.0.clone();
                let Some(lookup) = lookup_from_koto(&method_arg(&ctx, 0)) else {
                    return Ok(KPattern::wrap(rudel_core::silence()));
                };
                Ok(KPattern::wrap(pick_from_lookup(lookup, selector, true)))
            }

            #[koto_method(alias = "loop")]
            fn loop_play(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let arg = method_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.loop_play(arg.clone()))
            }

            #[koto_method(alias = "loopBegin", alias = "loopb")]
            fn loop_begin(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let arg = method_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.loop_begin(arg.clone()))
            }

            #[koto_method(alias = "loopEnd", alias = "loope")]
            fn loop_end(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let arg = method_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.loop_end(arg.clone()))
            }

            // `.p(name)`: tag a pattern with an `id` (Strudel's per-pattern
            // naming, e.g. `s("bd").p("drums")`). The name may be a string or a
            // number (`$1`-style slots).
            #[koto_method]
            fn p(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let name = match method_arg(&ctx, 0) {
                    KValue::Str(s) => s.to_string(),
                    KValue::Number(n) => n.to_string(),
                    _ => String::new(),
                };
                with_instance(&ctx, |pat| {
                    pat.ctrl("id", rudel_core::pure(Value::Str(name.clone())))
                })
            }

            // `.midi(device?)`: route this pattern to the MIDI output. The
            // optional device-name hint is stored as `_midiport`. Sets the
            // routing tag the app reads via `output_targets`/`filter_output`.
            #[koto_method]
            fn midi(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let port = match method_arg(&ctx, 0) {
                    KValue::Str(s) => Some(s.to_string()),
                    _ => None,
                };
                with_instance(&ctx, |pat| {
                    let mut p = pat.ctrl(IO_KEY, rudel_core::pure(Value::Str("midi".into())));
                    if let Some(port) = &port {
                        p = p.ctrl("_midiport", rudel_core::pure(Value::Str(port.clone())));
                    }
                    p
                })
            }

            // `.osc(target?)`: route this pattern to the OSC output. An optional
            // `"host:port"` target sets `oschost`/`oscport` (per-event routing).
            #[koto_method]
            fn osc(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let target = match method_arg(&ctx, 0) {
                    KValue::Str(s) => Some(s.to_string()),
                    _ => None,
                };
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

            // `.chord()` (zero-arg) expands chord names into note stacks;
            // `.chord(value)` sets the Strudel-style chord control consumed by
            // `.voicing()` / `.root_notes()`.
            #[koto_method]
            fn chord(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                if ctx.args.is_empty() {
                    with_instance(&ctx, |pat| pat.chord())
                } else {
                    let arg = method_pattern_arg(&ctx, 0);
                    with_instance(&ctx, |pat| {
                        pat.set(rudel_core::control_dyn("chord", arg.clone()))
                    })
                }
            }

            // -- Strudel-style camelCase aliases for snake_case transforms. --
            // Each is named in camelCase (with `#[allow(non_snake_case)]`) so the
            // exposed Koto method name matches Strudel exactly.
            #[koto_method]
            #[allow(non_snake_case)]
            fn iterBack(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = method_i64_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.iter_back(n))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn fastGap(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let arg = method_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.fast_gap(arg.clone()))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn repeatCycles(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = method_i64_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.repeat_cycles(n))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn pressBy(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = method_frac_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.press_by(n))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn swingBy(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let a = method_frac_arg(&ctx, 0);
                let b = method_frac_arg(&ctx, 1);
                with_instance(&ctx, |pat| pat.swing_by(a, b))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn euclidRot(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let a = method_i64_arg(&ctx, 0);
                let b = method_i64_arg(&ctx, 1);
                let c = method_i64_arg(&ctx, 2);
                with_instance(&ctx, |pat| pat.euclid_rot(a, b, c))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn euclidLegato(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let a = method_i64_arg(&ctx, 0);
                let b = method_i64_arg(&ctx, 1);
                with_instance(&ctx, |pat| pat.euclid_legato(a, b))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn euclidLegatoRot(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let a = method_i64_arg(&ctx, 0);
                let b = method_i64_arg(&ctx, 1);
                let c = method_i64_arg(&ctx, 2);
                with_instance(&ctx, |pat| pat.euclid_legato_rot(a, b, c))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn scaleTranspose(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let arg = method_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.scale_transpose(arg.clone()))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn scaleTrans(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let arg = method_pattern_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.strans(arg.clone()))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn rootNotes(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = method_i64_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.root_notes(n))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn loopAt(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = method_frac_arg(&ctx, 0);
                with_instance(&ctx, |pat| pat.loop_at(n))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn toBipolar(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                with_instance(&ctx, |pat| pat.to_bipolar())
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn fromBipolar(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                with_instance(&ctx, |pat| pat.from_bipolar())
            }

            // camelCase aliases for the higher-order (callback) combinators.
            #[koto_method]
            #[allow(non_snake_case)]
            fn firstOf(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = method_i64_arg(&ctx, 0);
                with_callback(&ctx, 1, |pat, cb| pat.first_of(n, |p| cb.apply(p)))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn lastOf(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = method_i64_arg(&ctx, 0);
                with_callback(&ctx, 1, |pat, cb| pat.last_of(n, |p| cb.apply(p)))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn chunkBack(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = method_i64_arg(&ctx, 0);
                with_callback(&ctx, 1, |pat, cb| pat.chunk_back(n, |p| cb.apply(p)))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn juxBy(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = method_f64_arg(&ctx, 0);
                with_callback(&ctx, 1, |pat, cb| pat.jux_by(n, |p| cb.apply(p)))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn sometimesBy(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = method_f64_arg(&ctx, 0);
                with_callback(&ctx, 1, |pat, cb| pat.sometimes_by(n, |p| cb.apply(p)))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn someCycles(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                with_callback(&ctx, 0, |pat, cb| pat.some_cycles(|p| cb.apply(p)))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn someCyclesBy(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                let n = method_f64_arg(&ctx, 0);
                with_callback(&ctx, 1, |pat, cb| pat.some_cycles_by(n, |p| cb.apply(p)))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn almostAlways(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                with_callback(&ctx, 0, |pat, cb| pat.almost_always(|p| cb.apply(p)))
            }

            #[koto_method]
            #[allow(non_snake_case)]
            fn almostNever(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                with_callback(&ctx, 0, |pat, cb| pat.almost_never(|p| cb.apply(p)))
            }
        }
    };
}

kpattern_methods! {
    pattern_arg: [
        fast, slow, ply, segment, seg, add, sub, mul, div, modulo, pow, set, keep, mask, struct_pat,
        early, late, fast_gap,
        note, n, s, mpe, gain, postgain, pan, speed, cutoff, resonance, room, size, shape, crush, delay,
        delaytime, delayfeedback, dry, attack, decay, sustain, release, vowel, bank, cut, accelerate, coarse,
        orbit, velocity, begin, end, legato, clip,
        hcutoff, hresonance, bandf, bandq, ftype,
        // filter envelopes + short aliases
        lpenv, lpattack, lpdecay, lpsustain, lprelease,
        hpenv, hpattack, hpdecay, hpsustain, hprelease,
        bpenv, bpattack, bpdecay, bpsustain, bprelease, fanchor,
        lpe, lpa, lpd, lps, lpr, hpe, hpa, hpd, hps, hpr, bpe, bpa, bpd, bps, bpr,
        // supersaw + FM + ADSR shortcuts
        unison, detune, spread, fm, fmh, fmi, fmwave, fmattack, fmdecay, fmsustain, fmrelease,
        fmi2, fmh2, fmwave2, fmattack2, fmdecay2, fmsustain2, fmrelease2,
        pw, noise, pcurve, adsr, ad, ar, hold,
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
        // tonal / voicing controls
        mtranspose, ctranspose, dictionary, dict, anchor, offset, octaves, mode,
        // OSC routing controls
        oschost, oscport,
    ],
    no_arg: [
        rev, revv, palindrome, degrade, undegrade, press, brak, round, floor, ceil,
        to_bipolar, from_bipolar, ratio, fit, arpeggiate, voicing,
    ],
    i64_arg: [
        iter, iter_back, repeat_cycles, expand, extend, contract, shrink, grow,
        chop, striate, take, drop, root_notes,
    ],
    frac_arg: [hurry, press_by, swing, loop_at, pace],
    pattern_pattern_arg: [slice, splice],
    frac_frac_arg: [focus, swing_by, compress, zoom, ribbon, rib],
    f64_f64_arg: [range, range2, rangex],
    i64_i64_arg: [euclid, euclid_legato],
    i64_i64_i64_arg: [euclid_rot, euclid_legato_rot],
    i64_frac_f64_arg: [echo],
    i64_f64_frac_arg: [stut],
    fn_arg: [
        superimpose, jux, sometimes, often, rarely, almost_always, almost_never, some_cycles,
        apply, always, never,
    ],
    i64_fn_arg: [every, first_of, last_of, chunk, chunk_back],
    frac_fn_arg: [inside, outside],
    f64_fn_arg: [jux_by, sometimes_by, some_cycles_by],
    pattern_fn_arg: [off, when],
    frac_frac_fn_arg: [within],
}

/// Add the rudel top-level functions to a Koto prelude.
pub(crate) fn register(prelude: &KMap) {
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
    prelude.add_fn("pick", |ctx| {
        Ok(KPattern(pick_args(ctx.args(), false)).into())
    });
    prelude.add_fn("pickmod", |ctx| {
        Ok(KPattern(pick_args(ctx.args(), true)).into())
    });
    prelude.add_fn("pat", |ctx| Ok(KPattern(arg_to_pattern(&arg0(ctx))).into()));
    prelude.add_fn("rev", |ctx| {
        Ok(KPattern(arg_to_pattern(&arg0(ctx)).rev()).into())
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
    // MIDI input: `ccin(cc)` / `ccin(cc, chan)` is a 0..1 signal of the latest
    // value of an incoming control-change (the input counterpart to `ccn`).
    prelude.add_fn("ccin", |ctx| {
        let cc = arg_to_f64(&arg0(ctx)) as u8;
        let chan = ctx
            .args()
            .get(1)
            .map(|v| arg_to_f64(v) as u8)
            .filter(|c| *c >= 1);
        Ok(KPattern(rudel_core::cc_in(cc, chan)).into())
    });
}

pub(crate) fn arg0(ctx: &mut CallContext) -> KValue {
    ctx.args().first().cloned().unwrap_or(KValue::Null)
}

/// Convert a Koto value into a literal rudel [`Value`], recursing into
/// lists/tuples. Used by list-valued controls like `partials`/`phases`.
fn koto_to_value(value: &KValue) -> Value {
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
        KValue::List(l) => Value::List(l.data().iter().map(koto_to_value).collect()),
        KValue::Tuple(t) => Value::List(t.data().iter().map(koto_to_value).collect()),
        _ => Value::Null,
    }
}

/// Convert a Koto value into a literal rudel [`Value`] (no mini-notation
/// parsing — used by `pure`).
pub(crate) fn arg_to_value(value: &KValue) -> Value {
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
