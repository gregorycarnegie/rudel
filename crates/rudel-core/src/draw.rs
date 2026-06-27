// draw.rs - draw-param pattern transforms.
// Ported from strudel/packages/draw/animate.mjs (`rescale`, `moveXY`, `zoomIn`).
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// These set the `x`/`y`/`w`/`h` visual params that Strudel's `animate` runtime
// consumes per frame. Rudel has no `animate` painter — the realtime/draw path
// never runs the Koto VM (docs/UNSUPPORTED.md) — so these produce no visual on
// their own. They are provided for hap/API parity: each transform evaluates and
// emits the same control maps as Strudel, exactly as the `register`-wrapped
// originals do, so `.rescale(2)` / `.moveXY(0.1, 0.1)` / `.zoomIn(0.5)` are
// chainable and queryable like any other transform.

use crate::{
    hap::Context,
    pattern::{Pattern, pure},
    transforms::IntoPattern,
    value::{Value, ValueMap},
};
use std::sync::Arc;

/// A pure pattern of `{ key: value, ... }` for the given draw params.
fn param_map(pairs: &[(&str, &Value)]) -> Pattern {
    let mut m = ValueMap::new();
    for (key, value) in pairs {
        m.insert((*key).to_string(), (*value).clone());
    }
    pure(Value::Map(m))
}

/// `pat.mul(x(f).w(f).y(f).h(f))` for a single sampled scale value.
fn rescale_one(pat: &Pattern, f: &Value) -> Pattern {
    pat.mul(param_map(&[("x", f), ("w", f), ("y", f), ("h", f)]))
}

/// `pat.add(x(dx).y(dy))` for a single sampled `(dx, dy)`.
fn move_one(pat: &Pattern, dx: &Value, dy: &Value) -> Pattern {
    pat.add(param_map(&[("x", dx), ("y", dy)]))
}

/// `pat.rescale(f).move(d, d)` with `d = (1 - f) / 2`, for one sampled `f`.
fn zoom_one(pat: &Pattern, f: &Value) -> Pattern {
    let d = Value::F64((1.0 - f.as_f64().unwrap_or(0.0)) / 2.0);
    move_one(&rescale_one(pat, f), &d, &d)
}

fn push_loc(result: Pattern, loc: Option<(usize, usize)>) -> Pattern {
    let Some((start, end)) = loc else {
        return result;
    };
    result.with_context(move |context: &Context| {
        let mut context = context.clone();
        context.locations.push((start, end));
        context
    })
}

/// Patternify a single value argument the way Strudel's `register` does for an
/// arity-2 transform: pure args bypass (keeping their source location), patterned
/// args map to the per-value result and `innerJoin`.
fn patternify_value<F>(pat: &Pattern, arg: Pattern, f: F) -> Pattern
where
    F: Fn(&Pattern, &Value) -> Pattern + Send + Sync + 'static,
{
    if let Some(v) = &arg.pure_value {
        return push_loc(f(pat, v), arg.pure_loc);
    }
    let pat = pat.clone();
    let f = Arc::new(f);
    arg.fmap(move |v| Value::Pat(Box::new(f(&pat, &v))))
        .inner_join()
}

/// Patternify two value arguments the way Strudel's `register` does for an
/// arity-3 transform: `a` is the structural outer (`fmap`), `b` is sampled by
/// `appLeft`, then `innerJoin`. Both-pure bypasses to a direct call.
fn patternify_value2<F>(pat: &Pattern, a: Pattern, b: Pattern, f: F) -> Pattern
where
    F: Fn(&Pattern, &Value, &Value) -> Pattern + Send + Sync + 'static,
{
    if let (Some(av), Some(bv)) = (&a.pure_value, &b.pure_value) {
        let loc = a.pure_loc.or(b.pure_loc);
        return push_loc(f(pat, av, bv), loc);
    }
    let pat = pat.clone();
    let f = Arc::new(f);
    a.fmap(move |av| {
        let pat = pat.clone();
        let f = f.clone();
        Value::func(move |bv| Value::Pat(Box::new(f(&pat, &av, &bv))))
    })
    .app_left(&b)
    .inner_join()
}

impl Pattern {
    /// `rescale`: scale the `x`/`w`/`y`/`h` draw params by `f`
    /// (`pat.mul(x(f).w(f).y(f).h(f))`).
    pub fn rescale(&self, f: impl IntoPattern) -> Pattern {
        patternify_value(self, f.into_pattern(), rescale_one)
    }

    /// `moveXY`: shift the `x`/`y` draw params by `dx`/`dy`
    /// (`pat.add(x(dx).y(dy))`).
    pub fn move_xy(&self, dx: impl IntoPattern, dy: impl IntoPattern) -> Pattern {
        patternify_value2(self, dx.into_pattern(), dy.into_pattern(), move_one)
    }

    /// `zoomIn`: zoom toward the center by `f` (`rescale(f)` then recenter by
    /// `(1 - f) / 2` on both axes).
    pub fn zoom_in(&self, f: impl IntoPattern) -> Pattern {
        patternify_value(self, f.into_pattern(), zoom_one)
    }
}

#[cfg(test)]
mod tests {
    use crate::{pattern::pure, value::Value};

    fn map_of(pat: &crate::Pattern) -> std::collections::BTreeMap<String, f64> {
        let haps = pat.query_arc(crate::Frac::zero(), crate::Frac::one());
        let hap = haps.first().expect("one hap");
        let Value::Map(m) = &hap.value else {
            panic!("expected map, got {:?}", hap.value);
        };
        m.iter()
            .map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(f64::NAN)))
            .collect()
    }

    #[test]
    fn rescale_multiplies_xywh() {
        // A pattern already carrying x/y/w/h gets each scaled by f.
        let base = pure(Value::Map(crate::value::ValueMap::from([
            ("x".to_string(), Value::F64(0.4)),
            ("y".to_string(), Value::F64(0.5)),
            ("w".to_string(), Value::F64(1.0)),
            ("h".to_string(), Value::F64(1.0)),
        ])));
        let m = map_of(&base.rescale(0.5));
        assert_eq!(m["x"], 0.2);
        assert_eq!(m["y"], 0.25);
        assert_eq!(m["w"], 0.5);
        assert_eq!(m["h"], 0.5);
    }

    #[test]
    fn move_xy_adds_offsets() {
        let base = pure(Value::Map(crate::value::ValueMap::from([
            ("x".to_string(), Value::F64(0.1)),
            ("y".to_string(), Value::F64(0.2)),
        ])));
        let m = map_of(&base.move_xy(0.25, 0.5));
        assert!((m["x"] - 0.35).abs() < 1e-9);
        assert!((m["y"] - 0.7).abs() < 1e-9);
    }

    #[test]
    fn zoom_in_rescales_and_recenters() {
        // zoomIn(0.5): rescale by 0.5 then add d=(1-0.5)/2=0.25 to x and y.
        let base = pure(Value::Map(crate::value::ValueMap::from([
            ("x".to_string(), Value::F64(1.0)),
            ("y".to_string(), Value::F64(1.0)),
            ("w".to_string(), Value::F64(1.0)),
            ("h".to_string(), Value::F64(1.0)),
        ])));
        let m = map_of(&base.zoom_in(0.5));
        // x: 1*0.5 + 0.25 = 0.75 ; w: 1*0.5 = 0.5 (no recenter on w/h)
        assert!((m["x"] - 0.75).abs() < 1e-9);
        assert!((m["y"] - 0.75).abs() < 1e-9);
        assert!((m["w"] - 0.5).abs() < 1e-9);
        assert!((m["h"] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn rescale_patterned_arg_samples_per_event() {
        // A patterned scale should innerJoin: first half scaled by 1, second by 2.
        let base = pure(Value::Map(crate::value::ValueMap::from([(
            "x".to_string(),
            Value::F64(1.0),
        )])));
        let arg = crate::pattern::fastcat(&[pure(Value::F64(1.0)), pure(Value::F64(2.0))]);
        let scaled = base.rescale(arg);
        let haps = scaled.query_arc(crate::Frac::zero(), crate::Frac::one());
        assert_eq!(haps.len(), 2);
    }
}
