// pick - select patterns from a list or lookup table via a selector pattern.
//
// Ports strudel's core/pick.mjs `_pick`: the selector's values index into the
// lookup, and the picked patterns are flattened with one of the joins
// (`pick` = inner, `pickOut` = outer, `inhabit` = squeeze, `pickReset` /
// `pickRestart` = retriggering). List lookups either clamp the index
// (`pick`) or wrap it (`pickmod`); name lookups ignore the modulo flag,
// matching strudel.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    pattern::{Pattern, silence},
    value::Value,
};
use std::collections::HashMap;

/// Which join flattens the picked pattern-of-patterns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickJoin {
    Inner,
    Outer,
    Squeeze,
    Reset,
    Restart,
}

fn apply_join(pat: Pattern, join: PickJoin) -> Pattern {
    match join {
        PickJoin::Inner => pat.inner_join(),
        PickJoin::Outer => pat.outer_join(),
        PickJoin::Squeeze => pat.squeeze_join(),
        PickJoin::Reset => pat.reset_join(),
        PickJoin::Restart => pat.restart_join(),
    }
}

/// Pick from a list by (rounded) numeric index. `modulo` wraps out-of-range
/// indices (`pickmod`); otherwise they clamp to the ends (`pick`).
pub fn pick_list(items: &[Pattern], selector: &Pattern, modulo: bool, join: PickJoin) -> Pattern {
    if items.is_empty() {
        return silence();
    }
    let items = items.to_vec();
    let picked = selector.fmap(move |v| {
        let raw = v.as_f64().unwrap_or(0.0).round() as i64;
        let idx = if modulo {
            raw.rem_euclid(items.len() as i64)
        } else {
            raw.clamp(0, items.len() as i64 - 1)
        } as usize;
        Value::Pat(Box::new(items[idx].clone()))
    });
    apply_join(picked, join)
}

/// Pick from a name -> pattern table. Selector values are used as keys
/// (numbers are formatted as integers); missing keys produce silence.
pub fn pick_map(items: &HashMap<String, Pattern>, selector: &Pattern, join: PickJoin) -> Pattern {
    if items.is_empty() {
        return silence();
    }
    let items = items.clone();
    let picked = selector
        .fmap(move |v| {
            let key = match v {
                Value::Str(s) => s,
                Value::Int(n) => n.to_string(),
                Value::F64(x) => format!("{x:.0}"),
                _ => String::new(),
            };
            items
                .get(&key)
                .cloned()
                .map(|p| Value::Pat(Box::new(p)))
                .unwrap_or(Value::Null)
        })
        .filter_values(|v| !matches!(v, Value::Null));
    apply_join(picked, join)
}
