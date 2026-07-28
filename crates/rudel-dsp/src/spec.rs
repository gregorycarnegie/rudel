use crate::{
    bus::{BusParams, BusVoice},
    bytebeat::{ByteBeatParams, ByteBeatVoice},
    drum::{DrumParams, DrumVoice},
    filter::FilterSet,
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
    ByteBeat(Box<ByteBeatParams>),
    Bus(BusParams),
}

impl VoiceSpec {
    pub fn into_voice(self, sample_rate: f32) -> Box<dyn VoiceLike> {
        self.into_voice_with_mods(sample_rate, &[])
    }

    fn into_voice_with_mods(self, sample_rate: f32, mods: &[ModSpec]) -> Box<dyn VoiceLike> {
        match self {
            VoiceSpec::Synth(p) => Box::new(Voice::with_mods(*p, sample_rate, mods)),
            VoiceSpec::Sampler(p) => Box::new(SamplerVoice::with_mods(p, sample_rate, mods)),
            // These four render from a fixed recipe, so of the voice-side
            // targets only the filter chain is theirs to offset; the rest of
            // their bank ticks and goes unread.
            VoiceSpec::Drum(p) => Box::new(DrumVoice::with_mods(p, sample_rate, mods)),
            VoiceSpec::Zzfx(p) => Box::new(ZzfxVoice::with_mods(*p, sample_rate, mods)),
            VoiceSpec::ByteBeat(p) => Box::new(ByteBeatVoice::with_mods(*p, sample_rate, mods)),
            VoiceSpec::Bus(p) => Box::new(BusVoice::with_mods(p, sample_rate, mods)),
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
                VoiceSpec::ByteBeat(p) => p.gain,
                VoiceSpec::Bus(p) => p.gain,
            },
            ModTarget::Cutoff => match self {
                VoiceSpec::Sampler(p) => p.cutoff.unwrap_or(0.0),
                _ => self.filter_param(|f| f.lp.freq.unwrap_or(0.0)),
            },
            ModTarget::Resonance => match self {
                VoiceSpec::Sampler(p) => p.resonance,
                _ => self.filter_param(|f| f.lp.q),
            },
            ModTarget::Hcutoff => self.filter_param(|f| f.hp.freq.unwrap_or(0.0)),
            ModTarget::Hresonance => self.filter_param(|f| f.hp.q),
            ModTarget::Bandf => self.filter_param(|f| f.bp.freq.unwrap_or(0.0)),
            ModTarget::Bandq => self.filter_param(|f| f.bp.q),
            _ => fx.mod_base(target),
        }
    }

    /// Read one of the voice's filter slots. Every voice type but the sampler
    /// (whose filters predate `FilterSet`) carries the same three slots.
    fn filter_param(&self, f: impl Fn(&FilterSet) -> f32) -> f32 {
        match self {
            VoiceSpec::Synth(p) => f(&FilterSet {
                lp: p.lp,
                hp: p.hp,
                bp: p.bp,
            }),
            VoiceSpec::Drum(p) => f(&p.filters),
            VoiceSpec::Zzfx(p) => f(&p.filters),
            VoiceSpec::ByteBeat(p) => f(&p.filters),
            VoiceSpec::Bus(p) => f(&p.filters),
            VoiceSpec::Sampler(_) => 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Waveshaping / bitcrush / decimation post-effects (superdough crush/shape/
// distort/coarse worklets). Applied per voice, after the voice renders.
