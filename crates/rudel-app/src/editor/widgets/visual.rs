use super::{
    analyzer::{paint_fscope, paint_scope, paint_spectrum},
    claviature::paint_claviature,
    hydra_gpu::paint_hydra_gpu,
    options::{DrawWindow, VisualWidgetOptions},
    paint::WidgetPaintInput,
    pianoroll::paint_pianoroll,
    pitchwheel::paint_pitchwheel,
    query::{hap_is_active, in_window, widget_haps},
    shader::paint_shader,
    spiral::paint_spiral,
    spiral_gpu::paint_spiral_gpu,
    style::{WidgetDrawColors, event_color},
};
use crate::editor::decorations::WidgetDecoration;
use eframe::egui;
use rudel_core::Pattern;
use std::sync::Arc;

/// The id `rudel_lang` gives the hydra scene drawn behind the code.
const BACKDROP_ID: &str = "hydra-background";

pub(super) fn paint_pattern_widget(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget: &WidgetDecoration,
    pattern: &Pattern,
    colors: WidgetDrawColors,
    paint: WidgetPaintInput<'_>,
) -> bool {
    let time = paint.time_cycles.unwrap_or(0.0);
    let options = VisualWidgetOptions::from_widget(widget);
    // The cached whole cycles, shared not copied; `in_window` narrows each use
    // of them to what the widget actually draws.
    let cycles = |window| widget_haps(ui.ctx(), paint.pattern_generation, pattern, widget, window);
    // The audio ring feeding an analyzer widget: the tap registered under this
    // widget's id, filled by the voices whose haps carry the widget tag.
    let widget_tap = || paint.taps.map(|taps| taps.get_or_create(&widget.id));
    // Strudel's tscope/spectrum color the trace by the active hap's `color`
    // control, falling back to the theme foreground.
    let hap_color = |fallback: Option<egui::Color32>| {
        let window = DrawWindow::around(time);
        in_window(&cycles(window), window)
            .into_iter()
            .find(|hap| hap_is_active(hap, time))
            .map(|hap| event_color(hap, colors.foreground))
            .or(fallback)
    };
    match widget.widget_type.as_str() {
        "_pianoroll" | "_punchcard" => {
            let window = options.window(time);
            let cycles = cycles(window);
            let haps = in_window(&cycles, window);
            paint_pianoroll(ui, rect, &widget.id, &haps, time, colors, options);
            true
        }
        "_wordfall" => {
            let window = options.window(time);
            let cycles = cycles(window);
            let haps = in_window(&cycles, window);
            paint_pianoroll(
                ui,
                rect,
                &widget.id,
                &haps,
                time,
                colors,
                options.with_wordfall_defaults(widget),
            );
            true
        }
        "_pitchwheel" => {
            let window = DrawWindow::around(time);
            let cycles = cycles(window);
            let haps = in_window(&cycles, window)
                .into_iter()
                .filter(|hap| hap_is_active(hap, time))
                .collect::<Vec<_>>();
            paint_pitchwheel(ui, rect, &haps, colors, options);
            true
        }
        "_spiral" => {
            let window = DrawWindow::around(time);
            let cycles = cycles(window);
            let haps = in_window(&cycles, window);
            let options = options.zoomed(paint.zoom);
            if options.gpu && paint.gpu_available {
                paint_spiral_gpu(ui, rect, &widget.id, &haps, time, colors, options);
            } else {
                paint_spiral(ui, rect, &haps, time, colors, options);
            }
            true
        }
        "_claviature" => {
            let window = DrawWindow::around(time);
            let cycles = cycles(window);
            let haps = in_window(&cycles, window)
                .into_iter()
                .filter(|hap| hap_is_active(hap, time))
                .collect::<Vec<_>>();
            paint_claviature(ui, rect, &haps, colors, options);
            true
        }
        "_shader" => {
            let window = DrawWindow::around(time);
            let cycles = cycles(window);
            let haps = in_window(&cycles, window)
                .into_iter()
                .filter(|hap| hap_is_active(hap, time))
                .collect::<Vec<_>>();
            paint_shader(ui, rect, widget, &haps, time, colors);
            true
        }
        "_hydra" => {
            let window = DrawWindow::around(time);
            let cycles = cycles(window);
            let haps = in_window(&cycles, window)
                .into_iter()
                .filter(|hap| hap_is_active(hap, time))
                .collect::<Vec<_>>();
            // The scene behind the code keeps hydra's own clock, in seconds
            // and running whether or not the transport is, as upstream's
            // canvas does; an inline widget follows the cycle.
            let time = if widget.id == BACKDROP_ID {
                paint.hydra_time
            } else {
                time
            };
            // `initHydra({feedStrudel})`: the canvas visuals become `s0`.
            let feed = (super::options::option_bool(&widget.options, "feed") == Some(true))
                .then(|| capture_canvas(ui, rect, pattern, colors, paint));
            paint_hydra_gpu(
                ui,
                rect,
                widget,
                &haps,
                time,
                colors,
                paint.hydra_values,
                feed,
            );
            true
        }
        "_scope" => {
            let color = options
                .active_color
                .or_else(|| hap_color(None))
                .unwrap_or(colors.foreground);
            paint_scope(
                ui,
                rect,
                &widget.id,
                widget_tap().as_deref(),
                options,
                color,
            );
            true
        }
        "_fscope" => {
            let color = options.active_color.unwrap_or(colors.foreground);
            paint_fscope(
                ui,
                rect,
                &widget.id,
                widget_tap().as_deref(),
                options,
                color,
            );
            true
        }
        "_spectrum" => {
            let hap_color = options.active_color.or_else(|| hap_color(None));
            paint_spectrum(
                ui,
                rect,
                &widget.id,
                widget_tap().as_deref(),
                options,
                hap_color,
                colors.foreground,
            );
            true
        }
        _ => false,
    }
}

/// The canvas visuals (`.scope()`, `.pianoroll()`, …) painted over `rect` as
/// upstream's full-screen canvas would show them, tessellated and moved to
/// `rect`'s corner, for hydra to read as `s0`.
///
/// They paint onto a layer of their own, which is emptied again before egui
/// draws it. CPU painters only, and no text: the meshes are drawn with a
/// one-white-pixel font texture, which suits every shape but glyphs.
fn capture_canvas(
    ui: &egui::Ui,
    rect: egui::Rect,
    pattern: &Pattern,
    colors: WidgetDrawColors,
    paint: WidgetPaintInput<'_>,
) -> Feed {
    let ctx = ui.ctx();
    let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("rudel-hydra-feed"));
    let mut canvas_ui = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("rudel-hydra-feed-ui"),
        egui::UiBuilder::new().layer_id(layer).max_rect(rect),
    );
    canvas_ui.set_clip_rect(rect);
    let canvas = paint.canvas;
    let paint = WidgetPaintInput {
        gpu_available: false,
        canvas: &[],
        ..paint
    };
    for widget in canvas {
        paint_pattern_widget(&canvas_ui, rect, widget, pattern, colors, paint);
    }
    let shapes: Vec<egui::epaint::ClippedShape> = ctx.graphics_mut(|g| {
        std::mem::take(g.entry(layer))
            .all_entries()
            .cloned()
            .collect()
    });
    let offset = -rect.min.to_vec2();
    let shapes: Vec<_> = shapes
        .into_iter()
        .filter(|s| !matches!(s.shape, egui::Shape::Callback(_)))
        .map(|mut s| {
            s.shape.translate(offset);
            s.clip_rect = s.clip_rect.translate(offset);
            s
        })
        .collect();
    let has_text = shapes
        .iter()
        .any(|s| matches!(s.shape, egui::Shape::Text(_)));
    Feed {
        primitives: ctx.tessellate(shapes, ctx.pixels_per_point()),
        atlas: has_text.then(|| font_atlas(ctx)).flatten(),
    }
}

/// The canvas visuals for `s0`, tessellated, and the font atlas their text
/// samples when the renderer needs a fresh copy of it.
pub(super) struct Feed {
    pub(super) primitives: Vec<egui::epaint::ClippedPrimitive>,
    pub(super) atlas: Option<Arc<egui::ColorImage>>,
}

/// egui's font atlas, for the feed renderer to draw text with: when it has
/// grown, and otherwise at most twice a second (glyphs are added as text is
/// laid out; copying the whole atlas every frame would cost megabytes).
fn font_atlas(ctx: &egui::Context) -> Option<Arc<egui::ColorImage>> {
    let id = egui::Id::new("rudel-hydra-feed-atlas");
    let now = ctx.input(|i| i.time);
    let size = ctx.fonts(|f| f.font_image_size());
    let last: Option<([usize; 2], f64)> = ctx.data(|d| d.get_temp(id));
    if last.is_some_and(|(s, at)| s == size && now - at < 0.5) {
        return None;
    }
    ctx.data_mut(|d| d.insert_temp(id, (size, now)));
    Some(Arc::new(ctx.fonts(|f| f.image())))
}
