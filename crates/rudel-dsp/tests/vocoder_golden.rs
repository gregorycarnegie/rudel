// vocoder_golden.rs — audio parity for superdough's `phase-vocoder-processor`,
// the worklet behind `stretch`, which rudel ports by hand in
// crates/rudel-dsp/src/vocoder.rs.
//
// `vocoder_golden.json` (from tools/oracle/gen_vocoder_oracle.mjs) holds one
// shared stereo input plus, per `stretch` value, the output the real processor
// produces at 44.1kHz in 128-frame blocks. The generator slices the
// `PhaseVocoderProcessor` class straight out of the vendored worklets.mjs and
// runs it against upstream's own `OLAProcessor` and `fft.js`, so this compares
// against the algorithm rather than a transcription of it.
//
// Why an oracle rather than assertions: the port is ~200 lines of index and
// phase arithmetic — peak regions of influence, the `omegaDelta * timeCursor`
// correction, the overlap-add normalisation. Every one of those is a number
// that has to be *right*, not merely finite, and the 2026-08 mutation run put
// 63 of vocoder.rs's 151 mutants in exactly that arithmetic.
//
// One deliberate deviation, and the reason there are two reference sets below:
// `fft.js`'s `realTransform` fills only bins 0..N/2 and leaves its own butterfly
// scratch above Nyquist, in a buffer that is reused frame to frame. Upstream's
// `shiftPeaks` reads those bins whenever a peak's region of influence runs off
// the top of the spectrum, which happens only when the pitch factor is below 1
// — a negative `stretch`. Those values belong to fft.js's internal layout and no
// other FFT can reproduce them, so rudel stops at the last real bin instead.
// `cases` is upstream with that read neutralised (rudel matches it exactly);
// `upstreamRaw` is upstream untouched, and the second test below pins that the
// two differ *only* below unity — so the deviation is bounded by a check rather
// than by a claim.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_dsp::PhaseVocoder;

const HOP: usize = 128;

fn golden() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../tools/oracle/vocoder_golden.json"))
        .expect("parse vocoder_golden.json")
}

fn floats(v: &serde_json::Value) -> Vec<f32> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_f64().expect("number") as f32)
        .collect()
}

/// Run rudel's vocoder over the shared input, hop by hop, as the engine does.
fn render(stretch: f32, left: &[f32], right: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut pv = PhaseVocoder::new(stretch);
    let (mut out_l, mut out_r) = (Vec::new(), Vec::new());
    for block in 0..left.len() / HOP {
        let mut l = [0.0f32; HOP];
        let mut r = [0.0f32; HOP];
        l.copy_from_slice(&left[block * HOP..(block + 1) * HOP]);
        r.copy_from_slice(&right[block * HOP..(block + 1) * HOP]);
        pv.process(&mut l, &mut r);
        out_l.extend_from_slice(&l);
        out_r.extend_from_slice(&r);
    }
    (out_l, out_r)
}

/// The worst absolute difference, and where it happened. A non-finite sample on
/// either side counts as infinitely wrong — `(a - b).abs()` on a NaN compares
/// false against every threshold and would pass silently.
fn worst_diff(got: &[f32], want: &[f32]) -> (f32, usize) {
    assert_eq!(got.len(), want.len(), "length mismatch");
    let (mut worst, mut at) = (0.0f32, 0);
    for (i, (&g, &w)) in got.iter().zip(want).enumerate() {
        let d = if g.is_finite() && w.is_finite() {
            (g - w).abs()
        } else {
            f32::INFINITY
        };
        if d > worst {
            worst = d;
            at = i;
        }
    }
    (worst, at)
}

#[test]
fn the_phase_vocoder_matches_the_superdough_worklet() {
    let g = golden();
    let left = floats(&g["input"]["left"]);
    let right = floats(&g["input"]["right"]);
    assert_eq!(g["hopSize"].as_u64().unwrap() as usize, HOP);
    assert!(!left.is_empty(), "the corpus should not be empty");

    let mut checked = 0;
    for case in g["cases"].as_array().expect("cases") {
        let stretch = case["stretch"].as_f64().expect("stretch") as f32;
        let want_l = floats(&case["left"]);
        let want_r = floats(&case["right"]);
        let (got_l, got_r) = render(stretch, &left, &right);

        for (side, got, want) in [("left", &got_l, &want_l), ("right", &got_r, &want_r)] {
            let (worst, at) = worst_diff(got, want);
            let peak = want.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > 1e-3, "stretch {stretch} {side} is silent");
            // Upstream runs the FFT in doubles and rudel in `f32`; two 2048-point
            // transforms per frame is the only thing this tolerance covers.
            assert!(
                worst < peak * 0.02,
                "stretch {stretch} {side}: worst diff {worst} at {at} \
                 (got {}, want {}), peak {peak}",
                got[at],
                want[at]
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 5, "expected every stretch case");
}

#[test]
fn the_scratch_read_is_the_only_place_upstream_differs() {
    // Bounds the deviation described at the top of this file. Above unity the
    // regions of influence never run off the top of the spectrum, so upstream's
    // uninitialised bins are never reached and the two references have to agree
    // exactly. Below unity they are reached, and the references have to differ —
    // otherwise the neutralised reference is not testing anything and rudel
    // could be matching upstream's raw output by accident.
    let g = golden();
    let raw = g["upstreamRaw"].as_array().expect("upstreamRaw");
    let neutralised = g["cases"].as_array().expect("cases");
    assert_eq!(raw.len(), neutralised.len());

    let mut below = 0;
    let mut at_or_above = 0;
    for (r, n) in raw.iter().zip(neutralised) {
        let stretch = r["stretch"].as_f64().expect("stretch") as f32;
        assert_eq!(stretch, n["stretch"].as_f64().unwrap() as f32);
        // The pitch factor upstream derives from `stretch`.
        let factor = (if stretch < 0.0 {
            stretch * 0.25
        } else {
            stretch
        } + 1.0)
            .max(0.0);
        let (worst, _) = worst_diff(&floats(&r["left"]), &floats(&n["left"]));

        if factor >= 1.0 {
            assert_eq!(
                worst, 0.0,
                "stretch {stretch} (factor {factor}) should never reach the scratch"
            );
            at_or_above += 1;
        } else {
            assert!(
                worst > 1e-3,
                "stretch {stretch} (factor {factor}) should reach the scratch, \
                 but the two references agree — the neutralising is not working"
            );
            below += 1;
        }
    }
    assert!(
        at_or_above >= 4 && below >= 1,
        "expected both halves covered"
    );
}

#[test]
fn the_output_is_not_merely_a_copy_of_the_input() {
    // The tolerance above is relative to the signal, so a port that passed audio
    // straight through would have to be caught here instead: each stretch has to
    // differ from the input, and from the other stretches.
    let g = golden();
    let left = floats(&g["input"]["left"]);
    let cases = g["cases"].as_array().expect("cases");

    let rendered: Vec<(f32, Vec<f32>)> = cases
        .iter()
        .map(|c| {
            let stretch = c["stretch"].as_f64().unwrap() as f32;
            (stretch, floats(&c["left"]))
        })
        .collect();

    // Past the 2048-sample priming, the output is nothing like the input.
    let (_, first) = &rendered[0];
    let tail = 2048..first.len();
    let tracking = first[tail.clone()]
        .iter()
        .zip(&left[tail.clone()])
        .filter(|(a, b)| (**a - **b).abs() < 1e-4)
        .count();
    assert!(
        tracking < tail.len() / 2,
        "the vocoder output should not track its input"
    );

    // And each stretch factor gives a different result.
    for pair in rendered.windows(2) {
        let ((s0, a), (s1, b)) = (&pair[0], &pair[1]);
        let (worst, _) = worst_diff(a, b);
        assert!(worst > 1e-3, "stretch {s0} and {s1} gave the same output");
    }
}
