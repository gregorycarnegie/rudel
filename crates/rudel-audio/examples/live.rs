// Evaluates a Koto script into a pattern and plays it.
// Pass a script as the first arg, or a default is used.
// Run with:  cargo run -p rudel-audio --example live -- 'note("c4 e4 g4 b4").fast(2).room(0.4)'
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_audio::Engine;

fn main() {
    let script = std::env::args().nth(1).unwrap_or_else(|| {
        r#"stack(
            note("c4 e4 g4 b4 a4 g4 e4 d4").s("triangle").room(0.5),
            note("c2 ~ g2 ~").s("saw").cutoff("400 1600").gain(0.6).delay(0.3)
        )"#
        .to_string()
    });

    let pat = match rudel_lang::eval(&script) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("script error: {e}");
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
    engine.set_cps(0.5);
    engine.set_pattern(pat);

    println!("playing Koto-evaluated pattern for 8 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(8));
    println!("done");
}
