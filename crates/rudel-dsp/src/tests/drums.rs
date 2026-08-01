use super::common::*;

const SAMPLE_RATE: f32 = 44100.0;

/// Every synthesized kind, in a fixed order the snapshot depends on.
const KINDS: [DrumKind; 11] = [
    DrumKind::Bd,
    DrumKind::Sd,
    DrumKind::Rim,
    DrumKind::Clap,
    DrumKind::Hh,
    DrumKind::Oh,
    DrumKind::Lt,
    DrumKind::Mt,
    DrumKind::Ht,
    DrumKind::Rd,
    DrumKind::Cr,
];

fn render(kind: DrumKind, n: usize) -> Vec<f32> {
    render_at(kind, n, SAMPLE_RATE)
}

fn render_at(kind: DrumKind, n: usize, sample_rate: f32) -> Vec<f32> {
    let mut v = DrumVoice::new(DrumParams::new(kind), sample_rate);
    (0..n).map(|_| v.tick().0).collect()
}

/// The snapshot covers 48kHz as well as 44.1kHz. Several boundaries in the
/// voices are only reachable at one rate or the other — the kick's 3ms click
/// ends at `t < 0.003`, and 0.003 × 44100 is 132.3, so at 44.1kHz `t` steps
/// straight over the boundary and never lands on it. At 48kHz it is sample 144
/// exactly.
const SNAPSHOT_RATES: [f32; 2] = [44100.0, 48000.0];

#[test]
fn drum_names_resolve() {
    // Every alias is something a user can type in `s("…")`, so a dropped one is
    // a drum that silently stops making a sound.
    for (names, want) in [
        (["bd", "bassdrum", "kick"].as_slice(), DrumKind::Bd),
        (["sd", "snare", "sn"].as_slice(), DrumKind::Sd),
        (["rim", "rs", "rimshot"].as_slice(), DrumKind::Rim),
        (["cp", "clap", "hc"].as_slice(), DrumKind::Clap),
        (["hh", "ch", "hat", "hihat"].as_slice(), DrumKind::Hh),
        (["oh", "oht", "openhat"].as_slice(), DrumKind::Oh),
        (["lt", "lowtom"].as_slice(), DrumKind::Lt),
        (["mt", "midtom"].as_slice(), DrumKind::Mt),
        (["ht", "hightom"].as_slice(), DrumKind::Ht),
        (["rd", "ride"].as_slice(), DrumKind::Rd),
        (["cr", "crash"].as_slice(), DrumKind::Cr),
    ] {
        for name in names {
            assert_eq!(DrumKind::from_name(name), Some(want), "{name}");
        }
    }
    for name in ["sawtooth", "supersaw", "", "BD"] {
        assert_eq!(DrumKind::from_name(name), None, "{name} is not a drum");
    }
}

#[test]
fn each_kind_rings_for_its_own_lifetime() {
    // The voice reports `is_done` once it passes `DrumKind::lifetime`, and those
    // differ per kind — a single shared value would leave hats ringing and
    // cymbals cut off.
    let ring = |kind| {
        let mut v = DrumVoice::new(DrumParams::new(kind), SAMPLE_RATE);
        let mut n = 0;
        while !v.is_done() && n < (3.0 * SAMPLE_RATE) as usize {
            v.tick();
            n += 1;
        }
        n as f32 / SAMPLE_RATE
    };
    for (kind, want) in [
        (DrumKind::Bd, 0.4),
        (DrumKind::Sd, 0.3),
        (DrumKind::Rim, 0.06),
        (DrumKind::Clap, 0.4),
        (DrumKind::Hh, 0.12),
        (DrumKind::Oh, 0.4),
        (DrumKind::Lt, 0.4),
        (DrumKind::Mt, 0.4),
        (DrumKind::Ht, 0.4),
        (DrumKind::Rd, 0.7),
        (DrumKind::Cr, 1.2),
    ] {
        let got = ring(kind);
        assert!(
            (got - want).abs() < 0.002,
            "{kind:?} should ring for {want}s, got {got:.3}s"
        );
    }
}

#[test]
fn controls_set_gain_pan_and_filters() {
    let mut p = DrumParams::new(DrumKind::Bd);
    assert_eq!((p.gain, p.pan), (1.0, 0.5));

    let map: ValueMap = [
        ("gain".to_string(), Value::F64(0.25)),
        ("pan".to_string(), Value::F64(1.0)),
        ("lpf".to_string(), Value::F64(800.0)),
    ]
    .into_iter()
    .collect();
    p.apply_controls(&map);

    assert_eq!(p.gain, 0.25);
    assert_eq!(p.pan, 1.0);
    // The filter set has to come through too, or `lpf` on a drum does nothing.
    let quiet_and_right = DrumVoice::new(p, SAMPLE_RATE).tick();
    assert!(
        quiet_and_right.0.abs() < 1e-6,
        "pan 1 from controls should be hard right"
    );

    // An empty map leaves the defaults alone rather than zeroing them.
    let mut untouched = DrumParams::new(DrumKind::Bd);
    untouched.apply_controls(&ValueMap::new());
    assert_eq!((untouched.gain, untouched.pan), (1.0, 0.5));
}

#[test]
fn drum_produces_sound_then_finishes() {
    for kind in KINDS {
        let mut v = DrumVoice::new(DrumParams::new(kind), SAMPLE_RATE);
        let mut out = Vec::new();
        let mut ticks = 0;
        for _ in 0..(44100 * 2) {
            let (l, _r) = v.tick();
            out.push(l);
            ticks += 1;
            if v.is_done() {
                break;
            }
        }
        assert_is_signal(&out, &format!("{kind:?}"));
        assert!(v.is_done(), "{kind:?} should finish");
        assert!(ticks < 44100 * 2, "{kind:?} should finish within 2s");
    }
}

// --- what the kinds are supposed to *be* -----------------------------------
//
// These drums are rudel's own design — superdough plays samples here, so there
// is no upstream to check against and the goldens below can only say "this has
// not changed". The assertions in this section say what each kind is meant to
// be instead, so a coefficient that gets edited to something musically wrong
// fails here rather than merely showing up as a diff to re-bless.

/// Mean interval between rising zero crossings, in samples — one over the
/// average frequency across the window.
fn zero_crossing_period(samples: &[f32]) -> f32 {
    let crossings: Vec<usize> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0)
        .map(|(i, _)| i)
        .collect();
    assert!(crossings.len() >= 2, "not enough zero crossings to measure");
    (crossings[crossings.len() - 1] - crossings[0]) as f32 / (crossings.len() - 1) as f32
}

#[test]
fn bass_drum_pitch_sweeps_downward() {
    // `48 + 90*exp(-t/0.03)`: the click starts up around 138Hz and drops toward
    // the 48Hz body. If that decay were added rather than subtracted, or the
    // envelope inverted, the sweep would run the other way.
    let out = render(DrumKind::Bd, (0.25 * SAMPLE_RATE) as usize);
    let early = zero_crossing_period(&out[..2000]);
    let late = zero_crossing_period(&out[6000..]);
    assert!(
        late > early * 1.5,
        "bd should fall in pitch: early period {early:.1} samples, late {late:.1}"
    );

    // ...and it settles near the 48Hz floor rather than anywhere else.
    let settled = SAMPLE_RATE / zero_crossing_period(&out[8000..]);
    assert!(
        (settled - 48.0).abs() < 6.0,
        "bd should settle near 48Hz, got {settled:.1}Hz"
    );
}

#[test]
fn toms_are_tuned_low_mid_high() {
    let pitch = |kind| {
        let out = render(kind, (0.2 * SAMPLE_RATE) as usize);
        SAMPLE_RATE / zero_crossing_period(&out[4000..])
    };
    let (lt, mt, ht) = (
        pitch(DrumKind::Lt),
        pitch(DrumKind::Mt),
        pitch(DrumKind::Ht),
    );
    assert!(
        lt < mt && mt < ht,
        "toms should rise in pitch: lt {lt:.0}Hz, mt {mt:.0}Hz, ht {ht:.0}Hz"
    );
    // They are tuned to 90/150/230Hz plus a sweep that has decayed by here.
    for (kind, want, got) in [("lt", 90.0, lt), ("mt", 150.0, mt), ("ht", 230.0, ht)] {
        assert!(
            (got - want) / want < 0.25,
            "{kind} should settle near {want}Hz, got {got:.0}Hz"
        );
    }
}

#[test]
fn hats_and_cymbals_ring_out_in_the_expected_order() {
    // Closed hat is the shortest, then open hat, ride, crash: the whole point of
    // having separate kinds. Measured as the time to fall below a twentieth of
    // the peak rather than the declared lifetime, so it tracks the envelope
    // coefficients rather than the `done_at` bookkeeping.
    let decay = |kind| {
        let out = render(kind, (1.5 * SAMPLE_RATE) as usize);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        out.iter()
            .rposition(|s| s.abs() > peak * 0.05)
            .expect("some audible span") as f32
            / SAMPLE_RATE
    };
    let (hh, oh, rd, cr) = (
        decay(DrumKind::Hh),
        decay(DrumKind::Oh),
        decay(DrumKind::Rd),
        decay(DrumKind::Cr),
    );
    assert!(
        hh < oh && oh < rd && rd < cr,
        "hh {hh:.3}s < oh {oh:.3}s < rd {rd:.3}s < cr {cr:.3}s"
    );
}

#[test]
fn hats_are_high_passed_and_the_kick_is_not() {
    // hh/oh/rd/cr run through a highpass; bd has no built-in filter. Compare
    // energy above and below ~5kHz by how much a one-pole difference
    // (a crude highpass) keeps.
    let brightness = |kind| {
        let out = render(kind, (0.1 * SAMPLE_RATE) as usize);
        let total: f32 = out.iter().map(|s| s * s).sum();
        let high: f32 = out.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
        high / total.max(1e-12)
    };
    let kick = brightness(DrumKind::Bd);
    for kind in [DrumKind::Hh, DrumKind::Oh, DrumKind::Cr] {
        assert!(
            brightness(kind) > kick * 10.0,
            "{kind:?} should be far brighter than bd ({:.4} vs {kick:.4})",
            brightness(kind)
        );
    }
}

#[test]
fn clap_decays_in_three_overlapping_stages() {
    // The envelope sums exponentials with 12ms, 12ms-from-10ms and 20ms-from-
    // 20ms time constants. Because the later two are `(t - offset).max(0.0)`,
    // they sit at exp(0) = 1 until their onset instead of at 0 — so all three
    // start together and the result decays monotonically rather than
    // re-attacking. (The comment on the voice says "three quick bursts"; the
    // arithmetic does not produce bursts. Left alone deliberately — changing it
    // changes what `cp` sounds like, and there is no upstream to arbitrate.)
    //
    // What is checkable is that the extra two stages carry the tail: a single
    // 12ms exponential would be down to exp(-20/12) = 0.19 of its peak by 20ms,
    // and this stays well above that.
    let out = render(DrumKind::Clap, (0.05 * SAMPLE_RATE) as usize);
    let block = (0.001 * SAMPLE_RATE) as usize;
    let env: Vec<f32> = out
        .chunks(block)
        .map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs())))
        .collect();

    let ratio = env[20] / env[0];
    assert!(
        ratio > 0.35,
        "clap's later stages should fatten the tail well past a single 12ms \
         decay (0.19); got {ratio:.3}"
    );
    // ...and it really is monotone-ish rather than a burst train: no block
    // recovers by more than a tenth over the one before it.
    assert!(
        env.windows(2).all(|w| w[1] <= w[0] * 1.15),
        "clap envelope should not re-attack: {env:?}"
    );
    // The per-block check above is too coarse on its own — a third stage that
    // grew instead of decaying only gains exp(0.05) = 5% per block, under that
    // threshold. Anchoring the far end pins the sign and the 0.02 time constant
    // as well: by 50ms the whole envelope is down to about a tenth.
    let tail = env[env.len() - 1] / env[0];
    assert!(
        tail < 0.25,
        "clap should be most of the way gone by 50ms; tail is {tail:.3} of the peak"
    );
}

#[test]
fn pan_splits_the_channels_with_constant_power() {
    let mut centred = DrumVoice::new(DrumParams::new(DrumKind::Bd), SAMPLE_RATE);
    let (l, r) = centred.tick();
    assert!((l - r).abs() < 1e-6, "pan 0.5 should be centred");

    let mut left = DrumParams::new(DrumKind::Bd);
    left.pan = 0.0;
    let (l, r) = DrumVoice::new(left, SAMPLE_RATE).tick();
    assert!(l.abs() > 0.0 && r.abs() < 1e-6, "pan 0 is hard left");

    let mut right = DrumParams::new(DrumKind::Bd);
    right.pan = 1.0;
    let (l, r) = DrumVoice::new(right, SAMPLE_RATE).tick();
    assert!(r.abs() > 0.0 && l.abs() < 1e-6, "pan 1 is hard right");

    // Out-of-range pan clamps rather than wrapping or inverting.
    let mut over = DrumParams::new(DrumKind::Bd);
    over.pan = 4.0;
    let (l, r) = DrumVoice::new(over, SAMPLE_RATE).tick();
    assert!(
        r.abs() > 0.0 && l.abs() < 1e-6,
        "pan above 1 clamps to hard right"
    );
}

#[test]
fn gain_scales_the_output_linearly() {
    let at = |gain: f32| {
        let mut p = DrumParams::new(DrumKind::Bd);
        p.gain = gain;
        let mut v = DrumVoice::new(p, SAMPLE_RATE);
        (0..512)
            .map(|_| v.tick().0)
            .fold(0.0f32, |m, s| m.max(s.abs()))
    };
    let unit = at(1.0);
    assert!(
        (at(0.5) - unit * 0.5).abs() < 1e-5,
        "gain should scale linearly"
    );
    assert!(
        (at(2.0) - unit * 2.0).abs() < 1e-5,
        "gain above 1 should scale too"
    );
    assert_eq!(at(0.0), 0.0, "zero gain is silence");
}

// --- exact-waveform snapshot ------------------------------------------------

/// First 512 samples of each kind. **Not a parity golden**: it is generated from
/// this crate, so it can only catch an *unintended* change to the drum voicing —
/// it cannot tell you the voicing is right. The assertions above are what say
/// that. Regenerate deliberately, after listening, with:
///
/// ```text
/// cargo test -p rudel-dsp --lib regenerate_drum_snapshot -- --ignored
/// ```
const SNAPSHOT_SAMPLES: usize = 512;
const SNAPSHOT: &str = include_str!("drum_snapshot.json");

fn snapshot_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/drum_snapshot.json")
}

fn snapshot_key(kind: DrumKind, rate: f32) -> String {
    format!("{kind:?}@{rate}")
}

#[test]
fn drum_voicing_has_not_drifted() {
    let want: serde_json::Value = serde_json::from_str(SNAPSHOT).expect("parse snapshot");
    for rate in SNAPSHOT_RATES {
        for kind in KINDS {
            let key = snapshot_key(kind, rate);
            let want: Vec<f32> = want[&key]
                .as_array()
                .unwrap_or_else(|| panic!("snapshot missing {key}; regenerate it"))
                .iter()
                .map(|x| x.as_f64().unwrap() as f32)
                .collect();
            let got = render_at(kind, SNAPSHOT_SAMPLES, rate);
            assert_eq!(got.len(), want.len(), "{key}: length");
            for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                assert!(
                    g.is_finite() && (g - w).abs() < 1e-6,
                    "{key} changed at sample {i}: was {w}, now {g}. If deliberate, \
                     regenerate with `cargo test -p rudel-dsp --lib \
                     regenerate_drum_snapshot -- --ignored`"
                );
            }
        }
    }
}

#[test]
#[ignore = "rewrites drum_snapshot.json; run deliberately after changing a drum voice"]
fn regenerate_drum_snapshot() {
    let mut map = serde_json::Map::new();
    for rate in SNAPSHOT_RATES {
        for kind in KINDS {
            let samples: Vec<serde_json::Value> = render_at(kind, SNAPSHOT_SAMPLES, rate)
                .into_iter()
                .map(|s| serde_json::json!(s))
                .collect();
            map.insert(snapshot_key(kind, rate), serde_json::Value::Array(samples));
        }
    }
    let json = serde_json::to_string(&serde_json::Value::Object(map)).expect("serialize");
    std::fs::write(snapshot_path(), json).expect("write snapshot");
    println!("wrote {}", snapshot_path().display());
}

#[test]
fn drums_are_deterministic_across_voices() {
    // The snapshot is only meaningful if a fresh voice always renders the same
    // thing — the noise source is seeded per voice, not shared.
    for kind in KINDS {
        assert_eq!(
            render(kind, 256),
            render(kind, 256),
            "{kind:?} should render identically every time"
        );
    }
}
