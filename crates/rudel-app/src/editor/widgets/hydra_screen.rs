//! hydra's `s0.initScreen()`: the primary monitor as a live source
//! (`hydra_live`). Upstream asks the browser, which lets the user pick a
//! screen or window; here it is the primary monitor, about 15 times a second,
//! no wider than 1280 so a 4K desktop does not cost a 4K texture.

use super::hydra_images::Picture;
use eframe::egui;
use std::sync::Arc;

#[cfg(any(windows, target_os = "macos"))]
pub(super) fn frame(ctx: &egui::Context) -> Option<Arc<Picture>> {
    super::hydra_live::latest(ctx, "initScreen()", capture)
}

/// Screen capture is not built on Linux (see `Cargo.toml`): say so once.
#[cfg(not(any(windows, target_os = "macos")))]
pub(super) fn frame(_ctx: &egui::Context) -> Option<Arc<Picture>> {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        rudel_core::log_line(
            "hydra: initScreen(): screen capture is not available on Linux".to_string(),
        );
    });
    None
}

#[cfg(any(windows, target_os = "macos"))]
fn capture(sink: &super::hydra_live::Sink) -> Result<(), String> {
    use std::time::{Duration, Instant};
    const FRAME: Duration = Duration::from_millis(66);
    const MAX_WIDTH: u32 = 1280;
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or(monitors.first())
        .ok_or("no monitor to capture")?;
    while sink.wanted() {
        let started = Instant::now();
        let mut image = monitor.capture_image().map_err(|e| e.to_string())?;
        if image.width() > MAX_WIDTH {
            let height = image.height() * MAX_WIDTH / image.width();
            image = image::imageops::thumbnail(&image, MAX_WIDTH, height);
        }
        sink.publish(Picture {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        });
        if let Some(wait) = FRAME.checked_sub(started.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    Ok(())
}
