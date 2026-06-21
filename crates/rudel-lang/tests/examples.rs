//! Parity example suite: representative Strudel-style snippets across the major
//! feature areas, each asserted to evaluate to a queryable pattern in Rudel.
//!
//! This is the curated counterpart to Strudel's `test/examples.test.mjs` (which
//! runs every jsdoc `@example`): rather than snapshot every doc example — many
//! of which use intentionally-unsupported features (motion, draw callbacks,
//! soundfonts) — this pins a hand-checked example per category from the
//! supported surface, so an eval regression anywhere across first-sounds /
//! notes / effects / pattern-effects / mini-notation / tonal / xen / MIDI / OSC
//! / samples / synths / visual feedback is caught.

use rudel_core::Frac;
use rudel_lang::eval;

/// (category, label, source). Each must evaluate to a pattern.
const EXAMPLES: &[(&str, &str, &str)] = &[
    // --- first sounds ---
    ("first-sounds", "single drum", r#"s("bd")"#),
    ("first-sounds", "drum sequence", r#"s("bd sd hh oh")"#),
    ("first-sounds", "drum machine", r#"s("bd*2, ~ sd, hh*4")"#),
    // --- notes ---
    ("notes", "note names", r#"note("c e g b")"#),
    ("notes", "numeric notes", r#"n("0 2 4 7").s("piano")"#),
    ("notes", "octaves + sharps", r#"note("c4 e4 g4 c5 f#3")"#),
    // --- effects ---
    ("effects", "filter + room", r#"note("c e g").lpf(800).room(0.4)"#),
    ("effects", "gain + pan", r#"s("hh*8").gain("0.6 1").pan(sine)"#),
    ("effects", "delay + crush", r#"s("cp").delay(0.5).crush(4)"#),
    ("effects", "distortion shortcut", r#"note("c2").soft(2)"#),
    // --- pattern effects ---
    ("pattern-fx", "fast/slow", r#"s("bd sd").fast(2).slow(3)"#),
    ("pattern-fx", "every + rev", r#"note("c e g").every(3, |x| x.rev())"#),
    ("pattern-fx", "jux", r#"s("bd sd").jux(rev)"#),
    ("pattern-fx", "off + add", r#"note("c").off(0.25, |x| x.add(note(7)))"#),
    ("pattern-fx", "sometimesBy", r#"s("hh*8").sometimesBy(0.4, |x| x.speed(2))"#),
    ("pattern-fx", "chop/striate", r#"s("break").chop(8).striate(4)"#),
    // --- mini-notation ---
    ("mini", "euclid", r#"s("bd(3,8) sd(5,8,2)")"#),
    ("mini", "alternation", r#"note("<c e g>")"#),
    ("mini", "polymeter", r#"s("{bd sd, hh hh hh}%4")"#),
    ("mini", "ranges + replicate", r#"n("0 .. 3 4!2")"#),
    ("mini", "subdivisions", r#"s("bd [sd sd] hh*3")"#),
    // --- tonal ---
    ("tonal", "scale", r#"n("0 1 2 3 4 5 6 7").scale("c:major")"#),
    ("tonal", "transpose", r#"note("c e g").transpose("<0 7>")"#),
    ("tonal", "scaleTranspose", r#"n("0 2 4").scale("a:minor").scaleTranspose(2)"#),
    ("tonal", "voicing", r#"chord("<C^7 Dm7>").voicing()"#),
    // --- xen ---
    ("xen", "edo", r#"i("0 8 18").xen("31edo")"#),
    ("xen", "ftrans", r#"note("c e g").ftrans("<0 7:31>")"#),
    ("xen", "edoScale", r#"n("0 1 2 3").edoScale("C:LLsLLLs:2:1")"#),
    // --- samples ---
    ("samples", "load + play", r#"samples("github:tidalcycles/dirt-samples")
s("bd sd")"#),
    ("samples", "bank + index", r#"s("bd:3 hh:1").bank("RolandTR909")"#),
    ("samples", "slice", r#"s("break").slice(8, "0 1 2 3")"#),
    // --- synths ---
    ("synths", "oscillator", r#"note("c e g").s("sawtooth").lpf(1200)"#),
    ("synths", "fm", r#"note("c").s("sine").fm(4).fmh(2)"#),
    ("synths", "adsr", r#"note("c e g").s("square").attack(0.01).release(0.3)"#),
    ("synths", "zzfx", r#"note("c e g").s("z_sawtooth")"#),
    // --- MIDI ---
    ("midi", "midi out", r#"note("c e g").midi()"#),
    ("midi", "cc + channel", r#"note("c").ccn(74).ccv(64).midichan(2)"#),
    // --- OSC ---
    ("osc", "superdirt osc", r#"s("bd sd").osc()"#),
    // --- visual feedback ---
    ("visual", "color control", r#"note("c e g").color("cyan")"#),
    ("visual", "pianoroll widget", r#"s("bd sd hh oh")._pianoroll()"#),
    ("visual", "punchcard widget", r#"n("0 2 4 7").s("piano")._punchcard()"#),
];

#[test]
fn parity_examples_all_evaluate() {
    let mut failures = Vec::new();
    let mut categories = std::collections::BTreeSet::new();
    for (category, label, src) in EXAMPLES {
        categories.insert(*category);
        match eval(src) {
            Ok(pat) => {
                // Querying must not panic; a cycle's worth of haps is a sanity
                // check that the pattern is structurally sound.
                let _ = pat.query_arc(Frac::zero(), Frac::one());
            }
            Err(e) => failures.push(format!("[{category}] {label}: {e}\n    src: {src}")),
        }
    }

    assert!(
        failures.is_empty(),
        "parity examples failed to evaluate:\n{}",
        failures.join("\n")
    );

    // Guard that every advertised category is represented, so the suite cannot
    // quietly lose coverage of an area.
    for expected in [
        "first-sounds",
        "notes",
        "effects",
        "pattern-fx",
        "mini",
        "tonal",
        "xen",
        "samples",
        "synths",
        "midi",
        "osc",
        "visual",
    ] {
        assert!(
            categories.contains(expected),
            "example suite is missing the {expected:?} category"
        );
    }
}
