// warp_golden.rs — parity for the wavetable oscillator's phase warping.
//
// `warp_golden.json` (from tools/oracle/gen_warp_oracle.mjs) holds, per warp
// mode, the exact phase superdough's `wavetable-oscillator-processor` produces
// across a grid of input phases and warp amounts. Here each is rebuilt with
// rudel's `warp_phase` and compared value-for-value.
// SPDX-License-Identifier: AGPL-3.0-or-later

use libtest_mimic::{Arguments, Failed, Trial};
use rudel_dsp::{WarpMode, warp_phase};
use serde_json::Value;

/// The oracle rounds to f32 before writing, so only float-op ordering can
/// differ; a couple of ULPs of slack covers `powf`/`exp`/`sin` differing in the
/// last bit between the two runtimes.
const EPS: f32 = 2e-6;

fn main() {
    let golden: serde_json::Map<String, Value> =
        serde_json::from_str(include_str!("../../../tools/oracle/warp_golden.json"))
            .expect("parse golden");
    let count = golden.len();
    let mut trials = vec![Trial::test("every_warp_mode_is_in_the_golden", move || {
        assert_eq!(count, 22, "every warp mode is covered");
        Ok(())
    })];
    for (label, entry) in golden {
        let phases = entry.clone();
        trials.push(Trial::test(format!("phase/{label}"), move || {
            warp_phase_matches_superdough(&phases)
        }));
        let name = label.clone();
        trials.push(Trial::test(format!("name/{label}"), move || {
            the_mode_name_resolves_to_upstreams_index(&name, &entry)
        }));
    }
    // Anything else is not a mode, rather than silently becoming one.
    for unknown in ["", "nonesuch", "asymm", "asy", "bend", "0", "flipp"] {
        trials.push(Trial::test(format!("not_a_mode/{unknown:?}"), move || {
            assert!(WarpMode::from_name(unknown).is_none());
            Ok(())
        }));
    }
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

/// One mode's grid of phases and warp amounts, value for value.
fn warp_phase_matches_superdough(entry: &Value) -> Result<(), Failed> {
    let floats = |k: &str| -> Vec<f32> {
        entry[k]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect()
    };
    let mode = WarpMode::from_index(entry["mode"].as_u64().unwrap() as u8);
    let (amounts, phases) = (floats("amounts"), floats("phases"));
    // A dense grid, so a mode is not passed on a handful of points.
    assert!(amounts.len() * phases.len() > 400, "grid too sparse");
    let mut failures = Vec::new();
    for (row, amt) in amounts.iter().enumerate() {
        let want = entry["values"][row].as_array().unwrap();
        for (col, phase) in phases.iter().enumerate() {
            let expected = want[col].as_f64().unwrap() as f32;
            let got = warp_phase(*phase, *amt, mode);
            if (got - expected).abs() > EPS {
                failures.push(format!("phase {phase} amt {amt} -> {got}, want {expected}"));
            }
        }
    }
    if failures.is_empty() {
        return Ok(());
    }
    let shown = failures[..failures.len().min(20)].join("\n");
    Err(format!("{} mismatches:\n{shown}", failures.len()).into())
}

/// `WarpMode::from_name` is a 22-arm string match, and the 2026-08 mutation run
/// left 21 of its arms alive: nothing named a mode by string, so every arm could
/// return the wrong variant undetected. A hand-written table would be the same
/// list typed twice and could drift the same way, so this reads the names and
/// indices out of the golden file — which comes from upstream's own `Warpmode`
/// object.
fn the_mode_name_resolves_to_upstreams_index(name: &str, entry: &Value) -> Result<(), Failed> {
    let index = entry["mode"].as_u64().expect("mode index") as u8;
    // Upstream matches on the upper-cased name; rudel takes it in any case.
    for spelling in [name.to_ascii_lowercase(), name.to_ascii_uppercase()] {
        let mode = WarpMode::from_name(&spelling)
            .ok_or_else(|| format!("{spelling} should be a warp mode"))?;
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
    Ok(())
}
