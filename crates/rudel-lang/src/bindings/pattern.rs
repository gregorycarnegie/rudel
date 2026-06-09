// Several Koto methods are deliberately named in camelCase to match Strudel's
// public API exactly (e.g. `iterBack`, `euclidLegato`); the koto derive macro
// also generates `__koto_<name>` shims that inherit those names.
#![allow(non_snake_case)]

use super::routing::IO_KEY;
use koto::derive::*;
use koto::prelude::*;
use koto::runtime::{Error as KotoError, KotoObject, Result as KotoResult};
use rudel_core::{Frac, Pattern, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

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
pub(super) fn arg_to_pattern(value: &KValue) -> Pattern {
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
pub(super) fn arg_to_weighted_pair(value: &KValue) -> (Frac, Pattern) {
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
pub(super) fn arg_to_pattern_weight(value: &KValue) -> (Pattern, f64) {
    let pair = |slice: &[KValue]| (arg_to_pattern(&slice[0]), arg_to_f64(&slice[1]));
    match value {
        KValue::List(l) if l.data().len() == 2 => pair(&l.data()),
        KValue::Tuple(t) if t.data().len() == 2 => pair(t.data()),
        _ => (arg_to_pattern(value), 1.0),
    }
}

/// Interpret an argument as a group of patterns for `stepalt`. A list/tuple
/// becomes a multi-element group; anything else is a single-element group.
pub(super) fn arg_to_group(value: &KValue) -> Vec<Pattern> {
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

pub(super) fn pick_args(args: &[KValue], modulo: bool) -> Pattern {
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

fn with_pattern_arg(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, Pattern) -> Pattern,
) -> KotoResult<KValue> {
    let arg = method_pattern_arg(ctx, 0);
    with_instance(ctx, |pat| f(pat, arg))
}

fn with_literal_or_pattern_arg(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, Pattern) -> Pattern,
) -> KotoResult<KValue> {
    let arg = method_literal_or_pattern_arg(ctx, 0);
    with_instance(ctx, |pat| f(pat, arg))
}

fn with_i64_arg(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, i64) -> Pattern,
) -> KotoResult<KValue> {
    let n = method_i64_arg(ctx, 0);
    with_instance(ctx, |pat| f(pat, n))
}

fn with_frac_arg(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, Frac) -> Pattern,
) -> KotoResult<KValue> {
    let n = method_frac_arg(ctx, 0);
    with_instance(ctx, |pat| f(pat, n))
}

fn with_pattern_pattern_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, Pattern, Pattern) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_pattern_arg(ctx, 0);
    let b = method_pattern_arg(ctx, 1);
    with_instance(ctx, |pat| f(pat, a, b))
}

fn with_frac_frac_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, Frac, Frac) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_frac_arg(ctx, 0);
    let b = method_frac_arg(ctx, 1);
    with_instance(ctx, |pat| f(pat, a, b))
}

fn with_f64_f64_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, f64, f64) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_f64_arg(ctx, 0);
    let b = method_f64_arg(ctx, 1);
    with_instance(ctx, |pat| f(pat, a, b))
}

fn with_i64_i64_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, i64, i64) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_i64_arg(ctx, 0);
    let b = method_i64_arg(ctx, 1);
    with_instance(ctx, |pat| f(pat, a, b))
}

fn with_i64_i64_i64_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, i64, i64, i64) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_i64_arg(ctx, 0);
    let b = method_i64_arg(ctx, 1);
    let c = method_i64_arg(ctx, 2);
    with_instance(ctx, |pat| f(pat, a, b, c))
}

fn with_i64_frac_f64_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, i64, Frac, f64) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_i64_arg(ctx, 0);
    let b = method_frac_arg(ctx, 1);
    let c = method_f64_arg(ctx, 2);
    with_instance(ctx, |pat| f(pat, a, b, c))
}

fn with_i64_f64_frac_args(
    ctx: &MethodContext<KPattern>,
    f: impl FnOnce(&Pattern, i64, f64, Frac) -> Pattern,
) -> KotoResult<KValue> {
    let a = method_i64_arg(ctx, 0);
    let b = method_f64_arg(ctx, 1);
    let c = method_frac_arg(ctx, 2);
    with_instance(ctx, |pat| f(pat, a, b, c))
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

    /// Invoke the Koto function with a single Rudel value and convert the
    /// result back into a Rudel value.
    fn apply_value(&self, value: Value) -> Value {
        let fallback = value.clone();
        let call = self
            .vm
            .borrow_mut()
            .call_function(self.func.clone(), CallArgs::Single(value_to_koto(value)));
        match call {
            Ok(value) => koto_to_value(&value),
            Err(e) => {
                if self.err.borrow().is_none() {
                    *self.err.borrow_mut() = Some(e);
                }
                fallback
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

fn static_period_pattern(
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
                let mut hap = template.with_span(|span| span.with_time(|t| t + offset));
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

/// `pat.layer([f, g, ...])`: stack the results of applying each function in
/// the list to the pattern. Accepts a list/tuple of callables, or bare callable
/// args.
fn kpattern_layer(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
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

/// `pat.fmap(f)`: Strudel's value-level mapper. The Koto VM isn't Send+Sync,
/// so map one probe window eagerly and repeat that shape.
fn kpattern_fmap(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
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

/// `pat.arp_with(|chord| ...)`: arpeggiate chords, transforming each chord
/// (presented as a sequence of its notes) with a callback.
///
/// The callback can't run in the (Send+Sync) query path because the Koto VM
/// isn't Send, so we evaluate it eagerly here: probe the distinct chords over
/// the first `PROBE` cycles, run the callback on each, and bake the results
/// into a lookup the query path consults. Chords first appearing after the
/// probe window fall back to silence.
fn kpattern_arp_with(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    const PROBE: i64 = 16;
    let collected = ctx.instance()?.0.collect();
    let cb = Callback::new(&ctx, method_arg(&ctx, 0));
    let mut table: HashMap<String, Pattern> = HashMap::new();
    for cycle in 0..PROBE {
        for hap in collected.query_arc(Frac::int(cycle), Frac::int(cycle + 1)) {
            if let Value::List(notes) = &hap.value {
                let sig = value_sig(&hap.value);
                if !table.contains_key(&sig) {
                    let pats: Vec<Pattern> = notes.iter().cloned().map(rudel_core::pure).collect();
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

fn kpattern_voicings(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let dict = match method_arg(&ctx, 0) {
        KValue::Str(s) => s.to_string(),
        _ => "legacy".to_string(),
    };
    with_instance(&ctx, |pat| pat.voicings(dict.clone()))
}

fn kpattern_scale(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let name = match method_arg(&ctx, 0) {
        KValue::Str(s) => rudel_core::pure(Value::Str(s.to_string())),
        other => arg_to_pattern(&other),
    };
    with_instance(&ctx, |pat| pat.scale(name))
}

fn kpattern_i(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    if ctx.args.is_empty() {
        with_instance(&ctx, |pat| pat.wrap_control("i"))
    } else {
        with_pattern_arg(&ctx, |pat, arg| pat.i(arg))
    }
}

fn kpattern_freq(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    if ctx.args.is_empty() {
        with_instance(&ctx, |pat| pat.wrap_control("freq"))
    } else {
        with_pattern_arg(&ctx, |pat, arg| pat.freq(arg))
    }
}

fn kpattern_tune(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_literal_or_pattern_arg(&ctx, |pat, scale| pat.tune(scale))
}

fn kpattern_xen(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_literal_or_pattern_arg(&ctx, |pat, scale| pat.xen(scale))
}

fn kpattern_with_base(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_literal_or_pattern_arg(&ctx, |pat, base| pat.with_base(base))
}

fn kpattern_ftrans(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_literal_or_pattern_arg(&ctx, |pat, amount| pat.ftrans(amount))
}

fn kpattern_ftranspose(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_literal_or_pattern_arg(&ctx, |pat, amount| pat.ftrans(amount))
}

fn kpattern_partials(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let v = koto_to_value(&method_arg(&ctx, 0));
    with_instance(&ctx, |pat| {
        pat.ctrl("partials", rudel_core::pure(v.clone()))
    })
}

fn kpattern_phases(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let v = koto_to_value(&method_arg(&ctx, 0));
    with_instance(&ctx, |pat| pat.ctrl("phases", rudel_core::pure(v.clone())))
}

fn kpattern_ctrl(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let name = match method_arg(&ctx, 0) {
        KValue::Str(s) => s.to_string(),
        other => return runtime_error!("ctrl: expected a control name string, got {other:?}"),
    };
    let value = method_pattern_arg(&ctx, 1);
    with_instance(&ctx, |pat| pat.ctrl(name.clone(), value.clone()))
}

fn kpattern_sound(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_pattern_arg(&ctx, |pat, arg| pat.s(arg))
}

fn kpattern_struct_alias(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_pattern_arg(&ctx, |pat, arg| pat.struct_pat(arg))
}

fn kpattern_pick(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let selector = ctx.instance()?.0.clone();
    let Some(lookup) = lookup_from_koto(&method_arg(&ctx, 0)) else {
        return Ok(KPattern::wrap(rudel_core::silence()));
    };
    Ok(KPattern::wrap(pick_from_lookup(lookup, selector, false)))
}

fn kpattern_pickmod(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let selector = ctx.instance()?.0.clone();
    let Some(lookup) = lookup_from_koto(&method_arg(&ctx, 0)) else {
        return Ok(KPattern::wrap(rudel_core::silence()));
    };
    Ok(KPattern::wrap(pick_from_lookup(lookup, selector, true)))
}

fn kpattern_loop_play(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_pattern_arg(&ctx, |pat, arg| pat.loop_play(arg))
}

fn kpattern_loop_begin(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_pattern_arg(&ctx, |pat, arg| pat.loop_begin(arg))
}

fn kpattern_loop_end(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_pattern_arg(&ctx, |pat, arg| pat.loop_end(arg))
}

fn kpattern_p(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let name = match method_arg(&ctx, 0) {
        KValue::Str(s) => s.to_string(),
        KValue::Number(n) => n.to_string(),
        _ => String::new(),
    };
    with_instance(&ctx, |pat| {
        pat.ctrl("id", rudel_core::pure(Value::Str(name.clone())))
    })
}

fn kpattern_midi(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
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

fn kpattern_osc(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
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

fn kpattern_chord(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    if ctx.args.is_empty() {
        with_instance(&ctx, |pat| pat.chord())
    } else {
        with_pattern_arg(&ctx, |pat, arg| {
            pat.set(rudel_core::control_dyn("chord", arg))
        })
    }
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
        // CamelCase alias groups: each maps Camel => snake
        camel_pattern: [$($camel_pattern:ident => $snake_pattern:ident),* $(,)?],
        camel_literal_or_pattern: [$($camel_literal_or_pattern:ident => $snake_literal_or_pattern:ident),* $(,)?],
        camel_no_arg: [$($camel_no_arg:ident => $snake_no_arg:ident),* $(,)?],
        camel_noarg_fn: [$($camel_noarg_fn:ident => $snake_noarg_fn:ident),* $(,)?],
        camel_i64: [$($camel_i64:ident => $snake_i64:ident),* $(,)?],
        camel_frac: [$($camel_frac:ident => $snake_frac:ident),* $(,)?],
        camel_frac_frac: [$($camel_frac_frac:ident => $snake_frac_frac:ident),* $(,)?],
        camel_i64_i64: [$($camel_i64_i64:ident => $snake_i64_i64:ident),* $(,)?],
        camel_i64_i64_i64: [$($camel_i64_i64_i64:ident => $snake_i64_i64_i64:ident),* $(,)?],
        camel_i64_fn: [$($camel_i64_fn:ident => $snake_i64_fn:ident),* $(,)?],
        camel_f64_fn: [$($camel_f64_fn:ident => $snake_f64_fn:ident),* $(,)?],
    ) => {
        #[koto_impl]
        impl KPattern {
            $(
                #[koto_method]
                fn $pattern_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_pattern_arg(&ctx, |pat, arg| pat.$pattern_arg_method(arg))
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
                    with_i64_arg(&ctx, |pat, n| pat.$i64_arg_method(n))
                }
            )*

            $(
                #[koto_method]
                fn $frac_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_frac_arg(&ctx, |pat, n| pat.$frac_arg_method(n))
                }
            )*

            $(
                #[koto_method]
                fn $pattern_pattern_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_pattern_pattern_args(&ctx, |pat, a, b| pat.$pattern_pattern_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $frac_frac_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_frac_frac_args(&ctx, |pat, a, b| pat.$frac_frac_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $f64_f64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_f64_f64_args(&ctx, |pat, a, b| pat.$f64_f64_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $i64_i64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_i64_args(&ctx, |pat, a, b| pat.$i64_i64_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $i64_i64_i64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_i64_i64_args(&ctx, |pat, a, b, c| pat.$i64_i64_i64_arg_method(a, b, c))
                }
            )*

            $(
                #[koto_method]
                fn $i64_frac_f64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_frac_f64_args(&ctx, |pat, a, b, c| pat.$i64_frac_f64_arg_method(a, b, c))
                }
            )*

            $(
                #[koto_method]
                fn $i64_f64_frac_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_f64_frac_args(&ctx, |pat, a, b, c| pat.$i64_f64_frac_arg_method(a, b, c))
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
                kpattern_layer(ctx)
            }

            // `pat.fmap(f)`: Strudel's value-level mapper. The Koto VM isn't
            // Send+Sync, so map one cycle eagerly and repeat that shape.
            #[koto_method]
            fn fmap(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_fmap(ctx)
            }

            // CamelCase aliases: generate small wrappers that call the
            // existing snake_case implementations to reduce duplication.
            // Each alias group maps CamelCase -> snake_case and uses the
            // appropriate argument extractor.
            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_pattern(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_pattern_arg(&ctx, |pat, arg| pat.$snake_pattern(arg))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_literal_or_pattern(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_literal_or_pattern_arg(&ctx, |pat, arg| pat.$snake_literal_or_pattern(arg))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_no_arg(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_instance(&ctx, |pat| pat.$snake_no_arg())
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_noarg_fn(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_callback(&ctx, 0, |pat, cb| pat.$snake_noarg_fn(|p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_i64(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_arg(&ctx, |pat, n| pat.$snake_i64(n))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_frac(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_frac_arg(&ctx, |pat, n| pat.$snake_frac(n))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_frac_frac(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_frac_frac_args(&ctx, |pat, a, b| pat.$snake_frac_frac(a, b))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_i64_i64(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_i64_args(&ctx, |pat, a, b| pat.$snake_i64_i64(a, b))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_i64_i64_i64(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_i64_i64_args(&ctx, |pat, a, b, c| pat.$snake_i64_i64_i64(a, b, c))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_i64_fn(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_i64_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$snake_i64_fn(n, |p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_f64_fn(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_f64_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$snake_f64_fn(n, |p| cb.apply(p)))
                }
            )*

            // (no camel_frac_fn group)

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
                kpattern_arp_with(ctx)
            }

            // `pat.voicings("lefthand")`: voice chords with a named dictionary.
            #[koto_method]
            fn voicings(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_voicings(ctx)
            }

            #[koto_method]
            fn scale(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_scale(ctx)
            }

            #[koto_method]
            fn i(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_i(ctx)
            }

            #[koto_method]
            fn freq(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_freq(ctx)
            }

            #[koto_method]
            fn tune(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_tune(ctx)
            }

            #[koto_method]
            fn xen(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_xen(ctx)
            }

            #[koto_method]
            fn with_base(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_with_base(ctx)
            }

            #[koto_method]
            fn ftrans(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_ftrans(ctx)
            }

            #[koto_method]
            fn ftranspose(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_ftranspose(ctx)
            }

            // `bendRange` alias generated above.

            // Sample looping. `loop` is a Koto keyword but is allowed after `.`,
            // so these expose the Strudel names (`loop`/`loopBegin`/`loopEnd`)
            // as aliases of the keyword-safe Rust method names.
            // `pat.partials([1, 0.5, 0.3])` / `pat.partials(8)`: additive
            // harmonic magnitudes (or a count). `pat.phases([...])`: per-harmonic
            // phase offsets. The value is the whole list, applied to every event.
            #[koto_method]
            fn partials(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_partials(ctx)
            }

            #[koto_method]
            fn phases(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_phases(ctx)
            }

            // `pat.ctrl("fmi20", 3)`: set an arbitrary named control. The escape
            // hatch for FM-matrix edges / higher operators without a method.
            #[koto_method]
            fn ctrl(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_ctrl(ctx)
            }

            #[koto_method]
            fn sound(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_sound(ctx)
            }

            #[koto_method(alias = "struct")]
            fn struct_alias(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_struct_alias(ctx)
            }

            #[koto_method]
            fn pick(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick(ctx)
            }

            #[koto_method]
            fn pickmod(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pickmod(ctx)
            }

            #[koto_method(alias = "loop")]
            fn loop_play(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_loop_play(ctx)
            }

            #[koto_method(alias = "loopBegin", alias = "loopb")]
            fn loop_begin(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_loop_begin(ctx)
            }

            #[koto_method(alias = "loopEnd", alias = "loope")]
            fn loop_end(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_loop_end(ctx)
            }

            // `.p(name)`: tag a pattern with an `id` (Strudel's per-pattern
            // naming, e.g. `s("bd").p("drums")`). The name may be a string or a
            // number (`$1`-style slots).
            #[koto_method]
            fn p(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_p(ctx)
            }

            // `.midi(device?)`: route this pattern to the MIDI output. The
            // optional device-name hint is stored as `_midiport`. Sets the
            // routing tag the app reads via `output_targets`/`filter_output`.
            #[koto_method]
            fn midi(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_midi(ctx)
            }

            // `.osc(target?)`: route this pattern to the OSC output. An optional
            // `"host:port"` target sets `oschost`/`oscport` (per-event routing).
            #[koto_method]
            fn osc(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_osc(ctx)
            }

            // `.chord()` (zero-arg) expands chord names into note stacks;
            // `.chord(value)` sets the Strudel-style chord control consumed by
            // `.voicing()` / `.root_notes()`.
            #[koto_method]
            fn chord(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_chord(ctx)
            }

            // CamelCase aliases are generated above from compact lists.
        }
    };
}

kpattern_methods! {
    pattern_arg: [
        fast, slow, ply, segment, seg, add, sub, mul, div, modulo, pow, set, keep, mask, struct_pat,
        early, late, fast_gap,
        note, n, s, mpe, gain, postgain, pan, speed, cutoff, resonance, room, roomlp, roomdim,
        roomfade, size, shape, crush, delay,
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
        delayt, delayfb, rlp, rdim, rfade, o, trans, strans,
        // alignment matrix (`in` is the default plain op; these are the rest)
        add_out, add_mix, add_squeeze, add_squeezeout, add_reset, add_restart,
        sub_out, mul_out, mul_squeeze, div_out,
        set_out, set_mix, set_squeeze, set_squeezeout,
        keep_out, keep_squeeze,
        add_poly, mul_poly, set_poly, keep_poly,
        transpose, scale_transpose, bend_range,
        overlay, arp,
        // tonal / voicing controls
        mtranspose, ctranspose, dictionary, dict, anchor, offset, octaves, mode,
        // OSC routing controls
        oschost, oscport,
    ],
    no_arg: [
        rev, revv, palindrome, degrade, undegrade, press, brak, round, floor, ceil,
        to_bipolar, from_bipolar, ratio, fit, arpeggiate, voicing, piano,
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
    // CamelCase alias mappings (Camel => snake)
    camel_pattern: [bendRange => bend_range, fastGap => fast_gap, scaleTranspose => scale_transpose, scaleTrans => strans],
    camel_literal_or_pattern: [withBase => with_base, fTrans => ftrans, fTranspose => ftranspose],
    camel_no_arg: [toBipolar => to_bipolar, fromBipolar => from_bipolar],
    camel_noarg_fn: [someCycles => some_cycles, almostAlways => almost_always, almostNever => almost_never],
    camel_i64: [iterBack => iter_back, repeatCycles => repeat_cycles, rootNotes => root_notes],
    camel_frac: [pressBy => press_by, loopAt => loop_at],
    camel_frac_frac: [swingBy => swing_by],
    camel_i64_i64: [euclidLegato => euclid_legato],
    camel_i64_i64_i64: [euclidRot => euclid_rot, euclidLegatoRot => euclid_legato_rot],
    camel_i64_fn: [firstOf => first_of, lastOf => last_of, chunkBack => chunk_back],
    camel_f64_fn: [juxBy => jux_by, sometimesBy => sometimes_by, someCyclesBy => some_cycles_by],
}

pub(crate) fn arg0(ctx: &mut CallContext) -> KValue {
    ctx.args().first().cloned().unwrap_or(KValue::Null)
}

/// Convert a Koto value into a literal rudel [`Value`], recursing into
/// lists/tuples. Used by list-valued controls like `partials`/`phases`.
pub(super) fn koto_to_value(value: &KValue) -> Value {
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
        KValue::Map(m) => {
            let mut out = BTreeMap::new();
            for (k, v) in m.data().iter() {
                if let KValue::Str(key) = k.value() {
                    out.insert(key.to_string(), koto_to_value(v));
                }
            }
            Value::Map(out)
        }
        _ => Value::Null,
    }
}

fn value_to_koto(value: Value) -> KValue {
    match value {
        Value::Null => KValue::Null,
        Value::Bool(b) => KValue::Bool(b),
        Value::Int(n) => KValue::Number(KNumber::from(n)),
        Value::F64(n) => KValue::Number(KNumber::from(n)),
        Value::Frac(f) => KValue::Number(KNumber::from(f.to_f64())),
        Value::Str(s) => KValue::Str(s.into()),
        Value::List(items) => {
            KList::with_data(items.into_iter().map(value_to_koto).collect()).into()
        }
        Value::Map(items) => {
            let map = KMap::new();
            for (key, value) in items {
                map.insert(key.as_str(), value_to_koto(value));
            }
            map.into()
        }
        Value::Func(_) => KValue::Null,
        Value::Pat(p) => KPattern(*p).into(),
    }
}

/// Convert a Koto value into a literal rudel [`Value`] (no mini-notation
/// parsing — used by `pure`).
pub(super) fn arg_to_value(value: &KValue) -> Value {
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
