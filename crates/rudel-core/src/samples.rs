// samples.rs - sample-manipulation transforms (chop, striate, slice, splice,
// loopAt, fit). Ported from strudel/packages/core/pattern.mjs. These work by
// rewriting the `begin`/`end`/`speed`/`unit` control keys of map values; the
// audio layer (rudel-dsp `SamplerVoice`) interprets them.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::Frac;
use crate::pattern::{Pattern, fastcat, pure, slowcat};
use crate::state::State;
use crate::transforms::IntoPattern;
use crate::value::Value;
use std::collections::BTreeMap;

/// Strudel's default cycles-per-second, used when the query state carries no
/// `_cps` control (the cps-dependent transforms read it from there).
const DEFAULT_CPS: f64 = 0.5;

/// Read `_cps` from a query state's controls, defaulting to [`DEFAULT_CPS`].
fn cps_of(state: &State) -> f64 {
    state
        .controls
        .get("_cps")
        .and_then(|v| v.as_f64())
        .unwrap_or(DEFAULT_CPS)
}

/// Coerce a hap value into a control map: maps pass through, anything else
/// becomes `{ s: value }` (mirrors Strudel's `o instanceof Object ? o : {s:o}`).
fn as_control_map(v: &Value) -> BTreeMap<String, Value> {
    match v {
        Value::Map(m) => m.clone(),
        other => BTreeMap::from([("s".to_string(), other.clone())]),
    }
}

fn map_f64(m: &BTreeMap<String, Value>, key: &str) -> Option<f64> {
    m.get(key).and_then(|v| v.as_f64())
}

impl Pattern {
    /// Play this pattern at `cpm` cycles per minute regardless of the global
    /// tempo (`cpm`): fast-es by `cpm / 60 / cps`, reading the live `_cps` from
    /// the query state (Strudel reads `scheduler.cps`). A non-positive cps
    /// leaves the pattern unchanged.
    pub fn cpm(&self, cpm: f64) -> Pattern {
        let pat = self.clone();
        Pattern::new(move |state| {
            let cps = cps_of(state);
            if cps <= 0.0 {
                return pat.query(state);
            }
            pat._fast(Frac::from_f64(cpm / 60.0 / cps)).query(state)
        })
    }

    /// Mul-or-keep helper for step counts (`Fraction.mulmaybe`).
    fn steps_times(&self, n: i64) -> Option<Frac> {
        self.steps.map(|s| s * Frac::int(n))
    }

    /// Cut each sample into `n` equal pieces, played in order across the event
    /// (`chop`). Granular-synthesis building block.
    pub fn chop(&self, n: i64) -> Pattern {
        if n <= 0 {
            return self.clone();
        }
        let steps = self.steps_times(n);
        self.squeeze_bind(move |o| {
            // Scale the slice into any existing begin/end sub-range.
            let (base_b, base_e) = match &o {
                Value::Map(m) => (map_f64(m, "begin"), map_f64(m, "end")),
                _ => (None, None),
            };
            let base = as_control_map(&o);
            let slices: Vec<Pattern> = (0..n)
                .map(|i| {
                    let sb = i as f64 / n as f64;
                    let se = (i + 1) as f64 / n as f64;
                    let (b, e) = match (base_b, base_e) {
                        (Some(ab), Some(ae)) => {
                            let d = ae - ab;
                            (ab + sb * d, ab + se * d)
                        }
                        _ => (sb, se),
                    };
                    let mut m = base.clone();
                    m.insert("begin".to_string(), Value::F64(b));
                    m.insert("end".to_string(), Value::F64(e));
                    pure(Value::Map(m))
                })
                .collect();
            Value::Pat(Box::new(fastcat(&slices)))
        })
        .set_steps(steps)
    }

    /// Cut each sample into `n` parts, but interleave progressive portions of
    /// each across the cycle (`striate`).
    pub fn striate(&self, n: i64) -> Pattern {
        if n <= 0 {
            return self.clone();
        }
        let slices: Vec<Pattern> = (0..n)
            .map(|i| {
                let mut m = BTreeMap::new();
                m.insert("begin".to_string(), Value::F64(i as f64 / n as f64));
                m.insert("end".to_string(), Value::F64((i + 1) as f64 / n as f64));
                pure(Value::Map(m))
            })
            .collect();
        let slice_pat = slowcat(&slices);
        self.set(slice_pat)
            ._fast(Frac::int(n))
            .set_steps(self.steps_times(n))
    }

    /// Slice the sample into `n` pieces and trigger them by a pattern of indices
    /// (`slice`). `n` may instead be a list of split points in `0..1`.
    pub fn slice(&self, npat: impl IntoPattern, ipat: impl IntoPattern) -> Pattern {
        let opat = self.clone();
        let ipat = ipat.into_pattern();
        let steps = ipat.steps;
        npat.into_pattern()
            .inner_bind(move |nval| {
                let opat = opat.clone();
                let nval = nval.clone();
                ipat.outer_bind(move |ival| {
                    let nval = nval.clone();
                    opat.outer_bind(move |oval| pure(slice_value(&nval, &ival, &oval)))
                })
            })
            .set_steps(steps)
    }

    /// Like [`slice`](Self::slice), but also sets `speed`/`unit` so each slice
    /// is time-stretched to fill its step (`splice`).
    pub fn splice(&self, npat: impl IntoPattern, ipat: impl IntoPattern) -> Pattern {
        let ipat = ipat.into_pattern();
        let steps = ipat.steps;
        let sliced = self.slice(npat, ipat);
        sliced
            .with_haps(|haps, state| {
                let cps = cps_of(state);
                haps.into_iter()
                    .map(|hap| {
                        let dur = hap.duration().to_f64();
                        hap.with_value(|v| {
                            let mut m = as_control_map(&v);
                            let slices = map_f64(&m, "_slices").unwrap_or(1.0);
                            let prev_speed = map_f64(&m, "speed").unwrap_or(1.0);
                            if dur > 0.0 && slices > 0.0 {
                                let speed = (cps / slices / dur) * prev_speed;
                                m.insert("speed".to_string(), Value::F64(speed));
                            }
                            m.entry("unit".to_string())
                                .or_insert_with(|| Value::Str("c".to_string()));
                            Value::Map(m)
                        })
                    })
                    .collect()
            })
            .set_steps(steps)
    }

    /// Stretch the sample to span `factor` cycles by adjusting `speed`/`unit`
    /// (`loopAt`).
    pub fn loop_at(&self, factor: impl Into<Frac>) -> Pattern {
        let factor = factor.into();
        let pat = self.clone();
        let steps = self.steps.map(|s| s / factor);
        Pattern::new(move |state| {
            let cps = cps_of(state);
            let f = factor.to_f64();
            let speed = if f != 0.0 { (1.0 / f) * cps } else { 0.0 };
            pat.speed(Value::F64(speed))
                .unit(Value::Str("c".to_string()))
                .slow(factor)
                .query(state)
        })
        .set_steps(steps)
    }

    /// Slice this pattern into `n` equal zoomed pieces and trigger them with a
    /// pattern of indices (`bite`). Like [`slice`](Self::slice), but it slices
    /// the *pattern* rather than the sample (it zooms instead of setting
    /// `begin`/`end`), squeezing each selected slice into its step.
    pub fn bite(&self, npat: impl IntoPattern, ipat: impl IntoPattern) -> Pattern {
        let pat = self.clone();
        let npat = npat.into_pattern();
        ipat.into_pattern()
            .fmap(move |ival| {
                let pat = pat.clone();
                let i = ival.to_frac();
                Value::func(move |nval| {
                    let n = nval.to_frac();
                    if n == Frac::zero() {
                        return Value::Pat(Box::new(pat.clone()));
                    }
                    let q = i / n;
                    let a = q - q.floor(); // `Fraction.mod(1)`: fractional part
                    let b = a + Frac::one() / n;
                    Value::Pat(Box::new(pat.zoom(a, b)))
                })
            })
            .app_left(&npat)
            .squeeze_join()
    }

    /// Like [`loop_at`](Self::loop_at) but with an explicit cps (`loopAtCps`,
    /// deprecated in Strudel in favour of `loopAt`/`fit` with `setCps`).
    pub fn loop_at_cps(&self, factor: impl Into<Frac>, cps: f64) -> Pattern {
        let factor = factor.into();
        let f = factor.to_f64();
        let speed = if f != 0.0 { (1.0 / f) * cps } else { 0.0 };
        self.speed(Value::F64(speed))
            .unit(Value::Str("c".to_string()))
            .slow(factor)
    }

    /// Stretch each sample to fill its own event duration (`fit`).
    pub fn fit(&self) -> Pattern {
        self.with_haps(|haps, state| {
            let cps = cps_of(state);
            haps.into_iter()
                .map(|hap| {
                    let dur = hap.duration().to_f64();
                    hap.with_value(|v| {
                        let mut m = as_control_map(&v);
                        let begin = map_f64(&m, "begin").unwrap_or(0.0);
                        let end = map_f64(&m, "end").unwrap_or(1.0);
                        let slicedur = end - begin;
                        if dur > 0.0 {
                            m.insert("speed".to_string(), Value::F64((cps / dur) * slicedur));
                        }
                        m.insert("unit".to_string(), Value::Str("c".to_string()));
                        Value::Map(m)
                    })
                })
                .collect()
        })
    }
}

/// Build the control map for one `slice` event: `{ begin, end, _slices, ...o }`.
fn slice_value(nval: &Value, ival: &Value, oval: &Value) -> Value {
    let i = ival.as_f64().unwrap_or(0.0);
    let (begin, end, slices) = match nval {
        Value::List(items) => {
            let idx = i.max(0.0) as usize;
            let b = items.get(idx).and_then(|v| v.as_f64()).unwrap_or(0.0);
            let e = items.get(idx + 1).and_then(|v| v.as_f64()).unwrap_or(1.0);
            (b, e, (items.len().saturating_sub(1)) as f64)
        }
        _ => {
            let n = nval.as_f64().unwrap_or(1.0);
            let n = if n == 0.0 { 1.0 } else { n };
            (i / n, (i + 1.0) / n, n)
        }
    };
    let mut m = BTreeMap::new();
    m.insert("begin".to_string(), Value::F64(begin));
    m.insert("end".to_string(), Value::F64(end));
    m.insert("_slices".to_string(), Value::F64(slices));
    // The sound's own keys win over the slice defaults.
    for (k, v) in as_control_map(oval) {
        m.insert(k, v);
    }
    Value::Map(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{State, TimeSpan, s, sequence, silence};

    fn maps(pat: &Pattern, b: i64, e: i64) -> Vec<BTreeMap<String, Value>> {
        let mut haps = pat.query_arc(Frac::int(b), Frac::int(e));
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter()
            .map(|h| match h.value {
                Value::Map(m) => m,
                other => BTreeMap::from([("v".to_string(), other)]),
            })
            .collect()
    }

    fn query_cps(pat: &Pattern, cps: f64) -> Vec<BTreeMap<String, Value>> {
        let controls = BTreeMap::from([("_cps".to_string(), Value::F64(cps))]);
        let state = State::with_controls(TimeSpan::new(Frac::zero(), Frac::one()), controls);
        let mut haps = pat.query(&state);
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter()
            .map(|h| match h.value {
                Value::Map(m) => m,
                other => BTreeMap::from([("v".to_string(), other)]),
            })
            .collect()
    }

    #[test]
    fn chop_splits_into_n_slices() {
        let pat = s("bd").chop(4);
        let ms = maps(&pat, 0, 1);
        assert_eq!(ms.len(), 4);
        assert_eq!(ms[0].get("begin"), Some(&Value::F64(0.0)));
        assert_eq!(ms[0].get("end"), Some(&Value::F64(0.25)));
        assert_eq!(ms[3].get("begin"), Some(&Value::F64(0.75)));
        assert_eq!(ms[3].get("end"), Some(&Value::F64(1.0)));
        // sample name preserved across slices
        assert_eq!(ms[0].get("s"), Some(&Value::Str("bd".to_string())));
    }

    #[test]
    fn chop_nests_into_existing_range() {
        // chop a sub-range [0.5, 1.0] into 2 -> [0.5,0.75], [0.75,1.0]
        let pat = s("bd").begin(0.5).end(1.0).chop(2);
        let ms = maps(&pat, 0, 1);
        assert_eq!(ms[0].get("begin"), Some(&Value::F64(0.5)));
        assert_eq!(ms[0].get("end"), Some(&Value::F64(0.75)));
        assert_eq!(ms[1].get("begin"), Some(&Value::F64(0.75)));
        assert_eq!(ms[1].get("end"), Some(&Value::F64(1.0)));
    }

    #[test]
    fn striate_interleaves_slices() {
        // two events, striate(2): each event plays a progressive slice
        let pat = sequence(&[s("bd"), s("sd")]).striate(2);
        let ms = maps(&pat, 0, 1);
        assert_eq!(ms.len(), 4);
        // first half of cycle uses begin 0, second half begin 0.5
        assert_eq!(ms[0].get("begin"), Some(&Value::F64(0.0)));
        assert_eq!(ms[2].get("begin"), Some(&Value::F64(0.5)));
    }

    #[test]
    fn slice_indexes_into_pieces() {
        // slice 4 pieces, play index 0 then 2
        let pat = s("bd").slice(4, sequence(&[pure(Value::Int(0)), pure(Value::Int(2))]));
        let ms = maps(&pat, 0, 1);
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].get("begin"), Some(&Value::F64(0.0)));
        assert_eq!(ms[0].get("end"), Some(&Value::F64(0.25)));
        assert_eq!(ms[1].get("begin"), Some(&Value::F64(0.5)));
        assert_eq!(ms[1].get("end"), Some(&Value::F64(0.75)));
        assert_eq!(ms[0].get("s"), Some(&Value::Str("bd".to_string())));
    }

    #[test]
    fn slice_accepts_split_point_list() {
        let pat = s("bd").slice(
            Value::List(vec![Value::F64(0.0), Value::F64(0.25), Value::F64(1.0)]),
            sequence(&[pure(Value::Int(0)), pure(Value::Int(1))]),
        );
        let ms = maps(&pat, 0, 1);
        assert_eq!(ms[0].get("begin"), Some(&Value::F64(0.0)));
        assert_eq!(ms[0].get("end"), Some(&Value::F64(0.25)));
        assert_eq!(ms[1].get("begin"), Some(&Value::F64(0.25)));
        assert_eq!(ms[1].get("end"), Some(&Value::F64(1.0)));
    }

    #[test]
    fn loop_at_sets_speed_and_unit() {
        // loopAt(2) with cps=0.5 -> speed = (1/2)*0.5 = 0.25, unit 'c'
        let pat = s("bd").loop_at(2);
        let ms = query_cps(&pat, 0.5);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].get("speed"), Some(&Value::F64(0.25)));
        assert_eq!(ms[0].get("unit"), Some(&Value::Str("c".to_string())));
    }

    #[test]
    fn fit_sets_speed_from_duration() {
        // single event spanning a full cycle, cps=1 -> speed = (1/1)*1 = 1
        let pat = s("bd").fit();
        let ms = query_cps(&pat, 1.0);
        assert_eq!(ms[0].get("speed"), Some(&Value::F64(1.0)));
        assert_eq!(ms[0].get("unit"), Some(&Value::Str("c".to_string())));
    }

    #[test]
    fn splice_sets_speed_from_slices() {
        // splice 2 pieces over 2 steps; each step is half a cycle (dur=0.5),
        // cps=1, slices=2 -> speed = (1 / 2 / 0.5) * 1 = 1
        let pat = s("bd").splice(2, sequence(&[pure(Value::Int(0)), pure(Value::Int(1))]));
        let ms = query_cps(&pat, 1.0);
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].get("speed"), Some(&Value::F64(1.0)));
        assert_eq!(ms[0].get("unit"), Some(&Value::Str("c".to_string())));
    }

    #[test]
    fn bite_zooms_pattern_slices_by_index() {
        // bite(4, "0 2") over `a b c d`: slice 0 is `a`, slice 2 is `c`, each
        // squeezed into its half-cycle step (matches Strudel hap-for-hap).
        let pat = sequence(&[s("a"), s("b"), s("c"), s("d")])
            .bite(4, sequence(&[pure(Value::Int(0)), pure(Value::Int(2))]));
        let ms = maps(&pat, 0, 1);
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].get("s"), Some(&Value::Str("a".to_string())));
        assert_eq!(ms[1].get("s"), Some(&Value::Str("c".to_string())));
    }

    #[test]
    fn bite_into_two_replays_index_ranges() {
        // bite(2, "1 0") over `0 1 2 3`: slice 1 = [2 3], slice 0 = [0 1].
        let pat = sequence(&[
            pure(Value::Int(0)),
            pure(Value::Int(1)),
            pure(Value::Int(2)),
            pure(Value::Int(3)),
        ])
        .bite(2, sequence(&[pure(Value::Int(1)), pure(Value::Int(0))]));
        let vals: Vec<i64> = maps(&pat, 0, 1)
            .iter()
            .filter_map(|m| m.get("v").and_then(|v| v.as_f64()).map(|f| f as i64))
            .collect();
        assert_eq!(vals, vec![2, 3, 0, 1]);
    }

    #[test]
    fn loop_at_cps_uses_explicit_cps() {
        // loopAtCps(2, 1.0): speed = (1/2)*1 = 0.5, unit 'c', slowed by 2.
        let pat = s("bd").loop_at_cps(2, 1.0);
        let ms = query_cps(&pat, 0.5);
        assert_eq!(ms[0].get("speed"), Some(&Value::F64(0.5)));
        assert_eq!(ms[0].get("unit"), Some(&Value::Str("c".to_string())));
    }

    #[test]
    fn chop_zero_is_noop() {
        let pat = s("bd").chop(0);
        assert_eq!(maps(&pat, 0, 1).len(), 1);
        let _ = silence();
    }
}
