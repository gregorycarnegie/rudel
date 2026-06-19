// modulator.rs - LFO modulation source. Ported from the `lfo-processor`
// AudioWorklet in strudel/packages/superdough/worklets.mjs (waveshapes + the
// per-sample process loop) and the `getLfo` defaults in helpers.mjs. This is the
// deterministic modulation-source core of superdough's modulator engine
// (modulate/lfo/env/bmod); the per-voice control-target routing that connects a
// source to a node's AudioParam is a Web Audio graph concern tracked separately.
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::f64::consts::TAU;

/// Smooth a saw discontinuity (PolyBLEP), used by the `sawblep` shape.
fn poly_blep(phase: f64, dt: f64) -> f64 {
    let invdt = 1.0 / dt;
    if phase < dt {
        let p = phase * invdt;
        2.0 * p - p * p - 1.0
    } else if phase > 1.0 - dt {
        let p = (phase - 1.0) * invdt;
        p * p + 2.0 * p + 1.0
    } else {
        0.0
    }
}

/// A unipolar (mostly 0..1) LFO waveshape by index, matching the order in
/// superdough's `waveshapes` table: 0 tri, 1 sine, 2 ramp, 3 saw, 4 square,
/// 5 custom, 6 sawblep. `skew` doubles as the `dt` argument for `sawblep`
/// (as the worklet passes it). `custom` (5) needs an array of break-points the
/// scalar worklet path can't supply, so it is treated as silence here.
pub fn waveshape(shape: usize, phase: f64, skew: f64) -> f64 {
    match shape {
        0 => {
            let x = 1.0 - skew;
            if phase >= skew {
                1.0 / x - phase / x
            } else {
                phase / skew
            }
        }
        1 => (TAU * phase).sin() * 0.5 + 0.5,
        2 => phase,
        3 => 1.0 - phase,
        4 => {
            if phase >= skew {
                0.0
            } else {
                1.0
            }
        }
        6 => {
            let v = 2.0 * phase - 1.0;
            v - poly_blep(phase, skew)
        }
        _ => 0.0,
    }
}

/// Configuration for an [`Lfo`], mirroring the `lfo-processor` parameters and
/// `getLfo`'s defaults.
#[derive(Clone, Debug)]
pub struct LfoConfig {
    pub shape: usize,
    pub frequency: f64,
    pub skew: f64,
    pub depth: f64,
    pub dcoffset: f64,
    pub phaseoffset: f64,
    pub curve: f64,
    pub time: f64,
    pub min: f64,
    pub max: f64,
}

impl Default for LfoConfig {
    fn default() -> LfoConfig {
        // getLfo defaults (helpers.mjs): the unwritten min/max default to
        // dcoffset*depth .. dcoffset*depth + depth.
        let depth = 1.0;
        let dcoffset = -0.5;
        LfoConfig {
            shape: 0,
            frequency: 1.0,
            skew: 0.5,
            depth,
            dcoffset,
            phaseoffset: 0.0,
            curve: 1.0,
            time: 0.0,
            min: dcoffset * depth,
            max: dcoffset * depth + depth,
        }
    }
}

/// A stateful per-sample LFO (one `lfo-processor` instance).
#[derive(Clone, Debug)]
pub struct Lfo {
    phase: f64,
    dt: f64,
    shape: usize,
    skew: f64,
    depth: f64,
    dcoffset: f64,
    curve: f64,
    min: f64,
    max: f64,
}

impl Lfo {
    pub fn new(cfg: &LfoConfig, sample_rate: f64) -> Lfo {
        // `ffrac(time * frequency + phaseoffset)`; phase stays non-negative.
        let init = cfg.time * cfg.frequency + cfg.phaseoffset;
        Lfo {
            phase: init - init.trunc(),
            dt: cfg.frequency / sample_rate,
            shape: cfg.shape,
            skew: cfg.skew,
            depth: cfg.depth,
            dcoffset: cfg.dcoffset,
            curve: cfg.curve,
            min: cfg.min,
            max: cfg.max,
        }
    }

    /// The next modulation value.
    pub fn tick(&mut self) -> f64 {
        let mut modval =
            (waveshape(self.shape, self.phase, self.skew) + self.dcoffset) * self.depth;
        modval = modval.powf(self.curve);
        // JS `clamp` is min(max(v,min),max), which (unlike f64::clamp) does not
        // assume min <= max and never panics.
        let out = modval.max(self.min).min(self.max);
        self.phase += self.dt;
        if self.phase > 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_lfo_is_centered_and_bounded() {
        // a sine LFO (dcoffset -0.5, depth 1) oscillates in [-0.5, 0.5] around 0.
        let cfg = LfoConfig {
            shape: 1,
            frequency: 100.0,
            ..LfoConfig::default()
        };
        let mut lfo = Lfo::new(&cfg, 44100.0);
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for _ in 0..1000 {
            let v = lfo.tick();
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(lo >= -0.5 - 1e-9, "min too low: {lo}");
        assert!(lo < -0.45, "min not reached: {lo}");
        assert!(hi <= 0.5 + 1e-9, "max too high: {hi}");
        assert!(hi > 0.45, "max not reached: {hi}");
    }

    #[test]
    fn ramp_sweeps_up_then_resets() {
        // a ramp LFO with dcoffset 0 / depth 1 rises through 0..1 and resets.
        let cfg = LfoConfig {
            shape: 2,
            frequency: 4.0,
            dcoffset: 0.0,
            min: 0.0,
            max: 1.0,
            ..LfoConfig::default()
        };
        let mut lfo = Lfo::new(&cfg, 64.0); // 16 samples per cycle
        let vals: Vec<f64> = (0..20).map(|_| lfo.tick()).collect();
        assert!(vals[0].abs() < 1e-12, "starts at 0");
        assert!(
            vals.iter().all(|&v| (0.0..=1.0).contains(&v)),
            "bounded 0..1"
        );
        // rises across the first cycle, then drops back near 0 after the wrap.
        assert!(vals[10] > vals[1], "rising within a cycle");
        assert!(vals[17] < vals[15], "resets after the period");
    }
}
