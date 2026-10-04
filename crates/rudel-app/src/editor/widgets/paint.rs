use super::{
    geometry::{WIDGET_GAP_PADDING, WidgetLayout, line_column_at_byte, widget_rect},
    host::{WidgetHostState, WidgetSurface},
    style::{WidgetDrawColors, widget_draw_colors},
    visual::paint_pattern_widget,
};
use crate::editor::{decorations::WidgetDecoration, settings::DrawTheme};
use eframe::egui;
use rudel_audio::ScopeTaps;
use rudel_core::Pattern;

#[derive(Clone, Copy)]
pub(crate) struct WidgetPaintInput<'a> {
    pub(crate) pattern: Option<&'a Pattern>,
    /// Bumped on every evaluation, so the widgets' hap cache drops results
    /// queried from the pattern this one replaced.
    pub(crate) pattern_generation: u64,
    pub(crate) time_cycles: Option<f64>,
    pub(crate) draw_theme: DrawTheme,
    /// The engine's analyzer taps for scope/fscope/spectrum widgets (`None`
    /// when no audio device is running — those widgets then draw empty).
    pub(crate) taps: Option<&'a ScopeTaps>,
    /// Whether this frame is rendered through the wgpu backend, on a device
    /// the GPU spiral can run on. A GPU painter draws nothing without the
    /// backend — its pipeline and buffers live in that renderer's callback
    /// resources — and panics on a device without fragment storage buffers, so
    /// a widget that prefers one falls back to its CPU painter instead.
    pub(crate) gpu_available: bool,
    /// How much bigger than its inline surface the widget is drawn: 1 inline,
    /// more when popped out. Only a painter with a fixed size (the spiral)
    /// needs it; the rest fill whatever rect they are given.
    pub(crate) zoom: f32,
    /// hydra's clock in seconds, for the scene behind the code.
    pub(crate) hydra_time: f64,
    /// This frame's values for the evaluation's per-frame hydra arguments
    /// (`H(pattern)`, arrays, functions), by slot.
    pub(crate) hydra_values: &'a [Option<f64>],
    /// The visuals `initHydra({feedStrudel})` feeds into `s0`: the ones spelled
    /// the public way (`.scope()`), drawn there instead of inline. Empty
    /// without a feed.
    pub(crate) canvas: &'a [WidgetDecoration],
}

pub(crate) fn draw_widget_hosts(
    ui: &mut egui::Ui,
    code: &str,
    layout: WidgetLayout<'_>,
    widgets: &[WidgetDecoration],
    backdrop: Option<&WidgetDecoration>,
    host: &mut WidgetHostState,
    paint: WidgetPaintInput<'_>,
) {
    // The backdrop has a surface like any widget, so it can pop out, but it
    // is drawn behind the code rather than inline.
    let all: Vec<WidgetDecoration> = widgets.iter().chain(backdrop).cloned().collect();
    let sync = host.sync(&all);
    if !sync.created.is_empty() || !sync.removed.is_empty() {
        ui.ctx().request_repaint();
    }
    // Free the audio-side analyzer rings of widgets that no longer exist.
    if let Some(taps) = paint.taps {
        for id in &sync.removed {
            taps.remove(id);
        }
    }

    let clip = ui.clip_rect();
    // Double-clicking a surface pops it out into its own window. The surfaces
    // let pointer input through to the editor, so look at the raw pointer.
    let double_click = ui
        .input(|i| {
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary)
                .then(|| i.pointer.interact_pos())
        })
        .flatten();
    let mut to_toggle = None;
    // Widgets are sorted by source position; stack any that share a line within
    // the gap reserved below that line.
    let mut stack_line = usize::MAX;
    let mut stack_offset = 0.0;
    for widget in widgets {
        // Fed into hydra rather than shown, as upstream hides its canvas.
        if paint.canvas.iter().any(|c| c.id == widget.id) {
            continue;
        }
        let Some(surface) = host.surface(widget) else {
            continue;
        };
        let (line, _) = line_column_at_byte(code, widget.placement());
        if line != stack_line {
            stack_line = line;
            stack_offset = 0.0;
        }
        let rect = widget_rect(layout, code, widget, surface.size, stack_offset);
        stack_offset += surface.size.y + WIDGET_GAP_PADDING;
        if !clip.intersects(rect) {
            continue;
        }
        if double_click.is_some_and(|pos| clip.intersect(rect).contains(pos)) {
            to_toggle = Some(widget);
        }
        egui::Area::new(egui::Id::new((
            "rudel-inline-widget",
            widget.widget_type.as_str(),
            widget.id.as_str(),
            surface.serial,
        )))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        // Bound the overlay to the editor's visible area: that clips its paint
        // to the editor (never over the panels around it) and — because egui
        // intersects an area's interact rect with the same bounds — keeps a
        // surface scrolled under the transport bar from swallowing clicks meant
        // for the buttons there. `constrain(false)` afterwards keeps the
        // scroll-anchored position from being clamped back into view (a
        // constrained oversized surface would slide over the editor).
        .constrain_to(clip)
        .constrain(false)
        // Let pointer input — wheel scrolling in particular — fall through to
        // the editor below; the visualizations are display-only.
        .interactable(false)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(rect.size());
            let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
            paint_widget_surface(ui, rect, widget, surface, paint);
        });
    }
    if let Some(widget) = to_toggle {
        host.toggle_popped(widget);
    }
    show_popped_widget(ui.ctx(), &all, host, paint);
}

/// A widget filling `rect` behind whatever is drawn after it: the hydra
/// scene behind the code.
pub(crate) fn paint_backdrop(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget: &WidgetDecoration,
    paint: WidgetPaintInput<'_>,
) {
    if let Some(pattern) = paint.pattern {
        let colors = widget_draw_colors(paint.draw_theme);
        paint_pattern_widget(ui, rect, widget, pattern, colors, paint);
    }
}

/// The popped-out widget, in a window of its own: drag it to another screen
/// and double-click for borderless fullscreen there, as a presentation. Esc
/// leaves fullscreen; closing the window docks the widget again.
fn show_popped_widget(
    ctx: &egui::Context,
    widgets: &[WidgetDecoration],
    host: &mut WidgetHostState,
    paint: WidgetPaintInput<'_>,
) {
    let Some((widget, surface)) = host.popped(widgets) else {
        return;
    };
    let title = format!("rudel: {}", widget.widget_type.trim_start_matches('_'));
    let builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([800.0, 600.0]);
    let dock = ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("rudel-widget-popout"),
        builder,
        |ui, _| {
            let (rect, response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
            let zoom = (rect.size() / surface.size).min_elem().max(1.0);
            paint_widget_surface(
                ui,
                rect,
                widget,
                surface,
                WidgetPaintInput { zoom, ..paint },
            );
            let (fullscreen, escape, close) = ui.input(|i| {
                (
                    i.viewport().fullscreen.unwrap_or(false),
                    i.key_pressed(egui::Key::Escape),
                    i.viewport().close_requested(),
                )
            });
            if response.double_clicked() || (escape && fullscreen) {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            close
        },
    );
    if dock {
        host.dock();
    }
}

fn paint_widget_surface(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget: &WidgetDecoration,
    surface: &WidgetSurface,
    paint: WidgetPaintInput<'_>,
) {
    let painter = ui.painter();
    let colors = widget_draw_colors(paint.draw_theme);
    let stroke = egui::Stroke::new(1.0, colors.muted);
    painter.rect_filled(rect, 4.0, colors.background);
    painter.rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Outside);

    let painted = paint
        .pattern
        .map(|pattern| paint_pattern_widget(ui, rect, widget, pattern, colors, paint))
        .unwrap_or(false);

    if !painted {
        let left = egui::Rect::from_min_size(rect.min, egui::vec2(4.0, rect.height()));
        painter.rect_filled(left, 4.0, colors.foreground);
        paint_widget_label(ui, rect, widget, surface, colors);
    }
}

fn paint_widget_label(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget: &WidgetDecoration,
    surface: &WidgetSurface,
    colors: WidgetDrawColors,
) {
    let painter = ui.painter();
    let title = widget.widget_type.trim_start_matches('_');
    painter.text(
        rect.left_top() + egui::vec2(12.0, 8.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::TextStyle::Monospace.resolve(ui.style()),
        colors.foreground,
    );
    painter.text(
        rect.right_top() + egui::vec2(-8.0, 8.0),
        egui::Align2::RIGHT_TOP,
        format!("#{}", surface.serial),
        egui::TextStyle::Small.resolve(ui.style()),
        colors.muted,
    );
}
