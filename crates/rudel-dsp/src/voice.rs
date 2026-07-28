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
}
