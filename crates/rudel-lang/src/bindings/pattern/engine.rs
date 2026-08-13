//! The engine's own vocabulary, exposed to scripts: query state, time spans,
//! haps, exact fractions, and a `Pattern` built from a Koto function.
//!
//! This is the surface a script needs to define a pattern *combinator* rather
//! than just use one — Strudel scripts do it by patching `Pattern.prototype`
//! and returning `new Pattern(state => …)`, which is how `enumerate`, `warp`
//! and friends get written before they land upstream.
//!
//! It is only reachable because Koto is built with `arc` (see
//! `tests/send_sync.rs`): the query function a script writes is called *from
//! the query*, on whichever thread is asking. Everything else in the bindings
//! still runs Koto at construction time; this is the deliberate exception,
//! and the reason it is worth having is that no amount of eager probing can
//! express "look at the haps of this cycle and number them".
//!
//! Spans, haps and states are plain Koto **maps** rather than custom objects,
//! so `hap.part.begin` is ordinary map access and needs nothing from the
//! preprocessor. Fractions are an object, because they carry exact rational
//! arithmetic that a float would quietly lose.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    KPattern,
    convert::{koto_to_value, value_to_koto},
};
use koto::{derive::*, prelude::*, runtime::Result as KotoResult};
use rudel_core::{Frac, Hap, Pattern, State, TimeSpan};

/// An exact rational, as `Fraction(n)` hands back.
#[derive(Clone, Copy, KotoCopy, KotoType)]
pub struct KFrac(pub Frac);

impl KotoObject for KFrac {
    fn display(&self, ctx: &mut DisplayContext) -> KotoResult<()> {
        ctx.append(format!("{}/{}", self.0.numer(), self.0.denom()));
        Ok(())
    }
}

impl From<KFrac> for KValue {
    fn from(f: KFrac) -> KValue {
        KObject::from(f).into()
    }
}

/// A fraction from whatever a script passed: another fraction, or a number.
pub(super) fn to_frac(value: &KValue) -> Frac {
    match value {
        KValue::Object(o) if o.is_a::<KFrac>() => o.cast::<KFrac>().map_or(Frac::zero(), |f| f.0),
        KValue::Number(n) => Frac::from_f64(f64::from(n)),
        _ => Frac::zero(),
    }
}

fn arg(ctx: &MethodContext<KFrac>, i: usize) -> KValue {
    ctx.args.get(i).cloned().unwrap_or(KValue::Null)
}

#[koto_impl]
impl KFrac {
    #[koto_method]
    fn add(ctx: MethodContext<Self>) -> KotoResult<KValue> {
        Ok(KFrac(ctx.instance()?.0 + to_frac(&arg(&ctx, 0))).into())
    }

    #[koto_method]
    fn sub(ctx: MethodContext<Self>) -> KotoResult<KValue> {
        Ok(KFrac(ctx.instance()?.0 - to_frac(&arg(&ctx, 0))).into())
    }

    #[koto_method]
    fn mul(ctx: MethodContext<Self>) -> KotoResult<KValue> {
        Ok(KFrac(ctx.instance()?.0 * to_frac(&arg(&ctx, 0))).into())
    }

    #[koto_method]
    fn div(ctx: MethodContext<Self>) -> KotoResult<KValue> {
        let by = to_frac(&arg(&ctx, 0));
        if by == Frac::zero() {
            return Ok(KFrac(Frac::zero()).into());
        }
        Ok(KFrac(ctx.instance()?.0 / by).into())
    }

    /// The cycle this instant falls in, as a span (`wholeCycle`).
    #[koto_method]
    fn wholeCycle(ctx: MethodContext<Self>) -> KotoResult<KValue> {
        let t = ctx.instance()?.0;
        Ok(span_to_koto(&TimeSpan::new(t.sam(), t.next_sam())))
    }

    #[koto_method]
    fn sam(ctx: MethodContext<Self>) -> KotoResult<KValue> {
        Ok(KFrac(ctx.instance()?.0.sam()).into())
    }

    #[koto_method]
    fn floor(ctx: MethodContext<Self>) -> KotoResult<KValue> {
        Ok(KFrac(ctx.instance()?.0.floor()).into())
    }

    #[koto_method]
    fn ceil(ctx: MethodContext<Self>) -> KotoResult<KValue> {
        Ok(KFrac(ctx.instance()?.0.ceil()).into())
    }

    /// The float form, for arithmetic that does not need to stay exact.
    #[koto_method]
    fn toNumber(ctx: MethodContext<Self>) -> KotoResult<KValue> {
        Ok(KValue::Number(KNumber::from(ctx.instance()?.0.to_f64())))
    }
}

/// A time span as a map, so `span.begin` is ordinary map access.
pub(super) fn span_to_koto(span: &TimeSpan) -> KValue {
    let map = KMap::new();
    map.insert("begin", KFrac(span.begin));
    map.insert("end", KFrac(span.end));
    let this = *span;
    map.add_fn("duration", move |_| Ok(KFrac(this.duration()).into()));
    map.add_fn("cycleArc", move |_| Ok(span_to_koto(&this.cycle_arc())));
    // Empty overlap is `undefined` in Strudel, which scripts test for.
    map.add_fn("intersection", move |ctx| {
        let Some(other) = ctx.args().first().and_then(span_from_koto) else {
            return Ok(KValue::Null);
        };
        Ok(this
            .intersection(&other)
            .map_or(KValue::Null, |s| span_to_koto(&s)))
    });
    KValue::Map(map)
}

pub(super) fn span_from_koto(value: &KValue) -> Option<TimeSpan> {
    let KValue::Map(map) = value else {
        return None;
    };
    Some(TimeSpan::new(
        to_frac(&map.get("begin")?),
        to_frac(&map.get("end")?),
    ))
}

/// A hap as a map: `whole` (or `null` for an analog hap), `part`, `value`.
pub(super) fn hap_to_koto(hap: &Hap) -> KValue {
    let map = KMap::new();
    map.insert(
        "whole",
        hap.whole.as_ref().map_or(KValue::Null, span_to_koto),
    );
    map.insert("part", span_to_koto(&hap.part));
    map.insert("value", value_to_koto(hap.value.clone()));
    KValue::Map(map)
}

pub(super) fn hap_from_koto(value: &KValue) -> Option<Hap> {
    let KValue::Map(map) = value else {
        return None;
    };
    let part = span_from_koto(&map.get("part")?)?;
    let whole = map.get("whole").as_ref().and_then(span_from_koto);
    Some(Hap::new(
        whole,
        part,
        map.get("value")
            .map_or(rudel_core::Value::Null, |v| koto_to_value(&v)),
    ))
}

/// A query state as a map, with the `withSpan` a combinator narrows it by.
fn state_to_koto(state: &State) -> KValue {
    let map = KMap::new();
    map.insert("span", span_to_koto(&state.span));
    let this = state.clone();
    let for_set = state.clone();
    map.add_fn("withSpan", move |ctx| {
        let Some(f) = ctx.args().first().cloned().filter(|f| f.is_callable()) else {
            return Ok(state_to_koto(&this));
        };
        let span = span_to_koto(&this.span);
        let narrowed = ctx
            .vm
            .spawn_shared_vm()
            .call_function(f, CallArgs::Single(span))?;
        let span = span_from_koto(&narrowed).unwrap_or(this.span);
        Ok(state_to_koto(&this.set_span(span)))
    });
    map.add_fn("setSpan", move |ctx| {
        let span = ctx.args().first().and_then(span_from_koto);
        Ok(state_to_koto(&span.map_or_else(
            || for_set.clone(),
            |span| for_set.set_span(span),
        )))
    });
    KValue::Map(map)
}

/// `new Pattern(state => haps)`: a pattern whose query is a Koto function.
///
/// The function is called on whichever thread is querying, so it runs behind a
/// mutex; a call that fails yields no haps rather than unwinding through the
/// scheduler, which cannot report an error anyway.
pub(crate) fn pattern_from_koto_fn(func: KValue, vm: &KotoVm) -> Pattern {
    let vm = std::sync::Mutex::new(vm.spawn_shared_vm());
    Pattern::new(move |state| {
        let Ok(mut vm) = vm.lock() else {
            return Vec::new();
        };
        let arg = state_to_koto(state);
        let Ok(result) = vm.call_function(func.clone(), CallArgs::Single(arg)) else {
            return Vec::new();
        };
        match result {
            KValue::List(l) => l.data().iter().filter_map(hap_to_koto_hap).collect(),
            KValue::Tuple(t) => t.data().iter().filter_map(hap_to_koto_hap).collect(),
            _ => Vec::new(),
        }
    })
}

fn hap_to_koto_hap(value: &KValue) -> Option<Hap> {
    hap_from_koto(value)
}

/// `pat.query(state)`: the haps this pattern has in that state's span.
pub(super) fn kpattern_query(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let state = ctx
        .args
        .first()
        .and_then(|arg| match arg {
            KValue::Map(map) => map.get("span").as_ref().and_then(span_from_koto),
            _ => None,
        })
        .map_or_else(
            || State::new(TimeSpan::new(Frac::zero(), Frac::one())),
            State::new,
        );
    let haps: Vec<KValue> = pat.query(&state).iter().map(hap_to_koto).collect();
    Ok(KValue::List(KList::from_slice(&haps)))
}

/// `pat.splitQueries()`: ask one cycle at a time, so a query function that
/// reasons about "this cycle" only ever sees one.
pub(super) fn kpattern_split_queries(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    Ok(KPattern(ctx.instance()?.0.split_queries()).into())
}

/// `pat.sortHapsByPart()`: the same haps, in a stable order by their part, so
/// numbering them is reproducible.
pub(super) fn kpattern_sort_haps_by_part(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let steps = pat.steps;
    Ok(KPattern(
        Pattern::new(move |state| {
            let mut haps = pat.query(state);
            haps.sort_by(|a, b| {
                a.part
                    .begin
                    .cmp(&b.part.begin)
                    .then(a.part.end.cmp(&b.part.end))
            });
            haps
        })
        .set_steps(steps),
    )
    .into())
}

/// `pat.compressSpan(span)` / `focusSpan(span)` / `zoomArc(arc)`: the span-object
/// forms of `compress`/`focus`/`zoom`.
///
/// Upstream these are the variants the two-argument versions delegate to, and
/// they throw on anything but a `TimeSpan` — which is why they were unreachable
/// until `TimeSpan` became something a script could hold.
/// What a span form does once it has its span.
type SpanOp = fn(&Pattern, TimeSpan) -> Pattern;

fn compress_span(pat: &Pattern, span: TimeSpan) -> Pattern {
    pat.compress(span.begin, span.end)
}

fn focus_span(pat: &Pattern, span: TimeSpan) -> Pattern {
    pat._focus_span(span)
}

fn zoom_arc(pat: &Pattern, span: TimeSpan) -> Pattern {
    pat.zoom(span.begin, span.end)
}

/// Silence stands in for the throw upstream does on a non-span argument: the
/// caller may be whichever thread is querying, which has nowhere to report it.
fn span_method(ctx: MethodContext<KPattern>, f: SpanOp) -> KotoResult<KValue> {
    let pat = ctx.instance()?.0.clone();
    let span = ctx.args.first().and_then(span_from_koto);
    Ok(KPattern(span.map_or_else(rudel_core::silence, |span| f(&pat, span))).into())
}

pub(super) fn kpattern_compress_span(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    span_method(ctx, compress_span)
}

pub(super) fn kpattern_focus_span(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    span_method(ctx, focus_span)
}

pub(super) fn kpattern_zoom_arc(ctx: MethodContext<KPattern>) -> KotoResult<KValue> {
    span_method(ctx, zoom_arc)
}

/// Every spelling of the three, against what it does — the same functions the
/// methods above are built from, so the two forms cannot drift apart.
const SPAN_FNS: [(&[&str], SpanOp); 3] = [
    (
        &["compressSpan", "compress_span", "compressspan"],
        compress_span,
    ),
    (&["focusSpan", "focus_span", "focusspan"], focus_span),
    (&["zoomArc", "zoom_arc", "zoomarc"], zoom_arc),
];

/// The standalone forms: `compressSpan(span, pat)` reads the same as
/// `pat.compressSpan(span)`, as it does for every transform Strudel `register`s
/// — each one exports a top-level function taking the pattern last as well as a
/// method. A call short of the pattern partially applies, like its neighbours.
pub(crate) fn register_span_fns(prelude: &KMap) {
    for (names, op) in SPAN_FNS {
        for name in names {
            crate::bindings::prelude::add_curried_fn(prelude, name, 2, move |a: &[KValue]| {
                let last = a.len().saturating_sub(1);
                let pat = super::arg_to_pattern(a.get(last).unwrap_or(&KValue::Null));
                let span = a.first().filter(|_| last >= 1).and_then(span_from_koto);
                Ok(KPattern(span.map_or_else(rudel_core::silence, |span| op(&pat, span))).into())
            });
        }
    }
}

/// The engine constructors a script reaches for: `Pattern`, `Hap`, `Fraction`.
pub(crate) fn register_engine_fns(prelude: &KMap) {
    prelude.add_fn("Pattern", |ctx| {
        let Some(func) = ctx.args().first().cloned().filter(|f| f.is_callable()) else {
            return Ok(KPattern(rudel_core::silence()).into());
        };
        Ok(KPattern(pattern_from_koto_fn(func, ctx.vm)).into())
    });
    prelude.add_fn("Hap", |ctx| {
        let args = ctx.args();
        let whole = args.first().and_then(span_from_koto);
        let part = args.get(1).and_then(span_from_koto);
        // A hap with no part covers nothing; Strudel's own code then filters it
        // out by testing `hap.part`, so hand back the null it is testing for.
        let Some(part) = part else {
            return Ok(KValue::Null);
        };
        let value = args.get(2).map_or(rudel_core::Value::Null, koto_to_value);
        Ok(hap_to_koto(&Hap::new(whole, part, value)))
    });
    prelude.add_fn("Fraction", |ctx| {
        Ok(KFrac(to_frac(ctx.args().first().unwrap_or(&KValue::Null))).into())
    });
    prelude.add_fn("TimeSpan", |ctx| {
        let args = ctx.args();
        Ok(span_to_koto(&TimeSpan::new(
            to_frac(args.first().unwrap_or(&KValue::Null)),
            to_frac(args.get(1).unwrap_or(&KValue::Null)),
        )))
    });
}
