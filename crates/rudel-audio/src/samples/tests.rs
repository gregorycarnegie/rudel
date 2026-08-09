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
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
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

/// Build a WAV around a raw data payload. `fmt_extra` extends the fmt chunk
/// (so a WAVE_FORMAT_EXTENSIBLE sub-format GUID can be supplied) and `junk`
/// puts an odd-sized chunk before it, which the chunk walker must word-align
/// past to find `fmt ` at all.
fn wav(
    tag: u16,
    bits: u16,
    channels: u16,
    rate: u32,
    data: &[u8],
    fmt_extra: &[u8],
    junk: bool,
) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(b"RIFF");
    let riff_size = 4 + 8 + 16 + fmt_extra.len() + 8 + data.len() + if junk { 12 } else { 0 };
    b.extend((riff_size as u32).to_le_bytes());
    b.extend(b"WAVE");
    if junk {
        b.extend(b"JUNK");
        b.extend(3u32.to_le_bytes());
        b.extend([0xAA, 0xBB, 0xCC]);
        b.push(0); // pad to a word boundary
    }
    let block = channels * bits / 8;
    b.extend(b"fmt ");
    b.extend((16u32 + fmt_extra.len() as u32).to_le_bytes());
    b.extend(tag.to_le_bytes());
    b.extend(channels.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * block as u32).to_le_bytes());
    b.extend(block.to_le_bytes());
    b.extend(bits.to_le_bytes());
    b.extend(fmt_extra);
    b.extend(b"data");
    b.extend((data.len() as u32).to_le_bytes());
    b.extend(data);
    b
}

/// Every PCM/float width the lenient reader claims to handle, decoded to exact
/// values. Old sample packs really do ship all of these, and a wrong shift or
/// divisor here is silently a quieter or louder sample rather than an error.
#[test]
fn lenient_decoder_scales_every_supported_sample_width() {
    let half = |s: &Sample| s.data[1];
    // 8-bit PCM is unsigned, biased by 128.
    let eight = decode_wav_lenient(&wav(1, 8, 1, 22050, &[0, 192, 255], &[], false)).unwrap();
    assert_eq!(eight.sample_rate, 22050.0);
    assert_eq!(eight.data, vec![-1.0, 0.5, 127.0 / 128.0]);

    // The rest are signed, so half-scale is the telling value: a wrong divisor
    // or shift moves it, where 0.0 and full-scale would not.
    let s16 = decode_wav_lenient(&wav(1, 16, 1, 44100, &[0, 0, 0, 0x40], &[], false)).unwrap();
    assert_eq!(half(&s16), 0.5);

    let s24 =
        decode_wav_lenient(&wav(1, 24, 1, 44100, &[0, 0, 0, 0, 0, 0x40], &[], false)).unwrap();
    assert_eq!(half(&s24), 0.5);

    let s32 = decode_wav_lenient(&wav(1, 32, 1, 44100, [0; 4].as_slice(), &[], false)).unwrap();
    assert_eq!(s32.data, vec![0.0]);
    let mut d = vec![0u8; 4];
    d.extend(0x4000_0000i32.to_le_bytes());
    let s32 = decode_wav_lenient(&wav(1, 32, 1, 44100, &d, &[], false)).unwrap();
    assert_eq!(half(&s32), 0.5);

    let mut f32d = 0.0f32.to_le_bytes().to_vec();
    f32d.extend(0.25f32.to_le_bytes());
    assert_eq!(
        half(&decode_wav_lenient(&wav(3, 32, 1, 44100, &f32d, &[], false)).unwrap()),
        0.25
    );
    let mut f64d = 0.0f64.to_le_bytes().to_vec();
    f64d.extend(0.75f64.to_le_bytes());
    assert_eq!(
        half(&decode_wav_lenient(&wav(3, 64, 1, 44100, &f64d, &[], false)).unwrap()),
        0.75
    );

    // An unsupported width is an error, not silence.
    assert!(decode_wav_lenient(&wav(1, 12, 1, 44100, &[0; 4], &[], false)).is_err());
    // A fmt chunk too short to hold a format (`body.len() >= 16`) is skipped,
    // so the file reads as having no fmt chunk rather than panicking on it.
    let mut stub = b"RIFF".to_vec();
    stub.extend(0u32.to_le_bytes());
    stub.extend(b"WAVE");
    stub.extend(b"fmt ");
    stub.extend(8u32.to_le_bytes());
    stub.extend([0u8; 8]);
    assert!(decode_wav_lenient(&stub).is_err());
}

/// The chunk walker has to skip unknown chunks with word alignment, and read
/// WAVE_FORMAT_EXTENSIBLE's real format out of the sub-format GUID.
#[test]
fn lenient_decoder_walks_odd_chunks_and_unwraps_extensible() {
    // 22 bytes of extension: cbSize, validBits, channelMask, then a GUID whose
    // first word is the real format tag (1 = PCM).
    let mut ext = Vec::new();
    ext.extend(22u16.to_le_bytes());
    ext.extend(16u16.to_le_bytes());
    ext.extend(3u32.to_le_bytes());
    ext.extend(1u16.to_le_bytes()); // sub-format tag
    ext.extend([0u8; 14]);

    let s = decode_wav_lenient(&wav(0xFFFE, 16, 1, 44100, &[0, 0x40], &ext, true)).unwrap();
    assert_eq!(s.data, vec![0.5]);
    // Without the GUID lookup 0xFFFE is not a format the decoder knows.
    assert!(decode_wav_lenient(&wav(0xFFFE, 16, 1, 44100, &[0, 0x40], &[], false)).is_err());
}

/// Channels are averaged, not summed — and the two sides must differ, or a
/// wrong divisor is invisible.
#[test]
fn lenient_decoder_averages_asymmetric_channels() {
    let mut d = Vec::new();
    for (l, r) in [(1.0f32, 0.0f32), (0.5, 0.25)] {
        d.extend(l.to_le_bytes());
        d.extend(r.to_le_bytes());
    }
    let s = decode_wav_lenient(&wav(3, 32, 2, 44100, &d, &[], false)).unwrap();
    assert_eq!(s.data, vec![0.5, 0.375]);
}

/// A chunk body is bounded by its declared size, not by the rest of the file:
/// a `data` chunk followed by anything else must not swallow it.
#[test]
fn lenient_decoder_stops_a_chunk_at_its_declared_size() {
    let mut d = Vec::new();
    for v in [0.0f32, 0.5] {
        d.extend(v.to_le_bytes());
    }
    let mut b = wav(3, 32, 1, 44100, &d, &[], false);
    // A trailing chunk after `data` — real files carry `LIST`/`id3 ` here.
    b.extend(b"LIST");
    b.extend(16u32.to_le_bytes());
    b.extend([0x7Fu8; 16]);

    let s = decode_wav_lenient(&b).unwrap();
    assert_eq!(s.data, vec![0.0, 0.5], "the trailing chunk is not audio");
}

/// The symphonia path averages channels too, and it is the one nearly every
/// real sample takes — so it needs its own asymmetric-stereo check.
#[test]
fn the_normal_decode_path_averages_channels() {
    // Standard 16-bit stereo PCM: symphonia accepts this, so `wave_to_sample`
    // does the mixdown rather than the lenient fallback.
    let frames = [(1.0f32, 0.0f32), (0.5, 0.0), (0.25, 0.25)];
    let mut d = Vec::new();
    for (l, r) in frames {
        d.extend(((l * 32767.0) as i16).to_le_bytes());
        d.extend(((r * 32767.0) as i16).to_le_bytes());
    }
    let s = decode_sample_bytes(wav(1, 16, 2, 44100, &d, &[], false)).expect("decodes");
    assert_eq!(s.sample_rate, 44100.0);
    assert_eq!(s.data.len(), 3);
    for (got, want) in s.data.iter().zip([0.5f32, 0.25, 0.25]) {
        assert!((got - want).abs() < 1e-3, "{got} != {want}");
    }
}

/// The lenient fallback is only for things that really are WAVs; anything else
/// keeps symphonia's own error rather than adding a confusing second one.
#[test]
fn only_riff_wave_bytes_fall_back_to_the_lenient_decoder() {
    let Err(err) = decode_sample_bytes(b"not audio at all".to_vec()) else {
        panic!("garbage should not decode")
    };
    assert!(!err.contains("lenient wav"), "{err}");
    // RIFF, but not a WAVE: still not ours.
    let mut riff_avi = b"RIFF".to_vec();
    riff_avi.extend(0u32.to_le_bytes());
    riff_avi.extend(b"AVI ");
    riff_avi.extend([0u8; 32]);
    let Err(err) = decode_sample_bytes(riff_avi) else {
        panic!("a RIFF/AVI container should not decode")
    };
    assert!(!err.contains("lenient wav"), "{err}");
}

#[test]
fn load_sample_map_reads_local_files() {
    // A strudel.json-style map whose files live in a local base directory.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
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
}

#[test]
fn load_samples_source_loads_a_local_json_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_wav(&root.join("kick.wav"), &[0.4; 32], 44100);
    let json_path = root.join("strudel.json");
    std::fs::write(&json_path, r#"{ "bd": "kick.wav" }"#).unwrap();

    let mut bank = SampleBank::new();
    let count = bank
        .load_samples_source(json_path.to_str().unwrap())
        .expect("load source");
    assert_eq!(count, 1);
    assert!((bank.get("bd", 0).unwrap().data[0] - 0.4).abs() < 1e-3);
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
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sound_dir = root.join("tone");
    std::fs::create_dir_all(&sound_dir).unwrap();
    write_wav(&sound_dir.join("02.wav"), &[0.2; 16], 44100);
    write_wav(&sound_dir.join("01.wav"), &[0.1; 16], 44100);

    let mut bank = SampleBank::new();
    let count = bank.load_dir(root).expect("load sample dir");
    assert_eq!(count, 2);
    assert!((bank.get("tone", 0).unwrap().data[0] - 0.1).abs() < 1e-4);
    assert!((bank.get("tone", 1).unwrap().data[0] - 0.2).abs() < 1e-4);
}

#[test]
fn registering_a_map_defers_the_audio_until_something_plays_it() {
    // Strudel's prebake registers a map's names and lets the browser fetch a
    // file only when it is first played. Loading every bank in full instead
    // measured at 3.1 GB and about nine minutes for the seven maps the REPL
    // preloads, nearly all of it audio nobody asked to hear.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_wav(&root.join("a.wav"), &[0.1; 32], 44100);
    write_wav(&root.join("b.wav"), &[0.2; 32], 44100);
    let map = root.join("strudel.json");
    std::fs::write(&map, r#"{ "bd": ["a.wav", "b.wav"], "sd": "b.wav" }"#).unwrap();

    let mut bank = SampleBank::new();
    let sounds = bank
        .register_samples_source(map.to_str().unwrap())
        .expect("register map");
    assert_eq!(sounds, 2, "both sounds are known");

    // Known and offered for completion, but nothing is decoded yet.
    assert!(bank.contains("bd"));
    assert_eq!(bank.names(), vec!["bd".to_string(), "sd".to_string()]);
    let _ = take_sample_requests(); // clear anything an earlier test left

    // Playing it comes up empty *and* records the miss for the host to fetch.
    assert!(bank.resolve("bd", 0, None).is_none());
    assert_eq!(take_sample_requests(), vec!["bd".to_string()]);

    // Once fetched, it plays and stops being pending.
    assert_eq!(bank.load_pending("bd").expect("load pending"), 2);
    assert_eq!(bank.get("bd", 0).unwrap().data.len(), 32);
    assert!((bank.get("bd", 1).unwrap().data[0] - 0.2).abs() < 1e-3);
    assert!(bank.pending_files("bd").is_none());
    let _ = take_sample_requests();
    assert!(bank.resolve("bd", 0, None).is_some());
    assert!(
        take_sample_requests().is_empty(),
        "a loaded sound must not keep asking to be fetched"
    );

    // A second load is a no-op rather than an error.
    assert_eq!(bank.load_pending("bd").expect("already loaded"), 0);
}
