use super::{RudelApp, SampleJob};
use eframe::egui;
use std::time::Duration;

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
            let job = self.sample_jobs.swap_remove(i);
            match job.handle.join() {
                Ok(Ok(n)) => {
                    loaded += n;
                    finished += 1;
                }
                Ok(Err(e)) => {
                    self.loaded_sample_sources.remove(&job.key);
                    self.io_error = Some(format!("{}: {e}", job.label));
                    failed = true;
                    finished += 1;
                }
                Err(_) => {
                    self.loaded_sample_sources.remove(&job.key);
                    self.io_error = Some(format!("{}: loader thread panicked", job.label));
                    failed = true;
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
                self.status = "sample load failed".to_string();
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
            });
        }
    }

    fn queue_sample_source(&mut self, source: String) {
        if self.engine.is_none() {
            self.io_error = Some("no audio engine to load samples into".to_string());
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
            let name = requested.trim();
            rudel_midi::MidiIn::connect((!name.is_empty()).then_some(name))
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
