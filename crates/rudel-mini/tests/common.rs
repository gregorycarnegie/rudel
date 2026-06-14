// Shared helpers for the parity-oracle integration tests. The golden JSON is
// produced by tools/oracle/*.mjs from Strudel's real engine.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::{Frac, Pattern, Value};

/// Canonical numeric string, collapsing Int/F64/Frac so Strudel's `0` and
/// rudel's `Int(0)` compare equal.
pub fn num_tag(x: f64) -> String {
    if x.is_finite() && x.fract() == 0.0 {
        format!("n:{}", x as i64)
    } else {
        format!("n:{x}")
    }
}

/// Canonical form of a rudel value.
pub fn canon_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => format!("b:{b}"),
        Value::Int(n) => num_tag(*n as f64),
        Value::F64(x) => num_tag(*x),
        Value::Frac(f) => num_tag(f.to_f64()),
        Value::Str(s) => format!("s:{s}"),
        Value::List(items) => {
            let inner: Vec<String> = items.iter().map(canon_value).collect();
            format!("[{}]", inner.join(","))
        }
        Value::Map(m) => {
            let mut parts: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{k}={}", canon_value(v)))
                .collect();
            parts.sort();
            format!("{{{}}}", parts.join(","))
        }
        other => format!("?{other:?}"),
    }
}

/// Canonical form of a golden JSON value (must match [`canon_value`]).
pub fn canon_json(v: &serde_json::Value) -> String {
    use serde_json::Value as J;
    match v {
        J::Null => "null".to_string(),
        J::Bool(b) => format!("b:{b}"),
        J::Number(n) => num_tag(n.as_f64().unwrap()),
        J::String(s) => format!("s:{s}"),
        J::Array(items) => {
            let inner: Vec<String> = items.iter().map(canon_json).collect();
            format!("[{}]", inner.join(","))
        }
        J::Object(m) => {
            let mut parts: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{k}={}", canon_json(v)))
                .collect();
            parts.sort();
            format!("{{{}}}", parts.join(","))
        }
    }
}

fn frac_str(f: Frac) -> String {
    format!("{}/{}", f.numer(), f.denom())
}

/// Canonical "a..b,c..d" form of sorted source locations.
pub fn canon_locs(mut locs: Vec<(usize, usize)>) -> String {
    locs.sort_unstable();
    locs.iter()
        .map(|(a, b)| format!("{a}..{b}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Sorted "pb|pe|wb|we|value|locs" lines for a rudel pattern over cycles
/// `0..cycles`. `with_locs` is false for goldens predating location dumps.
pub fn rudel_rows(pat: &Pattern, cycles: i64, with_locs: bool) -> Vec<String> {
    let mut rows: Vec<String> = pat
        .query_arc(Frac::zero(), Frac::int(cycles))
        .into_iter()
        .map(|h| {
            let (wb, we) = match h.whole {
                Some(w) => (frac_str(w.begin), frac_str(w.end)),
                None => ("_".to_string(), "_".to_string()),
            };
            let locs = if with_locs {
                format!("|{}", canon_locs(h.context.locations.clone()))
            } else {
                String::new()
            };
            format!(
                "{}|{}|{}|{}|{}{}",
                frac_str(h.part.begin),
                frac_str(h.part.end),
                wb,
                we,
                canon_value(&h.value),
                locs
            )
        })
        .collect();
    rows.sort();
    rows
}

/// Parse a golden `[[a,b],...]` location list.
pub fn golden_locs(v: &serde_json::Value) -> Vec<(usize, usize)> {
    v.as_array()
        .map(|pairs| {
            pairs
                .iter()
                .map(|p| {
                    (
                        p[0].as_u64().unwrap_or(0) as usize,
                        p[1].as_u64().unwrap_or(0) as usize,
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The matching sorted lines from a golden JSON hap array.
pub fn golden_rows(rows: &[serde_json::Value], with_locs: bool) -> Vec<String> {
    let s = |r: &serde_json::Value, k: &str| match &r[k] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "_".to_string(),
        other => other.to_string(),
    };
    let mut out: Vec<String> = rows
        .iter()
        .map(|r| {
            let locs = if with_locs {
                format!("|{}", canon_locs(golden_locs(&r["l"])))
            } else {
                String::new()
            };
            format!(
                "{}|{}|{}|{}|{}{}",
                s(r, "pb"),
                s(r, "pe"),
                s(r, "wb"),
                s(r, "we"),
                canon_json(&r["v"]),
                locs
            )
        })
        .collect();
    out.sort();
    out
}
