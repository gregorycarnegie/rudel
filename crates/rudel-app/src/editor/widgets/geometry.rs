use super::size::surface_size;
use crate::editor::{decorations::WidgetDecoration, text::char_index_at_byte};
use eframe::egui;
use std::collections::HashMap;

/// Vertical padding added around a block widget inside its reserved row.
pub(super) const WIDGET_GAP_PADDING: f32 = 6.0;

/// Where the editor's laid-out text ended up, so widgets can be anchored to the
/// real row geometry (which accounts for the space reserved for them).
#[derive(Clone, Copy)]
pub(crate) struct WidgetLayout<'a> {
    pub(crate) galley: &'a egui::Galley,
    pub(crate) galley_pos: egui::Pos2,
    pub(crate) editor_rect: egui::Rect,
    pub(crate) base_row_height: f32,
}

/// Full row height (base text row plus the gap reserved below it) for each
/// source line that hosts one or more block widgets. The layouter inflates
/// these rows so the following code is pushed down instead of overlapped.
pub(crate) fn block_widget_line_heights(
    code: &str,
    widgets: &[WidgetDecoration],
    base_row_height: f32,
) -> HashMap<usize, f32> {
    let mut heights: HashMap<usize, f32> = HashMap::new();
    for widget in widgets {
        let (line, _) = line_column_at_byte(code, widget.placement());
        let reserved = surface_size(widget).y + WIDGET_GAP_PADDING;
        *heights.entry(line).or_insert(base_row_height) += reserved;
    }
    heights
}

pub(super) fn widget_rect(
    layout: WidgetLayout<'_>,
    code: &str,
    widget: &WidgetDecoration,
    size: egui::Vec2,
    stack_offset: f32,
) -> egui::Rect {
    // Anchor to the bottom of the widget's (inflated) row in the real galley, so
    // the widget sits in the gap reserved below the line, pushing following code
    // down rather than overlapping it.
    let char_index = char_index_at_byte(code, widget.placement());
    let row = layout
        .galley
        .pos_from_cursor(egui::text::CCursor::new(char_index));
    let top = layout.galley_pos.y + row.min.y + layout.base_row_height + stack_offset;
    let x = layout.editor_rect.min.x + 6.0;
    let max_width = (layout.editor_rect.right() - x).max(160.0);
    egui::Rect::from_min_size(
        egui::pos2(x, top),
        egui::vec2(size.x.min(max_width), size.y),
    )
}

pub(super) fn line_column_at_byte(code: &str, byte: usize) -> (usize, usize) {
    let byte = byte.min(code.len());
    let prefix = &code[..byte];
    let line = prefix.bytes().filter(|b| *b == b'\n').count();
    let line_start = prefix.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let column = prefix[line_start..].chars().count();
    (line, column)
}
