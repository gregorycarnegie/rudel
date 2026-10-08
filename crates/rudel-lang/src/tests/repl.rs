use super::common::*;
use rstest::rstest;

// REPL pattern slots (`p`/`d1`/`p1`/`q`) and `hush`. `eval` resets the slot
// registry on entry, so these tests are independent of each other.

fn id_of(v: &Value) -> Option<String> {
    match v {
        Value::Map(m) => m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()),
        _ => None,
    }
}

#[test]
fn d_slot_registers_and_tags_a_single_pattern() {
    // `note("c").d1` registers slot "1"; the result is that pattern, tagged
    // with its id.
    let pat = eval(r#"note("c").d1"#).expect("eval");
    let vals = values(&pat, 0, 1);
    assert_eq!(vals.len(), 1);
    assert_eq!(id_of(&vals[0]).as_deref(), Some("1"));
}

#[test]
fn multiple_slots_stack() {
    // Two slots across two statements stack into one pattern, even though a
    // script's value is only its last expression.
    let pat = eval("note(\"c\").d1\nnote(\"e\").d2").expect("eval");
    let vals = values(&pat, 0, 1);
    assert_eq!(vals.len(), 2);
    let ids: std::collections::BTreeSet<_> = vals.iter().filter_map(id_of).collect();
    assert_eq!(ids, ["1", "2"].iter().map(|s| s.to_string()).collect());
}

#[test]
fn p_and_p_slot_use_the_given_id() {
    // p("foo") uses a string id; p1() is shorthand for p(1).
    let pat = eval(r#"note("c").p("foo")"#).expect("eval");
    assert_eq!(id_of(&values(&pat, 0, 1)[0]).as_deref(), Some("foo"));
    let pat = eval(r#"note("c").p1"#).expect("eval");
    assert_eq!(id_of(&values(&pat, 0, 1)[0]).as_deref(), Some("1"));
}

#[test]
fn a_numeric_slot_id_renders_without_a_decimal_point() {
    // The id becomes a control value and a registry key, so `p(1)` has to read
    // as "1" rather than "1.0", whatever number type it arrives as.
    let id = |script: &str| id_of(&values(&eval(script).expect("eval"), 0, 1)[0]).expect("an id");
    assert_eq!(id(r#"note("c").p(1)"#), "1");
    assert_eq!(id(r#"note("c").p(12)"#), "12");
    assert_eq!(id(r#"note("c").p("1")"#), "1");
    // A genuinely fractional id keeps its point rather than being truncated.
    assert_eq!(id(r#"note("c").p(1.5)"#), "1.5");
}

#[test]
fn q_slot_is_silent() {
    // q/q1 mute their pattern (a queued slot): no events, nothing registered.
    let pat = eval(r#"note("c").q1"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 0);
    let pat = eval(r#"note("c").q("a")"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 0);
}

#[test]
fn underscore_id_mutes_the_slot() {
    // A `_x`/`x_` id mutes (Strudel's pattern-muting convention).
    let pat = eval(r#"note("c").p("_a")"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 0);
    let pat = eval(r#"note("c").p("a_")"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 0);
}

#[test]
fn hush_clears_registered_slots() {
    // Registering a slot then calling hush() yields silence (no events).
    let pat = eval("note(\"c\").d1\nhush()").expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 0);
}

#[test]
fn cpm_fasts_relative_to_cps() {
    // At the default cps (0.5), cpm(60) -> fast(60/60/0.5) = fast(2): one event
    // per cycle becomes two; cpm(30) -> fast(1) leaves it unchanged.
    let pat = eval(r#"note("c").cpm(60)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 2);
    let pat = eval(r#"note("c").cpm(30)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 1);
}

#[test]
fn slots_do_not_leak_between_evaluations() {
    // A slot registered in one eval must not appear in the next.
    let _ = eval(r#"note("c").d1"#).expect("eval");
    let pat = eval(r#"note("e")"#).expect("eval");
    let vals = values(&pat, 0, 1);
    assert_eq!(vals.len(), 1);
    assert_eq!(id_of(&vals[0]), None, "no slot id should carry over");
}

#[test]
fn all_transforms_the_stacked_patterns() {
    // `all(f)` applies `f` to the whole stack: two one-event slots stacked and
    // fast(2)'d yield four events per cycle.
    let src = "note(\"c\").d1\nnote(\"e\").d2\nall(x => x.fast(2))";
    let pat = eval(src).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
}

#[test]
fn all_on_labels_transforms_the_stack() {
    // `$:` labels are picked up by `all`, just like slots.
    let src = "$: note(\"c\")\n$: note(\"e\")\nall(x => x.fast(2))";
    let pat = eval(src).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
}

#[test]
fn each_transforms_every_pattern_separately() {
    // `each(f)` applies `f` to each registered pattern before stacking: two
    // slots, each fast(2)'d, give four events.
    let src = "note(\"c\").d1\nnote(\"e\").d2\neach(x => x.fast(2))";
    let pat = eval(src).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
}

#[test]
fn each_without_slots_transforms_the_script_pattern() {
    // With no registered slots, `each` applies to the script's own pattern.
    let src = "each(x => x.fast(2))\nnote(\"c\")";
    let pat = eval(src).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 2);
}

#[test]
fn solo_slot_silences_the_others() {
    // An `S`-prefixed key solos: only that pattern plays. `p("S1")` solos over a
    // plain `d2` slot.
    let src = "note(\"c\").d2\nnote(\"e\").p(\"S1\")";
    let pat = eval(src).expect("eval");
    let ids: Vec<String> = values(&pat, 0, 1).iter().filter_map(id_of).collect();
    assert_eq!(ids, vec!["S1".to_string()], "only the soloed slot plays");
}

#[test]
fn solo_keeps_all_soloed_patterns() {
    // Multiple soloed slots all play; non-soloed ones drop out.
    let src = "note(\"c\").d1\nnote(\"e\").p(\"S2\")\nnote(\"g\").p(\"S3\")";
    let pat = eval(src).expect("eval");
    let ids: std::collections::BTreeSet<String> =
        values(&pat, 0, 1).iter().filter_map(id_of).collect();
    assert_eq!(
        ids,
        ["S2", "S3"].iter().map(|s| s.to_string()).collect(),
        "both soloed slots play, the plain one is dropped"
    );
}

#[test]
fn combiners_do_not_leak_between_evaluations() {
    // An `all` transform set in one eval must not affect the next.
    let _ = eval("note(\"c\").d1\nall(x => x.fast(4))").expect("eval");
    let pat = eval(r#"note("e")"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 1, "all() must not carry over");
}

#[test]
fn a_label_alone_on_its_line_takes_a_multi_line_statement_below_it() {
    // The bracket the statement opens on its first line has to count, or the
    // label would end there with the bracket unclosed.
    let pat = eval("$:\nstack(\n  s(\"bd\"),\n  s(\"hh\")\n)").expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 2);
}

#[test]
fn a_blank_line_inside_a_label_s_chain_does_not_end_it() {
    // The chain picks up again with a leading dot, so the gap is part of it.
    let pat = eval("$: s(\"bd\")\n\n  .fast(2)").expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 2);
}

#[test]
fn each_anonymous_label_gets_its_own_id() {
    let pat = eval("$: s(\"a\")\n$: s(\"b\")").expect("eval");
    let mut ids: Vec<_> = values(&pat, 0, 1)
        .into_iter()
        .filter_map(|v| match v {
            Value::Map(m) => m.get("id").cloned(),
            _ => None,
        })
        .collect();
    ids.dedup();
    assert_eq!(ids.len(), 2, "{ids:?}");
}

// `S` alone and `bc` are ordinary names; neither silences the others.
#[rstest]
fn only_a_longer_name_starting_with_s_solos(
    #[values("S: s(\"x\")\nd: s(\"y\")", "bc: s(\"x\")\nd: s(\"y\")")] src: &str,
) {
    let pat = eval(src).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 2);
}

#[test]
fn a_label_s_name_mutes_and_solos_as_a_slot_id_does() {
    // The name reaches the slot registry from generated code, so a label
    // starting `_` mutes and one starting `S` solos.
    let muted = eval("_a: s(\"x\")\nb: s(\"y\")").expect("eval");
    assert_eq!(values(&muted, 0, 1).len(), 1);
    let soloed = eval("a: s(\"x\")\nSb: s(\"y\")").expect("eval");
    assert_eq!(values(&soloed, 0, 1).len(), 1);
}

#[test]
fn a_label_s_statement_runs_until_its_brackets_close() {
    // Neither line looks like a continuation (`)` is not one), so only the
    // bracket count keeps the closing line in the label. Cut short, the code
    // would still parse — the label's own `)` closes `stack(` — but the
    // `.fast(2)` would apply outside the label and never be heard.
    let pat = eval("$:\nstack(s(\"bd\"), s(\"hh\")\n).fast(2)").expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
    // A line ending in `.` carries on to the next.
    let pat = eval("$: s(\"bd\").\n  fast(2)").expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 2);
}

fn sounds(pat: &Pattern) -> Vec<String> {
    let mut out: Vec<String> = values(pat, 0, 1)
        .iter()
        .filter_map(|v| match v {
            Value::Map(m) => m.get("s").and_then(|v| v.as_str()).map(|s| s.to_string()),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

/// Evaluate the block of `doc` that `marker` starts.
fn block(doc: &str, marker: &str, blocks: &mut crate::Blocks) -> Result<Pattern, String> {
    let from = doc.find(marker).expect("marker");
    let to = doc[from..].find("\n\n").map_or(doc.len(), |n| from + n);
    crate::eval_result_with_source_range(&doc[from..to], (from, to), blocks).map(|r| r.pattern)
}

#[test]
fn a_block_evaluation_keeps_the_other_blocks_playing() {
    let mut blocks = crate::Blocks::default();
    let doc = "const pat = s(\"bd\")\nfunction twice(p) { return p.fast(2) }\n\n\
               a: pat\n\nb: s(\"sd\")";
    crate::eval_document(doc, &mut blocks).expect("document");

    // Editing one block replaces its own label and leaves the others.
    let edited = doc.replace("s(\"sd\")", "s(\"cp hh\")");
    blocks.shift(
        doc.find("s(\"sd\")").unwrap(),
        doc.len(),
        "s(\"cp hh\")".len(),
    );
    let pat = block(&edited, "b:", &mut blocks).expect("block b");
    assert_eq!(sounds(&pat), ["bd", "cp", "hh"]);

    // Declarations made by another evaluation are still there, functions too.
    let grown = format!("{edited}\n\nc: twice(pat)");
    let pat = block(&grown, "c:", &mut blocks).expect("block c");
    assert_eq!(sounds(&pat), ["bd", "bd", "bd", "cp", "hh"]);

    // A block that no longer has its label drops it.
    let at = grown.find("a: pat").unwrap();
    let doc = grown.replace("a: pat", "s(\"hh\")");
    blocks.shift(at, at + "a: pat".len(), "s(\"hh\")".len());
    let pat = block(&doc, "s(\"hh\")", &mut blocks).expect("unlabelled block");
    assert_eq!(sounds(&pat), ["bd", "bd", "cp", "hh"]);

    // A full evaluation starts afresh.
    let pat = crate::eval_document("a: s(\"sd\")", &mut blocks)
        .expect("document")
        .pattern;
    assert_eq!(sounds(&pat), ["sd"]);
    let pat = block("a: s(\"sd\")\n\nb: s(\"hh\")", "b:", &mut blocks).expect("block");
    assert_eq!(sounds(&pat), ["hh", "sd"]);
}

#[test]
fn a_block_refuses_anonymous_labels() {
    // Evaluated again, they would stack on themselves.
    let err = block("x\n\n$: s(\"bd\")", "$:", &mut crate::Blocks::default())
        .err()
        .expect("refused");
    assert!(err.contains("anonymous labels disabled"), "{err}");
}

#[test]
fn a_label_used_twice_plays_the_last_one() {
    // Upstream's `pPatterns[id] = this`: the key is replaced, not stacked.
    let pat = eval("a: s(\"bd\")\nb: s(\"hh\")\na: s(\"sd\")").expect("eval");
    assert_eq!(sounds(&pat), ["hh", "sd"]);
}

#[test]
fn clear_scope_forgets_the_carried_declarations() {
    let mut blocks = crate::Blocks::default();
    let doc = "const drums = s(\"bd\")\n\na: drums";
    crate::eval_document(doc, &mut blocks).expect("document");
    let doc = format!("{doc}\n\nclearScope()");
    block(&doc, "clearScope", &mut blocks).expect("clearScope");
    let doc = format!("{doc}\n\nb: drums");
    let err = block(&doc, "b:", &mut blocks).err().expect("drums is gone");
    assert!(err.contains("drums"), "{err}");
}
