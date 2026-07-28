//! Output-device setup and the lookahead scheduler.

use super::{
    DEFAULT_MASTER_VOLUME, MAX_MASTER_VOLUME, MixScratch, Mixer, load_f64, store_f64, write_frames,
};
use crate::{
    Clock, NoteEvent, SampleBank, ScopeTaps, collect_events_at, samples, sf2, soundfont,
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
        })
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
            for ev in collect_events_at(&pat, &clock_now, begin_cycle, target_cycle, &bank) {
                let _ = tx.send(ev);
            }
            scheduled_cycle = target_cycle;
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
