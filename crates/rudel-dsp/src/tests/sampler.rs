use super::common::*;

#[test]
fn sampler_plays_a_buffer_then_finishes() {
    // a 0.1s buffer of a 200 Hz sine
    let sr = 44100.0;
    let n = (sr * 0.1) as usize;
    let data: Vec<f32> = (0..n)
        .map(|i| (TAU * 200.0 * i as f32 / sr).sin())
        .collect();
    let sample = Arc::new(Sample {
        data,
        sample_rate: sr,
    });
    let mut v = SamplerVoice::new(SamplerParams::new(sample), sr);
    let mut peak = 0.0f32;
    let mut frames = 0;
    while !v.is_done() && frames < 44100 {
        peak = peak.max(v.tick().0.abs());
        frames += 1;
    }
    assert!(peak > 0.0, "sampler should produce output");
    assert!(v.is_done(), "sampler should finish at the buffer end");
    assert!(frames < 44100, "sampler should not run forever");
}

#[test]
fn sampler_speed_changes_duration() {
    let sr = 44100.0;
    let data = vec![0.5f32; 4410]; // 0.1s of DC
    let sample = Arc::new(Sample {
        data,
        sample_rate: sr,
    });
    let mut fast = SamplerParams::new(sample.clone());
    fast.speed = 2.0;
    let mut v = SamplerVoice::new(fast, sr);
    let mut frames = 0;
    while !v.is_done() && frames < 44100 {
        v.tick();
        frames += 1;
    }
    // at 2x speed the 0.1s buffer should take ~0.05s (~2205 frames)
    assert!(frames < 3000, "2x speed should play back in ~half the time");
}

#[test]
fn loop_plays_past_the_buffers_natural_length() {
    // A 0.1s buffer asked to loop for 0.5s should still be audible well past
    // its own length, then stop near the hold time (not run forever).
    let sr = 44100.0;
    let n = (sr * 0.1) as usize;
    let data: Vec<f32> = (0..n)
        .map(|i| (TAU * 200.0 * i as f32 / sr).sin())
        .collect();
    let sample = Arc::new(Sample {
        data,
        sample_rate: sr,
    });
    let mut p = SamplerParams::new(sample);
    p.loop_on = true;
    p.duration = 0.5; // hold far longer than the 0.1s buffer
    let mut v = SamplerVoice::new(p, sr);

    let mut peak_late = 0.0f32;
    let mut frames = 0;
    while !v.is_done() && frames < 44100 {
        let s = v.tick().0.abs();
        if frames > (sr * 0.2) as usize {
            peak_late = peak_late.max(s); // sampled past the natural end
        }
        frames += 1;
    }
    assert!(
        peak_late > 0.0,
        "a looping sample should still sound past its natural length"
    );
    assert!(
        frames >= (sr * 0.4) as usize,
        "should play roughly the hold duration"
    );
    assert!(
        frames < (sr * 0.7) as usize,
        "should stop after the hold + release, not loop forever"
    );
}
