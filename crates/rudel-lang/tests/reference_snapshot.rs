//! Snapshot of Rudel's reference / autocomplete surface.
//!
//! `rudel_lang::reference()` is the single source the editor uses for keyword
//! highlighting, the reference panel, and autocomplete candidates. This test
//! snapshots that surface (top-level functions, pattern methods, controls) to a
//! committed file so any change to what Rudel exposes is visible in review,
//! rather than silently shifting the autocomplete/reference output.
//!
//!   RUDEL_BLESS=1 cargo test -p rudel-lang --test reference_snapshot

const SNAPSHOT_PATH: &str = "tests/reference_surface.txt";

fn render() -> String {
    let r = rudel_lang::reference();
    // reference() already returns each list sorted+deduped.
    let mut out = String::new();
    out.push_str("# Rudel reference / autocomplete surface snapshot\n");
    out.push_str("# Generated from rudel_lang::reference(); do not edit by hand.\n");
    out.push_str("# Regenerate: RUDEL_BLESS=1 cargo test -p rudel-lang --test reference_snapshot\n");
    out.push_str(&format!(
        "# functions={} methods={} controls={}\n",
        r.functions.len(),
        r.methods.len(),
        r.controls.len(),
    ));
    for (heading, list) in [
        ("[functions]", &r.functions),
        ("[methods]", &r.methods),
        ("[controls]", &r.controls),
    ] {
        out.push('\n');
        out.push_str(heading);
        out.push('\n');
        for name in list {
            out.push_str(name);
            out.push('\n');
        }
    }
    out
}

#[test]
fn reference_surface_snapshot_is_in_sync() {
    let generated = render();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SNAPSHOT_PATH);
    if std::env::var("RUDEL_BLESS").is_ok() {
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    let norm = |s: &str| s.replace("\r\n", "\n");
    assert_eq!(
        norm(&committed),
        norm(&generated),
        "Rudel reference surface changed; review the diff and regenerate with \
         `RUDEL_BLESS=1 cargo test -p rudel-lang --test reference_snapshot`"
    );
}
