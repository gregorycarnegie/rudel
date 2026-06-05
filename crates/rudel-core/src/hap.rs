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
}

impl Context {
    pub fn combine(&self, other: &Context) -> Context {
        let mut locations = self.locations.clone();
        locations.extend(other.locations.iter().copied());
        // Keep whichever side carries a scale tag (the later/other one wins).
        let scale = other.scale.clone().or_else(|| self.scale.clone());
        Context { locations, scale }
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
