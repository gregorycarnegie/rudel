// lib.rs - the dough synth engine, for `.dough()`.
//
// Upstream's `.dough()` plays a pattern through dough (https://codeberg.org/uzu/dough)
// instead of superdough. This crate is a Rust port of dough's engine
// (dough-synth 0.4.1, the version Strudel pins), and the mixer renders it: an
// event goes in as the same `key/value/...` string dough's JavaScript driver
// sends, and stereo frames come out. The output is bit-identical to the
// browser's WebAssembly build of the C (tests/wasm_golden.rs), and to the C
// itself (tests/c_reference.rs, with the `c-reference` feature).
// SPDX-License-Identifier: AGPL-3.0-or-later

mod dsp;
mod engine;
mod event;

#[cfg(feature = "c-reference")]
#[doc(hidden)]
pub mod c_reference;

use engine::{Engine, Limits};

/// What dough.js hands `dough_init` in the browser, so voice stealing, the
/// schedule and the delay line behave as they do there.
const LIMITS: Limits = Limits {
    voices: 32,
    delay_seconds: 1,
    events: 64,
    files: 128,
    // 196 MB of floats.
    pcm: (1024 * 1024 * 196) / 4,
};

/// The pitch a registered file is taken to be at, which dough.js gives every
/// sample: a note of this frequency plays the file at its own rate.
const FILE_FREQ: f32 = 65.46;

/// The longest message dough reads (`EVENT_INPUT_SIZE`), terminator included.
/// A longer one would be cut mid-pair, so it is not sent at all.
const MAX_MESSAGE: usize = 1024;

/// A dough engine.
pub struct Dough {
    engine: Engine,
    /// The keys of the registered sample files; a file's index is its place.
    files: Vec<String>,
}

impl Dough {
    /// A silent engine at time zero.
    pub fn new(sample_rate: u32) -> Dough {
        Dough {
            engine: Engine::new(sample_rate, &LIMITS),
            files: Vec::new(),
        }
    }

    /// Hand the engine one event, written as dough's driver writes it:
    /// `dough/play/s/saw/note/60/duration/0.5`. With no `time` pair it takes
    /// effect at the next rendered frame; a `time` is seconds on the engine's
    /// clock, which `dough/reset` restarts. Returns false, doing nothing, for a
    /// message dough could not read whole.
    pub fn evaluate(&mut self, message: &str) -> bool {
        if message.len() >= MAX_MESSAGE || message.contains('\0') {
            return false;
        }
        self.engine.evaluate(message.as_bytes());
        true
    }

    /// The engine's file index for the sample `key` names, copying `pcm`
    /// (interleaved, at the engine's sample rate or compensated for by the
    /// event's `speed`) into the engine the first time the key is seen. An
    /// event plays it with a `file/<index>` pair. `None` once the engine has
    /// no room for another file.
    pub fn sample_file(&mut self, key: &str, pcm: &[f32], channels: usize) -> Option<i32> {
        if let Some(index) = self.files.iter().position(|k| k == key) {
            return Some(index as i32);
        }
        let index = self.files.len();
        if !self.engine.register_pcm(index, pcm, channels, FILE_FREQ) {
            return None;
        }
        self.files.push(key.to_string());
        Some(index as i32)
    }

    /// Render the next `out.len()` frames and add them to `out`.
    pub fn render_into(&mut self, out: &mut [(f32, f32)]) {
        self.engine.render_add(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(dough: &mut Dough, frames: usize) -> f32 {
        let mut out = vec![(0.0f32, 0.0f32); frames];
        dough.render_into(&mut out);
        out.iter().fold(0.0, |m, f| m.max(f.0.abs()).max(f.1.abs()))
    }

    #[test]
    fn a_sine_plays_at_doughs_own_level_and_stops_after_its_duration() {
        let mut dough = Dough::new(44_100);
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
    fn engines_are_independent_and_reset_silences_one() {
        let mut first = Dough::new(44_100);
        let mut second = Dough::new(44_100);
        assert!(first.evaluate("dough/play/s/saw/freq/110"));
        assert!(peak(&mut first, 512) > 0.0);
        assert_eq!(peak(&mut second, 512), 0.0, "one engine heard the other");
        // An endless note is still sounding; a reset stops it.
        assert!(first.evaluate("dough/reset"));
        assert_eq!(peak(&mut first, 4_410), 0.0);
    }

    #[test]
    fn a_registered_sample_plays_and_keeps_its_index() {
        let mut dough = Dough::new(44_100);
        // Half a second of a full-scale square, as mono PCM.
        let pcm: Vec<f32> = (0..22_050)
            .map(|i| if (i / 50) % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let index = dough
            .sample_file("square:0", &pcm, 1)
            .expect("room for a file");
        assert_eq!(dough.sample_file("square:0", &pcm, 1), Some(index));
        assert_ne!(dough.sample_file("other:0", &pcm, 1), Some(index));
        assert!(dough.evaluate(&format!("dough/play/s/square:0/file/{index}")));
        assert!(peak(&mut dough, 4_410) > 0.1, "the sample is silent");
        // Files outlive a reset, as the engine's sample memory does in dough.
        assert!(dough.evaluate("dough/reset"));
        assert_eq!(dough.sample_file("square:0", &pcm, 1), Some(index));
    }

    #[test]
    fn a_message_dough_cannot_read_whole_is_refused() {
        let mut dough = Dough::new(44_100);
        let long = format!("dough/play/s/sine/x/{}", "9".repeat(MAX_MESSAGE));
        assert!(!dough.evaluate(&long));
        assert!(!dough.evaluate("dough/play\0/s/sine"));
        assert_eq!(peak(&mut dough, 512), 0.0);
    }
}
