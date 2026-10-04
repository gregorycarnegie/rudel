//! Strudel's block registry (`core/repl.mjs` `evaluateBlock`): what lets one
//! block of a document be evaluated while the others keep playing.
//!
//! Upstream the REPL's JavaScript realm lives across evaluations, so the
//! patterns of earlier blocks (`pPatterns`) and their top-level declarations
//! (copied onto `globalThis` by the block-based transpiler) are simply still
//! there. Here every evaluation gets a fresh engine, so the registry is kept
//! on this side instead:
//!
//! - each labelled pattern is remembered with the part of the document it came
//!   from (its label to the next one), holding its evaluation's engine, which
//!   keeps any script function inside it callable;
//! - evaluating a block drops the remembered labels that lay inside it (they are
//!   about to be re-registered, or were deleted, as upstream's
//!   `cleanupConflictingRanges`) and seeds the rest, so the result is every
//!   block's pattern stacked, through the usual solo/`each`/`all`;
//! - top-level `const`/`let`/`var`/`function` names are carried over: values
//!   that are rudel's own (patterns, numbers, strings, lists, plain objects,
//!   hydra chains) as they are, functions as their source, rebuilt in the next
//!   engine.
//!
//! A full evaluation starts the registry afresh and records what it defined, so
//! a block evaluated after it keeps the other blocks playing. (Upstream a full
//! evaluation leaves no blocks behind, and the next block evaluation stops
//! everything but itself.)

use crate::{
    hydra::Chain,
    js::{self, Arg, Session},
    keep_alive,
};
use rudel_core::{Frac, Pattern};
use std::{cell::Cell, sync::Arc};

thread_local! {
    /// Set by `clearScope()` during an evaluation.
    static CLEARED: Cell<bool> = const { Cell::new(false) };
}

/// `clearScope()`: the declarations carried so far, and this evaluation's, are
/// forgotten once it is recorded.
pub(crate) fn clear_scope() {
    CLEARED.set(true);
}

/// Called as an evaluation starts.
pub(crate) fn reset_cleared() {
    CLEARED.set(false);
}

/// A value a declaration held, in a form that outlives its engine.
#[derive(Clone)]
enum Carried {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Pat(Pattern),
    Frac(Frac),
    Hydra(Chain),
    List(Vec<Carried>),
    Map(Vec<(String, Carried)>),
    /// A script function, by its source text.
    Function(String),
}

impl Carried {
    /// `arg` as something to carry, or `None` for what cannot outlive its
    /// engine (a native function, a kabelsalat node, an object with a function
    /// inside).
    fn from_arg(arg: &Arg) -> Option<Carried> {
        Some(match arg {
            Arg::Null => Carried::Null,
            Arg::Bool(b) => Carried::Bool(*b),
            Arg::Num(n) => Carried::Num(*n),
            Arg::Str(s) => Carried::Str(s.clone()),
            Arg::Pat(p) => Carried::Pat(p.clone()),
            Arg::Frac(f) => Carried::Frac(*f),
            Arg::Hydra(c) => Carried::Hydra(c.clone()),
            Arg::List(items) => {
                Carried::List(items.iter().map(Carried::from_arg).collect::<Option<_>>()?)
            }
            Arg::Map(entries) => Carried::Map(
                entries
                    .iter()
                    .map(|(k, v)| Some((k.clone(), Carried::from_arg(v)?)))
                    .collect::<Option<_>>()?,
            ),
            _ => return None,
        })
    }

    /// Patterns inside, holding the engine they were made in.
    fn holding(self, hold: &impl Fn(Pattern) -> Pattern) -> Carried {
        match self {
            Carried::Pat(p) => Carried::Pat(hold(p)),
            Carried::List(items) => {
                Carried::List(items.into_iter().map(|c| c.holding(hold)).collect())
            }
            Carried::Map(entries) => Carried::Map(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, v.holding(hold)))
                    .collect(),
            ),
            other => other,
        }
    }

    fn to_arg(&self) -> Arg {
        match self {
            Carried::Null | Carried::Function(_) => Arg::Null,
            Carried::Bool(b) => Arg::Bool(*b),
            Carried::Num(n) => Arg::Num(*n),
            Carried::Str(s) => Arg::Str(s.clone()),
            Carried::Pat(p) => Arg::Pat(p.clone()),
            Carried::Frac(f) => Arg::Frac(*f),
            Carried::Hydra(c) => Arg::Hydra(c.clone()),
            Carried::List(items) => Arg::List(items.iter().map(Carried::to_arg).collect()),
            Carried::Map(entries) => Arg::Map(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_arg()))
                    .collect(),
            ),
        }
    }
}

/// A remembered labelled pattern.
struct Slot {
    key: String,
    /// The part of the document it came from, in document bytes.
    range: (usize, usize),
    /// Holding the engine its script functions live in.
    pattern: Pattern,
}

/// The blocks of one document: what its last full evaluation and the block
/// evaluations since left playing, and the declarations they made. The editor
/// keeps one and hands it to every evaluation of its document.
#[derive(Default)]
pub struct Blocks {
    slots: Vec<Slot>,
    scope: Vec<(String, Carried)>,
}

impl Blocks {
    /// Follow an edit of the document: `from..to` was replaced with `inserted`
    /// bytes. A remembered range keeps covering its own text.
    pub fn shift(&mut self, from: usize, to: usize, inserted: usize) {
        let map = |pos: usize, end: bool| {
            if pos >= to && pos > from {
                pos - (to - from) + inserted
            } else if pos > from {
                // Inside the replaced text: an end stays after the insertion, a
                // start before it.
                if end { from + inserted } else { from }
            } else {
                pos
            }
        };
        for slot in &mut self.slots {
            slot.range = (map(slot.range.0, false), map(slot.range.1, true));
        }
    }

    /// What a block evaluation over `range` starts from: the remembered patterns
    /// of the other blocks. The ones inside `range` are left out, since the block
    /// is about to define them again (or no longer does).
    pub(crate) fn others(&self, range: (usize, usize)) -> Vec<(String, Pattern)> {
        self.slots
            .iter()
            .filter(|s| s.range.1 <= range.0 || s.range.0 >= range.1)
            .map(|s| (s.key.clone(), s.pattern.clone()))
            .collect()
    }
}

/// Upstream refuses `$:` in a block: evaluated again, an anonymous pattern
/// would stack on itself rather than replace itself.
pub(crate) fn check_labels(labels: &[crate::preprocess::LabelLine]) -> Result<(), String> {
    if labels.iter().any(|(name, _)| name.starts_with('$')) {
        return Err("anonymous labels disabled for block based evaluation (see \
                    https://strudel.cc/blog/#label-notation)"
            .to_string());
    }
    Ok(())
}

impl Blocks {
    /// Define the declarations carried from earlier evaluations in a fresh engine,
    /// before a block runs in it. A name the block declares again shadows them.
    pub(crate) fn define_carried(&self, ctx: &mut js::Context) {
        for (name, value) in &self.scope {
            match value {
                Carried::Function(source) => {
                    // A function that will not rebuild (it closed over something
                    // gone) is left out rather than failing the block.
                    let _ = js::run(ctx, &format!("globalThis[{name:?}] = ({source});"));
                }
                other => js::lend(ctx, || js::Scope::global().value(name, other.to_arg())),
            }
        }
    }
}

/// The names `source` declares at its top level, rudel's own label variables
/// aside.
fn declared_names(source: &str) -> Vec<String> {
    use boa_engine::{
        ast::{
            operations::{lexically_declared_names, var_declared_names},
            scope::Scope,
        },
        interner::Interner,
        parser::{Parser, Source},
    };
    let mut interner = Interner::default();
    let Ok(script) =
        Parser::new(Source::from_bytes(source)).parse_script(&Scope::new_global(), &mut interner)
    else {
        return Vec::new();
    };
    let statements = script.statements();
    let mut names: Vec<String> = lexically_declared_names(statements)
        .into_iter()
        .chain(var_declared_names(statements))
        .map(|sym| interner.resolve_expect(sym).to_string())
        .filter(|name| !name.starts_with("rudel_label_"))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// What one evaluation declared, read out of its engine before it is parked.
pub(crate) struct Declared(Vec<(String, Carried)>);

/// Read the values of `source`'s top-level declarations from `ctx`.
pub(crate) fn read_declared(ctx: &mut js::Context, source: &str) -> Declared {
    let mut out = Vec::new();
    for name in declared_names(source) {
        let Ok(value) = js::run(ctx, &name) else {
            continue;
        };
        let carried = if matches!(value, Arg::Func(_)) {
            match js::run(ctx, &format!("String({name})")) {
                Ok(Arg::Str(source)) if !source.contains("[native code]") => {
                    Some(Carried::Function(source))
                }
                _ => None,
            }
        } else {
            Carried::from_arg(&value)
        };
        if let Some(carried) = carried {
            out.push((name, carried));
        }
    }
    Declared(out)
}

/// Where in the document each registered slot came from: a label's range runs
/// to the next label or the end of its paragraph, whichever is first (blank
/// lines are what separate blocks); a slot no label made (`.p("x")`, `d1`)
/// covers the whole evaluated text.
fn ranges(
    keys: &[String],
    labels: &[crate::preprocess::LabelLine],
    text: &str,
    base: usize,
) -> Vec<(usize, usize)> {
    // Each line's start and end, without its newline.
    let mut lines = Vec::new();
    let mut start = 0;
    for line in text.split('\n') {
        lines.push((start, start + line.len(), line.trim().is_empty()));
        start += line.len() + 1;
    }
    let label_range = |i: usize| {
        let first = labels[i].1.min(lines.len() - 1);
        let stop = labels.get(i + 1).map_or(lines.len(), |next| next.1);
        let last = (first..stop.max(first + 1))
            .take_while(|&l| l == first || lines.get(l).is_some_and(|line| !line.2))
            .last()
            .unwrap_or(first);
        (
            base + lines[first].0,
            base + lines[last.min(lines.len() - 1)].1,
        )
    };
    let whole = (base, base + text.len());
    let mut anonymous = labels
        .iter()
        .enumerate()
        .filter(|(_, (name, _))| name.contains('$'));
    keys.iter()
        .map(|key| {
            if key.contains('$') {
                return anonymous.next().map_or(whole, |(i, _)| label_range(i));
            }
            labels
                .iter()
                .rposition(|(name, _)| name == key)
                .map_or(whole, label_range)
        })
        .collect()
}

impl Blocks {
    /// Remember what an evaluation of `text` (at document offset `base`) left
    /// registered. `slots` is the final registry in order; `fresh` the keys this
    /// evaluation registered (the rest were seeded from earlier blocks and keep
    /// their ranges). What this evaluation made holds `session`, its engine. A
    /// block's declarations join those carried before; a full evaluation's replace
    /// them.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record(
        &mut self,
        slots: Vec<(String, Pattern)>,
        fresh: &[String],
        labels: &[crate::preprocess::LabelLine],
        text: &str,
        base: usize,
        declared: Declared,
        session: Option<Arc<Session>>,
        block: bool,
    ) {
        let hold = |pattern: Pattern| match &session {
            Some(session) => keep_alive(pattern, session.clone()),
            None => pattern,
        };
        let fresh_keys: Vec<String> = slots
            .iter()
            .map(|(key, _)| key.clone())
            .filter(|key| fresh.contains(key))
            .collect();
        let mut fresh_ranges = ranges(&fresh_keys, labels, text, base).into_iter();
        let old = std::mem::take(&mut self.slots);
        self.slots = slots
            .into_iter()
            .filter_map(|(key, pattern)| {
                let (range, pattern) = if fresh.contains(&key) {
                    (fresh_ranges.next()?, hold(pattern))
                } else {
                    // Seeded, so already holding its own engine.
                    (old.iter().find(|s| s.key == key)?.range, pattern)
                };
                Some(Slot {
                    key,
                    range,
                    pattern,
                })
            })
            .collect();
        let cleared = CLEARED.take();
        if !block || cleared {
            self.scope.clear();
        }
        if cleared {
            return;
        }
        for (name, value) in declared.0 {
            let value = value.holding(&hold);
            self.scope.retain(|(n, _)| *n != name);
            self.scope.push((name, value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_declarations_are_found_and_label_variables_are_not() {
        let names = declared_names(
            "const a = 1, {b, c} = {}\nlet d\nvar e\nfunction f() { var inner }\n\
             const rudel_label_0_x = 1\n{ let nested = 1 }",
        );
        assert_eq!(names, ["a", "b", "c", "d", "e", "f"]);
    }

    #[test]
    fn a_label_s_range_runs_to_the_next_label_or_the_end_of_its_paragraph() {
        let text =
            "setcps(1)\na: s(\"bd\")\n  .fast(2)\n\n// b next\nb: s(\"sd\") ; c: s(\"hh\")\n";
        let labels = vec![
            ("a".to_string(), 1),
            ("b".to_string(), 5),
            ("c".to_string(), 5),
        ];
        let keys = ["a", "b", "c", "x"].map(String::from);
        let [a, b, c, x] = ranges(&keys, &labels, text, 100)[..] else {
            panic!()
        };
        assert_eq!(&text[a.0 - 100..a.1 - 100], "a: s(\"bd\")\n  .fast(2)");
        assert_eq!(&text[b.0 - 100..b.1 - 100], "b: s(\"sd\") ; c: s(\"hh\")");
        assert_eq!(c, b);
        assert_eq!(x, (100, 100 + text.len()));
    }
}
