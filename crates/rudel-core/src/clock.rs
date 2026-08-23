// clock.rs - maps between the audio second-clock and cycle time.
// Mirrors strudel/packages/core/cyclist.mjs's cps bookkeeping: a live `cps`
// change re-anchors the mapping at the current moment (cyclist's
// `num_cycles_at_cps_change`/`seconds_at_cps_change`) so the cycle counter is
// continuous across the change instead of jumping.
// SPDX-License-Identifier: AGPL-3.0-or-later

/// Converts between absolute seconds (the audio clock) and cycle time.
///
/// The mapping is anchored at `(anchor_seconds, anchor_cycle)` and advances at
/// `cps` cycles per second, so `cycle = anchor_cycle + (seconds -
/// anchor_seconds) * cps`. A constant-`cps` clock anchored at the origin is the
/// plain `cycle = seconds * cps`. [`set_cps`](Self::set_cps) re-anchors at the
/// current instant, matching Strudel's cyclist: the cycle value at the moment
/// of the change is preserved, then time advances at the new rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    /// Seconds at the anchor point (the last cps change, or the origin).
    anchor_seconds: f64,
    /// Cycle position at the anchor point.
    anchor_cycle: f64,
    /// Current cycles per second.
    cps: f64,
}

impl Clock {
    /// A clock anchored at the origin (`0 s` = cycle `0`) running at `cps`.
    pub fn new(cps: f64) -> Clock {
        Clock {
            anchor_seconds: 0.0,
            anchor_cycle: 0.0,
            cps,
        }
    }

    /// The current cycles-per-second rate.
    pub fn cps(&self) -> f64 {
        self.cps
    }

    /// The cycle position at absolute time `seconds`.
    pub fn cycle_at(&self, seconds: f64) -> f64 {
        self.anchor_cycle + (seconds - self.anchor_seconds) * self.cps
    }

    /// The absolute time (seconds) at which cycle `cycle` occurs — the inverse
    /// of [`cycle_at`](Self::cycle_at). Used to turn a hap's onset cycle into a
    /// trigger time on the audio clock.
    pub fn seconds_at(&self, cycle: f64) -> f64 {
        self.anchor_seconds + (cycle - self.anchor_cycle) / self.cps
    }

    /// Switch to a new `cps` at absolute time `seconds`, re-anchoring so the
    /// cycle position is continuous across the change (cyclist's `setCps`).
    /// A no-op when `cps` is unchanged (matching cyclist's early return) or
    /// invalid (non-finite / non-positive), so repeated identical sets don't
    /// drift the anchor.
    pub fn set_cps(&mut self, seconds: f64, cps: f64) {
        if !cps.is_finite() || cps <= 0.0 || cps == self.cps {
            return;
        }
        self.anchor_cycle = self.cycle_at(seconds);
        self.anchor_seconds = seconds;
        self.cps = cps;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_clock_is_plain_seconds_times_cps() {
        let clock = Clock::new(0.5);
        assert!((clock.cycle_at(10.0) - 5.0).abs() < 1e-12);
        assert!((clock.seconds_at(5.0) - 10.0).abs() < 1e-12);
    }

    #[test]
    fn cycle_at_and_seconds_at_are_inverses() {
        let clock = Clock::new(1.5);
        let cycle = clock.cycle_at(7.25);
        assert!((clock.seconds_at(cycle) - 7.25).abs() < 1e-12);
    }

    #[test]
    fn set_cps_is_continuous_across_the_change() {
        // Stable at cps=1 from the origin; at t=10 the position is 10 cycles.
        let mut clock = Clock::new(1.0);
        assert!((clock.cycle_at(10.0) - 10.0).abs() < 1e-12);
        // Halving cps at t=10 must not move the cycle counter at that instant.
        clock.set_cps(10.0, 0.5);
        assert!((clock.cycle_at(10.0) - 10.0).abs() < 1e-12);
        // Afterwards time advances at the new rate: +2s -> +1 cycle.
        assert!((clock.cycle_at(12.0) - 11.0).abs() < 1e-12);
        // And the inverse still tracks the new anchor.
        assert!((clock.seconds_at(11.0) - 12.0).abs() < 1e-12);
    }

    #[test]
    fn set_cps_ignores_unchanged_and_invalid_rates() {
        let mut clock = Clock::new(1.0);
        let before = clock;
        clock.set_cps(5.0, 1.0); // same rate -> no re-anchor
        assert_eq!(clock, before);
        clock.set_cps(5.0, 0.0); // invalid
        assert_eq!(clock, before);
        clock.set_cps(5.0, f64::NAN); // invalid
        assert_eq!(clock, before);
    }

    #[test]
    fn repeated_identical_set_does_not_drift_the_anchor() {
        let mut clock = Clock::new(1.0);
        clock.set_cps(10.0, 2.0);
        let after_first = clock;
        clock.set_cps(20.0, 2.0); // identical cps later -> no-op
        assert_eq!(clock, after_first);
    }
}
