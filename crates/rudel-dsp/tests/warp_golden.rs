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

#[test]
fn every_warp_mode_name_resolves_to_upstreams_index() {
    // `WarpMode::from_name` is a 22-arm string match, and the 2026-08 mutation
    // run left 21 of its arms alive: nothing named a mode by string, so every
    // arm could return the wrong variant undetected. A hand-written table here
    // would be the same list typed twice and could drift the same way, so this
    // reads the names and indices out of the golden file — which comes from
    // upstream's own `Warpmode` object.
    let golden: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("../../../tools/oracle/warp_golden.json"))
            .expect("parse golden");

    for (name, entry) in &golden {
        let index = entry["mode"].as_u64().expect("mode index") as u8;

        // Upstream matches on the upper-cased name; rudel takes it in any case.
        for spelling in [name.to_ascii_lowercase(), name.to_ascii_uppercase()] {
            let mode = WarpMode::from_name(&spelling)
                .unwrap_or_else(|| panic!("{spelling} should be a warp mode"));
            assert_eq!(
                mode as u8, index,
                "{spelling} should be mode {index}, got {}",
                mode as u8
            );
            // ...and the two ways in agree, so a pattern giving `warpmode` as a
            // number and one giving it as a name reach the same warp.
            assert_eq!(
                WarpMode::from_index(index) as u8,
                mode as u8,
                "{spelling}: from_index and from_name disagree"
            );
        }
    }
    assert_eq!(golden.len(), 22, "expected every mode in the corpus");

    // Anything else is not a mode, rather than silently becoming one.
    for unknown in ["", "nonesuch", "asymm", "asy", "bend", "0", "flipp"] {
        assert!(
            WarpMode::from_name(unknown).is_none(),
            "{unknown:?} should not resolve to a warp mode"
        );
    }
}
