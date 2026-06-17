use super::decorations::WidgetDecoration;
use super::settings::DrawTheme;
use eframe::egui;
use rudel_core::{Frac, Hap, Pattern, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

const DRAW_LOOKBEHIND: f64 = -2.0;
const DRAW_LOOKAHEAD: f64 = 2.0;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WidgetHostSync {
    pub(crate) created: Vec<String>,
    pub(crate) removed: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct WidgetSurface {
    serial: u64,
    size: egui::Vec2,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WidgetHostState {
    surfaces: HashMap<WidgetKey, WidgetSurface>,
    next_serial: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct WidgetPaintInput<'a> {
    pub(crate) pattern: Option<&'a Pattern>,
    pub(crate) time_cycles: Option<f64>,
    pub(crate) draw_theme: DrawTheme,
}

impl WidgetHostState {
    pub(crate) fn sync(&mut self, widgets: &[WidgetDecoration]) -> WidgetHostSync {
        let mut active = HashSet::new();
        let mut created = Vec::new();
        for widget in widgets {
            let key = WidgetKey::from(widget);
            let size = surface_size(widget);
            active.insert(key.clone());
            if let Some(surface) = self.surfaces.get_mut(&key) {
                surface.size = size;
            } else {
                let serial = self.next_serial;
                self.next_serial += 1;
                self.surfaces
                    .insert(key.clone(), WidgetSurface { serial, size });
                created.push(widget.id.clone());
            }
        }

        let mut removed = Vec::new();
        self.surfaces.retain(|key, _| {
            let keep = active.contains(key);
            if !keep {
                removed.push(key.id.clone());
            }
            keep
        });
        removed.sort();
        removed.dedup();
        WidgetHostSync { created, removed }
    }

    fn surface(&self, widget: &WidgetDecoration) -> Option<&WidgetSurface> {
        self.surfaces.get(&WidgetKey::from(widget))
    }

    #[cfg(test)]
    fn surface_serial(&self, widget_type: &str, id: &str) -> Option<u64> {
        self.surfaces
            .get(&WidgetKey {
                widget_type: widget_type.to_string(),
                id: id.to_string(),
            })
            .map(|surface| surface.serial)
    }

    #[cfg(test)]
    fn surface_count(&self) -> usize {
        self.surfaces.len()
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct WidgetKey {
    widget_type: String,
    id: String,
}

impl From<&WidgetDecoration> for WidgetKey {
    fn from(widget: &WidgetDecoration) -> Self {
        Self {
            widget_type: widget.widget_type.clone(),
            id: widget.id.clone(),
        }
    }
}

pub(crate) fn draw_widget_hosts(
    ui: &mut egui::Ui,
    code: &str,
    editor_rect: egui::Rect,
    widgets: &[WidgetDecoration],
    host: &mut WidgetHostState,
    paint: WidgetPaintInput<'_>,
) {
    let sync = host.sync(widgets);
    if !sync.created.is_empty() || !sync.removed.is_empty() {
        ui.ctx().request_repaint();
    }

    let clip = ui.clip_rect();
    for widget in widgets {
        let Some(surface) = host.surface(widget) else {
            continue;
        };
        let rect = widget_rect(ui, code, widget, surface.size, editor_rect);
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

fn widget_rect(
    ui: &egui::Ui,
    code: &str,
    widget: &WidgetDecoration,
    size: egui::Vec2,
    editor_rect: egui::Rect,
) -> egui::Rect {
    let (line, column) = line_column_at_byte(code, widget.placement());
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font_id));
    let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font_id, 'm'));
    let origin = editor_rect.min + egui::vec2(6.0, 4.0);
    let pos = egui::pos2(
        origin.x + column as f32 * char_width,
        origin.y + (line as f32 + 1.15) * row_height,
    );
    let max_width = (editor_rect.right() - pos.x).max(160.0);
    egui::Rect::from_min_size(pos, egui::vec2(size.x.min(max_width), size.y))
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
        .map(|pattern| paint_pattern_widget(ui, rect, widget, pattern, paint.time_cycles, colors))
        .unwrap_or(false);

    if !painted {
        let left = egui::Rect::from_min_size(rect.min, egui::vec2(4.0, rect.height()));
        painter.rect_filled(left, 4.0, colors.active);
        paint_widget_label(ui, rect, widget, surface, colors);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WidgetDrawColors {
    background: egui::Color32,
    active: egui::Color32,
    inactive: egui::Color32,
    text: egui::Color32,
    muted: egui::Color32,
}

fn widget_draw_colors(draw_theme: DrawTheme) -> WidgetDrawColors {
    WidgetDrawColors {
        background: draw_theme.line_background,
        active: draw_theme.foreground,
        inactive: draw_theme.gutter_foreground,
        text: draw_theme.foreground,
        muted: draw_theme.gutter_foreground,
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

fn paint_pattern_widget(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget: &WidgetDecoration,
    pattern: &Pattern,
    time_cycles: Option<f64>,
    colors: WidgetDrawColors,
) -> bool {
    let time = time_cycles.unwrap_or(0.0);
    let options = VisualWidgetOptions::from_widget(widget);
    match widget.widget_type.as_str() {
        "_pianoroll" | "_punchcard" => {
            let haps = widget_haps(pattern, widget, options.window(time));
            paint_pianoroll(ui, rect, &haps, time, colors, options);
            true
        }
        "_wordfall" => {
            let haps = widget_haps(pattern, widget, options.window(time));
            paint_pianoroll(
                ui,
                rect,
                &haps,
                time,
                colors,
                options.with_wordfall_defaults(widget),
            );
            true
        }
        "_pitchwheel" => {
            let haps = widget_haps(pattern, widget, DrawWindow::around(time))
                .into_iter()
                .filter(|hap| hap_is_active(hap, time))
                .collect::<Vec<_>>();
            paint_pitchwheel(ui, rect, &haps, colors, options);
            true
        }
        "_spiral" => {
            let haps = widget_haps(pattern, widget, DrawWindow::around(time));
            paint_spiral(ui, rect, &haps, time, colors, options);
            true
        }
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DrawWindow {
    begin: f64,
    end: f64,
}

impl DrawWindow {
    fn around(time: f64) -> Self {
        Self {
            begin: time + DRAW_LOOKBEHIND,
            end: time + DRAW_LOOKAHEAD,
        }
    }
}

fn widget_haps(pattern: &Pattern, widget: &WidgetDecoration, window: DrawWindow) -> Vec<Hap> {
    let begin = Frac::from_f64(window.begin);
    let end = Frac::from_f64(window.end);
    let mut haps: Vec<Hap> = pattern
        .query_arc(begin, end)
        .into_iter()
        .filter(|hap| hap.whole.is_some())
        .filter(|hap| hap_matches_widget(hap, widget))
        .collect();
    haps.sort_by_key(|hap| hap.whole_or_part().begin);
    haps
}

fn hap_matches_widget(hap: &Hap, widget: &WidgetDecoration) -> bool {
    if hap.has_tag(&widget.id) {
        return true;
    }
    if !hap.context.tags.is_empty() {
        return false;
    }
    hap.context
        .locations
        .iter()
        .any(|&location| ranges_overlap(location, (widget.range.from, widget.range.to)))
}

fn ranges_overlap(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

fn hap_is_active(hap: &Hap, time: f64) -> bool {
    let t = Frac::from_f64(time);
    hap.whole
        .is_some_and(|whole| whole.begin <= t && hap.end_clipped() > t)
}

#[derive(Clone, Debug, PartialEq)]
enum RollValue {
    Number(f64),
    Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct VisualWidgetOptions {
    cycles: f64,
    playhead: f64,
    vertical: bool,
    labels: bool,
    flip_time: bool,
    flip_values: bool,
    fold: bool,
    hide_inactive: bool,
    hide_negative: bool,
    fill: bool,
    fill_active: bool,
    stroke: Option<bool>,
    stroke_active: bool,
    colorize_inactive: bool,
    min_midi: f64,
    max_midi: f64,
    autorange: bool,
    circle: bool,
    hapcircles: bool,
    edo: i64,
    thickness: f32,
    hap_radius: f32,
    margin: f32,
    polygon: bool,
    stretch: f32,
    spiral_size: f32,
    spiral_thickness: Option<f32>,
    inset: f32,
    playhead_length: f32,
    playhead_thickness: Option<f32>,
    padding: f32,
    steady: f32,
    colorize_spiral_inactive: bool,
    fade: bool,
    active_color: Option<egui::Color32>,
    inactive_color: Option<egui::Color32>,
    playhead_color: Option<egui::Color32>,
}

impl VisualWidgetOptions {
    fn from_widget(widget: &WidgetDecoration) -> Self {
        let options = &widget.options;
        let spiral_size = if widget.widget_type == "_spiral" {
            option_f32(options, "size")
                .map(|size| size / 5.0)
                .unwrap_or(55.0)
        } else {
            option_f32(options, "size").unwrap_or(80.0)
        }
        .max(0.001);
        Self {
            cycles: option_f64(options, "cycles").unwrap_or(4.0).max(0.001),
            playhead: option_f64(options, "playhead")
                .unwrap_or(0.5)
                .clamp(0.0, 1.0),
            vertical: option_bool(options, "vertical").unwrap_or(false),
            labels: option_bool(options, "labels").unwrap_or(false),
            flip_time: option_bool(options, "flipTime").unwrap_or(false),
            flip_values: option_bool(options, "flipValues").unwrap_or(false),
            fold: option_bool(options, "fold").unwrap_or(true),
            hide_inactive: option_bool(options, "hideInactive").unwrap_or(false),
            hide_negative: option_bool(options, "hideNegative").unwrap_or(false),
            fill: option_bool(options, "fill").unwrap_or(true),
            fill_active: option_bool(options, "fillActive").unwrap_or(false),
            stroke: option_bool(options, "stroke"),
            stroke_active: option_bool(options, "strokeActive").unwrap_or(true),
            colorize_inactive: option_bool(options, "colorizeInactive").unwrap_or(true),
            min_midi: option_f64(options, "minMidi").unwrap_or(10.0),
            max_midi: option_f64(options, "maxMidi").unwrap_or(90.0),
            autorange: option_bool(options, "autorange").unwrap_or(false),
            circle: option_bool(options, "circle").unwrap_or(false),
            hapcircles: option_bool(options, "hapcircles").unwrap_or(true),
            edo: option_f64(options, "edo").unwrap_or(12.0).round().max(0.0) as i64,
            thickness: option_f32(options, "thickness").unwrap_or(3.0),
            hap_radius: option_f32(options, "hapRadius").unwrap_or(6.0),
            margin: option_f32(options, "margin").unwrap_or(10.0),
            polygon: option_str(options, "mode") == Some("polygon"),
            stretch: option_f32(options, "stretch").unwrap_or(1.0).max(0.001),
            spiral_size,
            spiral_thickness: option_f32(options, "thickness"),
            inset: option_f32(options, "inset").unwrap_or(3.0),
            playhead_length: option_f32(options, "playheadLength").unwrap_or(0.02),
            playhead_thickness: option_f32(options, "playheadThickness"),
            padding: option_f32(options, "padding").unwrap_or(0.0),
            steady: option_f32(options, "steady").unwrap_or(1.0),
            colorize_spiral_inactive: option_bool(options, "colorizeInactive").unwrap_or(false),
            fade: option_bool(options, "fade").unwrap_or(true),
            active_color: option_color(options, "active")
                .or_else(|| option_color(options, "activeColor")),
            inactive_color: option_color(options, "inactive")
                .or_else(|| option_color(options, "inactiveColor")),
            playhead_color: option_color(options, "playheadColor"),
        }
    }

    fn with_wordfall_defaults(mut self, widget: &WidgetDecoration) -> Self {
        if !widget.options.contains_key("vertical") {
            self.vertical = true;
        }
        if !widget.options.contains_key("labels") {
            self.labels = true;
        }
        if !widget.options.contains_key("stroke") {
            self.stroke = Some(false);
        }
        if !widget.options.contains_key("fillActive") {
            self.fill_active = true;
        }
        self
    }

    fn window(self, time: f64) -> DrawWindow {
        let from = -self.cycles * self.playhead;
        let to = self.cycles * (1.0 - self.playhead);
        DrawWindow {
            begin: time + from,
            end: time + to,
        }
    }
}

fn paint_pianoroll(
    ui: &egui::Ui,
    rect: egui::Rect,
    haps: &[Hap],
    time: f64,
    colors: WidgetDrawColors,
    options: VisualWidgetOptions,
) {
    let painter = ui.painter();
    let mut haps = haps
        .iter()
        .filter(|hap| {
            !options.hide_negative || hap.whole.is_some_and(|whole| whole.begin >= Frac::zero())
        })
        .collect::<Vec<_>>();
    if options.hide_inactive {
        haps.retain(|hap| hap_is_active(hap, time));
    }

    let mut values: Vec<RollValue> = haps.iter().map(|hap| pianoroll_value(hap)).collect();
    values.sort_by(roll_value_cmp);
    values.dedup_by(|a, b| a == b);

    let (min_midi, max_midi) = if options.autorange {
        autorange_midi(&values).unwrap_or((options.min_midi, options.max_midi))
    } else {
        (options.min_midi, options.max_midi)
    };
    let numeric_slots = ((max_midi - min_midi + 1.0).max(1.0)) as usize;
    let slots = if options.fold {
        values.len().max(1)
    } else {
        numeric_slots
    };
    let time_extent = options.cycles;
    let window_start = time - options.cycles * options.playhead;
    let playhead = if options.flip_time {
        (1.0 - options.playhead) as f32
    } else {
        options.playhead as f32
    };

    for hap in haps {
        let Some(whole) = hap.whole else {
            continue;
        };
        let value = pianoroll_value(hap);
        let Some(value_index) = roll_value_index(&values, &value, options.fold, min_midi, max_midi)
        else {
            continue;
        };
        let begin = whole.begin.to_f64();
        let end = hap.end_clipped().to_f64();
        let active = hap_is_active(hap, time);
        let fallback = if active {
            options.active_color.unwrap_or(colors.active)
        } else if options.colorize_inactive {
            options.inactive_color.unwrap_or(colors.inactive)
        } else {
            colors.inactive
        };
        let color = event_color(hap, fallback);
        let color = color_with_alpha(color, event_alpha(hap));

        let input = RollRectInput {
            value_index,
            slots,
            begin,
            end,
            window_start,
            time_extent,
            options,
        };
        let block = if options.vertical {
            vertical_roll_rect(rect, input)
        } else {
            horizontal_roll_rect(rect, input)
        };
        let block = block.intersect(rect);
        if block.width() <= 0.5 || block.height() <= 0.5 {
            continue;
        }
        let fill = (!active && options.fill) || (active && options.fill_active);
        if fill {
            painter.rect_filled(block, 1.5, color);
        }
        let stroke = options.stroke.unwrap_or(options.stroke_active && active);
        if stroke {
            painter.rect_stroke(
                block,
                1.5,
                egui::Stroke::new(1.0, color),
                egui::StrokeKind::Inside,
            );
        }

        if options.labels && block.width() > 16.0 && block.height() > 10.0 {
            painter.text(
                block.left_center() + egui::vec2(3.0, 0.0),
                egui::Align2::LEFT_CENTER,
                roll_label(hap),
                egui::FontId::monospace((block.height() * 0.55).clamp(9.0, 18.0)),
                colors.text,
            );
        }
    }

    let playhead_color = options.playhead_color.unwrap_or(colors.active);
    if options.vertical {
        let y = rect.bottom() - playhead * rect.height();
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.5, playhead_color),
        );
    } else {
        let x = rect.left() + playhead * rect.width();
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(1.5, playhead_color),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RollRectInput {
    value_index: usize,
    slots: usize,
    begin: f64,
    end: f64,
    window_start: f64,
    time_extent: f64,
    options: VisualWidgetOptions,
}

fn horizontal_roll_rect(rect: egui::Rect, input: RollRectInput) -> egui::Rect {
    let lane_h = (rect.height() / input.slots as f32).max(2.0);
    let t0 = time_progress(
        input.begin,
        input.window_start,
        input.time_extent,
        input.options.flip_time,
    );
    let t1 = time_progress(
        input.end,
        input.window_start,
        input.time_extent,
        input.options.flip_time,
    );
    let (t0, t1) = sorted_pair(t0, t1);
    let x0 = rect.left() + t0 * rect.width();
    let x1 = rect.left() + t1 * rect.width();
    let value_index = if input.options.flip_values {
        input
            .slots
            .saturating_sub(1)
            .saturating_sub(input.value_index)
    } else {
        input.value_index
    };
    let y0 = rect.bottom() - (value_index as f32 + 1.0) * lane_h;
    egui::Rect::from_min_max(
        egui::pos2(x0 + 1.0, y0 + 1.0),
        egui::pos2((x1 - 1.0).max(x0 + 2.0), y0 + lane_h - 1.0),
    )
}

fn vertical_roll_rect(rect: egui::Rect, input: RollRectInput) -> egui::Rect {
    let lane_w = (rect.width() / input.slots as f32).max(2.0);
    let t0 = time_progress(
        input.begin,
        input.window_start,
        input.time_extent,
        input.options.flip_time,
    );
    let t1 = time_progress(
        input.end,
        input.window_start,
        input.time_extent,
        input.options.flip_time,
    );
    let (t0, t1) = sorted_pair(t0, t1);
    let y0 = rect.bottom() - t0 * rect.height();
    let y1 = rect.bottom() - t1 * rect.height();
    let value_index = if input.options.flip_values {
        input
            .slots
            .saturating_sub(1)
            .saturating_sub(input.value_index)
    } else {
        input.value_index
    };
    let x0 = rect.left() + value_index as f32 * lane_w;
    egui::Rect::from_min_max(
        egui::pos2(x0 + 1.0, y1 + 1.0),
        egui::pos2(x0 + lane_w - 1.0, (y0 - 1.0).max(y1 + 2.0)),
    )
}

fn time_progress(value: f64, window_start: f64, time_extent: f64, flip: bool) -> f32 {
    let progress = ((value - window_start) / time_extent) as f32;
    if flip { 1.0 - progress } else { progress }
}

fn sorted_pair(a: f32, b: f32) -> (f32, f32) {
    if a <= b { (a, b) } else { (b, a) }
}

fn autorange_midi(values: &[RollValue]) -> Option<(f64, f64)> {
    let mut nums = values.iter().filter_map(|value| match value {
        RollValue::Number(value) => Some(*value),
        RollValue::Text(_) => None,
    });
    let first = nums.next()?;
    let (min, max) = nums.fold((first, first), |(min, max), value| {
        (min.min(value), max.max(value))
    });
    Some((min, max))
}

fn roll_value_index(
    values: &[RollValue],
    value: &RollValue,
    fold: bool,
    min_midi: f64,
    max_midi: f64,
) -> Option<usize> {
    if fold {
        return values.iter().position(|item| item == value);
    }
    match value {
        RollValue::Number(value) if *value >= min_midi && *value <= max_midi => {
            Some((*value - min_midi).floor() as usize)
        }
        RollValue::Text(_) => values.iter().position(|item| item == value),
        _ => None,
    }
}

fn pianoroll_value(hap: &Hap) -> RollValue {
    let mut controls = rudel_core::to_control_map(&hap.value);
    rudel_core::tonal::apply_transpose_controls(&mut controls, hap.context.scale.as_deref());
    if let Some(freq) = controls.get("freq").and_then(Value::as_f64) {
        return RollValue::Number(rudel_core::freq_to_midi(freq));
    }
    for key in ["note", "n", "value"] {
        if let Some(value) = controls.get(key)
            && let Some(midi) = value_to_midi(value)
        {
            return RollValue::Number(midi);
        }
    }
    if let Some(sound) = controls.get("s").and_then(Value::as_str) {
        return RollValue::Text(format!("_{sound}"));
    }
    match &hap.value {
        Value::Str(s) => RollValue::Text(s.clone()),
        other => other
            .as_f64()
            .map(RollValue::Number)
            .unwrap_or_else(|| RollValue::Text(format!("{other:?}"))),
    }
}

fn roll_value_cmp(a: &RollValue, b: &RollValue) -> std::cmp::Ordering {
    match (a, b) {
        (RollValue::Text(a), RollValue::Text(b)) => a.cmp(b),
        (RollValue::Text(_), RollValue::Number(_)) => std::cmp::Ordering::Less,
        (RollValue::Number(_), RollValue::Text(_)) => std::cmp::Ordering::Greater,
        (RollValue::Number(a), RollValue::Number(b)) => {
            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
        }
    }
}

fn roll_label(hap: &Hap) -> String {
    let controls = rudel_core::to_control_map(&hap.value);
    for key in ["label", "activeLabel", "note", "s", "n"] {
        if let Some(value) = controls.get(key) {
            return value_short(value);
        }
    }
    value_short(&hap.value)
}

fn value_to_midi(value: &Value) -> Option<f64> {
    match value {
        Value::Str(s) => s
            .parse::<f64>()
            .ok()
            .or_else(|| rudel_core::note_to_midi(s).map(|m| m as f64)),
        other => other.as_f64(),
    }
}

fn paint_pitchwheel(
    ui: &egui::Ui,
    rect: egui::Rect,
    haps: &[Hap],
    colors: WidgetDrawColors,
    options: VisualWidgetOptions,
) {
    let painter = ui.painter();
    let size = rect.width().min(rect.height());
    let center = rect.center();
    let thickness = options.thickness;
    let hap_radius = options.hap_radius;
    let margin = options.margin;
    let radius = (size / 2.0 - thickness / 2.0 - hap_radius - margin).max(4.0);
    let root = rudel_core::midi_to_freq(36.0);
    let edo = options.edo;

    if options.circle {
        painter.circle_stroke(center, radius, egui::Stroke::new(thickness, colors.active));
    }

    if edo > 0 {
        for i in 0..edo {
            let freq = root * 2f64.powf(i as f64 / edo as f64);
            let pos = pitchwheel_pos(center, radius, freq_to_angle(freq, root));
            painter.circle_filled(
                pos,
                hap_radius * 0.45,
                color_with_alpha(colors.inactive, 0.45),
            );
        }
    }

    if edo > 0 {
        painter.text(
            rect.right_bottom() - egui::vec2(8.0, 8.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{edo} EDO"),
            egui::FontId::monospace(11.0),
            colors.muted,
        );
    }

    let mut shape = Vec::new();
    for hap in haps {
        let Some(freq) = hap_frequency(hap) else {
            continue;
        };
        let angle = freq_to_angle(freq, root);
        let pos = pitchwheel_pos(center, radius, angle);
        let color = color_with_alpha(event_color(hap, colors.active), event_alpha(hap));
        shape.push((pos, angle, color));
        if !options.polygon {
            painter.line_segment([center, pos], egui::Stroke::new(1.0, color));
        }
        if options.hapcircles {
            painter.circle_filled(pos, hap_radius, color);
        }
    }

    if options.polygon && shape.len() > 1 {
        shape.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let points = shape.iter().map(|(pos, _, _)| *pos).collect::<Vec<_>>();
        painter.add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(hap_radius, colors.active),
        ));
    }
}

fn hap_frequency(hap: &Hap) -> Option<f64> {
    let mut controls = rudel_core::to_control_map(&hap.value);
    rudel_core::tonal::apply_transpose_controls(&mut controls, hap.context.scale.as_deref());
    if let Some(freq) = controls.get("freq").and_then(Value::as_f64) {
        return Some(freq);
    }
    controls
        .get("note")
        .or_else(|| controls.get("n"))
        .and_then(value_to_midi)
        .map(rudel_core::midi_to_freq)
}

fn freq_to_angle(freq: f64, root: f64) -> f32 {
    let octaves = (freq / root).log2();
    let js_remainder = octaves - octaves.trunc();
    (0.5 - js_remainder) as f32
}

fn pitchwheel_pos(center: egui::Pos2, radius: f32, angle: f32) -> egui::Pos2 {
    let radians = angle * std::f32::consts::TAU;
    egui::pos2(
        center.x + radians.sin() * radius,
        center.y + radians.cos() * radius,
    )
}

fn paint_spiral(
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

fn spiral_point(
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

fn event_color(hap: &Hap, fallback: egui::Color32) -> egui::Color32 {
    let controls = rudel_core::to_control_map(&hap.value);
    controls
        .get("color")
        .and_then(Value::as_str)
        .and_then(parse_hex_color)
        .unwrap_or(fallback)
}

fn event_alpha(hap: &Hap) -> f32 {
    let controls = rudel_core::to_control_map(&hap.value);
    let velocity = controls
        .get("velocity")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let gain = controls.get("gain").and_then(Value::as_f64).unwrap_or(1.0);
    (velocity * gain).clamp(0.0, 1.0) as f32
}

fn parse_hex_color(color: &str) -> Option<egui::Color32> {
    let hex = color.strip_prefix('#')?;
    let parse = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
    match hex.len() {
        6 => Some(egui::Color32::from_rgb(
            parse(0..2)?,
            parse(2..4)?,
            parse(4..6)?,
        )),
        8 => Some(egui::Color32::from_rgba_unmultiplied(
            parse(0..2)?,
            parse(2..4)?,
            parse(4..6)?,
            parse(6..8)?,
        )),
        _ => None,
    }
}

fn color_with_alpha(color: egui::Color32, alpha: f32) -> egui::Color32 {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let alpha = (a as f32 * alpha.clamp(0.0, 1.0)).round() as u8;
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

fn value_short(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::F64(x) => format!("{x:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string(),
        other => format!("{other:?}"),
    }
}

fn default_surface_size(widget_type: &str) -> egui::Vec2 {
    match widget_type {
        "_spiral" => egui::vec2(275.0, 275.0),
        "_pitchwheel" | "_spectrum" => egui::vec2(200.0, 200.0),
        "_wordfall" => egui::vec2(500.0, 120.0),
        _ => egui::vec2(500.0, 60.0),
    }
}

fn surface_size(widget: &WidgetDecoration) -> egui::Vec2 {
    let default = default_surface_size(&widget.widget_type);
    let size = option_f32(&widget.options, "size");
    let width = option_f32(&widget.options, "width")
        .or(size)
        .unwrap_or(default.x)
        .max(20.0);
    let height = option_f32(&widget.options, "height")
        .or(size)
        .unwrap_or(default.y)
        .max(20.0);
    egui::vec2(width, height)
}

fn option_bool(options: &BTreeMap<String, rudel_lang::WidgetOption>, key: &str) -> Option<bool> {
    options.get(key).and_then(rudel_lang::WidgetOption::as_bool)
}

fn option_f64(options: &BTreeMap<String, rudel_lang::WidgetOption>, key: &str) -> Option<f64> {
    options.get(key).and_then(rudel_lang::WidgetOption::as_f64)
}

fn option_f32(options: &BTreeMap<String, rudel_lang::WidgetOption>, key: &str) -> Option<f32> {
    option_f64(options, key).map(|value| value as f32)
}

fn option_str<'a>(
    options: &'a BTreeMap<String, rudel_lang::WidgetOption>,
    key: &str,
) -> Option<&'a str> {
    options.get(key).and_then(rudel_lang::WidgetOption::as_str)
}

fn option_color(
    options: &BTreeMap<String, rudel_lang::WidgetOption>,
    key: &str,
) -> Option<egui::Color32> {
    option_str(options, key).and_then(parse_hex_color)
}

fn line_column_at_byte(code: &str, byte: usize) -> (usize, usize) {
    let byte = byte.min(code.len());
    let prefix = &code[..byte];
    let line = prefix.bytes().filter(|b| *b == b'\n').count();
    let line_start = prefix.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let column = prefix[line_start..].chars().count();
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::decorations::SourceRange;
    use crate::editor::settings::EditorTheme;
    use std::collections::BTreeMap;

    fn widget(widget_type: &str, id: &str, from: usize, to: usize) -> WidgetDecoration {
        WidgetDecoration {
            widget_type: widget_type.to_string(),
            id: id.to_string(),
            range: SourceRange::new(from, to),
            index: 0,
            options: BTreeMap::new(),
        }
    }

    fn widget_with_options(
        widget_type: &str,
        options: &[(&str, rudel_lang::WidgetOption)],
    ) -> WidgetDecoration {
        let mut widget = widget(widget_type, "options", 0, 1);
        widget.options = options
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect();
        widget
    }

    fn hap(value: Value) -> Hap {
        Hap::new(
            Some(rudel_core::TimeSpan::new(Frac::zero(), Frac::new(1, 2))),
            rudel_core::TimeSpan::new(Frac::zero(), Frac::new(1, 2)),
            value,
        )
    }

    fn tagged_hap(tag: &str, value: Value) -> Hap {
        let mut hap = hap(value);
        hap.context.tags.push(tag.to_string());
        hap
    }

    #[test]
    fn sync_creates_reuses_and_removes_surfaces_by_type_and_id() {
        let mut host = WidgetHostState::default();
        let first = vec![
            widget("_spiral", "same", 0, 12),
            widget("_scope", "scope", 20, 30),
        ];
        let sync = host.sync(&first);
        let spiral_serial = host.surface_serial("_spiral", "same").unwrap();

        assert_eq!(sync.created, vec!["same", "scope"]);
        assert!(sync.removed.is_empty());
        assert_eq!(host.surface_count(), 2);

        let second = vec![
            widget("_spiral", "same", 100, 120),
            widget("_pitchwheel", "wheel", 40, 50),
        ];
        let sync = host.sync(&second);

        assert_eq!(host.surface_serial("_spiral", "same"), Some(spiral_serial));
        assert_eq!(sync.created, vec!["wheel"]);
        assert_eq!(sync.removed, vec!["scope"]);
        assert_eq!(host.surface_count(), 2);
    }

    #[test]
    fn widget_identity_includes_type_and_id() {
        let mut host = WidgetHostState::default();
        host.sync(&[
            widget("_scope", "shared", 0, 1),
            widget("_spectrum", "shared", 2, 3),
        ]);

        assert_eq!(host.surface_count(), 2);
        assert_ne!(
            host.surface_serial("_scope", "shared"),
            host.surface_serial("_spectrum", "shared")
        );
    }

    #[test]
    fn placement_uses_to_or_from_like_codemirror_widget_range() {
        assert_eq!(widget("_spiral", "a", 4, 12).placement(), 12);
        assert_eq!(widget("_spiral", "a", 4, 4).placement(), 4);
    }

    #[test]
    fn default_sizes_follow_strudel_canvas_defaults() {
        assert_eq!(default_surface_size("_pianoroll"), egui::vec2(500.0, 60.0));
        assert_eq!(default_surface_size("_scope"), egui::vec2(500.0, 60.0));
        assert_eq!(default_surface_size("_spiral"), egui::vec2(275.0, 275.0));
        assert_eq!(
            default_surface_size("_pitchwheel"),
            egui::vec2(200.0, 200.0)
        );
    }

    #[test]
    fn surface_size_follows_widget_size_width_and_height_options() {
        let sized = widget_with_options(
            "_spiral",
            &[("size", rudel_lang::WidgetOption::Number(180.0))],
        );
        let explicit = widget_with_options(
            "_pianoroll",
            &[
                ("width", rudel_lang::WidgetOption::Number(320.0)),
                ("height", rudel_lang::WidgetOption::Number(90.0)),
            ],
        );

        assert_eq!(surface_size(&sized), egui::vec2(180.0, 180.0));
        assert_eq!(surface_size(&explicit), egui::vec2(320.0, 90.0));
    }

    #[test]
    fn widget_draw_colors_follow_strudel_draw_theme_defaults() {
        let colors = widget_draw_colors(EditorTheme::StrudelDark.draw_theme());
        assert_eq!(colors.active, egui::Color32::WHITE);
        assert_eq!(
            colors.inactive,
            egui::Color32::from_rgba_unmultiplied(0x8a, 0x91, 0x99, 0x66)
        );
        assert_eq!(
            colors.background,
            egui::Color32::from_rgba_unmultiplied(0x22, 0x22, 0x22, 0x99)
        );
    }

    #[test]
    fn hap_matching_prefers_widget_tags_and_falls_back_to_source_locations() {
        let target = widget("_spiral", "target", 10, 20);
        let tagged = tagged_hap("target", Value::Int(60));
        let other = tagged_hap("other", Value::Int(60));
        let mut located = hap(Value::Int(60));
        located.context.locations.push((12, 14));

        assert!(hap_matches_widget(&tagged, &target));
        assert!(!hap_matches_widget(&other, &target));
        assert!(hap_matches_widget(&located, &target));
    }

    #[test]
    fn pianoroll_value_matches_strudel_value_priority() {
        let freq = hap(Value::Map(BTreeMap::from([(
            "freq".to_string(),
            Value::F64(440.0),
        )])));
        let note = hap(Value::Map(BTreeMap::from([(
            "note".to_string(),
            Value::Str("c4".to_string()),
        )])));
        let sound = hap(Value::Map(BTreeMap::from([(
            "s".to_string(),
            Value::Str("bd".to_string()),
        )])));

        assert_eq!(pianoroll_value(&freq), RollValue::Number(69.0));
        assert_eq!(pianoroll_value(&note), RollValue::Number(60.0));
        assert_eq!(pianoroll_value(&sound), RollValue::Text("_bd".to_string()));
    }

    #[test]
    fn pianoroll_rect_places_current_time_at_the_playhead() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 40.0));
        let block = horizontal_roll_rect(
            rect,
            RollRectInput {
                value_index: 0,
                slots: 1,
                begin: 10.0,
                end: 10.5,
                window_start: 8.0,
                time_extent: 4.0,
                options: VisualWidgetOptions::from_widget(&widget("_pianoroll", "piano", 0, 1)),
            },
        );

        assert!((block.left() - 201.0).abs() < 1e-4);
        assert!((block.right() - 249.0).abs() < 1e-4);
        assert!((block.top() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn visual_widget_options_read_strudel_style_booleans_numbers_and_colors() {
        let widget = widget_with_options(
            "_pianoroll",
            &[
                ("cycles", rudel_lang::WidgetOption::Number(2.0)),
                ("labels", rudel_lang::WidgetOption::Number(1.0)),
                (
                    "active",
                    rudel_lang::WidgetOption::String("#ff00ff".to_string()),
                ),
            ],
        );
        let options = VisualWidgetOptions::from_widget(&widget);

        assert_eq!(options.cycles, 2.0);
        assert!(options.labels);
        assert_eq!(
            options.active_color,
            Some(egui::Color32::from_rgb(0xff, 0, 0xff))
        );
    }

    #[test]
    fn spiral_options_map_inline_canvas_size_to_draw_size() {
        let default = VisualWidgetOptions::from_widget(&widget("_spiral", "spiral", 0, 1));
        let sized = VisualWidgetOptions::from_widget(&widget_with_options(
            "_spiral",
            &[("size", rudel_lang::WidgetOption::Number(250.0))],
        ));

        assert_eq!(default.spiral_size, 55.0);
        assert_eq!(sized.spiral_size, 50.0);
    }

    #[test]
    fn pitchwheel_angle_matches_strudel_frequency_mapping() {
        let root = rudel_core::midi_to_freq(36.0);

        assert!((freq_to_angle(root, root) - 0.5).abs() < 1e-6);
        assert!((freq_to_angle(root * 2f64.powf(0.5), root) - 0.0).abs() < 1e-6);
        assert!((freq_to_angle(root / 2f64.powf(0.25), root) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn spiral_point_matches_strudel_polar_mapping() {
        let center = egui::pos2(100.0, 100.0);
        let at_start = spiral_point(0.0, 10.0, center, 0.0, 1.0);
        let one_turn = spiral_point(1.0, 10.0, center, 0.0, 1.0);

        assert!((at_start.x - 100.0).abs() < 1e-4);
        assert!((at_start.y - 100.0).abs() < 1e-4);
        assert!((one_turn.x - 100.0).abs() < 1e-4);
        assert!((one_turn.y - 90.0).abs() < 1e-4);
    }

    #[test]
    fn parses_hex_event_colors_and_applies_alpha() {
        assert_eq!(
            parse_hex_color("#ff000080"),
            Some(egui::Color32::from_rgba_unmultiplied(0xff, 0, 0, 0x80))
        );
        assert_eq!(
            color_with_alpha(egui::Color32::from_rgba_unmultiplied(10, 20, 30, 200), 0.5),
            egui::Color32::from_rgba_unmultiplied(10, 20, 30, 100)
        );
    }
}
