//! The engine's own vocabulary, exposed to scripts: query state, time spans,
//! haps, exact fractions, and a `Pattern` built from a script function.
//!
//! This is the surface a script needs to define a pattern *combinator* rather
//! than just use one — Strudel scripts do it by patching `Pattern.prototype`
//! and returning `new Pattern(state => …)`, which is how `enumerate`, `warp`
//! and friends get written before they land upstream.
//!
//! The query function a script writes is called *from the query*, on whichever
//! thread is asking, and the engine that runs it lives on one thread — so the
//! call is handed over to that thread (see `js::SendFn`). Everything else in
//! the bindings still runs the script at construction time; this is the
//! deliberate exception, and the reason it is worth having is that no amount
//! of eager probing can express "look at the haps of this cycle and number
//! them".
//!
//! Spans, haps and states are plain objects, so `hap.part.begin` is ordinary
//! property access. Fractions are an object of their own, because they carry
//! exact rational arithmetic that a float would quietly lose.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    args::{arg, method},
    convert::{arg_to_pattern, to_value, value_to_arg},
};
use crate::js::{self, Arg, NULL, Scope};
use rudel_core::{Frac, Hap, Pattern, State, TimeSpan};

/// A fraction from whatever a script passed: another fraction, or a number.
pub(super) fn to_frac(value: &Arg) -> Frac {
    match value {
        Arg::Frac(f) => *f,
        Arg::Num(n) => Frac::from_f64(*n),
        _ => Frac::zero(),
    }
}

/// The methods of a `Fraction`.
fn register_fraction_methods(proto: &Scope) {
    fn on(proto: &Scope, name: &str, f: impl Fn(Frac, &[Arg]) -> Arg + Send + Sync + 'static) {
        proto.method(name, move |this, args| match this {
            Arg::Frac(frac) => Ok(f(*frac, args)),
            _ => Err("not called on a Fraction".to_string()),
        });
    }
    on(proto, "add", |t, a| (t + to_frac(arg(a, 0))).into());
    on(proto, "sub", |t, a| (t - to_frac(arg(a, 0))).into());
    on(proto, "mul", |t, a| (t * to_frac(arg(a, 0))).into());
    on(proto, "div", |t, a| {
        let by = to_frac(arg(a, 0));
        if by == Frac::zero() {
            return Frac::zero().into();
        }
        (t / by).into()
    });
    // The cycle this instant falls in, as a span (`wholeCycle`).
    on(proto, "wholeCycle", |t, _| {
        span_to_arg(&TimeSpan::new(t.sam(), t.next_sam()))
    });
    on(proto, "sam", |t, _| t.sam().into());
    on(proto, "floor", |t, _| t.floor().into());
    on(proto, "ceil", |t, _| t.ceil().into());
    // The float form, for arithmetic that does not need to stay exact — and
    // what `<`, `+` and friends read, since `valueOf` is how JavaScript asks.
    on(proto, "toNumber", |t, _| t.to_f64().into());
    on(proto, "valueOf", |t, _| t.to_f64().into());
    on(proto, "toString", |t, _| {
        format!("{}/{}", t.numer(), t.denom()).into()
    });
}

/// A time span as an object, so `span.begin` is ordinary property access.
pub(super) fn span_to_arg(span: &TimeSpan) -> Arg {
    Arg::Span(*span)
}

/// A span, or any object with a `begin` and an `end`.
pub(super) fn span_from_arg(value: &Arg) -> Option<TimeSpan> {
    if let Arg::Span(span) = value {
        return Some(*span);
    }
    Some(TimeSpan::new(
        to_frac(value.get("begin")?),
        to_frac(value.get("end")?),
    ))
}

/// The methods of a `TimeSpan`. On the prototype rather than on each span,
/// because a query hands a script a pair of them for every hap.
fn register_span_methods(proto: &Scope) {
    fn on(proto: &Scope, name: &str, f: impl Fn(TimeSpan, &[Arg]) -> Arg + Send + Sync + 'static) {
        proto.method(name, move |this, args| match this {
            Arg::Span(span) => Ok(f(*span, args)),
            _ => Err("not called on a TimeSpan".to_string()),
        });
    }
    on(proto, "duration", |t, _| t.duration().into());
    on(proto, "cycleArc", |t, _| span_to_arg(&t.cycle_arc()));
    // Empty overlap is `undefined` in Strudel, which scripts test for.
    on(proto, "intersection", |t, a| {
        a.first()
            .and_then(span_from_arg)
            .and_then(|other| t.intersection(&other))
            .map_or(Arg::Null, |s| span_to_arg(&s))
    });
}

/// A hap as an object: `whole` (or `undefined` for an analog hap), `part`,
/// `value`.
pub(super) fn hap_to_arg(hap: &Hap) -> Arg {
    Arg::Map(vec![
        (
            "whole".into(),
            hap.whole.as_ref().map_or(Arg::Null, span_to_arg),
        ),
        ("part".into(), span_to_arg(&hap.part)),
        ("value".into(), value_to_arg(hap.value.clone())),
    ])
}

pub(super) fn hap_from_arg(value: &Arg) -> Option<Hap> {
    let part = span_from_arg(value.get("part")?)?;
    let whole = value.get("whole").and_then(span_from_arg);
    Some(Hap::new(
        whole,
        part,
        value.get("value").map_or(rudel_core::Value::Null, to_value),
    ))
}

/// A query state as an object, with the `withSpan` a combinator narrows it by.
fn state_to_arg(state: &State) -> Arg {
    Arg::State(state.clone())
}

/// The state a script handed back: one it was given, or any object with a
/// `span`.
fn state_from_arg(value: &Arg) -> Option<State> {
    match value {
        Arg::State(state) => Some(state.clone()),
        other => other.get("span").and_then(span_from_arg).map(State::new),
    }
}

/// The methods of a query state.
fn register_state_methods(proto: &Scope) {
    proto.method("withSpan", |this, a| {
        let Arg::State(state) = this else {
            return Err("withSpan: not called on a query state".to_string());
        };
        let Some(f) = a.first().filter(|f| f.is_callable()) else {
            return Ok(state_to_arg(state));
        };
        let narrowed = js::call(f, vec![span_to_arg(&state.span)])?;
        let span = span_from_arg(&narrowed).unwrap_or(state.span);
        Ok(state_to_arg(&state.set_span(span)))
    });
    proto.method("setSpan", |this, a| {
        let Arg::State(state) = this else {
            return Err("setSpan: not called on a query state".to_string());
        };
        Ok(state_to_arg(&match a.first().and_then(span_from_arg) {
            Some(span) => state.set_span(span),
            None => state.clone(),
        }))
    });
}

/// `new Pattern(state => haps)`: a pattern whose query is a script function.
///
/// The function is called on the JS thread, whichever thread is querying; a
/// call that fails yields no haps rather than unwinding through the scheduler,
/// which cannot report an error anyway.
pub(crate) fn pattern_from_fn(func: &Arg) -> Pattern {
    let Some(func) = js::keep(func) else {
        return rudel_core::silence();
    };
    Pattern::new(move |state| {
        let state = state.clone();
        func.run(move |f| match js::call(f, vec![state_to_arg(&state)]) {
            Ok(Arg::List(haps)) => haps.iter().filter_map(hap_from_arg).collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
    })
}

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

/// `pat.compressSpan(span)` / `focusSpan(span)` / `zoomArc(arc)`: the span-object
/// forms of `compress`/`focus`/`zoom`, in every spelling.
///
/// Upstream these are the variants the two-argument versions delegate to, and
/// they throw on anything but a `TimeSpan` — which is why they were unreachable
/// until `TimeSpan` became something a script could hold.
const SPAN_FNS: [(&[&str], SpanOp); 3] = [
    (
        &["compressSpan", "compress_span", "compressspan"],
        compress_span,
    ),
    (&["focusSpan", "focus_span", "focusspan"], focus_span),
    (&["zoomArc", "zoom_arc", "zoomarc"], zoom_arc),
];

/// The methods a script defining a combinator reaches for.
pub(super) fn register_engine_methods(proto: &Scope) {
    // `pat.query(state)`: the haps this pattern has in that state's span.
    // `pat.queryArc(begin, end)` / `pat.firstCycle()`: the haps over a span,
    // for a script that reads a pattern rather than transforming it.
    method(proto, "queryArc", |pat, a| {
        let haps = pat.query_arc(to_frac(arg(a, 0)), to_frac(arg(a, 1)));
        Ok(Arg::List(haps.iter().map(hap_to_arg).collect()))
    });
    method(proto, "firstCycle", |pat, _| {
        let haps = pat.query_arc(Frac::zero(), Frac::one());
        Ok(Arg::List(haps.iter().map(hap_to_arg).collect()))
    });
    method(proto, "query", |pat, a| {
        let state = state_from_arg(arg(a, 0))
            .unwrap_or_else(|| State::new(TimeSpan::new(Frac::zero(), Frac::one())));
        Ok(Arg::List(
            pat.query(&state).iter().map(hap_to_arg).collect(),
        ))
    });
    // `pat.splitQueries()`: ask one cycle at a time, so a query function that
    // reasons about "this cycle" only ever sees one.
    for name in ["splitQueries", "split_queries"] {
        method(proto, name, |pat, _| Ok(pat.split_queries().into()));
    }
    // `pat.sortHapsByPart()`: the same haps, in a stable order by their part,
    // so numbering them is reproducible.
    for name in ["sortHapsByPart", "sort_haps_by_part"] {
        method(proto, name, |pat, _| {
            let pat = pat.clone();
            let steps = pat.steps;
            Ok(Pattern::new(move |state| {
                let mut haps = pat.query(state);
                haps.sort_by(|a, b| {
                    a.part
                        .begin
                        .cmp(&b.part.begin)
                        .then(a.part.end.cmp(&b.part.end))
                });
                haps
            })
            .set_steps(steps)
            .into())
        });
    }
    // Silence stands in for the throw upstream does on a non-span argument:
    // the caller may be a query, which has nowhere to report it.
    for (names, op) in SPAN_FNS {
        for name in names {
            method(proto, name, move |pat, a| {
                let span = a.first().and_then(span_from_arg);
                Ok(span
                    .map_or_else(rudel_core::silence, |span| op(pat, span))
                    .into())
            });
        }
    }
}

/// The standalone forms: `compressSpan(span, pat)` reads the same as
/// `pat.compressSpan(span)`, as it does for every transform Strudel `register`s
/// — each one exports a top-level function taking the pattern last as well as a
/// method. A call short of the pattern partially applies, like its neighbours.
pub(crate) fn register_span_fns(prelude: &Scope) {
    for (names, op) in SPAN_FNS {
        for name in names {
            prelude.curried(name, 2, move |a| {
                let last = a.len().saturating_sub(1);
                let pat = arg_to_pattern(a.get(last).unwrap_or(NULL));
                let span = a.first().filter(|_| last >= 1).and_then(span_from_arg);
                Ok(span
                    .map_or_else(rudel_core::silence, |span| op(&pat, span))
                    .into())
            });
        }
    }
}

/// The engine constructors a script reaches for: `Pattern`, `Hap`, `Fraction`,
/// `TimeSpan`. Each works with or without `new`.
pub(crate) fn register_engine_fns(prelude: &Scope) {
    prelude.constructor("Pattern", |a| {
        Ok(match a.first().filter(|f| f.is_callable()) {
            Some(func) => pattern_from_fn(func).into(),
            None => rudel_core::silence().into(),
        })
    });
    // `Pattern.prototype.name = function …` and `x instanceof Pattern`.
    prelude.constructs("Pattern", &Scope::pattern());
    prelude.constructor("Hap", |a| {
        let whole = a.first().and_then(span_from_arg);
        // A hap with no part covers nothing; Strudel's own code then filters it
        // out by testing `hap.part`, so hand back the nothing it is testing for.
        let Some(part) = a.get(1).and_then(span_from_arg) else {
            return Ok(Arg::Null);
        };
        let value = a.get(2).map_or(rudel_core::Value::Null, to_value);
        Ok(hap_to_arg(&Hap::new(whole, part, value)))
    });
    prelude.constructor("Fraction", |a| Ok(to_frac(arg(a, 0)).into()));
    register_fraction_methods(&Scope::fraction());
    prelude.constructs("Fraction", &Scope::fraction());
    prelude.constructor("TimeSpan", |a| {
        Ok(span_to_arg(&TimeSpan::new(
            to_frac(arg(a, 0)),
            to_frac(arg(a, 1)),
        )))
    });
    register_span_methods(&Scope::span());
    register_state_methods(&Scope::state());
    prelude.constructs("TimeSpan", &Scope::span());
}
