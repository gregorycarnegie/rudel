//! In-memory audio decoding, including the lenient WAV fallback.

use fundsp::wave::Wave;
use rudel_dsp::Sample;
/// Decode in-memory audio bytes into a mono [`Sample`]. Symphonia (via fundsp)
/// handles all formats; WAVs it rejects fall back to our lenient in-house
/// reader, since old sample packs (e.g. dirt-samples' `mute`/`pluck`) have
/// nonstandard 20-byte PCM fmt chunks symphonia refuses to parse.
pub(super) fn decode_sample_bytes(bytes: Vec<u8>) -> Result<Sample, String> {
    let is_wav = bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(&b"WAVE"[..]);
    let bytes: std::sync::Arc<[u8]> = bytes.into();
    match Wave::load_slice(bytes.clone()) {
        Ok(wave) => Ok(wave_to_sample(&wave)),
        Err(e) if is_wav => {
            decode_wav_lenient(&bytes).map_err(|e2| format!("decode audio: {e}; lenient wav: {e2}"))
        }
        Err(e) => Err(format!("decode audio: {e}")),
    }
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
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect(),
        (1, 24) => data
            .chunks_exact(3)
            .map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0)
            .collect(),
        (1, 32) => data
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes(c.try_into().unwrap()) as f32 / 2_147_483_648.0)
            .collect(),
        (3, 32) => data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect(),
        (3, 64) => data
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap()) as f32)
            .collect(),
        _ => return Err(format!("unsupported wav format: tag {tag}, {bits}-bit")),
    };
    let data = samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    Ok(Sample { data, sample_rate })
}

/// Average a decoded [`Wave`]'s channels down to a mono [`Sample`].
fn wave_to_sample(wave: &Wave) -> Sample {
    let channels = wave.channels().max(1);
    let len = wave.len();
    let mut data = Vec::with_capacity(len);
    for i in 0..len {
        let mut sum = 0.0f32;
        for c in 0..channels {
            sum += wave.at(c, i);
        }
        data.push(sum / channels as f32);
    }
    Sample {
        data,
        sample_rate: wave.sample_rate() as f32,
    }
}
