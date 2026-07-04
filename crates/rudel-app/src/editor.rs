use eframe::egui;
use std::collections::HashSet;

pub(crate) mod blocks;
mod brackets;
mod completion;
pub(crate) mod decorations;
mod edit;
mod highlight;
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
use settings::{EditorSettings, apply_editor_style};
use sliders::{SliderHostUpdate, SliderLayout, draw_slider_hosts};
use text::byte_index_at_char;
use widgets::{WidgetHostState, WidgetLayout, WidgetPaintInput, draw_widget_hosts};

const CODE_EDITOR_ID: &str = "rudel_code_editor";

#[derive(Default)]
pub(crate) struct EditorOutput {
    pub(crate) text_change: Option<TextChange>,
    pub(crate) slider_update: Option<SliderHostUpdate>,
    /// Cursor byte offset, as plain `usize` for the app layer (block eval);
    /// inside the editor module byte offsets are typed [`egui::text::ByteIndex`].
    pub(crate) cursor_byte: Option<usize>,
}

pub(crate) struct CodeEditorInput<'a> {
    pub(crate) active: &'a [(usize, usize)],
    pub(crate) idents: &'a HashSet<String>,
    pub(crate) reference: &'a rudel_lang::Reference,
    pub(crate) sample_names: &'a [String],
    pub(crate) current_pattern: Option<&'a rudel_core::Pattern>,
    pub(crate) playback_position_cycles: Option<f64>,
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
        playback_position_cycles,
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
    let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&editor_font, 'm'));
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
                char_width,
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
            draw_line_number_gutter(ui, code, active_line, settings);
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
                state.selected = (state.selected + 1) % state.items.len();
                handled = true;
            } else if shortcuts.complete_prev {
                state.selected = (state.selected + state.items.len() - 1) % state.items.len();
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
                        let selected = prev
                            .as_ref()
                            .filter(|c| c.start == start)
                            .map(|c| c.selected.min(items.len() - 1))
                            .unwrap_or(0);
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
            time_cycles: playback_position_cycles,
            draw_theme,
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
    }
}

fn draw_line_number_gutter(
    ui: &mut egui::Ui,
    code: &str,
    active_line: Option<(usize, usize)>,
    settings: &EditorSettings,
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
    ui.set_width(width);
    for line in 0..line_count {
        let color = if Some(line) == active_line_index {
            settings.theme.palette().line_number_active
        } else {
            settings.theme.palette().line_number
        };
        ui.add_sized(
            [width, settings.font_size * 1.35],
            egui::Label::new(
                egui::RichText::new(format!("{:>digits$}", line + 1))
                    .font(font_id.clone())
                    .color(color),
            ),
        );
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
