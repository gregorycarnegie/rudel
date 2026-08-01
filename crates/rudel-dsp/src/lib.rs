// rudel-dsp - synthesis voices for Rudel.
// Phase-3 voices are hand-rolled (oscillator + ADSR + pan) so they're
// deterministic and testable offline; fundsp powers effects in a later phase.
// Param model mirrors strudel/packages/superdough/synth.mjs.
// SPDX-License-Identifier: AGPL-3.0-or-later

mod bus;
mod bytebeat;
mod convolver;
mod drum;
mod envelope;
mod fft;
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
mod vocoder;
mod voice;
mod wavetable;
mod zzfx;

pub use bus::{BusParams, BusVoice, DelayConfig, Djf, Duck, DuckEnv, OrbitSend, ReverbConfig};
pub use bytebeat::{ByteBeatExpr, ByteBeatParams, ByteBeatVoice, DEFAULT_BEATS};
pub use convolver::{Convolver, ImpulseResponse, adjust_length, generate_reverb_ir};
pub use drum::{DrumKind, DrumParams, DrumVoice};
pub use envelope::{Adsr, adsr_value, adsr_values};
pub use filter::{FilterModel, FilterParams, FilterSet, Ladder, VoiceFilters};
pub use fm::{FmOp, FmSpec};
pub use modulator::{
    EnvConfig, Lfo, LfoConfig, ModBank, ModContext, ModEnv, ModOwner, ModSpec, ModSpecs, ModTarget,
    waveshape,
};
pub use oscillator::{NoiseKind, Waveform};
pub use params::VoiceParams;
pub use pitch::{PitchMod, mtof, note_name_to_midi, note_to_freq};
pub use postfx::{DistortAlgo, PostFx, PostFxVoice, TransientShaper, Vowel};
pub use sampler::{Sample, SamplerParams, SamplerVoice};
pub use spec::VoiceSpec;
pub use synth::Voice;
pub use vocoder::{PhaseVocoder, StretchStage};
pub use voice::VoiceLike;
pub use wavetable::{WarpMode, WaveTable, WavetableOsc, warp_phase};
pub use zzfx::{ZzfxParams, ZzfxSynth, ZzfxVoice, build_samples};

#[cfg(test)]
mod tests;
