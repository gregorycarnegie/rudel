// Plays a mini-notation pattern for a few seconds through the default output.
// Run with:  cargo run -p rudel-audio --example play
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_audio::Engine;

fn main() {
    // Make &str parse as mini-notation everywhere.
    rudel_mini::install();

    let engine = match Engine::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("could not start audio engine: {e}");
            return;
        }
    };
    engine.set_cps(0.5);

    // A little melody with a euclidean bass-ish line, using the built-in synths.
    let pat = rudel_core::stack(&[
        rudel_core::note("c4 e4 g4 b4").s("triangle"),
        rudel_core::note("c2 ~ g2 ~").s("saw").gain(0.6),
    ]);
    engine.set_pattern(pat);

    println!("playing for 8 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(8));
    println!("done");
}
