//! Output formats for a recording.
//!
//! One [`Encoder`] per format. MP3 and Vorbis wrap the codec's own reference
//! library — LAME, libvorbis — rather than a reimplementation; FLAC and Opus
//! use pure-Rust encoders (`flacenc`, `opus-rs`, the latter a port of libopus
//! itself). WAV is the other exception: a 44-byte header and raw PCM is less
//! code than a dependency.
//!
//! Every encoder streams: the writer thread hands it one block at a time and
//! it writes through to the file, so a take is bounded by the file system
//! rather than by memory.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    fs::File,
    io::{BufWriter, Seek, SeekFrom, Write},
    num::{NonZeroU8, NonZeroU32},
    path::Path,
};

/// Stereo throughout: the mixer's master output is a stereo pair.
const CHANNELS: u16 = 2;

/// Every format here is fed 16-bit samples; `to_i16` is the only quantisation.
const BITS: u16 = 16;

/// What a recording is written as, chosen by the file name's extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Uncompressed 16-bit PCM.
    Wav,
    /// MP3, via LAME.
    Mp3,
    /// FLAC, lossless, via `flacenc`.
    Flac,
    /// Ogg Vorbis, via libvorbis.
    Vorbis,
    /// Ogg Opus, via libopus.
    Opus,
}

impl Format {
    /// Extensions offered in the save dialog, best-supported first.
    pub const EXTENSIONS: [&'static str; 5] = ["wav", "flac", "mp3", "ogg", "opus"];

    /// The format a file name asks for, or `None` for an extension that names
    /// no format rudel can write.
    pub fn from_path(path: &Path) -> Option<Format> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "wav" => Some(Format::Wav),
            "mp3" => Some(Format::Mp3),
            "flac" => Some(Format::Flac),
            "ogg" | "oga" => Some(Format::Vorbis),
            "opus" => Some(Format::Opus),
            _ => None,
        }
    }

    /// Open `path` for writing at `sample_rate`.
    pub(super) fn open(self, path: &Path, sample_rate: u32) -> Result<Box<dyn Encoder>, String> {
        let named = |e: String| format!("{}: {e}", path.display());
        match self {
            Format::Wav => Wav::open(path, sample_rate).map(boxed).map_err(named),
            Format::Mp3 => Mp3::open(path, sample_rate).map(boxed).map_err(named),
            Format::Flac => Flac::open(path, sample_rate).map(boxed).map_err(named),
            Format::Vorbis => Vorbis::open(path, sample_rate).map(boxed).map_err(named),
            Format::Opus => Opus::open(path, sample_rate).map(boxed).map_err(named),
        }
    }
}

fn boxed<E: Encoder + 'static>(encoder: E) -> Box<dyn Encoder> {
    Box::new(encoder)
}

/// Takes interleaved stereo blocks and writes a file.
pub(super) trait Encoder {
    /// One block of interleaved stereo samples, `[l, r, l, r, ...]`.
    fn write(&mut self, block: &[f32]) -> Result<(), String>;

    /// Flush the codec's tail and close the file.
    fn finish(self: Box<Self>) -> Result<(), String>;
}

/// One master sample as 16-bit PCM, clamped rather than wrapped — an overshoot
/// that wrapped would come back as a click rather than as distortion.
pub(super) fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

fn create(path: &Path) -> Result<BufWriter<File>, String> {
    File::create(path)
        .map(BufWriter::new)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- WAV

/// Uncompressed PCM. The two length fields are patched in at the end, so the
/// samples stream rather than being buffered.
struct Wav {
    out: BufWriter<File>,
    sample_rate: u32,
    data_len: u32,
}

/// The largest data chunk whose 36-bytes-larger RIFF size still fits in the
/// `u32` the format writes it in.
const MAX_WAV_DATA: u32 = u32::MAX - 36;

/// The 44-byte canonical WAV header for 16-bit stereo PCM.
pub(super) fn wav_header(sample_rate: u32, data_len: u32) -> [u8; 44] {
    let block_align = CHANNELS * BITS / 8;
    let byte_rate = sample_rate * block_align as u32;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    // Everything after this field: the 36 remaining header bytes plus the data.
    h[4..8].copy_from_slice(&data_len.saturating_add(36).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // format 1 = PCM
    h[22..24].copy_from_slice(&CHANNELS.to_le_bytes());
    h[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&block_align.to_le_bytes());
    h[34..36].copy_from_slice(&BITS.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

impl Wav {
    fn open(path: &Path, sample_rate: u32) -> Result<Wav, String> {
        let mut out = create(path)?;
        // A placeholder, rewritten at the end now the length is known.
        out.write_all(&wav_header(sample_rate, 0))
            .map_err(|e| e.to_string())?;
        Ok(Wav {
            out,
            sample_rate,
            data_len: 0,
        })
    }
}

impl Encoder for Wav {
    fn write(&mut self, block: &[f32]) -> Result<(), String> {
        for &sample in block {
            // A WAV's sizes are 32-bit; past 4 GiB the file stops growing
            // rather than being corrupted by a wrapped length. The ceiling is
            // the *RIFF* size, 36 bytes more than the data — about six hours
            // of 48 kHz stereo, which is a take long enough to reach it.
            let Some(next) = self.data_len.checked_add(2).filter(|&n| n <= MAX_WAV_DATA) else {
                return Ok(());
            };
            self.out
                .write_all(&to_i16(sample).to_le_bytes())
                .map_err(|e| e.to_string())?;
            self.data_len = next;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<(), String> {
        self.out
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        self.out
            .write_all(&wav_header(self.sample_rate, self.data_len))
            .map_err(|e| e.to_string())?;
        self.out.flush().map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------- MP3

/// LAME. 192 kbps stereo, which is transparent enough that nobody asks.
struct Mp3 {
    encoder: mp3lame_encoder::Encoder,
    out: BufWriter<File>,
    buf: Vec<u8>,
}

impl Mp3 {
    fn open(path: &Path, sample_rate: u32) -> Result<Mp3, String> {
        fn fail<E: std::fmt::Debug>(what: &str) -> impl FnOnce(E) -> String + '_ {
            move |e| format!("LAME {what}: {e:?}")
        }
        let mut builder = mp3lame_encoder::Builder::new().ok_or("LAME is out of memory")?;
        builder
            .set_num_channels(CHANNELS as u8)
            .map_err(fail("channels"))?;
        builder
            .set_sample_rate(sample_rate)
            .map_err(fail("sample rate"))?;
        builder
            .set_brate(mp3lame_encoder::Bitrate::Kbps192)
            .map_err(fail("bitrate"))?;
        builder
            .set_quality(mp3lame_encoder::Quality::Good)
            .map_err(fail("quality"))?;
        Ok(Mp3 {
            encoder: builder.build().map_err(fail("build"))?,
            out: create(path)?,
            buf: Vec::new(),
        })
    }

    /// `encode_to_vec` writes into spare capacity, so the room has to be there.
    fn drain(&mut self, written: usize) -> Result<(), String> {
        self.out
            .write_all(&self.buf[..written])
            .map_err(|e| e.to_string())
    }
}

impl Encoder for Mp3 {
    fn write(&mut self, block: &[f32]) -> Result<(), String> {
        let frames = block.len() / CHANNELS as usize;
        self.buf.clear();
        self.buf
            .reserve(mp3lame_encoder::max_required_buffer_size(frames));
        let written = self
            .encoder
            .encode_to_vec(mp3lame_encoder::InterleavedPcm(block), &mut self.buf)
            .map_err(|e| format!("LAME encode: {e:?}"))?;
        self.drain(written)
    }

    fn finish(mut self: Box<Self>) -> Result<(), String> {
        self.buf.clear();
        self.buf
            .reserve(mp3lame_encoder::max_required_buffer_size(0));
        let written = self
            .encoder
            .flush_to_vec::<mp3lame_encoder::FlushNoGap>(&mut self.buf)
            .map_err(|e| format!("LAME flush: {e:?}"))?;
        self.drain(written)?;
        self.out.flush().map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------- FLAC

/// The reference encoder's default block size, which `flacenc` is tuned for.
const FLAC_BLOCK: usize = 4096;

/// `flacenc`, in pure Rust. Lossless, so the 16-bit conversion is the only
/// thing lost.
///
/// The crate's one-call API encodes a whole `Stream` in memory; this drives it
/// a frame at a time instead, writing each one through to the file and keeping
/// only the header, so a take stays bounded by the file system like every
/// other encoder here. The header is rewritten at the end, once the totals it
/// describes are known — the same seek-and-patch `Wav` does.
struct Flac {
    out: BufWriter<File>,
    config: flacenc::error::Verified<flacenc::config::Encoder>,
    /// Never has a frame added to it: it is here for the `STREAMINFO` block,
    /// which is what the header is.
    stream: flacenc::component::Stream,
    framebuf: flacenc::source::FrameBuf,
    /// Counts samples and hashes them for `STREAMINFO`'s MD5.
    context: flacenc::source::Context,
    /// Interleaved samples not yet part of a whole frame.
    pending: Vec<i32>,
    sink: flacenc::bitsink::MemSink<u8>,
    /// Smallest and largest frame written, for `STREAMINFO`.
    frame_sizes: Option<(usize, usize)>,
}

impl Flac {
    fn open(path: &Path, sample_rate: u32) -> Result<Flac, String> {
        use flacenc::error::Verify;
        let config = flacenc::config::Encoder::default()
            .into_verified()
            .map_err(|(_, e)| format!("flac config: {e}"))?;
        let mut stream =
            flacenc::component::Stream::new(sample_rate as usize, CHANNELS as usize, BITS as usize)
                .map_err(|e| format!("flac stream: {e}"))?;
        // Fixed block size, like the reference encoder: the final frame may
        // still be shorter, which its own header says.
        stream
            .stream_info_mut()
            .set_block_sizes(FLAC_BLOCK, FLAC_BLOCK)
            .map_err(|e| format!("flac block size: {e}"))?;

        let mut flac = Flac {
            out: create(path)?,
            config,
            stream,
            framebuf: flacenc::source::FrameBuf::with_size(CHANNELS as usize, FLAC_BLOCK)
                .map_err(|e| format!("flac buffer: {e}"))?,
            context: flacenc::source::Context::new(BITS as usize, CHANNELS as usize),
            pending: Vec::new(),
            sink: flacenc::bitsink::MemSink::new(),
            frame_sizes: None,
        };
        // A placeholder of exactly the size the real one will be.
        let header = flac.header_bytes()?;
        flac.out.write_all(&header).map_err(|e| e.to_string())?;
        Ok(flac)
    }

    /// `fLaC` plus the `STREAMINFO` block, as the file starts and ends with.
    fn header_bytes(&mut self) -> Result<Vec<u8>, String> {
        use flacenc::component::BitRepr;
        self.sink.clear();
        self.stream
            .write(&mut self.sink)
            .map_err(|e| format!("flac header: {e}"))?;
        Ok(self.sink.as_slice().to_vec())
    }

    /// Encode the first `samples` interleaved values of `pending` as one frame.
    fn encode_frame(&mut self, samples: usize) -> Result<(), String> {
        use flacenc::{component::BitRepr, source::Fill};
        (&mut self.framebuf, &mut self.context)
            .fill_interleaved(&self.pending[..samples])
            .map_err(|e| format!("flac fill: {e}"))?;
        let number = self.context.current_frame_number().unwrap_or(0);
        let frame = flacenc::encode_fixed_size_frame(
            &self.config,
            &self.framebuf,
            number,
            self.stream.stream_info(),
        )
        .map_err(|e| format!("flac encode: {e}"))?;

        self.sink.clear();
        frame
            .write(&mut self.sink)
            .map_err(|e| format!("flac frame: {e}"))?;
        let bytes = self.sink.as_slice();
        self.out.write_all(bytes).map_err(|e| e.to_string())?;

        let (min, max) = self.frame_sizes.unwrap_or((usize::MAX, 0));
        self.frame_sizes = Some((min.min(bytes.len()), max.max(bytes.len())));
        self.pending.drain(..samples);
        Ok(())
    }
}

impl Encoder for Flac {
    fn write(&mut self, block: &[f32]) -> Result<(), String> {
        self.pending.extend(block.iter().map(|&s| to_i16(s) as i32));
        let frame = FLAC_BLOCK * CHANNELS as usize;
        while self.pending.len() >= frame {
            self.encode_frame(frame)?;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<(), String> {
        // Unlike Opus, FLAC's last frame may be short, so the tail needs no
        // padding — only a frame of its own.
        if !self.pending.is_empty() {
            let tail = self.pending.len();
            self.encode_frame(tail)?;
        }
        let (min, max) = self.frame_sizes.unwrap_or((0, 0));
        let (total, md5) = (self.context.total_samples(), self.context.md5_digest());
        let info = self.stream.stream_info_mut();
        info.set_frame_sizes(min.min(max), max)
            .map_err(|e| format!("flac frame sizes: {e}"))?;
        info.set_total_samples(total);
        info.set_md5_digest(&md5);

        // Now the header describes what follows it, put it where it belongs.
        let header = self.header_bytes()?;
        self.out
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        self.out.write_all(&header).map_err(|e| e.to_string())?;
        self.out.flush().map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------- Vorbis

/// libvorbis (the aoTuV/Lancer-tuned build the `vorbis_rs` crate bundles),
/// writing its own Ogg container.
struct Vorbis {
    encoder: Option<vorbis_rs::VorbisEncoder<BufWriter<File>>>,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Vorbis {
    fn open(path: &Path, sample_rate: u32) -> Result<Vorbis, String> {
        let rate = NonZeroU32::new(sample_rate).ok_or("a sample rate of zero")?;
        let channels = NonZeroU8::new(CHANNELS as u8).expect("two channels");
        let mut builder = vorbis_rs::VorbisEncoderBuilder::new(rate, channels, create(path)?)
            .map_err(|e| format!("libvorbis init: {e}"))?;
        let encoder = builder.build().map_err(|e| format!("libvorbis: {e}"))?;
        Ok(Vorbis {
            encoder: Some(encoder),
            left: Vec::new(),
            right: Vec::new(),
        })
    }
}

impl Encoder for Vorbis {
    fn write(&mut self, block: &[f32]) -> Result<(), String> {
        let Vorbis {
            encoder,
            left,
            right,
        } = &mut *self;
        let Some(encoder) = encoder.as_mut() else {
            return Ok(());
        };
        // libvorbis takes planar channels, not interleaved frames.
        left.clear();
        right.clear();
        for [l, r] in block.as_chunks::<2>().0 {
            left.push(*l);
            right.push(*r);
        }
        encoder
            .encode_audio_block([left.as_slice(), right.as_slice()])
            .map_err(|e| format!("libvorbis encode: {e}"))
    }

    fn finish(mut self: Box<Self>) -> Result<(), String> {
        match self.encoder.take() {
            Some(encoder) => encoder
                .finish()
                .map(|_| ())
                .map_err(|e| format!("libvorbis finish: {e}")),
            None => Ok(()),
        }
    }
}

// ---------------------------------------------------------------- Opus

/// libopus. Unlike the others this one carries no container of its own, so the
/// packets are muxed into Ogg here, per RFC 7845.
struct Opus {
    encoder: opus_rs::OpusEncoder,
    sample_rate: u32,
    writer: ogg::PacketWriter<'static, BufWriter<File>>,
    serial: u32,
    /// Interleaved samples not yet making up a whole frame.
    pending: Vec<f32>,
    /// Samples per channel in one packet.
    frame: usize,
    /// Decoded samples so far, at 48 kHz, as the Ogg granule position counts
    /// them: it includes the pre-skip.
    granule: u64,
    packet: Vec<u8>,
}

/// The encoder's own delay at 48 kHz: what libopus reports for every mode it
/// selects here, and `opus-rs` is a port of it with the same delay
/// compensation. Neither exposes `OPUS_GET_LOOKAHEAD`.
///
// ponytail: hard-coded rather than queried. Getting it wrong only shifts the
// start of the file by a couple of milliseconds; expose the CTL upstream (or
// bind it directly) if that ever matters.
const OPUS_PRE_SKIP: u16 = 312;

/// Stereo music, roughly what libopus chose on its own before the encoder
/// became `opus-rs`, which defaults to a speech rate instead.
const OPUS_BITRATE: i32 = 96_000;

/// The rates rudel encodes Opus at. A recording at any other rate would need
/// resampling, which is a bigger dependency than the codec.
///
/// Opus itself also allows 24 kHz, but `opus-rs` 0.1.32 encodes noise there —
/// a tone written at 24 kHz comes back from libopus (and from `opus-rs`'s own
/// decoder) as full-scale garbage, while every other rate round-trips within a
/// percent of the source amplitude. No sound card runs at 24 kHz, so it is
/// simply not offered rather than worked around; try it again on an upgrade.
const OPUS_RATES: [u32; 4] = [8_000, 12_000, 16_000, 48_000];

/// Opus counts granule positions at 48 kHz whatever it was fed.
fn to_48k(samples: u64, sample_rate: u32) -> u64 {
    samples * 48_000 / sample_rate.max(1) as u64
}

impl Opus {
    fn open(path: &Path, sample_rate: u32) -> Result<Opus, String> {
        if !OPUS_RATES.contains(&sample_rate) {
            return Err(format!(
                "Opus only encodes at {} Hz, and this device runs at {sample_rate} Hz — \
                 record to WAV or FLAC instead",
                OPUS_RATES.map(|r| r.to_string()).join("/"),
            ));
        }
        let mut encoder = opus_rs::OpusEncoder::new(
            sample_rate as i32,
            CHANNELS as usize,
            opus_rs::Application::Audio,
        )
        .map_err(|e| format!("opus init: {e}"))?;
        // opus-rs defaults to 64 kbps, which is a speech rate; libopus used to
        // pick about 100 kbps by itself for 48 kHz stereo music, so name one.
        encoder.bitrate_bps = OPUS_BITRATE;

        let mut opus = Opus {
            encoder,
            writer: ogg::PacketWriter::new(create(path)?),
            // The Ogg spec asks for a random serial; only uniqueness within a
            // file actually matters, and there is one stream here.
            serial: std::process::id(),
            pending: Vec::new(),
            sample_rate,
            // 20 ms, the size Opus is tuned around.
            frame: sample_rate as usize / 50,
            granule: 0,
            packet: vec![0u8; 4000], // over the largest packet Opus emits
        };
        opus.write_headers(sample_rate)?;
        Ok(opus)
    }

    /// The two mandatory header packets, each alone on its own Ogg page.
    fn write_headers(&mut self, sample_rate: u32) -> Result<(), String> {
        let mut head = Vec::with_capacity(19);
        head.extend_from_slice(b"OpusHead");
        head.push(1); // version
        head.push(CHANNELS as u8);
        head.extend_from_slice(&OPUS_PRE_SKIP.to_le_bytes());
        head.extend_from_slice(&sample_rate.to_le_bytes()); // original rate, informational
        head.extend_from_slice(&0i16.to_le_bytes()); // output gain
        head.push(0); // channel mapping family 0: plain stereo
        self.page(head, 0)?;

        let vendor = b"rudel";
        let mut tags = Vec::new();
        tags.extend_from_slice(b"OpusTags");
        tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        tags.extend_from_slice(vendor);
        tags.extend_from_slice(&0u32.to_le_bytes()); // no comments
        self.page(tags, 0)
    }

    /// Write one packet and end the page after it.
    fn page(&mut self, data: Vec<u8>, granule: u64) -> Result<(), String> {
        self.writer
            .write_packet(data, self.serial, ogg::PacketWriteEndInfo::EndPage, granule)
            .map_err(|e| format!("ogg write: {e}"))
    }

    /// Encode exactly one frame from the front of `pending`. `end_granule`
    /// overrides the position stamped on the packet, for the final frame whose
    /// tail is padding the decoder has to trim.
    fn encode_frame(
        &mut self,
        last: bool,
        sample_rate: u32,
        end_granule: Option<u64>,
    ) -> Result<(), String> {
        let samples = self.frame * CHANNELS as usize;
        let written = self
            .encoder
            .encode(&self.pending[..samples], self.frame, &mut self.packet)
            .map_err(|e| format!("opus encode: {e}"))?;
        self.pending.drain(..samples);
        self.granule += to_48k(self.frame as u64, sample_rate);
        let data = self.packet[..written].to_vec();
        let end = if last {
            ogg::PacketWriteEndInfo::EndStream
        } else {
            ogg::PacketWriteEndInfo::NormalPacket
        };
        // The granule position is what the decoder should have produced by the
        // end of this packet, pre-skip included.
        let granule = end_granule.unwrap_or(self.granule) + OPUS_PRE_SKIP as u64;
        self.writer
            .write_packet(data, self.serial, end, granule)
            .map_err(|e| format!("ogg write: {e}"))
    }
}

impl Encoder for Opus {
    fn write(&mut self, block: &[f32]) -> Result<(), String> {
        let sample_rate = self.sample_rate;
        self.pending.extend_from_slice(block);
        while self.pending.len() >= self.frame * CHANNELS as usize {
            self.encode_frame(false, sample_rate, None)?;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<(), String> {
        let sample_rate = self.sample_rate;
        // Opus only emits whole frames, so the tail is padded out with silence.
        // The granule position stops at the real length, so a decoder trims it
        // — stamped directly rather than by adding the frame and subtracting
        // it back, which went below zero on a take shorter than one frame.
        let real = self.pending.len() / CHANNELS as usize;
        self.pending.resize(self.frame * CHANNELS as usize, 0.0);
        let end = self.granule + to_48k(real as u64, sample_rate);
        self.encode_frame(true, sample_rate, Some(end))?;
        self.writer
            .inner_mut()
            .flush()
            .map_err(|e| format!("ogg flush: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_stops_growing_before_its_riff_size_wraps() {
        // ~6 hours of 48 kHz stereo reaches the 32-bit ceiling. The data chunk
        // used to be capped there but the RIFF size, 36 bytes larger, was not.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cap.wav");
        let mut wav = Wav::open(&path, 48_000).unwrap();
        wav.data_len = MAX_WAV_DATA - 2;
        // Four samples, of which only the first still fits.
        wav.write(&[0.0; 4]).unwrap();
        assert_eq!(wav.data_len, MAX_WAV_DATA, "the cap is the RIFF ceiling");
        Box::new(wav).finish().unwrap();

        let h = wav_header(48_000, MAX_WAV_DATA);
        assert_eq!(
            u32::from_le_bytes(h[4..8].try_into().unwrap()),
            u32::MAX,
            "the largest data chunk is exactly the largest RIFF size"
        );
    }

    #[test]
    fn a_header_asked_for_an_impossible_length_saturates_rather_than_wrapping() {
        let h = wav_header(48_000, u32::MAX);
        assert_eq!(u32::from_le_bytes(h[4..8].try_into().unwrap()), u32::MAX);
    }

    #[test]
    fn a_wav_header_carries_the_rate_the_device_ran_at() {
        let h = wav_header(44_100, 8);
        assert_eq!(&h[0..4], b"RIFF");
        assert_eq!(&h[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(h[24..28].try_into().unwrap()), 44_100);
        // Four bytes a frame: 16-bit stereo.
        assert_eq!(u16::from_le_bytes(h[32..34].try_into().unwrap()), 4);
        // The byte rate is the product of the two, not a sum of them.
        assert_eq!(
            u32::from_le_bytes(h[28..32].try_into().unwrap()),
            44_100 * 4
        );
        assert_eq!(u32::from_le_bytes(h[40..44].try_into().unwrap()), 8);
    }

    #[test]
    fn opus_granules_are_counted_at_48k_whatever_the_device_ran_at() {
        // A tenth of a second is 4800 granules however it was sampled.
        assert_eq!(to_48k(4_800, 48_000), 4_800);
        assert_eq!(to_48k(800, 8_000), 4_800);
        assert_eq!(to_48k(1_200, 12_000), 4_800);
        assert_eq!(to_48k(1_600, 16_000), 4_800);
        // A zero rate would divide by zero, so it is floored at 1 instead.
        assert_eq!(to_48k(2, 0), 96_000);
    }
}

#[cfg(test)]
mod round_trips {
    use super::*;
    use std::path::Path;

    /// Decode a recorded file the way a sample is loaded: mono, averaged.
    fn decode(path: &Path) -> Vec<f32> {
        let json = path.with_extension("json");
        let file = path.file_name().unwrap().to_str().unwrap();
        std::fs::write(&json, format!(r#"{{"x": "{file}"}}"#)).unwrap();
        let mut bank = crate::samples::SampleBank::new();
        bank.load_samples_source(json.to_str().unwrap())
            .expect("decodes");
        bank.get("x", 0).expect("loaded").data.clone()
    }

    /// `frames` of stereo: a ramp on the left, silence on the right, so a
    /// channel slip shows in the mono average.
    fn ramp(frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| [i as f32 / frames as f32 * 0.8, 0.0])
            .collect()
    }

    fn record(name: &str, frames: usize, block: usize) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        let format = Format::from_path(&path).expect("a known extension");
        let mut encoder = format.open(&path, 48_000).expect("opens");
        for chunk in ramp(frames).chunks(block * CHANNELS as usize) {
            encoder.write(chunk).expect("encodes");
        }
        encoder.finish().expect("finishes");
        (dir, path)
    }

    #[test]
    fn flac_keeps_every_sample_across_whole_and_partial_blocks() {
        let frames = FLAC_BLOCK * 5 / 2;
        let (_dir, path) = record("take.flac", frames, 1000);
        let got = decode(&path);
        assert_eq!(got.len(), frames);
        for i in [
            0,
            FLAC_BLOCK - 1,
            FLAC_BLOCK,
            FLAC_BLOCK * 2 + 7,
            frames - 1,
        ] {
            let want = i as f32 / frames as f32 * 0.4;
            assert!(
                (got[i] - want).abs() < 1e-3,
                "frame {i}: {} vs {want}",
                got[i]
            );
        }
    }

    /// Frames hold `FLAC_BLOCK` stereo frames, as STREAMINFO promises. A
    /// decoder that takes any block size hides a mismatch; a strict one
    /// rejects the file. Read off the first frame header's block-size code.
    #[test]
    fn flac_frames_are_the_block_size_streaminfo_declares() {
        let (_dir, path) = record("take.flac", FLAC_BLOCK * 2, 1000);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"fLaC");
        // Metadata blocks: a flags byte (top bit marks the last) and a
        // 24-bit length, then the body.
        let mut at = 4;
        loop {
            let last = bytes[at] & 0x80 != 0;
            let len = u32::from_be_bytes([0, bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
            at += 4 + len as usize;
            if last {
                break;
            }
        }
        assert_eq!(
            &bytes[at..at + 2],
            &[0xFF, 0xF8],
            "fixed-blocksize frame sync"
        );
        // Block-size code 0b1100 is 256 << 4 = 4096.
        assert_eq!(FLAC_BLOCK, 4096);
        assert_eq!(bytes[at + 2] >> 4, 0b1100, "block size code");
    }

    #[test]
    fn lossy_takes_decode_to_their_whole_length_once_finished() {
        // The tail only reaches the file when the encoder is flushed.
        for name in ["take.mp3", "take.ogg"] {
            let (_dir, path) = record(name, 48_000, 4_800);
            let got = decode(&path);
            assert!(got.len() + 200 >= 48_000, "{name}: {} frames", got.len());
        }
    }

    #[test]
    fn opus_granules_run_to_the_real_length_past_the_pre_skip() {
        // One whole 20ms frame and half another: 960 then 1440 at 48k.
        let (_dir, path) = record("take.opus", 1_440, 1_440);
        let mut reader = ogg::PacketReader::new(std::fs::File::open(&path).unwrap());
        let mut granules = Vec::new();
        while let Some(packet) = reader.read_packet().expect("reads") {
            granules.push(packet.absgp_page());
        }
        // Packets share pages, so only the page granule of the last one is
        // certain: the whole take, pre-skip included.
        let skip = u64::from(OPUS_PRE_SKIP);
        assert_eq!(granules.last(), Some(&(1_440 + skip)), "{granules:?}");
    }
}
