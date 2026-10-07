//! Hydra's looping, muted video source, decoded by the FFmpeg libraries built
//! into Rudel. Web videos stream directly, without downloading the whole file.

use super::{hydra_images::Picture, hydra_live};
use eframe::egui;
use rudel_ffmpeg::Video;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const USER_AGENT: &str = concat!(
    "rudel/",
    env!("CARGO_PKG_VERSION"),
    " (live-coding music app)"
);
const MAX_WIDTH: u32 = 1280;

pub(super) fn frame(ctx: &egui::Context, url: &str) -> Option<Arc<Picture>> {
    let owned = url.to_string();
    hydra_live::latest(ctx, &format!("initVideo({url})"), move |sink| {
        let result = play(sink, &owned);
        if sink.wanted() { result } else { Ok(()) }
    })
}

fn play(sink: &hydra_live::Sink, url: &str) -> Result<(), String> {
    let input = if url.starts_with("http://") || url.starts_with("https://") {
        url.to_owned()
    } else {
        rudel_audio::samples::fetch_cached_file(url)?
            .to_str()
            .ok_or("video path is not UTF-8")?
            .to_owned()
    };
    let wanted = || sink.wanted();
    let mut video = Video::open(&input, USER_AGENT, MAX_WIDTH, &wanted)?;
    while wanted() {
        let mut clock = PlaybackClock::default();
        while let Some(frame) = video.next_frame()? {
            let at = clock.advance(frame.timestamp, frame.duration);
            if !wait_until(clock.started, at, &wanted) {
                return Ok(());
            }
            sink.publish(Picture {
                width: frame.width,
                height: frame.height,
                rgba: frame.rgba,
            });
        }
        if clock.origin.is_none() {
            return Err("video contains no frames".into());
        }
        if !wait_until(clock.started, clock.next, &wanted) {
            return Ok(());
        }
        if video.rewind().is_err() {
            // Some HTTP sources cannot seek. Reopening starts the next loop.
            video = Video::open(&input, USER_AGENT, MAX_WIDTH, &wanted)?;
        }
    }
    Ok(())
}

struct PlaybackClock {
    started: Instant,
    origin: Option<f64>,
    next: Duration,
}

impl Default for PlaybackClock {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            origin: None,
            next: Duration::ZERO,
        }
    }
}

impl PlaybackClock {
    fn advance(&mut self, timestamp: Option<f64>, duration: Duration) -> Duration {
        // Start at the first decoded frame, so probing/network startup does
        // not turn into a burst of frames trying to catch up with wall time.
        if self.origin.is_none() {
            self.started = Instant::now();
            self.origin = Some(timestamp.unwrap_or(0.0));
        }
        let at = timestamp
            .and_then(|pts| Duration::try_from_secs_f64(pts - self.origin.unwrap()).ok())
            .unwrap_or(self.next);
        self.next = at.saturating_add(duration);
        at
    }
}

fn wait_until(start: Instant, at: Duration, wanted: &dyn Fn() -> bool) -> bool {
    while wanted() {
        let remaining = at.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return true;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(20)));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_timing_uses_pts_and_falls_back_to_frame_duration() {
        let mut clock = PlaybackClock::default();
        let step = Duration::from_millis(40);
        assert_eq!(clock.advance(Some(10.0), step), Duration::ZERO);
        assert_eq!(clock.advance(Some(10.5), step), Duration::from_millis(500));
        assert_eq!(clock.advance(None, step), Duration::from_millis(540));
        assert_eq!(clock.advance(Some(-1.0), step), Duration::from_millis(580));
        assert!(!wait_until(
            Instant::now(),
            Duration::from_secs(100),
            &|| false
        ));
    }
}
