use rudel_core::{Frac, Pattern, Value};

/// The control key marking which output a hap is routed to (`.midi()`/`.osc()`).
pub(super) const IO_KEY: &str = "_io";

/// Keep haps routed to `target` via the `_io` control, plus untagged haps when
/// `include_untagged` (the default output). The routing keys (`_io`/`_midiport`)
/// are stripped from kept haps so they don't leak into the back-end.
pub fn filter_output(pat: &Pattern, target: &str, include_untagged: bool) -> Pattern {
    let target = target.to_string();
    pat.filter_values(move |v| match v {
        Value::Map(m) => match m.get(IO_KEY).and_then(|x| x.as_str()) {
            Some(io) => io == target,
            None => include_untagged,
        },
        _ => include_untagged,
    })
    .fmap(|v| match v {
        Value::Map(mut m) => {
            m.shift_remove(IO_KEY);
            m.shift_remove("_midiport");
            Value::Map(m)
        }
        other => other,
    })
}

/// Which tagged outputs (`midi`, `osc`) the pattern routes any haps to over the
/// first cycle. The app uses this to decide which back-ends to start.
pub fn output_targets(pat: &Pattern) -> (bool, bool) {
    let (mut midi, mut osc) = (false, false);
    for hap in pat.query_arc(Frac::zero(), Frac::one()) {
        if let Value::Map(m) = &hap.value
            && let Some(io) = m.get(IO_KEY).and_then(|x| x.as_str())
        {
            match io {
                "midi" => midi = true,
                "osc" => osc = true,
                _ => {}
            }
        }
    }
    (midi, osc)
}
