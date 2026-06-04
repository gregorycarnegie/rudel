// rudel-audio - real-time clock, lookahead scheduler and cpal output.
// The scheduler maps cycle time to the audio sample clock and feeds timed
// note events to a mixer running in the audio callback.
// Clock approach mirrors strudel/packages/core/{zyklus,cyclist}.mjs.
// SPDX-License-Identifier: AGPL-3.0-or-later

pub mod events;

pub use events::{NoteEvent, collect_events, to_control_map};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender};
use rudel_core::Pattern;
use rudel_dsp::Voice;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

fn store_f64(a: &AtomicU64, v: f64) {
    a.store(v.to_bits(), Ordering::Relaxed);
}
fn load_f64(a: &AtomicU64) -> f64 {
    f64::from_bits(a.load(Ordering::Relaxed))
}

/// Mixes active voices and starts new ones as their onset time arrives. Lives
/// in the audio callback.
struct Mixer {
    rx: Receiver<NoteEvent>,
    pending: Vec<NoteEvent>,
    active: Vec<Voice>,
    sample_clock: u64,
    sample_rate: f32,
    played: Arc<AtomicU64>,
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
                self.active.push(Voice::new(ev.params, self.sample_rate));
            } else {
                i += 1;
            }
        }

        let (mut l, mut r) = (0.0f32, 0.0f32);
        self.active.retain_mut(|v| {
            let (a, b) = v.tick();
            l += a;
            r += b;
            !v.is_done()
        });

        self.sample_clock += 1;
        self.played.store(self.sample_clock, Ordering::Relaxed);
        (l, r)
    }
}

/// A running audio engine: owns the cpal stream and a scheduler thread.
pub struct Engine {
    _stream: cpal::Stream,
    pattern: Arc<std::sync::RwLock<Pattern>>,
    cps: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
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

        let (tx, rx) = crossbeam_channel::unbounded::<NoteEvent>();
        let played = Arc::new(AtomicU64::new(0));
        let pattern = Arc::new(std::sync::RwLock::new(rudel_core::silence()));
        let cps = Arc::new(AtomicU64::new(0));
        store_f64(&cps, 0.5); // Strudel default cps
        let running = Arc::new(AtomicBool::new(true));

        let mut mixer = Mixer {
            rx,
            pending: Vec::new(),
            active: Vec::new(),
            sample_clock: 0,
            sample_rate,
            played: played.clone(),
        };

        let err_fn = |e| eprintln!("[rudel-audio] stream error: {e}");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _| write_frames(data, channels, &mut mixer),
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &config.into(),
                move |data: &mut [i16], _| write_frames(data, channels, &mut mixer),
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                &config.into(),
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
            std::thread::spawn(move || {
                scheduler_loop(pattern, cps, running, played, tx, sample_rate)
            });
        }

        Ok(Engine {
            _stream: stream,
            pattern,
            cps,
            running,
            sample_rate,
        })
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
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
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

fn scheduler_loop(
    pattern: Arc<std::sync::RwLock<Pattern>>,
    cps: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    played: Arc<AtomicU64>,
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
            for ev in collect_events(&pat, cps_now, scheduled_cycle, target_cycle) {
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
    use rudel_core::Frac;

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
        };
        let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)));
        let events = collect_events(&pat, 1.0, 0.0, 1.0);
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
        let _ = Frac::zero();
    }
}
