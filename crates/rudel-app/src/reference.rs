/// Built-in synth waveforms + noise sources (always available as `s(...)`).
pub(crate) const WAVEFORMS: &[&str] = &[
    "sine", "saw", "square", "triangle", "pulse", "user", "supersaw", "white", "pink", "brown",
    "zzfx", "bytebeat", "bus",
];

/// Built-in synthesized drum sounds (always available as `s(...)`).
pub(crate) const DRUMS: &[&str] = &[
    "bd", "sd", "rim", "cp", "hh", "oh", "lt", "mt", "ht", "rd", "cr",
];

/// Continuous signals (used as values, e.g. `sine.range(0, 1)`).
pub(crate) const SIGNALS: &[&str] = &[
    "sine", "cosine", "saw", "isaw", "tri", "square", "sine2", "saw2", "rand", "rand2", "perlin",
    "time", "irand(n)", "run(n)",
];

/// Pattern factories (top-level constructors).
pub(crate) const FACTORIES: &[&str] = &[
    "stack",
    "cat",
    "seq",
    "fastcat",
    "slowcat",
    "randcat",
    "chooseCycles",
    "pure",
    "gap",
    "silence",
    "i",
    "freq",
    "getFreq",
    "Math",
];

pub(crate) const LANGUAGE_KEYWORDS: &[&str] = &[
    "const", "let", "fn", "if", "else", "for", "while", "in", "match", "return", "true", "false",
    "null",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The curated reference-panel lists are display-only, but every name in
    /// them must be a real part of the runtime surface, so the panel can't
    /// advertise functions/controls that no longer exist.
    #[test]
    fn curated_reference_lists_exist_in_the_generated_surface() {
        let reference = rudel_lang::reference();
        let known: HashSet<&str> = reference
            .functions
            .iter()
            .chain(reference.methods.iter())
            .chain(reference.controls.iter())
            .map(String::as_str)
            .collect();
        for f in FACTORIES {
            assert!(
                known.contains(f),
                "factory `{f}` is not exposed by the runtime"
            );
        }
        for s in SIGNALS {
            let base = s.strip_suffix("(n)").unwrap_or(s);
            assert!(
                known.contains(base),
                "signal `{base}` is not exposed by the runtime"
            );
        }
    }
}
