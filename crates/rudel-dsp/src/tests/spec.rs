//! `VoiceSpec`'s two lookup tables: which control a modulation target reads
//! its base value from, and whether a voice needs the post-fx wrapper at all.

use super::common::*;
use rstest::rstest;

fn synth() -> VoiceSpec {
    let mut p = VoiceParams {
        freq: 440.0,
        gain: 0.7,
        ..Default::default()
    };
    // Distinct values per slot: a mutant that reads the wrong one shows up
    // as another slot's number rather than as zero.
    p.lp.freq = Some(1000.0);
    p.lp.q = 2.0;
    p.hp.freq = Some(200.0);
    p.hp.q = 3.0;
    p.bp.freq = Some(3000.0);
    p.bp.q = 4.0;
    VoiceSpec::Synth(Box::new(p))
}

fn sampler() -> VoiceSpec {
    let sample = Arc::new(Sample {
        data: vec![0.0; 8],
        sample_rate: 44100.0,
    });
    let mut p = SamplerParams::new(sample);
    p.gain = 0.5;
    p.filters.lp.freq = Some(800.0);
    p.filters.lp.q = 5.0;
    VoiceSpec::Sampler(p)
}

fn modulated_fx() -> PostFx {
    PostFx {
        postgain: 0.8,
        shape: Some(0.3),
        distort: Some(1.5),
        crush: Some(6.0),
        coarse: Some(2.0),
        ..Default::default()
    }
}

#[rstest]
#[case(ModTarget::Frequency, 440.0)]
#[case(ModTarget::Gain, 0.7)]
#[case(ModTarget::Cutoff, 1000.0)]
#[case(ModTarget::Resonance, 2.0)]
#[case(ModTarget::Hcutoff, 200.0)]
#[case(ModTarget::Hresonance, 3.0)]
#[case(ModTarget::Bandf, 3000.0)]
#[case(ModTarget::Bandq, 4.0)]
// Anything the voice does not own comes from the post-fx chain.
#[case(ModTarget::Postgain, 0.8)]
#[case(ModTarget::Shape, 0.3)]
#[case(ModTarget::Distort, 1.5)]
#[case(ModTarget::Crush, 6.0)]
#[case(ModTarget::Coarse, 2.0)]
fn every_modulation_target_reads_its_own_control(#[case] target: ModTarget, #[case] want: f32) {
    assert_eq!(synth().mod_base(target, &modulated_fx()), want);
}

#[test]
fn the_sampler_reads_only_the_modulation_targets_it_owns() {
    // The sampler keeps its filter controls outside `FilterSet`, so it has its
    // own arms for the two it owns and zero for the rest.
    let fx = modulated_fx();
    let sampler = sampler();
    assert_eq!(sampler.mod_base(ModTarget::Gain, &fx), 0.5);
    assert_eq!(sampler.mod_base(ModTarget::Cutoff, &fx), 800.0);
    assert_eq!(sampler.mod_base(ModTarget::Resonance, &fx), 5.0);
    // ...and no oscillator frequency at all.
    assert_eq!(sampler.mod_base(ModTarget::Frequency, &fx), 0.0);
    assert_eq!(sampler.mod_base(ModTarget::Hcutoff, &fx), 0.0);
    assert_eq!(sampler.mod_base(ModTarget::Bandf, &fx), 0.0);
}

#[test]
fn a_voice_is_only_wrapped_when_something_downstream_wants_it() {
    // Wrapping every voice in a `PostFxVoice` costs a per-sample pass that
    // does nothing; *not* wrapping when a modulator targets the chain drops
    // that modulation silently. Both show up in the samples.
    let render = |fx: PostFx, mods: &ModSpecs| -> Vec<f32> {
        let mut v = synth().into_modulated_voice(44100.0, fx, mods);
        (0..256).map(|_| v.tick().0).collect()
    };
    let none = ModSpecs::default();
    let bare = render(PostFx::default(), &none);
    assert_is_signal(&bare, "unwrapped voice");

    // An active chain changes the output...
    let loud = PostFx {
        postgain: 0.5,
        ..Default::default()
    };
    assert_ne!(
        render(loud, &none),
        bare,
        "an active chain has to be wrapped"
    );

    // ...and so does a modulator aimed at the chain, even when the chain
    // itself is at its defaults.
    let modulated = render(PostFx::default(), &positive_lfo("postgain", 0.5, 2.0));
    assert_ne!(
        modulated, bare,
        "a post-fx modulation has to be wrapped even with an inert chain"
    );
}

#[test]
fn an_fx_stage_reads_its_effects_filters_and_length_from_the_controls() {
    let map: ValueMap = [
        ("shape".to_string(), Value::F64(0.5)),
        ("cutoff".to_string(), Value::F64(800.0)),
    ]
    .into_iter()
    .collect();
    let stage = FxStage::from_controls(&map, 2.5);
    assert_eq!(stage.fx.shape, Some(0.5));
    assert_eq!(stage.filters.lp.freq, Some(800.0));
    assert_eq!(stage.duration, 2.5);
}
