//! The oscillator voice's own arithmetic: the source mixer, the FM matrix, the
//! per-sample envelope and gain staging, and the two small helpers.
//!
//! The super-saw source has a parity golden of its own in `tests::supersaw`, and
//! the waveform/table/noise primitives have one in `tests::oscillator`. What is
//! left here is the wiring between them, which no oracle covers because it is
//! not a single upstream function — so these assert against the definitions the
//! code claims to implement.

use super::common::*;
use crate::synth::{rand_phase, wetfade};

const SR: f32 = 44100.0;

fn voice(params: VoiceParams) -> Voice {
    Voice::new(params, SR)
}

// --- the two free helpers ---------------------------------------------------

#[test]
fn wetfade_holds_full_across_the_first_half_then_ramps_out() {
    // superdough: `(d) => (d < 0.5 ? 1 : 1 - (d - 0.5) / 0.5)`. Used for both
    // sides of a dry/wet crossfade, so full-scale over the first half is what
    // keeps a 50% mix from dipping.
    for d in [0.0f32, 0.1, 0.25, 0.49] {
        assert_eq!(wetfade(d), 1.0, "wetfade({d}) should still be full");
    }
    assert_eq!(wetfade(0.5), 1.0, "the midpoint is the last full value");
    assert!((wetfade(0.75) - 0.5).abs() < 1e-6);
    assert!(
        wetfade(1.0).abs() < 1e-6,
        "fully wet fades the dry side out"
    );

    // Monotone across the ramp, and never negative.
    let mut prev = 1.0;
    for i in 50..=100 {
        let v = wetfade(i as f32 / 100.0);
        assert!(v <= prev + 1e-6 && v >= 0.0, "wetfade should fall to 0");
        prev = v;
    }

    // The pair used together sums to more than one at the midpoint, which is
    // the point of the shape — equal-gain, not equal-power.
    assert_eq!(wetfade(0.5) + wetfade(0.5), 2.0);
}

#[test]
fn rand_phase_spreads_across_the_cycle_without_repeating() {
    // Super-saw and wavetable unison voices start decorrelated; a constant here
    // would stack every voice at the same phase and turn the unison into one
    // loud voice.
    let draws: Vec<f32> = (0..512).map(|_| rand_phase()).collect();

    assert!(
        draws.iter().all(|&p| (0.0..1.0).contains(&p)),
        "phases must land in [0, 1)"
    );
    assert!(
        draws.windows(2).all(|w| w[0] != w[1]),
        "consecutive draws must differ"
    );
    // Spread over the whole cycle rather than clustering.
    let min = draws.iter().cloned().fold(f32::MAX, f32::min);
    let max = draws.iter().cloned().fold(f32::MIN, f32::max);
    assert!(min < 0.1 && max > 0.9, "draws should span the cycle");
    let mean = draws.iter().sum::<f32>() / draws.len() as f32;
    assert!(
        (mean - 0.5).abs() < 0.05,
        "draws should be roughly uniform, mean was {mean}"
    );
}

// --- the source mixer -------------------------------------------------------

#[test]
fn the_oscillator_advances_at_carrier_over_sample_rate() {
    // One second of a 100Hz tone is 100 whole cycles, so the phase returns to
    // where it started. This is what pins `phase + carrier / sr`.
    let mut v = voice(VoiceParams {
        freq: 100.0,
        duration: 2.0,
        ..Default::default()
    });
    for _ in 0..(SR as usize / 100) {
        v.next_source();
    }
    assert!(
        v.phase < 1e-3 || v.phase > 1.0 - 1e-3,
        "one cycle of 100Hz should land back at phase 0, got {}",
        v.phase
    );

    // Twice the frequency covers twice the phase in the same time.
    let mut slow = voice(VoiceParams {
        freq: 100.0,
        duration: 2.0,
        ..Default::default()
    });
    let mut fast = voice(VoiceParams {
        freq: 200.0,
        duration: 2.0,
        ..Default::default()
    });
    for _ in 0..50 {
        slow.next_source();
        fast.next_source();
    }
    assert!(
        (fast.phase - 2.0 * slow.phase).abs() < 1e-4,
        "200Hz should be twice as far along as 100Hz: {} vs {}",
        fast.phase,
        slow.phase
    );
}

#[test]
fn a_noise_source_bypasses_the_oscillator_entirely() {
    // `s("white")` short-circuits before the oscillator, so the phase never
    // moves and the output is the noise generator's.
    let mut v = voice(VoiceParams {
        noise: Some(NoiseKind::White),
        freq: 440.0,
        duration: 1.0,
        ..Default::default()
    });
    let out: Vec<f32> = (0..256).map(|_| v.next_source()).collect();
    assert_eq!(v.phase, 0.0, "a noise voice should not advance the phase");
    assert_is_signal(&out, "white noise source");
}

#[test]
fn the_additive_table_takes_precedence_over_the_waveform() {
    // With `partials` set, the built table is the source; the `s` waveform is
    // only the base series it was built from.
    let table =
        crate::oscillator::build_additive(&[1.0], None, crate::oscillator::AdditiveType::Saw);
    let mut additive = voice(VoiceParams {
        additive: Some(table),
        waveform: Waveform::Square,
        freq: 100.0,
        duration: 1.0,
        ..Default::default()
    });
    let mut square = voice(VoiceParams {
        waveform: Waveform::Square,
        freq: 100.0,
        duration: 1.0,
        ..Default::default()
    });
    // A single-partial saw table is a sine, which a square is not: the square
    // is on a rail at every sample, the table is not.
    let from_table: Vec<f32> = (0..64).map(|_| additive.next_source()).collect();
    let from_square: Vec<f32> = (0..64).map(|_| square.next_source()).collect();
    assert!(
        from_square.iter().all(|s| s.abs() > 0.99),
        "the square source should sit on its rails"
    );
    assert!(
        from_table.iter().any(|s| s.abs() < 0.9),
        "the additive table should be used instead of the square"
    );
}

#[test]
fn pulse_width_reaches_the_source() {
    // `s("pulse")` reads `pw`, unlike every other waveform. A narrow duty
    // spends most of the cycle low, so the mean goes negative.
    let mean = |pw: f32| {
        let mut v = voice(VoiceParams {
            waveform: Waveform::Pulse,
            pw,
            freq: 100.0,
            duration: 1.0,
            ..Default::default()
        });
        let n = 441;
        (0..n).map(|_| v.next_source()).sum::<f32>() / n as f32
    };
    assert!(mean(0.1) < -0.5, "a 10% duty should sit mostly low");
    assert!(mean(0.9) > 0.5, "a 90% duty should sit mostly high");
    assert!(mean(0.5).abs() < 0.1, "a 50% duty is balanced");
}

#[test]
fn the_noise_mix_crossfades_between_oscillator_and_pink() {
    // `noise` blends pink into the oscillator through the `wetfade` pair, so a
    // mix of 0 leaves the oscillator untouched and 1 replaces it outright.
    let render = |noise_mix: f32| {
        let mut v = voice(VoiceParams {
            waveform: Waveform::Sine,
            noise_mix,
            freq: 100.0,
            duration: 1.0,
            ..Default::default()
        });
        (0..512).map(|_| v.next_source()).collect::<Vec<_>>()
    };
    let dry = render(0.0);
    let mixed = render(0.6);
    let wet = render(1.0);

    // A pure sine tracks its own definition; the mixed and fully-wet versions
    // do not.
    let sine_error = |out: &[f32]| {
        out.iter()
            .enumerate()
            .map(|(i, s)| {
                let want = (std::f32::consts::TAU * (i as f32 * 100.0 / SR)).sin();
                (s - want).abs()
            })
            .fold(0.0f32, f32::max)
    };
    assert!(sine_error(&dry) < 1e-3, "mix 0 should be the bare sine");
    assert!(sine_error(&mixed) > 0.05, "mix 0.6 should add noise");
    assert!(sine_error(&wet) > 0.05, "mix 1 should be noise");
    assert_is_signal(&wet, "fully wet noise mix");
}

// --- FM ---------------------------------------------------------------------

/// A single operator modulating the carrier, which is `fmi` in Strudel terms.
fn one_op_fm(index: f32, ratio: f32) -> FmSpec {
    let mut ops = [FmOp::default(); crate::fm::FM_OPS + 1];
    ops[1] = FmOp {
        ratio,
        wave: Waveform::Sine,
        env: None,
    };
    let mut amt = [[0.0; crate::fm::FM_OPS + 1]; crate::fm::FM_OPS + 1];
    amt[1][0] = index; // operator 1 -> carrier
    FmSpec {
        ops,
        amt,
        max_op: 1,
    }
}

#[test]
fn fm_deviation_peaks_at_index_times_operator_frequency() {
    // Classic FM: peak deviation is the modulation index times the modulator's
    // frequency, and the modulator's frequency is `carrier * ratio`.
    let carrier = 200.0;
    for (index, ratio) in [(1.0f32, 1.0f32), (2.0, 1.0), (1.0, 3.0)] {
        let mut v = voice(VoiceParams {
            fm: one_op_fm(index, ratio),
            freq: carrier,
            duration: 1.0,
            ..Default::default()
        });
        let peak = (0..2048)
            .map(|_| v.fm_deviation(carrier).abs())
            .fold(0.0f32, f32::max);
        let want = index * carrier * ratio;
        assert!(
            (peak - want).abs() < want * 0.02,
            "index {index} ratio {ratio}: peak deviation should be ~{want}, got {peak}"
        );
    }
}

#[test]
fn no_modulation_index_means_no_deviation() {
    let mut v = voice(VoiceParams {
        fm: one_op_fm(0.0, 1.0),
        freq: 200.0,
        duration: 1.0,
        ..Default::default()
    });
    for _ in 0..256 {
        assert_eq!(v.fm_deviation(200.0), 0.0);
    }
}

#[test]
fn the_fm_operator_envelope_scales_its_output() {
    // `fm{adsr}` scales the operator 0..1, so an envelope that has decayed to
    // its sustain deviates less than one held at full.
    let with_env = |env: Option<Adsr>| {
        let mut fm = one_op_fm(2.0, 1.0);
        fm.ops[1] = FmOp { env, ..fm.ops[1] };
        let mut v = voice(VoiceParams {
            fm,
            freq: 200.0,
            duration: 1.0,
            ..Default::default()
        });
        // Step past the decay before measuring.
        let mut peak = 0.0f32;
        for i in 0..4410 {
            let d = v.fm_deviation(200.0).abs();
            if i > 2205 {
                peak = peak.max(d);
            }
            v.t = i as f32 / SR;
        }
        peak
    };
    let full = with_env(None);
    let decayed = with_env(Some(Adsr {
        attack: 0.001,
        decay: 0.01,
        sustain: 0.25,
        release: 0.01,
    }));
    assert!(
        decayed < full * 0.5,
        "a decayed operator envelope should cut the deviation: {decayed} vs {full}"
    );
}

// --- gain staging and lifetime ----------------------------------------------

#[test]
fn tick_scales_by_envelope_gain_and_the_strudel_turn_down() {
    // The output is `source * envelope * gain * 0.3`. With a full-sustain
    // envelope and a bare sine, the peak is that 0.3 times the gain.
    let peak_for = |gain: f32| {
        let mut v = voice(VoiceParams {
            waveform: Waveform::Sine,
            freq: 441.0,
            gain,
            pan: 0.5,
            adsr: Adsr {
                attack: 0.0001,
                decay: 0.0001,
                sustain: 1.0,
                release: 0.01,
            },
            duration: 0.5,
            ..Default::default()
        });
        // Equal-power pan at centre puts cos(pi/4) on each side.
        let centre = std::f32::consts::FRAC_1_SQRT_2;
        (0..4410).map(|_| v.tick().0.abs()).fold(0.0f32, f32::max) / centre
    };
    let unit = peak_for(1.0);
    assert!(
        (unit - 0.3).abs() < 0.01,
        "a full-scale source should peak at the 0.3 turn-down, got {unit}"
    );
    assert!(
        (peak_for(0.5) - 0.15).abs() < 0.01,
        "gain should scale the output linearly"
    );
}

#[test]
fn pan_is_equal_power_on_the_mono_path() {
    let sides = |pan: f32| {
        let mut v = voice(VoiceParams {
            waveform: Waveform::Sine,
            freq: 441.0,
            pan,
            adsr: Adsr {
                attack: 0.0001,
                decay: 0.0001,
                sustain: 1.0,
                release: 0.01,
            },
            duration: 0.5,
            ..Default::default()
        });
        let (mut l, mut r) = (0.0f32, 0.0f32);
        for _ in 0..2048 {
            let (a, b) = v.tick();
            l = l.max(a.abs());
            r = r.max(b.abs());
        }
        (l, r)
    };
    let (l, r) = sides(0.0);
    assert!(l > 0.0 && r < 1e-6, "pan 0 is hard left");
    let (l, r) = sides(1.0);
    assert!(r > 0.0 && l < 1e-6, "pan 1 is hard right");
    let (l, r) = sides(0.5);
    assert!((l - r).abs() < 1e-6, "pan 0.5 is centred");
    // Equal power: the centre pair is 1/sqrt(2) of a hard-panned side, not half.
    let (hard, _) = sides(0.0);
    assert!(
        (l / hard - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01,
        "centre should be -3dB per side, not -6dB"
    );
}

#[test]
fn a_voice_finishes_one_release_after_its_hold_and_then_stays_silent() {
    let mut v = voice(VoiceParams {
        waveform: Waveform::Sine,
        freq: 441.0,
        duration: 0.1,
        adsr: Adsr {
            attack: 0.001,
            decay: 0.01,
            sustain: 0.5,
            release: 0.05,
        },
        ..Default::default()
    });
    let mut n = 0;
    while !v.is_done() && n < (SR as usize) {
        v.tick();
        n += 1;
    }
    let got = n as f32 / SR;
    // hold_end is the note duration; the voice ends one release later.
    assert!(
        (got - 0.15).abs() < 0.002,
        "should finish at duration + release = 0.15s, got {got:.3}s"
    );
    // ...and it is silent after that rather than merely flagged.
    for _ in 0..64 {
        assert_eq!(v.tick(), (0.0, 0.0), "a finished voice outputs silence");
    }
}

#[test]
fn the_envelope_follows_the_adsr_over_the_notes_life() {
    let mut v = voice(VoiceParams {
        duration: 0.2,
        adsr: Adsr {
            attack: 0.05,
            decay: 0.05,
            sustain: 0.4,
            release: 0.1,
        },
        ..Default::default()
    });
    let at = |v: &mut Voice, t: f32| {
        v.t = t;
        v.envelope()
    };
    assert!(at(&mut v, 0.0) < 0.01, "starts from silence");
    assert!(
        (at(&mut v, 0.05) - 1.0).abs() < 0.01,
        "peaks at the attack end"
    );
    assert!(
        (at(&mut v, 0.1) - 0.4).abs() < 0.01,
        "decays to the sustain level"
    );
    assert!((at(&mut v, 0.15) - 0.4).abs() < 0.01, "holds there");
    assert!(at(&mut v, 0.3) < 0.01, "released to silence");
}

#[test]
fn the_voice_like_impl_forwards_to_the_inherent_methods() {
    // `VoiceLike` is what the mixer sees, so a stubbed-out forward would leave
    // finished voices playing forever.
    let mut v: Box<dyn VoiceLike> = Box::new(voice(VoiceParams {
        waveform: Waveform::Sine,
        freq: 441.0,
        duration: 0.02,
        adsr: Adsr {
            attack: 0.001,
            decay: 0.001,
            sustain: 1.0,
            release: 0.01,
        },
        ..Default::default()
    }));
    assert!(!v.is_done(), "a fresh voice is not done");
    let mut heard = false;
    for _ in 0..4410 {
        let (l, _r) = v.tick();
        heard |= l.abs() > 1e-4;
        if v.is_done() {
            break;
        }
    }
    assert!(heard, "ticking through the trait should produce audio");
    assert!(v.is_done(), "and the voice should report finishing");
}

// --- modulators reaching the voice ------------------------------------------

#[test]
fn a_frequency_modulator_is_added_to_the_oscillator_carrier() {
    // `next_source` reads `freq * pitch_mult() + mods.get(Frequency)`. Without a
    // modulator attached that term is a constant 0, so its sign never shows.
    // A strictly positive offset has to raise the pitch.
    let crossings = |mods: &[ModSpec]| {
        let mut v = Voice::with_mods(
            VoiceParams {
                waveform: Waveform::Sine,
                freq: 200.0,
                duration: 1.0,
                ..Default::default()
            },
            SR,
            mods,
        );
        let out: Vec<f32> = (0..8820).map(|_| v.tick().0).collect();
        out.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    };
    let plain = crossings(&[]);
    let modulated = crossings(&positive_lfo("freq", 300.0, 3.0).voice);
    assert!(
        modulated > plain,
        "a positive frequency offset must raise the pitch: {plain} unmodulated, {modulated} modulated"
    );
}

#[test]
fn a_gain_modulator_is_added_to_the_voice_gain() {
    // `tick` reads `params.gain + mods.get(Gain)`, the same shape.
    let peak = |mods: &[ModSpec]| {
        let mut v = Voice::with_mods(
            VoiceParams {
                waveform: Waveform::Sine,
                freq: 441.0,
                gain: 0.2,
                duration: 1.0,
                adsr: Adsr {
                    attack: 0.0001,
                    decay: 0.0001,
                    sustain: 1.0,
                    release: 0.01,
                },
                ..Default::default()
            },
            SR,
            mods,
        );
        (0..8820).map(|_| v.tick().0.abs()).fold(0.0f32, f32::max)
    };
    let plain = peak(&[]);
    let modulated = peak(&positive_lfo("gain", 0.8, 3.0).voice);
    assert!(
        modulated > plain * 1.5,
        "a positive gain offset must make the voice louder: {plain:.4} vs {modulated:.4}"
    );
}

#[test]
fn bus_input_reaches_the_modulator_bank() {
    // `set_bus_input` is how the mixer feeds one pattern's output into another's
    // modulators; stubbed out, `.bus(n)` modulation silently does nothing.
    let mut v: Box<dyn VoiceLike> = Box::new(voice(VoiceParams {
        waveform: Waveform::Sine,
        freq: 200.0,
        duration: 1.0,
        ..Default::default()
    }));
    // No bus modulator is attached, so this must be a harmless no-op rather
    // than a panic — the mixer calls it on every voice each block.
    v.set_bus_input(0, &[0.5; 64], &[0.5; 64]);
    let (l, _r) = v.tick();
    assert!(l.is_finite(), "feeding a bus must not corrupt the voice");
}

// --- FM cross-modulation ----------------------------------------------------

#[test]
fn one_operator_modulating_another_changes_the_carrier_deviation() {
    // The matrix is `amt[source][target]`, and target 0 is the carrier. A
    // two-operator stack (op2 -> op1 -> carrier) has to differ from op1 alone,
    // or the `amt[k][j]` inner loop is doing nothing.
    let deviation_spread = |fm: FmSpec| {
        let mut v = voice(VoiceParams {
            fm,
            freq: 200.0,
            duration: 1.0,
            ..Default::default()
        });
        let out: Vec<f32> = (0..4096).map(|_| v.fm_deviation(200.0)).collect();
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
        (peak, rms)
    };

    let single = one_op_fm(1.0, 1.0);
    let mut stacked = one_op_fm(1.0, 1.0);
    stacked.max_op = 2;
    stacked.ops[2] = FmOp {
        ratio: 2.0,
        wave: Waveform::Sine,
        env: None,
    };
    stacked.amt[2][1] = 3.0; // operator 2 modulates operator 1

    let (p1, r1) = deviation_spread(single);
    let (p2, r2) = deviation_spread(stacked);
    // Modulating op1 spreads its spectrum: the peak deviation stays pinned to
    // op1's own index and frequency, but the waveform is no longer a clean sine,
    // so the rms-to-peak ratio moves.
    assert!(
        (r2 / p2 - r1 / p1).abs() > 0.02,
        "op2 -> op1 should reshape the deviation: crest {:.3} vs {:.3}",
        r1 / p1,
        r2 / p2
    );
}

// --- the stereo path through `tick` -----------------------------------------
//
// The super-saw and wavetable sources return a stereo pair, so `tick` takes a
// different branch for them: its own gain staging, and the voice pan applied as
// a *balance* rather than the mono equal-power pair. The parity goldens drive
// `next_supersaw` directly and every other test here uses a mono waveform, so
// nothing reached this branch at all.

/// A super-saw voice with its unison phases planted. `rand_phase` draws from a
/// process-global counter, so two voices built in sequence start decorrelated
/// and their peaks are not comparable — every test below compares two voices.
fn supersaw_voice_with(pan: f32, gain: f32) -> Voice {
    let mut v = Voice::new(
        VoiceParams {
            supersaw: true,
            unison: 3,
            // The source has a stereo spread of its own (voices alternate L/R
            // weighted gains), which would sit on top of the pan balance under
            // test. Flatten it so the balance is the only asymmetry.
            panspread: 0.0,
            freq: 220.0,
            pan,
            gain,
            duration: 1.0,
            adsr: Adsr {
                attack: 0.0001,
                decay: 0.0001,
                sustain: 1.0,
                release: 0.01,
            },
            ..Default::default()
        },
        SR,
    );
    v.super_phases[..3].copy_from_slice(&[0.1, 0.4, 0.7]);
    v
}

fn supersaw_voice(pan: f32) -> Voice {
    supersaw_voice_with(pan, 1.0)
}

fn peaks(v: &mut Voice, n: usize) -> (f32, f32) {
    let (mut l, mut r) = (0.0f32, 0.0f32);
    for _ in 0..n {
        let (a, b) = v.tick();
        l = l.max(a.abs());
        r = r.max(b.abs());
    }
    (l, r)
}

#[test]
fn a_stereo_voice_pans_as_a_balance_not_equal_power() {
    // `p = 2*pan - 1`, then one side is attenuated by `1 - |p|` while the other
    // stays at full. So the centre keeps *both* sides at unity — unlike the mono
    // path, where centre is -3dB a side.
    let (cl, cr) = peaks(&mut supersaw_voice(0.5), 4410);
    assert!(
        (cl - cr).abs() < 1e-6,
        "a centred stereo voice should be symmetric"
    );

    let (ll, lr) = peaks(&mut supersaw_voice(0.0), 4410);
    assert!(lr < 1e-6, "pan 0 should silence the right side");
    assert!(
        (ll - cl).abs() < cl * 0.05,
        "the kept side stays at full: {ll:.4} vs centre {cl:.4}"
    );

    let (rl, rr) = peaks(&mut supersaw_voice(1.0), 4410);
    assert!(rl < 1e-6, "pan 1 should silence the left side");
    assert!((rr - cr).abs() < cr * 0.05, "the kept side stays at full");

    // Half-way over attenuates one side without touching the other.
    let (hl, hr) = peaks(&mut supersaw_voice(0.75), 4410);
    assert!(
        (hr - cr).abs() < cr * 0.05,
        "the side being panned toward is untouched"
    );
    assert!(
        hl < cl * 0.6 && hl > cl * 0.4,
        "the other side is scaled by 1 - |p| = 0.5, got {hl:.4} vs {cl:.4}"
    );
}

#[test]
fn the_stereo_path_applies_envelope_gain_and_the_turn_down_too() {
    let peak_for = |gain: f32| peaks(&mut supersaw_voice_with(0.5, gain), 4410).0;
    let unit = peak_for(1.0);
    assert!(unit > 0.0, "the stereo path should produce audio");
    assert!(
        (peak_for(0.5) - unit * 0.5).abs() < unit * 0.05,
        "gain should scale the stereo path linearly"
    );
    assert_eq!(
        peaks(&mut supersaw_voice_with(0.5, 0.0), 1024),
        (0.0, 0.0),
        "zero gain is silence"
    );
}

#[test]
fn a_stereo_voice_finishes_like_a_mono_one() {
    let mut v = Voice::new(
        VoiceParams {
            supersaw: true,
            unison: 3,
            freq: 220.0,
            duration: 0.1,
            adsr: Adsr {
                attack: 0.001,
                decay: 0.01,
                sustain: 0.5,
                release: 0.05,
            },
            ..Default::default()
        },
        SR,
    );
    let mut n = 0;
    while !v.is_done() && n < SR as usize {
        v.tick();
        n += 1;
    }
    let got = n as f32 / SR;
    assert!(
        (got - 0.15).abs() < 0.002,
        "should finish at duration + release, got {got:.3}s"
    );
    assert_eq!(v.tick(), (0.0, 0.0));
}

// --- terms that only appear once something else is switched on ---------------

#[test]
fn the_pitch_envelope_multiplies_the_oscillator_carrier() {
    // `freq * pitch_mult()` — with no `penv`/`vib` the multiplier is a constant
    // 1.0, which makes the multiply indistinguishable from a divide. A downward
    // pitch envelope has to slow the phase down.
    let advance = |penv: Option<f32>| {
        let mut v = voice(VoiceParams {
            waveform: Waveform::Sine,
            freq: 400.0,
            penv,
            pattack: penv.map(|_| 0.001),
            duration: 1.0,
            ..Default::default()
        });
        for i in 0..441 {
            v.t = i as f32 / SR;
            v.next_source();
        }
        v.phase
    };
    let plain = advance(None);
    let down = advance(Some(-12.0));
    assert!(
        down < plain * 0.75,
        "an octave-down pitch envelope should cut the phase advance: {down:.4} vs {plain:.4}"
    );
}

#[test]
fn fm_reaches_the_oscillator_through_next_source() {
    // `fm_deviation` is tested directly above, but `next_source` only adds it to
    // the carrier when `fm.active()`. Without that the increment is the bare
    // `carrier / sr` and the summing term never runs.
    let spread = |fm: FmSpec| {
        let mut v = voice(VoiceParams {
            waveform: Waveform::Sine,
            freq: 400.0,
            fm,
            duration: 1.0,
            ..Default::default()
        });
        let out: Vec<f32> = (0..4410).map(|_| v.next_source()).collect();
        out.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / out.len() as f32
    };
    let plain = spread(FmSpec::default());
    let modulated = spread(one_op_fm(4.0, 2.0));
    assert!(
        modulated > plain * 1.5,
        "FM should widen the oscillator spectrum: {plain:.5} vs {modulated:.5}"
    );
}

#[test]
fn the_noise_mix_gains_follow_the_wetfade_pair() {
    // `s * wetfade(w) + pink * wetfade(1 - w)`. At w = 0.5 both gains are 1, so
    // the oscillator arrives at full strength on top of the noise; by w = 0.9 the
    // dry side is down to wetfade(0.9) = 0.2 and the sine is nearly gone.
    let sine_component = |w: f32| {
        let mut v = voice(VoiceParams {
            waveform: Waveform::Sine,
            noise_mix: w,
            freq: 441.0,
            duration: 1.0,
            ..Default::default()
        });
        let out: Vec<f32> = (0..4410).map(|_| v.next_source()).collect();
        let mut num = 0.0f32;
        for (i, s) in out.iter().enumerate() {
            num += s * (std::f32::consts::TAU * (i as f32 * 441.0 / SR)).sin();
        }
        2.0 * num / out.len() as f32
    };
    let half = sine_component(0.5);
    let mostly_wet = sine_component(0.9);
    assert!(
        (half - 1.0).abs() < 0.1,
        "at mix 0.5 the dry gain is still full, got {half:.3}"
    );
    assert!(
        (mostly_wet - 0.2).abs() < 0.1,
        "at mix 0.9 the dry gain should be wetfade(0.9) = 0.2, got {mostly_wet:.3}"
    );
}

/// `rand_phase` draws from a process-wide counter, so its *sequence* is
/// whatever ran before it — only the hash itself can be pinned. Without this,
/// any rearrangement of the shifts still looks uniform to the statistics above.
#[test]
fn phase_hash_matches_its_golden_values() {
    for (x, want) in [
        (0u32, 0.0f32),
        (1, 0.526_656_75),
        (0x9E37_79B9, 0.392_125_13),
        (0xFFFF_FFFF, 0.600_431_8),
    ] {
        assert_eq!(crate::synth::phase_hash(x), want, "phase_hash({x:#x})");
    }
}

/// Same for ZzFX's `randomness` draw: an exact xorshift32 over the counter.
#[test]
fn the_zzfx_rng_step_matches_its_golden_values() {
    for (x, want) in [
        (0u32, 1_359_758_873u32),
        (1, 1_358_964_346),
        (0x2545_F491, 3_090_627_344),
        (0xFFFF_FFFF, 1_359_504_952),
    ] {
        assert_eq!(crate::zzfx::step(x), want, "step({x:#x})");
    }
}

#[test]
fn a_pattern_that_names_no_sound_plays_a_triangle() {
    // superdough's `defaultDefaultValues.s` is `'triangle'`, so `note("c3")`
    // with no `.s(...)` is a triangle. Defaulting to a sine — the one waveform
    // with no harmonics at all — made every tune written that way come out soft
    // and flute-like where upstream is bright.
    let mut map = ValueMap::new();
    map.insert("note".to_string(), Value::F64(48.0));
    let params = VoiceParams::from_controls_at(&map, 1.0, 0.5, 0.0);
    assert_eq!(params.waveform, Waveform::Triangle);

    // An explicit `s` still wins, sine included.
    for (name, want) in [
        ("sine", Waveform::Sine),
        ("sawtooth", Waveform::Saw),
        ("square", Waveform::Square),
    ] {
        let mut map = ValueMap::new();
        map.insert("s".to_string(), Value::Str(name.into()));
        let params = VoiceParams::from_controls_at(&map, 1.0, 0.5, 0.0);
        assert_eq!(params.waveform, want, "s({name:?})");
    }
}
