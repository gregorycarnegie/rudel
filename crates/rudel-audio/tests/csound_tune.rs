//! A Csound tune, end to end: script text in, audio samples out.
//!
//! The unit tests either side of this cover the halves — `csound.rs` that the
//! library renders, `events.rs` that a hap becomes the right score statement —
//! and neither would notice if the mixer stopped joining them up. This runs the
//! path the two Csound tunes on strudel.cc/examples actually take: evaluate,
//! compile the orchestra the script carries, schedule the events, render.
//!
//! Csound is an optional runtime dependency, so this skips where it is not
//! installed. Set `RUDEL_CSOUND_REQUIRED=1` to make a skip a failure instead.

use rudel_audio::{OfflineMixer, collect_events, csound::Csound, samples::SampleBank};

/// The orchestra from the "CSound demo" tune, trimmed to what makes a tone:
/// the pfields are the ones `.csound()` sends, so this exercises the contract
/// rather than a toy of its own.
const ORC: &str = r#"
instr CoolSynth
    iduration = p3
    ifreq = p4
    igain = p5
    asig = vco2(igain, ifreq)
    asig *= linsegr:a(0, .01, 1, iduration, 1, .1, 0)
    out(asig, asig)
endin
"#;

fn csound() -> Option<Csound> {
    match Csound::new(44100.0) {
        Ok(cs) => Some(cs),
        Err(why) => {
            assert!(
                std::env::var("RUDEL_CSOUND_REQUIRED").is_err(),
                "RUDEL_CSOUND_REQUIRED is set but Csound did not start: {why}"
            );
            eprintln!("skipping: {why}");
            None
        }
    }
}

/// Render one cycle of `src` at 1 cps, with `ORC` loaded.
fn render(src: &str) -> Option<Vec<(f32, f32)>> {
    let mut cs = csound()?;
    cs.compile_orc(ORC).expect("compile the tune's orchestra");

    let pattern = rudel_lang::eval(src).expect("eval");
    let mut mixer = OfflineMixer::new(44100.0);
    mixer.set_csound(cs);
    for ev in collect_events(&pattern, 1.0, 0.0, 1.0, &SampleBank::new()) {
        mixer.schedule(ev);
    }
    let mut out = vec![(0.0f32, 0.0f32); 44100];
    mixer.render_block(&mut out);
    Some(out)
}

fn peak(frames: &[(f32, f32)]) -> f32 {
    frames.iter().fold(0.0f32, |m, (l, _)| m.max(l.abs()))
}

#[test]
fn a_csound_pattern_reaches_the_output() {
    let Some(out) = render(r#"note("c3 e3 g3 c4").csound('CoolSynth')"#) else {
        return;
    };
    assert!(
        peak(&out) > 0.01,
        "expected Csound audio, peak {}",
        peak(&out)
    );

    // Four notes in the cycle, so each quarter of the second carries sound —
    // a single note ringing on would pass a whole-buffer peak check.
    for (i, quarter) in out.chunks(11_025).enumerate() {
        assert!(
            peak(quarter) > 0.01,
            "quarter {i} is silent; the events are not landing on their onsets"
        );
    }
}

#[test]
fn a_csound_voice_replaces_the_synth_rather_than_doubling_it() {
    // Upstream's `.csound()` is an `onTrigger`, which takes the sound over. If
    // the hap also started a Rudel synth, every Csound note would play twice.
    let Some(with) = render(r#"note("c3").csound('CoolSynth')"#) else {
        return;
    };
    // The same pattern with no Csound instance at all: nothing should sound.
    let pattern = rudel_lang::eval(r#"note("c3").csound('CoolSynth')"#).expect("eval");
    let mut bare = OfflineMixer::new(44100.0);
    for ev in collect_events(&pattern, 1.0, 0.0, 1.0, &SampleBank::new()) {
        bare.schedule(ev);
    }
    let mut out = vec![(0.0f32, 0.0f32); 44100];
    bare.render_block(&mut out);

    assert!(peak(&with) > 0.01, "Csound should sound");
    assert_eq!(
        peak(&out),
        0.0,
        "without Csound the hap must be silent, not fall back to a synth"
    );
}

#[test]
fn other_layers_still_play_alongside_csound() {
    // Both tunes stack a Csound layer with ordinary Rudel voices, so the two
    // have to sum. Csound's own layer is not enough to prove that: the mixer
    // could be overwriting the master instead of adding to it.
    let Some(both) = render(r#"stack(note("c3").csound('CoolSynth'), note("c2").s('sawtooth'))"#)
    else {
        return;
    };
    let saw = render(r#"note("c2").s('sawtooth')"#).expect("csound was available above");
    assert!(
        peak(&saw) > 0.01,
        "the sawtooth layer should sound on its own"
    );
    assert!(
        both.iter().zip(&saw).any(|(a, b)| (a.0 - b.0).abs() > 1e-4),
        "the Csound layer is missing from the stack"
    );
}
