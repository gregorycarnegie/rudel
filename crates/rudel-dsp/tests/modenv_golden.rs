// modenv_golden.rs — audio parity for the modulation envelope source
// (`env(...)`) against superdough, the companion to `lfo_golden.rs`.
//
// `modenv_golden.json` (from tools/oracle/gen_modenv_oracle.mjs) holds, per
// case, the envelope config and the buffer the real `envelope-processor`
// worklet produces at 44.1kHz — stored every `stride` samples to keep the file
// small. Here each is rebuilt with rudel's `ModEnv`, ticked for every sample,
// and compared at the stored points.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_dsp::{EnvConfig, ModEnv};

const SAMPLE_RATE: f64 = 44100.0;
const EPS: f64 = 1e-9;

fn config_from_json(c: &serde_json::Value) -> EnvConfig {
    let d = EnvConfig::default();
    let g = |k: &str, fallback: f64| c[k].as_f64().unwrap_or(fallback);
    EnvConfig {
        attack: g("attack", d.attack),
        decay: g("decay", d.decay),
        sustain: g("sustain", d.sustain),
        release: g("release", d.release),
        attack_curve: g("attackCurve", d.attack_curve),
        decay_curve: g("decayCurve", d.decay_curve),
        release_curve: g("releaseCurve", d.release_curve),
        depth: g("depth", d.depth),
        min: g("min", d.min),
        max: g("max", d.max),
        sustain_time: g("susTime", d.sustain_time),
    }
}

#[test]
fn modulation_envelope_matches_superdough() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/oracle/modenv_golden.json"))
            .expect("parse golden");
    let stride = golden["stride"].as_u64().unwrap() as usize;
    let length = golden["length"].as_u64().unwrap() as usize;
    let cases = golden["cases"].as_object().expect("cases");
    assert!(!cases.is_empty(), "golden has no cases");

    let mut failures = Vec::new();
    for (label, entry) in cases {
        let cfg = config_from_json(&entry["cfg"]);
        let want: Vec<f64> = entry["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let mut env = ModEnv::new(&cfg, SAMPLE_RATE);
        // Tick every sample; compare only the stored ones.
        let got: Vec<f64> = (0..length)
            .map(|_| env.tick())
            .step_by(stride)
            .collect::<Vec<_>>();
        assert_eq!(got.len(), want.len(), "{label}: length");
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            if (g - w).abs() > EPS {
                failures.push(format!("{label}[{}]: got {g}, want {w}", i * stride));
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
