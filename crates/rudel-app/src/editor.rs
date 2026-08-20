use eframe::egui;
use std::collections::HashSet;

pub(crate) mod blocks;
mod brackets;
mod completion;
#[cfg(test)]
mod contract;
pub(crate) mod decorations;
mod edit;
mod highlight;
mod menu;
pub(crate) mod settings;
mod sliders;
mod text;
pub(crate) mod widgets;

use brackets::bracket_match_spans;
use completion::{
    Completion, CompletionCatalog, apply_completion, completion_at, completion_popup,
    completion_tooltip, reference_tooltip_at,
};
use decorations::{SliderDecoration, TextChange, WidgetDecoration};
use edit::{
    apply_editor_text_edits, capture_editor_shortcuts, editor_enter_pressed, editor_typed_text,
};
use highlight::highlighted_editor_job;
pub(crate) use highlight::pack_color;
pub(crate) use menu::EditorAction;
use menu::{MenuChoice, editor_context_menu};
use settings::{EditorSettings, apply_editor_style};
use sliders::{SliderHostUpdate, SliderLayout, draw_slider_hosts};
use text::{byte_index_at_char, char_slice};
pub(crate) use widgets::{ShaderStore, SpiralStore, mark_color};
use widgets::{WidgetHostState, WidgetLayout, WidgetPaintInput, draw_widget_hosts};

const CODE_EDITOR_ID: &str = "rudel_code_editor";

#[derive(Default)]
pub(crate) struct EditorOutput {
    pub(crate) text_change: Option<TextChange>,
    pub(crate) slider_update: Option<SliderHostUpdate>,
    /// Cursor byte offset, as plain `usize` for the app layer (block eval);
    /// inside the editor module byte offsets are typed [`egui::text::ByteIndex`].
    pub(crate) cursor_byte: Option<usize>,
    /// Picked from the right-click menu; run by the app, which owns the engine.
    pub(crate) action: Option<EditorAction>,
}

pub(crate) struct CodeEditorInput<'a> {
    pub(crate) active: &'a [decorations::FlashSpan],
    pub(crate) idents: &'a HashSet<String>,
    pub(crate) reference: &'a rudel_lang::Reference,
    pub(crate) sample_names: &'a [String],
    pub(crate) current_pattern: Option<&'a rudel_core::Pattern>,
    /// Bumped on every evaluation; see [`WidgetPaintInput::pattern_generation`].
    pub(crate) pattern_generation: u64,
    pub(crate) playback_position_cycles: Option<f64>,
    /// The engine's analyzer taps for the scope/fscope/spectrum widgets.
    pub(crate) scope_taps: Option<&'a rudel_audio::ScopeTaps>,
    /// Whether the wgpu backend is running; see
    /// [`WidgetPaintInput::gpu_available`].
    pub(crate) gpu_available: bool,
    pub(crate) sliders: &'a [SliderDecoration],
    pub(crate) widgets: &'a [WidgetDecoration],
    pub(crate) widget_host: &'a mut WidgetHostState,
    pub(crate) settings: &'a EditorSettings,
    /// Text to insert at the cursor this frame (a double-clicked reference).
    pub(crate) insert_text: Option<String>,
}

pub(crate) fn code_editor(
    ui: &mut egui::Ui,
    code: &mut String,
    input: CodeEditorInput<'_>,
) -> EditorOutput {
    let CodeEditorInput {
        active,
        idents,
        reference,
        sample_names,
        current_pattern,
        pattern_generation,
        playback_position_cycles,
        scope_taps,
        gpu_available,
        sliders,
        widgets,
        widget_host,
        settings,
        insert_text,
    } = input;

    apply_editor_style(ui, settings);
    let before = code.clone();
    let editor_id = ui.make_persistent_id(egui::Id::new(CODE_EDITOR_ID));
    let bracket_id = editor_id.with("bracket_match");
    let completion_id = editor_id.with("completion");
    let tooltip_id = editor_id.with("tooltip");
    let active_line_id = editor_id.with("active_line");
    let completion_catalog = CompletionCatalog {
        idents,
        reference,
        sample_names,
    };

    // Completion popup state carried from last frame (empty items == inactive).
    let stored: Completion = if settings.autocomplete {
        ui.data(|d| d.get_temp(completion_id)).unwrap_or_default()
    } else {
        Completion::default()
    };
    let mut completion = settings
        .autocomplete
        .then_some(stored)
        .filter(|stored| !stored.items.is_empty());

    let shortcuts = capture_editor_shortcuts(ui, editor_id, completion.is_some(), settings);
    let typed_text = editor_typed_text(ui);
    let enter_pressed = editor_enter_pressed(ui);
    // Bracket-match spans computed from last frame's cursor (the layouter runs
    // before this frame's cursor is known); recomputed and stored below.
    let brackets: Vec<(usize, usize)> = if settings.bracket_matching {
        ui.data(|d| d.get_temp(bracket_id)).unwrap_or_default()
    } else {
        Vec::new()
    };
    let active_line: Option<(usize, usize)> = if settings.active_line {
        ui.data(|d| d.get_temp(active_line_id))
    } else {
        None
    };
    // Reserve layout space so block widgets push the code below them down and
    // inline sliders push the rest of their line right, rather than painting on
    // top of the code (matching Strudel's block/inline CodeMirror widgets).
    let editor_font = settings.font_id();
    let base_row_height = ui.fonts_mut(|fonts| fonts.row_height(&editor_font));
    let line_heights = widgets::block_widget_line_heights(code, widgets, base_row_height);
    let slider_reservations = sliders::slider_reservations(sliders);
    let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        let job = highlighted_editor_job(
            text.as_str(),
            wrap_width,
            active,
            &brackets,
            active_line,
            idents,
            settings,
            highlight::LayoutReservations {
                line_heights: &line_heights,
                sliders: &slider_reservations,
            },
        );
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    // Pin the editor background to its own theme so the syntax palette (whose
    // `Normal` tokens — punctuation like `().,` — use the theme foreground) sits
    // on the matching background regardless of the host/system egui theme.
    // Otherwise white punctuation lands on a light system background and vanishes.
    let editor_bg = settings.draw_theme().background;
    // Grow the editor to fill the remaining height of its panel so it resizes
    // with the window instead of staying a fixed 28-row box. Content longer than
    // this still scrolls inside the surrounding ScrollArea.
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    let desired_rows = ((ui.available_height() / row_height).floor() as usize).max(4);
    let mut output = if settings.line_numbers {
        ui.horizontal_top(|ui| {
            draw_line_number_gutter(
                ui,
                code,
                active_line,
                settings,
                &line_heights,
                base_row_height,
            );
            egui::TextEdit::multiline(code)
                // Pin an absolute id (not `id_salt`) so the widget keeps the
                // same id whether it sits in the outer `ui` or inside this
                // `horizontal_top` child `ui`. The shortcut focus gate matches
                // on `editor_id`, so a layout-dependent id silently disables
                // Ctrl+/ (and Tab/Alt+W) when the line-number gutter is on.
                .id(editor_id)
                .code_editor()
                .background_color(editor_bg)
                .layouter(&mut layouter)
                .desired_rows(desired_rows)
                .desired_width(f32::INFINITY)
                .show(ui)
        })
        .inner
    } else {
        egui::TextEdit::multiline(code)
            .id(editor_id)
            .code_editor()
            .background_color(editor_bg)
            .layouter(&mut layouter)
            .desired_rows(desired_rows)
            .desired_width(f32::INFINITY)
            .show(ui)
    };

    let mut cursor_byte = None;
    if output.response.has_focus()
        && let Some(cursor_range) = output.cursor_range
    {
        let mut cursor = cursor_range.primary.index;
        let mut handled = false;

        // Completion-popup interactions take priority over text editing.
        if let Some(state) = completion.as_mut() {
            if shortcuts.complete_dismiss {
                completion = None;
                handled = true;
            } else if shortcuts.complete_accept {
                let item = state.items[state.selected].clone();
                let cursor_byte = byte_index_at_char(code, cursor);
                cursor = apply_completion(code, state.start, cursor_byte, &item);
                output
                    .state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(
                        egui::text::CCursor::new(cursor),
                    )));
                output.state.clone().store(ui.ctx(), output.response.id);
                completion = None;
                handled = true;
            } else if shortcuts.complete_next {
                state.selected = stepped_selection(state.selected, state.items.len(), true);
                handled = true;
            } else if shortcuts.complete_prev {
                state.selected = stepped_selection(state.selected, state.items.len(), false);
                handled = true;
            }
        }

        if !handled {
            let edited = apply_editor_text_edits(
                code,
                cursor_range,
                shortcuts,
                typed_text.as_deref(),
                enter_pressed,
                settings,
            );
            cursor = edited.map(|r| r.primary.index).unwrap_or(cursor);
            if let Some(new_range) = edited {
                output.state.cursor.set_char_range(Some(new_range));
                output.state.clone().store(ui.ctx(), output.response.id);
            }
            // Open on typing, refresh while already open, otherwise close.
            let prev = completion.take();
            if settings.autocomplete && (typed_text.is_some() || prev.is_some()) {
                let cursor_byte = byte_index_at_char(code, cursor);
                completion = completion_at(code, cursor_byte, &completion_catalog).map(
                    |(start, _, items)| {
                        let selected = carried_selection(prev.as_ref(), start, items.len());
                        Completion {
                            start,
                            items,
                            selected,
                        }
                    },
                );
            }
        }

        if handled {
            ui.ctx().request_repaint();
        }

        // Refresh the bracket-match highlight for the (possibly moved) cursor.
        cursor_byte = Some(byte_index_at_char(code, cursor));
        if settings.bracket_matching {
            let new_brackets = bracket_match_spans(code, cursor)
                .map(|pair| pair.to_vec())
                .unwrap_or_default();
            if new_brackets != brackets {
                ui.data_mut(|d| d.insert_temp(bracket_id, new_brackets));
                ui.ctx().request_repaint();
            }
        }
        if settings.active_line {
            let new_active_line = line_span_at_char(code, cursor);
            if Some(new_active_line) != active_line {
                ui.data_mut(|d| d.insert_temp(active_line_id, new_active_line));
                ui.ctx().request_repaint();
            }
        }
    } else {
        completion = None;
        if !brackets.is_empty() {
            ui.data_mut(|d| d.insert_temp(bracket_id, Vec::<(usize, usize)>::new()));
            ui.ctx().request_repaint();
        }
        if active_line.is_some() {
            ui.data_mut(|d| d.remove::<(usize, usize)>(active_line_id));
            ui.ctx().request_repaint();
        }
    }

    // Right-click menu. Runs after the edit block so a menu-driven edit is the
    // last word on the cursor, and outside its `has_focus` gate — clicking a
    // menu entry takes focus off the editor.
    let selection = output
        .cursor_range
        .filter(|range| !range.is_empty())
        .map(|range| range.as_sorted_char_range());
    let mut action = None;
    if let Some(choice) = editor_context_menu(&output.response, selection.is_some()) {
        let moved = match choice {
            MenuChoice::App(app_action) => {
                action = Some(app_action);
                None
            }
            MenuChoice::Edit(shortcuts) => output.cursor_range.and_then(|range| {
                apply_editor_text_edits(code, range, shortcuts, None, false, settings)
            }),
            MenuChoice::Copy => {
                if let Some(range) = selection {
                    ui.ctx().copy_text(char_slice(code, range).to_string());
                }
                None
            }
            MenuChoice::Cut => selection.map(|range| {
                ui.ctx()
                    .copy_text(char_slice(code, range.clone()).to_string());
                text::replace_char_range(code, range.clone(), "");
                egui::text::CCursorRange::one(egui::text::CCursor::new(range.start))
            }),
            MenuChoice::Paste => menu::clipboard_text().map(|text| {
                let range = selection.unwrap_or(
                    output
                        .cursor_range
                        .map(|range| range.as_sorted_char_range())
                        .unwrap_or(
                            egui::text::CharIndex(code.chars().count())
                                ..egui::text::CharIndex(code.chars().count()),
                        ),
                );
                let after = range.start + text.chars().count();
                text::replace_char_range(code, range, &text);
                egui::text::CCursorRange::one(egui::text::CCursor::new(after))
            }),
            MenuChoice::SelectAll => Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(egui::text::CharIndex(0)),
                egui::text::CCursor::new(egui::text::CharIndex(code.chars().count())),
            )),
        };
        if let Some(range) = moved {
            output.state.cursor.set_char_range(Some(range));
            output.state.clone().store(ui.ctx(), output.response.id);
            cursor_byte = Some(byte_index_at_char(code, range.primary.index));
        }
        output.response.request_focus();
        ui.ctx().request_repaint();
    }

    // Insert a reference name from the side panel: a drag lands at the pointer
    // position, a double-click at the current cursor (end of code when the
    // editor has never had one). Mutating `code` here keeps the insertion
    // inside the `before`/after diff so decorations are remapped like any edit.
    let insertion: Option<(String, egui::text::CharIndex)> =
        if let Some(payload) = output.response.dnd_release_payload::<String>() {
            let pos = ui.ctx().pointer_interact_pos().unwrap_or(output.galley_pos);
            let at = output.galley.cursor_from_pos(pos - output.galley_pos).index;
            Some((payload.as_str().to_string(), at))
        } else {
            insert_text.map(|text| {
                let at = output
                    .state
                    .cursor
                    .char_range()
                    .map(|range| range.primary.index)
                    .unwrap_or(egui::text::CharIndex(code.chars().count()));
                (text, at)
            })
        };
    if let Some((text, at)) = insertion {
        text::insert_text_at_char(code, at, &text);
        let after = at + text.chars().count();
        output
            .state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(after),
            )));
        output.state.clone().store(ui.ctx(), output.response.id);
        cursor_byte = Some(byte_index_at_char(code, after));
        ui.ctx().request_repaint();
    }

    if let Some(state) = &completion {
        completion_popup(ui, completion_id, &output.response, state);
    }
    if settings.tooltips
        && ui.input(|i| i.modifiers.ctrl)
        && let Some(cursor) = cursor_byte
        && let Some(item) = reference_tooltip_at(code, cursor, &completion_catalog)
    {
        completion_tooltip(ui, tooltip_id, &output.response, &item);
    }
    if settings.autocomplete {
        ui.data_mut(|d| d.insert_temp(completion_id, completion.unwrap_or_default()));
    } else {
        ui.data_mut(|d| d.remove::<Completion>(completion_id));
    }
    let draw_theme = settings.draw_theme();
    let galley_pos = output.galley_pos;
    let galley = output.galley.clone();
    draw_widget_hosts(
        ui,
        code,
        WidgetLayout {
            galley: &galley,
            galley_pos,
            editor_rect: output.response.rect,
            base_row_height,
        },
        widgets,
        widget_host,
        WidgetPaintInput {
            pattern: current_pattern,
            pattern_generation,
            time_cycles: playback_position_cycles,
            draw_theme,
            taps: scope_taps,
            gpu_available,
        },
    );
    let slider_update = draw_slider_hosts(
        ui,
        code,
        SliderLayout {
            galley: &galley,
            galley_pos,
            base_row_height,
        },
        sliders,
        draw_theme,
    );

    EditorOutput {
        text_change: TextChange::from_texts(&before, code),
        slider_update,
        cursor_byte: cursor_byte.map(|byte: egui::text::ByteIndex| byte.0),
        action,
    }
}

/// The completion entry selected after a move, wrapping at both ends so
/// holding the key cycles rather than sticking.
fn stepped_selection(selected: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    match forward {
        true => (selected + 1) % len,
        false => (selected + len - 1) % len,
    }
}

/// The entry to keep selected when the popup refreshes. The choice survives
/// only while the word being completed is the same one — a new word starts at
/// the top — and is clamped, since the shorter list of a longer prefix may not
/// reach as far as the old index.
fn carried_selection(prev: Option<&Completion>, start: egui::text::ByteIndex, len: usize) -> usize {
    prev.filter(|c| c.start == start)
        .map(|c| c.selected.min(len.saturating_sub(1)))
        .unwrap_or(0)
}

fn draw_line_number_gutter(
    ui: &mut egui::Ui,
    code: &str,
    active_line: Option<(usize, usize)>,
    settings: &EditorSettings,
    line_heights: &std::collections::HashMap<usize, f32>,
    base_row_height: f32,
) {
    let font_id = settings.font_id();
    let line_count = code.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let digits = line_count.to_string().len().max(2);
    let width = digits as f32 * settings.font_size * 0.62 + 10.0;
    let active_line_index = active_line.map(|(from, _)| {
        code[..from.min(code.len())]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
    });
    let palette = settings.theme.palette();
    // ponytail: assumes no soft wrap — numbers drift when line_wrapping is on;
    // walk galley rows instead if that ever matters.
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 0.0;
        // Match TextEdit's inner top margin so row 1 lines up with the text.
        ui.add_space(2.0);
        for line in 0..line_count {
            let color = if Some(line) == active_line_index {
                palette.line_number_active
            } else {
                palette.line_number
            };
            // Rows hosting block widgets are inflated by the layouter; mirror
            // that height so later numbers stay aligned, with the number pinned
            // to the top where the text row is.
            let row_height = line_heights.get(&line).copied().unwrap_or(base_row_height);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width, row_height), egui::Sense::hover());
            ui.painter().text(
                egui::pos2(rect.right() - 4.0, rect.top()),
                egui::Align2::RIGHT_TOP,
                (line + 1).to_string(),
                font_id.clone(),
                color,
            );
        }
    });
}

fn line_span_at_char(code: &str, cursor_char: egui::text::CharIndex) -> (usize, usize) {
    let byte = byte_index_at_char(code, cursor_char).0;
    let start = code[..byte].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let end = code[byte..]
        .find('\n')
        .map(|offset| byte + offset)
        .unwrap_or(code.len());
    if start == end && end < code.len() {
        (start, end + 1)
    } else {
        (start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_span_covers_the_line_the_cursor_sits_on() {
        // Byte range of the cursor's line, newline excluded. This backs the
        // current-line highlight, so an off-by-one paints into a neighbour.
        let code = "one\ntwo\n\nfour";
        let span = |ch: usize| line_span_at_char(code, egui::text::CharIndex(ch));
        assert_eq!(span(0), (0, 3), "start of the first line");
        assert_eq!(span(2), (0, 3), "inside it");
        assert_eq!(span(3), (0, 3), "at its newline, still on that line");
        assert_eq!(span(4), (4, 7), "the second line");
        assert_eq!(span(9), (9, 13), "the last line runs to the end of input");
    }

    #[test]
    fn an_empty_line_spans_its_own_newline() {
        // A blank line has no bytes of its own, so the span would be empty and
        // the highlight would vanish; it takes in the newline instead. Except
        // at the very end of the buffer, where there is no newline to take.
        let code = "one\ntwo\n\nfour";
        assert_eq!(
            line_span_at_char(code, egui::text::CharIndex(8)),
            (8, 9),
            "the blank third line"
        );
        let trailing = "a\n";
        assert_eq!(
            line_span_at_char(trailing, egui::text::CharIndex(2)),
            (2, 2),
            "the empty line after a trailing newline stays empty"
        );
        assert_eq!(
            line_span_at_char("", egui::text::CharIndex(0)),
            (0, 0),
            "an empty buffer"
        );
    }
    /// Every text run the gutter painted, as `(text, top-left, colour)`.
    fn gutter_shapes(
        code: &str,
        active_line: Option<(usize, usize)>,
        heights: &std::collections::HashMap<usize, f32>,
    ) -> Vec<(String, egui::Pos2, egui::Color32)> {
        gutter_run(code, active_line, heights).0
    }

    /// As [`gutter_shapes`], plus the left edge the gutter was laid out from,
    /// so absolute positions can be checked rather than only differences.
    #[allow(clippy::type_complexity)]
    fn gutter_run(
        code: &str,
        active_line: Option<(usize, usize)>,
        heights: &std::collections::HashMap<usize, f32>,
    ) -> (Vec<(String, egui::Pos2, egui::Color32)>, f32) {
        let (shapes, left, _) = gutter_run_full(code, active_line, heights);
        (shapes, left)
    }

    /// As [`gutter_run`], plus the right edge of the first number's box — the
    /// text is RIGHT-anchored, so the shape's own `pos` is its *left* edge.
    #[allow(clippy::type_complexity)]
    fn gutter_run_full(
        code: &str,
        active_line: Option<(usize, usize)>,
        heights: &std::collections::HashMap<usize, f32>,
    ) -> (Vec<(String, egui::Pos2, egui::Color32)>, f32, f32) {
        let ctx = egui::Context::default();
        let settings = EditorSettings::default();
        // Fonts do not exist until a pass has run.
        // A real viewport: with the default zero-sized one, egui clamps the
        // gutter's allocation and the positions mean nothing.
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let mut warmup = ctx.run_ui(input(), |_| {});
        warmup.textures_delta.clear();
        let left = std::cell::Cell::new(0.0);
        let mut out = ctx.run_ui(input(), |ui| {
            left.set(ui.min_rect().left());
            draw_line_number_gutter(ui, code, active_line, &settings, heights, 14.0);
        });
        out.textures_delta.clear();
        let right = std::cell::Cell::new(0.0);
        for clipped in &out.shapes {
            if let egui::Shape::Text(text) = &clipped.shape {
                right.set(text.pos.x + text.galley.size().x);
                break;
            }
        }
        let shapes = out
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.text().to_string(),
                    text.pos,
                    text.galley
                        .job
                        .sections
                        .first()
                        .map(|s| s.format.color)
                        .unwrap_or(egui::Color32::PLACEHOLDER),
                )),
                _ => None,
            })
            .collect();
        (shapes, left.get(), right.get())
    }

    #[test]
    fn the_gutter_numbers_every_line_from_one() {
        let heights = std::collections::HashMap::new();
        let drawn = gutter_shapes("a
b
c", None, &heights);
        let labels: Vec<&str> = drawn.iter().map(|(t, _, _)| t.as_str()).collect();
        assert_eq!(labels, ["1", "2", "3"], "one number per line, 1-based");

        // A buffer with no newline is still one line, and a trailing newline
        // opens the next one.
        assert_eq!(gutter_shapes("a", None, &heights).len(), 1);
        assert_eq!(gutter_shapes("a
", None, &heights).len(), 2);
    }

    #[test]
    fn the_active_line_number_is_the_one_the_cursor_is_on() {
        let heights = std::collections::HashMap::new();
        let palette = EditorSettings::default().theme.palette();
        // Cursor in the second line: byte 2 is its first character.
        // Lines longer than one character: with single-char lines, counting
        // the newlines before the cursor and counting everything else give the
        // same answer, and a wrong one passes.
        let drawn = gutter_shapes("ab
cd
ef", Some((3, 3)), &heights);
        let active: Vec<&str> = drawn
            .iter()
            .filter(|(_, _, color)| *color == palette.line_number_active)
            .map(|(t, _, _)| t.as_str())
            .collect();
        assert_eq!(active, ["2"], "only the cursor's line is highlighted");

        // With no cursor, nothing is.
        let drawn = gutter_shapes("ab
cd", None, &heights);
        assert!(
            drawn
                .iter()
                .all(|(_, _, color)| *color == palette.line_number),
            "no line is active"
        );
    }

    #[test]
    fn the_gutter_widens_for_a_longer_line_count() {
        // Numbers are right-aligned inside a gutter sized to the widest one,
        // so a three-digit file has to push its column right.
        let heights = std::collections::HashMap::new();
        let short = gutter_shapes("a
b", None, &heights)[0].1.x;
        let long = gutter_shapes(&"x
".repeat(120), None, &heights)[0].1.x;
        let settings = EditorSettings::default();
        assert!(
            (long - short - settings.font_size * 0.62).abs() < 0.01,
            "one more digit of room, got {short} then {long}"
        );

        // ...and the column sits an exact distance in from the left edge: the
        // gutter's own width, less the right-hand padding the number keeps.
        let (_, left, right) = gutter_run_full("a
b", None, &heights);
        let width = 2.0 * settings.font_size * 0.62 + 10.0;
        assert!(
            (right - (left + width - 4.0)).abs() < 0.01,
            "right-aligned inside the gutter, got {right} from {left}"
        );
    }

    #[test]
    fn a_row_hosting_a_widget_pushes_the_numbers_below_it_down() {
        // The layouter inflates rows carrying inline widgets; the gutter has to
        // mirror that or every number after one drifts out of line.
        let mut heights = std::collections::HashMap::new();
        let flat = gutter_shapes("a
b
c", None, &heights);
        heights.insert(0, 60.0);
        let inflated = gutter_shapes("a
b
c", None, &heights);
        assert_eq!(flat[0].1.y, inflated[0].1.y, "the inflated row itself");
        assert!(
            inflated[1].1.y - flat[1].1.y > 40.0,
            "the numbers below it move down by the extra height"
        );
    }
    #[test]
    fn the_completion_selection_wraps_at_both_ends() {
        // Holding the key cycles the list rather than sticking at either end.
        assert_eq!(stepped_selection(0, 3, true), 1);
        assert_eq!(stepped_selection(2, 3, true), 0, "past the end wraps to 0");
        assert_eq!(stepped_selection(1, 3, false), 0);
        assert_eq!(
            stepped_selection(0, 3, false),
            2,
            "back past the start wraps to the end"
        );
        // A one-entry list stays put either way, and an empty one has nothing
        // to select — the `% len` would divide by zero.
        assert_eq!(stepped_selection(0, 1, true), 0);
        assert_eq!(stepped_selection(0, 1, false), 0);
        assert_eq!(stepped_selection(0, 0, true), 0);
    }

    #[test]
    fn a_refreshed_popup_keeps_the_choice_only_for_the_same_word() {
        let completion = |start: usize, selected: usize| Completion {
            start: egui::text::ByteIndex(start),
            items: Vec::new(),
            selected,
        };
        let at = egui::text::ByteIndex(4);

        // Same word, so the highlighted entry survives another keystroke.
        assert_eq!(carried_selection(Some(&completion(4, 2)), at, 5), 2);
        // A different word starts at the top...
        assert_eq!(carried_selection(Some(&completion(9, 2)), at, 5), 0);
        // ...as does a first open.
        assert_eq!(carried_selection(None, at, 5), 0);
        // A longer prefix means a shorter list, which may not reach as far as
        // the old index.
        assert_eq!(carried_selection(Some(&completion(4, 4)), at, 2), 1);
        assert_eq!(carried_selection(Some(&completion(4, 4)), at, 0), 0);
    }
}
