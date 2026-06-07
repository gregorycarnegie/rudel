pub trait VoiceLike: Send {
    /// Render the next stereo sample.
    fn tick(&mut self) -> (f32, f32);
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
