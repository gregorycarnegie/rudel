#![no_main]

use libfuzzer_sys::fuzz_target;
use rudel_dsp::ByteBeatExpr;

fuzz_target!(|source: &str| {
    let expr = ByteBeatExpr::parse(source);
    // Non-finite results are legal JS arithmetic; panics and stateful evaluation aren't.
    for t in [0.0, -1.0, 255.0, 65536.0, f64::MAX, f64::INFINITY, f64::NAN] {
        let first = expr.eval_with(t, &[("$input", -0.5), ("time", t)]);
        let second = expr.eval_with(t, &[("$input", -0.5), ("time", t)]);
        assert!(first == second || (first.is_nan() && second.is_nan()));
    }
});
