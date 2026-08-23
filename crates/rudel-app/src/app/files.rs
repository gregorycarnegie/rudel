//! Open/Save for the editor buffer, and recording the output to disk.
//!
//! eframe's autosave already keeps the buffer across a close or a crash (see
//! `RudelApp::save`); this is the other half — naming a pattern and keeping it
//! somewhere the user chose.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use super::RudelApp;
use eframe::egui;
use std::path::{Path, PathBuf};

/// Patterns are written as Strudel JavaScript (the preprocessor turns them
/// into Koto), and the corpora they come from are directories of `.js`.
const EXTENSION: &str = "js";

/// What a recording is saved as when the name carries no extension of its own.
/// The others are reachable by typing one; see `rudel_audio::record::Format`.
const AUDIO_EXTENSION: &str = "wav";

// ponytail: `rfd`'s dialogs are modal and block the UI thread while they are
// open. Audio, MIDI and OSC all run on their own threads, so the sound keeps
// going; only repainting stops. Move to the async dialogs if that ever matters.
fn dialog(start: Option<&Path>, label: &str, extensions: &[&str]) -> rfd::FileDialog {
    let d = rfd::FileDialog::new()
        .add_filter(label, extensions)
        .add_filter("All files", &["*"]);
    // Reopen where the current file lives, not wherever the OS last was.
    match start.and_then(Path::parent) {
        Some(dir) => d.set_directory(dir),
        None => d,
    }
}

/// The pattern dialog: Open, Save and Save As.
fn pattern_dialog(start: Option<&Path>) -> rfd::FileDialog {
    dialog(start, "Pattern", &[EXTENSION])
}

impl RudelApp {
    /// Pick a file and load it into the editor.
    pub(super) fn open_file(&mut self) {
        let Some(path) = pattern_dialog(self.file_path.as_deref()).pick_file() else {
            return; // cancelled
        };
        self.load_path(&path);
    }

    /// Read `path` into the editor, taking it as the current file.
    pub(super) fn load_path(&mut self, path: &Path) {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                self.code = text;
                self.file_path = Some(path.to_path_buf());
                self.io_error = None;
                self.status = format!("opened {}", file_label(Some(path)));
            }
            Err(e) => self.io_error = Some(format!("open {}: {e}", path.display())),
        }
    }

    /// Write to the current file, falling back to Save As when there isn't one.
    pub(super) fn save_file(&mut self) {
        match self.file_path.clone() {
            Some(path) => self.write_to(&path),
            None => self.save_file_as(),
        }
    }

    /// Pick a destination and write to it.
    pub(super) fn save_file_as(&mut self) {
        let name = file_label(self.file_path.as_deref());
        let Some(path) = pattern_dialog(self.file_path.as_deref())
            .set_file_name(&name)
            .save_file()
        else {
            return; // cancelled
        };
        // A dialog that dropped the extension still means a pattern file.
        let path = match path.extension() {
            Some(_) => path,
            None => path.with_extension(EXTENSION),
        };
        self.write_to(&path);
    }

    fn write_to(&mut self, path: &Path) {
        match std::fs::write(path, &self.code) {
            Ok(()) => {
                self.file_path = Some(path.to_path_buf());
                self.io_error = None;
                self.status = format!("saved {}", file_label(Some(path)));
            }
            Err(e) => self.io_error = Some(format!("save {}: {e}", path.display())),
        }
    }

    /// Whether a recording is running.
    pub(super) fn is_recording(&self) -> bool {
        self.engine.as_ref().is_some_and(|e| e.is_recording())
    }

    /// Start a take (asking where to put it) or finish the running one.
    pub(super) fn toggle_recording(&mut self) {
        let Some(engine) = &self.engine else {
            self.io_error = Some("no audio device to record".to_string());
            return;
        };
        if engine.is_recording() {
            match engine.stop_recording() {
                Ok(Some(path)) => {
                    self.io_error = None;
                    self.status = format!("recorded {}", file_label(Some(&path)));
                }
                Ok(None) => {}
                Err(e) => self.io_error = Some(format!("recording: {e}")),
            }
            return;
        }
        // Name the take after the pattern, which is what it is a take of.
        let stem = self
            .file_path
            .as_deref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "rudel-take".to_string());
        let Some(path) = dialog(
            self.file_path.as_deref(),
            "Audio",
            &rudel_audio::record::Format::EXTENSIONS,
        )
        .set_file_name(format!("{stem}.{AUDIO_EXTENSION}"))
        .save_file() else {
            return; // cancelled
        };
        let path = match path.extension() {
            Some(_) => path,
            None => path.with_extension(AUDIO_EXTENSION),
        };
        match engine.start_recording(&path) {
            Ok(()) => {
                self.io_error = None;
                self.status = format!("recording to {}", file_label(Some(&path)));
            }
            Err(e) => self.io_error = Some(format!("recording: {e}")),
        }
    }

    /// Keep the window title showing which file is open. Only sent when it
    /// actually changes: a viewport command every frame asks for a repaint
    /// every frame, which spins the UI at full rate forever.
    pub(super) fn sync_window_title(&mut self, ctx: &egui::Context) {
        let title = match &self.file_path {
            Some(p) => format!("rudel — {}", file_label(Some(p))),
            None => "rudel".to_string(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }
}

/// The file name to show and to seed Save As with; `untitled.js` when there
/// is no file yet.
fn file_label(path: Option<&Path>) -> String {
    path.and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("untitled.{EXTENSION}"))
}

/// A stored path is only worth restoring if it is still readable.
pub(super) fn restore_path(stored: &str) -> Option<PathBuf> {
    let path = PathBuf::from(stored);
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_trip_through_a_real_file_preserves_the_buffer() {
        let dir = std::env::temp_dir().join(format!("rudel-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pattern.js");

        let mut app = RudelApp::headless();
        app.code = "s(\"bd sd\")".to_string();
        app.write_to(&path);
        assert_eq!(app.file_path.as_deref(), Some(path.as_path()));
        assert_eq!(app.io_error, None);

        // A different buffer, then read the file back over it.
        app.code = "silence()".to_string();
        app.load_path(&path);
        assert_eq!(app.code, "s(\"bd sd\")");
        assert!(app.status.contains("pattern.js"), "status: {}", app.status);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failed_read_reports_instead_of_clobbering_the_buffer() {
        let mut app = RudelApp::headless();
        app.code = "keep me".to_string();
        app.load_path(Path::new("no/such/rudel/pattern.js"));
        assert_eq!(app.code, "keep me");
        assert!(app.io_error.is_some(), "a missing file should report");
        assert_eq!(app.file_path, None, "and must not become the current file");
    }

    #[test]
    fn the_window_title_names_the_open_file() {
        let mut app = RudelApp::headless();
        assert_eq!(app.window_title, "", "nothing sent before the first frame");
        app.file_path = Some(PathBuf::from("/tmp/beat.js"));
        let ctx = egui::Context::default();
        app.sync_window_title(&ctx);
        assert_eq!(app.window_title, "rudel — beat.js");
    }

    #[test]
    fn the_save_as_name_falls_back_to_untitled() {
        assert_eq!(file_label(None), "untitled.js");
        assert_eq!(file_label(Some(Path::new("/tmp/beat.js"))), "beat.js");
    }

    #[test]
    fn a_stored_path_is_only_restored_when_it_still_exists() {
        assert_eq!(restore_path("no/such/rudel/pattern.js"), None);
        let this_file = concat!(env!("CARGO_MANIFEST_DIR"), "/src/app/files.rs");
        assert!(restore_path(this_file).is_some());
    }
}
