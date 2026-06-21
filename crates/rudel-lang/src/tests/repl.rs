use super::common::*;

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
    // `note("c").d1()` registers slot "1"; the result is that pattern, tagged
    // with its id.
    let pat = eval(r#"note("c").d1()"#).expect("eval");
    let vals = values(&pat, 0, 1);
    assert_eq!(vals.len(), 1);
    assert_eq!(id_of(&vals[0]).as_deref(), Some("1"));
}

#[test]
fn multiple_slots_stack() {
    // Two slots across two statements stack into one pattern, even though Koto
    // only returns the last expression.
    let pat = eval("note(\"c\").d1()\nnote(\"e\").d2()").expect("eval");
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
    let pat = eval(r#"note("c").p1()"#).expect("eval");
    assert_eq!(id_of(&values(&pat, 0, 1)[0]).as_deref(), Some("1"));
}

#[test]
fn q_slot_is_silent() {
    // q/q1 mute their pattern (a queued slot): no events, nothing registered.
    let pat = eval(r#"note("c").q1()"#).expect("eval");
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
    let pat = eval("note(\"c\").d1()\nhush()").expect("eval");
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
    let _ = eval(r#"note("c").d1()"#).expect("eval");
    let pat = eval(r#"note("e")"#).expect("eval");
    let vals = values(&pat, 0, 1);
    assert_eq!(vals.len(), 1);
    assert_eq!(id_of(&vals[0]), None, "no slot id should carry over");
}

#[test]
fn all_transforms_the_stacked_patterns() {
    // `all(f)` applies `f` to the whole stack: two one-event slots stacked and
    // fast(2)'d yield four events per cycle.
    let src = "note(\"c\").d1()\nnote(\"e\").d2()\nall(|x| x.fast(2))";
    let pat = eval(src).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
}

#[test]
fn all_on_labels_transforms_the_stack() {
    // `$:` labels are picked up by `all`, just like slots.
    let src = "$: note(\"c\")\n$: note(\"e\")\nall(|x| x.fast(2))";
    let pat = eval(src).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
}

#[test]
fn each_transforms_every_pattern_separately() {
    // `each(f)` applies `f` to each registered pattern before stacking: two
    // slots, each fast(2)'d, give four events.
    let src = "note(\"c\").d1()\nnote(\"e\").d2()\neach(|x| x.fast(2))";
    let pat = eval(src).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
}

#[test]
fn each_without_slots_transforms_the_script_pattern() {
    // With no registered slots, `each` applies to the script's own pattern.
    let src = "each(|x| x.fast(2))\nnote(\"c\")";
    let pat = eval(src).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 2);
}

#[test]
fn solo_slot_silences_the_others() {
    // An `S`-prefixed key solos: only that pattern plays. `p("S1")` solos over a
    // plain `d2` slot.
    let src = "note(\"c\").d2()\nnote(\"e\").p(\"S1\")";
    let pat = eval(src).expect("eval");
    let ids: Vec<String> = values(&pat, 0, 1).iter().filter_map(id_of).collect();
    assert_eq!(ids, vec!["S1".to_string()], "only the soloed slot plays");
}

#[test]
fn solo_keeps_all_soloed_patterns() {
    // Multiple soloed slots all play; non-soloed ones drop out.
    let src = "note(\"c\").d1()\nnote(\"e\").p(\"S2\")\nnote(\"g\").p(\"S3\")";
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
    let _ = eval("note(\"c\").d1()\nall(|x| x.fast(4))").expect("eval");
    let pat = eval(r#"note("e")"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 1, "all() must not carry over");
}
