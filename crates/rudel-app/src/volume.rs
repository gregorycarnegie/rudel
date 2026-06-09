use eframe::egui;

pub(crate) const DEFAULT_VOLUME_PERCENT: f32 = 100.0;
pub(crate) const MAX_VOLUME_PERCENT: f32 = 200.0;

pub(crate) fn vlc_volume_slider(ui: &mut egui::Ui, volume_percent: &mut f32) -> egui::Response {
    let desired = egui::vec2(136.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(desired, egui::Sense::click_and_drag());
    let track = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 54.0, rect.top() + 5.0),
        egui::pos2(rect.right() - 4.0, rect.bottom() - 3.0),
    );

    if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        *volume_percent = volume_percent_from_track_x(pos.x, track);
        response.mark_changed();
    }

    let painter = ui.painter_at(rect);
    draw_speaker_icon(
        &painter,
        egui::Rect::from_min_size(rect.min, egui::vec2(18.0, rect.height())),
        ui.visuals().text_color(),
        egui::Color32::from_rgb(230, 145, 45),
    );
    painter.text(
        egui::pos2(rect.left() + 19.0, rect.top() + 1.0),
        egui::Align2::LEFT_TOP,
        format!("{:.0}%", volume_percent.clamp(0.0, MAX_VOLUME_PERCENT)),
        egui::FontId::proportional(11.0),
        ui.visuals().text_color(),
    );
    draw_volume_wedge(
        &painter,
        track,
        (*volume_percent / MAX_VOLUME_PERCENT).clamp(0.0, 1.0),
    );

    response
}

fn volume_percent_from_track_x(x: f32, track: egui::Rect) -> f32 {
    let t = ((x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
    t * MAX_VOLUME_PERCENT
}

fn draw_speaker_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    accent: egui::Color32,
) {
    let cy = rect.center().y + 2.0;
    let left = rect.left() + 2.0;
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(left, cy - 3.0), egui::pos2(left + 4.0, cy + 3.0)),
        1.0,
        color,
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(left + 4.0, cy - 4.5),
            egui::pos2(left + 10.0, cy - 8.0),
            egui::pos2(left + 10.0, cy + 8.0),
            egui::pos2(left + 4.0, cy + 4.5),
        ],
        color,
        egui::Stroke::NONE,
    ));

    let center = egui::pos2(left + 8.0, cy);
    for radius in [5.0, 8.0] {
        painter.add(egui::Shape::line(
            arc_points(center, radius, -0.75, 0.75),
            egui::Stroke::new(1.2, accent),
        ));
    }
}

fn arc_points(center: egui::Pos2, radius: f32, start: f32, end: f32) -> Vec<egui::Pos2> {
    (0..=10)
        .map(|i| {
            let t = i as f32 / 10.0;
            let a = start + (end - start) * t;
            egui::pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

fn draw_volume_wedge(painter: &egui::Painter, rect: egui::Rect, amount: f32) {
    let amount = amount.clamp(0.0, 1.0);
    let bottom = rect.bottom();
    let top_at = |x: f32| {
        let t = ((x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
        bottom - 2.0 - t * (rect.height() - 2.0)
    };
    let outline = vec![
        egui::pos2(rect.left(), bottom),
        egui::pos2(rect.right(), bottom),
        egui::pos2(rect.right(), top_at(rect.right())),
        egui::pos2(rect.left(), top_at(rect.left())),
    ];

    painter.add(egui::Shape::convex_polygon(
        outline.clone(),
        egui::Color32::from_gray(42),
        egui::Stroke::NONE,
    ));

    let segments = 48;
    for i in 0..segments {
        let t0 = i as f32 / segments as f32;
        if t0 >= amount {
            break;
        }
        let t1 = ((i + 1) as f32 / segments as f32).min(amount);
        let x0 = rect.left() + rect.width() * t0;
        let x1 = rect.left() + rect.width() * t1;
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x0, bottom),
                egui::pos2(x1, bottom),
                egui::pos2(x1, top_at(x1)),
                egui::pos2(x0, top_at(x0)),
            ],
            volume_ramp_color(t1),
            egui::Stroke::NONE,
        ));
    }

    let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(145));
    for line in outline.windows(2) {
        painter.line_segment([line[0], line[1]], stroke);
    }
    painter.line_segment([outline[3], outline[0]], stroke);
}

fn volume_ramp_color(t: f32) -> egui::Color32 {
    let green = egui::Color32::from_rgb(58, 205, 65);
    let yellow = egui::Color32::from_rgb(248, 218, 44);
    let red = egui::Color32::from_rgb(244, 80, 42);
    if t < 0.5 {
        lerp_color(green, yellow, t * 2.0)
    } else {
        lerp_color(yellow, red, (t - 0.5) * 2.0)
    }
}

fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let blend = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    egui::Color32::from_rgb(
        blend(a.r(), b.r()),
        blend(a.g(), b.g()),
        blend(a.b(), b.b()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_track_maps_x_to_percent() {
        let track = egui::Rect::from_min_max(egui::pos2(10.0, 0.0), egui::pos2(110.0, 10.0));
        assert_eq!(volume_percent_from_track_x(10.0, track), 0.0);
        assert_eq!(volume_percent_from_track_x(60.0, track), 100.0);
        assert_eq!(volume_percent_from_track_x(110.0, track), 200.0);
        assert_eq!(volume_percent_from_track_x(140.0, track), 200.0);
    }
}
