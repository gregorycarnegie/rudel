use super::decorations::{SliderDecoration, SourceRange, TextChange};
use super::settings::DrawTheme;
use eframe::egui;

const SLIDER_WIDTH: f32 = 64.0;
const SLIDER_HEIGHT: f32 = 18.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SliderHostUpdate {
    pub(crate) id: String,
    pub(crate) insert: String,
    pub(crate) value: f64,
}

pub(crate) fn draw_slider_hosts(
    ui: &mut egui::Ui,
    code: &mut String,
    editor_rect: egui::Rect,
    sliders: &[SliderDecoration],
    draw_theme: DrawTheme,
) -> Option<SliderHostUpdate> {
    let clip = ui.clip_rect();
    for slider in sliders {
        let Some(mut value) = slider_value_from_source(code, slider) else {
            continue;
        };
        let (min, max) = slider_bounds(slider);
        if min >= max {
            continue;
        }
        value = value.clamp(min, max);
        let rect = slider_rect(ui, code, slider.range, editor_rect);
        if !clip.intersects(rect) {
            continue;
        }

        let step = slider_step(slider, min, max);
        let area = egui::Area::new(egui::Id::new(("rudel-inline-slider", slider.id.as_str())))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ui.ctx(), |ui| {
                // Clip to the editor's visible area so the foreground slider never
                // paints over the surrounding panels.
                ui.set_clip_rect(clip);
                ui.set_min_size(rect.size());
                ui.spacing_mut().slider_width = SLIDER_WIDTH;
                ui.visuals_mut().widgets.inactive.bg_fill = draw_theme.line_background;
                ui.visuals_mut().widgets.hovered.bg_fill = draw_theme.line_highlight;
                ui.visuals_mut().widgets.active.bg_fill = draw_theme.selection;
                ui.visuals_mut().widgets.inactive.fg_stroke.color = draw_theme.foreground;
                ui.visuals_mut().widgets.hovered.fg_stroke.color = draw_theme.foreground;
                ui.visuals_mut().widgets.active.fg_stroke.color = draw_theme.foreground;
                ui.add_sized(
                    rect.size(),
                    egui::Slider::new(&mut value, min..=max)
                        .step_by(step)
                        .show_value(false),
                )
            });

        if area.inner.changed()
            && let Some(update) = apply_slider_drag_value(code, slider, value, step)
        {
            ui.ctx().request_repaint();
            return Some(update);
        }
    }
    None
}

fn apply_slider_drag_value(
    code: &mut String,
    slider: &SliderDecoration,
    value: f64,
    step: f64,
) -> Option<SliderHostUpdate> {
    let insert = format_slider_literal(value, step);
    replace_slider_literal(code, slider.range, &insert)?;
    rudel_lang::set_slider_value(&slider.id, value);
    Some(SliderHostUpdate {
        id: slider.id.clone(),
        insert,
        value,
    })
}

fn slider_rect(
    ui: &egui::Ui,
    code: &str,
    range: SourceRange,
    editor_rect: egui::Rect,
) -> egui::Rect {
    let (line, column) = line_column_at_byte(code, range.from);
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font_id));
    let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font_id, 'm'));
    let origin = editor_rect.min + egui::vec2(6.0, 4.0);
    let pos = egui::pos2(
        origin.x + column as f32 * char_width,
        origin.y + line as f32 * row_height + row_height - SLIDER_HEIGHT * 0.75,
    );
    egui::Rect::from_min_size(pos, egui::vec2(SLIDER_WIDTH, SLIDER_HEIGHT))
}

fn line_column_at_byte(code: &str, byte: usize) -> (usize, usize) {
    let byte = byte.min(code.len());
    let prefix = &code[..byte];
    let line = prefix.bytes().filter(|b| *b == b'\n').count();
    let line_start = prefix.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let column = prefix[line_start..].chars().count();
    (line, column)
}

fn slider_value_from_source(code: &str, slider: &SliderDecoration) -> Option<f64> {
    code.get(slider.range.from..slider.range.to)
        .and_then(|literal| literal.trim().parse().ok())
        .or_else(|| slider.value.as_deref().and_then(|value| value.parse().ok()))
}

fn slider_bounds(slider: &SliderDecoration) -> (f64, f64) {
    (slider.min.unwrap_or(0.0), slider.max.unwrap_or(1.0))
}

fn slider_step(slider: &SliderDecoration, min: f64, max: f64) -> f64 {
    slider
        .step
        .filter(|step| step.is_finite() && *step > 0.0)
        .unwrap_or_else(|| ((max - min) / 1000.0).max(f64::EPSILON))
}

fn replace_slider_literal(
    code: &mut String,
    range: SourceRange,
    insert: &str,
) -> Option<TextChange> {
    code.get(range.from..range.to)?;
    code.replace_range(range.from..range.to, insert);
    Some(TextChange {
        from: range.from,
        to: range.to,
        insert_len: insert.len(),
    })
}

fn format_slider_literal(value: f64, step: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let precision = decimal_places(step).clamp(3, 12);
    let mut out = format!("{value:.precision$}");
    if out.contains('.') {
        while out.ends_with('0') {
            out.pop();
        }
        if out.ends_with('.') {
            out.pop();
        }
    }
    if out == "-0" {
        out = "0".to_string();
    }
    out
}

fn decimal_places(value: f64) -> usize {
    let mut value = value.abs();
    let mut places = 0;
    while places < 12 && (value.fract()).abs() > 1e-9 {
        value *= 10.0;
        places += 1;
    }
    places
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slider(range: SourceRange) -> SliderDecoration {
        SliderDecoration {
            id: "7:10".to_string(),
            range,
            index: 0,
            value: Some("0.5".to_string()),
            min: Some(0.0),
            max: Some(1.0),
            step: Some(0.01),
        }
    }

    #[test]
    fn slider_value_prefers_current_source_literal() {
        let slider = slider(SourceRange::new(7, 10));

        assert_eq!(slider_value_from_source("slider(0.7)", &slider), Some(0.7));
    }

    #[test]
    fn replacing_slider_literal_reports_the_text_change() {
        let mut code = "slider(0.5, 0, 1)".to_string();
        let change = replace_slider_literal(&mut code, SourceRange::new(7, 10), "0.75");

        assert_eq!(code, "slider(0.75, 0, 1)");
        assert_eq!(
            change,
            Some(TextChange {
                from: 7,
                to: 10,
                insert_len: 4
            })
        );
    }

    #[test]
    fn slider_literal_formatting_matches_range_input_style() {
        assert_eq!(format_slider_literal(0.7, 0.01), "0.7");
        assert_eq!(format_slider_literal(0.125, 0.001), "0.125");
        assert_eq!(format_slider_literal(-0.0, 0.01), "0");
    }

    #[test]
    fn byte_position_maps_to_monospace_line_and_column() {
        assert_eq!(line_column_at_byte("ab\ncd", 0), (0, 0));
        assert_eq!(line_column_at_byte("ab\ncd", 4), (1, 1));
        assert_eq!(line_column_at_byte("åb\nc", "åb\n".len()), (1, 0));
    }

    #[test]
    fn dragging_slider_updates_source_and_live_registry() {
        let result = rudel_lang::eval_result("slider(0.5, 0, 1)").expect("eval");
        let widget = &result.meta.widgets[0];
        let slider = SliderDecoration {
            id: widget.id.clone(),
            range: SourceRange::new(widget.from, widget.to),
            index: widget.index,
            value: widget.value.clone(),
            min: widget.min,
            max: widget.max,
            step: widget.step,
        };
        let mut code = "slider(0.5, 0, 1)".to_string();

        let update = apply_slider_drag_value(&mut code, &slider, 0.75, 0.01).unwrap();

        assert_eq!(code, "slider(0.75, 0, 1)");
        assert_eq!(update.insert, "0.75");
        assert_eq!(
            rudel_lang::slider_value(&slider.id).and_then(|value| value.as_f64()),
            Some(0.75)
        );
    }
}
