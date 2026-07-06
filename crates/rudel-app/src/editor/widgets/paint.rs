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
    pub(crate) time_cycles: Option<f64>,
    pub(crate) draw_theme: DrawTheme,
    /// The engine's analyzer taps for scope/fscope/spectrum widgets (`None`
    /// when no audio device is running — those widgets then draw empty).
    pub(crate) taps: Option<&'a ScopeTaps>,
}

pub(crate) fn draw_widget_hosts(
    ui: &mut egui::Ui,
    code: &str,
    layout: WidgetLayout<'_>,
    widgets: &[WidgetDecoration],
    host: &mut WidgetHostState,
    paint: WidgetPaintInput<'_>,
) {
    let sync = host.sync(widgets);
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
    // Widgets are sorted by source position; stack any that share a line within
    // the gap reserved below that line.
    let mut stack_line = usize::MAX;
    let mut stack_offset = 0.0;
    for widget in widgets {
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
        egui::Area::new(egui::Id::new((
            "rudel-inline-widget",
            widget.widget_type.as_str(),
            widget.id.as_str(),
            surface.serial,
        )))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        // Scroll-anchored overlay: don't clamp back into the screen when the
        // anchor scrolls out of view (a constrained oversized surface would
        // slide over the editor), and let pointer input — wheel scrolling in
        // particular — fall through to the editor below; the visualizations
        // are display-only.
        .constrain(false)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            // Clip to the editor's visible area so the (foreground) overlay never
            // paints over the transport / errors / reference panels around it.
            ui.set_clip_rect(clip);
            ui.set_min_size(rect.size());
            let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
            paint_widget_surface(ui, rect, widget, surface, paint);
        });
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
    let stroke = egui::Stroke::new(1.0, colors.inactive);
    painter.rect_filled(rect, 4.0, colors.background);
    painter.rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Outside);

    let painted = paint
        .pattern
        .map(|pattern| {
            paint_pattern_widget(
                ui,
                rect,
                widget,
                pattern,
                paint.time_cycles,
                colors,
                paint.taps,
            )
        })
        .unwrap_or(false);

    if !painted {
        let left = egui::Rect::from_min_size(rect.min, egui::vec2(4.0, rect.height()));
        painter.rect_filled(left, 4.0, colors.active);
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
        colors.text,
    );
    painter.text(
        rect.right_top() + egui::vec2(-8.0, 8.0),
        egui::Align2::RIGHT_TOP,
        format!("#{}", surface.serial),
        egui::TextStyle::Small.resolve(ui.style()),
        colors.muted,
    );
}
