use super::{
    options::{DRAW_LOOKBEHIND, VisualWidgetOptions},
    query::hap_is_active,
    style::{WidgetDrawColors, color_with_alpha, event_alpha, event_color},
};
use eframe::egui;
use rudel_core::Hap;

pub(super) fn paint_spiral(
    ui: &egui::Ui,
    rect: egui::Rect,
    haps: &[Hap],
    time: f64,
    colors: WidgetDrawColors,
    options: VisualWidgetOptions,
) {
    let painter = ui.painter();
    let size = options.spiral_size;
    let stretch = options.stretch;
    let margin = size / stretch;
    let thickness = options.spiral_thickness.unwrap_or(size / 2.0);
    let inset = options.inset;
    let rotate = options.steady * time as f32;
    let fade_span = DRAW_LOOKBEHIND.abs() as f32;

    for hap in haps {
        let Some(whole) = hap.whole else {
            continue;
        };
        let begin = whole.begin.to_f64();
        let from = (begin - time) as f32 + inset;
        let to = (hap.end_clipped().to_f64() - time) as f32 + inset - options.padding;
        if to <= from {
            continue;
        }
        let active = hap_is_active(hap, time);
        let active_color = options.active_color.unwrap_or(colors.active);
        let inactive_color = options.inactive_color.unwrap_or(colors.inactive);
        let base = if active || options.colorize_spiral_inactive {
            event_color(hap, active_color)
        } else {
            inactive_color
        };
        let opacity = if options.fade {
            let distance = ((begin - time) as f32).abs();
            (1.0 - distance / fade_span).clamp(0.08, 1.0)
        } else {
            1.0
        };
        paint_spiral_segment(
            painter,
            rect.center(),
            SpiralSegment {
                from,
                to,
                margin,
                rotate,
                stretch,
                thickness,
                color: color_with_alpha(base, opacity * event_alpha(hap)),
            },
        );
    }

    paint_spiral_segment(
        painter,
        rect.center(),
        SpiralSegment {
            from: inset - options.playhead_length,
            to: inset,
            margin,
            rotate,
            stretch,
            thickness: options.playhead_thickness.unwrap_or(thickness),
            color: options.playhead_color.unwrap_or(colors.active),
        },
    );
}

#[derive(Clone, Copy)]
struct SpiralSegment {
    from: f32,
    to: f32,
    margin: f32,
    rotate: f32,
    stretch: f32,
    thickness: f32,
    color: egui::Color32,
}

fn paint_spiral_segment(painter: &egui::Painter, center: egui::Pos2, segment: SpiralSegment) {
    let mut points = Vec::new();
    let mut angle = segment.from;
    while angle <= segment.to {
        points.push(spiral_point(
            angle,
            segment.margin,
            center,
            segment.rotate,
            segment.stretch,
        ));
        angle += 1.0 / 60.0;
    }
    points.push(spiral_point(
        segment.to,
        segment.margin,
        center,
        segment.rotate,
        segment.stretch,
    ));
    if points.len() >= 2 {
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(segment.thickness, segment.color),
        ));
    }
}

pub(super) fn spiral_point(
    angle: f32,
    margin: f32,
    center: egui::Pos2,
    rotate: f32,
    stretch: f32,
) -> egui::Pos2 {
    let angle = angle * stretch;
    let rotate = rotate * stretch;
    let radians = ((angle + rotate) * 360.0 - 90.0).to_radians();
    let radius = margin * angle;
    egui::pos2(
        center.x + radians.cos() * radius,
        center.y + radians.sin() * radius,
    )
}
