// midimap.rs - control-name -> MIDI CC mapping tables.
// Ports midi.mjs's `midicontrolMap` registry: `defaultmidimap`/`midimaps`
// register named tables mapping a control (`lpf`, `room`, ...) to a CC number
// with an optional input range and curve, and a hap carrying `midimap('name')`
// has every mapped control it holds turned into a CC message alongside its note.
//
// The registry is process-global and long-lived, like Strudel's module-level
// `midicontrolMap`: the language layer writes it at eval time and the MIDI
// back-end reads it at schedule time, the same split the CC input bus uses
// (see `input.rs`).
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{controls::control_name, value::ValueMap};
use std::{
    collections::HashMap,
    sync::{LazyLock, RwLock},
};

/// How one control maps onto a CC: the controller number plus the input range
/// and exponent used to normalise the control's value into 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CcMapping {
    pub ccn: u8,
    pub min: f64,
    pub max: f64,
    pub exp: f64,
}

impl CcMapping {
    /// A bare number in a midimap is shorthand for `{ ccn }` over the unit range.
    pub fn new(ccn: u8) -> Self {
        Self {
            ccn,
            min: 0.0,
            max: 1.0,
            exp: 1.0,
        }
    }
}

/// The named tables, keyed by midimap name. `default` is the one a hap uses
/// when it sets no `midimap` control, matching `midiConfig.midimap`.
static MIDIMAPS: LazyLock<RwLock<HashMap<String, HashMap<String, CcMapping>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Register a midimap under `name`, replacing any table already there.
/// Keys are canonicalised through [`control_name`] (upstream's `unifyMapping`
/// calls `getControlName`), so a mapping written against an alias (`lpf`) and a
/// hap carrying the canonical name (`cutoff`) still meet.
pub fn set_midimap(name: &str, mapping: impl IntoIterator<Item = (String, CcMapping)>) {
    let unified = mapping
        .into_iter()
        .map(|(key, m)| (control_name(&key), m))
        .collect();
    MIDIMAPS.write().unwrap().insert(name.to_string(), unified);
}

/// Whether a midimap is registered under `name`.
pub fn has_midimap(name: &str) -> bool {
    MIDIMAPS.read().unwrap().contains_key(name)
}

/// Normalise `value` from `min..max` through `exp` into 0..1, porting
/// midi.mjs's `normalize`. An empty range is degenerate (upstream throws), so
/// it yields 0 rather than a NaN that would travel into a CC byte.
fn normalize(value: f64, min: f64, max: f64, exp: f64) -> f64 {
    if min == max {
        return 0.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0).powf(exp)
}

/// The `(ccn, ccv)` pairs a hap produces under the midimap `name`, with `ccv`
/// normalised to 0..1 — midi.mjs's `mapCC`. Unknown map names and controls the
/// map does not mention yield nothing. Pairs come out sorted by controller so
/// the emitted message order does not depend on hash iteration order.
pub fn midimap_ccs(name: &str, controls: &ValueMap) -> Vec<(u8, f64)> {
    let maps = MIDIMAPS.read().unwrap();
    let Some(mapping) = maps.get(name) else {
        return Vec::new();
    };
    let mut out: Vec<(u8, f64)> = controls
        .iter()
        .filter_map(|(key, value)| {
            let m = mapping.get(&control_name(key))?;
            let v = value.as_f64()?;
            Some((m.ccn, normalize(v, m.min, m.max, m.exp)))
        })
        .collect();
    out.sort_by_key(|&(ccn, _)| ccn);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    fn map_of(pairs: &[(&str, f64)]) -> ValueMap {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), Value::F64(*v)))
            .collect()
    }

    #[test]
    fn bare_number_maps_over_the_unit_range() {
        set_midimap("unit_range", [("gain".to_string(), CcMapping::new(7))]);
        assert!(has_midimap("unit_range"));
        assert_eq!(
            midimap_ccs("unit_range", &map_of(&[("gain", 0.5)])),
            [(7, 0.5)]
        );
        // A control the map does not mention is ignored, as is an unknown map.
        assert!(midimap_ccs("unit_range", &map_of(&[("pan", 0.5)])).is_empty());
        assert!(midimap_ccs("no_such_map", &map_of(&[("gain", 0.5)])).is_empty());
    }

    #[test]
    fn range_and_exponent_normalize_the_value() {
        let lpf = CcMapping {
            ccn: 74,
            min: 0.0,
            max: 20000.0,
            exp: 0.5,
        };
        set_midimap("ranged", [("lpf".to_string(), lpf)]);
        let ccv = |hz| midimap_ccs("ranged", &map_of(&[("cutoff", hz)]))[0].1;
        // 5000/20000 = 0.25, then ^0.5 = 0.5.
        assert_eq!(ccv(5000.0), 0.5);
        // Out-of-range values clamp rather than running off the end of the CC.
        assert_eq!(ccv(1e9), 1.0);
        assert_eq!(ccv(-1.0), 0.0);
    }

    #[test]
    fn aliases_canonicalize_on_both_sides() {
        // The map is written against the alias `lpf` while the hap carries the
        // canonical `cutoff` (or the other way round) — both must resolve.
        assert_eq!(control_name("lpf"), "cutoff");
        set_midimap("aliased", [("lpf".to_string(), CcMapping::new(74))]);
        assert_eq!(
            midimap_ccs("aliased", &map_of(&[("lpf", 1.0)])),
            [(74, 1.0)]
        );
        assert_eq!(
            midimap_ccs("aliased", &map_of(&[("cutoff", 1.0)])),
            [(74, 1.0)]
        );
    }

    #[test]
    fn registering_again_replaces_the_table() {
        set_midimap("replaced", [("gain".to_string(), CcMapping::new(7))]);
        set_midimap("replaced", [("pan".to_string(), CcMapping::new(10))]);
        assert!(midimap_ccs("replaced", &map_of(&[("gain", 1.0)])).is_empty());
        assert_eq!(
            midimap_ccs("replaced", &map_of(&[("pan", 1.0)])),
            [(10, 1.0)]
        );
    }
}
