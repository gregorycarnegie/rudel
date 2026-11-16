//! The Rust dough engine against the C it was ported from, on random events.
//!
//!   cargo test -p rudel-dough --features c-reference --test c_reference
//!
//! The browser comparison (wasm_golden.rs) pins seventeen hand-picked cases.
//! This drives both engines with a few hundred seeded random scenarios instead,
//! each a handful of messages over every parameter dough reads (sources,
//! samples, envelopes, filters and slopes, pitch and FM envelopes, vibrato,
//! amplitude and ring modulation, phaser, flanger, chorus, coarse, crush,
//! distortion, panning, delay, reverb, voice groups, glide, retriggers, timed
//! and repeating events, and every command), and requires every frame to match
//! bit for bit. Values stay inside what the C accepts without undefined
//! behaviour: `coarse` is at least 1, `delaytime` is not negative.
//!
//! The scenarios run in order on one engine of each kind, reset between them,
//! as the browser comparison does, so what a reset keeps carries over alike.
#![cfg(feature = "c-reference")]

use rudel_dough::{Dough, c_reference::CDough};

const RATE: u32 = 44_100;
const BLOCK: usize = 128;
const FRAMES: usize = 4096;
const SCENARIOS: usize = 400;

/// xorshift64*, so the scenarios are the same every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// A value in [lo, hi), to three decimals, as a message carries it.
    fn range(&mut self, lo: f64, hi: f64) -> String {
        let unit = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        format!("{:.3}", lo + unit * (hi - lo))
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }
}

/// `key/value` pairs for one event, each parameter group included by chance.
fn parameters(r: &mut Random) -> Vec<String> {
    let mut p: Vec<String> = Vec::new();
    let set = |p: &mut Vec<String>, key: &str, value: String| {
        p.push(format!("{key}/{value}"));
    };
    let sources = [
        "tri", "sine", "saw", "zaw", "pulse", "square", "pulze", "white", "pink", "brown", "const",
        "nope",
    ];
    if r.chance(15) {
        set(&mut p, "s", "smp:0".into());
        set(&mut p, "file", "0".into());
        if r.chance(50) {
            set(&mut p, "begin", r.range(0.0, 0.5));
            set(&mut p, "end", r.range(0.4, 1.0));
        }
    } else {
        set(&mut p, "s", r.pick(&sources).into());
    }
    if r.chance(50) {
        set(&mut p, "note", r.range(24.0, 96.0));
    } else if r.chance(70) {
        set(&mut p, "freq", r.range(30.0, 2000.0));
    }
    if r.chance(20) {
        set(&mut p, "speed", r.range(-2.0, 2.0));
    }
    if r.chance(80) {
        set(
            &mut p,
            "duration",
            r.pick(&["0", "0.02", "0.05", "0.08"]).into(),
        );
    }
    for (key, lo, hi, percent) in [
        ("gain", 0.0, 1.5, 30),
        ("velocity", 0.0, 1.0, 15),
        ("postgain", 0.0, 2.0, 15),
        ("pan", 0.0, 1.0, 30),
        ("pw", 0.05, 0.95, 20),
    ] {
        if r.chance(percent) {
            set(&mut p, key, r.range(lo, hi));
        }
    }
    let envelope = |r: &mut Random, p: &mut Vec<String>, keys: [&str; 4]| {
        for (key, lo, hi) in [
            (keys[0], 0.0, 0.03),
            (keys[1], 0.0, 0.05),
            (keys[2], 0.0, 1.0),
            (keys[3], 0.0, 0.03),
        ] {
            if r.chance(50) {
                p.push(format!("{key}/{}", r.range(lo, hi)));
            }
        }
    };
    if r.chance(40) {
        envelope(r, &mut p, ["attack", "decay", "sustain", "release"]);
    }
    if r.chance(40) {
        set(&mut p, "lpf", r.range(50.0, 18000.0));
        set(&mut p, "lpq", r.range(0.0, 1.0));
        if r.chance(50) {
            set(&mut p, "lpe", r.range(-1.0, 2.0));
            envelope(r, &mut p, ["lpa", "lpd", "lps", "lpr"]);
        }
    }
    if r.chance(25) {
        set(&mut p, "hpf", r.range(20.0, 8000.0));
        set(&mut p, "hpq", r.range(0.0, 1.0));
        if r.chance(50) {
            set(&mut p, "hpe", r.range(-1.0, 2.0));
            envelope(r, &mut p, ["hpa", "hpd", "hps", "hpr"]);
        }
    }
    if r.chance(20) {
        set(&mut p, "bpf", r.range(100.0, 10000.0));
        set(&mut p, "bpq", r.range(0.1, 10.0));
        if r.chance(50) {
            set(&mut p, "bpe", r.range(-1.0, 2.0));
            envelope(r, &mut p, ["bpa", "bpd", "bps", "bpr"]);
        }
    }
    if r.chance(30) {
        set(
            &mut p,
            "ftype",
            r.pick(&["12db", "24db", "48db", "0", "1", "2"]).into(),
        );
    }
    if r.chance(20) {
        set(&mut p, "penv", r.range(-24.0, 24.0));
        set(&mut p, "psus", r.pick(&["0", "1", "0.5"]).into());
        envelope(r, &mut p, ["patt", "pdec", "prel", "prel"]);
    }
    if r.chance(20) {
        set(&mut p, "vib", r.range(0.0, 10.0));
        set(&mut p, "vibmod", r.range(0.0, 1.0));
    }
    if r.chance(25) {
        set(&mut p, "fm", r.range(0.0, 5.0));
        set(&mut p, "fmh", r.range(0.5, 4.0));
        if r.chance(50) {
            set(&mut p, "fme", r.range(-1.0, 3.0));
            envelope(r, &mut p, ["fma", "fmd", "fms", "fmr"]);
        }
    }
    for (keys, ranges, percent) in [
        (&["am", "amdepth"][..], &[(0.0, 20.0), (0.0, 1.2)][..], 15),
        (&["rm", "rmdepth"][..], &[(0.0, 1000.0), (0.0, 1.2)][..], 15),
        (
            &["phaser", "phaserdepth", "phasersweep", "phasercenter"][..],
            &[(0.0, 5.0), (0.0, 1.0), (0.0, 4000.0), (100.0, 4000.0)][..],
            15,
        ),
        (
            &["flanger", "flangerdepth", "flangerfeedback"][..],
            &[(0.0, 3.0), (0.0, 1.0), (0.0, 1.0)][..],
            15,
        ),
        (
            &["chorus", "chorusdepth", "chorusdelay"][..],
            &[(0.0, 3.0), (0.0, 1.0), (1.0, 40.0)][..],
            15,
        ),
        (
            &["distort", "distortvol"][..],
            &[(0.0, 3.0), (0.0, 1.5)][..],
            15,
        ),
        (
            &["delay", "delaytime", "delayfeedback"][..],
            &[(0.0, 1.0), (0.0, 0.99), (0.0, 0.95)][..],
            20,
        ),
        (
            &["room", "verbdecay", "verbdamp", "verbpredelay", "verbdiff"][..],
            &[(0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0)][..],
            20,
        ),
    ] {
        if r.chance(percent) {
            for (key, &(lo, hi)) in keys.iter().zip(ranges) {
                if r.chance(75) {
                    set(&mut p, key, r.range(lo, hi));
                }
            }
        }
    }
    if r.chance(15) {
        set(&mut p, "coarse", (1 + r.below(16)).to_string());
    }
    if r.chance(15) {
        set(&mut p, "crush", r.range(1.0, 16.0));
    }
    if r.chance(10) {
        set(&mut p, "orbit", r.below(4).to_string());
    }
    if r.chance(30) {
        set(&mut p, "group", r.below(3).to_string());
        set(&mut p, "voices", (1 + r.below(3)).to_string());
        if r.chance(50) {
            set(&mut p, "glide", r.range(0.0, 0.1));
        }
        if r.chance(30) {
            set(&mut p, "reset", r.below(2).to_string());
        }
    }
    p
}

/// One scenario: a few messages, the later ones possibly timed or commands.
fn scenario(r: &mut Random) -> Vec<String> {
    let commands = [
        "update_all",
        "update_group",
        "hush",
        "hush_endless",
        "hush_group",
        "panic",
    ];
    let count = 1 + r.below(5);
    (0..count)
        .map(|i| {
            let command = if i > 0 && r.chance(25) {
                r.pick(&commands)
            } else {
                "play"
            };
            let mut message = format!("dough/{command}");
            for pair in parameters(r) {
                message.push('/');
                message.push_str(&pair);
            }
            if i > 0 && r.chance(60) {
                message.push_str(&format!("/time/{}", r.range(0.0, 0.08)));
                if r.chance(15) {
                    message.push_str(&format!("/repeat/{}", r.range(0.01, 0.03)));
                }
            }
            message
        })
        .collect()
}

#[test]
fn the_port_renders_what_the_c_does() {
    let pcm: Vec<f32> = (0..3000)
        .map(|i| ((i as f32) * 0.031).sin() * (1.0 - i as f32 / 3000.0))
        .collect();
    let mut rust = Dough::new(RATE);
    assert_eq!(rust.sample_file("smp:0", &pcm, 1), Some(0));
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let mut failures = Vec::new();
    let mut c = None;
    for n in 0..SCENARIOS {
        let messages = scenario(&mut random);
        // The C engine is reset by taking it again (its wrapper's clock
        // restarts with it); the Rust one by `dough/reset`.
        drop(c.take());
        let mut engine = CDough::acquire(RATE).expect("the C engine is free");
        if n == 0 {
            assert_eq!(engine.sample_file("smp:0", &pcm, 1), Some(0));
        } else {
            assert!(rust.evaluate("dough/reset"));
        }
        for m in &messages {
            assert!(engine.evaluate(m), "C refused {m}");
            assert!(rust.evaluate(m), "Rust refused {m}");
        }
        let mut out_c = vec![(0.0f32, 0.0f32); FRAMES];
        let mut out_rust = vec![(0.0f32, 0.0f32); FRAMES];
        for (a, b) in out_c.chunks_mut(BLOCK).zip(out_rust.chunks_mut(BLOCK)) {
            engine.render_into(a);
            rust.render_into(b);
        }
        c = Some(engine);
        let bits = |f: &(f32, f32)| (f.0.to_bits(), f.1.to_bits());
        if let Some(frame) = (0..FRAMES).find(|&i| bits(&out_c[i]) != bits(&out_rust[i])) {
            failures.push(format!(
                "scenario {n}, frame {frame}: C {:?}, Rust {:?}\n  {}",
                out_c[frame],
                out_rust[frame],
                messages.join("\n  ")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {SCENARIOS} scenarios differ; the first:\n{}",
        failures.len(),
        failures
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
