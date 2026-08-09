use super::{
    options::VisualWidgetOptions,
    query::hap_is_active,
    style::{WidgetDrawColors, color_with_alpha, event_alpha, event_color},
    values::{value_short, value_to_midi},
};
use eframe::egui;
use rudel_core::{Frac, Hap, Value};
use std::sync::{Arc, Mutex};

/// Blocks kept for `smear`, and the draw time they were last added at.
/// ponytail: a flat cap on the trail; a ring buffer only matters if a very
/// long session makes the redraw cost show up.
#[derive(Default)]
pub(super) struct SmearTrail {
    blocks: Vec<(egui::Rect, egui::Color32)>,
    last_time: f64,
}

/// Most smeared blocks kept before the oldest are dropped.
const MAX_SMEAR_BLOCKS: usize = 4096;

/// Smallest height (width, when vertical) a note block is drawn at, in points.
/// Below this an unfolded roll over a wide MIDI range paints nothing visible.
const MIN_NOTE_EXTENT: f32 = 2.5;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum RollValue {
    Number(f64),
    Text(String),
}

pub(super) fn paint_pianoroll(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget_id: &str,
    haps: &[Hap],
    time: f64,
    colors: WidgetDrawColors,
    options: VisualWidgetOptions,
) {
    // Clip to the widget, the way a canvas clips. The note blocks are already
    // intersected with `rect` below, but a label drawn inside a block that
    // reaches the edge is not, and would otherwise run over the editor.
    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));

    // `smear`: Strudel skips the canvas clear, so drawn notes stay put and
    // build up a solid trace. egui repaints from scratch every frame, so the
    // blocks are remembered and redrawn underneath the current ones. They are
    // only recorded when the draw time actually advanced, so a paused
    // transport does not restack the same blocks forever.
    let trail: Arc<Mutex<SmearTrail>> = ui.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<SmearTrail>>>(egui::Id::new((
            "rudel-pianoroll-smear",
            widget_id,
        )))
        .clone()
    });
    let mut trail = trail.lock().unwrap();
    let smear = options.smear > 0.0;
    if smear {
        for (block, color) in &trail.blocks {
            painter.rect_filled(*block, 1.5, *color);
        }
    } else if !trail.blocks.is_empty() {
        trail.blocks.clear();
    }
    let record = smear && time > trail.last_time;
    if record {
        trail.last_time = time;
    }
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
        let block = keep_note_visible(block, options.vertical).intersect(rect);
        if block.width() <= 0.5 || block.height() <= 0.5 {
            continue;
        }
        // Rounding cannot exceed half the block, or a thin note is rounded away
        // to nothing. An unfolded roll spreads `minMidi..maxMidi` over the
        // widget — 81 slots by default — so its notes are about a pixel tall and
        // vanished completely against the fixed 1.5 radius.
        let rounding = 1.5f32.min(block.height() / 2.0).min(block.width() / 2.0);
        let fill = (!active && options.fill) || (active && options.fill_active);
        if fill {
            painter.rect_filled(block, rounding, color);
            if record {
                if trail.blocks.len() >= MAX_SMEAR_BLOCKS {
                    trail.blocks.remove(0);
                }
                trail.blocks.push((block, color));
            }
        }
        let stroke = options.stroke.unwrap_or(options.stroke_active && active);
        if stroke {
            painter.rect_stroke(
                block,
                rounding,
                egui::Stroke::new(1.0, color),
                egui::StrokeKind::Inside,
            );
        }

        if options.labels && block.width() > 16.0 && block.height() > 10.0 {
            painter.text(
                block.left_center() + egui::vec2(3.0, 0.0),
                egui::Align2::LEFT_CENTER,
                roll_label(hap, active),
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
pub(super) struct RollRectInput {
    pub(super) value_index: usize,
    pub(super) slots: usize,
    pub(super) begin: f64,
    pub(super) end: f64,
    pub(super) window_start: f64,
    pub(super) time_extent: f64,
    pub(super) options: VisualWidgetOptions,
}

pub(super) fn horizontal_roll_rect(rect: egui::Rect, input: RollRectInput) -> egui::Rect {
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

/// Grow a note block across its value axis so it stays visible.
///
/// An unfolded roll gives every semitone between `minMidi` and `maxMidi` a slot
/// — 81 of them by default — and the inline widget is only about eighty points
/// tall, so a note came out under a point high and the roll looked empty.
/// Upstream draws the same way, into a canvas tall enough never to notice.
pub(super) fn keep_note_visible(block: egui::Rect, vertical: bool) -> egui::Rect {
    let across = if vertical {
        block.width()
    } else {
        block.height()
    };
    if across >= MIN_NOTE_EXTENT {
        return block;
    }
    let grow = (MIN_NOTE_EXTENT - across) / 2.0;
    block.expand2(if vertical {
        egui::vec2(grow, 0.0)
    } else {
        egui::vec2(0.0, grow)
    })
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

pub(super) fn pianoroll_value(hap: &Hap) -> RollValue {
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

/// The text drawn on a note block. Mirrors upstream's rule: `activeLabel`
/// replaces `label` only while the event is sounding, and with neither set the
/// label falls back to the note / sound name.
fn roll_label(hap: &Hap, active: bool) -> String {
    let controls = rudel_core::to_control_map(&hap.value);
    let custom = active
        .then(|| controls.get("activeLabel"))
        .flatten()
        .or_else(|| controls.get("label"));
    if let Some(value) = custom {
        return value_short(value);
    }
    for key in ["note", "s", "n"] {
        if let Some(value) = controls.get(key) {
            return value_short(value);
        }
    }
    value_short(&hap.value)
}
