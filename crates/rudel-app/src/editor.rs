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
pub(crate) use widgets::{HydraStore, ShaderStore, SpiralStore, mark_color, spiral_gpu_supported};
use widgets::{WidgetHostState, WidgetLayout, WidgetPaintInput, draw_widget_hosts, paint_backdrop};

const CODE_EDITOR_ID: &str = "rudel_code_editor";
/// Where the editor leaves the size of its visible area for the draw canvas.
pub(crate) const CANVAS_SIZE: &str = "rudel-draw-canvas-size";

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
    /// The hydra scene to draw behind the code, if the script made one.
    pub(crate) backdrop: Option<&'a WidgetDecoration>,
    /// hydra's clock, and this frame's per-frame hydra argument values.
    pub(crate) hydra_time: f64,
    pub(crate) hydra_values: &'a [Option<f64>],
    /// Strudel's draw canvas, drawn behind the code over any hydra scene.
    pub(crate) draw_canvas: Option<egui::TextureId>,
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
        backdrop,
        hydra_time,
        hydra_values,
        draw_canvas,
        widget_host,
        settings,
        insert_text,
    } = input;
    // `initHydra({feedStrudel})`: the canvas visuals feed the scene's `s0`.
    let feeds = backdrop.is_some_and(|b| widgets::option_bool(&b.options, "feed") == Some(true));
    let canvas: Vec<WidgetDecoration> = if feeds {
        widgets
            .iter()
            .filter(|w| w.options.contains_key(rudel_lang::CANVAS_OPTION))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let paint = WidgetPaintInput {
        canvas: &canvas,
        pattern: current_pattern,
        pattern_generation,
        time_cycles: playback_position_cycles,
        draw_theme: settings.draw_theme(),
        taps: scope_taps,
        gpu_available,
        zoom: 1.0,
        hydra_time,
        hydra_values,
    };

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
    // A visual fed into hydra is not drawn inline, so it gets no room there.
    let inline: Vec<WidgetDecoration> = widgets
        .iter()
        .filter(|w| !canvas.iter().any(|c| c.id == w.id))
        .cloned()
        .collect();
    let line_heights = widgets::block_widget_line_heights(code, &inline, base_row_height);
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
    let mut editor_bg = settings.draw_theme().background;
    // Hydra's own canvas sits behind the code, as upstream's does; the code
    // keeps a wash of the theme background so it stays readable over it.
    if let Some(backdrop) = backdrop {
        paint_backdrop(ui, ui.clip_rect(), backdrop, paint);
        editor_bg = editor_bg.gamma_multiply(0.5);
    }
    // The canvas is the size of what the editor shows; the app draws the next
    // frame at that size.
    let visible = ui.clip_rect();
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new(CANVAS_SIZE), visible.size()));
    if let Some(texture) = draw_canvas {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter()
            .image(texture, visible, uv, egui::Color32::WHITE);
        if backdrop.is_none() {
            editor_bg = editor_bg.gamma_multiply(0.5);
        }
    }
    // Grow the editor to fill the remaining height of its panel so it resizes
    // with the window instead of staying a fixed 28-row box. Content longer than
    // this still scrolls inside the surrounding ScrollArea.
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    let desired_rows = ((ui.available_height() / row_height).floor() as usize).max(4);
    // egui's TextEdit puts the cursor under the pointer on *any* button's
    // press, so a right-click would drop the selection the menu it opens is
    // there to copy or cut. Put it back afterwards.
    let selection_before_right_click = ui
        .input(|i| i.pointer.secondary_pressed())
        .then(|| egui::TextEdit::load_state(ui.ctx(), editor_id))
        .flatten()
        .and_then(|state| state.cursor.char_range())
        .filter(|range| !range.is_empty());
    let mut output = if settings.line_numbers {
        ui.horizontal_top(|ui| {
            // Reserve the column now; the numbers are painted once the editor
            // has laid out, from its galley's rows.
            let (gutter, _) = ui.allocate_exact_size(
                egui::vec2(line_number_gutter_width(code, settings), 0.0),
                egui::Sense::hover(),
            );
            let output = egui::TextEdit::multiline(code)
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
                .show(ui);
            draw_line_number_gutter(
                ui.painter(),
                gutter.right(),
                &output.galley,
                output.galley_pos,
                code,
                active_line,
                settings,
            );
            output
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

    if let Some(range) = selection_before_right_click.filter(|_| output.response.hovered()) {
        output.state.cursor.set_char_range(Some(range));
        output.state.clone().store(ui.ctx(), output.response.id);
        output.cursor_range = Some(range);
    }

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
    if let Some(choice) =
        editor_context_menu(&output.response, selection.is_some(), backdrop.is_some())
    {
        let moved = match choice {
            MenuChoice::PopOutBackdrop => {
                if let Some(backdrop) = backdrop {
                    widget_host.toggle_popped(backdrop);
                }
                None
            }
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
            // eframe answers with the clipboard as a paste event, which the
            // refocused editor takes like Ctrl+V: over the selection, if any.
            MenuChoice::Paste => {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                None
            }
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
        backdrop,
        widget_host,
        paint,
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
        paint.draw_theme,
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

/// Width of the line-number gutter: room for the widest number, two digits
/// at least.
fn line_number_gutter_width(code: &str, settings: &EditorSettings) -> f32 {
    let line_count = code.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let digits = line_count.to_string().len().max(2);
    digits as f32 * settings.font_size * 0.62 + 10.0
}

/// Number each logical line at the top of its first row in the editor's laid
/// out galley. Reading the galley's own rows, rather than summing a guessed row
/// height per line, keeps the numbers on their lines however the layouter
/// sized the rows — widget-inflated, pixel-rounded or soft-wrapped.
fn draw_line_number_gutter(
    painter: &egui::Painter,
    gutter_right: f32,
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    code: &str,
    active_line: Option<(usize, usize)>,
    settings: &EditorSettings,
) {
    let font_id = settings.font_id();
    let active_line_index = active_line.map(|(from, _)| {
        code[..from.min(code.len())]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
    });
    let palette = settings.theme.palette();
    let mut line = 0;
    let mut starts_line = true;
    for row in &galley.rows {
        if starts_line {
            let color = if Some(line) == active_line_index {
                palette.line_number_active
            } else {
                palette.line_number
            };
            painter.text(
                egui::pos2(gutter_right - 4.0, galley_pos.y + row.pos.y),
                egui::Align2::RIGHT_TOP,
                (line + 1).to_string(),
                font_id.clone(),
                color,
            );
            line += 1;
        }
        starts_line = row.ends_with_newline;
    }
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
    /// Where the gutter's numbers end, right-anchored against it.
    const GUTTER_RIGHT: f32 = 100.0;
    /// Where the galley is drawn; numbers are placed relative to it.
    const GALLEY_TOP: f32 = 10.0;

    type GutterShape = (String, egui::Pos2, egui::Color32);

    /// Every text run the gutter painted over `job`'s galley, as
    /// `(text, top-left, colour)`, with the right edge of the first one — the
    /// text is RIGHT-anchored, so the shape's own `pos` is its *left* edge —
    /// and the galley's row tops on screen.
    fn gutter_run(
        code: &str,
        job: egui::text::LayoutJob,
        active_line: Option<(usize, usize)>,
    ) -> (Vec<GutterShape>, f32, Vec<f32>) {
        let ctx = egui::Context::default();
        let settings = EditorSettings::default();
        // A real viewport, so nothing painted is clipped away.
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        // Fonts do not exist until a pass has run.
        let mut warmup = ctx.run_ui(input(), |_| {});
        warmup.textures_delta.clear();
        let row_tops = std::cell::RefCell::new(Vec::new());
        let mut out = ctx.run_ui(input(), |ui| {
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job.clone()));
            let galley_pos = egui::pos2(GUTTER_RIGHT, GALLEY_TOP);
            draw_line_number_gutter(
                ui.painter(),
                GUTTER_RIGHT,
                &galley,
                galley_pos,
                code,
                active_line,
                &settings,
            );
            *row_tops.borrow_mut() = galley.rows.iter().map(|r| GALLEY_TOP + r.pos.y).collect();
        });
        out.textures_delta.clear();
        let mut right = 0.0;
        let shapes: Vec<GutterShape> = out
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
                    text.galley.size().x,
                )),
                _ => None,
            })
            .enumerate()
            .map(|(i, (text, pos, color, width))| {
                if i == 0 {
                    right = pos.x + width;
                }
                (text, pos, color)
            })
            .collect();
        (shapes, right, row_tops.into_inner())
    }

    /// `code` laid out flat in the editor font, unwrapped.
    fn plain_job(code: &str) -> egui::text::LayoutJob {
        egui::text::LayoutJob::simple(
            code.to_string(),
            EditorSettings::default().font_id(),
            egui::Color32::WHITE,
            f32::INFINITY,
        )
    }

    fn gutter_shapes(code: &str, active_line: Option<(usize, usize)>) -> Vec<GutterShape> {
        gutter_run(code, plain_job(code), active_line).0
    }

    #[test]
    fn the_gutter_numbers_every_line_from_one() {
        let drawn = gutter_shapes("a\nb\nc", None);
        let labels: Vec<&str> = drawn.iter().map(|(t, _, _)| t.as_str()).collect();
        assert_eq!(labels, ["1", "2", "3"], "one number per line, 1-based");

        // A buffer with no newline is still one line, and a trailing newline
        // opens the next one.
        assert_eq!(gutter_shapes("a", None).len(), 1);
        assert_eq!(gutter_shapes("a\n", None).len(), 2);
    }

    #[test]
    fn the_active_line_number_is_the_one_the_cursor_is_on() {
        let palette = EditorSettings::default().theme.palette();
        // Cursor in the second line: byte 3 is its first character.
        // Lines longer than one character: with single-char lines, counting
        // the newlines before the cursor and counting everything else give the
        // same answer, and a wrong one passes.
        let drawn = gutter_shapes("ab\ncd\nef", Some((3, 3)));
        let active: Vec<&str> = drawn
            .iter()
            .filter(|(_, _, color)| *color == palette.line_number_active)
            .map(|(t, _, _)| t.as_str())
            .collect();
        assert_eq!(active, ["2"], "only the cursor's line is highlighted");

        // With no cursor, nothing is.
        let drawn = gutter_shapes("ab\ncd", None);
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
        // so a three-digit file needs one more digit of room.
        let settings = EditorSettings::default();
        let short = line_number_gutter_width("a\nb", &settings);
        let long = line_number_gutter_width(&"x\n".repeat(120), &settings);
        assert!(
            (long - short - settings.font_size * 0.62).abs() < 0.01,
            "one more digit of room, got {short} then {long}"
        );
        assert!(
            (short - (2.0 * settings.font_size * 0.62 + 10.0)).abs() < 0.01,
            "two digits at least, got {short}"
        );

        // ...and each number ends the same padding in from the gutter's edge.
        let (_, right, _) = gutter_run("a\nb", plain_job("a\nb"), None);
        assert!(
            (right - (GUTTER_RIGHT - 4.0)).abs() < 0.01,
            "right-aligned inside the gutter, got {right}"
        );
    }

    #[test]
    fn every_number_sits_on_its_line_however_tall_the_rows_are() {
        // The numbers come from the galley's rows, so they cannot drift from
        // the text: not over a long buffer, and not past a row the layouter
        // inflated for a block widget.
        let code = "x\n".repeat(60);
        let (drawn, _, rows) = gutter_run(&code, plain_job(&code), None);
        assert_eq!(drawn.len(), rows.len());
        for ((label, pos, _), top) in drawn.iter().zip(&rows) {
            assert_eq!(pos.y, *top, "line {label} is off its row");
        }

        let font_id = EditorSettings::default().font_id();
        let mut job = egui::text::LayoutJob::default();
        let mut tall = egui::TextFormat::simple(font_id.clone(), egui::Color32::WHITE);
        tall.line_height = Some(60.0);
        job.append("a\n", 0.0, tall);
        job.append(
            "b\nc",
            0.0,
            egui::TextFormat::simple(font_id, egui::Color32::WHITE),
        );
        let (drawn, _, rows) = gutter_run("a\nb\nc", job, None);
        assert!(rows[1] - rows[0] > 40.0, "the row really is inflated");
        let tops: Vec<f32> = drawn.iter().map(|(_, pos, _)| pos.y).collect();
        assert_eq!(tops, rows, "the numbers below it move down with it");
    }

    #[test]
    fn a_soft_wrapped_line_keeps_one_number() {
        // A line wrapped onto several rows is still one line: its number on
        // the first row, the next line's on the row after the last.
        let code = "aaaa aaaa aaaa aaaa\nb";
        let mut job = plain_job(code);
        job.wrap.max_width = 40.0;
        let (drawn, _, rows) = gutter_run(code, job, None);
        assert!(rows.len() > 2, "the first line wraps");
        let labels: Vec<&str> = drawn.iter().map(|(t, _, _)| t.as_str()).collect();
        assert_eq!(labels, ["1", "2"]);
        assert_eq!(drawn[0].1.y, rows[0]);
        assert_eq!(drawn[1].1.y, *rows.last().unwrap());
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
