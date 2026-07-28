use super::decoding::{decode_sample_bytes, decode_wav_lenient};
use super::loading::{expand_home, fetch_and_decode, fetch_text};
use super::*;
use crate::sample_map;
use std::f32::consts::TAU;
use std::path::Path;

/// Write a minimal 16-bit mono PCM WAV so we can exercise the real decoder.
fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) {
    use std::io::Write;
    let mut f = std::fs::File::create(path).unwrap();
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = sample_rate * 2;
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
    f.write_all(b"WAVE").unwrap();
    f.write_all(b"fmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
    f.write_all(&1u16.to_le_bytes()).unwrap(); // mono
    f.write_all(&sample_rate.to_le_bytes()).unwrap();
    f.write_all(&byte_rate.to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap(); // block align
    f.write_all(&16u16.to_le_bytes()).unwrap(); // bits
    f.write_all(b"data").unwrap();
    f.write_all(&data_len.to_le_bytes()).unwrap();
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        f.write_all(&v.to_le_bytes()).unwrap();
    }
}

#[test]
fn loads_a_wav_file() {
    let dir = std::env::temp_dir().join("rudel_sample_test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tone.wav");
    let samples: Vec<f32> = (0..4410)
        .map(|i| (TAU * 220.0 * i as f32 / 44100.0).sin())
        .collect();
    write_wav(&path, &samples, 44100);

    let mut bank = SampleBank::new();
    bank.load_file("tone", &path).expect("load wav");
    let s = bank.get("tone", 0).expect("sample present");
    assert_eq!(s.sample_rate, 44100.0);
    assert!(s.data.len() > 4000);
    assert!(s.data.iter().any(|&x| x.abs() > 0.1));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn decodes_wav_with_nonstandard_fmt_chunk() {
    // 20-byte PCM fmt chunk (16 + 4 junk bytes), as found in dirt-samples'
    // "mute"/"pluck" banks; symphonia rejects it, the lenient fallback must not.
    let samples: Vec<f32> = (0..64).map(|i| (TAU * i as f32 / 64.0).sin()).collect();
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((40 + data_len).to_le_bytes());
    b.extend(b"WAVE");
    b.extend(b"fmt ");
    b.extend(20u32.to_le_bytes());
    b.extend(1u16.to_le_bytes()); // PCM
    b.extend(1u16.to_le_bytes()); // mono
    b.extend(44100u32.to_le_bytes());
    b.extend((44100u32 * 2).to_le_bytes());
    b.extend(2u16.to_le_bytes()); // block align
    b.extend(16u16.to_le_bytes()); // bits
    b.extend([0u8; 4]); // the nonstandard trailing bytes
    b.extend(b"data");
    b.extend(data_len.to_le_bytes());
    for &s in &samples {
        b.extend(((s * 32767.0) as i16).to_le_bytes());
    }

    let s = decode_sample_bytes(b).expect("lenient wav fallback decodes");
    assert_eq!(s.sample_rate, 44100.0);
    assert_eq!(s.data.len(), 64);
    assert!((s.data[16] - 1.0).abs() < 1e-2); // sin peak survives roundtrip
}

#[test]
fn lenient_decoder_handles_stereo_float32() {
    // Exercises the IEEE-float branch and channel averaging directly.
    let frames: Vec<(f32, f32)> = (0..32)
        .map(|i| (i as f32 / 32.0, -(i as f32) / 32.0))
        .collect();
    let data_len = (frames.len() * 8) as u32;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + data_len).to_le_bytes());
    b.extend(b"WAVE");
    b.extend(b"fmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(3u16.to_le_bytes()); // IEEE float
    b.extend(2u16.to_le_bytes()); // stereo
    b.extend(48000u32.to_le_bytes());
    b.extend((48000u32 * 8).to_le_bytes());
    b.extend(8u16.to_le_bytes()); // block align
    b.extend(32u16.to_le_bytes()); // bits
    b.extend(b"data");
    b.extend(data_len.to_le_bytes());
    for &(l, r) in &frames {
        b.extend(l.to_le_bytes());
        b.extend(r.to_le_bytes());
    }

    let s = decode_wav_lenient(&b).expect("float32 stereo decodes");
    assert_eq!(s.sample_rate, 48000.0);
    assert_eq!(s.data.len(), 32);
    // L and R are mirrored, so the mono average is ~0 everywhere.
    assert!(s.data.iter().all(|&x| x.abs() < 1e-6));
}

#[test]
fn load_sample_map_reads_local_files() {
    // A strudel.json-style map whose files live in a local base directory.
    let root = std::env::temp_dir().join(format!("rudel_map_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    write_wav(&root.join("a.wav"), &[0.1; 32], 44100);
    write_wav(&root.join("b.wav"), &[0.2; 32], 44100);
    write_wav(&root.join("c.wav"), &[0.3; 32], 44100);

    let json = r#"{ "bd": ["a.wav", "b.wav"], "sd": "c.wav" }"#;
    let base = root.to_str().unwrap();

    let mut bank = SampleBank::new();
    let count = bank.load_sample_map(json, base).expect("load map");
    assert_eq!(count, 3);
    assert_eq!(bank.get("bd", 0).unwrap().data.len(), 32);
    assert!((bank.get("bd", 1).unwrap().data[0] - 0.2).abs() < 1e-3);
    assert!((bank.get("sd", 0).unwrap().data[0] - 0.3).abs() < 1e-3);
    assert!(bank.get("bd", 2).is_some()); // index wraps over the 2 bd samples

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn load_samples_source_loads_a_local_json_file() {
    let root = std::env::temp_dir().join(format!("rudel_src_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    write_wav(&root.join("kick.wav"), &[0.4; 32], 44100);
    let json_path = root.join("strudel.json");
    std::fs::write(&json_path, r#"{ "bd": "kick.wav" }"#).unwrap();

    let mut bank = SampleBank::new();
    let count = bank
        .load_samples_source(json_path.to_str().unwrap())
        .expect("load source");
    assert_eq!(count, 1);
    assert!((bank.get("bd", 0).unwrap().data[0] - 0.4).abs() < 1e-3);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "hits the network (github.com)"]
fn fetches_parses_and_decodes_a_real_github_pack() {
    // End-to-end smoke test of the ureq fetch + JSON parse + remote decode
    // path against a real repo. Run with `--ignored`. Decodes exactly one
    // file (not the whole pack) to keep it light.
    let url = sample_map::github_path("github:tidalcycles/dirt-samples", "strudel.json").unwrap();
    let json = fetch_text(&url).expect("fetch strudel.json");
    let base = sample_map::base_url_of(&url);
    let entries = sample_map::parse_sample_map(&json, &base).expect("parse map");
    assert!(entries.len() > 10, "expected many sounds in the pack");

    let (_, files) = entries
        .iter()
        .find(|(name, _)| name == "bd")
        .expect("a `bd` sound");
    let url = match files {
        sample_map::SoundFiles::Flat(urls) => urls.first().expect("bd files"),
        sample_map::SoundFiles::Pitched(groups) => &groups.first().expect("bd groups").1[0],
    };
    let sample = fetch_and_decode(url).expect("fetch + decode one sample");
    assert!(!sample.data.is_empty(), "decoded sample should have audio");
}

#[test]
fn expand_home_replaces_leading_tilde() {
    // SAFETY: single-threaded test; we set HOME for the duration of the call.
    unsafe { std::env::set_var("HOME", "/home/me") };
    assert_eq!(expand_home("~/samples"), "/home/me/samples");
    assert_eq!(expand_home("~"), "/home/me");
    assert_eq!(expand_home("/abs/path"), "/abs/path");
    assert_eq!(expand_home("relative/path"), "relative/path");
}

#[test]
fn index_wraps() {
    let mut bank = SampleBank::new();
    let mk = |v: f32| {
        Arc::new(Sample {
            data: vec![v],
            sample_rate: 44100.0,
        })
    };
    bank.register("bd", mk(0.1));
    bank.register("bd", mk(0.2));
    assert_eq!(bank.get("bd", 0).unwrap().data[0], 0.1);
    assert_eq!(bank.get("bd", 1).unwrap().data[0], 0.2);
    assert_eq!(bank.get("bd", 2).unwrap().data[0], 0.1); // wraps
    // negative indices count from the end (superdough's `_mod`).
    assert_eq!(bank.get("bd", -1).unwrap().data[0], 0.2);
    assert_eq!(bank.get("bd", -2).unwrap().data[0], 0.1);
    assert!(bank.get("missing", 0).is_none());
}

fn mk(v: f32) -> Arc<Sample> {
    Arc::new(Sample {
        data: vec![v],
        sample_rate: 44100.0,
    })
}

#[test]
fn resolve_picks_the_closest_pitched_group() {
    let mut bank = SampleBank::new();
    bank.register_note("piano", 60, mk(0.60)); // c4
    bank.register_note("piano", 64, mk(0.64)); // e4

    // midi 63 -> e4 is closest (dist 1), repitch down one semitone
    let (s, t) = bank.resolve("piano", 0, Some(63.0)).unwrap();
    assert_eq!(s.data[0], 0.64);
    assert_eq!(t, -1.0);

    // midi 61 -> c4 is closest, repitch up one semitone
    let (s, t) = bank.resolve("piano", 0, Some(61.0)).unwrap();
    assert_eq!(s.data[0], 0.60);
    assert_eq!(t, 1.0);

    // no note -> fall back to C3 (36) target -> nearest is c4 (60)
    let (s, t) = bank.resolve("piano", 0, None).unwrap();
    assert_eq!(s.data[0], 0.60);
    assert_eq!(t, 36.0 - 60.0);
}

#[test]
fn registering_publishes_the_duration_getduration_reads() {
    let mut bank = SampleBank::new();
    // 22050 frames at 44.1kHz == half a second; the second one is a full
    // second, and its index matches the `n` `resolve` would use.
    bank.register(
        "dur_test",
        Arc::new(Sample {
            data: vec![0.0; 22050],
            sample_rate: 44100.0,
        }),
    );
    bank.register(
        "dur_test",
        Arc::new(Sample {
            data: vec![0.0; 44100],
            sample_rate: 44100.0,
        }),
    );
    assert_eq!(rudel_core::sample_duration("dur_test", 0), Some(0.5));
    assert_eq!(rudel_core::sample_duration("dur_test", 1), Some(1.0));
    assert_eq!(rudel_core::sample_duration("dur_test", 2), None);
}

#[test]
fn flat_sound_repitches_only_when_a_note_is_set() {
    let mut bank = SampleBank::new();
    bank.register("bd", mk(0.5));
    // no note -> no repitch
    assert_eq!(bank.resolve("bd", 0, None).unwrap().1, 0.0);
    // baseline is MIDI 36 (C2); note 36 -> 0, note 48 (C3) -> +12 semitones
    assert_eq!(bank.resolve("bd", 0, Some(36.0)).unwrap().1, 0.0);
    assert_eq!(bank.resolve("bd", 0, Some(48.0)).unwrap().1, 12.0);
}

#[test]
fn load_dir_keeps_sorted_sample_indices() {
    let root = std::env::temp_dir().join(format!("rudel_sample_dir_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let sound_dir = root.join("tone");
    std::fs::create_dir_all(&sound_dir).unwrap();
    write_wav(&sound_dir.join("02.wav"), &[0.2; 16], 44100);
    write_wav(&sound_dir.join("01.wav"), &[0.1; 16], 44100);

    let mut bank = SampleBank::new();
    let count = bank.load_dir(&root).expect("load sample dir");
    assert_eq!(count, 2);
    assert!((bank.get("tone", 0).unwrap().data[0] - 0.1).abs() < 1e-4);
    assert!((bank.get("tone", 1).unwrap().data[0] - 0.2).abs() < 1e-4);

    let _ = std::fs::remove_dir_all(&root);
}
