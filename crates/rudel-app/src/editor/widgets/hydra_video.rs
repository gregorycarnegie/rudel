//! hydra's `s0.initVideo(url)`: a video file as a live source (`hydra_live`),
//! looping and muted like upstream's `<video>` element.
//!
//! FFmpeg plays it: any format it reads (MP4, WebM, MOV, GIF, …), at the
//! file's own pace, looping, its raw RGBA frames read from a pipe. Releases
//! ship `ffmpeg` next to the `rudel` executable, where `ffmpeg-sidecar` looks
//! first; a build without one falls back to the `ffmpeg` on the PATH.

use super::{hydra_images::Picture, hydra_live};
use eframe::egui;
use ffmpeg_sidecar::{
    command::FfmpegCommand,
    event::{FfmpegEvent, LogLevel},
};
use std::{ffi::OsString, sync::Arc};

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
    let mut command = FfmpegCommand::new();
    let input = if url.starts_with("http://") || url.starts_with("https://") {
        // An http option: ffmpeg refuses to open a local file given it.
        command.args(["-user_agent", USER_AGENT]);
        OsString::from(url)
    } else {
        rudel_audio::samples::fetch_cached_file(url)?.into_os_string()
    };
    // `-re` paces it at the file's own rate and `-stream_loop -1` loops it.
    let mut child = command
        .hide_banner()
        .args(["-re", "-stream_loop", "-1"])
        .input(&input)
        .no_audio()
        .filter(format!("scale='min(iw,{MAX_WIDTH})':-1"))
        .args(["-f", "rawvideo", "-pix_fmt", "rgba"])
        .output("-")
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "initVideo needs ffmpeg, next to rudel or on the PATH".to_string()
            }
            _ => format!("ffmpeg: {e}"),
        })?;
    let mut errors = Vec::new();
    for event in child.iter().map_err(|e| e.to_string())? {
        if !sink.wanted() {
            break;
        }
        match event {
            FfmpegEvent::OutputFrame(frame) => sink.publish(Picture {
                width: frame.width,
                height: frame.height,
                rgba: frame.data,
            }),
            FfmpegEvent::Error(e) | FfmpegEvent::Log(LogLevel::Error | LogLevel::Fatal, e) => {
                errors.push(e)
            }
            _ => {}
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    if sink.wanted() {
        Err(format!("ffmpeg stopped: {}", errors.join("; ")))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_video_plays_scaled_to_a_background_s_width() {
        if !ffmpeg_sidecar::command::ffmpeg_is_installed() {
            eprintln!("skipped: no ffmpeg");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wide.webm");
        let made = FfmpegCommand::new()
            .args(["-f", "lavfi", "-i", "testsrc=size=1600x200:duration=1"])
            .output(path.to_str().unwrap())
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
        assert!(made.success());
        let ctx = egui::Context::default();
        let url = path.to_str().unwrap();
        let started = std::time::Instant::now();
        let picture = loop {
            if let Some(picture) = frame(&ctx, url) {
                break picture;
            }
            assert!(started.elapsed().as_secs() < 20, "no frame");
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert_eq!((picture.width, picture.height), (1280, 160));
        assert_eq!(picture.rgba.len(), 1280 * 160 * 4);
    }
}
