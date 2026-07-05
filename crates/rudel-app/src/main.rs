//! rudel-app - native live-coding editor for Rudel.
//! Type Koto in the editor, Ctrl+Enter to evaluate and hot-swap the pattern into
//! the running output (audio, MIDI or OSC). A reference pane lists sounds and
//! controls.
//! SPDX-License-Identifier: AGPL-3.0-or-later

mod app;
mod editor;
mod icon;
mod reference;
mod theme;
mod volume;

fn main() -> eframe::Result {
    app::run()
}
