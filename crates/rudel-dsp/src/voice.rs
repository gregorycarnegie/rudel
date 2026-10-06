pub trait VoiceLike: Send {
    /// Render the next stereo sample.
    fn tick(&mut self) -> (f32, f32);

    /// Render `out_l.len()` stereo frames into `out_l`/`out_r` (which must be
    /// equal length). The default renders sample-by-sample via [`tick`](Self::tick);
    /// voices with vectorizable memoryless post-processing override it to run a
    /// whole block at once (amortizing dispatch and using SIMD). Semantically
    /// identical to calling `tick` `out_l.len()` times.
    fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        for (l, r) in out_l.iter_mut().zip(out_r.iter_mut()) {
            let (a, b) = self.tick();
            *l = a;
            *r = b;
        }
    }

    /// Hand bus `bus`'s stereo signal for the block about to be rendered to
    /// anything in this voice that reads it — a `bmod` modulator, or a
    /// [`BusVoice`](crate::BusVoice) playing the bus back as a source. The
    /// default is a no-op, since most voices read no bus at all.
    fn set_bus_input(&mut self, bus: i32, left: &[f32], right: &[f32]) {
        let _ = (bus, left, right);
    }

    fn is_done(&self) -> bool;

    /// Cents a modulator adds to this voice's `detune`, for a voice standing
    /// in for an `AudioBufferSourceNode` (a drum, a ZZFX sound), which
    /// [`RateVoice`] then plays back at that rate.
    fn detune_cents(&self) -> f32 {
        0.0
    }
}

/// Plays a voice back at a varying rate, as an `AudioBufferSourceNode` with a
/// modulated `detune` plays its buffer: `2^(cents/1200)` of the voice's own
/// samples per output sample, linearly interpolated. Rudel synthesizes the
/// drums and ZZFX sounds where superdough plays them from a buffer, so this is
/// how a `detune` modulator reaches them.
pub(crate) struct RateVoice {
    inner: Box<dyn VoiceLike>,
    /// Position between `prev` and `next`, 0..1.
    frac: f64,
    prev: (f32, f32),
    next: (f32, f32),
    cents: f32,
    started: bool,
}

impl RateVoice {
    pub(crate) fn new(inner: Box<dyn VoiceLike>) -> RateVoice {
        RateVoice {
            inner,
            frac: 0.0,
            prev: (0.0, 0.0),
            next: (0.0, 0.0),
            cents: 0.0,
            started: false,
        }
    }

    fn pull(&mut self) {
        self.prev = self.next;
        self.next = self.inner.tick();
        self.cents = self.inner.detune_cents();
    }
}

impl VoiceLike for RateVoice {
    fn tick(&mut self) -> (f32, f32) {
        if !self.started {
            self.started = true;
            self.next = self.inner.tick();
            self.cents = self.inner.detune_cents();
            self.prev = self.next;
        }
        let out = (
            self.prev.0 + (self.next.0 - self.prev.0) * self.frac as f32,
            self.prev.1 + (self.next.1 - self.prev.1) * self.frac as f32,
        );
        self.frac += 2f64.powf(self.cents as f64 / 1200.0);
        while self.frac >= 1.0 {
            self.frac -= 1.0;
            self.pull();
        }
        out
    }

    fn set_bus_input(&mut self, bus: i32, left: &[f32], right: &[f32]) {
        self.inner.set_bus_input(bus, left, right);
    }

    fn is_done(&self) -> bool {
        self.inner.is_done()
    }
}
