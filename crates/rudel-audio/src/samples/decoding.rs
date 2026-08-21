//! In-memory audio decoding, including the lenient WAV fallback.

use rudel_dsp::Sample;
use symphonia::core::{
    codecs::{CodecParameters, audio::AudioDecoderOptions},
    errors::Error,
    formats::{FormatOptions, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

/// Decode in-memory audio bytes into a mono [`Sample`]. Symphonia handles all
/// formats; WAVs it rejects fall back to our lenient in-house reader, since old
/// sample packs (e.g. dirt-samples' `mute`/`pluck`) have nonstandard 20-byte
/// PCM fmt chunks symphonia refuses to parse.
pub(super) fn decode_sample_bytes(bytes: Vec<u8>) -> Result<Sample, String> {
    let is_wav = bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(&b"WAVE"[..]);
    let bytes: std::sync::Arc<[u8]> = bytes.into();
    match decode_symphonia(bytes.clone()) {
        Ok(sample) => Ok(sample),
        Err(e) if is_wav => {
            decode_wav_lenient(&bytes).map_err(|e2| format!("decode audio: {e}; lenient wav: {e2}"))
        }
        Err(e) => Err(format!("decode audio: {e}")),
    }
}

/// Decode the first audio track with symphonia, mixed down to mono.
///
/// Gapless is the reason this is not `fundsp`'s `Wave::load_slice`, which
/// hard-codes it off. A LAME-encoded MP3 carries ~1100 frames of encoder delay
/// at the head — silence the encoder added and the file's own Xing/LAME header
/// says to drop. `decodeAudioData` in the browser drops it, so upstream never
/// sees it; keeping it made every MP3 sample start ~25 ms late. That is
/// inaudible on a drum hit at `speed(1)`, but the offset is in *source* frames,
/// so it stretches with playback rate: the "Wavy kalimba" tune plays one MP3 at
/// 0.25 and 0.125 (a melody and a bass line three octaves down), which put its
/// two layers 100 ms apart from each other.
///
/// `gapless` is a *decoder* option here and defaults to on, but it is set
/// explicitly: it was a format option that defaulted to off in symphonia 0.5,
/// so leaving it implicit would make a silent regression out of a default the
/// upstream is free to change again.
fn decode_symphonia(bytes: std::sync::Arc<[u8]>) -> Result<Sample, Error> {
    let stream = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let mut reader = symphonia::default::get_probe().probe(
        &Hint::new(),
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    // Scoped so the track borrow of `reader` ends before the packet loop takes
    // it mutably.
    let (track_id, mut decoder) = {
        let track = reader
            .first_track(TrackType::Audio)
            .ok_or(Error::DecodeError("no audio track"))?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.as_ref() else {
            return Err(Error::DecodeError("track has no audio codec parameters"));
        };
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default().gapless(true))?;
        (track.id, decoder)
    };

    let mut interleaved: Vec<f32> = Vec::new();
    let mut packet_samples: Vec<f32> = Vec::new();
    let mut spec = None;
    // `Ok(None)` is the end of the stream. A read error ends it too: a
    // truncated download is still worth whatever decoded.
    while let Ok(Some(packet)) = reader.next_packet() {
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                spec = Some(decoded.spec().clone());
                // Resizes to exactly this packet's samples rather than
                // appending, so it is a scratch buffer, not the accumulator.
                decoded.copy_to_vec_interleaved(&mut packet_samples);
                interleaved.extend_from_slice(&packet_samples);
            }
            // Documented as recoverable: skip the packet, keep the stream.
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e),
        }
    }
    let spec = spec.ok_or(Error::DecodeError("no audio decoded"))?;
    let channels = spec.channels().count().max(1);
    Ok(Sample {
        data: interleaved
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect(),
        sample_rate: spec.rate() as f32,
    })
}

/// Fallback lenient WAV decode (replaces the archived `wavers` crate): skips
/// unknown chunks, tolerates oversized fmt chunks and truncated data, and
/// handles 8/16/24/32-bit PCM plus 32/64-bit IEEE float.
pub(super) fn decode_wav_lenient(bytes: &[u8]) -> Result<Sample, String> {
    let mut fmt: Option<(u16, usize, f32, u16)> = None; // (tag, channels, rate, bits)
    let mut data: Option<&[u8]> = None;
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &bytes[pos + 8..(pos + 8 + size).min(bytes.len())];
        match id {
            b"fmt " if body.len() >= 16 => {
                let u16_at = |o: usize| u16::from_le_bytes(body[o..o + 2].try_into().unwrap());
                let mut tag = u16_at(0);
                // WAVE_FORMAT_EXTENSIBLE: real format is the first word of the sub-format GUID
                if tag == 0xFFFE && body.len() >= 26 {
                    tag = u16_at(24);
                }
                let rate = u32::from_le_bytes(body[4..8].try_into().unwrap()) as f32;
                fmt = Some((tag, u16_at(2).max(1) as usize, rate, u16_at(14)));
            }
            b"data" => data = Some(body),
            _ => {}
        }
        pos += 8 + size + (size & 1); // chunks are word-aligned
    }
    let (tag, channels, sample_rate, bits) = fmt.ok_or("no fmt chunk")?;
    let data = data.ok_or("no data chunk")?;
    let samples: Vec<f32> = match (tag, bits) {
        (1, 8) => data.iter().map(|&v| (v as f32 - 128.0) / 128.0).collect(),
        (1, 16) => data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c) as f32 / 32768.0)
            .collect(),
        (1, 24) => data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0)
            .collect(),
        (1, 32) => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_le_bytes(*c) as f32 / 2_147_483_648.0)
            .collect(),
        (3, 32) => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect(),
        (3, 64) => data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| f64::from_le_bytes(*c) as f32)
            .collect(),
        _ => return Err(format!("unsupported wav format: tag {tag}, {bits}-bit")),
    };
    let data = samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    Ok(Sample { data, sample_rate })
}

#[cfg(test)]
mod tests {
    use super::decode_sample_bytes;

    /// A quarter-second 440 Hz tone, LAME-encoded:
    ///
    /// ```text
    /// ffmpeg -f lavfi -i "sine=frequency=440:duration=0.25:sample_rate=44100" \
    ///        -ac 1 -codec:a libmp3lame -b:a 64k tone.mp3
    /// ```
    ///
    /// Its Xing/LAME header declares the encoder delay, so a decode that
    /// ignores it starts the sample with ~1100 frames of silence the browser
    /// would never play.
    const MP3: &[u8] = include_bytes!("../../tests/fixtures/tone.mp3");

    #[test]
    fn an_mp3s_encoder_delay_is_trimmed_off_the_front() {
        let sample = decode_sample_bytes(MP3.to_vec()).expect("decode the fixture");
        let peak = sample.data.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak > 0.1, "the fixture should be a tone, peak {peak}");
        let onset = sample
            .data
            .iter()
            .position(|v| v.abs() > peak * 0.01)
            .expect("the tone starts somewhere");
        // Untrimmed this is ~1105 frames — the LAME encoder delay plus the
        // decoder's own. 25 ms of silence is inaudible in a drum hit at
        // `speed(1)` and 200 ms of it is not at `speed(0.125)`, which is how
        // this surfaced: the offset is in source frames, so it stretches with
        // the playback rate and pulls a tune's layers apart from each other.
        assert!(
            onset < 100,
            "the tone should start at once, not {onset} frames in"
        );
    }
}
