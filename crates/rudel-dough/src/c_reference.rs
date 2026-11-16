// c_reference.rs - dough's C engine (vendor/dough.h), for comparison only.
//
// Rudel plays `.dough()` through the Rust port in this crate. The C it was
// ported from is kept, behind the `c-reference` feature, so the port can be
// checked against it (tests/c_reference.rs) and timed against it
// (benches/engine.rs). It keeps its state in C globals, so there is one per
// process: `CDough::acquire` fails while another is alive.
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::ffi::{CString, c_char, c_double, c_float, c_int};
use std::sync::Mutex;
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
    fn dough_register_pcm(
        file_index: c_int,
        pcm: *const c_float,
        frames: c_int,
        channels: c_int,
        freq: c_float,
    ) -> c_int;
}

// What dough.js hands `dough_init` in the browser, so voice stealing and the
// delay line behave as they do there.
const MAX_VOICES: c_int = 32;
const MAX_ORBITS: c_int = 1;
const MAX_DELAY_TIME: c_int = 1;
const MAX_EVENTS: c_int = 64;
const MAX_FILES: c_int = 128;
/// Sample memory, in floats: 196 MB, laid out with the engine.
const MAX_PCM: c_int = (1024 * 1024 * 196) / 4;

/// The pitch a registered file is taken to be at, which dough.js gives every
/// sample: a note of this frequency plays the file at its own rate.
const FILE_FREQ: c_float = 65.46;

/// The longest message the engine reads (`EVENT_INPUT_SIZE`), terminator
/// included. A longer one would be cut mid-pair, so it is not sent at all.
const MAX_MESSAGE: usize = 1024;

/// Set while a [`CDough`] exists. The engine keeps its state in C globals, so
/// there is one per process.
static TAKEN: AtomicBool = AtomicBool::new(false);
/// The sample rate the engine's memory was last laid out for; 0 before the
/// first [`CDough::acquire`].
static LAID_OUT_FOR: AtomicU32 = AtomicU32::new(0);

/// The keys of the sample files registered with the engine; a file's index is
/// its place here. They outlive any one [`CDough`], as the engine's memory does,
/// and go when that memory is laid out afresh.
static FILES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The process's one C dough engine. Whoever holds this may play and render it.
pub struct CDough {
    sample_rate: f64,
    /// Frames rendered so far: the engine's clock.
    clock: u64,
    /// Interleaved stereo, reused between calls.
    scratch: Vec<f32>,
}

impl CDough {
    /// Take the engine, silent and at time zero, or `None` while another
    /// `CDough` is alive.
    pub fn acquire(sample_rate: u32) -> Option<CDough> {
        TAKEN
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        let mut dough = CDough {
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
            FILES.lock().unwrap_or_else(|e| e.into_inner()).clear();
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

    /// The engine's file index for the sample `key` names, copying `pcm`
    /// (interleaved, at the engine's sample rate or compensated for by the
    /// event's `speed`) into the engine the first time the key is seen. An
    /// event plays it with a `file/<index>` pair. `None` once the engine has
    /// no room for another file.
    pub fn sample_file(&mut self, key: &str, pcm: &[f32], channels: usize) -> Option<i32> {
        let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = files.iter().position(|k| k == key) {
            return Some(index as i32);
        }
        if channels == 0 || files.len() >= MAX_FILES as usize {
            return None;
        }
        let index = files.len() as c_int;
        let frames = (pcm.len() / channels) as c_int;
        // SAFETY: holding `self` is holding the engine; it copies
        // `frames * channels` floats, which `pcm` has.
        let status = unsafe {
            dough_register_pcm(index, pcm.as_ptr(), frames, channels as c_int, FILE_FREQ)
        };
        if status != 0 {
            return None; // out of sample memory
        }
        files.push(key.to_string());
        Some(index)
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

impl Drop for CDough {
    fn drop(&mut self) {
        TAKEN.store(false, Ordering::Release);
    }
}
