use super::base::control;
use crate::pattern::Pattern;
use crate::transforms::IntoPattern;

// Controls whose Strudel key can't be a Rust fn name (keywords like `loop`,
// camelCase keys like `loopBegin`). The builder fn is snake_case while still
// writing the Strudel control key.
macro_rules! named_controls {
    ($($fn:ident => $key:literal),* $(,)?) => {
        $(
            #[doc = concat!("The `", $key, "` control.")]
            pub fn $fn(pat: impl IntoPattern) -> Pattern {
                control($key, pat.into_pattern())
            }
        )*
        impl Pattern {
            $(
                #[doc = concat!("Set the `", $key, "` control, keeping this pattern's structure.")]
                pub fn $fn(&self, x: impl IntoPattern) -> Pattern {
                    self.set($fn(x))
                }
            )*
        }

        /// `(key, builder)` pairs for the controls above; used by
        /// [`control_name`](crate::controls::control_name) to resolve canonical
        /// control keys.
        pub(super) static NAMED_CONTROL_BUILDERS: &[(&str, fn(Pattern) -> Pattern)] = &[
            $( ($key, |p: Pattern| $fn(p)) ),*
        ];
    };
}

named_controls!(
    loop_play => "loop",
    loop_begin => "loopBegin",
    loop_end => "loopEnd",
    steps_per_octave => "stepsPerOctave",
    octave_r => "octaveR",
    ctl_num => "ctlNum",
    prog_num => "progNum",
    poly_touch => "polyTouch",
    compressor_knee => "compressorKnee",
    compressor_ratio => "compressorRatio",
    compressor_attack => "compressorAttack",
    compressor_release => "compressorRelease",
    frame_rate => "frameRate",
    song_ptr => "songPtr",
    delta_slide => "deltaSlide",
    pitch_jump => "pitchJump",
    pitch_jump_time => "pitchJumpTime",
    fade_time => "fadeTime",
    fade_in_time => "fadeInTime",
    byte_beat_expression => "byteBeatExpression",
    byte_beat_start_time => "byteBeatStartTime",
    fx_release => "FXrelease",
);
