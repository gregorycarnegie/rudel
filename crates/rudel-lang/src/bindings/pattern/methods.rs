use super::KPattern;
use super::args::{
    method_arg, method_pattern_arg, with_instance, with_literal_or_pattern_arg, with_pattern_arg,
};
use super::callback::{Callback, static_period_pattern};
use super::convert::{arg_to_pattern, koto_to_value};
use super::pick::{is_lookup, lookup_from_koto, pick_from_lookup};
use crate::bindings::routing::IO_KEY;
use koto::prelude::*;
use koto::runtime::Result as KotoResult;
use rudel_core::{Frac, Pattern, PickJoin, Value};
use std::collections::HashMap;
use std::sync::Arc;

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

/// Collect vararg-style arguments (`layer`, `tour`): a single list/tuple is
/// expanded into its elements, otherwise the varargs are used as-is.
fn collect_callables(args: &[KValue]) -> Vec<KValue> {
    match args {
        [KValue::List(l)] => l.data().iter().cloned().collect(),
        [KValue::Tuple(t)] => t.data().to_vec(),
        _ => args.to_vec(),
    }
}

/// `pat.tour(a, b, ...)`: insert the pattern into the list of patterns
/// stepwise, moving backwards one slot per repetition (also accepts a single
/// list/tuple of patterns).
pub(super) fn kpattern_tour(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let many: Vec<Pattern> = collect_callables(&ctx.args)
        .iter()
        .map(arg_to_pattern)
        .collect();
    Ok(KPattern::wrap(pat.tour(&many)))
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
    let pats: Vec<Pattern> = collect_callables(&ctx.args)
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

/// `pat.arp_with(|chord| ...)`: arpeggiate chords, transforming each chord
/// (presented as a sequence of its notes) with a callback.
///
/// The callback can't run in the (Send+Sync) query path because the Koto VM
/// isn't Send, so we evaluate it eagerly here: probe the distinct chords over
/// the first `PROBE` cycles, run the callback on each, and bake the results
/// into a lookup the query path consults. Chords first appearing after the
/// probe window fall back to silence.
pub(super) fn kpattern_arp_with(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
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

pub(super) fn kpattern_voicings(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let dict = match method_arg(&ctx, 0) {
        KValue::Str(s) => s.to_string(),
        _ => "legacy".to_string(),
    };
    with_instance(&ctx, |pat| pat.voicings(dict.clone()))
}

pub(super) fn kpattern_scale(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let name = match method_arg(&ctx, 0) {
        KValue::Str(s) => rudel_core::pure(Value::Str(s.to_string())),
        other => arg_to_pattern(&other),
    };
    with_instance(&ctx, |pat| pat.scale(name))
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

pub(super) fn kpattern_tune(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_literal_or_pattern_arg(&ctx, |pat, scale| pat.tune(scale))
}

pub(super) fn kpattern_xen(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_literal_or_pattern_arg(&ctx, |pat, scale| pat.xen(scale))
}

pub(super) fn kpattern_with_base(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_literal_or_pattern_arg(&ctx, |pat, base| pat.with_base(base))
}

pub(super) fn kpattern_ftrans(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_literal_or_pattern_arg(&ctx, |pat, amount| pat.ftrans(amount))
}

pub(super) fn kpattern_ftranspose(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_literal_or_pattern_arg(&ctx, |pat, amount| pat.ftrans(amount))
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
    let name = match method_arg(&ctx, 0) {
        KValue::Str(s) => s.to_string(),
        other => return runtime_error!("ctrl: expected a control name string, got {other:?}"),
    };
    let value = method_pattern_arg(&ctx, 1);
    with_instance(&ctx, |pat| pat.ctrl(name.clone(), value.clone()))
}

pub(super) fn kpattern_sound(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_pattern_arg(&ctx, |pat, arg| pat.s(arg))
}

pub(super) fn kpattern_struct_alias(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_pattern_arg(&ctx, |pat, arg| pat.struct_pat(arg))
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
    let names: Vec<String> = match method_arg(&ctx, 0) {
        KValue::Str(s) => s.split(':').map(str::to_string).collect(),
        KValue::List(items) => items
            .data()
            .iter()
            .filter_map(|v| match v {
                KValue::Str(s) => Some(s.to_string()),
                _ => None,
            })
            .collect(),
        KValue::Tuple(items) => items
            .iter()
            .filter_map(|v| match v {
                KValue::Str(s) => Some(s.to_string()),
                _ => None,
            })
            .collect(),
        other => {
            return runtime_error!("as: expected a control-name string or list, got {other:?}");
        }
    };
    with_instance(&ctx, |pat| {
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        pat.as_controls(&refs)
    })
}

pub(super) fn kpattern_loop_play(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_pattern_arg(&ctx, |pat, arg| pat.loop_play(arg))
}

pub(super) fn kpattern_loop_begin(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_pattern_arg(&ctx, |pat, arg| pat.loop_begin(arg))
}

pub(super) fn kpattern_loop_end(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    with_pattern_arg(&ctx, |pat, arg| pat.loop_end(arg))
}

pub(super) fn kpattern_p(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let name = match method_arg(&ctx, 0) {
        KValue::Str(s) => s.to_string(),
        KValue::Number(n) => n.to_string(),
        _ => String::new(),
    };
    with_instance(&ctx, |pat| {
        pat.ctrl("id", rudel_core::pure(Value::Str(name.clone())))
    })
}

pub(super) fn kpattern_midi(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
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

pub(super) fn kpattern_osc(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
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

pub(super) fn kpattern_chord(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    if ctx.args.is_empty() {
        with_instance(&ctx, |pat| pat.chord())
    } else {
        with_pattern_arg(&ctx, |pat, arg| {
            pat.set(rudel_core::control_dyn("chord", arg))
        })
    }
}
