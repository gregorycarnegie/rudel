//! The pictures hydra's `s0.initImage(url)` loads: fetched and decoded off the
//! UI thread, once per URL, through the sample cache (so a picture used last
//! session does not download again).

use eframe::egui;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

/// A decoded picture, RGBA8 rows top first.
pub(super) struct Picture {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba: Vec<u8>,
}

enum Slot {
    Loading,
    Ready(Arc<Picture>),
    Failed,
}

static PICTURES: LazyLock<Mutex<HashMap<String, Slot>>> = LazyLock::new(Default::default);

/// No side longer than this: a texture's limit on most GPUs is 8192, and a
/// background never needs more.
const MAX_SIDE: u32 = 4096;

/// The picture at `url`, once it has loaded. The first call starts the load
/// and repaints when it lands; a failure is logged to the console once.
pub(super) fn picture(ctx: &egui::Context, url: &str) -> Option<Arc<Picture>> {
    let mut pictures = PICTURES.lock().unwrap_or_else(|e| e.into_inner());
    match pictures.get(url) {
        Some(Slot::Ready(picture)) => return Some(picture.clone()),
        Some(Slot::Loading | Slot::Failed) => return None,
        None => {}
    }
    pictures.insert(url.to_string(), Slot::Loading);
    let (ctx, url) = (ctx.clone(), url.to_string());
    std::thread::spawn(move || {
        let slot = match load(&url) {
            Ok(picture) => Slot::Ready(Arc::new(picture)),
            Err(e) => {
                rudel_core::log_line(format!("hydra: initImage {url}: {e}"));
                Slot::Failed
            }
        };
        PICTURES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(url, slot);
        ctx.request_repaint();
    });
    None
}

fn load(url: &str) -> Result<Picture, String> {
    let bytes = rudel_audio::samples::fetch_cached_bytes(url)?;
    let mut image = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    if image.width() > MAX_SIDE || image.height() > MAX_SIDE {
        image = image.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    let rgba = image.into_rgba8();
    Ok(Picture {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}
