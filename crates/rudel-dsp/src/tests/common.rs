pub(super) use super::super::*;
pub(super) use rudel_core::{Value, ValueMap};
pub(super) use std::{f32::consts::TAU, sync::Arc};

/// What a buffer has to look like to be a signal rather than a stuck value.
///
/// `assert!(peak > 0.0)` is satisfied by a constant, so every "produces sound"
/// test here passed with the voice replaced by DC — mutation testing walked off
/// with 110 return-a-constant survivors. These are the cheapest properties a
/// constant fails, and they say nothing about tuning, so they do not break when
/// a coefficient is legitimately changed.
pub(super) fn assert_is_signal(samples: &[f32], what: &str) {
    assert!(!samples.is_empty(), "{what}: no samples");
    assert!(
        samples.iter().all(|s| s.is_finite()),
        "{what}: non-finite sample"
    );

    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.0, "{what}: silent");

    // A constant has no variance; anything oscillating does.
    let mean = samples.iter().sum::<f32>() / samples.len() as f32;
    let variance = samples.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / samples.len() as f32;
    assert!(
        variance > 1e-9,
        "{what}: constant at {mean}, not a signal (peak {peak})"
    );

    // ...and audio swings both ways rather than sitting to one side of zero.
    assert!(
        samples.iter().any(|&s| s > 0.0) && samples.iter().any(|&s| s < 0.0),
        "{what}: never crosses zero, mean {mean}"
    );
}

/// A voice-level LFO on `freq`, in the nested-map shape the Koto side hands
/// over. `dcoffset: 0` keeps the offset in `0..depth` so it only ever pushes the
/// pitch up, which is what makes the direction checkable below.
pub(super) fn positive_freq_lfo(depth_hz: f64, rate: f64) -> ModSpecs {
    positive_lfo("freq", depth_hz, rate)
}

/// As above for any modulatable control. `depthabs` plus `dcoffset: 0` keeps the
/// offset in `0..depth`, so it only ever pushes the target up — which is what
/// makes the *sign* of a `+ mods.get(..)` term checkable rather than just its
/// presence.
pub(super) fn positive_lfo(control: &str, depth: f64, rate: f64) -> ModSpecs {
    let mut entry = ValueMap::new();
    entry.insert("control".to_string(), Value::from(control));
    entry.insert("depthabs".to_string(), Value::F64(depth));
    entry.insert("frequency".to_string(), Value::F64(rate));
    entry.insert("dcoffset".to_string(), Value::F64(0.0));

    let mut desc = ValueMap::new();
    desc.insert("__ids".to_string(), Value::List(vec![Value::from("0")]));
    desc.insert("0".to_string(), Value::Map(entry));

    let mut map = ValueMap::new();
    map.insert("lfo".to_string(), Value::Map(desc));

    let ctx = ModContext {
        cps: 0.5,
        cycle: 0.0,
        note_seconds: 1.0,
    };
    // The base the depth resolves against is deliberately below 30Hz: above
    // that, `range_for` replaces the dcoffset/depth band with the frequency
    // clamp `(20 - current, 24000 - current)`, which spans both signs and would
    // make the direction of the offset untestable.
    ModSpecs::from_controls(&map, &ctx, |_| 25.0)
}
