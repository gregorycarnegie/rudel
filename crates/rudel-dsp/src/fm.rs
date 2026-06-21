// fm.rs - multi-operator FM matrix. Ports superdough's `applyFM`
// (strudel/packages/superdough/helpers.mjs): operators 1..=8, each tuned to the
// carrier by an `fmh` ratio with its own `fmwave` and modulation-index envelope,
// routed by an `fmiIJ` matrix into each other and the carrier (target 0).
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::envelope::Adsr;
use crate::oscillator::Waveform;
use rudel_core::ValueMap;

/// Number of FM operators (1..=`FM_OPS`); index 0 is the carrier target.
pub const FM_OPS: usize = 8;

/// One FM operator: a sine-ish oscillator tuned to `carrier * ratio`, optionally
/// gated by a modulation-index envelope.
#[derive(Clone, Copy, Debug)]
pub struct FmOp {
    /// `fmh{n}`: operator frequency / carrier frequency.
    pub ratio: f32,
    /// `fmwave{n}`: operator waveform.
    pub wave: Waveform,
    /// `fm{adsr}{n}`: modulation-index envelope (scales the operator 0..1).
    pub env: Option<Adsr>,
}

impl Default for FmOp {
    fn default() -> Self {
        FmOp {
            ratio: 1.0,
            wave: Waveform::Sine,
            env: None,
        }
    }
}

/// A multi-operator FM matrix. `amt[i][j]` is the modulation index from operator
/// `i` (1..=8) into target `j` (0 = carrier, else operator `j`).
#[derive(Clone, Debug)]
pub struct FmSpec {
    /// Operators; index `1..=FM_OPS` are used (index 0 is unused padding).
    pub ops: [FmOp; FM_OPS + 1],
    /// Modulation amounts `amt[source][target]`; target 0 is the carrier.
    pub amt: [[f32; FM_OPS + 1]; FM_OPS + 1],
    /// Highest operator referenced as a modulation source (0 = no FM).
    pub max_op: usize,
}

impl Default for FmSpec {
    fn default() -> Self {
        FmSpec {
            ops: [FmOp::default(); FM_OPS + 1],
            amt: [[0.0; FM_OPS + 1]; FM_OPS + 1],
            max_op: 0,
        }
    }
}

impl FmSpec {
    /// True when any FM routing is active.
    pub fn active(&self) -> bool {
        self.max_op > 0
    }

    /// A single-operator FM (operator 1 → carrier) — the common case and what
    /// the bare `fm`/`fmi`/`fmh`/`fmwave`/`fm{adsr}` controls build.
    pub fn single(index: f32, ratio: f32, wave: Waveform, env: Option<Adsr>) -> FmSpec {
        let mut spec = FmSpec::default();
        spec.ops[1] = FmOp { ratio, wave, env };
        spec.amt[1][0] = index;
        spec.max_op = if index != 0.0 { 1 } else { 0 };
        spec
    }

    /// Build the matrix from a control map.
    pub fn from_controls(map: &ValueMap) -> FmSpec {
        let f = |k: &str| map.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
        let mut spec = FmSpec::default();

        // Per-operator ratio / waveform / index envelope. Operator 1 uses the
        // un-suffixed control names (`fmh`, `fmwave`, `fmattack`, ...).
        for n in 1..=FM_OPS {
            let s = if n == 1 { String::new() } else { n.to_string() };
            if let Some(r) = f(&format!("fmh{s}")) {
                spec.ops[n].ratio = r;
            }
            if let Some(w) = map.get(&format!("fmwave{s}")).and_then(|v| v.as_str())
                && let Some(wave) = Waveform::from_name(w)
            {
                spec.ops[n].wave = wave;
            }
            spec.ops[n].env = op_env(
                f(&format!("fmattack{s}")),
                f(&format!("fmdecay{s}")),
                f(&format!("fmsustain{s}")),
                f(&format!("fmrelease{s}")),
            );
        }

        // Modulation matrix. The control name per (i, j) matches superdough:
        // adjacent `i == j+1` is the chain control `fmi{i}` (`fmi` for i=1);
        // everything else is the two-digit `fmi{i}{j}`.
        for i in 1..=FM_OPS {
            for j in 0..=FM_OPS {
                let name = if i == j + 1 {
                    if i == 1 {
                        "fmi".to_string()
                    } else {
                        format!("fmi{i}")
                    }
                } else {
                    format!("fmi{i}{j}")
                };
                if let Some(a) = f(&name) {
                    spec.amt[i][j] = a;
                }
            }
        }
        // `fm` is an alias for `fmi` (operator 1 → carrier).
        if let Some(a) = f("fm") {
            spec.amt[1][0] = a;
        }

        spec.max_op = (1..=FM_OPS)
            .rev()
            .find(|&i| (0..=FM_OPS).any(|j| spec.amt[i][j] != 0.0))
            .unwrap_or(0);
        spec
    }
}

/// Build an operator's modulation-index envelope from its `fm{adsr}` values,
/// mirroring superdough's `getADSRValues` (active only if any value is set;
/// sustain defaults to full when only attack/decay are given).
fn op_env(a: Option<f32>, d: Option<f32>, su: Option<f32>, r: Option<f32>) -> Option<Adsr> {
    if a.is_none() && d.is_none() && su.is_none() && r.is_none() {
        return None;
    }
    let sustain = su.unwrap_or(if d.is_none() { 1.0 } else { 0.001 });
    Some(Adsr {
        attack: a.unwrap_or(0.0).max(0.001),
        decay: d.unwrap_or(0.0).max(0.001),
        sustain: sustain.clamp(0.0, 1.0),
        release: r.unwrap_or(0.0).max(0.01),
    })
}
