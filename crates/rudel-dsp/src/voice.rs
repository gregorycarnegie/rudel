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

    fn is_done(&self) -> bool;
    /// Reverb (`room`) send amount.
    fn room(&self) -> f32;
    /// Delay (`delay`) send amount.
    fn delay_send(&self) -> f32;
    /// Dry (direct) signal level (`dry`), 0..1. Defaults to full (`1.0`); the
    /// reverb/delay sends are unaffected, so `dry(0)` leaves only the wet signal.
    fn dry(&self) -> f32 {
        1.0
    }
}
