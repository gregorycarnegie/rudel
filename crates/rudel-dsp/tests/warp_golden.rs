// warp_golden.rs — parity for the wavetable oscillator's phase warping.
//
// `warp_golden.json` (from tools/oracle/gen_warp_oracle.mjs) holds, per warp
// mode, the exact phase superdough's `wavetable-oscillator-processor` produces
// across a grid of input phases and warp amounts. Here each is rebuilt with
// rudel's `warp_phase` and compared value-for-value.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_dsp::{WarpMode, warp_phase};

/// The oracle rounds to f32 before writing, so only float-op ordering can
/// differ; a couple of ULPs of slack covers `powf`/`exp`/`sin` differing in the
/// last bit between the two runtimes.
const EPS: f32 = 2e-6;

#[test]
fn warp_phase_matches_superdough() {
    let golden: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("../../../tools/oracle/warp_golden.json"))
            .expect("parse golden");

    let mut failures = Vec::new();
    let mut checked = 0usize;
    for (label, entry) in &golden {
        let mode = WarpMode::from_index(entry["mode"].as_u64().unwrap() as u8);
        let amounts: Vec<f32> = entry["amounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();
        let phases: Vec<f32> = entry["phases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();
        for (row, amt) in amounts.iter().enumerate() {
            let want = entry["values"][row].as_array().unwrap();
            for (col, phase) in phases.iter().enumerate() {
                let expected = want[col].as_f64().unwrap() as f32;
                let got = warp_phase(*phase, *amt, mode);
                checked += 1;
                if (got - expected).abs() > EPS {
                    failures.push(format!(
                        "{label}: phase {phase} amt {amt} -> {got}, want {expected}"
                    ));
                }
            }
        }
    }
    assert_eq!(golden.len(), 22, "every warp mode is covered");
    assert!(checked > 9000, "grid should be dense, checked {checked}");
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
