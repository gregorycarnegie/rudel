/// Built-in synth waveforms + noise sources (always available as `s(...)`).
pub(crate) const WAVEFORMS: &[&str] = &[
    "sine", "saw", "square", "triangle", "pulse", "user", "supersaw", "white", "pink", "brown",
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

/// Control names exposed by the engine, for the reference pane.
pub(crate) const CONTROLS: &[&str] = &[
    "note",
    "n",
    "i",
    "freq",
    "s",
    "tune",
    "xen",
    "withBase",
    "ftrans",
    "mpe",
    "bendRange",
    "gain",
    "pan",
    "speed",
    "cutoff",
    "resonance",
    "lpf",
    "lpq",
    "hcutoff",
    "hresonance",
    "hpf",
    "hpq",
    "bandf",
    "bandq",
    "bpf",
    "bpq",
    "lpenv",
    "lpattack",
    "lpdecay",
    "lpsustain",
    "lprelease",
    "fanchor",
    "room",
    "roomlp",
    "roomdim",
    "roomfade",
    "rlp",
    "rdim",
    "rfade",
    "size",
    "shape",
    "crush",
    "distort",
    "distortvol",
    "distorttype",
    "compressor",
    "postgain",
    "amp",
    "source",
    "stretch",
    "duration",
    "gate",
    "delay",
    "delaytime",
    "delayfeedback",
    "attack",
    "decay",
    "sustain",
    "release",
    "adsr",
    "ad",
    "ar",
    "hold",
    "unison",
    "detune",
    "spread",
    "fm",
    "fmh",
    "fmwave",
    "fmattack",
    "fmdecay",
    "fmsustain",
    "fmrelease",
    "fmi2",
    "fmh2",
    "fmwave2",
    "partials",
    "phases",
    "pw",
    "noise",
    "pcurve",
    "vib",
    "vibmod",
    "penv",
    "pattack",
    "vowel",
    "accelerate",
    "coarse",
    "wt",
    "warp",
    "chorus",
    "drive",
    "duck",
    "djf",
    "squiz",
    "octave",
    "channels",
    "ir",
    "color",
    "midichan",
    "ccn",
    "ccv",
    "orbit",
    "velocity",
    "begin",
    "end",
    "legato",
    "clip",
    "unit",
    "fmap",
    "piano",
    "pow",
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
        for c in CONTROLS {
            assert!(
                known.contains(c),
                "control `{c}` is not exposed by the runtime"
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
