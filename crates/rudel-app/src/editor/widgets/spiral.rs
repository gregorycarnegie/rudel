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
    // Clip to the widget, the way a canvas clips: with `overscan` (and any
    // note that began before the window) a hap's geometry can extend past
    // the rect, and an unclipped painter would draw it over the editor.
    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
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
            &painter,
            rect.center(),
            SpiralSegment {
                from,
                to,
                margin,
                rotate,
                stretch,
                thickness,
                color: color_with_alpha(base, opacity * event_alpha(hap)),
                cap: options.spiral_cap,
            },
        );
    }

    paint_spiral_segment(
        &painter,
        rect.center(),
        SpiralSegment {
            from: inset - options.playhead_length,
            to: inset,
            margin,
            rotate,
            stretch,
            thickness: options.playhead_thickness.unwrap_or(thickness),
            color: options.playhead_color.unwrap_or(colors.active),
            cap: options.spiral_cap,
        },
    );
}

#[derive(Clone, Copy)]
pub(super) struct SpiralSegment {
    from: f32,
    to: f32,
    margin: f32,
    rotate: f32,
    stretch: f32,
    thickness: f32,
    color: egui::Color32,
    cap: SpiralCap,
}

fn paint_spiral_segment(painter: &egui::Painter, center: egui::Pos2, segment: SpiralSegment) {
    let mut points: Vec<egui::Pos2> = Vec::new();
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
        let last = points.len() - 1;
        painter.add(egui::Shape::line(
            points.clone(),
            egui::Stroke::new(segment.thickness, segment.color),
        ));
        for (end, inward) in [(points[0], points[1]), (points[last], points[last - 1])] {
            if let Some(shape) = cap_shape(end, end - inward, segment.thickness, segment) {
                painter.add(shape);
            }
        }
    }
}

/// Points used to trace a round cap's half-circle. Eight is smooth enough at
/// the stroke widths a spiral uses and keeps the polygon cheap.
const ROUND_CAP_STEPS: usize = 8;

/// The cap drawn at one end of a segment, or `None` for `butt`.
///
/// egui polylines carry no line-cap setting, so the cap is a separate shape —
/// which means it must not overlap the stroke. Two translucent shapes drawn on
/// top of each other composite twice and read brighter than the segment they
/// cap, and spiral haps are translucent whenever `fade` is on (the default).
/// So the geometry lives entirely *beyond* the line's butt end, and is convex,
/// which is what egui's tessellator fills correctly and with anti-aliasing.
pub(super) fn cap_shape(
    end: egui::Pos2,
    outward: egui::Vec2,
    thickness: f32,
    segment: SpiralSegment,
) -> Option<egui::Shape> {
    if segment.cap == SpiralCap::Butt || outward.length() <= f32::EPSILON {
        return None;
    }
    let radius = thickness / 2.0;
    let outward = outward.normalized();
    let across = outward.rot90() * radius;
    let points = match segment.cap {
        // A half-disc swept from one side of the stroke to the other.
        SpiralCap::Round => {
            let base = outward.angle();
            (0..=ROUND_CAP_STEPS)
                .map(|step| {
                    let turn = std::f32::consts::PI * step as f32 / ROUND_CAP_STEPS as f32;
                    let angle = base - std::f32::consts::FRAC_PI_2 + turn;
                    end + egui::vec2(angle.cos(), angle.sin()) * radius
                })
                .collect()
        }
        // A half-stroke-width extension: the stroke's own end, pushed out.
        SpiralCap::Square => vec![
            end + across,
            end + across + outward * radius,
            end - across + outward * radius,
            end - across,
        ],
        SpiralCap::Butt => return None,
    };
    Some(egui::Shape::convex_polygon(
        points,
        segment.color,
        egui::Stroke::NONE,
    ))
}

/// Line-end style for spiral segments, matching the canvas `lineCap` values
/// Strudel passes through (`butt` default, `round`, `square`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SpiralCap {
    #[default]
    Butt,
    Round,
    Square,
}

impl SpiralCap {
    #[cfg(test)]
    pub(super) fn test_segment(cap: SpiralCap, thickness: f32) -> SpiralSegment {
        SpiralSegment {
            from: 0.0,
            to: 1.0,
            margin: 1.0,
            rotate: 0.0,
            stretch: 1.0,
            thickness,
            color: egui::Color32::WHITE,
            cap,
        }
    }

    pub(super) fn from_name(name: &str) -> SpiralCap {
        match name {
            "round" => SpiralCap::Round,
            "square" => SpiralCap::Square,
            _ => SpiralCap::Butt,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cap_points(cap: SpiralCap, thickness: f32) -> Vec<egui::Pos2> {
        let segment = SpiralCap::test_segment(cap, thickness);
        let end = egui::pos2(10.0, 4.0);
        let outward = egui::vec2(1.0, 0.0);
        match cap_shape(end, outward, thickness, segment) {
            Some(egui::Shape::Path(path)) => path.points,
            other => panic!("expected a filled path, got {other:?}"),
        }
    }

    #[test]
    fn butt_draws_no_cap() {
        let segment = SpiralCap::test_segment(SpiralCap::Butt, 8.0);
        assert!(cap_shape(egui::pos2(1.0, 1.0), egui::vec2(1.0, 0.0), 8.0, segment).is_none());
        // A degenerate direction (a zero-length segment) is skipped too.
        let round = SpiralCap::test_segment(SpiralCap::Round, 8.0);
        assert!(cap_shape(egui::pos2(1.0, 1.0), egui::Vec2::ZERO, 8.0, round).is_none());
    }

    #[test]
    fn caps_stay_beyond_the_stroke_end() {
        // The whole point of the shape: it must sit on the outward side of the
        // line's butt end. Overlapping the stroke would composite twice and
        // make the cap read brighter than the segment it caps.
        let thickness = 8.0;
        let end = egui::pos2(10.0, 4.0);
        for cap in [SpiralCap::Round, SpiralCap::Square] {
            for point in cap_points(cap, thickness) {
                let along = (point - end).x; // outward is +x here
                assert!(
                    along >= -1e-3,
                    "{cap:?} point {point:?} falls behind the stroke end"
                );
                // and never reaches further out than a half stroke width
                assert!(along <= thickness / 2.0 + 1e-3, "{cap:?} overshoots");
            }
        }
    }

    #[test]
    fn caps_span_the_stroke_width() {
        // Both caps must meet the stroke edge-to-edge, or a seam shows.
        let thickness = 8.0;
        for cap in [SpiralCap::Round, SpiralCap::Square] {
            let points = cap_points(cap, thickness);
            let (min, max) = points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                (lo.min(p.y), hi.max(p.y))
            });
            assert!(
                (max - min - thickness).abs() < 1e-3,
                "{cap:?} spans {} across a {thickness} stroke",
                max - min
            );
        }
        // The round cap bulges out to a half width at its apex; square is flat.
        let apex = cap_points(SpiralCap::Round, 8.0)
            .iter()
            .fold(f32::MIN, |acc, p| acc.max(p.x - 10.0));
        assert!((apex - 4.0).abs() < 1e-3, "round apex at {apex}");
    }
}
