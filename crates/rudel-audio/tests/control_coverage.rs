//! Every control a pattern can set must be read by *something*.
//!
//! Two bugs got past every other guard here in the same way: `clip` and
//! `velocity` were registered as controls, rode along on the hap — so the tune
//! hap-parity oracle saw them and was satisfied — and were then silently
//! dropped at the boundary between the control map and the voice. Nothing
//! failed; the tunes just sounded wrong, and only by ear.
//!
//! A control is consumed by looking its canonical key up in a control map, so
//! that key has to appear as a string literal somewhere in the crates that turn
//! controls into output. This checks exactly that and nothing more: it says a
//! control is *wired up*, not that it does the right thing — that is what the
//! per-unit DSP goldens in `rudel-dsp` and the tune oracle in `rudel-lang` are
//! for.
//!
//! A behavioural version of this was tried first — apply each control to a
//! probe pattern, render, and compare. It does not work: most controls only act
//! in the presence of another (`resonance` needs `cutoff`, `fmh` needs `fm`,
//! `delaytime` needs `delay`), so it reported 298 of 364 controls as dead and
//! would have needed a prerequisite table larger than the thing it was
//! checking. It also took seven minutes. This runs in milliseconds and would
//! have caught both real bugs, because neither `"clip"` nor `"velocity"`
//! appeared anywhere outside the registry.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// The crates that turn a control into *sound*: the DSP voices and mixer, the
/// scheduler that builds their parameters, and the parts of core that resolve a
/// control before a voice ever sees it (`scale`'s tagging, the transpose
/// family, sample durations).
///
/// Deliberately not rudel-midi/osc/app. `velocity` is also a MIDI note
/// velocity, so a corpus that included those crates still found the string
/// after the audio path had dropped it — the guard passed while the bug it
/// exists for was present. A control that belongs only to MIDI, OSC or the
/// editor is named in [`UNREAD`] instead.
const CONSUMERS: &[&str] = &["rudel-dsp", "rudel-audio", "rudel-core"];

/// Files inside those crates that are *not* the audio path, by file name.
/// `rudel-core` also hosts the MIDI input bus and the visualiser helpers, and
/// they name controls the mixer never sees — `input.rs` reads an incoming note's
/// `velocity`, which was enough to hide the very bug this guard exists for.
const NOT_AUDIO_PATH: &[&str] = &["input.rs", "midimap.rs", "draw.rs", "color.rs"];

/// Controls with no reader in any consumer crate.
///
/// Seeded from the state when this guard was introduced: it is an inventory
/// of what is not wired up, not a claim that each one is fine. The value is
/// the ratchet — nothing *new* can be dropped silently, which is all that was
/// needed to catch `clip` and `velocity`. Deleting an entry as it gets
/// implemented is the intended direction of travel.
const UNREAD: &[&str] = &[
    // SuperDirt synth parameters: Strudel registers them so `.osc()` can send
    // them on, and superdough does not implement them either. `superdirt_message`
    // forwards the whole control map verbatim, so these do reach SuperDirt —
    // just never by name, which is why nothing here mentions them.
    "binshift",
    "comb",
    "density",
    "enhance",
    "expression",
    "freeze",
    "fshift",
    "fshiftnote",
    "fshiftphase",
    "harmonic",
    "hbrick",
    "imag",
    "kcutoff",
    "krush",
    "lbrick",
    "leslie",
    "lock",
    "lrate",
    "lsize",
    "octer",
    "octersub",
    "octersubsub",
    "overgain",
    "overshape",
    "real",
    "ring",
    "ringdf",
    "ringf",
    "scram",
    "semitone",
    "triode",
    "tsdelay",
    "voice",
    "waveloss",
    "xsdelay",
    // superdough features not ported: the per-filter and wavetable LFOs
    // (`{lp,hp,bp,wt,warp}{rate,depth,shape,skew,sync,dc}`), pulse-width sweep,
    // stereo spread, and tremolo shaping.
    "bpdc",
    "bpdepth",
    "bpdepthfrequency",
    "bprate",
    "bpshape",
    "bpskew",
    "bpsync",
    "hpdc",
    "hpdepth",
    "hpdepthfrequency",
    "hprate",
    "hpshape",
    "hpskew",
    "hpsync",
    "lpdc",
    "lpdepth",
    "lpdepthfrequency",
    "lprate",
    "lpshape",
    "lpskew",
    "lpsync",
    "panorient",
    "panspan",
    "pansplay",
    "panwidth",
    "pwrate",
    "pwsweep",
    "tremolophase",
    "tremoloshape",
    "tremoloskew",
    "tremolosync",
    "warpattack",
    "warpdc",
    "warpdecay",
    "warpdepth",
    "warpenv",
    "warprate",
    "warprelease",
    "warpshape",
    "warpskew",
    "warpsustain",
    "warpsync",
    "wtattack",
    "wtdc",
    "wtdecay",
    "wtdepth",
    "wtenv",
    "wtrate",
    "wtrelease",
    "wtshape",
    "wtskew",
    "wtsustain",
    "wtsync",
    // Analyser feeds, read by the browser's canvas visualisers.
    "analyze",
    "fft",
    "frames",
    // MIDI transport, timecode and routing, handled outside the mixer.
    "hours",
    "midiport",
    "minutes",
    "seconds",
    "sustainpedal",
    "uid",
    "val",
    // Read by rudel-midi / rudel-osc, not by the mixer: note routing, channel
    // and port selection, and the pitch/aftertouch messages.
    "amp",
    "channel",
    "channels",
    "gate",
    "midibend",
    "midichan",
    "midicmd",
    "midimap",
    "miditouch",
    "mpe",
    "nrpnn",
    "nrpv",
    "octave",
    "oschost",
    "oscport",
    // Read by the editor's widgets: the colour an event is drawn in, the CSS
    // for its source highlight, and the pianoroll's trail.
    "color",
    "markcss",
    "smear",
    // superdough features not ported: sample playback-rate ramp, chorus, the
    // squiz harmoniser, and the `source` bus selector.
    "accelerate",
    "chorus",
    "source",
    "squiz",
    // Not yet ported.
    "delayspeed",
    "nudge",
];

/// Every `.rs` file under a crate's `src`, minus its own tests — a control
/// named only by a test is not being consumed by anything.
fn sources(crate_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![crate_dir.join("src")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "tests") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                && path.file_name().is_none_or(|n| n != "tests.rs")
                && !path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| NOT_AUDIO_PATH.contains(&n))
            {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn every_control_is_read_by_something() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf();

    let mut corpus = String::new();
    let mut files = 0usize;
    for name in CONSUMERS {
        for path in sources(&workspace.join(name)) {
            corpus.push_str(&std::fs::read_to_string(&path).expect("read source"));
            files += 1;
        }
    }
    assert!(
        files > 50,
        "expected the consumer crates' sources, found {files} file(s)"
    );

    // Canonical keys, not method spellings: the audio path reads `cutoff`, and
    // `lpf` is only how a pattern spells it. Each key remembers a spelling so a
    // failure names something the user would recognise.
    let mut keys: BTreeMap<String, &str> = BTreeMap::new();
    for (name, _) in rudel_core::control_builders() {
        keys.entry(rudel_core::control_name(name)).or_insert(name);
    }
    for (name, key) in rudel_core::numbered_control_names() {
        keys.entry(key)
            .or_insert_with(|| Box::leak(name.into_boxed_str()));
    }

    // A key is read if it appears as a literal, or if its stem does: the
    // numbered families are looked up with `format!("fmh{s}")`, so `fmh3` never
    // appears but `"fmh` does.
    let read = |key: &str| {
        if corpus.contains(&format!("\"{key}\"")) {
            return true;
        }
        let stem = key.trim_end_matches(|c: char| c.is_ascii_digit());
        stem != key && corpus.contains(&format!("\"{stem}"))
    };

    let excused: BTreeSet<&str> = UNREAD.iter().copied().collect();
    let missing: Vec<String> = keys
        .iter()
        .filter(|(key, _)| !excused.contains(key.as_str()))
        .filter(|(key, _)| !read(key))
        .map(|(key, spelling)| {
            if key == spelling {
                key.to_string()
            } else {
                format!("{key} (as `{spelling}`)")
            }
        })
        .collect();

    assert!(
        missing.is_empty(),
        "{} control(s) are registered but never read by {}:\n  {}\n\
         Each rides along on the hap and is dropped at the voice boundary — the \
         way `clip` and `velocity` both were. Wire each up, or add it to UNREAD \
         with the reason nothing reads it.",
        missing.len(),
        CONSUMERS.join(", "),
        missing.join("\n  ")
    );
}
