// zzfx_golden.rs — audio parity for the ZzFX synth core against superdough.
//
// `zzfx_golden.json` (from tools/oracle/gen_zzfx_oracle.mjs) holds, per case, the
// 20 buildSamples params and the exact sample buffer the real
// superdough/zzfx_fork.mjs produces at 44.1kHz with randomness 0. Here each
// buffer is rebuilt with rudel's `build_samples` and compared sample-for-sample.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_dsp::{ZzfxSynth, build_samples};

const SAMPLE_RATE: f64 = 44100.0;
const EPS: f64 = 1e-9;

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

#[test]
fn build_samples_matches_superdough() {
    let golden: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("zzfx_golden.json")).expect("parse golden");

    let mut failures = Vec::new();
    for (label, entry) in &golden {
        let params: Vec<f64> = entry["params"]
            .as_array()
            .expect("params array")
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let want: Vec<f64> = entry["samples"]
            .as_array()
            .expect("samples array")
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        // randomness is 0 in every golden case, so the rand draw is a no-op.
        let got = build_samples(&synth_from_params(&params), SAMPLE_RATE, 0.5);

        if got.len() != want.len() {
            failures.push(format!(
                "{label}: length {} != {} (strudel)",
                got.len(),
                want.len()
            ));
            continue;
        }
        let mut worst = 0.0_f64;
        let mut at = 0usize;
        for (k, (g, w)) in got.iter().zip(&want).enumerate() {
            let d = (g - w).abs();
            if d > worst {
                worst = d;
                at = k;
            }
        }
        if worst > EPS {
            failures.push(format!(
                "{label}: max diff {worst:.3e} at sample {at} (rudel {} vs strudel {})",
                got[at], want[at]
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "zzfx build_samples mismatches:\n{}",
        failures.join("\n")
    );
}
