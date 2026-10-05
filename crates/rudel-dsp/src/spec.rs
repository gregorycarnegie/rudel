use crate::{
    bus::{BusParams, BusVoice},
    bytebeat::{ByteBeatParams, ByteBeatVoice},
    drum::{DrumParams, DrumVoice},
    filter::{FilterSet, FilterStageVoice},
    modulator::{ModOwner, ModSpec, ModSpecs, ModTarget},
    params::VoiceParams,
    postfx::{PostFx, PostFxVoice},
    sampler::{SamplerParams, SamplerVoice},
    synth::Voice,
    voice::VoiceLike,
    zzfx::{ZzfxParams, ZzfxVoice},
};

/// One stage of an `FX(...)` chain: the same effects a voice takes from its own
/// controls, read off the stage's control map instead.
#[derive(Clone, Default)]
pub struct FxStage {
    pub fx: PostFx,
    pub filters: FilterSet,
    /// The note's length, which drives this stage's filter envelope — the same
    /// value the voice under it was built with.
    pub duration: f32,
}

impl FxStage {
    pub fn from_controls(map: &rudel_core::ValueMap, duration: f32) -> FxStage {
        FxStage {
            fx: PostFx::from_controls(map),
            filters: FilterSet::from_controls(map),
            duration,
        }
    }
}

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
        self.into_chained_voice(sample_rate, &[], fx, mods)
    }

    /// Build the voice under an `FX(…)` chain: each stage is another rack of
    /// effects, applied in turn before `fx`.
    ///
    /// That is what upstream does with the list — `FX = [...FX, value]` and then
    /// one pass of the post-effects section per entry (superdough.mjs), the
    /// hap's own controls last. Here each pass is another wrapper around the one
    /// before it, so `chain[0]` sits nearest the source and `fx` ends up
    /// outermost, which is the order `.FX(a).FX(b)` reads in.
    ///
    /// ponytail: insert effects only. A stage's own `delay`/`room` sends and its
    /// `lfo`/`env` modulators stay with the main controls, because both are
    /// resolved once per event, outside the voice — upstream indexes them per
    /// stage with `fxi`. Give the stages their own `OrbitSend` if a tune ever
    /// wants a different delay in two places at once.
    pub fn into_chained_voice(
        self,
        sample_rate: f32,
        chain: &[FxStage],
        fx: PostFx,
        mods: &ModSpecs,
    ) -> Box<dyn VoiceLike> {
        let post = mods.for_owner(ModOwner::PostFx);
        let mut voice = self.into_voice_with_mods(sample_rate, mods.for_owner(ModOwner::Voice));
        for stage in chain {
            // Filters first, then the post-fx rack: the order the two sit in
            // within a single voice, so a stage behaves like one.
            if stage.filters.is_active() {
                voice = Box::new(FilterStageVoice::new(
                    voice,
                    &stage.filters,
                    sample_rate,
                    stage.duration,
                ));
            }
            if stage.fx.is_active() {
                voice = Box::new(PostFxVoice::new(voice, stage.fx, sample_rate));
            }
        }
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
            ModTarget::Cutoff => self.filter_param(|f| f.lp.freq.unwrap_or(0.0)),
            ModTarget::Resonance => self.filter_param(|f| f.lp.q),
            ModTarget::Hcutoff => self.filter_param(|f| f.hp.freq.unwrap_or(0.0)),
            ModTarget::Hresonance => self.filter_param(|f| f.hp.q),
            ModTarget::Bandf => self.filter_param(|f| f.bp.freq.unwrap_or(0.0)),
            ModTarget::Bandq => self.filter_param(|f| f.bp.q),
            _ => fx.mod_base(target),
        }
    }

    /// Read one of the voice's filter slots; every voice type carries the same
    /// three.
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
            VoiceSpec::Sampler(p) => f(&p.filters),
        }
    }
}

// ---------------------------------------------------------------------------
// Waveshaping / bitcrush / decimation post-effects (superdough crush/shape/
// distort/coarse worklets). Applied per voice, after the voice renders.
