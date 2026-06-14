use eframe::egui;
use std::collections::HashSet;

mod brackets;
mod completion;
mod edit;
mod highlight;
mod text;

use brackets::bracket_match_spans;
use completion::{Completion, apply_completion, completion_at, completion_popup};
use edit::{
    apply_editor_text_edits, capture_editor_shortcuts, editor_enter_pressed, editor_typed_text,
};
use highlight::highlighted_editor_job;
use text::byte_index_at_char;

const CODE_EDITOR_ID: &str = "rudel_code_editor";

pub(crate) fn code_editor(
    ui: &mut egui::Ui,
    code: &mut String,
    active: &[(usize, usize)],
    idents: &HashSet<String>,
) {
    let editor_id = ui.make_persistent_id(egui::Id::new(CODE_EDITOR_ID));
    let bracket_id = editor_id.with("bracket_match");
    let completion_id = editor_id.with("completion");

    // Completion popup state carried from last frame (empty items == inactive).
    let stored: Completion = ui.data(|d| d.get_temp(completion_id)).unwrap_or_default();
    let mut completion = (!stored.items.is_empty()).then_some(stored);

    let shortcuts = capture_editor_shortcuts(ui, editor_id, completion.is_some());
    let typed_text = editor_typed_text(ui);
    let enter_pressed = editor_enter_pressed(ui);
    // Bracket-match spans computed from last frame's cursor (the layouter runs
    // before this frame's cursor is known); recomputed and stored below.
    let brackets: Vec<(usize, usize)> = ui.data(|d| d.get_temp(bracket_id)).unwrap_or_default();
    let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        let job = highlighted_editor_job(text.as_str(), ui, wrap_width, active, &brackets, idents);
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let mut output = egui::TextEdit::multiline(code)
        .id_salt(CODE_EDITOR_ID)
        .code_editor()
        .layouter(&mut layouter)
        .desired_rows(28)
        .desired_width(f32::INFINITY)
        .show(ui);

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
            );
            cursor = edited.map(|r| r.primary.index).unwrap_or(cursor);
            if let Some(new_range) = edited {
                output.state.cursor.set_char_range(Some(new_range));
                output.state.clone().store(ui.ctx(), output.response.id);
            }
            // Open on typing, refresh while already open, otherwise close.
            let prev = completion.take();
            if typed_text.is_some() || prev.is_some() {
                let cursor_byte = byte_index_at_char(code, cursor);
                completion = completion_at(code, cursor_byte, idents).map(|(start, _, items)| {
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
                });
            }
        }

        if handled {
            ui.ctx().request_repaint();
        }

        // Refresh the bracket-match highlight for the (possibly moved) cursor.
        let new_brackets = bracket_match_spans(code, cursor)
            .map(|pair| pair.to_vec())
            .unwrap_or_default();
        if new_brackets != brackets {
            ui.data_mut(|d| d.insert_temp(bracket_id, new_brackets));
            ui.ctx().request_repaint();
        }
    } else {
        completion = None;
        if !brackets.is_empty() {
            ui.data_mut(|d| d.insert_temp(bracket_id, Vec::<(usize, usize)>::new()));
            ui.ctx().request_repaint();
        }
    }

    if let Some(state) = &completion {
        completion_popup(ui, completion_id, &output.response, state);
    }
    ui.data_mut(|d| d.insert_temp(completion_id, completion.unwrap_or_default()));
}
