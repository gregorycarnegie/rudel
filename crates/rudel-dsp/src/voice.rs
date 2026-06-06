pub trait VoiceLike: Send {
    /// Render the next stereo sample.
    fn tick(&mut self) -> (f32, f32);
    fn is_done(&self) -> bool;
    /// Reverb (`room`) send amount.
    fn room(&self) -> f32;
    /// Delay (`delay`) send amount.
    fn delay_send(&self) -> f32;
}
