//! Mondo Notation — Strudel's Lisp-like alternative pattern language
//! (`@strudel/mondo` + `@strudel/mondough`), compiled to Koto source.
//!
//! Mondo is a *source language*, not new musical capability: every form in it
//! maps onto a function Rudel already exposes. So rather than build a second
//! evaluator against `rudel-core`, this pass does what upstream does — parse,
//! desugar, and emit — with Koto as the target instead of JavaScript. That is
//! also why it lives in the preprocessor: `mondo`s hh*8`` is rewritten into the
//! Koto call it stands for before the script is compiled, so every control,
//! transform and signal in the prelude is reachable from mondo the day it is
//! added, with no dispatch table to keep in sync.
//!
//! The parser is a faithful port of `mondo.mjs` (tokeniser, precedence-climbing
//! desugar, pipes, lambdas); the code generator plays the role of
//! `mondough.mjs`'s evaluator, mapping each desugared head onto its Rudel
//! spelling. Upstream's `mondo.test.mjs` parser/sugar cases are ported below as
//! the oracle for the first half.
//!
//! SPDX-License-Identifier: AGPL-3.0-or-later

use super::scanner::{Chunk, chunks, is_ident_char};
use koto::prelude::KMap;
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

// ---------------------------------------------------------------------------
// Tokenizer

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Comment,
    Str,
    OpenList,
    CloseList,
    OpenAngle,
    CloseAngle,
    OpenSquare,
    CloseSquare,
    OpenCurly,
    CloseCurly,
    Number,
    Op,
    Pipe,
    Stack,
    Or,
    Plain,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    value: String,
}

/// `-?[0-9]*\.?[0-9]+`, matched by hand so the crate stays regex-free. Returns
/// the matched length. Deliberately backtracks off a trailing `.` so `0..2`
/// tokenises as `0`, `..`, `2` rather than swallowing the range operator.
fn match_number(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    let int_start = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let int_len = i - int_start;
    if b.get(i) == Some(&b'.') {
        let dot = i;
        i += 1;
        let frac_start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i > frac_start {
            return Some(i);
        }
        i = dot;
    }
    (int_len > 0).then_some(i)
}

fn is_plain_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '~' | '_' | '^' | '#')
}

/// The single next token, as `(kind, byte length)`. Order matches upstream's
/// `token_types`, which is load-bearing: `number` before `op` so `-1` is a
/// number, `pipe` before `plain` so a leading `#` chains.
fn next_token(code: &str) -> Result<(Kind, usize), String> {
    let first = code.chars().next().ok_or("mondo: empty token")?;
    if code.starts_with("//") {
        let end = code.find('\n').unwrap_or(code.len());
        return Ok((Kind::Comment, end));
    }
    if first == '"' || first == '\'' {
        let rest = &code[first.len_utf8()..];
        let end = rest.find(first).ok_or_else(|| {
            format!(
                "unterminated string: {}",
                code.lines().next().unwrap_or_default()
            )
        })?;
        return Ok((Kind::Str, first.len_utf8() * 2 + end));
    }
    let bracket = match first {
        '(' => Some(Kind::OpenList),
        ')' => Some(Kind::CloseList),
        '<' => Some(Kind::OpenAngle),
        '>' => Some(Kind::CloseAngle),
        '[' => Some(Kind::OpenSquare),
        ']' => Some(Kind::CloseSquare),
        '{' => Some(Kind::OpenCurly),
        '}' => Some(Kind::CloseCurly),
        _ => None,
    };
    if let Some(kind) = bracket {
        return Ok((kind, 1));
    }
    if let Some(len) = match_number(code) {
        return Ok((Kind::Number, len));
    }
    if "*/:!@%?+-&".contains(first) {
        return Ok((Kind::Op, 1));
    }
    if code.starts_with("..") {
        return Ok((Kind::Op, 2));
    }
    match first {
        '#' => return Ok((Kind::Pipe, 1)),
        ',' | '$' => return Ok((Kind::Stack, 1)),
        '|' => return Ok((Kind::Or, 1)),
        _ => {}
    }
    let len = code.find(|c| !is_plain_char(c)).unwrap_or(code.len());
    if len == 0 {
        // Just where it stopped, not everything after it: `code` is the whole
        // rest of the script, which is no help in an error message.
        let stuck = code.lines().next().unwrap_or_default();
        return Err(format!("could not match '{stuck}'"));
    }
    Ok((Kind::Plain, len))
}

fn tokenize(code: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut rest = code.trim_start();
    while !rest.is_empty() {
        let (kind, len) = next_token(rest)?;
        tokens.push(Token {
            kind,
            value: rest[..len].to_string(),
        });
        rest = rest[len..].trim_start();
    }
    Ok(tokens)
}

// ---------------------------------------------------------------------------
// Parser

#[derive(Clone, Debug, PartialEq)]
enum Node {
    List(Vec<Node>),
    Leaf { kind: Kind, value: String },
}

fn plain(value: &str) -> Node {
    Node::Leaf {
        kind: Kind::Plain,
        value: value.to_string(),
    }
}

impl Node {
    fn leaf_kind(&self) -> Option<Kind> {
        match self {
            Node::Leaf { kind, .. } => Some(*kind),
            Node::List(_) => None,
        }
    }
}

/// The name at the head of a list, i.e. the function it calls.
fn head_name(children: &[Node]) -> Option<&str> {
    match children.first() {
        Some(Node::Leaf {
            kind: Kind::Plain,
            value,
        }) => Some(value),
        _ => None,
    }
}

/// Operators, in the order they bind. Everything in a group is applied
/// left-to-right before the next group is looked at (upstream `op_precedence`).
const OP_PRECEDENCE: [&[&str]; 2] = [&["*", "/", ":", "!", "@", "%", "?", "+", "-", ".."], &["&"]];

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<Kind> {
        self.tokens.get(self.pos).map(|t| t.kind)
    }

    fn parse_expr(&mut self) -> Result<Node, String> {
        let kind = self.peek().ok_or("unexpected end of file")?;
        match kind {
            Kind::OpenList => self.parse_wrapped(Kind::CloseList, None),
            Kind::OpenAngle => self.parse_wrapped(Kind::CloseAngle, Some("angle")),
            Kind::OpenSquare => self.parse_wrapped(Kind::CloseSquare, Some("square")),
            Kind::OpenCurly => self.parse_wrapped(Kind::CloseCurly, Some("curly")),
            _ => {
                let token = &self.tokens[self.pos];
                self.pos += 1;
                Ok(Node::Leaf {
                    kind: token.kind,
                    value: token.value.clone(),
                })
            }
        }
    }

    fn parse_wrapped(&mut self, close: Kind, ty: Option<&str>) -> Result<Node, String> {
        self.pos += 1; // opening bracket
        let mut children = Vec::new();
        loop {
            match self.peek() {
                None => return Err("unexpected end of file: missing closing bracket".into()),
                Some(kind) if kind == close => {
                    self.pos += 1;
                    break;
                }
                _ => children.push(self.parse_expr()?),
            }
        }
        if let Some(ty) = ty {
            children.insert(0, plain(ty));
        }
        Ok(Node::List(desugar(children, ty)?))
    }
}

fn parse(code: &str) -> Result<Node, String> {
    let mut parser = Parser {
        tokens: tokenize(code)?,
        pos: 0,
    };
    let mut expressions = Vec::new();
    while parser.pos < parser.tokens.len() {
        expressions.push(parser.parse_expr()?);
    }
    match expressions.len() {
        0 => Ok(Node::List(Vec::new())),
        // A single bracketed expression is already the whole program; anything
        // else is an implicit top-level list.
        1 if matches!(expressions[0], Node::List(_)) => Ok(expressions.pop().unwrap()),
        _ => Ok(Node::List(desugar(expressions, None)?)),
    }
}

// ---------------------------------------------------------------------------
// Desugaring: infix operators, `#` pipes, `,`/`$` stacks and `|` choices all
// become ordinary prefix calls.

fn split_children(children: &[Node], split: Kind) -> Vec<Vec<Node>> {
    let mut chunks = vec![Vec::new()];
    for child in children {
        if child.leaf_kind() == Some(split) {
            chunks.push(Vec::new());
        } else {
            chunks.last_mut().expect("one chunk").push(child.clone());
        }
    }
    chunks
}

type Next<'a> = &'a dyn Fn(Vec<Node>) -> Result<Vec<Node>, String>;

fn desugar_split(
    children: Vec<Node>,
    split: Kind,
    name: &str,
    next: Next,
) -> Result<Vec<Node>, String> {
    let chunks = split_children(&children, split);
    if chunks.len() == 1 {
        return next(children);
    }
    let mut args = vec![plain(name)];
    for chunk in chunks {
        // Empty chunks are dropped, which is what makes a leading `$` work.
        match chunk.len() {
            0 => continue,
            1 => args.push(chunk.into_iter().next().expect("one child")),
            _ => args.push(Node::List(next(chunk)?)),
        }
    }
    Ok(args)
}

/// `((x y))` is `(x y)` — undo the extra nesting an operator rewrite leaves
/// when it consumed every sibling.
fn unwrap_children(children: Vec<Node>) -> Vec<Node> {
    match children.first() {
        Some(Node::List(inner)) if children.len() == 1 => inner.clone(),
        _ => children,
    }
}

/// Infix to prefix: `a * 2` becomes `(* 2 a)`, so operators are just functions
/// whose pattern argument comes last — the same shape as every other call.
fn desugar_ops(mut children: Vec<Node>, types: &[&str]) -> Result<Vec<Node>, String> {
    loop {
        let Some(i) = children.iter().position(|child| match child {
            Node::Leaf {
                kind: Kind::Op,
                value,
            } => types.contains(&value.as_str()),
            _ => false,
        }) else {
            return Ok(children);
        };
        let Node::Leaf { value, .. } = children[i].clone() else {
            unreachable!("position matched a leaf")
        };
        let op = plain(&value);
        // An operator with nothing to its left (or right) is a plain function
        // reference, e.g. the `# *2` chain and the trailing `-` of `[c -]`.
        let piped = i > 0 && children[i - 1].leaf_kind() == Some(Kind::Pipe);
        if i == 0 || i == children.len() - 1 || piped {
            children[i] = op;
            continue;
        }
        if children[i - 1].leaf_kind() == Some(Kind::Op) {
            let Node::Leaf { value: left, .. } = &children[i - 1] else {
                unreachable!()
            };
            return Err(format!("got 2 ops in a row: \"{left}{value}\""));
        }
        if children[i + 1].leaf_kind() == Some(Kind::Op) {
            let Node::Leaf { value: right, .. } = &children[i + 1] else {
                unreachable!()
            };
            let mut err = format!("got 2 ops in a row: \"{value}{right}\"");
            if value == "-" {
                err.push_str(". you probably want a rest, which is \"_\" in mondo!");
            }
            return Err(err);
        }
        let call = Node::List(vec![op, children[i + 1].clone(), children[i - 1].clone()]);
        children.splice(i - 1..=i + 1, [call]);
        children = unwrap_children(children);
    }
}

/// `s jazz # fast 2` becomes `(fast 2 (s jazz))`: each `#` makes what came
/// before it the last argument of what comes after. A leading `#` has no left
/// side, so it becomes a lambda instead — that is mondo's `x => x.` shorthand.
fn desugar_pipes(children: Vec<Node>) -> Result<Vec<Node>, String> {
    let mut chunks = split_children(&children, Kind::Pipe);
    while chunks.len() > 1 {
        if chunks[0].is_empty() {
            let arg = plain("_");
            let mut body = vec![arg.clone()];
            body.extend(children);
            return get_lambda(vec![arg], body);
        }
        let left = chunks.remove(0);
        let mut right = chunks.remove(0);
        right.push(match left.len() {
            1 => left.into_iter().next().expect("one child"),
            _ => Node::List(left),
        });
        chunks.insert(0, right);
    }
    Ok(chunks.pop().unwrap_or_default())
}

fn get_lambda(args: Vec<Node>, children: Vec<Node>) -> Result<Vec<Node>, String> {
    let mut children = desugar(children, None)?;
    let body = match children.len() {
        1 => children.pop().expect("one child"),
        _ => Node::List(children),
    };
    Ok(vec![plain("fn"), Node::List(args), body])
}

/// `ty` is the bracket kind whose name was already unshifted onto `children`;
/// it is stripped before splitting (so `[a, b]` stacks rather than sequencing)
/// and put back for each chunk that keeps sequencing.
fn desugar(children: Vec<Node>, ty: Option<&str>) -> Result<Vec<Node>, String> {
    let children = match ty {
        Some(_) => children[1..].to_vec(),
        None => children,
    };
    desugar_split(children, Kind::Stack, "stack", &|children| {
        desugar_split(children, Kind::Or, "or", &|children| {
            let mut children = match ty {
                Some(ty) => [vec![plain(ty)], children].concat(),
                None => children,
            };
            for ops in OP_PRECEDENCE {
                children = desugar_ops(children, ops)?;
            }
            desugar_pipes(children)
        })
    })
}

/// The desugared tree in upstream's compact `printAst` form. Only used by the
/// tests, where it is what the ported upstream cases assert on.
#[cfg(test)]
fn print_ast(node: &Node) -> String {
    match node {
        Node::List(children) => format!(
            "({})",
            children.iter().map(print_ast).collect::<Vec<_>>().join(" ")
        ),
        Node::Leaf { value, .. } => value.clone(),
    }
}

// ---------------------------------------------------------------------------
// Code generation

/// Infix operators that are just a Rudel function under a punctuation name.
/// Each is called `(op arg subject)`, which is already the pattern-last shape
/// the prelude's standalone functions take.
const OP_FNS: &[(&str, &str)] = &[
    ("*", "fast"),
    ("/", "slow"),
    // `extend` (repeat and take the extra steps), not `replicate`: only the
    // former carries the step count that makes `[bd hh!3]` four steps, and it
    // is what the Mondo Notation page documents `!` as.
    ("!", "extend"),
    ("@", "expand"),
    ("%", "pace"),
    ("?", "degradeBy"),
    ("+", "late"),
    ("-", "early"),
];

/// Every top-level name the Koto runtime exposes. A bareword that names one is
/// emitted as that value (`jux rev`), anything else is a string (`s bd`) —
/// upstream resolves leaves the same way, and collides the same way, which is
/// why its own docs write `s "sine"` to mean the sample rather than the signal.
fn prelude_names() -> &'static HashSet<String> {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let prelude = KMap::default();
        crate::bindings::register(&prelude);
        crate::bindings::function_names(&prelude)
            .into_iter()
            .collect()
    })
}

/// Controls are the one prelude family that is *not* pattern-last: `s('bd')`
/// builds a control pattern from its only argument, so mondo's `# bank tr909`
/// — which passes the pattern as well — has to reach the method instead.
///
/// The method is also the only form that promotes a value already wrapped in a
/// control map, which is what `[bd (cp # delay .6)]` produces: as a factory
/// call the `cp` would stay an unnamed `value` instead of becoming `s`.
fn control_names() -> &'static HashSet<String> {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        rudel_core::control_builders()
            .map(|(name, _)| name.to_string())
            .chain(
                rudel_core::numbered_control_names()
                    .into_iter()
                    .map(|(name, _)| name),
            )
            .collect()
    })
}

/// Escape a string for a Koto literal. `$` matters: it opens interpolation in
/// Koto, and is mondo's pattern separator. A newline matters because a compile
/// error carries a snippet of the user's source, and a literal that runs onto a
/// second line stops being a literal.
fn escape(text: &str, quote: char) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => {
                if c == '\\' || c == '$' || c == quote {
                    out.push('\\');
                }
                out.push(c);
            }
        }
    }
    out
}

fn koto_string(text: &str) -> String {
    format!("'{}'", escape(text, '\''))
}

/// A double-quoted literal, which the mini pass turns into `m("...")` — the
/// only spelling that gets mini-notation parsing rather than a plain string.
fn mini_string(text: &str) -> String {
    format!("\"{}\"", escape(text, '"'))
}

fn koto_number(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix('.') {
        format!("0.{rest}")
    } else if let Some(rest) = raw.strip_prefix("-.") {
        format!("-0.{rest}")
    } else {
        raw.to_string()
    }
}

struct Gen {
    /// `def`ined names, mapped to the Koto expression they stand for. Mondo
    /// values are pure, so a reference is compiled by substitution rather than
    /// by emitting a statement — that keeps the whole program one expression,
    /// which is what a `mondo`...`` call has to be.
    // ponytail: substitution, so a name used n times compiles n times. Hoist to
    // Koto assignments if a tune ever makes that cost visible.
    defs: HashMap<String, String>,
    /// Lambda parameters in scope, innermost last.
    scope: Vec<(String, String)>,
}

impl Gen {
    fn new() -> Self {
        Gen {
            defs: HashMap::new(),
            scope: Vec::new(),
        }
    }

    /// `receiver` marks a position that has to be a pattern to be usable — the
    /// left of a `.method()` call — where a bare literal needs `pure`.
    fn emit(&mut self, node: &Node, receiver: bool) -> Result<String, String> {
        match node {
            Node::List(children) => self.emit_list(children),
            Node::Leaf { kind, value } => {
                let literal = match kind {
                    Kind::Number => koto_number(value),
                    Kind::Str => koto_string(&value[1..value.len().saturating_sub(1)]),
                    _ => return self.emit_word(value, receiver),
                };
                Ok(match receiver {
                    true => format!("pure({literal})"),
                    false => literal,
                })
            }
        }
    }

    fn emit_word(&mut self, word: &str, receiver: bool) -> Result<String, String> {
        if let Some((_, id)) = self.scope.iter().rev().find(|(name, _)| name == word) {
            return Ok(id.clone());
        }
        if let Some(body) = self.defs.get(word) {
            return Ok(body.clone());
        }
        // `_` and `~` are mondo's rests; a trailing operator survives desugaring
        // as a bare word and means the same thing (upstream's `[c -]` case).
        if matches!(word, "_" | "~" | "-") {
            return Ok("silence()".to_string());
        }
        if prelude_names().contains(word) {
            return Ok(word.to_string());
        }
        Ok(match receiver {
            true => format!("pure({})", koto_string(word)),
            false => koto_string(word),
        })
    }

    fn emit_args(&mut self, args: &[Node]) -> Result<Vec<String>, String> {
        args.iter().map(|arg| self.emit(arg, false)).collect()
    }

    fn emit_list(&mut self, children: &[Node]) -> Result<String, String> {
        let children: Vec<Node> = children
            .iter()
            .filter(|child| child.leaf_kind() != Some(Kind::Comment))
            .cloned()
            .collect();
        let Some(head) = head_name(&children).map(str::to_string) else {
            // An empty program, or a call whose head is itself computed — the
            // latter needs a pattern of functions, which only an evaluator can
            // do.
            return match children.is_empty() {
                true => Ok("silence()".to_string()),
                false => Err("expected a function name at the head of a list".into()),
            };
        };
        let args = &children[1..];
        let joined = |parts: Vec<String>| parts.join(", ");
        match head.as_str() {
            // Bracket kinds. `stepcat` weights each child by its own step
            // count, which is what makes `@`/`!` work; `[]` then reports one
            // step so it nests as a single element, and `<>` paces to one step
            // per cycle.
            "square" => Ok(format!(
                "stepcat({}).setSteps(1)",
                joined(self.emit_args(args)?)
            )),
            "angle" => Ok(format!(
                "stepcat({}).pace(1)",
                joined(self.emit_args(args)?)
            )),
            "curly" => Ok(format!("stepcat({})", joined(self.emit_args(args)?))),
            "stack" => Ok(format!("stack({})", joined(self.emit_args(args)?))),
            "or" => Ok(format!("chooseIn({})", joined(self.emit_args(args)?))),
            "fn" => self.emit_lambda(args),
            "def" => self.emit_def(args),
            ":" => Ok(mini_string(&colon_chain(&children)?.join(":"))),
            ".." => {
                let (from, to) = (literal_of(&args[1])?, literal_of(&args[0])?);
                Ok(mini_string(&format!("{from} .. {to}")))
            }
            // `bjork` takes the euclid arguments as a list, so the `:` chain
            // that carries them is flattened rather than compiled.
            "&" => {
                let euclid = colon_parts(&args[0])?.join(", ");
                let subject = self.emit(&args[1], false)?;
                Ok(format!("bjork([{euclid}], {subject})"))
            }
            _ => {
                if let Some((_, name)) = OP_FNS.iter().find(|(op, _)| *op == head) {
                    let parts = self.emit_args(args)?;
                    return Ok(format!("{name}({})", joined(parts)));
                }
                // A name the prelude exports is a standalone pattern-last
                // function and takes the arguments as they are; everything else
                // is a method on the pattern, which mondo always passes last.
                if prelude_names().contains(&head) && !control_names().contains(&head) {
                    return Ok(format!("{head}({})", joined(self.emit_args(args)?)));
                }
                let Some((subject, rest)) = args.split_last() else {
                    return Err(format!("unknown function \"{head}\""));
                };
                let subject = self.emit(subject, true)?;
                Ok(format!(
                    "{subject}.{head}({})",
                    joined(self.emit_args(rest)?)
                ))
            }
        }
    }

    fn emit_lambda(&mut self, args: &[Node]) -> Result<String, String> {
        let [Node::List(params), body] = args else {
            return Err("expected (fn (args) body)".into());
        };
        let depth = self.scope.len();
        let mut ids = Vec::new();
        for (i, param) in params.iter().enumerate() {
            let Node::Leaf { value, .. } = param else {
                return Err("expected a name as a function argument".into());
            };
            // `_` is the name the `#` lambda shorthand generates, and is not a
            // usable Koto identifier.
            let id = match value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !value.starts_with(|c: char| c.is_ascii_digit())
                && value != "_"
            {
                true => value.clone(),
                false => format!("mondoArg{}", depth + i),
            };
            self.scope.push((value.clone(), id.clone()));
            ids.push(id);
        }
        let body = self.emit(body, false)?;
        self.scope.truncate(depth);
        Ok(format!("(|{}| {body})", ids.join(", ")))
    }

    fn emit_def(&mut self, args: &[Node]) -> Result<String, String> {
        let [Node::Leaf { value: name, .. }, body] = args else {
            return Err("expected (def name value); function defs are not supported".into());
        };
        let body = self.emit(body, false)?;
        self.defs.insert(name.clone(), format!("({body})"));
        Ok("silence()".to_string())
    }
}

/// The literal text of a leaf, for the operators compiled through
/// mini-notation rather than through a function.
fn literal_of(node: &Node) -> Result<String, String> {
    match node {
        Node::Leaf { kind, value } => Ok(match kind {
            Kind::Str => value[1..value.len().saturating_sub(1)].to_string(),
            _ => value.clone(),
        }),
        Node::List(_) => Err("expected a literal".into()),
    }
}

/// Flatten a `:` chain (`bd:3`, `C4:minor`, `x:y:z`) into its parts, left to
/// right. `:` builds a list value, which is exactly what mini-notation's own
/// `:` does — so the chain is compiled to a mini string instead of needing a
/// binding of its own.
// ponytail: literal operands only. `bd:<0 1>` needs a real `tail`, which no
// tune has asked for; `euclid`/`# euclid` cover the patterned case today.
fn colon_parts(node: &Node) -> Result<Vec<String>, String> {
    match node {
        Node::List(children) => colon_chain(children),
        leaf => Ok(vec![literal_of(leaf)?]),
    }
}

fn colon_chain(children: &[Node]) -> Result<Vec<String>, String> {
    if head_name(children) != Some(":") || children.len() != 3 {
        return Err("\":\" needs literal operands".into());
    }
    let mut parts = colon_parts(&children[2])?;
    parts.extend(colon_parts(&children[1])?);
    Ok(parts)
}

/// Compile mondo source to the Koto expression it stands for.
pub(super) fn compile(code: &str) -> Result<String, String> {
    Gen::new().emit(&parse(code)?, false)
}

// ---------------------------------------------------------------------------
// Preprocessor entry point

/// Rewrite ``mondo`...` `` (and `mondi`/`mondolang`, and the plain call form)
/// into Koto.
///
/// This runs before every other pass, so what follows sees ordinary Koto and
/// mondo needs no special case anywhere else. The cost is that it shifts the
/// byte offsets of anything after it, so a script that mixes mondo with normal
/// patterns gets mini-notation highlight ranges that are off by the length of
/// the rewrite.
// ponytail: no location mapping. Thread offsets through the emitter if
// highlighting inside mondo code is ever wanted.
pub(super) fn rewrite_mondo_templates(src: &str) -> String {
    if !src.contains("mond") {
        return src.to_string();
    }
    // A whole document of Mondo Notation, which is how upstream's own examples
    // are written — they are typed straight into a REPL put in mondo mode, with
    // no tag around them. The marker line is what stands in for that mode here.
    if let Some(body) = mondo_document(src) {
        return match compile(body) {
            Ok(koto) => koto,
            Err(err) => format!("throw {}", koto_string(&format!("mondo: {err}"))),
        };
    }
    let mut out = String::with_capacity(src.len());
    let mut last = 0;
    for (kind, start, end) in chunks(src) {
        if kind != Chunk::Str || start < last {
            continue;
        }
        let Some((name, name_start, is_call)) = tag_before(src, start) else {
            continue;
        };
        // The call form has to be closed before it can be swallowed.
        let mut call_end = end;
        if is_call {
            let after = src[end..].trim_start();
            if !after.starts_with(')') {
                continue;
            }
            call_end = src.len() - after.len() + 1;
        }
        let quote = src[start..].chars().next().unwrap_or('"');
        let content_end = match src[..end].ends_with(quote) && end > start + quote.len_utf8() {
            true => end - quote.len_utf8(),
            false => end,
        };
        let content = &src[start + quote.len_utf8()..content_end];
        // `mondi` is mondo wrapped in a sequence, i.e. mini-notation's reading
        // of the same text.
        let code = match name {
            "mondi" => format!("[{content}]"),
            _ => content.to_string(),
        };
        out.push_str(&src[last..name_start]);
        out.push_str(&match compile(&code) {
            Ok(koto) => koto,
            // Preprocessing has no error channel, so the failure is deferred to
            // evaluation, where it reaches the user as the script's error.
            Err(err) => format!("throw {}", koto_string(&format!("mondo: {err}"))),
        });
        last = call_end;
    }
    out.push_str(&src[last..]);
    out
}

/// The body of a script marked as Mondo Notation, i.e. one whose first
/// non-blank line is `// mondo`. One exact spelling — the error a near miss
/// gets names the right one.
///
/// A marker rather than a guess: almost any text parses as mondo (a bare word is
/// a sample name, so there is nothing to reject), which makes "does this look
/// like mondo?" a question with no honest answer. [`looks_like_mondo`] asks it
/// anyway, but only about a script Koto has already refused, and only to point
/// at this line.
pub(super) fn mondo_document(src: &str) -> Option<&str> {
    let mut rest = src.trim_start_matches(|c: char| c.is_whitespace());
    rest = rest.strip_prefix("//")?.trim_start_matches(' ');
    let (word, body) = match rest.find('\n') {
        Some(end) => (&rest[..end], &rest[end + 1..]),
        None => (rest, ""),
    };
    (word.trim_end() == "mondo").then_some(body)
}

/// Whether `src` compiles as Mondo Notation. Only meaningful for a script that
/// is not valid Koto, since the two languages overlap.
pub(crate) fn looks_like_mondo(src: &str) -> bool {
    mondo_document(src).is_none() && compile(src).is_ok()
}

/// The mondo tag immediately before the string at `at`, as
/// `(name, name start, is the call form)`.
fn tag_before(src: &str, at: usize) -> Option<(&'static str, usize, bool)> {
    let mut head = &src[..at];
    let is_call = head.ends_with('(');
    if is_call {
        head = head[..head.len() - 1].trim_end();
    }
    ["mondolang", "mondo", "mondi"]
        .into_iter()
        .find_map(|name| {
            let rest = head.strip_suffix(name)?;
            (!rest.ends_with(is_ident_char)).then_some((name, rest.len(), is_call))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Upstream's `desguar` helper: parse, then print the desugared tree.
    fn sugar(code: &str) -> String {
        print_ast(&parse(code).expect("parse"))
    }

    // -- ported from strudel/packages/mondo/test/mondo.test.mjs --------------

    #[test]
    fn tokenizes_the_token_types() {
        let kinds = |code: &str| {
            tokenize(code)
                .expect("tokenize")
                .into_iter()
                .map(|t| (t.kind, t.value))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            kinds("(one two)"),
            [
                (Kind::OpenList, "(".into()),
                (Kind::Plain, "one".into()),
                (Kind::Plain, "two".into()),
                (Kind::CloseList, ")".into()),
            ]
        );
        // Numbers before operators, so `-1` is one token and `0..2` is three.
        assert_eq!(
            kinds("1 .2 1.2 10 22.3 -1"),
            [
                (Kind::Number, "1".into()),
                (Kind::Number, ".2".into()),
                (Kind::Number, "1.2".into()),
                (Kind::Number, "10".into()),
                (Kind::Number, "22.3".into()),
                (Kind::Number, "-1".into()),
            ]
        );
        assert_eq!(
            kinds("0..2"),
            [
                (Kind::Number, "0".into()),
                (Kind::Op, "..".into()),
                (Kind::Number, "2".into()),
            ]
        );
        assert_eq!(kinds("a // hello").last().unwrap().0, Kind::Comment);
        assert_eq!(
            kinds("\"a b\" 'c d'"),
            [(Kind::Str, "\"a b\"".into()), (Kind::Str, "'c d'".into()),]
        );
    }

    #[test]
    fn parses_s_expressions() {
        assert_eq!(sugar(""), "()");
        assert_eq!(sugar("a"), "(a)");
        assert_eq!(sugar("()"), "()");
        assert_eq!(sugar("(a)"), "(a)");
        assert_eq!(sugar("(a b)"), "(a b)");
        assert_eq!(sugar("(a (b c))"), "(a (b c))");
        assert_eq!(sugar("a // hello"), "(a // hello)");
    }

    #[test]
    fn desugars_brackets() {
        assert_eq!(sugar("[a b c]"), "(square a b c)");
        assert_eq!(sugar("[a [b c] d]"), "(square a (square b c) d)");
        assert_eq!(sugar("<a b c>"), "(angle a b c)");
        assert_eq!(sugar("<a <b c> d>"), "(angle a (angle b c) d)");
        assert_eq!(sugar("[a <b c>]"), "(square a (angle b c))");
        assert_eq!(sugar("<a [b c]>"), "(angle a (square b c))");
    }

    #[test]
    fn desugars_pipes() {
        assert_eq!(sugar("s jazz # fast 2"), "(fast 2 (s jazz))");
        assert_eq!(sugar("[bd cp # fast 2]"), "(fast 2 (square bd cp))");
        assert_eq!(
            sugar("s jazz # fast 2 # slow 2"),
            "(slow 2 (fast 2 (s jazz)))"
        );
        assert_eq!(sugar("(s cp # fast 2)"), "(fast 2 (s cp))");
        assert_eq!(
            sugar("[bd cp # fast 2, x]"),
            "(stack (fast 2 (square bd cp)) x)"
        );
    }

    #[test]
    fn desugars_stacks_and_choices() {
        assert_eq!(sugar("[bd, hh | oh]"), "(stack bd (or hh oh))");
        assert_eq!(
            sugar("[bd, hh | [oh rim]]"),
            "(stack bd (or hh (square oh rim)))"
        );
        assert_eq!(sugar("[bd, hh]"), "(stack bd hh)");
        assert_eq!(sugar("[bd, hh oh]"), "(stack bd (square hh oh))");
        assert_eq!(
            sugar("[bd cp, hh oh]"),
            "(stack (square bd cp) (square hh oh))"
        );
        assert_eq!(sugar("<bd, hh>"), "(stack bd hh)");
        assert_eq!(sugar("<bd, hh oh>"), "(stack bd (angle hh oh))");
        assert_eq!(
            sugar("<bd cp, hh oh>"),
            "(stack (angle bd cp) (angle hh oh))"
        );
        assert_eq!(sugar("(s bd, s cp)"), "(stack (s bd) (s cp))");
        // `$` is an alias for `,`, including the empty leading chunk.
        assert_eq!(sugar("$ s bd $ s hh"), "(stack (s bd) (s hh))");
    }

    #[test]
    fn desugars_operators() {
        assert_eq!(sugar("[a b*2 c d/3 e]"), "(square a (* 2 b) c (/ 3 d) e)");
        assert_eq!(sugar("[a [b c]*3]"), "(square a (* 3 (square b c)))");
        assert_eq!(sugar("[a b*<2 3> c]"), "(square a (* (angle 2 3) b) c)");
        assert_eq!(sugar("x:y"), "(: y x)");
        assert_eq!(sugar("x:y:z"), "(: z (: y x))");
        assert_eq!(sugar("bd:0*2"), "(* 2 (: 0 bd))");
        assert_eq!(sugar("bd&3:8"), "(& (: 8 3) bd)");
        assert_eq!(sugar("0..2"), "(.. 2 0)");
    }

    #[test]
    fn desugars_lambdas() {
        assert_eq!(sugar("(#)"), "(fn (_) _)");
        assert_eq!(sugar("(# fast 2)"), "(fn (_) (fast 2 _))");
        assert_eq!(sugar("((# mul 2) 2)"), "((fn (_) (mul 2 _)) 2)");
        assert_eq!(sugar("(# fast 2 # room 1)"), "(fn (_) (room 1 (fast 2 _)))");
    }

    #[test]
    fn desugars_the_readme_example() {
        assert_eq!(
            sugar("s [bd hh*2 (cp # crush 4) <mt ht lt>] # speed .8"),
            "(speed .8 (s (square bd (* 2 hh) (crush 4 cp) (angle mt ht lt))))"
        );
    }

    #[test]
    fn rejects_two_operators_in_a_row() {
        let err = parse("a * - b").expect_err("two ops");
        assert!(err.contains("2 ops in a row"), "{err}");
        // The rest hint fires only for `-`, which is the one that reads as one.
        let err = parse("a - * b").expect_err("two ops");
        assert!(err.contains("rest"), "{err}");
    }

    // -- code generation ----------------------------------------------------

    #[test]
    fn compiles_calls_and_chains() {
        assert_eq!(compile("s hh*8").unwrap(), "fast(8, 'hh').s()");
        // A prelude name is a standalone pattern-last function; controls and
        // method-only names take mondo's last argument as their receiver.
        assert_eq!(
            compile("s jazz # fast 2").unwrap(),
            "fast(2, pure('jazz').s())"
        );
        assert_eq!(compile("n 0 # jux rev").unwrap(), "jux(rev, pure(0).n())");
        assert_eq!(
            compile("s bd # tag hi").unwrap(),
            "pure('bd').s().tag('hi')"
        );
        // A bareword naming a runtime value is that value, not a string.
        assert_eq!(compile("lpf sine").unwrap(), "sine.lpf()");
        assert_eq!(compile("s 'sine'").unwrap(), "pure('sine').s()");
    }

    #[test]
    fn compiles_brackets_and_operators() {
        assert_eq!(
            compile("[bd hh]").unwrap(),
            "stepcat('bd', 'hh').setSteps(1)"
        );
        assert_eq!(compile("<bd hh>").unwrap(), "stepcat('bd', 'hh').pace(1)");
        assert_eq!(compile("{bd hh}").unwrap(), "stepcat('bd', 'hh')");
        assert_eq!(compile("[bd, hh]").unwrap(), "stack('bd', 'hh')");
        assert_eq!(compile("[bd | hh]").unwrap(), "chooseIn('bd', 'hh')");
        assert_eq!(compile("bd?.3").unwrap(), "degradeBy(0.3, 'bd')");
        // `:` and `..` build the same values mini-notation does, so they are
        // compiled to it rather than to a binding of their own.
        assert_eq!(compile("s bd:3").unwrap(), "\"bd:3\".s()");
        assert_eq!(compile("n 0..7").unwrap(), "\"0 .. 7\".n()");
        assert_eq!(compile("s bd&3:8").unwrap(), "bjork([3, 8], 'bd').s()");
    }

    #[test]
    fn compiles_lambdas_and_defs() {
        assert_eq!(
            compile("n 0 # sometimes (# dec .1)").unwrap(),
            "sometimes((|mondoArg0| mondoArg0.dec(0.1)), pure(0).n())"
        );
        // A `def` is substituted at each use and evaluates to silence itself.
        assert_eq!(
            compile("$ def melody [0 1] $ n melody").unwrap(),
            "stack(silence(), (stepcat(0, 1).setSteps(1)).n())"
        );
    }

    #[test]
    fn compiles_rests_and_empty_programs() {
        assert_eq!(compile("").unwrap(), "silence()");
        assert_eq!(
            compile("s [bd ~ _]").unwrap(),
            "stepcat('bd', silence(), silence()).setSteps(1).s()"
        );
    }

    // -- template rewriting -------------------------------------------------

    #[test]
    fn rewrites_only_mondo_tags() {
        assert_eq!(rewrite_mondo_templates("mondo`s hh`"), "pure('hh').s()");
        assert_eq!(rewrite_mondo_templates("mondolang`s hh`"), "pure('hh').s()");
        assert_eq!(
            rewrite_mondo_templates(r#"mondo("s hh")"#),
            "pure('hh').s()"
        );
        // `mondi` reads its argument as a sequence, like mini-notation.
        assert_eq!(
            rewrite_mondo_templates("mondi`bd hh`"),
            "stepcat('bd', 'hh').setSteps(1)"
        );
        // Surrounding code is kept, and a tag that is not mondo is left alone.
        assert_eq!(
            rewrite_mondo_templates("stack(mondo`s hh`, s(\"bd\"))"),
            "stack(pure('hh').s(), s(\"bd\"))"
        );
        assert_eq!(
            rewrite_mondo_templates("loadCsound`instr 1`"),
            "loadCsound`instr 1`"
        );
        // A name that merely ends in the tag is not the tag.
        assert_eq!(rewrite_mondo_templates("demondo`s hh`"), "demondo`s hh`");
    }

    #[test]
    fn reads_a_marked_script_as_a_whole_mondo_document() {
        assert_eq!(mondo_document("// mondo\ns hh"), Some("s hh"));
        assert_eq!(mondo_document("\n\n//mondo  \ns hh"), Some("s hh"));
        assert_eq!(mondo_document("// mondo"), Some(""));
        // One exact spelling, and only on the first line.
        assert_eq!(mondo_document("// MONDO\ns hh"), None);
        assert_eq!(mondo_document("// mondo notation\ns hh"), None);
        assert_eq!(mondo_document("s hh\n// mondo"), None);
        assert_eq!(mondo_document("s(\"hh\")"), None);
        // The marker turns the rest of the document into one Koto expression.
        assert_eq!(
            rewrite_mondo_templates("// mondo\n$ s bd $ s hh"),
            "stack(pure('bd').s(), pure('hh').s())"
        );
    }

    #[test]
    fn defers_a_parse_error_to_evaluation() {
        let out = rewrite_mondo_templates("mondo`s [bd`");
        assert!(out.starts_with("throw 'mondo: "), "{out}");
        // The message carries a snippet of the user's source, which must not be
        // able to end the string literal it is deferred in.
        let out = rewrite_mondo_templates("// mondo\ns .bd\ns hh");
        assert_eq!(out.lines().count(), 1, "{out}");
        assert!(out.starts_with("throw 'mondo: "), "{out}");
    }
    #[test]
    fn an_operator_with_nothing_on_one_side_is_a_plain_function() {
        // The comment on `desugar_ops` names both shapes: `# *2` is a lambda
        // over the missing left side, and the trailing `-` of `[c -]` is a
        // reference rather than a subtraction. Getting the guard wrong here
        // indexes off the end of the child list rather than misreading it.
        assert_eq!(compile("* 2").unwrap(), "fast(2)");
        assert_eq!(
            compile("[c -]").unwrap(),
            "stepcat('c', silence()).setSteps(1)"
        );
        assert_eq!(compile("# *2").unwrap(), "(|mondoArg0| fast(2, mondoArg0))");
        // An operator straight after a pipe has no left side either.
        assert_eq!(
            compile("s bd | * 2").unwrap(),
            "chooseIn(pure('bd').s(), fast(2))"
        );
        // With both sides present it is an ordinary call, innermost first.
        assert_eq!(compile("* 2 3").unwrap(), "fast(2, 3)");
        assert_eq!(
            compile("s bd # * 2 # fast 3").unwrap(),
            "fast(3, fast(2, pure('bd').s()))"
        );
    }

    #[test]
    fn two_operators_in_a_row_say_so() {
        assert_eq!(
            compile("s bd * * 2").unwrap_err(),
            "got 2 ops in a row: \"**\""
        );
        // Named in source order, which only two *different* operators can
        // show — with a doubled one, either side reads the same.
        assert_eq!(
            compile("s bd * + 2").unwrap_err(),
            "got 2 ops in a row: \"*+\""
        );
        assert_eq!(
            compile("s bd + * 2").unwrap_err(),
            "got 2 ops in a row: \"+*\""
        );
        // A doubled `-` is nearly always a rest written the JavaScript way.
        assert_eq!(
            compile("s bd - - 2").unwrap_err(),
            "got 2 ops in a row: \"--\". you probably want a rest, which is \"_\" in mondo!"
        );
    }

    #[test]
    fn a_string_is_escaped_for_the_koto_it_lands_in() {
        // The emitted source is Koto, so a literal newline, carriage return or
        // `$` would end the string or start an interpolation.
        // The input carries the real character; the output carries its escape.
        assert_eq!(compile("s \"a\nb\"").unwrap(), "pure('a\\nb').s()");
        assert_eq!(compile("s \"a\rb\"").unwrap(), "pure('a\\rb').s()");
        assert_eq!(compile("s \"a$b\"").unwrap(), "pure('a\\$b').s()");
    }

    #[test]
    fn a_lambda_parameter_keeps_its_name_only_when_koto_can_use_it() {
        assert_eq!(compile("(fn (x) (s x))").unwrap(), "(|x| x.s())");
        // `_` is what the `#` shorthand generates and is not an identifier,
        // and a name starting with a digit is not one either.
        assert_eq!(
            compile("(fn (_) (s _))").unwrap(),
            "(|mondoArg0| mondoArg0.s())"
        );
        assert_eq!(
            compile("(fn (1x) (s 1x))").unwrap(),
            "(|mondoArg0, x| x.s(1))"
        );
    }
    #[test]
    fn an_unclosed_bracket_is_reported_rather_than_guessed_at() {
        for src in ["[c", "(c", "<c", "{c"] {
            assert_eq!(
                compile(src).unwrap_err(),
                "unexpected end of file: missing closing bracket",
                "{src}"
            );
        }
    }

    #[test]
    fn a_colon_chain_needs_literal_operands() {
        // `bd:3` is a sample index, so both sides have to be literals; a
        // bracketed group or a call on the right is not one.
        assert_eq!(compile("s bd:3").unwrap(), "\"bd:3\".s()");
        assert_eq!(
            compile("s bd:[a b]").unwrap_err(),
            "\":\" needs literal operands"
        );
        assert_eq!(
            compile("s bd:(fast 2)").unwrap_err(),
            "\":\" needs literal operands"
        );
    }

    #[test]
    fn a_range_takes_its_ends_as_written() {
        // Quoted or bare, the ends of a `..` are literals and the quotes are
        // not part of them.
        assert_eq!(compile("note c .. e").unwrap(), "\"c .. e\".note()");
        assert_eq!(
            compile("note \"c\" .. \"e\"").unwrap(),
            "\"c .. e\".note()"
        );
    }

    #[test]
    fn an_unterminated_template_does_not_take_the_script_with_it() {
        // A half-typed `mondo\`` is what the editor holds between keystrokes,
        // so the rewriter has to hand back something compilable rather than
        // index past the end of the source.
        assert_eq!(rewrite_mondo_templates("x = mondo`"), "x = silence()");
        assert_eq!(rewrite_mondo_templates("x = mondo``"), "x = silence()");
        // A template next to a plain one keeps them apart.
        assert_eq!(
            rewrite_mondo_templates("x = f(mondo`s bd`, `plain`)"),
            "x = f(pure('bd').s(), `plain`)"
        );
    }
}
