//! rudel-audio - real-time clock, lookahead scheduler and cpal output.
//! The scheduler maps cycle time to the audio sample clock and feeds timed
//! note events to a mixer running in the audio callback.
//! Clock approach mirrors strudel/packages/core/{zyklus,cyclist}.mjs.
//! SPDX-License-Identifier: AGPL-3.0-or-later

#![warn(missing_docs)]

/// Cycle/seconds clock with cyclist-style cps re-anchoring.
pub mod clock;
/// Note event creation and scheduling logic.
pub mod events;
mod sample_map;
/// In-memory audio sample bank and decoding utilities.
pub mod samples;

pub use clock::Clock;
pub use events::{NoteEvent, collect_events, collect_events_at, to_control_map};
pub use samples::SampleBank;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender};
use fundsp::prelude32::{AudioUnit, reverb_stereo};
use rudel_core::Pattern;
use rudel_dsp::VoiceLike;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

/// A simple stereo feedback delay line for the `delay` send bus.
struct StereoDelay {
    /// Circular buffer for the left channel delay line.
    left: Vec<f32>,
    /// Circular buffer for the right channel delay line.
    right: Vec<f32>,
    /// Current circular buffer read/write index.
    idx: usize,
    /// Feedback amount (typically between 0.0 and 1.0).
    feedback: f32,
}

impl StereoDelay {
    /// Create a new `StereoDelay` configured for the target sample rate, delay time, and feedback level.
    fn new(sample_rate: f32, time: f32, feedback: f32) -> StereoDelay {
        let len = (sample_rate * time).max(1.0) as usize;
        StereoDelay {
            left: vec![0.0; len],
            right: vec![0.0; len],
            idx: 0,
            feedback,
        }
    }

    /// Process a single stereo input frame and return the delayed output frame.
    fn process(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let out_l = self.left[self.idx];
        let out_r = self.right[self.idx];
        self.left[self.idx] = in_l + out_l * self.feedback;
        self.right[self.idx] = in_r + out_r * self.feedback;
        self.idx = (self.idx + 1) % self.left.len();
        (out_l, out_r)
    }
}

/// Stores an `f64` value in an atomic variable by encoding it as binary bits.
fn store_f64(a: &AtomicU64, v: f64) {
    a.store(v.to_bits(), Ordering::Relaxed);
}
/// Loads an `f64` value from an atomic variable by decoding its binary bits.
fn load_f64(a: &AtomicU64) -> f64 {
    f64::from_bits(a.load(Ordering::Relaxed))
}

/// A playing voice plus its `cut` group and an optional choke ramp.
struct ActiveVoice {
    /// The actual synthesizer or sampler voice implementation.
    voice: Box<dyn VoiceLike>,
    /// Optional cut group (e.g. for choking open/closed hi-hats).
    cut: Option<i32>,
    /// When choked, the remaining gain (ramps 1.0 → 0.0 over `CHOKE_SECS`).
    /// `None` means the voice is playing normally.
    choke_gain: Option<f32>,
}

/// Fade time applied when a `cut`-group voice is choked (matches Strudel's 10ms).
const CHOKE_SECS: f32 = 0.01;
const DEFAULT_MASTER_VOLUME: f64 = 1.0;
const MAX_MASTER_VOLUME: f64 = 2.0;

/// Reusable per-block scratch buffers: one voice's rendered stereo block
/// (`src_*`) and the dry / reverb / delay accumulation buses. Grown to the
/// callback's block size on first use, then reused.
#[derive(Default)]
struct MixScratch {
    src_l: Vec<f32>,
    src_r: Vec<f32>,
    dry_l: Vec<f32>,
    dry_r: Vec<f32>,
    room_l: Vec<f32>,
    room_r: Vec<f32>,
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
}

impl MixScratch {
    /// Ensure every buffer holds at least `n` samples.
    fn ensure(&mut self, n: usize) {
        for b in [
            &mut self.src_l,
            &mut self.src_r,
            &mut self.dry_l,
            &mut self.dry_r,
            &mut self.room_l,
            &mut self.room_r,
            &mut self.delay_l,
            &mut self.delay_r,
        ] {
            if b.len() < n {
                b.resize(n, 0.0);
            }
        }
    }
}

/// Mixes active voices and starts new ones as their onset time arrives. Lives
/// in the audio callback.
struct Mixer {
    /// Channel receiver for note events from the scheduler thread.
    rx: Receiver<NoteEvent>,
    /// List of note events scheduled in the future.
    pending: Vec<NoteEvent>,
    /// List of voices currently rendering audio.
    active: Vec<ActiveVoice>,
    /// Elapsed sample clock since the audio engine started.
    sample_clock: u64,
    /// The output device sample rate.
    sample_rate: f32,
    /// Atomic tracking of played frames, shared with the scheduling thread.
    played: Arc<AtomicU64>,
    /// The global stereo delay line.
    delay: StereoDelay,
    /// The global reverb effect unit.
    reverb: Box<dyn AudioUnit>,
    /// Master output volume, shared with the UI/control thread.
    volume: Arc<AtomicU64>,
    /// Reusable per-block render/accumulation buffers.
    scratch: MixScratch,
}

impl Mixer {
    /// Render a single stereo frame (a one-frame [`render_block`](Self::render_block)).
    fn render_frame(&mut self) -> (f32, f32) {
        let mut out = [(0.0f32, 0.0f32)];
        self.render_block(&mut out);
        out[0]
    }

    /// Render `out.len()` stereo frames. The callback buffer is split into
    /// sub-blocks at voice-onset boundaries so onsets stay sample-accurate;
    /// within each sub-block no voice starts, so all active voices render a
    /// whole block at once via [`VoiceLike::process_block`].
    fn render_block(&mut self, out: &mut [(f32, f32)]) {
        while let Ok(ev) = self.rx.try_recv() {
            self.pending.push(ev);
        }
        let sr = self.sample_rate as f64;
        let total = out.len();
        let mut offset = 0;
        while offset < total {
            let now = self.sample_clock as f64 / sr;
            self.start_due_events(now);
            // Run until the next not-yet-started onset (or the end of the buffer).
            let next_onset_clock = self
                .pending
                .iter()
                .map(|ev| (ev.onset_seconds * sr).ceil() as u64)
                .filter(|&c| c > self.sample_clock)
                .min();
            let remaining = total - offset;
            let sub_len = match next_onset_clock {
                Some(c) => ((c - self.sample_clock) as usize).min(remaining).max(1),
                None => remaining,
            };
            self.mix_sub_block(&mut out[offset..offset + sub_len]);
            offset += sub_len;
        }
        self.played.store(self.sample_clock, Ordering::Relaxed);
    }

    /// Start every pending event whose onset has arrived by `now`, choking any
    /// same-`cut`-group voice (last-one-wins, like Strudel's cut groups).
    fn start_due_events(&mut self, now: f64) {
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].onset_seconds <= now {
                let ev = self.pending.swap_remove(i);
                if let Some(g) = ev.cut {
                    for av in &mut self.active {
                        if av.cut == Some(g) && av.choke_gain.is_none() {
                            av.choke_gain = Some(1.0);
                        }
                    }
                }
                self.active.push(ActiveVoice {
                    voice: ev.spec.into_voice_with_fx(self.sample_rate, ev.fx),
                    cut: ev.cut,
                    choke_gain: None,
                });
            } else {
                i += 1;
            }
        }
    }

    /// Mix all active voices over `out` (no new voices start within it): render
    /// each voice's block, accumulate the dry / reverb / delay buses, then apply
    /// the global delay + reverb per sample and write the master mix.
    fn mix_sub_block(&mut self, out: &mut [(f32, f32)]) {
        let len = out.len();
        let volume = load_f64(&self.volume) as f32;
        let choke_step = 1.0 / (self.sample_rate * CHOKE_SECS);
        self.scratch.ensure(len);

        let Mixer {
            active,
            scratch,
            delay,
            reverb,
            sample_clock,
            ..
        } = self;
        let MixScratch {
            src_l,
            src_r,
            dry_l,
            dry_r,
            room_l,
            room_r,
            delay_l,
            delay_r,
        } = scratch;
        for b in [
            &mut *dry_l,
            dry_r,
            room_l,
            room_r,
            delay_l,
            delay_r,
        ] {
            b[..len].fill(0.0);
        }

        active.retain_mut(|av| {
            av.voice.process_block(&mut src_l[..len], &mut src_r[..len]);
            // `dry` scales the direct signal; the reverb/delay sends are taken
            // pre-dry, so `dry(0)` leaves only the wet signal.
            let dry = av.voice.dry();
            let room = av.voice.room();
            let dsend = av.voice.delay_send();
            if let Some(g) = &mut av.choke_gain {
                // Choked voices fade per sample; drop the voice once silent.
                let mut gain = *g;
                for i in 0..len {
                    let (a, b) = (src_l[i] * gain, src_r[i] * gain);
                    dry_l[i] += a * dry;
                    dry_r[i] += b * dry;
                    if room > 0.0 {
                        room_l[i] += a * room;
                        room_r[i] += b * room;
                    }
                    if dsend > 0.0 {
                        delay_l[i] += a * dsend;
                        delay_r[i] += b * dsend;
                    }
                    gain -= choke_step;
                    if gain <= 0.0 {
                        return false; // fully faded — drop the voice
                    }
                }
                *g = gain;
            } else {
                for i in 0..len {
                    dry_l[i] += src_l[i] * dry;
                    dry_r[i] += src_r[i] * dry;
                }
                if room > 0.0 {
                    for i in 0..len {
                        room_l[i] += src_l[i] * room;
                        room_r[i] += src_r[i] * room;
                    }
                }
                if dsend > 0.0 {
                    for i in 0..len {
                        delay_l[i] += src_l[i] * dsend;
                        delay_r[i] += src_r[i] * dsend;
                    }
                }
            }
            !av.voice.is_done()
        });

        for (i, frame) in out.iter_mut().enumerate() {
            let (dl_out, dr_out) = delay.process(delay_l[i], delay_r[i]);
            let mut rout = [0.0f32; 2];
            reverb.tick(&[room_l[i], room_r[i]], &mut rout);
            *frame = (
                (dry_l[i] + dl_out + rout[0]) * volume,
                (dry_r[i] + dr_out + rout[1]) * volume,
            );
        }
        *sample_clock += len as u64;
    }
}

/// A headless [`Mixer`] with no audio device, for offline rendering and
/// benchmarks. Schedule [`NoteEvent`]s, then pull frames or blocks.
#[doc(hidden)]
pub struct OfflineMixer {
    tx: Sender<NoteEvent>,
    mixer: Mixer,
}

impl OfflineMixer {
    /// Build an offline mixer at the given sample rate (global reverb + delay
    /// configured exactly as the real engine).
    pub fn new(sample_rate: f32) -> OfflineMixer {
        let (tx, rx) = crossbeam_channel::unbounded::<NoteEvent>();
        let volume = Arc::new(AtomicU64::new(0));
        store_f64(&volume, DEFAULT_MASTER_VOLUME);
        let mixer = Mixer {
            rx,
            pending: Vec::new(),
            active: Vec::new(),
            sample_clock: 0,
            sample_rate,
            played: Arc::new(AtomicU64::new(0)),
            delay: StereoDelay::new(sample_rate, 1.0 / 6.0, 0.4),
            reverb: build_reverb(sample_rate),
            volume,
            scratch: MixScratch::default(),
        };
        OfflineMixer { tx, mixer }
    }

    /// Queue a note event (delivered on the next render call).
    pub fn schedule(&self, ev: NoteEvent) {
        let _ = self.tx.send(ev);
    }

    /// Render one stereo frame.
    pub fn render_frame(&mut self) -> (f32, f32) {
        self.mixer.render_frame()
    }

    /// Render `out.len()` stereo frames into `out`.
    pub fn render_block(&mut self, out: &mut [(f32, f32)]) {
        self.mixer.render_block(out);
    }

    /// Number of currently active voices.
    pub fn active_len(&self) -> usize {
        self.mixer.active.len()
    }
}

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

        let (tx, rx) = crossbeam_channel::unbounded::<NoteEvent>();
        let played = Arc::new(AtomicU64::new(0));
        let pattern = Arc::new(RwLock::new(rudel_core::silence()));
        let clock = Arc::new(Mutex::new(Clock::new(0.5))); // Strudel default cps
        let running = Arc::new(AtomicBool::new(true));
        let bank = Arc::new(RwLock::new(SampleBank::new()));
        let volume = Arc::new(AtomicU64::new(0));
        store_f64(&volume, DEFAULT_MASTER_VOLUME);

        let mut mixer = Mixer {
            rx,
            pending: Vec::new(),
            active: Vec::new(),
            sample_clock: 0,
            sample_rate,
            played: played.clone(),
            delay: StereoDelay::new(sample_rate, 1.0 / 6.0, 0.4),
            reverb: build_reverb(sample_rate),
            volume: volume.clone(),
            scratch: MixScratch::default(),
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
        })
    }

    /// Load a directory of samples (subfolders become sound names).
    pub fn load_samples(&self, dir: impl AsRef<std::path::Path>) -> Result<usize, String> {
        let loaded = SampleBank::load_dir_entries(dir.as_ref())?;
        Ok(self.bank.write().unwrap().extend_loaded(loaded))
    }

    /// The `samples(...)` loader: load from a `github:`/`bubo:` pseudo-URL, an
    /// http(s) URL to a `strudel.json`, a local `.json` map, or a local sample
    /// directory. Returns the number of samples registered.
    pub fn samples(&self, source: &str) -> Result<usize, String> {
        let loaded = SampleBank::load_samples_source_entries(source)?;
        Ok(self.bank.write().unwrap().extend_loaded(loaded))
    }

    /// Load an inline Strudel-format sample map (`samples({...}, base)`). `base`
    /// resolves relative file paths. Returns the number of samples registered.
    pub fn load_sample_map(&self, json: &str, base: &str) -> Result<usize, String> {
        let loaded = SampleBank::load_sample_map_entries(json, base)?;
        Ok(self.bank.write().unwrap().extend_loaded(loaded))
    }

    /// Start a background `samples(...)` load and merge the decoded samples into
    /// the bank when it completes.
    pub fn spawn_samples(&self, source: String) -> JoinHandle<Result<usize, String>> {
        let bank = self.bank.clone();
        std::thread::spawn(move || {
            let loaded = SampleBank::load_samples_source_entries(&source)?;
            Ok(bank.write().unwrap().extend_loaded(loaded))
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
            Ok(bank.write().unwrap().extend_loaded(loaded))
        })
    }

    /// Register a bank alias (`aliasBank`): a pack loaded as `<canonical>_<s>`
    /// also resolves via `<alias>_<s>`.
    pub fn alias_bank(&self, canonical: &str, alias: &str) {
        self.bank.write().unwrap().alias_bank(canonical, alias);
    }

    /// Register a single decoded sample under `name`.
    pub fn register_sample(&self, name: &str, sample: std::sync::Arc<rudel_dsp::Sample>) {
        self.bank.write().unwrap().register(name, sample);
    }

    /// Swap in a new pattern (live update).
    pub fn set_pattern(&self, pat: Pattern) {
        *self.pattern.write().unwrap() = pat;
    }

    /// Set cycles per second (cps). `cpm`/`bpm` can be converted by the caller.
    /// Re-anchors the clock at the current playhead so the cycle position stays
    /// continuous across the change (cyclist's `setCps`); a no-op when the rate
    /// is unchanged.
    pub fn set_cps(&self, cps: f64) {
        let now = self.played.load(Ordering::Relaxed) as f64 / self.sample_rate as f64;
        self.clock.lock().unwrap().set_cps(now, cps);
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

    /// Total elapsed cycles since the stream started (fractional). The visualizer
    /// uses `position_cycles().fract()` as the within-cycle playhead.
    pub fn position_cycles(&self) -> f64 {
        let seconds = self.played.load(Ordering::Relaxed) as f64 / self.sample_rate as f64;
        self.clock.lock().unwrap().cycle_at(seconds)
    }

    /// The sound names currently registered in the sample bank, sorted.
    pub fn sample_names(&self) -> Vec<String> {
        self.bank.read().unwrap().names()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

/// Build the global FDN reverb (fundsp), configured for the sample rate.
fn build_reverb(sample_rate: f32) -> Box<dyn AudioUnit> {
    // room size 10m, ~1.5s tail, moderate damping
    let mut unit = Box::new(reverb_stereo(10.0, 1.5, 0.5));
    unit.set_sample_rate(sample_rate as f64);
    unit
}

/// Writes rendered mixer output frames into a target slice buffer for cpal playback.
fn write_frames<T>(data: &mut [T], channels: usize, mixer: &mut Mixer)
where
    T: cpal::Sample + cpal::FromSample<f32>,
{
    for frame in data.chunks_mut(channels.max(1)) {
        let (l, r) = mixer.render_frame();
        match frame {
            [] => {}
            [mono] => *mono = T::from_sample((l + r) * 0.5),
            [left, right, rest @ ..] => {
                *left = T::from_sample(l);
                *right = T::from_sample(r);
                for s in rest {
                    *s = T::from_sample((l + r) * 0.5);
                }
            }
        }
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
        let clock_now = *clock.lock().unwrap();
        let now = played.load(Ordering::Relaxed) as f64 / sample_rate as f64;
        let current_cycle = clock_now.cycle_at(now);
        let target_cycle = clock_now.cycle_at(now + lookahead);
        if let Some((begin_cycle, target_cycle)) =
            next_schedule_window(scheduled_cycle, current_cycle, target_cycle)
        {
            let pat = pattern.read().unwrap().clone();
            let bank = bank.read().unwrap();
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
fn next_schedule_window(
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_volume(value: f64) -> Arc<AtomicU64> {
        let volume = Arc::new(AtomicU64::new(0));
        store_f64(&volume, value);
        volume
    }

    fn test_mixer(rx: crossbeam_channel::Receiver<NoteEvent>) -> Mixer {
        test_mixer_with_volume(rx, test_volume(DEFAULT_MASTER_VOLUME))
    }

    fn test_mixer_with_volume(
        rx: crossbeam_channel::Receiver<NoteEvent>,
        volume: Arc<AtomicU64>,
    ) -> Mixer {
        Mixer {
            rx,
            pending: Vec::new(),
            active: Vec::new(),
            sample_clock: 0,
            sample_rate: 44100.0,
            played: Arc::new(AtomicU64::new(0)),
            delay: StereoDelay::new(44100.0, 1.0 / 6.0, 0.4),
            reverb: build_reverb(44100.0),
            volume,
            scratch: MixScratch::default(),
        }
    }

    #[test]
    fn stereo_delay_echoes_after_its_time() {
        let mut d = StereoDelay::new(1000.0, 0.01, 0.5); // 10-sample delay
        let (o0, _) = d.process(1.0, 0.0); // impulse in
        assert_eq!(o0, 0.0, "no output before the delay time");
        let mut max_echo = 0.0f32;
        for _ in 0..20 {
            max_echo = max_echo.max(d.process(0.0, 0.0).0);
        }
        assert!(
            max_echo > 0.0,
            "impulse should re-emerge after the delay time"
        );
    }

    #[test]
    fn reverb_send_produces_a_tail() {
        let (tx, rx) = crossbeam_channel::unbounded::<NoteEvent>();
        let mut mixer = test_mixer(rx);
        // a short note with a big reverb send
        let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69))).room(1.0);
        for ev in collect_events(&pat, 4.0, 0.0, 1.0, &SampleBank::new()) {
            tx.send(ev).unwrap();
        }
        drop(tx);

        // play past the (short) note, then measure the tail afterwards
        for _ in 0..6000 {
            mixer.render_frame();
        }
        let mut tail = 0.0f32;
        for _ in 0..4000 {
            tail += mixer.render_frame().0.abs();
        }
        assert!(tail > 0.0, "reverb should ring out after the note ends");
    }

    #[test]
    fn cut_group_chokes_the_previous_voice() {
        // Two sustained notes in cut group 1, the second a little later. After
        // the second starts, the first should be choked to silence within the
        // ~10ms fade, leaving only one voice's worth of energy.
        let (tx, rx) = crossbeam_channel::unbounded::<NoteEvent>();
        let mut mixer = test_mixer(rx);
        // A long held saw so the voice is still audible when the next one cuts it.
        let held = |onset: f64| NoteEvent {
            onset_seconds: onset,
            spec: rudel_dsp::VoiceSpec::Synth(Box::new(rudel_dsp::VoiceParams::from_controls(
                &rudel_core::to_control_map(&rudel_core::Value::Str("sawtooth".into())),
                10.0,
            ))),
            fx: rudel_dsp::PostFx::default(),
            cut: Some(1),
        };
        tx.send(held(0.0)).unwrap();
        tx.send(held(0.2)).unwrap();
        drop(tx);

        // Render up to just before the second onset: only voice A is active.
        for _ in 0..((0.2 * 44100.0) as usize) {
            mixer.render_frame();
        }
        assert_eq!(mixer.active.len(), 1);
        // Render past the choke fade (~10ms). The choked first voice is dropped,
        // leaving just the second voice.
        for _ in 0..((CHOKE_SECS * 44100.0) as usize + 64) {
            mixer.render_frame();
        }
        assert_eq!(mixer.active.len(), 1, "the choked voice should be gone");
        assert!(
            mixer.active[0].choke_gain.is_none(),
            "the surviving voice is the new one, not choking"
        );
    }

    #[test]
    fn block_render_matches_frame_render_across_onsets() {
        // The sub-block splitting in `render_block` must be sample-for-sample
        // equivalent to stepping `render_frame`, including onsets that land
        // partway through a buffer. Drive two identical mixers with the same
        // staggered notes — one in a single 256-frame block, one frame by frame —
        // and confirm they agree. The notes are plain synths (no post-fx), so the
        // default `process_block` is a `tick` loop and the two paths are exact.
        let note = |onset: f64| NoteEvent {
            onset_seconds: onset,
            spec: rudel_dsp::VoiceSpec::Synth(Box::new(rudel_dsp::VoiceParams::from_controls(
                &rudel_core::to_control_map(&rudel_core::Value::Str("sawtooth".into())),
                10.0,
            ))),
            fx: rudel_dsp::PostFx::default(),
            cut: None,
        };
        // Onsets at frames 0, ~37 and ~150 (44.1kHz) force mid-buffer splits.
        let onsets = [0.0, 37.0 / 44100.0, 150.0 / 44100.0];

        let (tx_a, rx_a) = crossbeam_channel::unbounded::<NoteEvent>();
        let (tx_b, rx_b) = crossbeam_channel::unbounded::<NoteEvent>();
        for &o in &onsets {
            tx_a.send(note(o)).unwrap();
            tx_b.send(note(o)).unwrap();
        }
        drop(tx_a);
        drop(tx_b);

        let mut by_block = test_mixer(rx_a);
        let mut by_frame = test_mixer(rx_b);

        let n = 256;
        let mut block_out = vec![(0.0f32, 0.0f32); n];
        by_block.render_block(&mut block_out);

        let mut max_diff = 0.0f32;
        for frame in block_out {
            let (fl, fr) = by_frame.render_frame();
            max_diff = max_diff.max((frame.0 - fl).abs()).max((frame.1 - fr).abs());
        }
        assert!(
            max_diff < 1e-6,
            "block render diverged from frame render (max diff {max_diff:e})"
        );
        assert_eq!(by_block.active.len(), by_frame.active.len(), "voice counts");
    }

    #[test]
    fn mixer_renders_a_scheduled_note() {
        // Drive a Mixer directly (no audio device) and confirm a scheduled
        // note produces non-silent output once its onset passes.
        let (tx, rx) = crossbeam_channel::unbounded::<NoteEvent>();
        let mut mixer = test_mixer(rx);
        let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)));
        let events = collect_events(&pat, 1.0, 0.0, 1.0, &SampleBank::new());
        for ev in events {
            tx.send(ev).unwrap();
        }
        drop(tx);

        let mut peak = 0.0f32;
        for _ in 0..4410 {
            let (l, _r) = mixer.render_frame();
            peak = peak.max(l.abs());
        }
        assert!(peak > 0.0, "scheduled note should produce sound");
    }

    #[test]
    fn master_volume_scales_the_final_mix() {
        struct ConstVoice;

        impl VoiceLike for ConstVoice {
            fn tick(&mut self) -> (f32, f32) {
                (1.0, 1.0)
            }

            fn is_done(&self) -> bool {
                false
            }

            fn room(&self) -> f32 {
                0.0
            }

            fn delay_send(&self) -> f32 {
                0.0
            }
        }

        let (_tx, rx) = crossbeam_channel::unbounded::<NoteEvent>();
        let volume = test_volume(0.5);
        let mut mixer = test_mixer_with_volume(rx, volume.clone());
        mixer.active.push(ActiveVoice {
            voice: Box::new(ConstVoice),
            cut: None,
            choke_gain: None,
        });

        assert_eq!(mixer.render_frame(), (0.5, 0.5));
        store_f64(&volume, 2.0);
        assert_eq!(mixer.render_frame(), (2.0, 2.0));
    }

    #[test]
    fn scheduler_window_continues_from_the_cursor() {
        // cps=1, now=10s, lookahead 0.1 -> current 10.0, target 10.1.
        let clock = Clock::new(1.0);
        let (begin, end) =
            next_schedule_window(10.08, clock.cycle_at(10.0), clock.cycle_at(10.1)).unwrap();
        assert!((begin - 10.08).abs() < 1e-9);
        assert!((end - 10.1).abs() < 1e-9);
    }

    #[test]
    fn scheduler_window_snaps_to_current_when_cursor_is_stale() {
        // A cursor left behind the live window (e.g. after a gap) snaps forward
        // to current_cycle so no time is double-scheduled.
        let (begin, end) = next_schedule_window(2.0, 5.0, 5.05).unwrap();
        assert!((begin - 5.0).abs() < 1e-9);
        assert!((end - 5.05).abs() < 1e-9);
    }

    #[test]
    fn scheduler_window_waits_when_cursor_is_ahead_of_the_window() {
        // A cursor past the window (e.g. a cps drop shrank the lookahead) must
        // not re-schedule already-covered cycles — the window is empty.
        assert!(next_schedule_window(20.0, 5.0, 5.05).is_none());
    }

    #[test]
    fn live_cps_change_does_not_double_schedule_or_jump() {
        // Stable at cps=1; the scheduler has reached cycle ~10.1 by t=10s.
        let mut clock = Clock::new(1.0);
        let scheduled = 10.1;
        // Halving cps at t=10 re-anchors: the cycle position is unchanged (no
        // jump), and the cycle lookahead shrinks to 0.05.
        clock.set_cps(10.0, 0.5);
        assert!(
            (clock.cycle_at(10.0) - 10.0).abs() < 1e-9,
            "cps change must not jump cycles"
        );
        // Right after the change the cursor (10.1) is past the new target
        // (10.05), so nothing is scheduled — no double-trigger.
        assert!(
            next_schedule_window(scheduled, clock.cycle_at(10.0), clock.cycle_at(10.1)).is_none()
        );
        // Once time advances so the cursor enters the window, scheduling
        // continues seamlessly from it (cycle 10.1 falls at t=10.2s).
        let (begin, _end) =
            next_schedule_window(scheduled, clock.cycle_at(10.2), clock.cycle_at(10.3)).unwrap();
        assert!((begin - scheduled).abs() < 1e-9);
    }
}
