//! Real-time voice mixing, orbit effects, and offline rendering.

mod engine;
#[cfg(test)]
mod tests;

pub use engine::{CsoundSource, Engine};

use crate::{NoteEvent, csound::Csound, scope::ScopeTaps, sync::read_lock};
use rudel_dsp::{Convolver, DelayConfig, Djf, Duck, DuckEnv, OrbitSend, ReverbConfig, VoiceLike};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};

/// The Csound instance a script's `loadCsound`/`loadOrc` builds, shared between
/// the host thread that compiles orchestras into it and the audio callback that
/// renders it.
///
/// `None` until a script asks for Csound, so a session that never mentions it
/// never loads the library. The audio callback only ever *tries* the lock: a
/// compile can take milliseconds, and a block of Csound silence during a live
/// re-evaluation is cheaper than missing the device deadline for every layer.
pub(crate) type SharedCsound = Arc<Mutex<Option<Csound>>>;
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
    reverb: Convolver,
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
            reverb: build_reverb(sample_rate, &send.reverb),
            reverb_cfg: send.reverb.clone(),
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
    ///
    /// A voice only gets to configure an effect it actually *sends to*:
    /// superdough calls `getReverb`/`getDelay` from inside its `if (room > 0)`
    /// and `if (delay > 0 && ...)` branches, so a voice sending nothing there
    /// never touches those settings. Reading the
    /// controls off every event instead means one layer's defaults keep
    /// overwriting another's: in "Amensister" the chord layer's
    /// `delay(.8).delaytime(.125)` shares orbit 1 with a bass line that sets no
    /// delay at all, and each bass note — eight a cycle — retuned the delay
    /// back to the default 3/16, yanking the read head across the echo still
    /// ringing in the buffer.
    fn configure(&mut self, send: &OrbitSend) {
        if send.room > 0.0 && send.reverb != self.reverb_cfg {
            self.reverb = build_reverb(self.sample_rate, &send.reverb);
            self.reverb_cfg = send.reverb.clone();
        }
        if send.delay > 0.0 && send.delay_cfg != self.delay_cfg {
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
            let (rl, rr) = self.reverb.process(self.room_l[i], self.room_r[i]);
            let (mut l, mut r) = (self.dry_l[i] + dl + rl, self.dry_r[i] + dr + rr);
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
    /// The fade running this voice out, if one is. `None` means the voice is
    /// playing normally.
    choke: Option<Choke>,
}

/// A voice fading to silence: what is left of its gain, and how much of it each
/// sample takes off. The rate is per voice because the two things that choke
/// one want different ones.
#[derive(Clone, Copy)]
struct Choke {
    gain: f32,
    step: f32,
}

impl Choke {
    fn over(seconds: f32, sample_rate: f32) -> Choke {
        Choke {
            gain: 1.0,
            step: 1.0 / (sample_rate * seconds),
        }
    }
}

/// Fade time applied when a `cut`-group voice is choked (matches Strudel's 10ms).
const CHOKE_SECS: f32 = 0.01;
/// Fade time applied when `setMaxPolyphony` makes room for a new voice —
/// superdough ramps the ones it drops to zero over a quarter second.
const POLYPHONY_FADE_SECS: f32 = 0.25;
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
    /// Signal buses keyed by `bus`, created on demand as sending voices arrive.
    /// Stereo, like superdough's `getStereoNode`; `bmod` sums to mono on read,
    /// `s("bus")` plays both channels.
    signal_buses: HashMap<i32, (Vec<f32>, Vec<f32>)>,
    /// Master output volume, shared with the UI/control thread.
    volume: Arc<AtomicU64>,
    /// Reusable per-block render/accumulation buffers.
    scratch: MixScratch,
    /// Output rings shared with UI scope/spectrum visualizers.
    taps: Arc<ScopeTaps>,
    /// Per-widget-tag mono accumulation buffers feeding the named taps.
    tag_bufs: HashMap<String, Vec<f32>>,
    /// Csound, when a script has asked for it.
    csound: SharedCsound,
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
        // Csound is locked once for the whole callback, so a note injected at
        // a sub-block boundary is heard in that sub-block rather than a buffer
        // later. `try_lock` because the only other holder is a script
        // compiling an orchestra, and waiting for that would drop the buffer.
        // The `Arc` is cloned so the guard does not borrow `self`.
        let csound = self.csound.clone();
        let mut guard = csound.try_lock().ok();
        let sr = self.sample_rate as f64;
        let total = out.len();
        let mut offset = 0;
        while offset < total {
            let now = self.sample_clock as f64 / sr;
            self.start_due_events(now, guard.as_deref_mut().and_then(Option::as_mut));
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
            self.mix_sub_block(
                &mut out[offset..offset + sub_len],
                guard.as_deref_mut().and_then(Option::as_mut),
            );
            offset += sub_len;
        }
        self.played.store(self.sample_clock, Ordering::Relaxed);
    }

    /// Start every pending event whose onset has arrived by `now`, choking any
    /// same-`cut`-group voice (last-one-wins, like Strudel's cut groups).
    fn start_due_events(&mut self, now: f64, mut csound: Option<&mut Csound>) {
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].onset_seconds <= now {
                let ev = self.pending.swap_remove(i);
                // `.csound(...)` plays the hap on a Csound instrument *instead*
                // of a Rudel voice — upstream's is an `onTrigger`, which
                // replaces the sound. A statement with no Csound to send it to
                // is dropped rather than falling back to a synth: the tune
                // asked for an instrument this engine does not have, and a
                // sawtooth in its place is a worse answer than silence.
                if let Some(statement) = &ev.csound {
                    if let Some(cs) = csound.as_deref_mut() {
                        cs.input_message(statement);
                    }
                    continue;
                }
                if let Some(g) = ev.cut {
                    for av in &mut self.active {
                        if av.cut == Some(g) && av.choke.is_none() {
                            av.choke = Some(Choke::over(CHOKE_SECS, self.sample_rate));
                        }
                    }
                }
                // `setMaxPolyphony(n)`: making room for this voice fades out
                // the oldest ones, oldest first — `self.active` is in start
                // order, as superdough's insertion-ordered map is. A voice
                // already fading no longer counts against the cap, which is
                // upstream deleting it from the map as it starts the ramp.
                let cap = rudel_core::max_polyphony();
                let sounding = self.active.iter().filter(|av| av.choke.is_none()).count();
                let over = sounding.saturating_sub(cap.saturating_sub(1));
                let rate = Choke::over(POLYPHONY_FADE_SECS, self.sample_rate);
                for av in self
                    .active
                    .iter_mut()
                    .filter(|av| av.choke.is_none())
                    .take(over)
                {
                    av.choke = Some(rate);
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
                if let Some(b) = ev.send.bus {
                    self.signal_buses.entry(b).or_default();
                }
                self.active.push(ActiveVoice {
                    voice: ev
                        .spec
                        .into_modulated_voice(self.sample_rate, ev.fx, &ev.mods),
                    tags: ev.tags,
                    cut: ev.cut,
                    send: ev.send,
                    choke: None,
                });
            } else {
                i += 1;
            }
        }
    }

    /// Mix all active voices over `out` (no new voices start within it): render
    /// each voice's block, accumulate the dry / reverb / delay buses, then apply
    /// the global delay + reverb per sample and write the master mix.
    fn mix_sub_block(&mut self, out: &mut [(f32, f32)], csound: Option<&mut Csound>) {
        let len = out.len();
        let volume = load_f64(&self.volume) as f32;
        let sample_rate = self.sample_rate;
        self.scratch.ensure(len);

        let Mixer {
            active,
            scratch,
            orbits,
            signal_buses,
            sample_clock,
            taps,
            tag_bufs,
            ..
        } = self;
        let MixScratch { src_l, src_r } = scratch;
        for bus in orbits.values_mut() {
            bus.clear(len);
        }
        // A bus reader — a `bmod` carrier or an `s("bus")` voice — must see the
        // signal its sender wrote in the *same* sub-block, so senders render
        // first. `sort_by_key` is stable and near free once the order holds,
        // which it does after the first block.
        //
        // ponytail: one level deep. A voice that both sends to a bus and reads
        // one sorts with the senders and sees a partly-filled bus; chaining bus
        // to bus would need a topological sort, which nothing asks for yet.
        if !signal_buses.is_empty() {
            for (l, r) in signal_buses.values_mut() {
                for b in [l, r] {
                    if b.len() < len {
                        b.resize(len, 0.0);
                    }
                    b[..len].fill(0.0);
                }
            }
            active.sort_by_key(|av| av.send.bus.is_none());
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
            for (id, (l, r)) in signal_buses.iter() {
                av.voice.set_bus_input(*id, &l[..len], &r[..len]);
            }
            av.voice.process_block(&mut src_l[..len], &mut src_r[..len]);
            // `bus` sends the voice's post-fx output on to a signal bus, on top
            // of (not instead of) its orbit routing. A choked voice sends at its
            // current fade level.
            if let Some((l, r)) = av.send.bus.and_then(|b| signal_buses.get_mut(&b)) {
                let g = av.send.busgain * av.choke.map_or(1.0, |c| c.gain);
                for i in 0..len {
                    l[i] += src_l[i] * g;
                    r[i] += src_r[i] * g;
                }
            }
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
            if let Some(choke) = &mut av.choke {
                // Choked voices fade per sample; drop the voice once silent.
                let (mut gain, choke_step) = (choke.gain, choke.step);
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
                choke.gain = gain;
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
        // Csound sums straight into the master, after the orbits: upstream
        // wires it to the audio context's destination, so it never sees
        // superdough's reverb, delay or DJ filter. Master volume still applies
        // — that is the app's own output level, not an effect.
        if let Some(cs) = csound {
            cs.render_into(out);
        }
        for frame in out.iter_mut() {
            frame.0 *= volume;
            frame.1 *= volume;
        }
        taps.master().write_frames(out);
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
            signal_buses: HashMap::new(),
            volume,
            scratch: MixScratch::default(),
            taps: Arc::new(ScopeTaps::new()),
            tag_bufs: HashMap::new(),
            csound: SharedCsound::default(),
        };
        OfflineMixer { tx, mixer }
    }

    /// Queue a note event (delivered on the next render call).
    pub fn schedule(&self, ev: NoteEvent) {
        let _ = self.tx.send(ev);
    }

    /// Render `.csound(...)` events through `csound`, as the real engine does
    /// once a script has called `loadCsound`.
    pub fn set_csound(&mut self, csound: Csound) {
        *self.mixer.csound.lock().unwrap_or_else(|e| e.into_inner()) = Some(csound);
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

/// Build an orbit's convolution reverb, matching superdough's `createReverb`.
///
/// With no `ir`, the impulse response is the generated noise tail
/// (`reverbGen.generateReverb`) shaped by `size`/`roomfade`/`roomlp`/`roomdim`;
/// with one, the loaded sample is fitted to `size` by `adjustLength` using
/// `irspeed`/`irbegin`.
///
// ponytail: the IR is generated and partitioned on the audio thread, as the
// fundsp reverb it replaces was allocated there. It only happens when a reverb
// parameter actually changes (guarded by `!=`), i.e. on an edit rather than per
// note, but a very long `size` could still cost a buffer. Build it on the
// background job queue and hand the finished convolver over if that ever bites.
fn build_reverb(sample_rate: f32, cfg: &ReverbConfig) -> Convolver {
    let size = cfg.size.max(0.001);
    let ir = match &cfg.ir {
        // A loaded impulse response: rudel decodes samples to mono, so the same
        // buffer feeds both channels.
        Some(sample) => rudel_dsp::adjust_length(
            &sample.data,
            &sample.data,
            sample_rate,
            size,
            cfg.irspeed,
            cfg.irbegin,
        ),
        None => rudel_dsp::generate_reverb_ir(sample_rate, size, cfg.fade, cfg.lp, cfg.dim),
    };
    Convolver::new(&ir, sample_rate)
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
