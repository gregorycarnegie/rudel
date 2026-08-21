use super::{RudelApp, SampleJob};
use eframe::egui;
use std::time::Duration;

/// Which MIDI port `midin(device)` asked for: the named one, or `None` for the
/// system default when the script passed an empty name.
fn requested_port(device: &str) -> Option<&str> {
    let name = device.trim();
    (!name.is_empty()).then_some(name)
}

impl RudelApp {
    pub(super) fn poll_sample_jobs(&mut self, ctx: &egui::Context) {
        let mut finished = 0;
        let mut loaded = 0;
        let mut failed = false;
        let mut i = 0;
        while i < self.sample_jobs.len() {
            if !self.sample_jobs[i].handle.is_finished() {
                i += 1;
                continue;
            }
            // `join` consumes the handle, so read what the error path needs
            // off the job first.
            let SampleJob {
                key,
                label,
                handle,
                quiet,
            } = self.sample_jobs.swap_remove(i);
            match handle.join() {
                Ok(Ok(n)) => {
                    loaded += n;
                    finished += 1;
                }
                Ok(Err(e)) => {
                    self.loaded_sample_sources.remove(&key);
                    self.report_sample_failure(&label, quiet, &e);
                    failed |= !quiet;
                    finished += 1;
                }
                Err(_) => {
                    self.loaded_sample_sources.remove(&key);
                    self.report_sample_failure(&label, quiet, "loader thread panicked");
                    failed |= !quiet;
                    finished += 1;
                }
            }
        }

        if finished > 0 {
            if let Some(engine) = &self.engine {
                self.sample_names = engine.sample_names();
            }
            if loaded > 0 || !failed {
                self.status = format!(
                    "loaded {loaded} samples ({} sounds)",
                    self.sample_names.len()
                );
                if !failed {
                    self.io_error = None;
                }
            } else {
                // Not always samples: soundfonts, wavetables, midimaps and
                // Csound orchestras share this queue, and a failed `loadCsound`
                // reporting "sample load failed" sends the reader looking in
                // the wrong place. The detail is in the error panel below.
                self.status = "load failed".to_string();
            }
        }

        if !self.sample_jobs.is_empty() {
            self.status = format!("loading samples ({} job(s))", self.sample_jobs.len());
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// Fetch any soundfont a pattern asked for but that is not loaded yet.
    /// The scheduler records the miss (it can neither block nor spawn); this
    /// turns each one into a background job, the same way sample sources work.
    pub(super) fn poll_font_requests(&mut self) {
        let Some(engine) = &self.engine else {
            return;
        };
        for (name, n) in rudel_audio::take_font_requests() {
            if !self.requested_fonts.insert((name.clone(), n)) {
                continue;
            }
            let handle = engine.spawn_soundfont(name.clone(), n);
            self.sample_jobs.push(SampleJob {
                key: format!("soundfont:{name}:{n}"),
                label: format!("soundfont {name:?}"),
                handle,
                quiet: false,
            });
        }
    }

    /// Surface a failed sample job: the error bar for something the user asked
    /// for, the console for a startup bank, which is expected to fail offline.
    fn report_sample_failure(&mut self, label: &str, quiet: bool, error: &str) {
        let message = format!("{label}: {error}");
        if quiet {
            self.log_lines.push(message);
        } else {
            self.io_error = Some(message);
        }
    }

    /// Register the sample banks the Strudel REPL preloads, so a pattern naming
    /// `piano` or a drum machine has something to play without the user having
    /// to find and load a pack first.
    ///
    /// Only the maps are fetched — seven small JSON files — and the audio for a
    /// sound is downloaded the first time something plays it, which is what the
    /// browser does for Strudel. Failures are logged rather than raised: they
    /// are not something the user asked for, and rudel has to start offline.
    pub(super) fn prebake_default_samples(&mut self) {
        let Some(engine) = &self.engine else {
            return;
        };
        for source in rudel_audio::DEFAULT_SAMPLE_BANKS {
            let source = (*source).to_string();
            if !self.loaded_sample_sources.insert(source.clone()) {
                continue;
            }
            let handle = engine.spawn_register_samples(source.clone());
            self.sample_jobs.push(SampleJob {
                key: source.clone(),
                label: format!("samples({source:?})"),
                handle,
                quiet: true,
            });
        }
    }

    /// Download any sound a pattern played that a registered map knows but has
    /// not fetched yet. The bank records the miss (it is read from the audio
    /// thread, which can neither block nor spawn); this turns each one into a
    /// background job, exactly as `poll_font_requests` does for soundfonts.
    pub(super) fn poll_sample_requests(&mut self) {
        let Some(engine) = &self.engine else {
            return;
        };
        for name in rudel_audio::take_sample_requests() {
            let key = format!("pending:{name}");
            if !self.loaded_sample_sources.insert(key.clone()) {
                continue;
            }
            let handle = engine.spawn_pending_sample(name.clone());
            self.sample_jobs.push(SampleJob {
                key,
                label: format!("sample {name:?}"),
                handle,
                quiet: true,
            });
        }
    }

    fn queue_sample_source(&mut self, source: String) {
        self.queue_sample_source_quiet(source, false);
    }

    fn queue_sample_source_quiet(&mut self, source: String, quiet: bool) {
        if self.engine.is_none() {
            if !quiet {
                self.io_error = Some("no audio engine to load samples into".to_string());
            }
            return;
        }
        if !self.loaded_sample_sources.insert(source.clone()) {
            return;
        }
        let handle = self.engine.as_ref().unwrap().spawn_samples(source.clone());
        self.sample_jobs.push(SampleJob {
            key: source.clone(),
            label: format!("samples({source:?})"),
            handle,
            quiet,
        });
        self.status = format!("loading samples ({} job(s))", self.sample_jobs.len());
    }

    fn queue_sample_map(&mut self, json: String, base: String) {
        if self.engine.is_none() {
            self.io_error = Some("no audio engine to load samples into".to_string());
            return;
        }
        let key = format!("map:{base}\n{json}");
        if !self.loaded_sample_sources.insert(key.clone()) {
            return;
        }
        let handle = self
            .engine
            .as_ref()
            .unwrap()
            .spawn_load_sample_map(json, base);
        self.sample_jobs.push(SampleJob {
            key,
            label: "samples(map)".to_string(),
            handle,
            quiet: false,
        });
        self.status = format!("loading samples ({} job(s))", self.sample_jobs.len());
    }

    /// Apply `samples(...)` / `aliasBank(...)` requests from the script. Sample
    /// sources already loaded are skipped, so re-evaluation doesn't re-fetch.
    pub(super) fn apply_sample_effects(&mut self, effects: &rudel_lang::SampleEffects) {
        if let Some(cps) = effects.cps {
            self.set_cps(cps);
        }
        if let Some(engine) = &self.engine {
            for (canonical, alias) in &effects.bank_aliases {
                engine.alias_bank(canonical, alias);
            }
        }
        for source in &effects.sources {
            self.queue_sample_source(source.clone());
        }
        for (json, base) in &effects.maps {
            self.queue_sample_map(json.clone(), base.clone());
        }
        if let Some(url) = &effects.soundfont_url {
            rudel_audio::set_soundfont_url(url);
        }
        for (path, name) in &effects.soundfonts {
            self.queue_soundfont(path.clone(), name.clone());
        }
        for device in &effects.midi_inputs {
            self.queue_midi_input(device.clone());
        }
        for (source, frame_len) in &effects.tables {
            self.queue_tables(source.clone(), *frame_len);
        }
        for source in &effects.midimaps {
            self.queue_midimaps(source.clone());
        }
        for (is_url, text) in &effects.csound_orcs {
            self.queue_csound(*is_url, text.clone());
        }
    }

    /// Start Csound (on the first call) and compile an orchestra into it, once
    /// per distinct orchestra.
    ///
    /// Keyed on the text so a re-evaluation does not recompile — `loadOrc`
    /// caches by URL upstream for the same reason. A user editing the
    /// orchestra *does* change the text, so live-coding an instrument still
    /// works: that is the whole point of `loadCsound` being re-callable.
    fn queue_csound(&mut self, is_url: bool, text: String) {
        let Some(engine) = &self.engine else {
            self.io_error = Some("no audio engine to run Csound in".to_string());
            return;
        };
        let key = format!("csound:{is_url}:{text}");
        if !self.loaded_sample_sources.insert(key.clone()) {
            return;
        }
        let label = if is_url {
            format!("loadOrc({text:?})")
        } else {
            "loadCsound(...)".to_string()
        };
        let source = if is_url {
            rudel_audio::CsoundSource::Url(text)
        } else {
            rudel_audio::CsoundSource::Code(text)
        };
        let handle = engine.spawn_csound(source);
        self.sample_jobs.push(SampleJob {
            key,
            label,
            handle,
            quiet: false,
        });
    }

    /// Fetch a `midimaps(url)` control-to-CC table in the background, once per
    /// source. The registry it writes lives in `rudel-core`, so this needs no
    /// audio engine — a script can load a midimap with MIDI-only output.
    fn queue_midimaps(&mut self, source: String) {
        let key = format!("midimaps:{source}");
        if !self.loaded_sample_sources.insert(key.clone()) {
            return;
        }
        self.sample_jobs.push(SampleJob {
            key,
            label: format!("midimaps({source:?})"),
            handle: rudel_audio::spawn_midimaps(source),
            quiet: false,
        });
    }

    /// Load a wavetable collection in the background, once per
    /// `(source, frame length)` pair.
    fn queue_tables(&mut self, source: String, frame_len: usize) {
        let Some(engine) = &self.engine else {
            self.io_error = Some("no audio engine to load wavetables into".to_string());
            return;
        };
        let key = format!("tables:{source}:{frame_len}");
        if !self.loaded_sample_sources.insert(key.clone()) {
            return;
        }
        let handle = engine.spawn_tables(source.clone(), frame_len);
        self.sample_jobs.push(SampleJob {
            key,
            label: format!("tables({source:?})"),
            handle,
            quiet: false,
        });
    }

    /// Open the MIDI input port a `midin`/`midikeys` call named, once per name.
    /// The open can block while the OS MIDI subsystem starts, so it runs on a
    /// background thread like the UI-selected input; the factory the script
    /// already holds reads zero/no notes until the connection lands.
    fn queue_midi_input(&mut self, device: String) {
        if self.script_midi_ins.contains_key(&device)
            || self
                .script_midi_in_pending
                .iter()
                .any(|(name, _)| *name == device)
        {
            return;
        }
        let requested = device.clone();
        let handle = std::thread::spawn(move || {
            rudel_midi::MidiIn::connect(requested_port(&requested))
        });
        self.script_midi_in_pending.push((device, handle));
    }

    /// Adopt finished `midin`/`midikeys` port opens. Called each frame; returns
    /// `true` while any open is still in flight.
    pub(super) fn poll_script_midi_inputs(&mut self) -> bool {
        let mut i = 0;
        while i < self.script_midi_in_pending.len() {
            if !self.script_midi_in_pending[i].1.is_finished() {
                i += 1;
                continue;
            }
            let (device, handle) = self.script_midi_in_pending.swap_remove(i);
            match handle.join() {
                Ok(Ok(input)) => {
                    self.script_midi_ins.insert(device, input);
                }
                Ok(Err(e)) => self.io_error = Some(format!("midin({device:?}): {e}")),
                Err(_) => {
                    self.io_error = Some(format!("midin({device:?}): connect thread panicked"))
                }
            }
        }
        !self.script_midi_in_pending.is_empty()
    }

    /// Load a local `.sf2` file in the background, once per path.
    fn queue_soundfont(&mut self, path: String, name: String) {
        let Some(engine) = &self.engine else {
            self.io_error = Some("no audio engine to load a soundfont into".to_string());
            return;
        };
        let key = format!("sf2:{path}:{name}");
        if !self.loaded_sample_sources.insert(key.clone()) {
            return;
        }
        let handle = engine.spawn_sf2(path.clone(), name);
        self.sample_jobs.push(SampleJob {
            key,
            label: format!("loadSoundfont({path:?})"),
            handle,
            quiet: false,
        });
    }

    pub(super) fn load_samples(&mut self) {
        let source = self.sample_dir.trim().to_string();
        if source.is_empty() {
            self.io_error =
                Some("samples: enter a folder, strudel.json, URL, or github:user/repo".to_string());
            return;
        }
        // `samples()` accepts a local folder, a local strudel.json, an http(s)
        // URL, or a `github:`/`bubo:` pseudo-URL.
        self.queue_sample_source(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A job whose thread has already finished, so `poll_sample_jobs` sees it
    /// on the first pass.
    fn done(key: &str, quiet: bool, result: Result<usize, String>) -> SampleJob {
        let handle = std::thread::spawn(move || result);
        while !handle.is_finished() {
            std::thread::yield_now();
        }
        SampleJob {
            key: key.to_string(),
            label: format!("{key} label"),
            handle,
            quiet,
        }
    }

    fn app() -> RudelApp {
        RudelApp {
            status: String::new(),
            ..RudelApp::headless()
        }
    }

    #[test]
    fn a_loaded_job_reports_its_count_and_clears_the_error() {
        let mut app = app();
        app.io_error = Some("stale".to_string());
        app.sample_jobs.push(done("a", false, Ok(3)));
        app.sample_jobs.push(done("b", false, Ok(4)));
        app.poll_sample_jobs(&egui::Context::default());
        assert_eq!(app.status, "loaded 7 samples (0 sounds)");
        assert_eq!(app.io_error, None);
        assert!(app.sample_jobs.is_empty(), "finished jobs are taken off");
    }

    #[test]
    fn a_failed_job_raises_the_error_and_forgets_the_source() {
        let mut app = app();
        app.loaded_sample_sources.insert("a".to_string());
        app.sample_jobs
            .push(done("a", false, Err("404".to_string())));
        app.poll_sample_jobs(&egui::Context::default());
        assert_eq!(app.io_error.as_deref(), Some("a label: 404"));
        // Not "sample load failed": soundfonts, wavetables and Csound share
        // this queue.
        assert_eq!(app.status, "load failed");
        assert!(
            !app.loaded_sample_sources.contains("a"),
            "a failed source must be forgotten so a retry re-runs it"
        );
    }

    #[test]
    fn a_panicking_loader_is_reported_like_any_other_failure() {
        let mut app = app();
        let handle = std::thread::spawn(|| panic!("boom"));
        while !handle.is_finished() {
            std::thread::yield_now();
        }
        app.sample_jobs.push(SampleJob {
            key: "a".to_string(),
            label: "a label".to_string(),
            handle,
            quiet: false,
        });
        app.poll_sample_jobs(&egui::Context::default());
        assert_eq!(
            app.io_error.as_deref(),
            Some("a label: loader thread panicked")
        );
        assert_eq!(app.status, "load failed");
    }

    #[test]
    fn a_quiet_failure_goes_to_the_console_and_is_not_an_error() {
        // The startup banks: not something the user asked for, so a failure
        // must not open the error bar or turn the status red.
        let mut app = app();
        app.sample_jobs.push(done("a", true, Err("offline".to_string())));
        app.poll_sample_jobs(&egui::Context::default());
        assert_eq!(app.io_error, None);
        assert_eq!(app.log_lines, vec!["a label: offline".to_string()]);
        assert_eq!(app.status, "loaded 0 samples (0 sounds)");
    }

    #[test]
    fn a_failure_alongside_a_success_keeps_both() {
        // Something loaded, so the count is worth reporting — but the error
        // still stands and must not be cleared by the success.
        let mut app = app();
        app.sample_jobs.push(done("a", false, Ok(2)));
        app.sample_jobs
            .push(done("b", false, Err("404".to_string())));
        app.poll_sample_jobs(&egui::Context::default());
        assert_eq!(app.status, "loaded 2 samples (0 sounds)");
        assert_eq!(app.io_error.as_deref(), Some("b label: 404"));
    }

    #[test]
    fn an_unfinished_job_is_left_in_the_queue_and_shows_its_count() {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let mut app = app();
        app.sample_jobs.push(SampleJob {
            key: "slow".to_string(),
            label: "slow label".to_string(),
            handle: std::thread::spawn(move || {
                let _ = rx.recv();
                Ok(0)
            }),
            quiet: false,
        });
        app.sample_jobs.push(done("a", false, Ok(1)));
        app.poll_sample_jobs(&egui::Context::default());
        // The finished job is taken from the middle of the queue without
        // disturbing the one still running.
        assert_eq!(app.sample_jobs.len(), 1);
        assert_eq!(app.sample_jobs[0].key, "slow");
        assert_eq!(app.status, "loading samples (1 job(s))");
        drop(tx);
    }
    #[test]
    fn every_queue_says_what_is_missing_when_there_is_no_engine() {
        // Headless is the state a user hits when audio failed to start. Each
        // of these has to say which subsystem it wanted rather than dropping
        // the request silently.
        type Case = (&'static str, fn(&mut RudelApp));
        let cases: Vec<Case> = vec![
            ("no audio engine to load samples into", |a| {
                a.queue_sample_source("x".to_string())
            }),
            ("no audio engine to load samples into", |a| {
                a.queue_sample_map("{}".to_string(), "b".to_string())
            }),
            ("no audio engine to run Csound in", |a| {
                a.queue_csound(false, "instr 1
endin".to_string())
            }),
            ("no audio engine to load wavetables into", |a| {
                a.queue_tables("x".to_string(), 256)
            }),
            ("no audio engine to load a soundfont into", |a| {
                a.queue_soundfont("x.sf2".to_string(), "x".to_string())
            }),
        ];
        for (want, queue) in cases {
            let mut app = app();
            queue(&mut app);
            assert_eq!(app.io_error.as_deref(), Some(want));
            assert!(app.sample_jobs.is_empty(), "nothing to run the job on");
        }
    }

    #[test]
    fn a_quiet_sample_source_stays_quiet_without_an_engine() {
        // The startup banks queue quietly, so a missing engine is a console
        // matter at most — not seven red messages on first run.
        let mut app = app();
        app.queue_sample_source_quiet("x".to_string(), true);
        assert_eq!(app.io_error, None);
    }

    #[test]
    fn load_samples_asks_for_a_source_when_the_box_is_empty() {
        let mut app = app();
        app.sample_dir = "   ".to_string();
        app.load_samples();
        assert_eq!(
            app.io_error.as_deref(),
            Some("samples: enter a folder, strudel.json, URL, or github:user/repo")
        );
    }

    #[test]
    fn a_midimap_source_is_queued_once() {
        // No engine needed: midimaps are read by the app, not the audio thread.
        let mut app = app();
        app.queue_midimaps("bank".to_string());
        // Asserted after the *first* call: a guard that merely queues on some
        // other call than this one still ends up with one job.
        assert_eq!(app.sample_jobs.len(), 1, "the first call queues");
        app.queue_midimaps("bank".to_string());
        assert_eq!(app.sample_jobs.len(), 1, "the second call is a no-op");
        assert_eq!(app.sample_jobs[0].key, "midimaps:bank");
        assert_eq!(app.sample_jobs[0].label, "midimaps(\"bank\")");
        assert!(!app.sample_jobs[0].quiet);
    }

    #[test]
    fn a_midi_input_is_opened_once_per_device() {
        let mut app = app();
        app.queue_midi_input("port".to_string());
        assert_eq!(app.script_midi_in_pending.len(), 1, "the first call opens");
        app.queue_midi_input("port".to_string());
        assert_eq!(app.script_midi_in_pending.len(), 1, "the second is a no-op");
        assert_eq!(app.script_midi_in_pending[0].0, "port");
        // Polling drains it once the open has resolved — here to an error,
        // since there is no such port.
        while !app.script_midi_in_pending[0].1.is_finished() {
            std::thread::yield_now();
        }
        assert!(!app.poll_script_midi_inputs());
        assert!(app.script_midi_in_pending.is_empty());
    }

    #[test]
    fn a_failed_midi_open_is_reported_and_taken_off_the_queue() {
        let mut app = app();
        let handle = std::thread::spawn(|| Err("no such port".to_string()));
        while !handle.is_finished() {
            std::thread::yield_now();
        }
        app.script_midi_in_pending.push(("nope".to_string(), handle));
        assert!(
            !app.poll_script_midi_inputs(),
            "nothing is still in flight once the only open has finished"
        );
        assert_eq!(
            app.io_error.as_deref(),
            Some("midin(\"nope\"): no such port")
        );
        assert!(app.script_midi_ins.is_empty());
    }

    #[test]
    fn an_empty_device_name_means_the_default_port() {
        // `midin('')` and `midikeys()` take whatever port is there; a named
        // one has to be asked for by name, blanks and all trimmed off.
        assert_eq!(requested_port(""), None);
        assert_eq!(requested_port("   "), None);
        assert_eq!(requested_port(" IAC 1 "), Some("IAC 1"));
    }

    #[test]
    fn an_unfinished_midi_open_is_reported_as_still_in_flight() {
        // The caller keeps repainting while this is true, so an open that has
        // not resolved has to stay in the queue and keep saying so.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let mut app = app();
        app.script_midi_in_pending.push((
            "slow".to_string(),
            std::thread::spawn(move || {
                let _ = rx.recv();
                Err("cancelled".to_string())
            }),
        ));
        assert!(app.poll_script_midi_inputs());
        assert_eq!(app.script_midi_in_pending.len(), 1);
        assert_eq!(app.io_error, None, "nothing has failed yet");
        drop(tx);
    }

    #[test]
    fn a_panicking_midi_open_is_reported_too() {
        let mut app = app();
        let handle = std::thread::spawn(|| panic!("boom"));
        while !handle.is_finished() {
            std::thread::yield_now();
        }
        app.script_midi_in_pending.push(("nope".to_string(), handle));
        assert!(!app.poll_script_midi_inputs());
        assert_eq!(
            app.io_error.as_deref(),
            Some("midin(\"nope\"): connect thread panicked")
        );
    }
}
