// worklet_golden.rs — audio parity for the superdough AudioWorklet DSP that
// rudel ports by hand: the Moog `ladder-processor`, the orbit-bus
// `djf-processor`, and the `transient-processor` shaper.
//
// `worklet_golden.json` (from tools/oracle/gen_worklet_oracle.mjs) holds one
// shared mono input buffer plus, per case, the exact output the real worklet
// loop produces at 44.1kHz. Here each is replayed through rudel's port.
//
// Upstream runs in JS doubles; rudel's DSP is `f32` throughout (like every
// other filter in the engine), so these compare against a tolerance rather than
// bit-for-bit. The ladder is a nonlinear feedback structure, so its tolerance is
// the loosest of the three — it bounds accumulated single-precision drift, not
// a difference in the recurrence.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_dsp::{Djf, Ladder, TransientShaper};

const SAMPLE_RATE: f32 = 44100.0;

fn golden() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../tools/oracle/worklet_golden.json"))
        .expect("parse golden")
}

fn floats(v: &serde_json::Value) -> Vec<f32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap() as f32)
        .collect()
}

/// Compare against the golden, reporting the worst absolute deviation.
fn assert_close(label: &str, got: &[f32], want: &[f32], tol: f32) {
    assert_eq!(got.len(), want.len(), "{label}: length");
    let (i, worst) = got
        .iter()
        .zip(want)
        .map(|(a, b)| (a - b).abs())
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    assert!(
        worst <= tol,
        "{label}: worst deviation {worst:e} at sample {i} (got {}, want {}) exceeds {tol:e}",
        got[i],
        want[i]
    );
}

#[test]
fn worklet_ports_match_superdough() {
    let golden = golden();
    let input = floats(&golden["input"]);
    let cases = golden["cases"].as_object().expect("cases");
    assert!(!cases.is_empty(), "golden has no cases");

    for (label, case) in cases {
        let want = floats(&case["samples"]);
        let f = |k: &str| case[k].as_f64().unwrap() as f32;
        let (got, tol): (Vec<f32>, f32) = match case["kind"].as_str().unwrap() {
            "ladder" => {
                let mut l = Ladder::new(SAMPLE_RATE, f("frequency"), f("q"), f("drive"));
                (input.iter().map(|&x| l.process(x)).collect(), 2e-4)
            }
            "djf" => {
                let mut d = Djf::new(SAMPLE_RATE, f("value"));
                (input.iter().map(|&x| d.process(x)).collect(), 5e-5)
            }
            "transient" => {
                let mut t = TransientShaper::new(SAMPLE_RATE, f("attack"), f("sustain"));
                (input.iter().map(|&x| t.process(x)).collect(), 5e-5)
            }
            other => panic!("unknown case kind {other}"),
        };
        assert_close(label, &got, &want, tol);
    }
}
