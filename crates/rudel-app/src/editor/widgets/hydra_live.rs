//! hydra's live sources (`initCam`, `initVideo`, `initScreen`): pictures that
//! keep changing.
//!
//! Each source in use runs on a thread of its own, keeping its latest frame
//! for the painter. One nothing has read for a few seconds (its script was
//! replaced) stops, which releases the camera or file; the next read starts it
//! afresh.

use super::hydra_images::Picture;
use eframe::egui;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

/// How long a source keeps running unread.
const IDLE: Duration = Duration::from_secs(3);
/// How long a source that failed waits before it is tried again.
const RETRY: Duration = Duration::from_secs(5);

struct Feed {
    latest: Option<Arc<Picture>>,
    read: Instant,
    /// When it failed, if it did; the entry stays so the error logs once.
    failed: Option<Instant>,
}

static FEEDS: LazyLock<Mutex<HashMap<String, Arc<Mutex<Feed>>>>> = LazyLock::new(Default::default);

/// The errors already logged, so a retried source does not repeat one.
static LOGGED: LazyLock<Mutex<std::collections::HashSet<String>>> = LazyLock::new(Default::default);

/// What a source's thread hands its frames to.
pub(super) struct Sink {
    ctx: egui::Context,
    feed: Arc<Mutex<Feed>>,
}

impl Sink {
    /// Whether anything still shows this source; a thread stops once not.
    pub(super) fn wanted(&self) -> bool {
        lock(&self.feed).read.elapsed() < IDLE
    }

    pub(super) fn publish(&self, picture: Picture) {
        lock(&self.feed).latest = Some(Arc::new(picture));
        self.ctx.request_repaint();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The latest frame of the source `key`, starting `run` for it on first use.
/// `run` returns once the sink is no longer wanted, or with what went wrong,
/// which is logged.
pub(super) fn latest(
    ctx: &egui::Context,
    key: &str,
    run: impl FnOnce(&Sink) -> Result<(), String> + Send + 'static,
) -> Option<Arc<Picture>> {
    let mut feeds = lock(&FEEDS);
    if let Some(feed) = feeds.get(key) {
        let mut feed = lock(feed);
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
    feeds.insert(key.to_string(), feed.clone());
    let (ctx, key) = (ctx.clone(), key.to_string());
    std::thread::spawn(move || {
        let sink = Sink {
            ctx,
            feed: feed.clone(),
        };
        if let Err(e) = run(&sink) {
            // Retried every few seconds (a camera plugged in late), but said
            // once: the same error again is noise.
            let said = format!("{key}: {e}");
            if lock(&LOGGED).insert(said.clone()) {
                rudel_core::log_line(format!("hydra: {said}"));
            }
            lock(&feed).failed = Some(Instant::now());
            return;
        }
        let mut feeds = lock(&FEEDS);
        if feeds.get(&key).is_some_and(|f| Arc::ptr_eq(f, &feed)) {
            feeds.remove(&key);
        }
    });
    None
}
