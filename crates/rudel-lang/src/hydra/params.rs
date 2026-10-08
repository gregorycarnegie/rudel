//! Hydra arguments that change every frame.
//!
//! Upstream a function or array argument becomes a uniform that hydra-synth
//! re-reads each frame (`format-arguments.js`). Here such an argument is a
//! numbered slot: the chain compiles to a read of `hu.dyn` at that slot, and
//! the app fills the slots each frame from [`HydraParam::value`].

use crate::js::{self, Arg, SendFn};
use rudel_core::{Frac, Pattern};
use std::sync::Arc;

/// hydra-synth's default `bpm`, which its array sequencing counts in.
const BPM: f64 = 30.0;

/// One per-frame argument.
#[derive(Clone)]
pub struct HydraParam(Arc<Source>);

enum Source {
    /// An array, stepped through as `lib/array-utils.js` does.
    Seq(Seq),
    /// `H(pattern)`: the pattern's value at the current cycle.
    Pattern(Pattern),
    /// A script function, called with `{time, bpm}`.
    Func(SendFn),
}

pub(crate) struct Seq {
    pub(crate) values: Vec<f64>,
    pub(crate) speed: f64,
    pub(crate) smooth: f64,
    pub(crate) ease: String,
    pub(crate) offset: f64,
}

impl HydraParam {
    pub(crate) fn seq(seq: Seq) -> Self {
        Self(Arc::new(Source::Seq(seq)))
    }

    pub(crate) fn pattern(pattern: Pattern) -> Self {
        Self(Arc::new(Source::Pattern(pattern)))
    }

    pub(crate) fn func(func: SendFn) -> Self {
        Self(Arc::new(Source::Func(func)))
    }

    /// The value this frame, at hydra's `time` in seconds and the transport's
    /// `cycle`. `None` where upstream would fall back to the input's default:
    /// a function that throws or returns a non-number, an empty array, a
    /// pattern with nothing playing.
    pub fn value(&self, time: f64, cycle: f64) -> Option<f64> {
        match &*self.0 {
            Source::Seq(seq) => seq.value(time),
            Source::Pattern(pattern) => {
                let at = Frac::from_f64(cycle);
                pattern.query_arc(at, at).first()?.value.as_f64()
            }
            Source::Func(func) => func
                .run(move |f| {
                    let props = Arg::Map(vec![
                        ("time".to_string(), Arg::Num(time)),
                        ("bpm".to_string(), Arg::Num(BPM)),
                    ]);
                    match js::call(f, vec![props]) {
                        Ok(Arg::Num(n)) => Some(n),
                        _ => None,
                    }
                })
                .flatten(),
        }
    }
}

impl Seq {
    /// `arrayUtils.getValue`, ported.
    fn value(&self, time: f64) -> Option<f64> {
        let len = self.values.len() as f64;
        if len == 0.0 {
            return None;
        }
        let speed = if self.speed != 0.0 { self.speed } else { 1.0 };
        let index = time * speed * (BPM / 60.0) + self.offset;
        let at = |i: f64| self.values[i.rem_euclid(len).floor() as usize];
        if self.smooth != 0.0 {
            let index = index - self.smooth / 2.0;
            let current = at(index);
            let next = at(index + 1.0);
            let t = (index.rem_euclid(1.0) / self.smooth).min(1.0);
            Some(ease(&self.ease, t) * (next - current) + current)
        } else {
            Some(at(index))
        }
    }
}

/// hydra-synth's `easing-functions.js`; an unknown name is linear.
fn ease(name: &str, t: f64) -> f64 {
    match name {
        "easeInQuad" => t * t,
        "easeOutQuad" => t * (2.0 - t),
        "easeInOutQuad" if t < 0.5 => 2.0 * t * t,
        "easeInOutQuad" => -1.0 + (4.0 - 2.0 * t) * t,
        "easeInCubic" => t * t * t,
        "easeOutCubic" => (t - 1.0).powi(3) + 1.0,
        "easeInOutCubic" if t < 0.5 => 4.0 * t * t * t,
        "easeInOutCubic" => (t - 1.0) * (2.0 * t - 2.0) * (2.0 * t - 2.0) + 1.0,
        "easeInQuart" => t.powi(4),
        "easeOutQuart" => 1.0 - (t - 1.0).powi(4),
        "easeInOutQuart" if t < 0.5 => 8.0 * t.powi(4),
        "easeInOutQuart" => 1.0 - 8.0 * (t - 1.0).powi(4),
        "easeInQuint" => t.powi(5),
        "easeOutQuint" => 1.0 + (t - 1.0).powi(5),
        "easeInOutQuint" if t < 0.5 => 16.0 * t.powi(5),
        "easeInOutQuint" => 1.0 + 16.0 * (t - 1.0).powi(5),
        "sin" => (1.0 + (std::f64::consts::PI * t - std::f64::consts::FRAC_PI_2).sin()) / 2.0,
        _ => t,
    }
}

// Identity: a parameter is the one an evaluation made, and the sources
// (patterns, script functions) have no equality of their own.
impl PartialEq for HydraParam {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for HydraParam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match &*self.0 {
            Source::Seq(_) => "HydraParam::Seq",
            Source::Pattern(_) => "HydraParam::Pattern",
            Source::Func(_) => "HydraParam::Func",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn seq(values: &[f64], speed: f64, smooth: f64, offset: f64) -> Seq {
        Seq {
            values: values.to_vec(),
            speed,
            smooth,
            ease: "linear".to_string(),
            offset,
        }
    }

    #[test]
    fn an_array_steps_at_half_a_value_a_second_by_default() {
        // bpm 30: index = time / 2.
        let s = seq(&[10.0, 20.0, 30.0], 1.0, 0.0, 0.0);
        assert_eq!(s.value(0.0), Some(10.0));
        assert_eq!(s.value(1.9), Some(10.0));
        assert_eq!(s.value(2.0), Some(20.0));
        assert_eq!(s.value(6.0), Some(10.0), "wraps");
        // `.fast(2)` doubles the rate; `0` speed is treated as 1, as upstream.
        assert_eq!(seq(&[10.0, 20.0], 2.0, 0.0, 0.0).value(1.0), Some(20.0));
        assert_eq!(seq(&[10.0, 20.0], 0.0, 0.0, 0.0).value(2.0), Some(20.0));
        assert_eq!(seq(&[10.0, 20.0], 1.0, 0.0, 0.5).value(1.0), Some(20.0));
        assert_eq!(seq(&[], 1.0, 0.0, 0.0).value(1.0), None);
    }

    #[test]
    fn a_smoothed_array_interpolates_between_neighbours() {
        let s = seq(&[0.0, 1.0], 1.0, 1.0, 0.0);
        // index - 1/2 = 0.25 at time 1.5: a quarter of the way from 0 to 1.
        assert!((s.value(1.5).unwrap() - 0.25).abs() < 1e-9);
        // Early on the index is negative; it wraps, it does not go negative.
        let early = s.value(0.0).unwrap();
        assert!((0.0..=1.0).contains(&early), "{early}");
    }

    #[rstest]
    fn the_easings_run_from_zero_to_one(
        #[values("linear", "easeInOutQuad", "easeOutCubic", "easeInOutQuint", "sin")] name: &str,
    ) {
        assert!(ease(name, 0.0).abs() < 1e-9);
        assert!((ease(name, 1.0) - 1.0).abs() < 1e-9);
    }
}
