//! Right-click menu for the code editor. Every entry is an action the editor
//! already has on a keyboard shortcut; the menu just makes them discoverable.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use super::edit::EditorShortcuts;
use eframe::egui;

/// An action the app layer runs, since it owns the engine, not the editor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EditorAction {
    Evaluate,
    EvaluateBlock,
    Hush,
    Panic,
}

pub(super) enum MenuChoice {
    App(EditorAction),
    /// A text edit, run through the same path as its keyboard shortcut.
    Edit(EditorShortcuts),
    Copy,
    Cut,
    Paste,
    SelectAll,
}

/// The clipboard text, or `None` when there is none (or no clipboard at all, as
/// on a headless test harness). egui only offers `copy_text`, so reading goes
/// straight to the platform.
pub(super) fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .ok()
        .filter(|text| !text.is_empty())
}

pub(super) fn editor_context_menu(
    response: &egui::Response,
    has_selection: bool,
) -> Option<MenuChoice> {
    let mut choice = None;
    response.context_menu(|ui| {
        let item = |ui: &mut egui::Ui, label: &str, shortcut: &str, enabled: bool| {
            let hit = ui
                .add_enabled(
                    enabled,
                    egui::Button::new(label)
                        .shortcut_text(shortcut)
                        .frame(false),
                )
                .clicked();
            if hit {
                ui.close();
            }
            hit
        };

        if item(ui, "Evaluate", "Ctrl+Enter", true) {
            choice = Some(MenuChoice::App(EditorAction::Evaluate));
        }
        if item(ui, "Evaluate block", "Ctrl+Shift+Enter", true) {
            choice = Some(MenuChoice::App(EditorAction::EvaluateBlock));
        }
        if item(ui, "Hush", "Ctrl+.", true) {
            choice = Some(MenuChoice::App(EditorAction::Hush));
        }
        if item(ui, "Panic", "Ctrl+Shift+.", true) {
            choice = Some(MenuChoice::App(EditorAction::Panic));
        }
        ui.separator();
        if item(ui, "Cut", "Ctrl+X", has_selection) {
            choice = Some(MenuChoice::Cut);
        }
        if item(ui, "Copy", "Ctrl+C", has_selection) {
            choice = Some(MenuChoice::Copy);
        }
        // Read once, on open, so the entry can grey out when there is nothing
        // to paste rather than being a no-op.
        if item(ui, "Paste", "Ctrl+V", clipboard_text().is_some()) {
            choice = Some(MenuChoice::Paste);
        }
        if item(ui, "Select all", "Ctrl+A", true) {
            choice = Some(MenuChoice::SelectAll);
        }
        ui.separator();
        if item(ui, "Toggle comment", "Ctrl+/", true) {
            choice = Some(MenuChoice::Edit(EditorShortcuts {
                comment_toggle: true,
                ..EditorShortcuts::default()
            }));
        }
        if item(ui, "Indent", "Tab", true) {
            choice = Some(MenuChoice::Edit(EditorShortcuts {
                indent: true,
                ..EditorShortcuts::default()
            }));
        }
        if item(ui, "Outdent", "Shift+Tab", true) {
            choice = Some(MenuChoice::Edit(EditorShortcuts {
                outdent: true,
                ..EditorShortcuts::default()
            }));
        }
    });
    choice
}
