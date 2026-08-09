//! Output-device setup and the lookahead scheduler.

use super::{
    DEFAULT_MASTER_VOLUME, MAX_MASTER_VOLUME, MixScratch, Mixer, SharedCsound, load_f64, store_f64,
    write_frames,
};
use crate::{
    Clock, NoteEvent, SampleBank, ScopeTaps, collect_events_at,
    csound::Csound,
    samples, sf2, soundfont,
    sync::{lock_mutex, read_lock, write_lock},
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rudel_core::Pattern;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Sender},
    },
    thread::JoinHandle,
    time::Duration,
};
/// A running audio engine: owns the cpal stream and a scheduler thread.
pub struct Engine {
    _stream: cpal::Stream,
    pattern: Arc<RwLock<Pattern>>,
    /// Cycle/seconds mapping, re-anchored on every live cps change so the
    /// playhead is continuous across tempo changes (cyclist semantics).
    clock: Arc<Mutex<Clock>>,
    running: Arc<AtomicBool>,
    bank: Arc<RwLock<SampleBank>>,
    played: Arc<AtomicU64>,
    volume: Arc<AtomicU64>,
    sample_rate: f32,
    taps: Arc<ScopeTaps>,
    /// Csound, created the first time a script calls `loadCsound`/`loadOrc`.
    csound: SharedCsound,
}

impl Engine {
    /// Build the engine on the default output device and start its scheduler.
    pub fn new() -> Result<Engine, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default output device")?;
        let config = device
            .default_output_config()
            .map_err(|e| format!("default config: {e}"))?;
        let sample_rate = config.sample_rate() as f32;
        let channels = config.channels() as usize;
        let sample_format = config.sample_format();
        let stream_config = config.into();

        let (tx, rx) = mpsc::channel::<NoteEvent>();
        let played = Arc::new(AtomicU64::new(0));
        let pattern = Arc::new(RwLock::new(rudel_core::silence()));
        let clock = Arc::new(Mutex::new(Clock::new(0.5))); // Strudel default cps
        let running = Arc::new(AtomicBool::new(true));
        let bank = Arc::new(RwLock::new(SampleBank::new()));
        let volume = Arc::new(AtomicU64::new(0));
        store_f64(&volume, DEFAULT_MASTER_VOLUME);
        let taps = Arc::new(ScopeTaps::new());
        let csound = SharedCsound::default();

        let mut mixer = Mixer {
            rx,
            pending: Vec::new(),
            active: Vec::new(),
            sample_clock: 0,
            sample_rate,
            played: played.clone(),
            orbits: HashMap::new(),
            signal_buses: HashMap::new(),
            volume: volume.clone(),
            scratch: MixScratch::default(),
            taps: taps.clone(),
            tag_bufs: HashMap::new(),
            csound: csound.clone(),
        };

        let err_fn = |e| eprintln!("[rudel-audio] stream error: {e}");
        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                stream_config,
                move |data: &mut [f32], _| write_frames(data, channels, &mut mixer),
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                stream_config,
                move |data: &mut [i16], _| write_frames(data, channels, &mut mixer),
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                stream_config,
                move |data: &mut [u16], _| write_frames(data, channels, &mut mixer),
                err_fn,
                None,
            ),
            other => return Err(format!("unsupported sample format: {other:?}")),
        }
        .map_err(|e| format!("build stream: {e}"))?;

        stream.play().map_err(|e| format!("play: {e}"))?;

        // Scheduler thread.
        {
            let pattern = pattern.clone();
            let clock = clock.clone();
            let running = running.clone();
            let played = played.clone();
            let bank = bank.clone();
            std::thread::spawn(move || {
                scheduler_loop(pattern, clock, running, played, bank, tx, sample_rate)
            });
        }

        Ok(Engine {
            _stream: stream,
            pattern,
            clock,
            running,
            bank,
            played,
            volume,
            sample_rate,
            taps,
            csound,
        })
    }

    /// Compile Csound orchestra code, starting Csound on first use
    /// (`loadCsound`). `source` is either the code itself or a URL to fetch it
    /// from, which is what separates `loadCsound` from `loadOrc`; a `github:`
    /// pseudo-URL resolves the same way it does for sample packs.
    ///
    /// Runs on its own thread: opening `libcsound` and compiling a two thousand
    /// line orchestra both take long enough to drop frames from the UI.
    pub fn spawn_csound(&self, source: CsoundSource) -> JoinHandle<Result<usize, String>> {
        let csound = self.csound.clone();
        let sample_rate = self.sample_rate;
        std::thread::spawn(move || {
            // Fetched before the lock: the audio callback tries for it every
            // block, and a download can take seconds.
            let code = match source {
                CsoundSource::Code(code) => code,
                CsoundSource::Url(url) => samples::fetch_cached_text(&orc_url(&url))?,
            };
            let mut held = lock_mutex(&csound);
            if held.is_none() {
                *held = Some(Csound::new(sample_rate)?);
            }
            let instance = held.as_mut().expect("just created");
            if !code.trim().is_empty() {
                instance.compile_orc(&code)?;
            }
            Ok(1)
        })
    }

    /// Panic's Csound half: end any note still sounding. Without it a long
    /// `.csound(...)` note rings on after the rest of the app has gone quiet,
    /// since Csound renders its own notes and never sees the pattern stop.
    pub fn csound_all_notes_off(&self) {
        if let Some(instance) = lock_mutex(&self.csound).as_mut() {
            instance.all_notes_off();
        }
    }

    /// Load a directory of samples (subfolders become sound names).
    pub fn load_samples(&self, dir: impl AsRef<std::path::Path>) -> Result<usize, String> {
        let loaded = SampleBank::load_dir_entries(dir.as_ref())?;
        Ok(write_lock(&self.bank).extend_loaded(loaded))
    }

    /// The `samples(...)` loader: load from a `github:`/`bubo:` pseudo-URL, an
    /// http(s) URL to a `strudel.json`, a local `.json` map, or a local sample
    /// directory. Returns the number of samples registered.
    pub fn samples(&self, source: &str) -> Result<usize, String> {
        let loaded = SampleBank::load_samples_source_entries(source)?;
        Ok(write_lock(&self.bank).extend_loaded(loaded))
    }

    /// Load an inline Strudel-format sample map (`samples({...}, base)`). `base`
    /// resolves relative file paths. Returns the number of samples registered.
    pub fn load_sample_map(&self, json: &str, base: &str) -> Result<usize, String> {
        let loaded = SampleBank::load_sample_map_entries(json, base)?;
        Ok(write_lock(&self.bank).extend_loaded(loaded))
    }

    /// Start a background `samples(...)` load and merge the decoded samples into
    /// the bank when it completes.
    pub fn spawn_samples(&self, source: String) -> JoinHandle<Result<usize, String>> {
        let bank = self.bank.clone();
        std::thread::spawn(move || {
            let loaded = SampleBank::load_samples_source_entries(&source)?;
            Ok(write_lock(&bank).extend_loaded(loaded))
        })
    }

    /// Start a background *registration* of a sample source: fetch only the
    /// map and record what each sound's files are, leaving the audio to
    /// [`spawn_pending_sample`](Self::spawn_pending_sample) when it is first
    /// played.
    ///
    /// [`spawn_pending_sample`]: Engine::spawn_pending_sample
    pub fn spawn_register_samples(&self, source: String) -> JoinHandle<Result<usize, String>> {
        let bank = self.bank.clone();
        std::thread::spawn(move || {
            // Fetch the map outside the lock; only the registration takes it.
            let mut staging = SampleBank::new();
            let count = staging.register_samples_source(&source)?;
            let mut bank = write_lock(&bank);
            for name in staging.names() {
                if let Some(files) = staging.pending_files(&name) {
                    bank.register_pending(&name, files);
                }
            }
            Ok(count)
        })
    }

    /// Download the audio for one sound a registered map already knows about.
    pub fn spawn_pending_sample(&self, name: String) -> JoinHandle<Result<usize, String>> {
        let bank = self.bank.clone();
        std::thread::spawn(move || {
            // Read the file list under the lock, download outside it, merge
            // under it again: the audio thread reads this bank every block and
            // the fetch can take seconds.
            let Some(files) = read_lock(&bank).pending_files(&name) else {
                return Ok(0);
            };
            let loaded = SampleBank::fetch_pending_entries(&name, files);
            let mut bank = write_lock(&bank);
            let count = bank.extend_loaded(loaded);
            // Cleared even when every file failed, so a broken entry is not
            // retried on every event for the rest of the session.
            bank.clear_pending(&name);
            Ok(count)
        })
    }

    /// Start a background soundfont load: fetch the preset backing `(name, n)`,
    /// decode its zones and register it. HTTP responses go through the same
    /// on-disk cache as sample downloads, so a font is fetched once per machine.
    pub fn spawn_soundfont(&self, name: String, n: i64) -> JoinHandle<Result<usize, String>> {
        let bank = self.bank.clone();
        std::thread::spawn(move || {
            let preset = soundfont::load_gm_preset(
                &name,
                n,
                samples::fetch_cached_text,
                samples::decode_bytes,
            )?;
            let zones = preset.zones.len();
            write_lock(&bank).register_font(&name, n, preset);
            Ok(zones)
        })
    }

    /// Start a background SoundFont (`.sf2`) load: read the file, parse its
    /// presets and register each one under `name` at its own index, so `n`
    /// selects the preset. `path` may be a local path (with `~` expansion) or
    /// an http(s) URL, which is cached on disk like a sample pack.
    pub fn spawn_sf2(&self, path: String, name: String) -> JoinHandle<Result<usize, String>> {
        let bank = self.bank.clone();
        std::thread::spawn(move || {
            let bytes = samples::fetch_cached_bytes(&path)?;
            let presets = sf2::parse(&bytes)?.into_presets();
            let count = presets.len();
            let mut bank = write_lock(&bank);
            for (i, (_, preset)) in presets.into_iter().enumerate() {
                bank.register_font(&name, i as i64, preset);
            }
            Ok(count)
        })
    }

    /// Start a background `tables(...)` load: fetch and decode each `.wav` in
    /// the collection, slice it into `frame_len`-sample frames, and register the
    /// results as wavetable sounds. Returns how many tables were registered.
    pub fn spawn_tables(
        &self,
        source: String,
        frame_len: usize,
    ) -> JoinHandle<Result<usize, String>> {
        let bank = self.bank.clone();
        std::thread::spawn(move || {
            let tables = SampleBank::load_tables_entries(&source, frame_len)?;
            let count = tables.len();
            let mut bank = write_lock(&bank);
            for (name, table) in tables {
                bank.register_table(&name, table);
            }
            Ok(count)
        })
    }

    /// Start a background inline sample-map load.
    pub fn spawn_load_sample_map(
        &self,
        json: String,
        base: String,
    ) -> JoinHandle<Result<usize, String>> {
        let bank = self.bank.clone();
        std::thread::spawn(move || {
            let loaded = SampleBank::load_sample_map_entries(&json, &base)?;
            Ok(write_lock(&bank).extend_loaded(loaded))
        })
    }

    /// Register a bank alias (`aliasBank`): a pack loaded as `<canonical>_<s>`
    /// also resolves via `<alias>_<s>`.
    pub fn alias_bank(&self, canonical: &str, alias: &str) {
        write_lock(&self.bank).alias_bank(canonical, alias);
    }

    /// Register a single decoded sample under `name`.
    pub fn register_sample(&self, name: &str, sample: Arc<rudel_dsp::Sample>) {
        write_lock(&self.bank).register(name, sample);
    }

    /// Swap in a new pattern (live update).
    pub fn set_pattern(&self, pat: Pattern) {
        *write_lock(&self.pattern) = pat;
    }

    /// Set cycles per second (cps). `cpm`/`bpm` can be converted by the caller.
    /// Re-anchors the clock at the current playhead so the cycle position stays
    /// continuous across the change (cyclist's `setCps`); a no-op when the rate
    /// is unchanged.
    pub fn set_cps(&self, cps: f64) {
        let now = self.played.load(Ordering::Relaxed) as f64 / self.sample_rate as f64;
        lock_mutex(&self.clock).set_cps(now, cps);
    }

    /// Set the master audio output volume. `1.0` is unity; values above `1.0`
    /// boost the mixed output up to the VLC-style maximum of `2.0` (200%).
    pub fn set_volume(&self, volume: f64) {
        let volume = if volume.is_finite() {
            volume.clamp(0.0, MAX_MASTER_VOLUME)
        } else {
            DEFAULT_MASTER_VOLUME
        };
        store_f64(&self.volume, volume);
    }

    /// Current master audio output volume (`1.0` = 100%).
    pub fn volume(&self) -> f64 {
        load_f64(&self.volume)
    }

    /// The sample rate of the audio engine output.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// The scope taps (master mix + per-widget analyzer rings) feeding the
    /// scope/spectrum visualizers.
    pub fn scope_taps(&self) -> &ScopeTaps {
        &self.taps
    }

    /// Total elapsed cycles since the stream started (fractional). The visualizer
    /// uses `position_cycles().fract()` as the within-cycle playhead.
    pub fn position_cycles(&self) -> f64 {
        let seconds = self.played.load(Ordering::Relaxed) as f64 / self.sample_rate as f64;
        lock_mutex(&self.clock).cycle_at(seconds)
    }

    /// The sound names currently registered in the sample bank, sorted.
    pub fn sample_names(&self) -> Vec<String> {
        read_lock(&self.bank).names()
    }
}

/// Where a Csound orchestra comes from: the script's own text (`loadCsound`)
/// or a URL to fetch it from (`loadOrc`).
#[derive(Debug, Clone, PartialEq)]
pub enum CsoundSource {
    /// Orchestra code written inline in the script.
    Code(String),
    /// A URL, possibly a `github:user/repo/branch/path` pseudo-URL.
    Url(String),
}

/// Expand `loadOrc`'s `github:` pseudo-URL.
///
/// Deliberately *not* the sample loader's `github:` rule, which supplies a
/// default branch and appends a file name. `@strudel/csound` splits on the
/// prefix and pastes the rest straight onto raw.githubusercontent.com, so a
/// tune writes the branch itself — `github:kunstmusik/csound-live-code/master/
/// livecode.orc`. Reading it the sample way would look for `master` as a
/// sub-directory of the default branch.
fn orc_url(url: &str) -> String {
    match url.split_once("github:") {
        Some((_, path)) => format!("https://raw.githubusercontent.com/{path}"),
        None => url.to_string(),
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

/// Periodically queries the pattern and sends upcoming note events to the mixer.
#[allow(clippy::too_many_arguments)]
fn scheduler_loop(
    pattern: Arc<RwLock<Pattern>>,
    clock: Arc<Mutex<Clock>>,
    running: Arc<AtomicBool>,
    played: Arc<AtomicU64>,
    bank: Arc<RwLock<SampleBank>>,
    tx: Sender<NoteEvent>,
    sample_rate: f32,
) {
    let lookahead = 0.1_f64; // seconds scheduled ahead of the audio clock
    let mut scheduled_cycle = 0.0_f64;
    while running.load(Ordering::Relaxed) {
        // Snapshot the clock so the cycle window and the onset-seconds
        // conversion below use one consistent mapping even if cps changes.
        let clock_now = *lock_mutex(&clock);
        let now = played.load(Ordering::Relaxed) as f64 / sample_rate as f64;
        let current_cycle = clock_now.cycle_at(now);
        let target_cycle = clock_now.cycle_at(now + lookahead);
        if let Some((begin_cycle, target_cycle)) =
            next_schedule_window(scheduled_cycle, current_cycle, target_cycle)
        {
            let pat = read_lock(&pattern).clone();
            let bank = read_lock(&bank);
            let (events, cps_change) =
                collect_events_at(&pat, &clock_now, begin_cycle, target_cycle, &bank);
            for ev in events {
                let _ = tx.send(ev);
            }
            scheduled_cycle = target_cycle;

            // A hap's `cps` control retunes the transport (cyclist reads
            // `hap.value.cps` the same way). It takes effect from the end of
            // the window it appeared in: the events just sent keep the timing
            // they were given, and the new rate starts exactly where scheduling
            // resumes, so the cycle counter stays continuous. `set_cps` no-ops
            // on an unchanged or invalid rate.
            if let Some(cps) = cps_change {
                lock_mutex(&clock).set_cps(clock_now.seconds_at(target_cycle), cps);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Pick the cycle window `[begin, target)` to query next, given where we last
/// scheduled to (`scheduled_cycle`) and the current/lookahead cycle positions.
///
/// - cursor already past the window (e.g. a cps drop shrank the cycle
///   lookahead): schedule nothing and wait, so nothing is double-triggered;
/// - cursor behind the live window (the scheduler stalled): snap forward to
///   `current_cycle`, dropping the backlog rather than firing a burst of
///   late events;
/// - cursor inside the window: continue seamlessly from it.
pub(super) fn next_schedule_window(
    scheduled_cycle: f64,
    current_cycle: f64,
    target_cycle: f64,
) -> Option<(f64, f64)> {
    if !current_cycle.is_finite() || !target_cycle.is_finite() || target_cycle <= current_cycle {
        return None;
    }

    let begin_cycle = if scheduled_cycle.is_finite() {
        if scheduled_cycle > target_cycle {
            return None; // already scheduled past this window — wait for time to catch up
        }
        scheduled_cycle.max(current_cycle)
    } else {
        current_cycle
    };

    (target_cycle > begin_cycle).then_some((begin_cycle, target_cycle))
}
