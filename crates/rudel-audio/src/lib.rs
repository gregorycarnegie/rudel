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
/// SoundFont 2 (`.sf2`) file reading.
pub mod sf2;
/// General MIDI soundfont playback (WebAudioFont presets).
pub mod soundfont;

pub use clock::Clock;
pub use events::{NoteEvent, collect_events, collect_events_at, to_control_map};
pub use samples::SampleBank;
pub use soundfont::{gm_names, set_soundfont_url, take_font_requests};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use fundsp::prelude32::{AudioUnit, reverb_stereo};
use rudel_core::Pattern;
use rudel_dsp::{DelayConfig, Djf, Duck, DuckEnv, OrbitSend, ReverbConfig, VoiceLike};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::Duration,
};

/// Poison-recovering lock accessors: a panic on one thread must not cascade
/// into the audio/scheduler threads. The guarded data (pattern, bank, clock,
/// taps) holds no cross-panic invariants — worst case is one stale value — so
/// log and keep playing.
fn recover_poison<G>(e: std::sync::PoisonError<G>) -> G {
    // G is the lock guard type, whose name identifies both the lock kind and
    // the guarded data, e.g. "RwLockWriteGuard<SampleBank>".
    eprintln!(
        "[rudel-audio] recovered poisoned {} (another thread panicked)",
        std::any::type_name::<G>()
    );
    e.into_inner()
}
fn read_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(recover_poison)
}
fn write_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(recover_poison)
}
fn lock_mutex<T>(lock: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(recover_poison)
}

/// Longest `delaytime` the delay line can be retuned to, in seconds. Matches
/// the `maxDelayTime` superdough gives its `createFeedbackDelay(1, …)` node.
const MAX_DELAY_SECS: f32 = 1.0;

/// A stereo feedback delay line for an orbit's `delay` send bus. The buffer is
/// allocated once at [`MAX_DELAY_SECS`] and the delay time is a read offset
/// into it, so `delaytime` can be retuned live without allocating on the audio
/// thread.
struct StereoDelay {
    /// Circular buffer for the left channel delay line.
    left: Vec<f32>,
    /// Circular buffer for the right channel delay line.
    right: Vec<f32>,
    /// Current circular buffer write index.
    write: usize,
    /// Delay length in samples (at least 1, at most the buffer length).
    delay_samples: usize,
    /// Feedback amount, 0..0.98 (superdough's ear-saving clamp).
    feedback: f32,
    sample_rate: f32,
}

impl StereoDelay {
    fn new(sample_rate: f32, cfg: DelayConfig) -> StereoDelay {
        let len = (sample_rate * MAX_DELAY_SECS).max(1.0) as usize;
        let mut d = StereoDelay {
            left: vec![0.0; len],
            right: vec![0.0; len],
            write: 0,
            delay_samples: 1,
            feedback: 0.0,
            sample_rate,
        };
        d.configure(cfg);
        d
    }

    /// Retune time and feedback in place, keeping the buffer contents so a
    /// changing `delaytime` glides rather than clicking to silence.
    fn configure(&mut self, cfg: DelayConfig) {
        let max = self.left.len();
        self.delay_samples = ((self.sample_rate * cfg.time) as usize).clamp(1, max);
        self.feedback = cfg.feedback.clamp(0.0, 0.98);
    }

    /// Process a single stereo input frame and return the delayed output frame.
    fn process(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let len = self.left.len();
        let read = (self.write + len - self.delay_samples) % len;
        let (out_l, out_r) = (self.left[read], self.right[read]);
        self.left[self.write] = in_l + out_l * self.feedback;
        self.right[self.write] = in_r + out_r * self.feedback;
        self.write = (self.write + 1) % len;
        (out_l, out_r)
    }
}

/// One orbit's effect bus: its own reverb, feedback delay and DJ filter, plus
/// the accumulation buffers the voices routed to it mix into.
///
/// Mirrors superdough's `Orbit` (`superdoughoutput.mjs`): voices send `room`
/// into the reverb and `delay` into the delay line, both returns sum with the
/// dry signal, and the sum passes through `djf` on the way to the master mix.
struct OrbitBus {
    delay: StereoDelay,
    delay_cfg: DelayConfig,
    reverb: Box<dyn AudioUnit>,
    reverb_cfg: ReverbConfig,
    /// Per-channel DJ filter; `None` until an event sets `djf`.
    djf: Option<(Djf, Djf)>,
    /// Sidechain duck on this orbit's output gain, driven by other orbits'
    /// `duckorbit` events.
    duck: DuckEnv,
    /// Dry / reverb-send / delay-send accumulation buffers for this orbit.
    dry_l: Vec<f32>,
    dry_r: Vec<f32>,
    room_l: Vec<f32>,
    room_r: Vec<f32>,
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    /// Frames since this orbit last received any input, used to stop running
    /// its reverb and delay once the tail has died away.
    idle_frames: u64,
    sample_rate: f32,
}

impl OrbitBus {
    fn new(sample_rate: f32, send: &OrbitSend) -> OrbitBus {
        OrbitBus {
            delay: StereoDelay::new(sample_rate, send.delay_cfg),
            delay_cfg: send.delay_cfg,
            reverb: build_reverb(sample_rate, send.reverb),
            reverb_cfg: send.reverb,
            djf: send
                .djf
                .map(|v| (Djf::new(sample_rate, v), Djf::new(sample_rate, v))),
            duck: DuckEnv::default(),
            dry_l: Vec::new(),
            dry_r: Vec::new(),
            room_l: Vec::new(),
            room_r: Vec::new(),
            delay_l: Vec::new(),
            delay_r: Vec::new(),
            idle_frames: u64::MAX,
            sample_rate,
        }
    }

    /// Apply an event's orbit settings. Like superdough's `getReverb`, the
    /// reverb is only rebuilt when a parameter actually changed — otherwise a
    /// stack of voices all carrying the same defaults would reset the tail on
    /// every note.
    fn configure(&mut self, send: &OrbitSend) {
        if send.reverb != self.reverb_cfg {
            self.reverb = build_reverb(self.sample_rate, send.reverb);
            self.reverb_cfg = send.reverb;
        }
        if send.delay_cfg != self.delay_cfg {
            self.delay.configure(send.delay_cfg);
            self.delay_cfg = send.delay_cfg;
        }
        if let Some(v) = send.djf {
            match &mut self.djf {
                Some((l, r)) => {
                    l.set_value(v);
                    r.set_value(v);
                }
                None => {
                    self.djf = Some((Djf::new(self.sample_rate, v), Djf::new(self.sample_rate, v)))
                }
            }
        }
    }

    /// Zero this orbit's accumulation buffers, growing them to `n` frames.
    fn clear(&mut self, n: usize) {
        for b in [
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
            b[..n].fill(0.0);
        }
    }

    /// Start (or restart) a sidechain duck on this orbit's output.
    fn duck(&mut self, duck: &Duck) {
        self.duck.trigger(self.sample_rate, duck);
    }

    /// Run the delay, reverb, DJ filter and duck envelope over `n` accumulated
    /// frames and add the result into `out`. Returns without doing any work once
    /// the orbit has been silent long enough for its tail to have decayed, so
    /// idle orbits cost nothing.
    fn mix_into(&mut self, out: &mut [(f32, f32)]) {
        let n = out.len();
        let fed = self.dry_l[..n].iter().any(|&x| x != 0.0)
            || self.room_l[..n].iter().any(|&x| x != 0.0)
            || self.delay_l[..n].iter().any(|&x| x != 0.0)
            || self.dry_r[..n].iter().any(|&x| x != 0.0)
            || self.room_r[..n].iter().any(|&x| x != 0.0)
            || self.delay_r[..n].iter().any(|&x| x != 0.0);
        if fed {
            self.idle_frames = 0;
        } else {
            // ponytail: fixed idle window rather than measuring the tail's
            // actual level. Long reverbs (`size` above ~8s) with a long delay
            // could in principle be cut off; raise the multiplier or track the
            // output RMS if that ever bites.
            let tail = self.reverb_cfg.size.max(self.delay_cfg.time) * 2.0 + 1.0;
            // Keep running while a duck is in flight so its envelope stays in
            // step with the ducker, even across a silent stretch.
            if self.idle_frames > (self.sample_rate * tail) as u64 && self.duck.is_idle() {
                return;
            }
            self.idle_frames = self.idle_frames.saturating_add(n as u64);
        }

        for (i, frame) in out.iter_mut().enumerate() {
            let (dl, dr) = self.delay.process(self.delay_l[i], self.delay_r[i]);
            let mut rout = [0.0f32; 2];
            self.reverb
                .tick(&[self.room_l[i], self.room_r[i]], &mut rout);
            let (mut l, mut r) = (self.dry_l[i] + dl + rout[0], self.dry_r[i] + dr + rout[1]);
            if let Some((fl, fr)) = &mut self.djf {
                l = fl.process(l);
                r = fr.process(r);
            }
            // The duck rides the orbit's output gain, after `djf` — the same
            // place superdough puts it (`Orbit.output.gain`).
            let duck = self.duck.next_gain();
            frame.0 += l * duck;
            frame.1 += r * duck;
        }
    }
}

/// Ring-buffer capacity of [`ScopeTap`] (power of two; ~0.19s at 44.1kHz).
const SCOPE_TAP_LEN: usize = 8192;

/// Lock-free ring buffer of the most recent mono output samples, written by
/// the audio callback and read by UI visualizers (`_scope` / `_spectrum`).
/// Single writer (the audio thread); a reader may catch a window mid-update,
/// which is harmless for display.
pub struct ScopeTap {
    /// Sample ring, each slot an `f32` stored as bits.
    buf: Box<[AtomicU32]>,
    /// Total samples written since start (monotonic write cursor).
    written: AtomicU64,
}

impl ScopeTap {
    fn new() -> ScopeTap {
        ScopeTap {
            buf: (0..SCOPE_TAP_LEN).map(|_| AtomicU32::new(0)).collect(),
            written: AtomicU64::new(0),
        }
    }

    /// Append samples to the ring (audio thread only).
    fn write(&self, samples: impl ExactSizeIterator<Item = f32>) {
        let mask = (self.buf.len() - 1) as u64;
        let start = self.written.load(Ordering::Relaxed);
        let n = samples.len() as u64;
        for (i, s) in samples.enumerate() {
            self.buf[((start + i as u64) & mask) as usize].store(s.to_bits(), Ordering::Relaxed);
        }
        self.written.store(start + n, Ordering::Release);
    }

    /// Append the mono mix of rendered stereo frames (audio thread only).
    fn write_frames(&self, frames: &[(f32, f32)]) {
        self.write(frames.iter().map(|(l, r)| (l + r) * 0.5));
    }

    /// Copy the most recent `out.len()` samples into `out`, oldest first.
    /// Samples older than the ring (or not yet written) read as silence.
    pub fn latest(&self, out: &mut [f32]) {
        let mask = (self.buf.len() - 1) as u64;
        let end = self.written.load(Ordering::Acquire);
        let n = (out.len() as u64).min(self.buf.len() as u64).min(end);
        let pad = out.len() - n as usize;
        out[..pad].fill(0.0);
        for (o, idx) in out[pad..].iter_mut().zip((end - n)..end) {
            *o = f32::from_bits(self.buf[(idx & mask) as usize].load(Ordering::Relaxed));
        }
    }
}

/// The engine's scope taps: the master mix plus one ring per analyzed widget
/// tag. Mirrors Strudel's per-`analyze`-id `AnalyserNode`s: each inline
/// scope/spectrum widget registers a tap under its widget id, and the mixer
/// feeds it from just the voices whose haps carry that tag, so a scope shows
/// its own pattern's audio rather than the master mix.
pub struct ScopeTaps {
    master: ScopeTap,
    named: RwLock<HashMap<String, Arc<ScopeTap>>>,
}

impl ScopeTaps {
    fn new() -> ScopeTaps {
        ScopeTaps {
            master: ScopeTap::new(),
            named: RwLock::new(HashMap::new()),
        }
    }

    /// The tap for a widget id, created on first use.
    pub fn get_or_create(&self, id: &str) -> Arc<ScopeTap> {
        if let Some(tap) = read_lock(&self.named).get(id) {
            return tap.clone();
        }
        write_lock(&self.named)
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(ScopeTap::new()))
            .clone()
    }

    /// Drop the tap of a removed widget.
    pub fn remove(&self, id: &str) {
        write_lock(&self.named).remove(id);
    }

    /// The master-mix tap (post master volume).
    pub fn master(&self) -> &ScopeTap {
        &self.master
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
    /// Widget tags of the source hap; the voice's output is added to the
    /// matching per-widget scope taps.
    tags: Vec<String>,
    /// Optional cut group (e.g. for choking open/closed hi-hats).
    cut: Option<i32>,
    /// Orbit routing and send levels for this voice.
    send: OrbitSend,
    /// When choked, the remaining gain (ramps 1.0 → 0.0 over `CHOKE_SECS`).
    /// `None` means the voice is playing normally.
    choke_gain: Option<f32>,
}

/// Fade time applied when a `cut`-group voice is choked (matches Strudel's 10ms).
const CHOKE_SECS: f32 = 0.01;
const DEFAULT_MASTER_VOLUME: f64 = 1.0;
const MAX_MASTER_VOLUME: f64 = 2.0;

/// Reusable per-block scratch: one voice's rendered stereo block. Grown to the
/// callback's block size on first use, then reused. (The dry / reverb / delay
/// accumulation buffers live on each [`OrbitBus`].)
#[derive(Default)]
struct MixScratch {
    src_l: Vec<f32>,
    src_r: Vec<f32>,
}

impl MixScratch {
    /// Ensure every buffer holds at least `n` samples.
    fn ensure(&mut self, n: usize) {
        for b in [&mut self.src_l, &mut self.src_r] {
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
    /// Effect buses keyed by `orbit`, created on demand as voices arrive.
    orbits: HashMap<i32, OrbitBus>,
    /// Master output volume, shared with the UI/control thread.
    volume: Arc<AtomicU64>,
    /// Reusable per-block render/accumulation buffers.
    scratch: MixScratch,
    /// Output rings shared with UI scope/spectrum visualizers.
    taps: Arc<ScopeTaps>,
    /// Per-widget-tag mono accumulation buffers feeding the named taps.
    tag_bufs: HashMap<String, Vec<f32>>,
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
                // Create the orbit on first use and let this event configure
                // it, as superdough does when it builds a voice's chain.
                let sample_rate = self.sample_rate;
                self.orbits
                    .entry(ev.send.orbit)
                    .or_insert_with(|| OrbitBus::new(sample_rate, &ev.send))
                    .configure(&ev.send);
                // Sidechain: duck the orbits this voice targets. The target is
                // created if it does not exist yet (superdough logs an error
                // instead; creating it means the duck still lands once that
                // orbit's own pattern starts).
                for d in &ev.duck {
                    let send = OrbitSend {
                        orbit: d.orbit,
                        ..OrbitSend::default()
                    };
                    self.orbits
                        .entry(d.orbit)
                        .or_insert_with(|| OrbitBus::new(sample_rate, &send))
                        .duck(d);
                }
                self.active.push(ActiveVoice {
                    voice: ev
                        .spec
                        .into_modulated_voice(self.sample_rate, ev.fx, &ev.mods),
                    tags: ev.tags,
                    cut: ev.cut,
                    send: ev.send,
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
        let sample_rate = self.sample_rate;
        self.scratch.ensure(len);

        let Mixer {
            active,
            scratch,
            orbits,
            sample_clock,
            taps,
            tag_bufs,
            ..
        } = self;
        let MixScratch { src_l, src_r } = scratch;
        for bus in orbits.values_mut() {
            bus.clear(len);
        }

        // Zero an accumulation buffer for every registered widget tap. The
        // read lock is only contended when the UI adds/removes a widget.
        let named = read_lock(&taps.named);
        for id in named.keys() {
            let buf = tag_bufs.entry(id.clone()).or_default();
            if buf.len() < len {
                buf.resize(len, 0.0);
            }
            buf[..len].fill(0.0);
        }
        if tag_bufs.len() > named.len() {
            tag_bufs.retain(|id, _| named.contains_key(id));
        }

        active.retain_mut(|av| {
            av.voice.process_block(&mut src_l[..len], &mut src_r[..len]);
            // Feed per-widget analyzer taps from the voice's raw output
            // (Strudel taps the sound chain, pre orbit sends / master fx).
            if !named.is_empty() {
                for tag in &av.tags {
                    if let Some(buf) = tag_bufs.get_mut(tag) {
                        for i in 0..len {
                            buf[i] += (src_l[i] + src_r[i]) * 0.5;
                        }
                    }
                }
            }
            // `dry` scales the direct signal; the reverb/delay sends are taken
            // pre-dry, so `dry(0)` leaves only the wet signal.
            let (dry, room, dsend) = (av.send.dry, av.send.room, av.send.delay);
            // Normally the orbit already exists (created when the voice
            // started); create it here too so a voice can never be routed into
            // a missing bus and silently disappear.
            let bus = orbits.entry(av.send.orbit).or_insert_with(|| {
                let mut b = OrbitBus::new(sample_rate, &av.send);
                b.clear(len);
                b
            });
            if let Some(g) = &mut av.choke_gain {
                // Choked voices fade per sample; drop the voice once silent.
                let mut gain = *g;
                for i in 0..len {
                    let (a, b) = (src_l[i] * gain, src_r[i] * gain);
                    bus.dry_l[i] += a * dry;
                    bus.dry_r[i] += b * dry;
                    if room > 0.0 {
                        bus.room_l[i] += a * room;
                        bus.room_r[i] += b * room;
                    }
                    if dsend > 0.0 {
                        bus.delay_l[i] += a * dsend;
                        bus.delay_r[i] += b * dsend;
                    }
                    gain -= choke_step;
                    if gain <= 0.0 {
                        return false; // fully faded — drop the voice
                    }
                }
                *g = gain;
            } else {
                for i in 0..len {
                    bus.dry_l[i] += src_l[i] * dry;
                    bus.dry_r[i] += src_r[i] * dry;
                }
                if room > 0.0 {
                    for i in 0..len {
                        bus.room_l[i] += src_l[i] * room;
                        bus.room_r[i] += src_r[i] * room;
                    }
                }
                if dsend > 0.0 {
                    for i in 0..len {
                        bus.delay_l[i] += src_l[i] * dsend;
                        bus.delay_r[i] += src_r[i] * dsend;
                    }
                }
            }
            !av.voice.is_done()
        });

        // Every orbit runs its own delay + reverb + DJ filter and sums into the
        // master mix.
        out.fill((0.0, 0.0));
        for bus in orbits.values_mut() {
            bus.mix_into(out);
        }
        for frame in out.iter_mut() {
            frame.0 *= volume;
            frame.1 *= volume;
        }
        taps.master.write_frames(out);
        for (id, tap) in named.iter() {
            if let Some(buf) = tag_bufs.get(id) {
                tap.write(buf[..len].iter().copied());
            }
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
        let (tx, rx) = mpsc::channel::<NoteEvent>();
        let volume = Arc::new(AtomicU64::new(0));
        store_f64(&volume, DEFAULT_MASTER_VOLUME);
        let mixer = Mixer {
            rx,
            pending: Vec::new(),
            active: Vec::new(),
            sample_clock: 0,
            sample_rate,
            played: Arc::new(AtomicU64::new(0)),
            orbits: HashMap::new(),
            volume,
            scratch: MixScratch::default(),
            taps: Arc::new(ScopeTaps::new()),
            tag_bufs: HashMap::new(),
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

    /// The mixer's scope taps (master + per-widget-tag rings).
    pub fn taps(&self) -> &ScopeTaps {
        &self.mixer.taps
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
    /// selects the preset.
    pub fn spawn_sf2(&self, path: String, name: String) -> JoinHandle<Result<usize, String>> {
        let bank = self.bank.clone();
        std::thread::spawn(move || {
            let bytes = std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
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

/// Build the global FDN reverb (fundsp), configured for the sample rate.
fn build_reverb(sample_rate: f32, cfg: ReverbConfig) -> Box<dyn AudioUnit> {
    // ponytail: a parametric FDN, not superdough's convolution against a
    // generated noise impulse response. The upstream IR is literally
    // `Math.random()` per sample (`reverbGen.mjs`), so there is no sample-exact
    // target to hit; what matters for parity is that the *controls* do what
    // they say. `roomsize` is the -60dB decay time either way, and the IR's
    // gradual `roomlp` -> `roomdim` lowpass becomes the FDN's HF damping. The
    // upgrade path, if `ir`/`iresponse` is ever wanted, is a partitioned FFT
    // convolver — which would subsume this.
    //
    // `roomfade` (the IR's fade-in) has no FDN analogue and is accepted but
    // ignored; it needs the convolver too.
    let decay = cfg.size.max(0.01);
    // Log-scaled so the defaults (lp 15000, dim 1000) land on 0.5 — the damping
    // the fixed reverb used before — and more `roomdim` closing means more
    // damping.
    let damping = ((cfg.lp.max(1.0) / cfg.dim.max(1.0)).log2() / 8.0).clamp(0.0, 1.0);
    let mut unit = Box::new(reverb_stereo(10.0, decay, damping));
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

    fn test_mixer(rx: Receiver<NoteEvent>) -> Mixer {
        test_mixer_with_volume(rx, test_volume(DEFAULT_MASTER_VOLUME))
    }

    fn test_mixer_with_volume(rx: Receiver<NoteEvent>, volume: Arc<AtomicU64>) -> Mixer {
        Mixer {
            rx,
            pending: Vec::new(),
            active: Vec::new(),
            sample_clock: 0,
            sample_rate: 44100.0,
            played: Arc::new(AtomicU64::new(0)),
            orbits: HashMap::new(),
            volume,
            scratch: MixScratch::default(),
            taps: Arc::new(ScopeTaps::new()),
            tag_bufs: HashMap::new(),
        }
    }

    #[test]
    fn scope_tap_returns_the_most_recent_samples() {
        let tap = ScopeTap::new();
        // Fewer samples written than requested: left-padded with silence,
        // stereo frames averaged to mono.
        tap.write_frames(&[(1.0, 1.0), (1.0, 3.0)]);
        let mut out = [9.0f32; 4];
        tap.latest(&mut out);
        assert_eq!(out, [0.0, 0.0, 1.0, 2.0]);
        // Wrap the ring and confirm the newest window is still returned.
        tap.write_frames(&vec![(0.5, 0.5); SCOPE_TAP_LEN + 3]);
        tap.latest(&mut out);
        assert_eq!(out, [0.5; 4]);
    }

    #[test]
    fn tagged_voices_feed_their_widget_tap_only() {
        let mut mixer = OfflineMixer::new(44100.0);
        let tagged = mixer.taps().get_or_create("w1");
        let silent = mixer.taps().get_or_create("w2");
        let ev = |tags: Vec<String>| NoteEvent {
            onset_seconds: 0.0,
            spec: rudel_dsp::VoiceSpec::Synth(Box::new(rudel_dsp::VoiceParams::from_controls(
                &rudel_core::to_control_map(&rudel_core::Value::Str("sawtooth".into())),
                10.0,
            ))),
            fx: rudel_dsp::PostFx::default(),
            cut: None,
            send: OrbitSend::default(),
            duck: Vec::new(),
            mods: Default::default(),
            tags,
        };
        mixer.schedule(ev(vec!["w1".to_string()]));
        mixer.schedule(ev(Vec::new()));
        let mut out = vec![(0.0f32, 0.0f32); 2048];
        mixer.render_block(&mut out);

        let mut got = [0.0f32; 256];
        tagged.latest(&mut got);
        assert!(
            got.iter().any(|s| s.abs() > 0.0),
            "the w1 tap should hear the w1-tagged voice"
        );
        silent.latest(&mut got);
        assert!(
            got.iter().all(|s| *s == 0.0),
            "the w2 tap should stay silent (no voice carries its tag)"
        );
        let mut master = [0.0f32; 256];
        mixer.taps().master().latest(&mut master);
        assert!(
            master.iter().any(|s| s.abs() > 0.0),
            "the master tap hears the mix"
        );
    }

    #[test]
    fn stereo_delay_echoes_after_its_time() {
        let mut d = StereoDelay::new(
            1000.0,
            DelayConfig {
                time: 0.01, // 10-sample delay
                feedback: 0.5,
            },
        );
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
        let (tx, rx) = mpsc::channel::<NoteEvent>();
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

    /// Render `secs` seconds of `pat` at `cps` and return the mono frames.
    fn render_pattern(pat: &Pattern, cps: f64, secs: f32) -> Vec<f32> {
        let (tx, rx) = mpsc::channel::<NoteEvent>();
        let mut mixer = test_mixer(rx);
        for ev in collect_events(pat, cps, 0.0, 1.0, &SampleBank::new()) {
            tx.send(ev).unwrap();
        }
        drop(tx);
        (0..(44100.0 * secs) as usize)
            .map(|_| {
                let (l, r) = mixer.render_frame();
                (l + r) * 0.5
            })
            .collect()
    }

    /// Index of the loudest frame in `frames[from..]`, as a time in seconds.
    fn peak_time(frames: &[f32], from: usize) -> f32 {
        let (i, _) = frames[from..]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap();
        (from + i) as f32 / 44100.0
    }

    #[test]
    fn delaytime_places_the_echo() {
        // `delaytime` used to be inert (the delay line was hardwired to 1/6s).
        // A short note with a full delay send should echo at `delaytime`.
        let echo_at = |delaytime: f64| {
            let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
                .delay(rudel_core::Value::F64(1.0))
                .delaytime(rudel_core::Value::F64(delaytime))
                .delayfeedback(rudel_core::Value::F64(0.0));
            // Skip the direct signal (the note itself is ~0.06s at 4 cps).
            let frames = render_pattern(&pat, 4.0, 0.6);
            peak_time(&frames, (44100.0 * 0.12) as usize)
        };
        for want in [0.2, 0.35] {
            let got = echo_at(want);
            assert!(
                (got - want as f32).abs() < 0.02,
                "delaytime({want}) should echo at ~{want}s, got {got}s"
            );
        }
    }

    #[test]
    fn delaysync_scales_the_echo_with_cps() {
        // With no explicit `delaytime`, superdough derives it from `delaysync`
        // (a fraction of a cycle), so the echo tracks the tempo.
        let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
            .delay(rudel_core::Value::F64(1.0))
            .delaysync(rudel_core::Value::F64(0.25))
            .delayfeedback(rudel_core::Value::F64(0.0));
        // 0.25 cycles at 1 cps = 0.25s; at 2 cps = 0.125s.
        let slow = peak_time(&render_pattern(&pat, 1.0, 0.6), (44100.0 * 0.06) as usize);
        let fast = peak_time(&render_pattern(&pat, 2.0, 0.6), (44100.0 * 0.06) as usize);
        assert!(
            (slow - 0.25).abs() < 0.02,
            "1 cps echo at {slow}s, want 0.25s"
        );
        assert!(
            (fast - 0.125).abs() < 0.02,
            "2 cps echo at {fast}s, want 0.125s"
        );
    }

    #[test]
    fn roomsize_lengthens_the_reverb_tail() {
        // `size`/`roomsize` used to be inert (one fixed 1.5s reverb).
        let tail_energy = |size: f64| {
            let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
                .room(rudel_core::Value::F64(1.0))
                .dry(rudel_core::Value::F64(0.0))
                .size(rudel_core::Value::F64(size));
            let frames = render_pattern(&pat, 4.0, 3.0);
            // Energy well after the note has finished.
            frames[(44100.0 * 1.5) as usize..]
                .iter()
                .map(|x| x.abs())
                .sum::<f32>()
        };
        let short = tail_energy(0.3);
        let long = tail_energy(6.0);
        assert!(
            long > short * 2.0,
            "a 6s room ({long}) should ring far longer than a 0.3s room ({short})"
        );
    }

    /// `pat.lfo({...})` / `pat.env({...})` with a literal config.
    fn modulate(pat: &Pattern, kind: &str, cfg: &[(&str, rudel_core::Value)]) -> Pattern {
        let config = cfg
            .iter()
            .map(|(k, v)| (k.to_string(), rudel_core::pure(v.clone())))
            .collect();
        rudel_core::modulate(pat, kind, config, rudel_core::pure(rudel_core::Value::Null))
    }

    /// The loudest-to-quietest ratio across an amplitude envelope, skipping the
    /// first two windows (the note's own attack).
    fn spread(v: &[f32]) -> f32 {
        let (lo, hi) = v[2..]
            .iter()
            .fold((f32::MAX, 0.0f32), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        hi / lo.max(1e-9)
    }

    /// Per-window peak levels of `frames`, `windows` windows across the whole
    /// buffer — a cheap amplitude envelope.
    fn window_peaks(frames: &[f32], windows: usize) -> Vec<f32> {
        let w = frames.len() / windows;
        (0..windows)
            .map(|i| {
                frames[i * w..(i + 1) * w]
                    .iter()
                    .fold(0.0f32, |m, x| m.max(x.abs()))
            })
            .collect()
    }

    #[test]
    fn an_lfo_modulates_the_gain_it_targets() {
        // `.gain(1).lfo({control:'gain', rate:8})`: depth defaults to 1, scaled
        // by the target's own value (1), and the LFO's dcoffset of -0.5 makes
        // the offset swing +/-0.5 — so the gain tremolos between 0.5 and 1.5.
        let held = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
            .gain(rudel_core::Value::F64(1.0));
        let modulated = modulate(
            &held,
            "lfo",
            &[
                ("control", rudel_core::Value::Str("gain".into())),
                ("rate", rudel_core::Value::F64(2.0)),
                ("shape", rudel_core::Value::Str("sine".into())),
            ],
        );

        // 32 windows over 0.9s is ~28ms each, well inside the 500ms LFO period.
        let flat = window_peaks(&render_pattern(&held, 1.0, 0.9), 32);
        let swept = window_peaks(&render_pattern(&modulated, 1.0, 0.9), 32);
        assert!(spread(&flat) < 1.2, "unmodulated gain should be steady");
        assert!(
            spread(&swept) > 2.5,
            "a gain LFO should swing the level ~3x ({})",
            spread(&swept)
        );
    }

    #[test]
    fn an_lfo_defaults_to_the_control_before_it_in_the_chain() {
        // The documented example: `.lpf(500).lfo({rate:2})` sweeps the cutoff,
        // because a modulator with no explicit `control` targets whatever was
        // applied just before it.
        // The note sits at 440Hz and the cutoff sweeps 200..600Hz, so the
        // fundamental moves in and out of the passband.
        let saw = rudel_core::s(rudel_core::pure(rudel_core::Value::Str("sawtooth".into())))
            .note(rudel_core::Value::Int(69))
            .cutoff(rudel_core::Value::F64(400.0));
        let modulated = modulate(&saw, "lfo", &[("rate", rudel_core::Value::F64(2.0))]);

        let flat = window_peaks(&render_pattern(&saw, 1.0, 0.9), 32);
        let swept = window_peaks(&render_pattern(&modulated, 1.0, 0.9), 32);
        assert!(spread(&flat) < 1.2, "a static cutoff should be steady");
        assert!(
            spread(&swept) > 2.0,
            "a cutoff LFO should sweep the level ({})",
            spread(&swept)
        );
    }

    #[test]
    fn an_envelope_modulator_sweeps_its_target() {
        // `.gain(1).env({attack:0.5, sustain:1})` ramps the gain offset 0 -> 1
        // over half the note and holds it, so the level roughly doubles.
        let held = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
            .gain(rudel_core::Value::F64(1.0));
        let swept = modulate(
            &held,
            "env",
            &[
                ("attack", rudel_core::Value::F64(0.5)),
                ("sustain", rudel_core::Value::F64(1.0)),
            ],
        );
        let flat = window_peaks(&render_pattern(&held, 1.0, 0.9), 8);
        let swept = window_peaks(&render_pattern(&swept, 1.0, 0.9), 8);
        // Against the unmodulated note: still climbing early (the offset is
        // partway up its 0.5s attack), fully doubled once it reaches sustain.
        let early = swept[1] / flat[1];
        let late = swept[6] / flat[6];
        assert!(
            early < 1.6,
            "the envelope should still be climbing early ({early})"
        );
        assert!(
            late > 1.8,
            "the envelope should hold the gain at ~2x once sustained ({late})"
        );
    }

    /// Peak level of `frames` over the time window `[from, to)` seconds.
    fn peak_between(frames: &[f32], from: f32, to: f32) -> f32 {
        let idx = |t: f32| ((t * 44100.0) as usize).min(frames.len());
        frames[idx(from)..idx(to)]
            .iter()
            .fold(0.0f32, |m, x| m.max(x.abs()))
    }

    /// A held note on orbit 2, plus a silent ducker on orbit 1 that fires at
    /// the half cycle and ducks the given targets.
    fn duck_pattern(targets: Pattern, extra: impl Fn(Pattern) -> Pattern) -> Pattern {
        let held = |orbit: i64| {
            rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
                .orbit(rudel_core::Value::Int(orbit))
        };
        // `postgain(0)` silences the ducker itself, like Strudel's own examples,
        // so the measurement only sees the ducked orbit.
        let ducker = rudel_core::sequence(&[
            rudel_core::silence(),
            extra(
                rudel_core::s(rudel_core::pure(rudel_core::Value::Str("bd".into())))
                    .orbit(rudel_core::Value::Int(1))
                    .postgain(rudel_core::Value::F64(0.0))
                    .duckorbit(targets),
            ),
        ]);
        rudel_core::stack(&[held(2), ducker])
    }

    #[test]
    fn duckorbit_dips_the_target_orbit_and_recovers() {
        let pat = duck_pattern(rudel_core::pure(rudel_core::Value::Int(2)), |p| {
            p.duckattack(rudel_core::Value::F64(0.3))
        });
        let frames = render_pattern(&pat, 1.0, 1.0);
        let before = peak_between(&frames, 0.35, 0.49);
        let during = peak_between(&frames, 0.5, 0.55);
        let after = peak_between(&frames, 0.85, 0.99);
        assert!(before > 0.01, "the held note should be sounding ({before})");
        assert!(
            during < before * 0.2,
            "duckorbit(2) should dip orbit 2 ({during} vs {before})"
        );
        assert!(
            after > before * 0.8,
            "orbit 2 should recover after duckattack ({after} vs {before})"
        );
    }

    #[test]
    fn duckdepth_zero_leaves_the_target_alone() {
        // floor = 1 - sqrt(0) = 1, so there is nothing to duck.
        let pat = duck_pattern(rudel_core::pure(rudel_core::Value::Int(2)), |p| {
            p.duckdepth(rudel_core::Value::F64(0.0))
        });
        let frames = render_pattern(&pat, 1.0, 1.0);
        let before = peak_between(&frames, 0.35, 0.49);
        let during = peak_between(&frames, 0.5, 0.55);
        assert!(
            during > before * 0.8,
            "duckdepth(0) should not duck ({during} vs {before})"
        );
    }

    #[test]
    fn duck_control_lists_are_read_per_target() {
        // `duckorbit("2:3")` with `duckdepth("1:0")`: orbit 2 ducks fully,
        // orbit 3 not at all. Only orbit 2 carries the held note here, so a
        // per-target read is what makes it dip; a first-entry-for-all read
        // would too, so also check the reverse order.
        let targets = |a: i64, b: i64| {
            rudel_core::pure(rudel_core::Value::List(vec![
                rudel_core::Value::Int(a),
                rudel_core::Value::Int(b),
            ]))
        };
        let depths =
            rudel_core::Value::List(vec![rudel_core::Value::Int(1), rudel_core::Value::Int(0)]);
        let dip = |t: Pattern| {
            let pat = duck_pattern(t, |p| p.duckdepth(depths.clone()));
            let frames = render_pattern(&pat, 1.0, 1.0);
            (
                peak_between(&frames, 0.35, 0.49),
                peak_between(&frames, 0.5, 0.55),
            )
        };
        // Orbit 2 is first, so it takes depth 1 and is ducked.
        let (before, during) = dip(targets(2, 3));
        assert!(during < before * 0.2, "orbit 2 first: {during} vs {before}");
        // Orbit 2 is second, so it takes depth 0 and is left alone.
        let (before, during) = dip(targets(3, 2));
        assert!(
            during > before * 0.8,
            "orbit 2 second: {during} vs {before}"
        );
    }

    #[test]
    fn orbits_have_independent_effect_buses() {
        // A heavy `djf` lowpass on orbit 2 must not touch orbit 1. Both orbits
        // play the same bright note; only the filtered one should lose level.
        let note = |orbit: i64, djf: Option<f64>| {
            let p = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(90)))
                .orbit(rudel_core::Value::Int(orbit));
            match djf {
                Some(v) => p.djf(rudel_core::Value::F64(v)),
                None => p,
            }
        };
        let peak = |pat: &Pattern| {
            render_pattern(pat, 4.0, 0.3)
                .iter()
                .fold(0.0f32, |m, x| m.max(x.abs()))
        };

        let clean = peak(&note(1, None));
        // Same note on orbit 2 with the DJ filter fully closed.
        let filtered = peak(&note(2, Some(0.0)));
        assert!(
            filtered < clean * 0.5,
            "djf(0) should cut the note ({filtered} vs {clean})"
        );
        // Stacking them: orbit 1 keeps its level even though orbit 2 is filtered.
        let both = peak(&rudel_core::stack(&[note(1, None), note(2, Some(0.0))]));
        assert!(
            both >= clean * 0.9,
            "orbit 2's djf must not affect orbit 1 ({both} vs {clean})"
        );
    }

    #[test]
    fn cut_group_chokes_the_previous_voice() {
        // Two sustained notes in cut group 1, the second a little later. After
        // the second starts, the first should be choked to silence within the
        // ~10ms fade, leaving only one voice's worth of energy.
        let (tx, rx) = mpsc::channel::<NoteEvent>();
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
            send: OrbitSend::default(),
            duck: Vec::new(),
            mods: Default::default(),
            tags: Vec::new(),
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
            send: OrbitSend::default(),
            duck: Vec::new(),
            mods: Default::default(),
            tags: Vec::new(),
        };
        // Onsets at frames 0, ~37 and ~150 (44.1kHz) force mid-buffer splits.
        let onsets = [0.0, 37.0 / 44100.0, 150.0 / 44100.0];

        let (tx_a, rx_a) = mpsc::channel::<NoteEvent>();
        let (tx_b, rx_b) = mpsc::channel::<NoteEvent>();
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
        let (tx, rx) = mpsc::channel::<NoteEvent>();
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
        }

        let (_tx, rx) = mpsc::channel::<NoteEvent>();
        let volume = test_volume(0.5);
        let mut mixer = test_mixer_with_volume(rx, volume.clone());
        mixer.active.push(ActiveVoice {
            voice: Box::new(ConstVoice),
            tags: Vec::new(),
            cut: None,
            send: OrbitSend::default(),
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
