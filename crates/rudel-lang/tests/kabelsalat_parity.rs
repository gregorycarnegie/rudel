// kabelsalat_parity.rs — audio parity for the `K(...)` port, against real
// kabelsalat.
//
// `kabelsalat_golden.json` (from tools/oracle/gen_kabelsalat_oracle.mjs) holds,
// per patch, the topologically sorted graph `@kabelsalat/lib` builds and the
// samples superdough's `generic-processor` renders from it. Rudel goes from the
// same source text to samples by a completely different route — the graph is
// built by native bindings, compiled to an instruction list, and interpreted —
// so agreeing here means the whole chain agrees.
//
// This is the one test that crosses the crate seam: `rudel-lang` compiles the
// graph and `rudel-dsp` runs it, and neither can see the other outside a test.
// `rudel-dsp` is a dev-dependency for exactly this, which is sound because the
// dependency only ever points that way.
// SPDX-License-Identifier: AGPL-3.0-or-later

use libtest_mimic::{Arguments, Failed, Trial};
use rudel_core::{Frac, Value};
use rudel_dsp::{KabelProgram, KabelVoice, VoiceLike};
use std::sync::Arc;

/// Both sides do the arithmetic in f64 and land in f32, and the ugen bodies are
/// transcribed operation for operation, so the only spread is the last bit or
/// two of the f32. A filter or a delay accumulates that over 64 samples.
const EPS: f32 = 2e-5;

/// A silent voice under the graph: no patch here uses `audioin`.
struct Silence;

impl VoiceLike for Silence {
    fn tick(&mut self) -> (f32, f32) {
        (0.0, 0.0)
    }
    fn is_done(&self) -> bool {
        false
    }
}

/// The `worklet` control a `K(...)` source compiles to.
fn program_value(source: &str) -> Value {
    let script = format!("K({source})");
    let pattern = rudel_lang::eval(&script).unwrap_or_else(|e| panic!("{script}: {e}"));
    let haps = pattern.query_arc(Frac::zero(), Frac::int(1));
    let Some(Value::Map(controls)) = haps.first().map(|h| h.value.clone()) else {
        panic!("{script} should produce a control map");
    };
    controls
        .get("worklet")
        .cloned()
        .unwrap_or_else(|| panic!("{script} should carry a `worklet` control"))
}

fn main() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/oracle/kabelsalat_golden.json"))
            .expect("parse golden");
    let golden = Arc::new(golden);
    let cases = golden["cases"].as_array().expect("cases").clone();
    let mut trials = vec![Trial::test("the_golden_is_the_pinned_full_set", {
        let golden = golden.clone();
        move || {
            // The version is pinned in the golden so a bump to the vendored
            // kabelsalat is a reviewable change rather than a silent one.
            assert_eq!(
                golden["version"].as_str(),
                Some("0.4.1"),
                "regenerate the golden after changing the vendored @kabelsalat/lib"
            );
            let count = golden["cases"].as_array().expect("cases").len();
            assert!(count > 30, "expected the full case set, got {count}");
            Ok(())
        }
    })];
    for case in cases {
        let name = case["name"].as_str().expect("name").to_string();
        let (samples, golden) = (case.clone(), golden.clone());
        trials.push(Trial::test(format!("samples/{name}"), move || {
            a_graph_renders_the_samples_kabelsalat_renders(&golden, &samples)
        }));
        trials.push(Trial::test(format!("graph/{name}"), move || {
            a_graph_has_the_nodes_kabelsalat_builds(&case)
        }));
    }
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

fn a_graph_renders_the_samples_kabelsalat_renders(
    golden: &serde_json::Value,
    case: &serde_json::Value,
) -> Result<(), Failed> {
    let sample_rate = golden["sampleRate"].as_f64().expect("sampleRate") as f32;
    let frames = golden["frames"].as_u64().expect("frames") as usize;
    let source = case["source"].as_str().expect("source");
    let expected = |side: &str| -> Vec<f32> {
        case[side]
            .as_array()
            .expect("channel")
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect()
    };
    let (want_l, want_r) = (expected("left"), expected("right"));

    let value = program_value(source);
    let program = KabelProgram::from_value(&value)
        .ok_or_else(|| format!("`{source}` did not compile to a program"))?;
    // The graph is the whole voice here, so the gate never falls and nothing
    // ends inside the window.
    let mut voice = KabelVoice::new(
        program,
        Box::new(Silence),
        sample_rate,
        440.0,
        Vec::new(),
        f32::MAX,
        f32::MAX,
    );
    for i in 0..frames {
        let (got_l, got_r) = voice.tick();
        for (side, got, want) in [("L", got_l, want_l[i]), ("R", got_r, want_r[i])] {
            let off = (got - want).abs();
            if off.is_nan() || off > EPS {
                return Err(format!("`{source}` frame {i}{side}: got {got}, want {want}").into());
            }
        }
    }
    Ok(())
}

/// The sample test would catch a wrong graph too, but not say so; this one
/// names the node list, which is where a compiler bug actually lives.
fn a_graph_has_the_nodes_kabelsalat_builds(case: &serde_json::Value) -> Result<(), Failed> {
    let source = case["source"].as_str().expect("source");
    // The generator has already dropped upstream's output plumbing, which
    // rudel keeps in a side table rather than as nodes.
    let want: Vec<&str> = case["graph"]
        .as_array()
        .expect("graph")
        .iter()
        .filter_map(|n| n["type"].as_str())
        .collect();
    let Value::Map(map) = program_value(source) else {
        return Err("no program".into());
    };
    let Some(Value::List(types)) = map.get("types") else {
        return Err("no types".into());
    };
    let got: Vec<&str> = types.iter().filter_map(|t| t.as_str()).collect();
    if got != want {
        return Err(format!("`{source}`:\n  got  {got:?}\n  want {want:?}").into());
    }
    Ok(())
}
