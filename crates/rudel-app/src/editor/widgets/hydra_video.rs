//! hydra's `s0.initVideo(url)`: a video file as a live source (`hydra_live`),
//! looping and muted like upstream's `<video>` element.
//!
//! The installed `ffmpeg` plays it when there is one: any format it reads
//! (MP4, WebM, MOV, GIF, …), at the file's own pace, looping, its raw RGBA
//! frames read from a pipe. Without it, MP4 with H.264, which is most shared
//! patterns' videos, still plays: the `mp4` crate demuxes and OpenH264
//! decodes.

use super::{hydra_images::Picture, hydra_live};
use eframe::egui;
use openh264::formats::YUVSource as _;
use std::{
    ffi::{OsStr, OsString},
    io::{Cursor, Read as _},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

/// What ffmpeg says it is when it fetches a web video, as rudel's own
/// downloads do: some hosts refuse a generic one.
const USER_AGENT: &str = concat!(
    "rudel/",
    env!("CARGO_PKG_VERSION"),
    " (live-coding music app)"
);

/// No wider than this: a background does not need a 4K texture.
const MAX_WIDTH: u32 = 1280;

/// The current frame of the video at `url` (already resolved to a path or
/// http(s) URL), starting it on first use.
pub(super) fn frame(ctx: &egui::Context, url: &str) -> Option<Arc<Picture>> {
    let owned = url.to_string();
    hydra_live::latest(ctx, &format!("initVideo({url})"), move |sink| {
        play(sink, &owned)
    })
}

fn play(sink: &hydra_live::Sink, url: &str) -> Result<(), String> {
    // ffmpeg streams a web video itself, starting at once, where downloading
    // first would hold a long clip back for minutes (one shared pattern's is
    // 678 MB). A local file is read where it is.
    let input = if url.starts_with("http://") || url.starts_with("https://") {
        OsString::from(url)
    } else {
        rudel_audio::samples::fetch_cached_file(url)?.into_os_string()
    };
    match probe(&input) {
        Ok((width, height)) => play_ffmpeg(sink, &input, width, height),
        // No ffmpeg installed: the built-in MP4/H.264 reader.
        Err(Probe::Missing) => play_mp4(sink, url),
        Err(Probe::Failed(e)) => Err(e),
    }
}

enum Probe {
    Missing,
    Failed(String),
}

/// A command that opens no console window of its own on Windows.
fn tool(name: &str) -> Command {
    let command = Command::new(name);
    #[cfg(windows)]
    let command = {
        let mut command = command;
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
        command
    };
    command
}

/// The first video stream's size, by `ffprobe`.
fn probe(input: &OsStr) -> Result<(u32, u32), Probe> {
    let output = tool("ffprobe")
        .args([
            "-v",
            "error",
            "-user_agent",
            USER_AGENT,
            "-select_streams",
            "v:0",
        ])
        .args(["-show_entries", "stream=width,height", "-of", "csv=p=0"])
        .arg(input)
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Probe::Missing,
            _ => Probe::Failed(e.to_string()),
        })?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut size = text.trim().split(',').map(|n| n.trim().parse::<u32>());
    match (size.next(), size.next()) {
        (Some(Ok(width)), Some(Ok(height))) if width > 0 && height > 0 => Ok((width, height)),
        _ => Err(Probe::Failed(format!(
            "ffprobe found no video in it: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

/// The size frames are scaled to: the video's, no wider than [`MAX_WIDTH`].
fn fit(width: u32, height: u32) -> (u32, u32) {
    if width <= MAX_WIDTH {
        (width, height)
    } else {
        (MAX_WIDTH, (height * MAX_WIDTH / width).max(1))
    }
}

fn play_ffmpeg(
    sink: &hydra_live::Sink,
    input: &OsStr,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let (width, height) = fit(width, height);
    // `-re` paces it at the file's own rate and `-stream_loop -1` loops it;
    // `-noautorotate` keeps the frames the size ffprobe reported.
    let mut child = tool("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-noautorotate"])
        .args(["-user_agent", USER_AGENT, "-re", "-stream_loop", "-1", "-i"])
        .arg(input)
        .args(["-an", "-vf", &format!("scale={width}:{height}")])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let mut frames = child.stdout.take().ok_or("ffmpeg gave no output")?;
    let mut frame = vec![0; (width * height * 4) as usize];
    let mut result = Ok(());
    while sink.wanted() {
        if frames.read_exact(&mut frame).is_err() {
            let mut why = String::new();
            if let Some(mut stderr) = child.stderr.take() {
                let _ = stderr.read_to_string(&mut why);
            }
            result = Err(format!("ffmpeg stopped: {}", why.trim()));
            break;
        }
        sink.publish(Picture {
            width,
            height,
            rgba: frame.clone(),
        });
    }
    let _ = child.kill();
    let _ = child.wait();
    result
}

/// The built-in reader, for when no `ffmpeg` is installed.
fn play_mp4(sink: &hydra_live::Sink, url: &str) -> Result<(), String> {
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
            .ok_or(
                "no H.264 video track (without ffmpeg installed, rudel reads MP4 with H.264 only)",
            )?;
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
    fn a_wide_video_is_scaled_to_a_background_s_width() {
        assert_eq!(fit(640, 480), (640, 480));
        assert_eq!(fit(3840, 2160), (1280, 720));
    }

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
