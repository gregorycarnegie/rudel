//! Evaluate a directory of Strudel songs and report which ones rudel can read.
//!
//!     cargo run -p rudel-lang --example songs -- path/to/songs
//!
//! SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::Frac;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: songs <dir>");
    rudel_lang::install_mini();

    let path = std::path::PathBuf::from(&dir);
    let mut files: Vec<_> = if path.is_file() {
        vec![path]
    } else {
        std::fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                (path.extension()? == "js").then_some(path)
            })
            .collect()
    };
    files.sort();
    // A single file is a debugging run: show the whole error, not just line one.
    let full_errors = files.len() == 1;

    let (mut ok, mut failed) = (0, 0);
    for path in &files {
        let name = path.file_stem().unwrap().to_string_lossy();
        let source = std::fs::read_to_string(path).expect("read song");
        match rudel_lang::eval_result(&source) {
            Ok(result) => {
                // Eval alone proves it parses; querying proves it produces events.
                let haps = result.pattern.query_arc(Frac::zero(), Frac::int(4));
                if haps.is_empty() {
                    failed += 1;
                    println!("EMPTY {name}: evaluated but no events in 4 cycles");
                } else {
                    ok += 1;
                    println!("OK    {name}: {} haps", haps.len());
                }
            }
            Err(err) => {
                failed += 1;
                let err = if full_errors {
                    err.trim().to_string()
                } else {
                    err.lines().next().unwrap_or("").to_string()
                };
                println!("FAIL  {name}: {err}");
            }
        }
    }
    println!("\n{ok}/{} songs playable, {failed} failed", files.len());
}
