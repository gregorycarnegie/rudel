// rudel-audio - real-time clock, lookahead scheduler and cpal output.
// The scheduler maps cycle time to the audio sample clock and feeds timed
// note events to a mixer running in the audio callback.
// Clock approach mirrors strudel/packages/core/{zyklus,cyclist}.mjs.
// SPDX-License-Identifier: AGPL-3.0-or-later

pub mod events;
mod sample_map;
pub mod samples;

pub use events::{NoteEvent, collect_events, to_control_map};
pub use samples::SampleBank;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender};
use fundsp::prelude32::{AudioUnit, reverb_stereo};
use rudel_core::Pattern;
use rudel_dsp::VoiceLike;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// A simple stereo feedback delay line for the `delay` send bus.
struct StereoDelay {
    left: Vec<f32>,
    right: Vec<f32>,
    idx: usize,
    feedback: f32,
}

impl StereoDelay {
    fn new(sample_rate: f32, time: f32, feedback: f32) -> StereoDelay {
        let len = (sample_rate * time).max(1.0) as usize;
        StereoDelay {
            left: vec![0.0; len],
            right: vec![0.0; len],
            idx: 0,
            feedback,
        }
    }

    fn process(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let out_l = self.left[self.idx];
        let out_r = self.right[self.idx];
        self.left[self.idx] = in_l + out_l * self.feedback;
        self.right[self.idx] = in_r + out_r * self.feedback;
        self.idx = (self.idx + 1) % self.left.len();
        (out_l, out_r)
    }
}

fn store_f64(a: &AtomicU64, v: f64) {
    a.store(v.to_bits(), Ordering::Relaxed);
}
fn load_f64(a: &AtomicU64) -> f64 {
    f64::from_bits(a.load(Ordering::Relaxed))
}

/// A playing voice plus its `cut` group and an optional choke ramp.
struct ActiveVoice {
    voice: Box<dyn VoiceLike>,
    cut: Option<i32>,
    /// When choked, the remaining gain (ramps 1.0 → 0.0 over `CHOKE_SECS`).
    /// `None` means the voice is playing normally.
    choke_gain: Option<f32>,
}

/// Fade time applied when a `cut`-group voice is choked (matches Strudel's 10ms).
const CHOKE_SECS: f32 = 0.01;

/// Mixes active voices and starts new ones as their onset time arrives. Lives
/// in the audio callback.
struct Mixer {
    rx: Receiver<NoteEvent>,
    pending: Vec<NoteEvent>,
    active: Vec<ActiveVoice>,
    sample_clock: u64,
    sample_rate: f32,
    played: Arc<AtomicU64>,
    delay: StereoDelay,
    reverb: Box<dyn AudioUnit>,
}

impl Mixer {
    fn render_frame(&mut self) -> (f32, f32) {
        while let Ok(ev) = self.rx.try_recv() {
            self.pending.push(ev);
        }
        let now = self.sample_clock as f64 / self.sample_rate as f64;

        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].onset_seconds <= now {
                let ev = self.pending.swap_remove(i);
                // A new voice in a `cut` group chokes any still-playing voice in
                // the same group (last-one-wins, like Strudel's cut groups).
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

        // dry mix plus reverb (room) and delay sends
        let (mut dl, mut dr) = (0.0f32, 0.0f32);
        let (mut rl, mut rr) = (0.0f32, 0.0f32);
        let (mut el, mut er) = (0.0f32, 0.0f32);
        let choke_step = 1.0 / (self.sample_rate * CHOKE_SECS);
        self.active.retain_mut(|av| {
            let (mut a, mut b) = av.voice.tick();
            if let Some(g) = &mut av.choke_gain {
                a *= *g;
                b *= *g;
                *g -= choke_step;
                if *g <= 0.0 {
                    return false; // fully faded — drop the voice
                }
            }
            dl += a;
            dr += b;
            let room = av.voice.room();
            if room > 0.0 {
                rl += a * room;
                rr += b * room;
            }
            let dsend = av.voice.delay_send();
            if dsend > 0.0 {
                el += a * dsend;
                er += b * dsend;
            }
            !av.voice.is_done()
        });

        let (delay_l, delay_r) = self.delay.process(el, er);
        let mut rout = [0.0f32; 2];
        self.reverb.tick(&[rl, rr], &mut rout);

        self.sample_clock += 1;
        self.played.store(self.sample_clock, Ordering::Relaxed);
        (dl + delay_l + rout[0], dr + delay_r + rout[1])
    }
}

/// A running audio engine: owns the cpal stream and a scheduler thread.
pub struct Engine {
    _stream: cpal::Stream,
    pattern: Arc<RwLock<Pattern>>,
    cps: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    bank: Arc<RwLock<SampleBank>>,
    played: Arc<AtomicU64>,
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
        let cps = Arc::new(AtomicU64::new(0));
        store_f64(&cps, 0.5); // Strudel default cps
        let running = Arc::new(AtomicBool::new(true));
        let bank = Arc::new(RwLock::new(SampleBank::new()));

        let mut mixer = Mixer {
            rx,
            pending: Vec::new(),
            active: Vec::new(),
            sample_clock: 0,
            sample_rate,
            played: played.clone(),
            delay: StereoDelay::new(sample_rate, 1.0 / 6.0, 0.4),
            reverb: build_reverb(sample_rate),
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
            let cps = cps.clone();
            let running = running.clone();
            let played = played.clone();
            let bank = bank.clone();
            std::thread::spawn(move || {
                scheduler_loop(pattern, cps, running, played, bank, tx, sample_rate)
            });
        }

        Ok(Engine {
            _stream: stream,
            pattern,
            cps,
            running,
            bank,
            played,
            sample_rate,
        })
    }

    /// Load a directory of samples (subfolders become sound names).
    pub fn load_samples(&self, dir: impl AsRef<std::path::Path>) -> Result<usize, String> {
        self.bank.write().unwrap().load_dir(dir.as_ref())
    }

    /// The `samples(...)` loader: load from a `github:`/`bubo:` pseudo-URL, an
    /// http(s) URL to a `strudel.json`, a local `.json` map, or a local sample
    /// directory. Returns the number of samples registered.
    pub fn samples(&self, source: &str) -> Result<usize, String> {
        self.bank.write().unwrap().load_samples_source(source)
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
    pub fn set_cps(&self, cps: f64) {
        store_f64(&self.cps, cps);
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Total elapsed cycles since the stream started (fractional). The visualizer
    /// uses `position_cycles().fract()` as the within-cycle playhead.
    pub fn position_cycles(&self) -> f64 {
        let seconds = self.played.load(Ordering::Relaxed) as f64 / self.sample_rate as f64;
        seconds * load_f64(&self.cps)
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

#[allow(clippy::too_many_arguments)]
fn scheduler_loop(
    pattern: Arc<RwLock<Pattern>>,
    cps: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    played: Arc<AtomicU64>,
    bank: Arc<RwLock<SampleBank>>,
    tx: Sender<NoteEvent>,
    sample_rate: f32,
) {
    let lookahead = 0.1_f64; // seconds scheduled ahead of the audio clock
    let mut scheduled_cycle = 0.0_f64;
    while running.load(Ordering::Relaxed) {
        let cps_now = load_f64(&cps);
        let now = played.load(Ordering::Relaxed) as f64 / sample_rate as f64;
        let target_cycle = (now + lookahead) * cps_now;
        if target_cycle > scheduled_cycle {
            let pat = pattern.read().unwrap().clone();
            let bank = bank.read().unwrap();
            for ev in collect_events(&pat, cps_now, scheduled_cycle, target_cycle, &bank) {
                let _ = tx.send(ev);
            }
            scheduled_cycle = target_cycle;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut mixer = Mixer {
            rx,
            pending: Vec::new(),
            active: Vec::new(),
            sample_clock: 0,
            sample_rate: 44100.0,
            played: Arc::new(AtomicU64::new(0)),
            delay: StereoDelay::new(44100.0, 1.0 / 6.0, 0.4),
            reverb: build_reverb(44100.0),
        };
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
        let mut mixer = Mixer {
            rx,
            pending: Vec::new(),
            active: Vec::new(),
            sample_clock: 0,
            sample_rate: 44100.0,
            played: Arc::new(AtomicU64::new(0)),
            delay: StereoDelay::new(44100.0, 1.0 / 6.0, 0.4),
            reverb: build_reverb(44100.0),
        };
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
    fn mixer_renders_a_scheduled_note() {
        // Drive a Mixer directly (no audio device) and confirm a scheduled
        // note produces non-silent output once its onset passes.
        let (tx, rx) = crossbeam_channel::unbounded::<NoteEvent>();
        let mut mixer = Mixer {
            rx,
            pending: Vec::new(),
            active: Vec::new(),
            sample_clock: 0,
            sample_rate: 44100.0,
            played: Arc::new(AtomicU64::new(0)),
            delay: StereoDelay::new(44100.0, 1.0 / 6.0, 0.4),
            reverb: build_reverb(44100.0),
        };
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
}
