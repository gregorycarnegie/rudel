import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "strudel" / "packages" / "xen" / "tunejs.js"
OUT = ROOT / "crates" / "rudel-core" / "src" / "tune_table.rs"


def main() -> None:
    text = SRC.read_text(encoding="utf-8")
    match = re.search(r"var TuningList = (\{.*\})\s*$", text, re.S)
    if not match:
        match = re.search(r"var TuningList = (\{.*?\});\s*(?:\n|$)", text, re.S)
    if not match:
        raise SystemExit("TuningList not found")

    archive = json.loads(match.group(1))
    lines = [
        "// Generated from strudel/packages/xen/tunejs.js. Do not edit by hand.",
        "// Strudel/Tune.js scale names are case-sensitive.",
        "//",
        "// tune.js stores each scale as absolute frequencies whose final entry is the",
        "// octave duplicate of the tonic. Rudel only consumes octave-normalised ratios,",
        "// so they are pre-divided here (each freq / the tonic, dropping the octave) at",
        "// generation time. Named-scale lookups therefore return a static ratio slice",
        "// directly, with no per-call division or allocation.",
        "",
        "#[rustfmt::skip]",
        "pub(crate) static TUNE_SCALES: phf::Map<&'static str, &'static [f64]> = phf::phf_map! {",
    ]
    count = 0
    for name in sorted(archive):
        freqs = archive[name].get("frequencies", [])
        if not freqs:
            continue
        tonic = float(freqs[0])
        if tonic == 0.0:
            continue
        # Drop the trailing octave duplicate and normalise to ratios. This mirrors
        # rudel's previous runtime `ratios_from_frequencies`: same IEEE-754 double
        # division, so the emitted literals are bit-identical to that conversion.
        ratios = [float(f) / tonic for f in freqs[:-1]] or [1.0]
        name_lit = json.dumps(name)
        # `repr` gives the shortest round-tripping decimal; Rust's f64 parser is
        # round-trip correct, so each literal reloads to the exact same double.
        ratio_lits = ", ".join(repr(r) for r in ratios)
        lines.append(f"    {name_lit} => &[{ratio_lits}],")
        count += 1
    lines.append("};")
    lines.append("")
    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {OUT} with {count} scales")


if __name__ == "__main__":
    main()
