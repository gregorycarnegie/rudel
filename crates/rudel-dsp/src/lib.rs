// rudel-dsp - synthesis voices for Rudel.
// Phase-3 voices are hand-rolled (oscillator + ADSR + pan) so they're
// deterministic and testable offline; fundsp powers effects in a later phase.
// Param model mirrors strudel/packages/superdough/synth.mjs.
// SPDX-License-Identifier: AGPL-3.0-or-later

mod drum;
mod envelope;
mod filter;
mod fm;
mod modulator;
mod oscillator;
mod params;
mod pitch;
mod postfx;
mod sampler;
mod spec;
mod synth;
mod voice;
mod zzfx;

pub use drum::{DrumKind, DrumParams, DrumVoice};
pub use envelope::{Adsr, adsr_value};
pub use filter::FilterParams;
pub use fm::{FmOp, FmSpec};
pub use modulator::{Lfo, LfoConfig, waveshape};
pub use oscillator::{NoiseKind, Waveform};
pub use params::VoiceParams;
pub use pitch::{mtof, note_name_to_midi, note_to_freq};
pub use postfx::{DistortAlgo, PostFx, PostFxVoice, Vowel};
pub use sampler::{Sample, SamplerParams, SamplerVoice};
pub use spec::VoiceSpec;
pub use synth::Voice;
pub use voice::VoiceLike;
pub use zzfx::{ZzfxParams, ZzfxSynth, ZzfxVoice, build_samples};

#[cfg(test)]
mod tests;
