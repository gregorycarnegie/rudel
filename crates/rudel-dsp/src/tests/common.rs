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
