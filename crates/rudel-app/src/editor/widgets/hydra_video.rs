//! hydra's `s0.initVideo(url)`: a video file as a live source (`hydra_live`),
//! looping and muted like upstream's `<video>` element.
//!
//! MP4 with H.264 video, which is what most shared patterns point at: the
//! `mp4` crate demuxes and OpenH264 decodes. WebM (VP8/VP9) is not read.

use super::{hydra_images::Picture, hydra_live};
use eframe::egui;
use openh264::formats::YUVSource as _;
use std::{
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant},
};

/// The current frame of the video at `url` (already resolved to a path or
/// http(s) URL), starting it on first use.
pub(super) fn frame(ctx: &egui::Context, url: &str) -> Option<Arc<Picture>> {
    let owned = url.to_string();
    hydra_live::latest(ctx, &format!("initVideo({url})"), move |sink| {
        play(sink, &owned)
    })
}

fn play(sink: &hydra_live::Sink, url: &str) -> Result<(), String> {
    // ponytail: the whole file is held in memory, fine for the clips hydra
    // patterns use; stream it if anyone points one at a feature film.
    let bytes = rudel_audio::samples::fetch_cached_bytes(url)?;
    loop {
        let mut mp4 = mp4::Mp4Reader::read_header(Cursor::new(&bytes[..]), bytes.len() as u64)
            .map_err(|e| format!("not an MP4 rudel can read ({e})"))?;
        let (id, track) = mp4
            .tracks()
            .iter()
            .find(|(_, t)| matches!(t.media_type(), Ok(mp4::MediaType::H264)))
            .ok_or("no H.264 video track (rudel reads MP4 with H.264 only)")?;
        let (id, timescale, count) = (*id, f64::from(track.timescale()), track.sample_count());
        // The parameter sets live in the track header, not in the samples.
        let mut head = Vec::new();
        for set in [
            track.sequence_parameter_set().map_err(|e| e.to_string())?,
            track.picture_parameter_set().map_err(|e| e.to_string())?,
        ] {
            head.extend([0, 0, 0, 1]);
            head.extend(set);
        }
        let mut decoder = openh264::decoder::Decoder::new().map_err(|e| e.to_string())?;
        let started = Instant::now();
        for i in 1..=count {
            if !sink.wanted() {
                return Ok(());
            }
            let Ok(Some(sample)) = mp4.read_sample(id, i) else {
                continue;
            };
            let mut annex_b = if i == 1 { head.clone() } else { Vec::new() };
            length_prefixed_to_annex_b(&sample.bytes, &mut annex_b);
            // A frame that does not decode is skipped, not fatal.
            let Ok(Some(yuv)) = decoder.decode(&annex_b) else {
                continue;
            };
            let (width, height) = yuv.dimensions();
            let mut rgba = vec![0; width * height * 4];
            yuv.write_rgba8(&mut rgba);
            let due = Duration::from_secs_f64(sample.start_time as f64 / timescale);
            if let Some(wait) = due.checked_sub(started.elapsed()) {
                std::thread::sleep(wait);
            }
            sink.publish(Picture {
                width: width as u32,
                height: height as u32,
                rgba,
            });
        }
    }
}

/// MP4 stores each NAL unit behind a 4-byte big-endian length; OpenH264 reads
/// them behind start codes.
fn length_prefixed_to_annex_b(mut data: &[u8], out: &mut Vec<u8>) {
    while let Some((len, rest)) = data.split_first_chunk::<4>() {
        let len = (u32::from_be_bytes(*len) as usize).min(rest.len());
        out.extend([0, 0, 0, 1]);
        out.extend(&rest[..len]);
        data = &rest[len..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_prefixes_become_start_codes() {
        let mut out = Vec::new();
        length_prefixed_to_annex_b(&[0, 0, 0, 2, 9, 8, 0, 0, 0, 1, 7], &mut out);
        assert_eq!(out, [0, 0, 0, 1, 9, 8, 0, 0, 0, 1, 7]);
        // A length past the end takes what is there rather than panicking.
        let mut out = Vec::new();
        length_prefixed_to_annex_b(&[0, 0, 0, 9, 1], &mut out);
        assert_eq!(out, [0, 0, 0, 1, 1]);
    }
}
