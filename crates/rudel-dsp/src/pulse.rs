// pulse.rs - superdough's `pulse` oscillator (the `pulse-oscillator` worklet).
//
// Not a plain duty-cycle pulse: two half-Tomisawa oscillators, each a cosine
// phase-modulated by its own low-passed output, the second offset by the pulse
// width, with their difference as the output. A smoothed phase increment and a
// feedback amount that falls with pitch tame the top end. Ported line for line
// from worklets.mjs, in f64 as the JS runs, including its quirk of restarting
// the decay `env` at 1 on every 128-frame render quantum.
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::f64::consts::{PI, TAU};

/// Web Audio's render quantum, which the worklet's `env` restarts on.
const QUANTUM: u32 = 128;

#[derive(Clone, Debug)]
pub(crate) struct PulseOsc {
    phi: f64,
    y0: f64,
    y1: f64,
    dphif: f64,
    envf: f64,
    env: f64,
    /// Frames into the current render quantum.
    frame: u32,
}

impl Default for PulseOsc {
    fn default() -> PulseOsc {
        PulseOsc {
            phi: -PI,
            y0: 0.0,
            y1: 0.0,
            dphif: 0.0,
            envf: 0.0,
            env: 1.0,
            frame: 0,
        }
    }
}

impl PulseOsc {
    /// The next sample at `frequency` Hz (detune already applied) and
    /// `pulsewidth` (0..1, 0.5 square; clamped to ±0.99).
    pub(crate) fn next(&mut self, frequency: f64, pulsewidth: f64, sample_rate: f64) -> f32 {
        if self.frame == 0 {
            self.env = 1.0;
        }
        self.frame = (self.frame + 1) % QUANTUM;
        let pw = (1.0 - pulsewidth.clamp(-0.99, 0.99)) * PI;
        let dphi = frequency * TAU / sample_rate;
        self.dphif += 0.1 * (dphi - self.dphif);
        self.env *= 0.9998;
        self.envf += 0.1 * (self.env - self.envf);
        let b = (2.3 * (1.0 - 0.0001 * frequency)).max(0.0);
        self.phi += self.dphif;
        if self.phi >= PI {
            self.phi -= TAU;
        }
        let out0 = (self.phi + b * self.y0).cos();
        self.y0 = 0.5 * (out0 + self.y0);
        let out1 = (self.phi + b * self.y1 + pw).cos();
        self.y1 = 0.5 * (out1 + self.y1);
        (0.15 * (out0 - out1) * self.envf) as f32
    }
}
