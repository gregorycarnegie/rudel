// bytebeat.rs - the `bytebeat` synth. Ports superdough's `byte-beat-processor`
// worklet (strudel/packages/superdough/worklets.mjs) plus the `registerSound`
// wrapper in synth.mjs that picks a default expression by `n` and wraps the
// output in a linear ADSR.
//
// Upstream compiles the expression with `new Function(...)`, i.e. it runs real
// JavaScript. Rudel has no JS engine, so this module carries a small parser and
// evaluator for the integer-expression language bytebeats actually use: JS
// operator precedence, JS `ToInt32` coercion on the bitwise operators, and the
// `Math` functions exposed to the compiled function.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    envelope::{Adsr, adsr_value},
    filter::{FilterSet, VoiceFilters},
    modulator::{ModBank, ModSpec},
    pitch::note_to_freq,
    voice::VoiceLike,
};
use rudel_core::ValueMap;
use std::f32::consts::FRAC_PI_2;

/// The 15 built-in expressions, selected by `n % 15`. Verbatim from
/// `synth.mjs`'s `defaultBeats`.
pub const DEFAULT_BEATS: [&str; 15] = [
    "(t%255 >= t/255%255)*255",
    "(t*(t*8%60 <= 300)|(-t)*(t*4%512 < 256))+t/400",
    "t",
    "t*(t >> 10^t)",
    "t&128",
    "t&t>>8",
    "((t%255+t%128+t%64+t%32+t%16+t%127.8+t%64.8+t%32.8+t%16.8)/3)",
    "((t%64+t%63.8+t%64.15+t%64.35+t%63.5)/1.25)",
    "(t&(t>>7)-t)",
    "(sin(t*PI/128)*127+127)",
    "((t^t/2+t+64*(sin((t*PI/64)+(t*PI/32768))+64))%128*2)",
    "((t^t/2+t+64*(cos >> 0))%127.85*2)",
    "((t^t/2+t+64)%128*2)",
    "(((t * .25)^(t * .25)/100+(t * .25))%128)*2",
    "((t^t/2+t+64)%7 * 24)",
];

// ---------------------------------------------------------------------------
// Expression language

#[derive(Clone, Debug, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    UShr,
    BitAnd,
    BitOr,
    BitXor,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Clone, Debug)]
enum Node {
    Num(f64),
    /// The sample counter `t`.
    T,
    /// A `Math` constant, or any identifier we do not know (JS yields a function
    /// object there, which coerces to 0 through the bitwise ops that use it).
    Const(f64),
    Unary(char, Box<Node>),
    Bin(Op, Box<Node>, Box<Node>),
    Cond(Box<Node>, Box<Node>, Box<Node>),
    Call(Fun, Vec<Node>),
}

#[derive(Clone, Copy, Debug)]
enum Fun {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sqrt,
    Abs,
    Floor,
    Ceil,
    Round,
    Trunc,
    Sign,
    Log,
    Log2,
    Exp,
    Pow,
    Min,
    Max,
}

impl Fun {
    fn from_name(name: &str) -> Option<Fun> {
        Some(match name {
            "sin" => Fun::Sin,
            "cos" => Fun::Cos,
            "tan" => Fun::Tan,
            "asin" => Fun::Asin,
            "acos" => Fun::Acos,
            "atan" => Fun::Atan,
            "sqrt" => Fun::Sqrt,
            "abs" => Fun::Abs,
            "floor" | "int" => Fun::Floor,
            "ceil" => Fun::Ceil,
            "round" => Fun::Round,
            "trunc" => Fun::Trunc,
            "sign" => Fun::Sign,
            "log" => Fun::Log,
            "log2" => Fun::Log2,
            "exp" => Fun::Exp,
            "pow" => Fun::Pow,
            "min" => Fun::Min,
            "max" => Fun::Max,
            _ => return None,
        })
    }

    fn eval(self, args: &[f64]) -> f64 {
        let a = args.first().copied().unwrap_or(f64::NAN);
        let b = args.get(1).copied().unwrap_or(f64::NAN);
        match self {
            Fun::Sin => a.sin(),
            Fun::Cos => a.cos(),
            Fun::Tan => a.tan(),
            Fun::Asin => a.asin(),
            Fun::Acos => a.acos(),
            Fun::Atan => a.atan(),
            Fun::Sqrt => a.sqrt(),
            Fun::Abs => a.abs(),
            Fun::Floor => a.floor(),
            Fun::Ceil => a.ceil(),
            Fun::Round => (a + 0.5).floor(), // JS Math.round: half rounds up
            Fun::Trunc => a.trunc(),
            Fun::Sign => {
                if a > 0.0 {
                    1.0
                } else if a < 0.0 {
                    -1.0
                } else {
                    a
                }
            }
            Fun::Log => a.ln(),
            Fun::Log2 => a.log2(),
            Fun::Exp => a.exp(),
            Fun::Pow => a.powf(b),
            Fun::Min => a.min(b),
            Fun::Max => a.max(b),
        }
    }
}

/// JS `ToInt32`: truncate toward zero, wrap modulo 2^32, reinterpret as signed.
/// `NaN`/`±Infinity` become 0.
fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    (x.trunc().rem_euclid(4294967296.0) as u32) as i32
}

/// A parsed bytebeat expression, evaluable per sample.
#[derive(Clone, Debug)]
pub struct ByteBeatExpr {
    root: Node,
}

impl ByteBeatExpr {
    /// Parse `src`. An expression that does not parse falls back to the
    /// constant `0` (silence), which is also what `new Function` is handed for
    /// an empty `codetext` upstream.
    pub fn parse(src: &str) -> ByteBeatExpr {
        let root = Parser::new(src).parse().unwrap_or(Node::Num(0.0));
        ByteBeatExpr { root }
    }

    /// Evaluate at sample counter `t`.
    pub fn eval(&self, t: f64) -> f64 {
        eval(&self.root, t)
    }
}

fn eval(node: &Node, t: f64) -> f64 {
    match node {
        Node::Num(v) => *v,
        Node::T => t,
        Node::Const(v) => *v,
        Node::Unary(op, inner) => {
            let v = eval(inner, t);
            match op {
                '-' => -v,
                '~' => !to_int32(v) as f64,
                '!' => {
                    if truthy(v) {
                        0.0
                    } else {
                        1.0
                    }
                }
                _ => v,
            }
        }
        Node::Cond(c, a, b) => {
            if truthy(eval(c, t)) {
                eval(a, t)
            } else {
                eval(b, t)
            }
        }
        Node::Call(f, args) => {
            let vals: Vec<f64> = args.iter().map(|a| eval(a, t)).collect();
            f.eval(&vals)
        }
        Node::Bin(op, l, r) => {
            // `&&`/`||` short-circuit and yield an operand, not a boolean.
            match op {
                Op::And => {
                    let a = eval(l, t);
                    return if truthy(a) { eval(r, t) } else { a };
                }
                Op::Or => {
                    let a = eval(l, t);
                    return if truthy(a) { a } else { eval(r, t) };
                }
                _ => {}
            }
            let a = eval(l, t);
            let b = eval(r, t);
            match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div => a / b,
                Op::Rem => a % b,
                // Bitwise operands are ToInt32'd; shift counts use the low 5 bits.
                Op::Shl => (to_int32(a) << (to_int32(b) & 31)) as f64,
                Op::Shr => (to_int32(a) >> (to_int32(b) & 31)) as f64,
                Op::UShr => ((to_int32(a) as u32) >> (to_int32(b) & 31)) as f64,
                Op::BitAnd => (to_int32(a) & to_int32(b)) as f64,
                Op::BitOr => (to_int32(a) | to_int32(b)) as f64,
                Op::BitXor => (to_int32(a) ^ to_int32(b)) as f64,
                Op::Lt => bool_num(a < b),
                Op::Gt => bool_num(a > b),
                Op::Le => bool_num(a <= b),
                Op::Ge => bool_num(a >= b),
                Op::Eq => bool_num(a == b),
                Op::Ne => bool_num(a != b),
                Op::And | Op::Or => unreachable!("handled above"),
            }
        }
    }
}

fn truthy(v: f64) -> bool {
    v != 0.0 && !v.is_nan()
}

fn bool_num(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Parser<'a> {
        Parser {
            src: src.as_bytes(),
            pos: 0,
        }
    }

    fn parse(&mut self) -> Option<Node> {
        let node = self.conditional()?;
        self.skip_ws();
        // Trailing junk means we misread the expression; fail rather than
        // silently playing half of it.
        if self.pos < self.src.len() {
            return None;
        }
        Some(node)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.src.len() && self.src[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// Consume `tok` if it is next (after whitespace).
    fn eat(&mut self, tok: &str) -> bool {
        self.skip_ws();
        let end = self.pos + tok.len();
        if end <= self.src.len() && &self.src[self.pos..end] == tok.as_bytes() {
            self.pos = end;
            true
        } else {
            false
        }
    }

    /// Peek whether `tok` is next without consuming it.
    fn at(&mut self, tok: &str) -> bool {
        self.skip_ws();
        let end = self.pos + tok.len();
        end <= self.src.len() && &self.src[self.pos..end] == tok.as_bytes()
    }

    fn conditional(&mut self) -> Option<Node> {
        let cond = self.binary(0)?;
        if self.eat("?") {
            let a = self.conditional()?;
            if !self.eat(":") {
                return None;
            }
            let b = self.conditional()?;
            return Some(Node::Cond(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Some(cond)
    }

    /// Precedence-climbing over the JS binary operators, lowest level first.
    /// Each level's operators are listed longest-first so `>>>` wins over `>>`
    /// and `<=` over `<`.
    fn binary(&mut self, level: usize) -> Option<Node> {
        const LEVELS: &[&[(&str, Op)]] = &[
            &[("||", Op::Or)],
            &[("&&", Op::And)],
            &[("|", Op::BitOr)],
            &[("^", Op::BitXor)],
            &[("&", Op::BitAnd)],
            &[
                ("===", Op::Eq),
                ("!==", Op::Ne),
                ("==", Op::Eq),
                ("!=", Op::Ne),
            ],
            &[
                ("<<", Op::Shl),
                (">>>", Op::UShr),
                (">>", Op::Shr),
                ("<=", Op::Le),
                (">=", Op::Ge),
                ("<", Op::Lt),
                (">", Op::Gt),
            ],
            &[("+", Op::Add), ("-", Op::Sub)],
            &[("*", Op::Mul), ("/", Op::Div), ("%", Op::Rem)],
        ];
        if level >= LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        'outer: loop {
            for (tok, op) in LEVELS[level] {
                // `|` must not swallow `||`, `&` must not swallow `&&`.
                if (*tok == "|" && self.at("||")) || (*tok == "&" && self.at("&&")) {
                    continue;
                }
                if self.eat(tok) {
                    let right = self.binary(level + 1)?;
                    left = Node::Bin(op.clone(), Box::new(left), Box::new(right));
                    continue 'outer;
                }
            }
            break;
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<Node> {
        self.skip_ws();
        for op in ['-', '+', '~', '!'] {
            let s = op.to_string();
            // `!=` is a binary operator, not a unary `!`.
            if op == '!' && self.at("!=") {
                break;
            }
            if self.eat(&s) {
                let inner = self.unary()?;
                return Some(if op == '+' {
                    inner
                } else {
                    Node::Unary(op, Box::new(inner))
                });
            }
        }
        self.primary()
    }

    fn primary(&mut self) -> Option<Node> {
        self.skip_ws();
        if self.pos >= self.src.len() {
            return None;
        }
        if self.eat("(") {
            let inner = self.conditional()?;
            if !self.eat(")") {
                return None;
            }
            return Some(inner);
        }
        let c = self.src[self.pos];
        if c.is_ascii_digit() || (c == b'.' && self.peek_digit(1)) {
            return self.number();
        }
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            return self.identifier();
        }
        None
    }

    fn peek_digit(&self, ahead: usize) -> bool {
        self.src
            .get(self.pos + ahead)
            .is_some_and(u8::is_ascii_digit)
    }

    fn number(&mut self) -> Option<Node> {
        let start = self.pos;
        // Hex literals show up in bytebeats (`0xff`).
        if self.src[self.pos] == b'0'
            && matches!(self.src.get(self.pos + 1), Some(b'x' | b'X'))
            && self
                .src
                .get(self.pos + 2)
                .is_some_and(u8::is_ascii_hexdigit)
        {
            self.pos += 2;
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_hexdigit() {
                self.pos += 1;
            }
            let text = std::str::from_utf8(&self.src[start + 2..self.pos]).ok()?;
            return i64::from_str_radix(text, 16)
                .ok()
                .map(|v| Node::Num(v as f64));
        }
        while self.pos < self.src.len()
            && (self.src[self.pos].is_ascii_digit() || self.src[self.pos] == b'.')
        {
            self.pos += 1;
        }
        // Exponent form (`1e3`, `2.5e-4`).
        if matches!(self.src.get(self.pos), Some(b'e' | b'E')) {
            let mark = self.pos;
            self.pos += 1;
            if matches!(self.src.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
                while self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
            } else {
                self.pos = mark;
            }
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).ok()?;
        text.parse::<f64>().ok().map(Node::Num)
    }

    fn identifier(&mut self) -> Option<Node> {
        let start = self.pos;
        while self.pos < self.src.len()
            && (self.src[self.pos].is_ascii_alphanumeric()
                || self.src[self.pos] == b'_'
                || self.src[self.pos] == b'$')
        {
            self.pos += 1;
        }
        let name = std::str::from_utf8(&self.src[start..self.pos]).ok()?;
        // A call: `sin(x)`, `pow(a, b)`.
        if self.at("(")
            && let Some(f) = Fun::from_name(name)
        {
            self.eat("(");
            let mut args = Vec::new();
            if !self.eat(")") {
                loop {
                    args.push(self.conditional()?);
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat(")") {
                        break;
                    }
                    return None;
                }
            }
            return Some(Node::Call(f, args));
        }
        Some(match name {
            "t" => Node::T,
            "PI" => Node::Const(std::f64::consts::PI),
            "E" => Node::Const(std::f64::consts::E),
            "LN2" => Node::Const(std::f64::consts::LN_2),
            "LN10" => Node::Const(std::f64::consts::LN_10),
            "SQRT2" => Node::Const(std::f64::consts::SQRT_2),
            // Anything else is a bare `Math`/global reference in upstream, which
            // is a function object: NaN in arithmetic, 0 through `|0`/`>>0`.
            _ => Node::Const(f64::NAN),
        })
    }
}

// ---------------------------------------------------------------------------
// Voice

/// Resolved `bytebeat` voice parameters (superdough's `registerSound('bytebeat')`).
#[derive(Clone, Debug)]
pub struct ByteBeatParams {
    pub expr: ByteBeatExpr,
    pub freq: f32,
    /// `byteBeatStartTime`, floored — the initial `t` offset.
    pub start_time: f64,
    pub gain: f32,
    pub pan: f32,
    pub adsr: Adsr,
    /// Note hold time in seconds, before the release.
    pub duration: f32,
    pub filters: FilterSet,
}

impl ByteBeatParams {
    pub fn from_controls(map: &ValueMap, duration: f32) -> ByteBeatParams {
        let num = |k: &str| map.get(k).and_then(|v| v.as_f64());
        let n = num("n").unwrap_or(0.0) as i64;
        // `defaultBeats[n % defaultBeats.length]` — JS `%` keeps the sign, and a
        // negative index reads `undefined`, which compiles to `0`. Mirror that
        // by falling back to silence rather than wrapping.
        let default = usize::try_from(n % DEFAULT_BEATS.len() as i64)
            .ok()
            .and_then(|i| DEFAULT_BEATS.get(i).copied())
            .unwrap_or("0");
        let src = map
            .get("byteBeatExpression")
            .and_then(|v| v.as_str())
            .unwrap_or(default);

        // getFrequencyFromValue: freq wins, then note, then n as a note number.
        let freq = map
            .get("freq")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32)
            .or_else(|| map.get("note").and_then(note_to_freq))
            .or_else(|| map.get("n").and_then(note_to_freq))
            .unwrap_or(440.0);

        // getADSRValues(..., 'linear', [0.001, 0.05, 0.6, 0.01]).
        let mut adsr = Adsr::default();
        if let Some(a) = num("attack") {
            adsr.attack = a as f32;
        }
        if let Some(d) = num("decay") {
            adsr.decay = d as f32;
        }
        if let Some(s) = num("sustain") {
            adsr.sustain = s as f32;
        }
        if let Some(r) = num("release") {
            adsr.release = r as f32;
        }

        ByteBeatParams {
            expr: ByteBeatExpr::parse(src),
            freq,
            start_time: num("byteBeatStartTime").unwrap_or(0.0).floor(),
            gain: num("gain").unwrap_or(1.0) as f32,
            pan: num("pan").unwrap_or(0.5) as f32,
            adsr,
            duration,
            filters: FilterSet::from_controls(map),
        }
    }
}

/// A bytebeat voice: the worklet's per-sample loop plus the ADSR gain the
/// `registerSound` wrapper puts after it.
pub struct ByteBeatVoice {
    params: ByteBeatParams,
    filters: VoiceFilters,
    mods: ModBank,
    sample_rate: f32,
    /// The worklet's own sample counter.
    t: f64,
    /// Elapsed seconds, driving the envelope.
    elapsed: f32,
    end: f32,
    left_gain: f32,
    right_gain: f32,
}

impl ByteBeatVoice {
    pub fn new(params: ByteBeatParams, sample_rate: f32) -> ByteBeatVoice {
        ByteBeatVoice::with_mods(params, sample_rate, &[])
    }

    pub fn with_mods(params: ByteBeatParams, sample_rate: f32, mods: &[ModSpec]) -> ByteBeatVoice {
        let pan = params.pan.clamp(0.0, 1.0);
        // synth.mjs: end = begin + duration + release + 0.01.
        let end = params.duration + params.adsr.release + 0.01;
        ByteBeatVoice {
            filters: VoiceFilters::new(&params.filters, sample_rate, false),
            mods: ModBank::new(mods, sample_rate as f64),
            sample_rate,
            t: 0.0,
            elapsed: 0.0,
            end,
            left_gain: (pan * FRAC_PI_2).cos(),
            right_gain: (pan * FRAC_PI_2).sin(),
            params,
        }
    }
}

impl VoiceLike for ByteBeatVoice {
    fn tick(&mut self) -> (f32, f32) {
        if self.is_done() {
            return (0.0, 0.0);
        }
        // local_t = 256/sampleRate * frequency * t + initialOffset
        let scale = 256.0 / self.sample_rate as f64;
        let local_t = scale * self.params.freq as f64 * self.t + self.params.start_time;
        let value = self.params.expr.eval(local_t);
        let signal = (to_int32(value) & 255) as f32 / 127.5 - 1.0;
        // The worklet clamps to ±0.4 to stop a runaway expression blowing up
        // the output.
        let out = (signal * 0.2).clamp(-0.4, 0.4);
        self.mods.tick();
        let out = self.filters.process(
            out,
            self.elapsed,
            self.params.duration,
            self.sample_rate,
            &self.mods,
        );
        let env = adsr_value(&self.params.adsr, self.elapsed, self.params.duration);

        self.t += 1.0;
        self.elapsed += 1.0 / self.sample_rate;
        let s = out * env * self.params.gain;
        (s * self.left_gain, s * self.right_gain)
    }

    fn is_done(&self) -> bool {
        self.elapsed >= self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudel_core::Value;

    fn ev(src: &str, t: f64) -> f64 {
        ByteBeatExpr::parse(src).eval(t)
    }

    #[test]
    fn arithmetic_and_precedence_match_js() {
        assert_eq!(ev("1+2*3", 0.0), 7.0);
        assert_eq!(ev("(1+2)*3", 0.0), 9.0);
        assert_eq!(ev("7%3", 0.0), 1.0);
        assert_eq!(ev("7/2", 0.0), 3.5);
        assert_eq!(ev("-3+1", 0.0), -2.0);
        // `+` binds tighter than `>>`, which binds tighter than `&`, `^`, `|`.
        assert_eq!(ev("1+1>>1", 0.0), 1.0);
        assert_eq!(ev("1|2^3&1", 0.0), 3.0);
        // Comparisons are numeric 1/0, as JS booleans coerce.
        assert_eq!(ev("2>1", 0.0), 1.0);
        assert_eq!(ev("(2>1)*255", 0.0), 255.0);
        // Ternary and short-circuit operators.
        assert_eq!(ev("1?10:20", 0.0), 10.0);
        assert_eq!(ev("0?10:20", 0.0), 20.0);
        assert_eq!(ev("0||5", 0.0), 5.0);
        assert_eq!(ev("3&&7", 0.0), 7.0);
    }

    #[test]
    fn bitwise_ops_use_js_int32_semantics() {
        // Operands truncate toward zero before the bitwise op.
        assert_eq!(ev("t&255", 300.7), 44.0);
        // Shift counts are masked to 5 bits, so `1<<33` is `1<<1`.
        assert_eq!(ev("1<<33", 0.0), 2.0);
        // `>>` is arithmetic (sign-extending), `>>>` is not.
        assert_eq!(ev("-8>>1", 0.0), -4.0);
        assert_eq!(ev("-1>>>28", 0.0), 15.0);
        // ~ operates on the int32 value.
        assert_eq!(ev("~5", 0.0), -6.0);
        // A value beyond int32 wraps rather than saturating.
        assert_eq!(ev("t|0", 4294967296.0 + 5.0), 5.0);
        // A bare identifier is a function object upstream: NaN, so `>>0` is 0.
        assert_eq!(ev("cos >> 0", 0.0), 0.0);
    }

    #[test]
    fn every_default_beat_parses_and_evaluates() {
        for (i, src) in DEFAULT_BEATS.iter().enumerate() {
            let expr = ByteBeatExpr::parse(src);
            // A parse failure degrades to the constant 0; make sure none does.
            let varied = (0..64)
                .map(|t| expr.eval(t as f64 * 37.0))
                .collect::<Vec<_>>();
            assert!(
                varied.iter().all(|v| !v.is_nan()),
                "beat {i} ({src}) produced NaN"
            );
            if i != 2 {
                // `t` (beat 2) is monotonic; the rest should at least not be
                // stuck at zero, which is what a failed parse would give.
                assert!(
                    varied.iter().any(|v| to_int32(*v) & 255 != 0),
                    "beat {i} ({src}) is silent — probably a parse failure"
                );
            }
        }
    }

    #[test]
    fn known_beats_match_hand_evaluated_values() {
        // t&t>>8 at t=768: 768 & (768>>8) = 768 & 3 = 0; at t=770: 770 & 3 = 2.
        assert_eq!(ev("t&t>>8", 768.0), 0.0);
        assert_eq!(ev("t&t>>8", 770.0), 2.0);
        // t&128 flips every 128 steps.
        assert_eq!(ev("t&128", 127.0), 0.0);
        assert_eq!(ev("t&128", 128.0), 128.0);
        // (t%255 >= t/255%255)*255 at t=10: 10 >= 0.039… -> 255.
        assert_eq!(ev("(t%255 >= t/255%255)*255", 10.0), 255.0);
    }

    #[test]
    fn malformed_expressions_fall_back_to_silence() {
        assert_eq!(ev("t &&& 3", 5.0), 0.0);
        assert_eq!(ev("(t", 5.0), 0.0);
        assert_eq!(ev("", 5.0), 0.0);
    }

    fn map(pairs: &[(&str, Value)]) -> ValueMap {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn n_selects_a_default_beat_and_an_expression_overrides_it() {
        let p = ByteBeatParams::from_controls(&map(&[("n", Value::F64(5.0))]), 0.2);
        // n=5 is "t&t>>8".
        assert_eq!(p.expr.eval(770.0), 2.0);
        let q = ByteBeatParams::from_controls(
            &map(&[
                ("n", Value::F64(5.0)),
                ("byteBeatExpression", Value::Str("t*2".into())),
            ]),
            0.2,
        );
        assert_eq!(q.expr.eval(21.0), 42.0);
    }

    #[test]
    fn voice_produces_bounded_sound_and_finishes() {
        let p = ByteBeatParams::from_controls(
            &map(&[("n", Value::F64(5.0)), ("freq", Value::F64(440.0))]),
            0.05,
        );
        let mut v = ByteBeatVoice::new(p, 44100.0);
        let mut peak = 0.0_f32;
        for _ in 0..44100 {
            let (l, r) = v.tick();
            peak = peak.max(l.abs()).max(r.abs());
        }
        assert!(peak > 0.0, "bytebeat should make sound");
        assert!(peak <= 0.4, "output is clamped to ±0.4, got {peak}");
        assert!(v.is_done());
    }

    // --- the voice around the expression ------------------------------------
    //
    // `bytebeat_golden.rs` pins the evaluator against V8, but nothing drove the
    // voice that feeds it: 20 of `ByteBeatVoice::tick`'s mutants survived. The
    // arithmetic there decides *which* `t` each sample is evaluated at and how
    // the resulting byte becomes audio, so a wrong step still plays a bytebeat
    // — just not this one.
    //
    // The trick that makes it readable: at a sample rate of 256 with `freq` 1,
    // `local_t = 256/sr * freq * t` is exactly the tick count, so an expression
    // of `t` makes each output sample name its own index.

    const BYTE_SR: f32 = 256.0;

    fn beat(src: &str) -> ByteBeatParams {
        ByteBeatParams {
            expr: ByteBeatExpr::parse(src),
            freq: 1.0,
            start_time: 0.0,
            gain: 1.0,
            pan: 0.5,
            adsr: Adsr {
                attack: 0.0,
                decay: 0.0,
                sustain: 1.0,
                release: 0.0,
            },
            duration: 10.0,
            filters: FilterSet::default(),
        }
    }

    /// The left channel of `n` ticks, with the centre-pan gain divided out.
    fn render(params: ByteBeatParams, n: usize) -> Vec<f32> {
        let g = std::f32::consts::FRAC_1_SQRT_2;
        let mut v = ByteBeatVoice::new(params, BYTE_SR);
        (0..n).map(|_| v.tick().0 / g).collect()
    }

    /// Sample 1 — the first one the gain envelope lets through. Every schedule
    /// opens with `setValueAtTime(0, begin)`, so sample 0 is silent by design
    /// whatever the expression says.
    fn first_audible(params: ByteBeatParams) -> f32 {
        render(params, 2)[1]
    }

    /// What `tick` should emit for a byte value, before gain and pan.
    fn expected(byte: u8) -> f32 {
        ((byte as f32 / 127.5 - 1.0) * 0.2).clamp(-0.4, 0.4)
    }

    #[test]
    fn a_byte_becomes_a_bipolar_sample_scaled_and_clamped() {
        // 0 and 255 are the ends of the range, 128 is just above the middle.
        assert!((first_audible(beat("0")) - expected(0)).abs() < 1e-6);
        assert!((first_audible(beat("255")) - expected(255)).abs() < 1e-6);
        assert!((first_audible(beat("128")) - expected(128)).abs() < 1e-6);
        // Only the low byte is used, so 256 wraps back to 0.
        assert!((first_audible(beat("256")) - expected(0)).abs() < 1e-6);
        assert!((first_audible(beat("511")) - expected(255)).abs() < 1e-6);
        // The ends of the range are ±0.2, well inside the ±0.4 clamp — the
        // clamp is there for the expression, not for this scaling.
        assert!((first_audible(beat("255")) - 0.2).abs() < 1e-6);
        assert!((first_audible(beat("0")) + 0.2).abs() < 1e-6);
        // ...and the envelope really does open at zero.
        assert_eq!(render(beat("255"), 1)[0], 0.0, "sample 0 is silent");
    }

    #[test]
    fn the_evaluated_t_advances_by_the_frequency_over_the_sample_rate() {
        // At this rate and freq 1, sample `n` is evaluated at `t = n`.
        let out = render(beat("t"), 8);
        for (n, &v) in out.iter().enumerate().skip(1) {
            assert!(
                (v - expected(n as u8)).abs() < 1e-6,
                "sample {n} should be byte {n}, got {v}"
            );
        }

        // Doubling the frequency doubles the step.
        let mut p = beat("t");
        p.freq = 2.0;
        let out = render(p, 8);
        for (n, &v) in out.iter().enumerate().skip(1) {
            assert!(
                (v - expected((n * 2) as u8)).abs() < 1e-6,
                "sample {n} at 2x should be byte {}, got {v}",
                n * 2
            );
        }

        // Halving the sample rate doubles it too, since the scale is 256/sr.
        let mut v = ByteBeatVoice::new(beat("t"), BYTE_SR / 2.0);
        let g = std::f32::consts::FRAC_1_SQRT_2;
        let out: Vec<f32> = (0..4).map(|_| v.tick().0 / g).collect();
        for (n, &s) in out.iter().enumerate().skip(1) {
            assert!(
                (s - expected((n * 2) as u8)).abs() < 1e-6,
                "sample {n} at half rate should be byte {}",
                n * 2
            );
        }
    }

    #[test]
    fn the_start_time_offsets_the_evaluated_t() {
        // `byteBeatStartTime` is where in the beat the note begins.
        let mut p = beat("t");
        p.start_time = 100.0;
        let out = render(p, 4);
        for (n, &v) in out.iter().enumerate().skip(1) {
            assert!(
                (v - expected((100 + n) as u8)).abs() < 1e-6,
                "sample {n} should be byte {}, got {v}",
                100 + n
            );
        }
    }

    #[test]
    fn gain_and_pan_shape_the_output_of_the_voice() {
        let peak = |gain: f32| {
            let mut p = beat("255");
            p.gain = gain;
            first_audible(p)
        };
        assert!((peak(1.0) - 0.2).abs() < 1e-6);
        assert!((peak(0.5) - 0.1).abs() < 1e-6, "half gain halves it");
        assert!(peak(0.0).abs() < 1e-9, "no gain is silence");

        let at = |pan: f32| {
            let mut p = beat("255");
            p.pan = pan;
            let mut v = ByteBeatVoice::new(p, BYTE_SR);
            v.tick();
            v.tick()
        };
        let (l, r) = at(0.0);
        assert!(l > 0.19 && r.abs() < 1e-6, "hard left: {l} {r}");
        let (l, r) = at(1.0);
        assert!(l.abs() < 1e-6 && r > 0.19, "hard right: {l} {r}");
        let (l, r) = at(0.5);
        assert!((l - r).abs() < 1e-6, "centre is equal: {l} {r}");
        // Out-of-range pans clamp rather than inverting a channel.
        let (l, r) = at(2.0);
        assert!(l.abs() < 1e-6 && r > 0.19, "clamped right: {l} {r}");
    }

    #[test]
    fn a_voice_runs_for_its_duration_plus_the_release() {
        // synth.mjs: end = duration + release + 0.01.
        let mut p = beat("255");
        p.duration = 0.1;
        p.adsr.release = 0.05;
        let mut v = ByteBeatVoice::new(p, BYTE_SR);
        let mut frames = 0;
        while !v.is_done() && frames < 10_000 {
            v.tick();
            frames += 1;
        }
        let seconds = frames as f32 / BYTE_SR;
        assert!(
            (seconds - 0.16).abs() < 0.01,
            "should run 0.1 + 0.05 + 0.01 seconds, ran {seconds}"
        );
        assert!(v.is_done(), "and then stop");
        // A finished voice keeps returning silence rather than restarting.
        assert_eq!(v.tick(), (0.0, 0.0));

        // A longer release runs longer.
        let mut p = beat("255");
        p.duration = 0.1;
        p.adsr.release = 0.2;
        let mut v = ByteBeatVoice::new(p, BYTE_SR);
        let mut longer = 0;
        while !v.is_done() && longer < 10_000 {
            v.tick();
            longer += 1;
        }
        assert!(longer > frames, "a longer release runs longer");
    }

    #[test]
    fn the_envelope_shapes_the_voice_over_its_hold() {
        // A constant expression, so the envelope is the only thing moving.
        let mut p = beat("255");
        p.duration = 0.5;
        p.adsr = Adsr {
            attack: 0.25,
            decay: 0.0,
            sustain: 1.0,
            release: 0.25,
        };
        let out = render(p, (BYTE_SR * 0.9) as usize);
        let at = |secs: f32| out[(secs * BYTE_SR) as usize];
        assert!(at(0.0).abs() < 0.02, "starts near silence: {}", at(0.0));
        assert!(at(0.1) < at(0.2), "the attack rises");
        assert!((at(0.4) - 0.2).abs() < 0.02, "holds at full: {}", at(0.4));
        assert!(at(0.6) > at(0.7), "the release falls");
    }

    #[test]
    fn what_the_javascript_oracle_cannot_be_asked() {
        // Unknown identifiers throw a ReferenceError in JS, so the golden
        // cannot carry them: upstream reaches a bare `Math` member, which is a
        // function object — NaN in arithmetic, 0 once the beat's `|0` lands.
        assert!(ev("nope", 0.0).is_nan());
        assert!(ev("$x", 0.0).is_nan());
        assert!(ev("_y", 0.0).is_nan());
        assert!(ev("a_$0", 0.0).is_nan());
        assert_eq!(ev("nope|0", 0.0), 0.0);
        // An identifier stops at the first character that cannot be in one, so
        // `t` is still `t` when something follows it.
        assert_eq!(ev("t+1", 5.0), 6.0);

        // Anything that does not parse is silence rather than half an
        // expression: trailing junk, an unterminated call, a bad literal.
        for bad in [
            "1+", "1 2", "(1+2", "sin(", "sin(1,", "1e", "1e+", "0x", ".", "+", "t)",
        ] {
            assert_eq!(ev(bad, 7.0), 0.0, "`{bad}` should not parse");
        }
        // ...while the same forms with what they were missing do parse.
        assert_eq!(ev("1e+2", 0.0), 100.0);
        assert_eq!(ev("0x10", 0.0), 16.0);
        // A hex literal that does not start at offset 0: `pos + 2` and
        // `pos * 2` agree at 0 and at 2, so only a third position separates
        // reading the digit after the `0x` from reading somewhere else.
        assert_eq!(ev("1+2+0x10", 0.0), 19.0);
        assert_eq!(ev("1+2+3+0xff", 0.0), 261.0);
        // An identifier whose *second* character is a digit is still an
        // identifier, not a number.
        assert!(ev("n1", 0.0).is_nan());
        assert!(ev("t1", 0.0).is_nan());
        assert_eq!(ev(".5", 0.0), 0.5);
        assert_eq!(ev("sin(0)", 0.0), 0.0);
    }
}
