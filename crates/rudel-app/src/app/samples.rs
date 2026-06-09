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
