//! The pictures hydra's `s0.initImage(url)` loads: fetched and decoded off the
//! UI thread, once per URL, through the sample cache (so a picture used last
//! session does not download again).

use eframe::egui;
use std::{
    collections::HashMap,
    path::PathBuf,
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

/// The folder of the file being edited, which a relative picture path is
/// read from.
static BASE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Read relative picture paths from `dir` (the open file's folder).
pub(crate) fn set_base_dir(dir: Option<PathBuf>) {
    *BASE_DIR.lock().unwrap_or_else(|e| e.into_inner()) = dir;
}

/// Where `url` points: an http(s) URL as it is; a `file://` URL as its path;
/// a relative path against the open file's folder; anything else (an
/// absolute path, `~/…`) as it is.
fn resolve(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    let path = match url.strip_prefix("file://") {
        Some(rest) => {
            let rest = rest.strip_prefix("localhost").unwrap_or(rest);
            // `file:///C:/x.png`: the slash before a drive letter is not part
            // of a Windows path.
            let bytes = rest.as_bytes();
            let rest = if bytes.len() > 2 && bytes[0] == b'/' && bytes[2] == b':' {
                &rest[1..]
            } else {
                rest
            };
            percent_decode(rest)
        }
        None => url.to_string(),
    };
    let relative = !path.starts_with('~') && std::path::Path::new(&path).is_relative();
    match BASE_DIR.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(dir) if relative => dir.join(&path).to_string_lossy().into_owned(),
        _ => path,
    }
}

/// `%20` and friends, as a `file://` URL writes them.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// No side longer than this: a texture's limit on most GPUs is 8192, and a
/// background never needs more.
const MAX_SIDE: u32 = 4096;

/// The picture at `url`, once it has loaded. The first call starts the load
/// and repaints when it lands; a failure is logged to the console once.
pub(super) fn picture(ctx: &egui::Context, url: &str) -> Option<Arc<Picture>> {
    // `s0.initCam(n)`: a new picture every frame the camera sends.
    if let Some(index) = camera_index(url) {
        return super::hydra_camera::frame(ctx, index);
    }
    let url = &resolve(url);
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

/// The webcam a `camera:N` source names (what `initCam(N)` records).
pub(super) fn camera_index(url: &str) -> Option<u32> {
    url.strip_prefix("camera:")?.parse().ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_url_resolves_to_where_it_is() {
        assert_eq!(
            resolve("https://e.org/a%20b.jpg"),
            "https://e.org/a%20b.jpg"
        );
        assert_eq!(
            resolve("file:///C:/My%20Pictures/a.png"),
            "C:/My Pictures/a.png"
        );
        assert_eq!(resolve("file:///home/me/a.png"), "/home/me/a.png");
        assert_eq!(resolve("~/a.png"), "~/a.png", "the fetch expands ~ itself");
        set_base_dir(Some(PathBuf::from("songs")));
        assert_eq!(
            PathBuf::from(resolve("pics/a.png")),
            PathBuf::from("songs").join("pics/a.png")
        );
        set_base_dir(None);
        assert_eq!(resolve("pics/a.png"), "pics/a.png");
    }
}
