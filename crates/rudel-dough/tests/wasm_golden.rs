//! The Rust dough engine against the browser's.
//!
//! `dough_golden.json` (from tools/oracle/gen_dough_oracle.mjs) holds, per
//! case, the stereo output of `dough.wasm`, the WebAssembly build of dough's
//! C that Strudel runs in the browser, for a few events. This plays the same
//! events through `rudel_dough` and compares frame for frame.
//!
//! The port follows the C's arithmetic exactly, including its number parsing
//! and rounding, so the output must match bit for bit: [`TOLERANCE`] is zero.
//! The cases run in the generator's order on one engine, reset between cases,
//! so state a reset keeps (the noise seed, registered files, the delay and
//! reverb tails) matches too.
//!
//! Regenerate after updating the vendored engine:
//!   cd tools/oracle && node gen_dough_oracle.mjs

use rudel_dough::Dough;
use serde_json::Value;

/// The largest difference allowed on any one sample, in full scale.
const TOLERANCE: f32 = 0.0;

#[test]
fn the_native_engine_renders_what_the_browser_does() {
    let golden: Value =
        serde_json::from_str(include_str!("../../../tools/oracle/dough_golden.json"))
            .expect("parse dough_golden.json");
    let rate = golden["sampleRate"].as_u64().expect("sampleRate") as u32;
    let block = golden["blockSize"].as_u64().expect("blockSize") as usize;
    let floats = |v: &Value| -> Vec<f32> {
        v.as_array()
            .expect("an array")
            .iter()
            .map(|x| x.as_f64().expect("a number") as f32)
            .collect()
    };
    let pcm = floats(&golden["samplePcm"]);

    let mut report = Vec::new();
    let mut failed = Vec::new();
    let mut dough = Dough::new(rate);
    for (i, case) in golden["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .enumerate()
    {
        let id = case["id"].as_str().expect("id");
        let (left, right) = (floats(&case["left"]), floats(&case["right"]));
        if i > 0 {
            assert!(dough.evaluate("dough/reset"));
        }
        if case["file"].as_bool() == Some(true) {
            assert_eq!(
                dough.sample_file("smp:0", &pcm, 1),
                Some(0),
                "{id}: file index"
            );
        }
        for message in case["messages"].as_array().expect("messages") {
            assert!(
                dough.evaluate(message.as_str().expect("message")),
                "{id}: {message}"
            );
        }
        let mut out = vec![(0.0f32, 0.0f32); left.len()];
        for chunk in out.chunks_mut(block) {
            dough.render_into(chunk);
        }

        let worst = out
            .iter()
            .zip(left.iter().zip(&right))
            .map(|(o, (l, r))| (o.0 - l).abs().max((o.1 - r).abs()))
            .fold(0.0f32, f32::max);
        let first = out
            .iter()
            .zip(left.iter().zip(&right))
            .position(|(o, (l, r))| (o.0 - l).abs().max((o.1 - r).abs()) > TOLERANCE);
        let differs = out
            .iter()
            .zip(left.iter().zip(&right))
            .position(|(o, (l, r))| o.0 != *l || o.1 != *r);
        report.push(match differs {
            None => format!("{id:22} identical"),
            Some(at) => format!("{id:22} worst {worst:.2e}, first difference at frame {at}"),
        });
        if let Some(frame) = first {
            failed.push(format!(
                "{id}: frame {frame} is ({:.6}, {:.6}) natively, ({:.6}, {:.6}) in the browser",
                out[frame].0, out[frame].1, left[frame], right[frame]
            ));
        }
    }
    eprintln!("{}", report.join("\n"));
    assert!(
        failed.is_empty(),
        "{} case(s) differ by more than {TOLERANCE}:\n{}",
        failed.len(),
        failed.join("\n")
    );
}
