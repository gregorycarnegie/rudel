// lib.rs - the dough synth engine, for `.dough()`.
//
// Upstream's `.dough()` plays a pattern through dough (https://codeberg.org/uzu/dough)
// instead of superdough. Rather than imitate its sound in Rust, this crate
// compiles the engine's own C source (vendor/dough.h, see vendor/NOTICE.md) and
// lets the mixer render it: an event goes in as the same `key/value/...`
// string dough's JavaScript driver sends, and stereo frames come out.
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::ffi::{CString, c_char, c_double, c_float, c_int};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

unsafe extern "C" {
    fn dough_init(
        sample_rate: c_int,
        max_voices: c_int,
        max_orbits: c_int,
        max_delay_time: c_int,
        max_pcm: c_int,
        max_events: c_int,
        max_files: c_int,
    );
    fn dough_evaluate(msg: *const c_char);
    fn dough_render(out: *mut c_float, frames: c_int, start_time: c_double);
}

// What dough.js hands `dough_init` in the browser, so voice stealing and the
// delay line behave as they do there.
const MAX_VOICES: c_int = 32;
const MAX_ORBITS: c_int = 1;
const MAX_DELAY_TIME: c_int = 1;
const MAX_EVENTS: c_int = 64;
const MAX_FILES: c_int = 128;
// shortcut: no sample memory, so only dough's own oscillators and noises
// sound; give this the browser's size once samples are registered with it.
const MAX_PCM: c_int = 0;

/// The longest message the engine reads (`EVENT_INPUT_SIZE`), terminator
/// included. A longer one would be cut mid-pair, so it is not sent at all.
const MAX_MESSAGE: usize = 1024;

/// Set while a [`Dough`] exists. The engine keeps its state in C globals, so
/// there is one per process.
static TAKEN: AtomicBool = AtomicBool::new(false);
/// The sample rate the engine's memory was last laid out for; 0 before the
/// first [`Dough::acquire`].
static LAID_OUT_FOR: AtomicU32 = AtomicU32::new(0);

/// The process's one dough engine. Whoever holds this may play and render it.
pub struct Dough {
    sample_rate: f64,
    /// Frames rendered so far: the engine's clock.
    clock: u64,
    /// Interleaved stereo, reused between calls.
    scratch: Vec<f32>,
}

impl Dough {
    /// Take the engine, silent and at time zero, or `None` while another
    /// `Dough` is alive.
    pub fn acquire(sample_rate: u32) -> Option<Dough> {
        TAKEN
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        let mut dough = Dough {
            sample_rate: f64::from(sample_rate),
            clock: 0,
            scratch: Vec::new(),
        };
        if LAID_OUT_FOR.swap(sample_rate, Ordering::AcqRel) == sample_rate {
            // Same layout as last time: drop its voices and pending events.
            dough.evaluate("dough/reset");
        } else {
            // SAFETY: `TAKEN` makes this the only caller into the engine. A
            // second layout (a different sample rate) leaks the first one's
            // block: the engine has no teardown, and a device rate changes
            // rarely enough for that to be the smaller evil.
            unsafe {
                dough_init(
                    sample_rate as c_int,
                    MAX_VOICES,
                    MAX_ORBITS,
                    MAX_DELAY_TIME,
                    MAX_PCM,
                    MAX_EVENTS,
                    MAX_FILES,
                );
            }
        }
        Some(dough)
    }

    /// Hand the engine one event, written as dough's driver writes it:
    /// `dough/play/s/saw/note/60/duration/0.5`. With no `time` pair it takes
    /// effect at the next rendered frame. Returns false, sending nothing, for
    /// a message the engine could not read whole.
    pub fn evaluate(&mut self, message: &str) -> bool {
        if message.len() >= MAX_MESSAGE {
            return false;
        }
        let Ok(message) = CString::new(message) else {
            return false; // an interior NUL
        };
        // SAFETY: holding `self` is holding the engine; the string outlives
        // the call, which copies it.
        unsafe { dough_evaluate(message.as_ptr()) };
        true
    }

    /// Render the next `out.len()` frames and add them to `out`.
    pub fn render_into(&mut self, out: &mut [(f32, f32)]) {
        let frames = out.len();
        if self.scratch.len() < frames * 2 {
            self.scratch.resize(frames * 2, 0.0);
        }
        let start = self.clock as f64 / self.sample_rate;
        // SAFETY: the buffer holds `frames` stereo frames, which is what
        // `dough_render` writes.
        unsafe { dough_render(self.scratch.as_mut_ptr(), frames as c_int, start) };
        self.clock += frames as u64;
        for (frame, pair) in out.iter_mut().zip(self.scratch.as_chunks::<2>().0) {
            frame.0 += pair[0];
            frame.1 += pair[1];
        }
    }
}

impl Drop for Dough {
    fn drop(&mut self) {
        TAKEN.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// One engine per process: the tests take turns with it.
    static ENGINE: Mutex<()> = Mutex::new(());

    fn peak(dough: &mut Dough, frames: usize) -> f32 {
        let mut out = vec![(0.0f32, 0.0f32); frames];
        dough.render_into(&mut out);
        out.iter().fold(0.0, |m, f| m.max(f.0.abs()).max(f.1.abs()))
    }

    #[test]
    fn a_sine_plays_at_doughs_own_level_and_stops_after_its_duration() {
        let _turn = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
        let mut dough = Dough::acquire(44_100).expect("the engine is free");
        assert_eq!(peak(&mut dough, 512), 0.0, "silent before any event");
        assert!(dough.evaluate("dough/play/s/sine/freq/330/duration/0.25"));
        // GAIN_SINE (0.2) times GAIN_MIX (0.5), the engine's own constants.
        let level = peak(&mut dough, 4_410);
        assert!((level - 0.1).abs() < 0.005, "peak {level}");
        // Past its duration and release it is silent again.
        peak(&mut dough, 44_100);
        assert_eq!(peak(&mut dough, 4_410), 0.0, "still sounding a second on");
    }

    #[test]
    fn there_is_one_engine_and_the_next_holder_gets_it_silent() {
        let _turn = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
        let mut first = Dough::acquire(44_100).expect("the engine is free");
        assert!(Dough::acquire(44_100).is_none(), "a second engine");
        assert!(first.evaluate("dough/play/s/saw/freq/110"));
        assert!(peak(&mut first, 512) > 0.0);
        drop(first);
        // An endless note was still sounding; the new holder must not hear it.
        let mut second = Dough::acquire(44_100).expect("released on drop");
        assert_eq!(peak(&mut second, 4_410), 0.0);
    }

    #[test]
    fn a_message_too_long_for_the_engine_is_refused() {
        let _turn = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
        let mut dough = Dough::acquire(44_100).expect("the engine is free");
        let long = format!("dough/play/s/sine/x/{}", "9".repeat(MAX_MESSAGE));
        assert!(!dough.evaluate(&long));
        assert_eq!(peak(&mut dough, 512), 0.0);
    }
}
