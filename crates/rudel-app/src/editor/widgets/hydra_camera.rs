//! hydra's `s0.initCam(index)`: a webcam as a live source.
//!
//! Each camera in use runs on a thread of its own, keeping the latest decoded
//! frame for the painter. A camera nothing has read for a few seconds (its
//! script was replaced) stops, which releases the device.

use super::hydra_images::Picture;
use eframe::egui;
use nokhwa::{
    Camera,
    pixel_format::RgbFormat,
    utils::{
        CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
    },
};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

/// How long a camera keeps running unread.
const IDLE: Duration = Duration::from_secs(3);
/// How long a camera that failed to open waits before it is tried again.
const RETRY: Duration = Duration::from_secs(5);

struct Feed {
    latest: Option<Arc<Picture>>,
    read: Instant,
    /// When it failed, if it did; the entry stays so the error logs once.
    failed: Option<Instant>,
}

static CAMERAS: LazyLock<Mutex<HashMap<u32, Arc<Mutex<Feed>>>>> = LazyLock::new(Default::default);

/// The latest frame of camera `index`, starting it on first use.
pub(super) fn frame(ctx: &egui::Context, index: u32) -> Option<Arc<Picture>> {
    let mut cameras = CAMERAS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(feed) = cameras.get(&index) {
        let mut feed = feed.lock().unwrap_or_else(|e| e.into_inner());
        if feed.failed.is_none_or(|at| at.elapsed() < RETRY) {
            feed.read = Instant::now();
            return feed.latest.clone();
        }
    }
    let feed = Arc::new(Mutex::new(Feed {
        latest: None,
        read: Instant::now(),
        failed: None,
    }));
    cameras.insert(index, feed.clone());
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        if let Err(e) = capture(&ctx, index, &feed) {
            rudel_core::log_line(format!("hydra: initCam({index}): {e}"));
            feed.lock().unwrap_or_else(|e| e.into_inner()).failed = Some(Instant::now());
            return;
        }
        // Stopped for want of readers: let the next read start it afresh.
        let mut cameras = CAMERAS.lock().unwrap_or_else(|e| e.into_inner());
        if cameras.get(&index).is_some_and(|f| Arc::ptr_eq(f, &feed)) {
            cameras.remove(&index);
        }
    });
    None
}

fn capture(ctx: &egui::Context, index: u32, feed: &Mutex<Feed>) -> Result<(), String> {
    // 720p at 30 fps where the camera offers it: what a background needs,
    // where asking for the highest rate can pick 1080p at 1 fps.
    let wanted = CameraFormat::new(Resolution::new(1280, 720), FrameFormat::MJPEG, 30);
    let mut camera = Camera::new(
        CameraIndex::Index(index),
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(wanted)),
    )
    .map_err(|e| e.to_string())?;
    camera.open_stream().map_err(|e| e.to_string())?;
    while feed
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .read
        .elapsed()
        < IDLE
    {
        let buffer = camera.frame().map_err(|e| e.to_string())?;
        let resolution = buffer.resolution();
        let picture = decode(
            buffer.source_frame_format(),
            resolution.width(),
            resolution.height(),
            buffer.buffer(),
        )?;
        feed.lock().unwrap_or_else(|e| e.into_inner()).latest = Some(Arc::new(picture));
        ctx.request_repaint();
    }
    Ok(())
}

/// A frame as RGBA. Most webcams send MJPEG; YUYV is the usual
/// uncompressed fallback.
fn decode(format: FrameFormat, width: u32, height: u32, data: &[u8]) -> Result<Picture, String> {
    let rgba = match format {
        FrameFormat::MJPEG => image::load_from_memory(data)
            .map_err(|e| e.to_string())?
            .into_rgba8()
            .into_raw(),
        FrameFormat::YUYV => yuyv_to_rgba(data),
        FrameFormat::RAWRGB => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        other => {
            return Err(format!(
                "the camera sends {other:?}, which rudel cannot read"
            ));
        }
    };
    if rgba.len() != (width * height * 4) as usize {
        return Err(format!(
            "a {width}x{height} frame came out as {} bytes",
            rgba.len()
        ));
    }
    Ok(Picture {
        width,
        height,
        rgba,
    })
}

/// YUYV 4:2:2 (BT.601, studio range): each four bytes are two pixels sharing
/// their chroma.
fn yuyv_to_rgba(data: &[u8]) -> Vec<u8> {
    let pixel = |y: u8, u: u8, v: u8| {
        let c = 1.164 * (f32::from(y) - 16.0);
        let (d, e) = (f32::from(u) - 128.0, f32::from(v) - 128.0);
        let clamp = |x: f32| x.round().clamp(0.0, 255.0) as u8;
        [
            clamp(c + 1.596 * e),
            clamp(c - 0.392 * d - 0.813 * e),
            clamp(c + 2.017 * d),
            255,
        ]
    };
    data.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[y0, u, y1, v]| [pixel(y0, u, v), pixel(y1, u, v)])
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuyv_reads_black_white_and_grey() {
        // Studio range: Y 16 is black, 235 white; neutral chroma is 128.
        let rgba = yuyv_to_rgba(&[16, 128, 235, 128]);
        assert_eq!(rgba, [0, 0, 0, 255, 255, 255, 255, 255]);
        let picture = decode(FrameFormat::YUYV, 2, 1, &[126, 128, 126, 128]).expect("decodes");
        assert!(
            picture.rgba[..3].iter().all(|&c| c.abs_diff(128) <= 1),
            "{:?}",
            picture.rgba
        );
        assert!(
            decode(FrameFormat::YUYV, 4, 1, &[16, 128, 235, 128]).is_err(),
            "short frame"
        );
    }
}
