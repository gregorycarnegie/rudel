// timespan.rs - ported from strudel/packages/core/timespan.mjs
// Copyright (C) 2022 Strudel contributors; 2026 Rudel contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::Frac;

/// A span of time `[begin, end)` in cycles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeSpan {
    pub begin: Frac,
    pub end: Frac,
}

impl TimeSpan {
    pub fn new(begin: Frac, end: Frac) -> Self {
        TimeSpan { begin, end }
    }

    pub fn duration(&self) -> Frac {
        self.end - self.begin
    }

    /// Splits the span at cycle boundaries (`get spanCycles`).
    pub fn span_cycles(&self) -> Vec<TimeSpan> {
        let mut spans = Vec::new();
        let mut begin = self.begin;
        let end = self.end;
        let end_sam = end.sam();

        // Support zero-width timespans.
        if begin == end {
            return vec![TimeSpan::new(begin, end)];
        }

        while end > begin {
            if begin.sam() == end_sam {
                spans.push(TimeSpan::new(begin, self.end));
                break;
            }
            let next_begin = begin.next_sam();
            spans.push(TimeSpan::new(begin, next_begin));
            begin = next_begin;
        }
        spans
    }

    /// Shifts a span to one of equal duration starting within cycle zero.
    pub fn cycle_arc(&self) -> TimeSpan {
        let b = self.begin.cycle_pos();
        let e = b + self.duration();
        TimeSpan::new(b, e)
    }

    pub fn with_time(&self, f: impl Fn(Frac) -> Frac) -> TimeSpan {
        TimeSpan::new(f(self.begin), f(self.end))
    }

    pub fn with_end(&self, f: impl Fn(Frac) -> Frac) -> TimeSpan {
        TimeSpan::new(self.begin, f(self.end))
    }

    /// Like `with_time`, but time is relative to the cycle (the sam of begin).
    pub fn with_cycle(&self, f: impl Fn(Frac) -> Frac) -> TimeSpan {
        let sam = self.begin.sam();
        let b = sam + f(self.begin - sam);
        let e = sam + f(self.end - sam);
        TimeSpan::new(b, e)
    }

    /// Intersection of two spans, or `None` if they don't intersect.
    pub fn intersection(&self, other: &TimeSpan) -> Option<TimeSpan> {
        let begin = self.begin.max(other.begin);
        let end = self.end.min(other.end);

        if begin > end {
            return None;
        }
        if begin == end {
            // Zero-width (point) intersection - doesn't count at the end of a
            // non-zero-width span.
            if begin == self.end && self.begin < self.end {
                return None;
            }
            if begin == other.end && other.begin < other.end {
                return None;
            }
        }
        Some(TimeSpan::new(begin, end))
    }

    /// Like `intersection`, but panics (`intersection_e`) if there is no overlap.
    pub fn intersection_e(&self, other: &TimeSpan) -> TimeSpan {
        self.intersection(other)
            .expect("TimeSpans do not intersect")
    }

    pub fn midpoint(&self) -> Frac {
        self.begin + self.duration() / Frac::int(2)
    }
}

impl std::fmt::Display for TimeSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} → {}", self.begin, self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_cycles_splits() {
        let s = TimeSpan::new(Frac::zero(), Frac::int(2));
        let cycles = s.span_cycles();
        assert_eq!(cycles.len(), 2);
        assert_eq!(cycles[0], TimeSpan::new(Frac::zero(), Frac::int(1)));
        assert_eq!(cycles[1], TimeSpan::new(Frac::int(1), Frac::int(2)));
    }

    #[test]
    fn intersection_basic() {
        let a = TimeSpan::new(Frac::zero(), Frac::int(1));
        let b = TimeSpan::new(Frac::new(1, 2), Frac::int(2));
        assert_eq!(
            a.intersection(&b),
            Some(TimeSpan::new(Frac::new(1, 2), Frac::int(1)))
        );
    }
}
