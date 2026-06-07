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
        "",
        "#[derive(Debug)]",
        "pub(crate) struct TuneScale {",
        "    pub(crate) name: &'static str,",
        "    pub(crate) freqs: &'static [f64],",
        "}",
        "",
        "#[rustfmt::skip]",
        "pub(crate) static TUNE_SCALES: &[TuneScale] = &[",
    ]
    count = 0
    for name in sorted(archive):
        freqs = archive[name].get("frequencies", [])
        if not freqs:
            continue
        name_lit = json.dumps(name)
        def f64_lit(x: object) -> str:
            lit = f"{float(x):.15g}"
            if "." not in lit and "e" not in lit and "E" not in lit:
                lit += ".0"
            return lit

        freq_lits = ", ".join(f64_lit(x) for x in freqs)
        lines.append(f"    TuneScale {{ name: {name_lit}, freqs: &[{freq_lits}] }},")
        count += 1
    lines.append("];")
    lines.append("")
    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {OUT} with {count} scales")


if __name__ == "__main__":
    main()
