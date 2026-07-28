//! Lock-free output rings for scope and spectrum visualizers.

use crate::sync::{read_lock, write_lock};
use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
};
/// Ring-buffer capacity of [`ScopeTap`] (power of two; ~0.19s at 44.1kHz).
pub(crate) const SCOPE_TAP_LEN: usize = 8192;

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
    pub(crate) fn new() -> ScopeTap {
        ScopeTap {
            buf: (0..SCOPE_TAP_LEN).map(|_| AtomicU32::new(0)).collect(),
            written: AtomicU64::new(0),
        }
    }

    /// Append samples to the ring (audio thread only).
    pub(crate) fn write(&self, samples: impl ExactSizeIterator<Item = f32>) {
        let mask = (self.buf.len() - 1) as u64;
        let start = self.written.load(Ordering::Relaxed);
        let n = samples.len() as u64;
        for (i, s) in samples.enumerate() {
            self.buf[((start + i as u64) & mask) as usize].store(s.to_bits(), Ordering::Relaxed);
        }
        self.written.store(start + n, Ordering::Release);
    }

    /// Append the mono mix of rendered stereo frames (audio thread only).
    pub(crate) fn write_frames(&self, frames: &[(f32, f32)]) {
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
    pub(crate) named: RwLock<HashMap<String, Arc<ScopeTap>>>,
}

impl ScopeTaps {
    pub(crate) fn new() -> ScopeTaps {
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
