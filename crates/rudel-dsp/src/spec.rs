use crate::drum::{DrumParams, DrumVoice};
use crate::params::VoiceParams;
use crate::postfx::{PostFx, PostFxVoice};
use crate::sampler::{SamplerParams, SamplerVoice};
use crate::synth::Voice;
use crate::voice::VoiceLike;

pub enum VoiceSpec {
    Synth(Box<VoiceParams>),
    Sampler(SamplerParams),
    Drum(DrumParams),
}

impl VoiceSpec {
    pub fn into_voice(self, sample_rate: f32) -> Box<dyn VoiceLike> {
        match self {
            VoiceSpec::Synth(p) => Box::new(Voice::new(*p, sample_rate)),
            VoiceSpec::Sampler(p) => Box::new(SamplerVoice::new(p, sample_rate)),
            VoiceSpec::Drum(p) => Box::new(DrumVoice::new(p, sample_rate)),
        }
    }

    /// Build the voice and, if any post-effects are active, wrap it in a
    /// [`PostFxVoice`].
    pub fn into_voice_with_fx(self, sample_rate: f32, fx: PostFx) -> Box<dyn VoiceLike> {
        let voice = self.into_voice(sample_rate);
        if fx.is_active() {
            Box::new(PostFxVoice::new(voice, fx, sample_rate))
        } else {
            voice
        }
    }
}

// ---------------------------------------------------------------------------
// Waveshaping / bitcrush / decimation post-effects (superdough crush/shape/
// distort/coarse worklets). Applied per voice, after the voice renders.
