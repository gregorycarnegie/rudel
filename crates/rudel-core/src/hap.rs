// hap.rs - ported from strudel/packages/core/hap.mjs
// Copyright (C) 2022 Strudel contributors; 2026 Rudel contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::Frac;
use crate::timespan::TimeSpan;
use crate::value::Value;

/// Source-location/context metadata carried by a hap. Kept minimal for now;
/// `locations` accumulates as haps combine (used later for editor highlighting).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    pub locations: Vec<(usize, usize)>,
    /// The scale tagged by `.scale(...)`, used by scale-aware transforms such as
    /// `scaleTranspose`.
    pub scale: Option<String>,
    /// The equal-division octave size tagged by `.xen("31edo")`, used as the
    /// default by frequency transposition (`ftrans`).
    pub edo_size: Option<f64>,
}

impl Context {
    pub fn combine(&self, other: &Context) -> Context {
        let mut locations = self.locations.clone();
        locations.extend(other.locations.iter().copied());
        // Keep whichever side carries a scale tag (the later/other one wins).
        let scale = other.scale.clone().or_else(|| self.scale.clone());
        let edo_size = other.edo_size.or(self.edo_size);
        Context {
            locations,
            scale,
            edo_size,
        }
    }
}

/// An event: a `value` active during the `part` span. `whole` is `None` for
/// continuous (analog) values; otherwise `part` ⊆ `whole`.
#[derive(Clone, Debug)]
pub struct Hap {
    pub whole: Option<TimeSpan>,
    pub part: TimeSpan,
    pub value: Value,
    pub context: Context,
}

impl Hap {
    pub fn new(whole: Option<TimeSpan>, part: TimeSpan, value: Value) -> Self {
        Hap {
            whole,
            part,
            value,
            context: Context::default(),
        }
    }

    pub fn with_context(mut self, context: Context) -> Self {
        self.context = context;
        self
    }

    pub fn duration(&self) -> Frac {
        match &self.whole {
            Some(w) => w.end - w.begin,
            // Continuous haps have no duration; fall back to part.
            None => self.part.duration(),
        }
    }

    /// The event's *sounding* duration, mirroring Strudel's `Hap.duration`
    /// getter ("event clipping"): a numeric `duration` control overrides the
    /// whole's length, and a numeric `clip` control (the canonical key behind
    /// `clip`/`legato`) multiplies it. This is what feeds the scheduler/synth
    /// to decide how long an event holds, as opposed to [`duration`](Self::duration),
    /// which is the structural whole length used by `splice`/`fit`.
    pub fn clipped_duration(&self) -> Frac {
        let mut duration =
            numeric_field(&self.value, "duration").unwrap_or_else(|| self.duration());
        if let Some(clip) = numeric_field(&self.value, "clip") {
            duration = duration * clip;
        }
        duration
    }

    /// The end of the event after clipping, `whole.begin + clipped_duration`
    /// (Strudel's `endClipped` getter). Continuous haps have no `whole`, so
    /// this falls back to the part end. The `isActive`/`isInPast`/`isInFuture`
    /// scheduler-timing predicates built on this in Strudel belong to the
    /// scheduler item, not the data model.
    pub fn end_clipped(&self) -> Frac {
        match &self.whole {
            Some(w) => w.begin + self.clipped_duration(),
            None => self.part.end,
        }
    }

    pub fn whole_or_part(&self) -> TimeSpan {
        self.whole.unwrap_or(self.part)
    }

    pub fn with_span(&self, f: impl Fn(TimeSpan) -> TimeSpan) -> Hap {
        Hap {
            whole: self.whole.map(&f),
            part: f(self.part),
            value: self.value.clone(),
            context: self.context.clone(),
        }
    }

    pub fn with_value(&self, f: impl Fn(Value) -> Value) -> Hap {
        Hap {
            whole: self.whole,
            part: self.part,
            value: f(self.value.clone()),
            context: self.context.clone(),
        }
    }

    /// True if the hap contains its own onset (`whole.begin == part.begin`).
    pub fn has_onset(&self) -> bool {
        match &self.whole {
            Some(w) => w.begin == self.part.begin,
            None => false,
        }
    }

    pub fn combine_context(&self, other: &Hap) -> Context {
        self.context.combine(&other.context)
    }

    pub fn set_context(&self, context: Context) -> Hap {
        Hap {
            whole: self.whole,
            part: self.part,
            value: self.value.clone(),
            context,
        }
    }

    /// Whole-span equality, treating two continuous haps (both `None`) as equal.
    pub fn span_equals(&self, other: &Hap) -> bool {
        match (&self.whole, &other.whole) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }
}

impl PartialEq for Hap {
    fn eq(&self, other: &Self) -> bool {
        self.span_equals(other) && self.part == other.part && self.value == other.value
    }
}

/// Read a control out of a hap value as a [`Frac`], but only when it is a
/// genuine number (`Int`/`F64`/`Frac`) — matching Strudel's
/// `typeof value?.key === 'number'` guard, so a string or boolean control is
/// ignored rather than coerced.
fn numeric_field(value: &Value, key: &str) -> Option<Frac> {
    match value {
        Value::Map(m) => match m.get(key) {
            Some(v @ (Value::Int(_) | Value::F64(_) | Value::Frac(_))) => Some(v.to_frac()),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn span(b: i64, e: i64) -> TimeSpan {
        TimeSpan::new(Frac::int(b), Frac::int(e))
    }

    fn map_hap(pairs: &[(&str, Value)]) -> Hap {
        let m: BTreeMap<String, Value> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        Hap::new(Some(span(0, 1)), span(0, 1), Value::Map(m))
    }

    #[test]
    fn clipped_duration_defaults_to_whole() {
        let hap = Hap::new(Some(span(0, 2)), span(0, 2), Value::Int(0));
        assert_eq!(hap.clipped_duration(), Frac::int(2));
    }

    #[test]
    fn clip_multiplies_whole_duration() {
        // whole = 1 cycle, clip 0.5 -> 1/2 (Strudel: duration.mul(value.clip)).
        let hap = map_hap(&[("clip", Value::F64(0.5))]);
        assert_eq!(hap.clipped_duration(), Frac::new(1, 2));
    }

    #[test]
    fn duration_control_overrides_whole_then_clip_multiplies() {
        // `duration` overrides the whole length; `clip` then multiplies it,
        // matching the order in Strudel's getter.
        let hap = map_hap(&[("duration", Value::F64(0.25)), ("clip", Value::Int(2))]);
        assert_eq!(hap.clipped_duration(), Frac::new(1, 2));
    }

    #[test]
    fn non_numeric_clip_is_ignored() {
        // typeof !== 'number' -> the control is not applied.
        let hap = map_hap(&[("clip", Value::Str("x".into()))]);
        assert_eq!(hap.clipped_duration(), Frac::int(1));
    }

    #[test]
    fn structural_duration_ignores_clip() {
        // `duration()` stays the structural whole length (used by splice/fit).
        let hap = map_hap(&[("clip", Value::F64(0.5))]);
        assert_eq!(hap.duration(), Frac::int(1));
    }

    #[test]
    fn end_clipped_uses_clipped_duration() {
        // whole [0,1) with clip 0.5 -> sounding event ends at 1/2.
        let m: BTreeMap<String, Value> = [("clip".to_string(), Value::F64(0.5))].into();
        let hap = Hap::new(Some(span(0, 1)), span(0, 1), Value::Map(m));
        assert_eq!(hap.end_clipped(), Frac::new(1, 2));
    }

    #[test]
    fn has_onset_only_when_whole_begins_with_part() {
        // A fragment whose part starts after the whole has no onset.
        let onset = Hap::new(Some(span(0, 1)), span(0, 1), Value::Int(0));
        assert!(onset.has_onset());
        let fragment = Hap::new(
            Some(span(0, 1)),
            TimeSpan::new(Frac::new(1, 2), Frac::int(1)),
            Value::Int(0),
        );
        assert!(!fragment.has_onset());
        // Continuous haps (no whole) never have an onset.
        let continuous = Hap::new(None, span(0, 1), Value::Int(0));
        assert!(!continuous.has_onset());
    }

    #[test]
    fn span_equals_treats_two_continuous_haps_as_equal() {
        let a = Hap::new(None, span(0, 1), Value::Int(0));
        let b = Hap::new(None, span(2, 3), Value::Int(9));
        assert!(a.span_equals(&b));
        let discrete = Hap::new(Some(span(0, 1)), span(0, 1), Value::Int(0));
        assert!(!a.span_equals(&discrete));
    }

    #[test]
    fn with_span_maps_whole_and_part_but_keeps_continuous_whole_none() {
        let hap = Hap::new(Some(span(0, 1)), span(0, 1), Value::Int(0));
        let shifted = hap.with_span(|s| s.with_time(|t| t + Frac::int(1)));
        assert_eq!(shifted.whole, Some(span(1, 2)));
        assert_eq!(shifted.part, span(1, 2));
        // whole_or_part falls back to part for continuous haps.
        let continuous = Hap::new(None, span(0, 1), Value::Int(0));
        assert_eq!(continuous.whole_or_part(), span(0, 1));
        assert_eq!(
            continuous.with_span(|s| s.with_time(|t| t + Frac::int(1))).whole,
            None
        );
    }
}
