//! Open/Save for the editor buffer, and recording the output to disk.
//!
//! eframe's autosave already keeps the buffer across a close or a crash (see
//! `RudelApp::save`); this is the other half — naming a pattern and keeping it
//! somewhere the user chose.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use super::RudelApp;
use eframe::egui;
use std::path::{Path, PathBuf};

/// Patterns are written as Strudel JavaScript, and the corpora they come from
/// are directories of `.js`.
const EXTENSION: &str = "js";

/// What a recording is saved as when the name carries no extension of its own.
/// The others are reachable by typing one; see `rudel_audio::record::Format`.
const AUDIO_EXTENSION: &str = "wav";

// ponytail: `rfd`'s dialogs are modal and block the UI thread while they are
// open. Audio, MIDI and OSC all run on their own threads, so the sound keeps
// going; only repainting stops. Move to the async dialogs if that ever matters.
fn dialog(start: Option<&Path>, filters: &[(&str, &[&str])]) -> rfd::FileDialog {
    let mut d = rfd::FileDialog::new();
    for (label, extensions) in filters {
        d = d.add_filter(*label, extensions);
    }
    let d = d.add_filter("All files", &["*"]);
    // Reopen where the current file lives, not wherever the OS last was.
    match start.and_then(Path::parent) {
        Some(dir) => d.set_directory(dir),
        None => d,
    }
}

/// The pattern dialog: Open, Save and Save As.
fn pattern_dialog(start: Option<&Path>) -> rfd::FileDialog {
    dialog(start, &[("Pattern", &[EXTENSION])])
}

/// The recording dialog: every format together, then one filter each, so the
/// file-type list is what picks the format — choosing FLAC in it renames
/// `take.wav` to `take.flac`, and that extension is what the encoder follows.
/// Labels are derived from the extensions themselves, so a new format shows up
/// here by existing.
fn audio_dialog(start: Option<&Path>) -> rfd::FileDialog {
    let every: Vec<&str> = rudel_audio::record::Format::EXTENSIONS.to_vec();
    let labels: Vec<String> = every
        .iter()
        .map(|e| format!("{} audio", e.to_uppercase()))
        .collect();
    let mut filters: Vec<(&str, &[&str])> = vec![("Audio", &every)];
    filters.extend(
        labels
            .iter()
            .zip(&every)
            .map(|(label, ext)| (label.as_str(), std::slice::from_ref(ext))),
    );
    dialog(start, &filters)
}

/// The OS's modal dialogs, as a field rather than direct calls so a test can
/// answer them; the [`Default`] asks the user.
pub(super) struct Dialogs {
    pub(super) pick: Box<dyn Fn(rfd::FileDialog) -> Option<PathBuf>>,
    pub(super) save: Box<dyn Fn(rfd::FileDialog) -> Option<PathBuf>>,
    pub(super) confirm: Box<dyn Fn(rfd::MessageDialog) -> rfd::MessageDialogResult>,
}

impl Default for Dialogs {
    fn default() -> Self {
        Dialogs {
            pick: Box::new(|d| d.pick_file()),
            save: Box::new(|d| d.save_file()),
            confirm: Box::new(|d| d.show()),
        }
    }
}

impl RudelApp {
    /// Whether the buffer differs from what is on disk (or, with no file yet,
    /// from what it started as).
    pub(super) fn is_dirty(&self) -> bool {
        self.code != self.saved_code
    }

    /// Ask before throwing edits away. `what` completes "... anyway?".
    /// `true` means go ahead.
    pub(super) fn confirm_discard(&self, what: &str) -> bool {
        !self.is_dirty()
            || (self.dialogs.confirm)(
                rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Warning)
                    .set_title("Unsaved changes")
                    .set_description(format!(
                        "{} has changes that are not saved. {what} anyway?",
                        file_label(self.file_path.as_deref())
                    ))
                    .set_buttons(rfd::MessageButtons::YesNo),
            ) == rfd::MessageDialogResult::Yes
    }

    /// Pick a file and load it into the editor.
    pub(super) fn open_file(&mut self) {
        if !self.confirm_discard("Open another file") {
            return;
        }
        let Some(path) = (self.dialogs.pick)(pattern_dialog(self.file_path.as_deref())) else {
            return; // cancelled
        };
        self.load_path(&path);
    }

    /// Read `path` into the editor, taking it as the current file.
    pub(super) fn load_path(&mut self, path: &Path) {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                self.saved_code = text.clone();
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
        let Some(path) =
            (self.dialogs.save)(pattern_dialog(self.file_path.as_deref()).set_file_name(&name))
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
                self.saved_code = self.code.clone();
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
                    // A take the encoder could not keep up with has gaps in it;
                    // say so rather than let it look like a clean recording.
                    let dropped = engine.dropped_blocks();
                    self.io_error = None;
                    let name = file_label(Some(&path));
                    self.status = match dropped {
                        0 => format!("recorded {name}"),
                        n => format!("recorded {name} — {n} blocks dropped, the disk fell behind"),
                    };
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
        let dialog = audio_dialog(self.file_path.as_deref())
            .set_file_name(format!("{stem}.{AUDIO_EXTENSION}"));
        let Some(path) = (self.dialogs.save)(dialog) else {
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
        // A trailing `*` is the usual editor marker for a buffer that is not
        // on disk — the same state the Open/close warning asks about.
        let unsaved = if self.is_dirty() { " *" } else { "" };
        let title = match &self.file_path {
            Some(p) => format!("rudel — {}{unsaved}", file_label(Some(p))),
            None => format!("rudel{unsaved}"),
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
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn a_round_trip_through_a_real_file_preserves_the_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pattern.js");

        let mut app = RudelApp::headless();
        app.code = "s(\"bd sd\")".to_string();
        app.write_to(&path);
        assert_eq!(app.file_path.as_deref(), Some(path.as_path()));
        assert_eq!(app.io_error, None);

        // A different buffer, then read the file back over it.
        app.code = "silence".to_string();
        app.load_path(&path);
        assert_eq!(app.code, "s(\"bd sd\")");
        assert!(app.status.contains("pattern.js"), "status: {}", app.status);
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
    fn the_buffer_is_dirty_only_between_a_change_and_the_next_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pattern.js");

        let mut app = RudelApp::headless();
        assert!(!app.is_dirty(), "an untouched buffer is not dirty");
        app.code = "s(\"bd\")".to_string();
        assert!(app.is_dirty(), "a typed-in change is dirty");
        app.write_to(&path);
        assert!(!app.is_dirty(), "saving cleans it");
        app.code = "s(\"sd\")".to_string();
        app.load_path(&path);
        assert!(!app.is_dirty(), "opening a file cleans it");
        // A read that fails leaves the buffer — and so its dirtiness — alone.
        app.code = "s(\"hh\")".to_string();
        app.load_path(Path::new("no/such/rudel/pattern.js"));
        assert!(app.is_dirty(), "a failed open must not pretend it saved");
    }

    #[test]
    fn a_clean_buffer_needs_no_confirmation() {
        // The dialog itself is modal and cannot run headless; what is testable
        // is that a clean buffer never reaches it.
        let app = RudelApp::headless();
        assert!(app.confirm_discard("Quit"), "no dialog, straight through");
    }

    #[test]
    fn the_window_title_names_the_open_file() {
        let mut app = RudelApp::headless();
        assert_eq!(app.window_title, "", "nothing sent before the first frame");
        app.file_path = Some(PathBuf::from("/tmp/beat.js"));
        let ctx = egui::Context::default();
        app.sync_window_title(&ctx);
        assert_eq!(app.window_title, "rudel — beat.js");

        // ...and marks it while the buffer is not what the file holds.
        app.code = "s(\"bd\")".to_string();
        app.sync_window_title(&ctx);
        assert_eq!(app.window_title, "rudel — beat.js *");
        app.saved_code = app.code.clone();
        app.sync_window_title(&ctx);
        assert_eq!(app.window_title, "rudel — beat.js", "saving clears it");
    }

    #[test]
    fn the_save_as_name_falls_back_to_untitled() {
        assert_eq!(file_label(None), "untitled.js");
        assert_eq!(file_label(Some(Path::new("/tmp/beat.js"))), "beat.js");
    }

    /// Dialogs that answer `path` and `yes`, keeping a line per dialog shown.
    fn answering(app: &mut RudelApp, path: Option<PathBuf>, yes: bool) -> Rc<RefCell<Vec<String>>> {
        let shown = Rc::new(RefCell::new(Vec::new()));
        let (pick, save, confirm) = (shown.clone(), shown.clone(), shown.clone());
        let picked = path.clone();
        app.dialogs = Dialogs {
            pick: Box::new(move |d| {
                pick.borrow_mut().push(format!("pick {d:?}"));
                picked.clone()
            }),
            save: Box::new(move |d| {
                save.borrow_mut().push(format!("save {d:?}"));
                path.clone()
            }),
            confirm: Box::new(move |_| {
                confirm.borrow_mut().push("confirm".to_string());
                if yes {
                    rfd::MessageDialogResult::Yes
                } else {
                    rfd::MessageDialogResult::No
                }
            }),
        };
        shown
    }

    #[test]
    fn open_asks_before_discarding_edits_and_loads_what_was_picked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.js");
        std::fs::write(&path, "s(\"a\")").unwrap();
        let mut app = RudelApp::headless();
        app.code = "unsaved".to_string();

        let shown = answering(&mut app, Some(path.clone()), false);
        app.open_file();
        assert_eq!(app.code, "unsaved", "declined, so kept");
        assert_eq!(*shown.borrow(), ["confirm"], "and nothing picked");

        let shown = answering(&mut app, Some(path.clone()), true);
        app.open_file();
        assert_eq!(app.code, "s(\"a\")");
        let shown = shown.borrow();
        assert_eq!(shown[0], "confirm");
        assert!(
            shown[1].starts_with("pick") && shown[1].contains("\"Pattern\""),
            "{}",
            shown[1]
        );
    }

    #[test]
    fn save_asks_for_a_name_once_and_then_writes_where_it_was_told() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = RudelApp::headless();
        app.code = "s(\"bd\")".to_string();
        // A name typed without its extension still saves a pattern.
        let shown = answering(&mut app, Some(dir.path().join("beat")), true);
        app.save_file();
        let path = dir.path().join("beat.js");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "s(\"bd\")");
        assert!(
            shown.borrow()[0].contains("untitled.js"),
            "{}",
            shown.borrow()[0]
        );

        app.code = "s(\"sd\")".to_string();
        app.save_file();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "s(\"sd\")");
        assert_eq!(shown.borrow().len(), 1, "the file has a name now");

        // Save As starts where the file is, offering its name.
        app.save_file_as();
        let dialog = &shown.borrow()[1];
        let folder = format!("{:?}", dir.path());
        assert!(dialog.contains(&folder[1..folder.len() - 1]), "{dialog}");
        assert!(dialog.contains("beat.js"), "{dialog}");
    }

    #[test]
    fn a_take_records_to_the_file_asked_for_until_toggled_off() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = RudelApp::headless();
        let (engine, output) = rudel_audio::Engine::with_fake_output(48_000.0);
        app.engine = Some(engine);
        let shown = answering(&mut app, Some(dir.path().join("take")), true);
        app.toggle_recording();
        assert!(app.is_recording(), "{:?}", app.io_error);
        let dialog = &shown.borrow()[0];
        assert!(
            dialog.contains("rudel-take.wav") && dialog.contains("\"FLAC audio\""),
            "{dialog}"
        );
        output.pull(4_800);
        app.toggle_recording();
        assert!(!app.is_recording());
        assert_eq!(app.status, "recorded take.wav");
        assert!(dir.path().join("take.wav").exists());
    }

    #[test]
    fn a_stored_path_is_only_restored_when_it_still_exists() {
        assert_eq!(restore_path("no/such/rudel/pattern.js"), None);
        let this_file = concat!(env!("CARGO_MANIFEST_DIR"), "/src/app/files.rs");
        assert!(restore_path(this_file).is_some());
    }
}
