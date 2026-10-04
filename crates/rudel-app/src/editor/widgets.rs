mod analyzer;
mod claviature;
mod geometry;
mod host;
mod hydra_gpu;
mod hydra_images;
mod options;
mod paint;
mod pianoroll;
mod pitchwheel;
mod query;
mod shader;
mod size;
mod spiral;
mod spiral_gpu;
mod style;
mod values;
mod visual;

#[cfg(test)]
mod tests;

pub(crate) use geometry::{WidgetLayout, block_widget_line_heights};
pub(crate) use host::WidgetHostState;
pub(crate) use hydra_gpu::HydraStore;
pub(crate) use paint::{WidgetPaintInput, draw_widget_hosts, paint_backdrop};
pub(crate) use shader::ShaderStore;
pub(crate) use spiral_gpu::{SpiralStore, supported as spiral_gpu_supported};
pub(crate) use style::mark_color;

/// How long a GPU widget's cached pipeline or buffers outlive its last paint.
/// Editing a widget's source shifts its id, so ids do accumulate.
const IDLE_EVICTION: std::time::Duration = std::time::Duration::from_secs(30);

/// The key a GPU painter keeps a widget's buffers under. A popped-out widget
/// is drawn in two windows each frame, inline and in its own, at two sizes, so
/// one key per widget would rebuild its buffers twice a frame and draw both
/// from whichever wrote last.
fn gpu_key(ui: &eframe::egui::Ui, id: &str) -> String {
    format!("{id}@{:?}", ui.ctx().viewport_id())
}

/// Drop every cache entry but `keep`'s that has not painted within
/// [`IDLE_EVICTION`]; `used` reads an entry's last paint.
fn evict_idle<T>(
    cache: &mut std::collections::HashMap<String, T>,
    keep: &str,
    used: impl Fn(&T) -> std::time::Instant,
) {
    let cutoff = std::time::Instant::now() - IDLE_EVICTION;
    cache.retain(|key, entry| key == keep || used(entry) > cutoff);
}

#[cfg(test)]
mod evict_tests {
    use super::*;
    use std::{collections::HashMap, time::Instant};

    #[test]
    fn an_idle_entry_is_evicted_but_a_recent_one_and_the_kept_one_stay() {
        let long_ago = Instant::now() - IDLE_EVICTION * 2;
        let mut cache = HashMap::from([
            ("idle".to_string(), long_ago),
            ("recent".to_string(), Instant::now()),
            ("kept".to_string(), long_ago),
        ]);
        evict_idle(&mut cache, "kept", |&used| used);
        let mut left: Vec<&str> = cache.keys().map(String::as_str).collect();
        left.sort();
        assert_eq!(left, ["kept", "recent"]);
    }
}
