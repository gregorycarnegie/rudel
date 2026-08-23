//! Output formats for a recording.
//!
//! One [`Encoder`] per format, each wrapping the codec's own reference
//! library — LAME, libFLAC, libvorbis and libopus — rather than a
//! reimplementation. WAV is the exception: a 44-byte header and raw PCM is
//! less code than a dependency.
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

/// What a recording is written as, chosen by the file name's extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Uncompressed 16-bit PCM.
    Wav,
    /// MP3, via LAME.
    Mp3,
    /// FLAC, lossless, via libFLAC.
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

/// The 44-byte canonical WAV header for 16-bit stereo PCM.
pub(super) fn wav_header(sample_rate: u32, data_len: u32) -> [u8; 44] {
    const BITS: u16 = 16;
    let block_align = CHANNELS * BITS / 8;
    let byte_rate = sample_rate * block_align as u32;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    // Everything after this field: the 36 remaining header bytes plus the data.
    h[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
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
            // rather than being corrupted by a wrapped length.
            let Some(next) = self.data_len.checked_add(2) else {
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

/// libFLAC. Lossless, so the 16-bit conversion is the only thing lost.
struct Flac {
    encoder: Option<flac_bound::FlacEncoder<'static>>,
    buf: Vec<i32>,
}

impl Flac {
    fn open(path: &Path, sample_rate: u32) -> Result<Flac, String> {
        let encoder = flac_bound::FlacEncoder::new()
            .ok_or("libFLAC is out of memory")?
            .channels(CHANNELS as u32)
            .bits_per_sample(16)
            .sample_rate(sample_rate)
            // 5 is libFLAC's own default: the knee of the size/speed curve.
            .compression_level(5)
            .init_file(&path)
            .map_err(|e| format!("libFLAC init: {e:?}"))?;
        Ok(Flac {
            encoder: Some(encoder),
            buf: Vec::new(),
        })
    }
}

impl Encoder for Flac {
    fn write(&mut self, block: &[f32]) -> Result<(), String> {
        let Some(encoder) = self.encoder.as_mut() else {
            return Ok(());
        };
        self.buf.clear();
        self.buf.extend(block.iter().map(|&s| to_i16(s) as i32));
        encoder
            .process_interleaved(&self.buf, (block.len() / CHANNELS as usize) as u32)
            .map_err(|()| "libFLAC encode failed".to_string())
    }

    fn finish(mut self: Box<Self>) -> Result<(), String> {
        match self.encoder.take() {
            // `finish` hands the encoder back on failure; there is nothing
            // useful left to do with it.
            Some(encoder) => encoder
                .finish()
                .map(|_| ())
                .map_err(|_| "libFLAC failed to close the stream".to_string()),
            None => Ok(()),
        }
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
    encoder: opus::Encoder,
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

/// libopus' own encoder delay at 48 kHz. The `opus` crate does not expose
/// `OPUS_GET_LOOKAHEAD`, and this is the value libopus reports for every mode
/// it selects here.
///
// ponytail: hard-coded rather than queried. Getting it wrong only shifts the
// start of the file by a couple of milliseconds; expose the CTL upstream (or
// bind it directly) if that ever matters.
const OPUS_PRE_SKIP: u16 = 312;

/// The rates libopus accepts. A recording at any other rate would need
/// resampling, which is a bigger dependency than the codec.
const OPUS_RATES: [u32; 5] = [8_000, 12_000, 16_000, 24_000, 48_000];

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
        let encoder = opus::Encoder::new(
            sample_rate,
            opus::Channels::Stereo,
            opus::Application::Audio,
        )
        .map_err(|e| format!("libopus init: {e}"))?;

        let mut opus = Opus {
            encoder,
            writer: ogg::PacketWriter::new(create(path)?),
            // The Ogg spec asks for a random serial; only uniqueness within a
            // file actually matters, and there is one stream here.
            serial: std::process::id(),
            pending: Vec::new(),
            // 20 ms, the size libopus is tuned around.
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

    /// Encode exactly one frame from the front of `pending`.
    fn encode_frame(&mut self, last: bool, sample_rate: u32) -> Result<(), String> {
        let samples = self.frame * CHANNELS as usize;
        let written = self
            .encoder
            .encode_float(&self.pending[..samples], &mut self.packet)
            .map_err(|e| format!("libopus encode: {e}"))?;
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
        let granule = self.granule + OPUS_PRE_SKIP as u64;
        self.writer
            .write_packet(data, self.serial, end, granule)
            .map_err(|e| format!("ogg write: {e}"))
    }
}

impl Encoder for Opus {
    fn write(&mut self, block: &[f32]) -> Result<(), String> {
        let sample_rate = self.encoder.get_sample_rate().unwrap_or(48_000);
        self.pending.extend_from_slice(block);
        while self.pending.len() >= self.frame * CHANNELS as usize {
            self.encode_frame(false, sample_rate)?;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<(), String> {
        let sample_rate = self.encoder.get_sample_rate().unwrap_or(48_000);
        // Opus only emits whole frames, so the tail is padded out with silence.
        // The granule position stops at the real length, so a decoder trims it.
        let real = self.pending.len() / CHANNELS as usize;
        self.pending.resize(self.frame * CHANNELS as usize, 0.0);
        self.granule += to_48k(real as u64, sample_rate);
        self.granule -= to_48k(self.frame as u64, sample_rate);
        self.encode_frame(true, sample_rate)?;
        self.writer
            .inner_mut()
            .flush()
            .map_err(|e| format!("ogg flush: {e}"))
    }
}
