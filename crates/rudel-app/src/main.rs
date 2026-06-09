//! rudel-app - native live-coding editor for Rudel.
//! Type Koto in the editor, Ctrl+Enter to evaluate and hot-swap the pattern into
//! the running output (audio, MIDI or OSC). The right panel visualizes one cycle
//! per orbit with a live playhead; a reference pane lists sounds and controls.
//! SPDX-License-Identifier: AGPL-3.0-or-later

mod app;
mod editor;
mod reference;
mod visualizer;
mod volume;

fn main() -> eframe::Result {
    app::run()
}
