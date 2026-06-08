//! Plays a drum pattern from a directory of samples.
//! Each subfolder is a sound name (e.g. `bd/`, `sd/`, `hh/`) holding wav files.
//!
//! Run with: `cargo run -p rudel-audio --example samples -- C:\path\to\samples`
//! SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_audio::Engine;

fn main() {
    rudel_mini::install();

    let dir = match std::env::args().nth(1) {
        Some(d) => d,
        None => {
            eprintln!("usage: cargo run -p rudel-audio --example samples -- <samples-dir>");
            eprintln!("(each subfolder = a sound name, e.g. bd/, sd/, hh/)");
            return;
        }
    };

    let engine = match Engine::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("could not start audio engine: {e}");
            return;
        }
    };

    match engine.load_samples(&dir) {
        Ok(n) => println!("loaded {n} samples from {dir}"),
        Err(e) => {
            eprintln!("could not load samples: {e}");
            return;
        }
    }

    engine.set_cps(0.5);
    let pat = rudel_core::stack(&[
        rudel_core::s("bd ~ bd ~"),
        rudel_core::s("~ sd ~ sd"),
        rudel_core::s("hh*8").gain(0.6),
    ]);
    engine.set_pattern(pat);

    println!("playing for 8 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(8));
    println!("done");
}
