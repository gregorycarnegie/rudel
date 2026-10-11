// lfo_golden.rs — audio parity for the LFO modulator source against superdough.
//
// `lfo_golden.json` (from tools/oracle/gen_lfo_oracle.mjs) holds, per case, the
// LFO config and the exact buffer the real superdough `lfo-processor` worklet
// produces at 44.1kHz. Here each is rebuilt with rudel's `Lfo` and compared
// sample-for-sample, one test per case.
// SPDX-License-Identifier: AGPL-3.0-or-later

use libtest_mimic::{Arguments, Failed, Trial};
use rudel_dsp::{Lfo, LfoConfig};
use serde_json::Value;

const SAMPLE_RATE: f64 = 44100.0;
const EPS: f64 = 1e-9;

fn config_from_json(c: &Value) -> LfoConfig {
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

fn main() {
    let golden: serde_json::Map<String, Value> =
        serde_json::from_str(include_str!("../../../tools/oracle/lfo_golden.json"))
            .expect("parse golden");
    let trials = golden
        .into_iter()
        .map(|(label, entry)| Trial::test(label, move || lfo_matches_superdough(&entry)))
        .collect();
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

fn lfo_matches_superdough(entry: &Value) -> Result<(), Failed> {
    let cfg = config_from_json(&entry["cfg"]);
    let want: Vec<f64> = entry["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();

    let mut lfo = Lfo::new(&cfg, SAMPLE_RATE);
    let got: Vec<f64> = (0..want.len()).map(|_| lfo.tick()).collect();

    let (at, worst) = got
        .iter()
        .zip(&want)
        .map(|(g, w)| (g - w).abs())
        .enumerate()
        .fold(
            (0, 0.0),
            |m, d| if d.1.is_nan() || d.1 > m.1 { d } else { m },
        );
    // NaN compares false against any bound: it is a failure, not a pass.
    if worst.is_nan() || worst > EPS {
        return Err(format!(
            "max diff {worst:.3e} at sample {at} (rudel {} vs strudel {})",
            got[at], want[at]
        )
        .into());
    }
    Ok(())
}
