use super::common::*;

/// Every mondo spelling must produce the same haps as the Koto/mini spelling it
/// stands for — the two front-ends share one pattern engine, so any difference
/// is a compiler bug rather than a dialect.
fn same(mondo: &str, koto: &str) {
    let a = eval(&format!("mondo`{mondo}`")).unwrap_or_else(|e| panic!("mondo `{mondo}`: {e}"));
    let b = eval(koto).unwrap_or_else(|e| panic!("koto `{koto}`: {e}"));
    assert_eq!(shape(&a, 2), shape(&b, 2), "`{mondo}` vs `{koto}`");
}

#[test]
fn calls_and_chains_match_their_koto_spelling() {
    same("s hh*8", r#"s("hh*8")"#);
    same("s jazz # fast 2", r#"s("jazz").fast(2)"#);
    same(
        "n <0 2 4> # scale 'C4:minor'",
        r#"n("<0 2 4>").scale("C4:minor")"#,
    );
    same("n 0 # jux rev", r#"n("0").jux(rev)"#);
    // Round parens apply a function to one element of a sequence, which JS
    // needs a seq() and a string boundary to say.
    same(
        "s [bd hh bd (cp # delay .6)] # bank tr909",
        r#"seq(s("bd"), s("hh"), s("bd"), s("cp").delay(0.6)).bank("tr909")"#,
    );
}

#[test]
fn brackets_match_mini_notation() {
    same("s [bd hh]", r#"s("bd hh")"#);
    same("s [bd [hh oh] cp]", r#"s("bd [hh oh] cp")"#);
    same("s <bd hh>", r#"s("<bd hh>")"#);
    same("s [bd hh@3]", r#"s("bd hh@3")"#);
    same("s [bd hh!3]", r#"s("bd hh!3")"#);
    same("s [bd, hh*4]", r#"s("bd, hh*4")"#);
    same("s [bd ~ hh]", r#"s("bd ~ hh")"#);
    same("s bd:3", r#"s("bd:3")"#);
    same("n 0..7", r#"n("0 .. 7")"#);
    same("s bd&3:8", r#"s("bd(3,8)")"#);
    same("s bd*<2 3>", r#"s("bd*<2 3>")"#);
    same("s {bd hh cp}%4", r#"s("{bd hh cp}%4")"#);
}

#[test]
fn stacks_and_defs_match_their_koto_spelling() {
    same(
        "$ s [bd rim] $ n 0 # s sawtooth",
        r#"stack(s("bd rim"), n("0").s("sawtooth"))"#,
    );
    // A `def` is silent itself and substitutes at each use.
    same(
        "$ def melody [0 1 2] $ n melody # scale 'C:minor'",
        r#"n("0 1 2").scale("C:minor")"#,
    );
}

#[test]
fn lambdas_match_an_arrow_function() {
    same(
        "n 0..7 # scale 'C:minor' # sometimes (# dec .1)",
        r#"n("0 .. 7").scale("C:minor").sometimes(x => x.dec(0.1))"#,
    );
    same(
        "n 0..7 # sometimes (# dec .1 # jux rev)",
        r#"n("0 .. 7").sometimes(x => x.dec(0.1).jux(rev))"#,
    );
}

#[test]
fn mondi_reads_its_argument_as_a_sequence() {
    same("[bd hh]", "mondi`bd hh`");
}

#[test]
fn a_parse_error_reaches_the_user() {
    let Err(err) = eval("mondo`s [bd`") else {
        panic!("an unclosed bracket should not evaluate");
    };
    assert!(err.contains("mondo:"), "{err}");
}

/// The whole example from the Mondo Notation page, verbatim, under the marker
/// line — which is how a script pasted from upstream's docs arrives. A smoke
/// test that a realistic program compiles and plays.
const DOC_EXAMPLE: &str = r#"$ note (c2 # euclid <3 6 3> <8 16>) # *2
# s "sine" # add (note [0 <12 24>]*2)
# dec(sine # range .2 2)
# room .5
# lpf (sine/3 # range 120 400)
# lpenv (rand # range .5 4)
# lpq (perlin # range 5 12 # * 2)
# dist 1 # fm 4 # fmh 5.01 # fmdecay <.1 .2>
# postgain .6 # delay .1 # clip 5

$ s [bd bd bd bd] # bank tr909 # clip .5

# ply <1 [1 [2 4]]>

$ s oh*4 # press # bank tr909 # speed.8

# dec (<.02 .05>*2 # add (saw/8 # range 0 1))
"#;

#[test]
fn a_marked_script_is_read_as_mondo() {
    for src in [
        format!("// mondo\n{DOC_EXAMPLE}"),
        // The marker is a line of its own, however it is spaced.
        format!("\n//mondo  \n{DOC_EXAMPLE}"),
    ] {
        let pat = eval(&src).unwrap_or_else(|e| panic!("doc example: {e}"));
        let haps = shape(&pat, 2);
        assert!(
            haps.len() > 20,
            "expected a busy pattern, got {}",
            haps.len()
        );
    }
    // Wrapping the same thing in the tag is the other way to say it.
    let tagged = eval(&format!("mondo`{DOC_EXAMPLE}`")).expect("tagged");
    let marked = eval(&format!("// mondo\n{DOC_EXAMPLE}")).expect("marked");
    assert_eq!(shape(&tagged, 2), shape(&marked, 2));
}

#[test]
fn an_unmarked_mondo_script_is_told_what_it_needs() {
    // Koto's `unexpected token` at the first `$` says nothing about why, and
    // pasting the notation bare is the obvious first thing to try.
    let Err(err) = eval(DOC_EXAMPLE) else {
        panic!("mondo is not valid Koto");
    };
    assert!(err.contains("Mondo Notation"), "{err}");
    assert!(err.contains("// mondo"), "{err}");
    // A Koto script with an ordinary mistake keeps its own error.
    let Err(err) = eval(r#"s("bd sd".fast(2)"#) else {
        panic!("unbalanced parens");
    };
    assert!(!err.contains("Mondo Notation"), "{err}");
    // The marker is one exact spelling, and a near miss is told which.
    let Err(err) = eval(&format!("// Mondo\n{DOC_EXAMPLE}")) else {
        panic!("`// Mondo` is not the marker");
    };
    assert!(err.contains("// mondo"), "{err}");
}

/// The two spellings the README puts side by side, so the claim that they are
/// the same pattern stays true.
#[test]
fn the_readme_example_matches_its_koto_spelling() {
    let mondo = "// mondo
$ s [bd rim [~ bd] rim] # bank tr707
$ n <0 2 4 [3 1] -1>*4 # scale C4:minor # jux rev # dec .2 # delay .5
";
    let koto = r#"stack(
  s("bd rim [~ bd] rim").bank("tr707"),
  n("<0 2 4 [3 1] -1>*4").scale("C4:minor").jux(rev).dec(0.2).delay(0.5)
)"#;
    let a = eval(mondo).unwrap_or_else(|e| panic!("mondo: {e}"));
    let b = eval(koto).unwrap_or_else(|e| panic!("koto: {e}"));
    assert_eq!(shape(&a, 2), shape(&b, 2));
}
