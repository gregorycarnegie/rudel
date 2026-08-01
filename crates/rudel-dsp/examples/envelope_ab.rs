//! Render the gain envelope before/after the `getADSRValues` fix, so the change
//! can be heard rather than read off a table.
//!
//!     cargo run -p rudel-dsp --example envelope_ab
//!
//! Writes one WAV per case into `target/envelope_ab/`. Each file plays the note
//! three times with the OLD resolution, a half-second of silence, then three
//! times with the NEW one, so you can A/B without switching files. `_old` and
//! `_new` versions of each are written separately too, for looping one of them.
//!
//! OLD is what rudel did before: start from the synth defaults and overwrite
//! only the stages a control names. NEW is superdough's `getADSRValues`, where
//! naming any stage re-defaults the rest.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_dsp::{Adsr, Voice, VoiceParams, Waveform, adsr_values};
use std::{fs, io::Write, path::Path};

const SAMPLE_RATE: f32 = 44100.0;
/// How long each note is held before its release.
const NOTE: f32 = 0.6;
/// Gap between repeats, and the longer gap between the old and new halves.
const GAP: f32 = 0.25;
const SECTION_GAP: f32 = 0.5;

/// The old behaviour: `Adsr::default()` with each named stage written over it.
fn old_resolution(a: Option<f32>, d: Option<f32>, s: Option<f32>, r: Option<f32>) -> Adsr {
    let mut adsr = Adsr::default();
    if let Some(a) = a {
        adsr.attack = a;
    }
    if let Some(d) = d {
        adsr.decay = d;
    }
    if let Some(s) = s {
        adsr.sustain = s;
    }
    if let Some(r) = r {
        adsr.release = r;
    }
    adsr
}

/// One note, rendered to its natural end.
fn note(adsr: Adsr, freq: f32) -> Vec<f32> {
    let mut v = Voice::new(
        VoiceParams {
            waveform: Waveform::Saw,
            freq,
            adsr,
            duration: NOTE,
            ..Default::default()
        },
        SAMPLE_RATE,
    );
    let mut out = Vec::new();
    for _ in 0..(SAMPLE_RATE * 4.0) as usize {
        let (l, _r) = v.tick();
        out.push(l);
        if v.is_done() {
            break;
        }
    }
    out
}

fn silence(seconds: f32) -> Vec<f32> {
    vec![0.0; (SAMPLE_RATE * seconds) as usize]
}

/// Three repeats of a note at a musical interval, so the envelope shape is
/// audible as a rhythm rather than a single event.
fn phrase(adsr: Adsr) -> Vec<f32> {
    let mut out = Vec::new();
    for freq in [220.0, 277.18, 329.63] {
        out.extend(note(adsr, freq));
        out.extend(silence(GAP));
    }
    out
}

fn write_wav(path: &Path, samples: &[f32]) {
    // 16-bit mono PCM. Hand-rolled so the example needs no encoder dependency.
    let n = samples.len() as u32;
    let data_len = n * 2;
    let mut f = fs::File::create(path).expect("create wav");
    let mut w = |b: &[u8]| f.write_all(b).expect("write wav");
    w(b"RIFF");
    w(&(36 + data_len).to_le_bytes());
    w(b"WAVEfmt ");
    w(&16u32.to_le_bytes()); // fmt chunk size
    w(&1u16.to_le_bytes()); // PCM
    w(&1u16.to_le_bytes()); // mono
    w(&(SAMPLE_RATE as u32).to_le_bytes());
    w(&(SAMPLE_RATE as u32 * 2).to_le_bytes()); // byte rate
    w(&2u16.to_le_bytes()); // block align
    w(&16u16.to_le_bytes()); // bits per sample
    w(b"data");
    w(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        w(&v.to_le_bytes());
    }
}

/// `(name, attack, decay, sustain, release)` — the controls a pattern names,
/// with `None` for the ones it leaves alone.
type Case = (&'static str, Opt, Opt, Opt, Opt);
type Opt = Option<f32>;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/envelope_ab")
        .canonicalize()
        .unwrap_or_else(|_| {
            let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/envelope_ab");
            fs::create_dir_all(&p).expect("create output dir");
            p
        });
    fs::create_dir_all(&dir).expect("create output dir");

    let cases: [Case; 5] = [
        // The headline case: `.attack(0.1)` alone.
        ("attack_only", Some(0.1), None, None, None),
        // A slow pad: `.attack(0.3).release(0.5)`.
        ("attack_and_release", Some(0.3), None, None, Some(0.5)),
        // Percussive: `.decay(0.15)` with nothing else.
        ("decay_only", None, Some(0.15), None, None),
        // A release under the 10ms floor, which upstream raises.
        ("tiny_release", None, None, None, Some(0.001)),
        // Everything named: both resolutions agree, so this is the control.
        (
            "all_four_named",
            Some(0.05),
            Some(0.1),
            Some(0.5),
            Some(0.2),
        ),
    ];

    println!("writing to {}\n", dir.display());
    for (name, a, d, s, r) in cases {
        let old = old_resolution(a, d, s, r);
        let new = adsr_values(a, d, s, r, Adsr::default());
        let same = (old.attack - new.attack).abs() < 1e-9
            && (old.decay - new.decay).abs() < 1e-9
            && (old.sustain - new.sustain).abs() < 1e-9
            && (old.release - new.release).abs() < 1e-9;

        println!("{name}{}", if same { "  (identical)" } else { "" });
        println!(
            "  old  a={:.4} d={:.4} s={:.4} r={:.4}",
            old.attack, old.decay, old.sustain, old.release
        );
        println!(
            "  new  a={:.4} d={:.4} s={:.4} r={:.4}",
            new.attack, new.decay, new.sustain, new.release
        );

        let (old_audio, new_audio) = (phrase(old), phrase(new));
        write_wav(&dir.join(format!("{name}_old.wav")), &old_audio);
        write_wav(&dir.join(format!("{name}_new.wav")), &new_audio);

        let mut both = old_audio;
        both.extend(silence(SECTION_GAP));
        both.extend(new_audio);
        write_wav(&dir.join(format!("{name}_ab.wav")), &both);
    }
    println!("\n*_ab.wav plays old, then new. Order is always old first.");
}
