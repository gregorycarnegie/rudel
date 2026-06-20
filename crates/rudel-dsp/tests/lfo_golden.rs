// lfo_golden.rs — audio parity for the LFO modulator source against superdough.
//
// `lfo_golden.json` (from tools/oracle/gen_lfo_oracle.mjs) holds, per case, the
// LFO config and the exact buffer the real superdough `lfo-processor` worklet
// produces at 44.1kHz. Here each is rebuilt with rudel's `Lfo` and compared
// sample-for-sample.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_dsp::{Lfo, LfoConfig};

const SAMPLE_RATE: f64 = 44100.0;
const EPS: f64 = 1e-9;

fn config_from_json(c: &serde_json::Value) -> LfoConfig {
    let g = |k: &str| c[k].as_f64().unwrap();
    LfoConfig {
        shape: g("shape") as usize,
        frequency: g("frequency"),
        skew: g("skew"),
        depth: g("depth"),
        dcoffset: g("dcoffset"),
        phaseoffset: g("phaseoffset"),
        curve: g("curve"),
        time: g("time"),
        min: g("min"),
        max: g("max"),
    }
}

#[test]
fn lfo_matches_superdough() {
    let golden: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("lfo_golden.json")).expect("parse golden");

    let mut failures = Vec::new();
    for (label, entry) in &golden {
        let cfg = config_from_json(&entry["cfg"]);
        let want: Vec<f64> = entry["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();

        let mut lfo = Lfo::new(&cfg, SAMPLE_RATE);
        let got: Vec<f64> = (0..want.len()).map(|_| lfo.tick()).collect();

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
        "lfo source mismatches:\n{}",
        failures.join("\n")
    );
}
