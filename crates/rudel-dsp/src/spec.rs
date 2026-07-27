use crate::{
    drum::{DrumParams, DrumVoice},
    modulator::{ModOwner, ModSpec, ModSpecs, ModTarget},
    params::VoiceParams,
    postfx::{PostFx, PostFxVoice},
    sampler::{SamplerParams, SamplerVoice},
    synth::Voice,
    voice::VoiceLike,
    zzfx::{ZzfxParams, ZzfxVoice},
};

pub enum VoiceSpec {
    Synth(Box<VoiceParams>),
    Sampler(SamplerParams),
    Drum(DrumParams),
    Zzfx(Box<ZzfxParams>),
}

impl VoiceSpec {
    pub fn into_voice(self, sample_rate: f32) -> Box<dyn VoiceLike> {
        self.into_voice_with_mods(sample_rate, &[])
    }

    fn into_voice_with_mods(self, sample_rate: f32, mods: &[ModSpec]) -> Box<dyn VoiceLike> {
        match self {
            VoiceSpec::Synth(p) => Box::new(Voice::with_mods(*p, sample_rate, mods)),
            VoiceSpec::Sampler(p) => Box::new(SamplerVoice::with_mods(p, sample_rate, mods)),
            // The drum and ZZFX voices render from a fixed recipe with no
            // per-sample parameters to offset, so they take no modulators.
            VoiceSpec::Drum(p) => Box::new(DrumVoice::new(p, sample_rate)),
            VoiceSpec::Zzfx(p) => Box::new(ZzfxVoice::new(*p, sample_rate)),
        }
    }

    /// Build the voice and, if any post-effects are active, wrap it in a
    /// [`PostFxVoice`].
    pub fn into_voice_with_fx(self, sample_rate: f32, fx: PostFx) -> Box<dyn VoiceLike> {
        self.into_modulated_voice(sample_rate, fx, &ModSpecs::default())
    }

    /// Build the voice with its post-effects and its modulators, each side
    /// taking the specs it can consume.
    pub fn into_modulated_voice(
        self,
        sample_rate: f32,
        fx: PostFx,
        mods: &ModSpecs,
    ) -> Box<dyn VoiceLike> {
        let post = mods.for_owner(ModOwner::PostFx);
        let voice = self.into_voice_with_mods(sample_rate, mods.for_owner(ModOwner::Voice));
        if fx.is_active() || !post.is_empty() {
            Box::new(PostFxVoice::with_mods(voice, fx, sample_rate, post))
        } else {
            voice
        }
    }

    /// The current value of a modulatable control, which a relative `depth`
    /// scales. superdough reads this off the target `AudioParam`; here it comes
    /// from the resolved voice params, with `fx` covering the post-fx targets.
    pub fn mod_base(&self, target: ModTarget, fx: &PostFx) -> f32 {
        match target {
            ModTarget::Frequency => match self {
                VoiceSpec::Synth(p) => p.freq,
                _ => 0.0,
            },
            ModTarget::Gain => match self {
                VoiceSpec::Synth(p) => p.gain,
                VoiceSpec::Sampler(p) => p.gain,
                VoiceSpec::Drum(p) => p.gain,
                VoiceSpec::Zzfx(p) => p.gain,
            },
            ModTarget::Cutoff => match self {
                VoiceSpec::Synth(p) => p.lp.freq.unwrap_or(0.0),
                VoiceSpec::Sampler(p) => p.cutoff.unwrap_or(0.0),
                _ => 0.0,
            },
            ModTarget::Resonance => match self {
                VoiceSpec::Synth(p) => p.lp.q,
                VoiceSpec::Sampler(p) => p.resonance,
                _ => 0.0,
            },
            ModTarget::Hcutoff => self.synth_param(|p| p.hp.freq.unwrap_or(0.0)),
            ModTarget::Hresonance => self.synth_param(|p| p.hp.q),
            ModTarget::Bandf => self.synth_param(|p| p.bp.freq.unwrap_or(0.0)),
            ModTarget::Bandq => self.synth_param(|p| p.bp.q),
            _ => fx.mod_base(target),
        }
    }

    fn synth_param(&self, f: impl Fn(&VoiceParams) -> f32) -> f32 {
        match self {
            VoiceSpec::Synth(p) => f(p),
            _ => 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Waveshaping / bitcrush / decimation post-effects (superdough crush/shape/
// distort/coarse worklets). Applied per voice, after the voice renders.
