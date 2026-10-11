// zzfx_golden.rs — audio parity for the ZzFX synth core against superdough.
//
// `zzfx_golden.json` (from tools/oracle/gen_zzfx_oracle.mjs) holds, per case, the
// 20 buildSamples params and the exact sample buffer the real
// superdough/zzfx_fork.mjs produces at 44.1kHz with randomness 0. Here each
// buffer is rebuilt with rudel's `build_samples` and compared sample-for-sample,
// one test per case.
// SPDX-License-Identifier: AGPL-3.0-or-later

use libtest_mimic::{Arguments, Failed, Trial};
use rudel_dsp::{ZzfxSynth, build_samples};
use serde_json::Value;

const SAMPLE_RATE: f64 = 44100.0;
const EPS: f64 = 1e-9;
/// zzfx's pseudo-noise is `((sin(i) + 1) * 1e9) % 2`, so it amplifies the last
/// bits of `sin` a billionfold — and no two libms round `sin` alike, JS engine
/// against Rust least of all. Cases with `noise != 0` therefore drift from the
/// oracle by whatever that amplified ulp does to the phase accumulator: fractions
/// of a bit here, well under -120 dBFS, but far more than [`EPS`].
const NOISE_EPS: f64 = 1e-6;

fn synth_from_params(p: &[f64]) -> ZzfxSynth {
    ZzfxSynth {
        volume: p[0],
        randomness: p[1],
        frequency: p[2],
        attack: p[3],
        sustain: p[4],
        release: p[5],
        shape: p[6] as i32,
        shape_curve: p[7],
        slide: p[8],
        delta_slide: p[9],
        pitch_jump: p[10],
        pitch_jump_time: p[11],
        repeat_time: p[12],
        noise: p[13],
        modulation: p[14],
        bit_crush: p[15],
        delay: p[16],
        sustain_volume: p[17],
        decay: p[18],
        tremolo: p[19],
    }
}

fn main() {
    let golden: serde_json::Map<String, Value> =
        serde_json::from_str(include_str!("../../../tools/oracle/zzfx_golden.json"))
            .expect("parse golden");
    let trials = golden
        .into_iter()
        .map(|(label, entry)| Trial::test(label, move || build_samples_matches_superdough(&entry)))
        .collect();
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

fn build_samples_matches_superdough(entry: &Value) -> Result<(), Failed> {
    let floats = |k: &str| -> Vec<f64> {
        entry[k]
            .as_array()
            .unwrap_or_else(|| panic!("{k} array"))
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect()
    };
    let (params, want) = (floats("params"), floats("samples"));
    // randomness is 0 in every golden case, so the rand draw is a no-op.
    let got = build_samples(&synth_from_params(&params), SAMPLE_RATE, 0.5);
    if got.len() != want.len() {
        return Err(format!("length {} != {} (strudel)", got.len(), want.len()).into());
    }
    let (at, worst) = got
        .iter()
        .zip(&want)
        .map(|(g, w)| (g - w).abs())
        .enumerate()
        .fold(
            (0, 0.0),
            |m, d| if d.1.is_nan() || d.1 > m.1 { d } else { m },
        );
    let eps = if params[13] == 0.0 { EPS } else { NOISE_EPS };
    // NaN compares false against any bound: it is a failure, not a pass.
    if worst.is_nan() || worst > eps {
        return Err(format!(
            "max diff {worst:.3e} at sample {at} (rudel {} vs strudel {})",
            got[at], want[at]
        )
        .into());
    }
    Ok(())
}
