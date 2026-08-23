//! Recording the master output to a file.
//!
//! The audio thread hands finished blocks to a writer thread, which encodes
//! them straight to disk — nothing is buffered in memory for the length of the
//! take, so a recording is bounded by the file system rather than by RAM.
//! The formats themselves live in [`encode`].
//! SPDX-License-Identifier: AGPL-3.0-or-later

mod encode;

pub use encode::Format;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
};

/// A running take: the file being written and the thread writing it.
type Take = (PathBuf, JoinHandle<Result<(), String>>);

/// Shared between the audio thread (which pushes blocks) and the UI thread
/// (which starts and stops takes).
#[derive(Default)]
pub struct Recorder {
    /// Read by the audio thread on every block; the only thing it touches when
    /// no recording is running.
    armed: AtomicBool,
    /// `try_lock`ed by the audio thread, like the Csound handle: the only other
    /// holder is a start/stop on the UI thread, and waiting for that would drop
    /// a buffer.
    tx: Mutex<Option<Sender<Vec<f32>>>>,
    /// The writer thread and where it is writing, owned by the control side.
    writer: Mutex<Option<Take>>,
}

impl Recorder {
    /// Whether a take is running.
    pub fn is_recording(&self) -> bool {
        self.armed.load(Ordering::Relaxed)
    }

    /// The file the current take is being written to.
    pub fn path(&self) -> Option<PathBuf> {
        let guard = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(|(p, _)| p.clone())
    }

    /// Begin a take at `path`, in the format its extension names. Errors if one
    /// is already running, if the extension names no format, or if the encoder
    /// cannot open the file — the failure surfaces here rather than at stop, so
    /// the button never lights up on a recording that was never going to work.
    pub fn start(&self, path: impl AsRef<Path>, sample_rate: f32) -> Result<(), String> {
        let path = path.as_ref().to_path_buf();
        let format = Format::from_path(&path).ok_or_else(|| {
            format!(
                "{}: rudel records {}",
                path.display(),
                Format::EXTENSIONS.join("/")
            )
        })?;
        let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        if writer.is_some() {
            return Err("already recording".to_string());
        }

        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        // The encoders are built on the writer thread — several of them wrap a
        // C handle that is not `Send` — so the thread reports back whether it
        // opened before `start` returns.
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let rate = sample_rate.max(1.0) as u32;
        let on_thread = path.clone();
        let handle = std::thread::spawn(move || run(format, on_thread, rate, rx, ready_tx));
        ready_rx
            .recv()
            .map_err(|_| "the recording thread died".to_string())??;

        *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        *writer = Some((path, handle));
        // Armed last: the audio thread must never see the flag before the
        // channel it is meant to send on.
        self.armed.store(true, Ordering::Release);
        Ok(())
    }

    /// End the take and wait for the file to be closed. Returns the path
    /// written, or `None` if nothing was recording.
    pub fn stop(&self) -> Result<Option<PathBuf>, String> {
        // Disarmed first, so the audio thread stops sending before the channel
        // goes away.
        self.armed.store(false, Ordering::Release);
        let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        let Some((path, handle)) = writer.take() else {
            return Ok(None);
        };
        // Dropping the sender is what tells the writer thread the take is over.
        *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        match handle.join() {
            Ok(Ok(())) => Ok(Some(path)),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the recording thread panicked".to_string()),
        }
    }

    /// Hand one rendered block to the writer. Called from the audio thread on
    /// every callback; a no-op — one relaxed load — when not recording.
    ///
    // ponytail: interleaving allocates a `Vec` per block on the audio thread
    // while a take is running, and `Sender::send` allocates its node. That is
    // the same trade the reverb build already makes here, and it only happens
    // while recording. Recycle the buffers through a return channel if a take
    // ever glitches.
    pub(crate) fn push(&self, frames: &[(f32, f32)]) {
        if !self.armed.load(Ordering::Acquire) {
            return;
        }
        let Ok(guard) = self.tx.try_lock() else {
            return; // a start/stop is in flight; skipping one block beats a drop-out
        };
        let Some(tx) = guard.as_ref() else {
            return;
        };
        let mut block = Vec::with_capacity(frames.len() * 2);
        for &(l, r) in frames {
            block.push(l);
            block.push(r);
        }
        let _ = tx.send(block);
    }
}

/// The writer thread: open the encoder, report whether that worked, then
/// encode blocks until the sender is dropped.
fn run(
    format: Format,
    path: PathBuf,
    sample_rate: u32,
    rx: Receiver<Vec<f32>>,
    ready: Sender<Result<(), String>>,
) -> Result<(), String> {
    let mut encoder = match format.open(&path, sample_rate) {
        Ok(encoder) => {
            let _ = ready.send(Ok(()));
            encoder
        }
        Err(e) => {
            let _ = ready.send(Err(e.clone()));
            return Err(e);
        }
    };
    for block in rx {
        encoder.write(&block)?;
    }
    encoder.finish()
}

/// Shared handle type used by the mixer and the engine.
pub(crate) type SharedRecorder = Arc<Recorder>;

#[cfg(test)]
mod tests {
    use super::*;
    use encode::{to_i16, wav_header};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rudel-rec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A second of a 440 Hz tone, interleaved stereo, as the mixer would hand
    /// it over: real signal, so an encoder that drops it is visible in the
    /// output size.
    fn tone(sample_rate: u32, seconds: u32) -> Vec<(f32, f32)> {
        (0..sample_rate * seconds)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
                // Not equal channels: a mixdown to mono would hide a codec
                // that dropped one of them, but not one that swapped them.
                (s, s * 0.8)
            })
            .collect()
    }

    /// Record `tone` to `name` and return the bytes written.
    fn take(name: &str, sample_rate: u32) -> Vec<u8> {
        let path = temp_dir().join(name);
        let rec = Recorder::default();
        rec.start(&path, sample_rate as f32)
            .unwrap_or_else(|e| panic!("start {name}: {e}"));
        for chunk in tone(sample_rate, 1).chunks(1024) {
            rec.push(chunk);
        }
        rec.stop().unwrap_or_else(|e| panic!("stop {name}: {e}"));
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();
        bytes
    }

    #[test]
    fn an_extension_picks_the_format() {
        let of = |name: &str| Format::from_path(Path::new(name));
        assert_eq!(of("take.wav"), Some(Format::Wav));
        assert_eq!(of("take.MP3"), Some(Format::Mp3), "case does not matter");
        assert_eq!(of("take.flac"), Some(Format::Flac));
        assert_eq!(of("take.ogg"), Some(Format::Vorbis));
        assert_eq!(of("take.opus"), Some(Format::Opus));
        assert_eq!(of("take.aiff"), None, "a format rudel cannot write");
        assert_eq!(of("take"), None, "no extension at all");
    }

    #[test]
    fn an_unwritable_format_is_refused_at_start() {
        let rec = Recorder::default();
        let err = rec
            .start(temp_dir().join("take.aiff"), 48_000.0)
            .unwrap_err();
        assert!(
            err.contains("wav"),
            "the error should list what works: {err}"
        );
        assert!(!rec.is_recording(), "a refused start must not arm");
    }

    #[test]
    fn a_wav_take_round_trips_with_patched_lengths() {
        let path = temp_dir().join("take.wav");
        let rec = Recorder::default();
        rec.start(&path, 48_000.0).unwrap();
        assert!(rec.is_recording());
        assert_eq!(rec.path().as_deref(), Some(path.as_path()));
        rec.push(&[(1.0, -1.0), (0.0, 0.5)]);
        assert_eq!(rec.stop().unwrap().as_deref(), Some(path.as_path()));
        assert!(!rec.is_recording());

        let bytes = std::fs::read(&path).unwrap();
        // Two stereo frames: 44 header bytes plus 4 samples of 16-bit PCM.
        assert_eq!(bytes.len(), 44 + 8, "header plus four samples");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 36 + 8);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8);
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            48_000,
            "the sample rate is the one the take was started at"
        );
        // Full scale, then its inverse, then silence and a half.
        let sample =
            |i: usize| i16::from_le_bytes(bytes[44 + i * 2..46 + i * 2].try_into().unwrap());
        assert_eq!(sample(0), i16::MAX);
        assert_eq!(sample(1), -i16::MAX);
        assert_eq!(sample(2), 0);
        assert_eq!(sample(3), i16::MAX / 2);

        std::fs::remove_file(&path).ok();
    }

    /// Decode a take back through the same symphonia path rudel loads samples
    /// with, and report `(sample_rate, seconds, rms)` of the mono mixdown.
    /// Magic bytes only prove a header; this proves a decoder gets audio out.
    fn decoded(bytes: &[u8]) -> (f32, f32, f32) {
        let sample = crate::samples::decode_bytes(bytes).expect("the take should decode");
        let rms =
            (sample.data.iter().map(|s| s * s).sum::<f32>() / sample.data.len() as f32).sqrt();
        let seconds = sample.data.len() as f32 / sample.sample_rate;
        (sample.sample_rate, seconds, rms)
    }

    #[test]
    fn every_format_decodes_back_to_the_second_of_audio_it_was_given() {
        // symphonia reads WAV, FLAC, MP3 and Vorbis; it has no Opus decoder,
        // which is checked by its container instead below.
        for name in ["tone.wav", "tone.flac", "tone.mp3", "tone.ogg"] {
            let (rate, seconds, rms) = decoded(&take(name, 48_000));
            assert_eq!(rate, 48_000.0, "{name} decoded at the wrong rate");
            assert!(
                (seconds - 1.0).abs() < 0.1,
                "{name} decoded to {seconds}s, not the second it was given"
            );
            // The tone is 0.5 amplitude; anything in this band is the signal
            // rather than silence or noise.
            assert!(
                (0.1..0.5).contains(&rms),
                "{name} decoded to rms {rms}: not the tone"
            );
        }
    }

    #[test]
    fn every_format_writes_a_file_its_own_decoder_recognises() {
        // A second of tone through each encoder, checked by magic bytes and by
        // being smaller than the PCM it came from (or, for WAV, exactly it).
        let pcm = 48_000 * 2 * 2; // one second, stereo, 16-bit

        let wav = take("tone.wav", 48_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(wav.len(), 44 + pcm, "WAV is the PCM plus a header");

        let flac = take("tone.flac", 48_000);
        assert_eq!(&flac[0..4], b"fLaC", "libFLAC's stream marker");
        assert!(flac.len() < pcm, "lossless should still beat raw PCM");

        let mp3 = take("tone.mp3", 48_000);
        // LAME writes a Xing/LAME info frame first; either way it is an MPEG
        // frame sync, or an ID3 header in front of one.
        assert!(
            mp3.starts_with(&[0xff]) || mp3.starts_with(b"ID3"),
            "not an MP3: {:?}",
            &mp3[..4.min(mp3.len())]
        );
        assert!(mp3.len() < pcm, "MP3 should be smaller than the PCM");

        let ogg = take("tone.ogg", 48_000);
        assert_eq!(&ogg[0..4], b"OggS", "Ogg page marker");
        assert!(ogg[28..].starts_with(b"\x01vorbis"), "not a Vorbis stream");
        assert!(ogg.len() < pcm);

        let opus = take("tone.opus", 48_000);
        assert_eq!(&opus[0..4], b"OggS");
        assert!(opus[28..].starts_with(b"OpusHead"), "not an Opus stream");
        assert!(opus.len() < pcm);
    }

    #[test]
    fn opus_refuses_a_rate_it_cannot_encode() {
        // 44.1 kHz is not an Opus rate and resampling is out of scope, so this
        // has to say so rather than write a file that plays at the wrong speed.
        let rec = Recorder::default();
        let err = rec
            .start(temp_dir().join("wrong-rate.opus"), 44_100.0)
            .unwrap_err();
        assert!(err.contains("44100"), "name the offending rate: {err}");
        assert!(!rec.is_recording());
        // The other formats take it happily.
        assert_eq!(&take("rate.flac", 44_100)[0..4], b"fLaC");
    }

    #[test]
    fn the_wav_header_describes_what_follows_it() {
        let h = wav_header(44_100, 1000);
        assert_eq!(u32::from_le_bytes(h[4..8].try_into().unwrap()), 36 + 1000);
        assert_eq!(u16::from_le_bytes(h[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(h[24..28].try_into().unwrap()), 44_100);
        // Byte rate is rate * channels * bytes per sample.
        assert_eq!(
            u32::from_le_bytes(h[28..32].try_into().unwrap()),
            44_100 * 4
        );
        assert_eq!(u16::from_le_bytes(h[32..34].try_into().unwrap()), 4);
    }

    #[test]
    fn blocks_pushed_outside_a_take_go_nowhere() {
        let rec = Recorder::default();
        rec.push(&[(1.0, 1.0)]); // must not panic, must not record
        assert_eq!(rec.stop().unwrap(), None, "nothing to stop");
    }

    #[test]
    fn a_second_start_is_refused_rather_than_replacing_the_take() {
        let path = temp_dir().join("busy.wav");
        let rec = Recorder::default();
        rec.start(&path, 44_100.0).unwrap();
        let err = rec
            .start(temp_dir().join("other.wav"), 44_100.0)
            .unwrap_err();
        assert!(err.contains("already recording"), "{err}");
        // The original take is untouched and still the one that stops.
        assert_eq!(rec.stop().unwrap().as_deref(), Some(path.as_path()));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_path_that_cannot_be_created_fails_at_start() {
        let rec = Recorder::default();
        let err = rec
            .start("no/such/rudel/dir/take.wav", 44_100.0)
            .unwrap_err();
        assert!(
            err.contains("take.wav"),
            "the path belongs in the error: {err}"
        );
        assert!(!rec.is_recording(), "a failed start must not arm");
    }

    #[test]
    fn samples_past_full_scale_clamp_instead_of_wrapping() {
        assert_eq!(to_i16(2.0), i16::MAX);
        assert_eq!(to_i16(-2.0), -i16::MAX);
    }
}
