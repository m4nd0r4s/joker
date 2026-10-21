//! `[dynamics]` of a simulation model (ref/03 §2.4.6): the expression
//! language of document 02 §10 with the three dynamics extensions
//! (`integrate`, `lag`, `delay` and named arguments), typed with the units
//! of `joker-units` (document 16 §4, ADR 0034) and evaluated in IEEE 754
//! `f64` without fused operations (ADR 0054).
//!
//! The quality functions (`good`, `quality`, `age`) have no meaning inside
//! the plant, whose values have no quality, and are `J0833`; `own` is
//! `J0306` as everywhere outside class methods.

use std::boxed::Box;
use std::collections::BTreeMap;
use std::string::String;
use std::vec::Vec;

use joker_units::{BaseDimension, Duration, ErrorKind, Unit};

use crate::codes::{J0108, J0109, J0155, J0301, J0302, J0303, J0305, J0306, J0832, J0833, J0836};
use crate::literal::{self, Literal};
use crate::model::{Enum, ModelError, Signal, SimDirection, SimType, err};

/// The longest window, `delay` or `lag` (ref/03 §2.1.5).
pub const MAX_WINDOW_NS: u64 = 15 * 60 * 1_000_000_000;

/// Binary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `==`.
    Eq,
    /// `!=`.
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
    /// `&&`.
    And,
    /// `||`.
    Or,
}

/// A stateless function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Func {
    /// `abs(x)`.
    Abs,
    /// `min(a, b, …)`.
    Min,
    /// `max(a, b, …)`.
    Max,
    /// `clamp(x, lo, hi)`.
    Clamp,
    /// `sqrt(x)`.
    Sqrt,
    /// `pow(x, n)`.
    Pow,
    /// `bit(x, n)`.
    Bit,
    /// `any(…)`.
    Any,
    /// `all(…)`.
    All,
    /// `count(…)`.
    Count,
    /// `sum(…)`.
    Sum,
    /// `maxs(…)`.
    Maxs,
    /// `mins(…)`.
    Mins,
}

/// A function of the language with memory, sampled once per step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemKind {
    /// `avg(x, w)` over `slots` samples.
    Avg {
        /// Samples.
        slots: usize,
    },
    /// `minw(x, w)`.
    MinW {
        /// Samples.
        slots: usize,
    },
    /// `maxw(x, w)`.
    MaxW {
        /// Samples.
        slots: usize,
    },
    /// `rate(x, w)`, per second.
    Rate {
        /// Samples.
        slots: usize,
    },
    /// `held(c, d)`.
    Held {
        /// Hold time in ns.
        ns: u64,
    },
    /// `rose(c)`.
    Rose,
    /// `fell(c)`.
    Fell,
    /// `hyst(x, lo, hi)`.
    Hyst,
}

/// The memory of one [`MemKind`] call.
#[derive(Clone, Debug, PartialEq)]
pub enum Mem {
    /// A window: `(t_ns, value)` samples, oldest first once full.
    Window {
        /// The ring.
        ring: Vec<(u64, f64)>,
        /// Next slot to write.
        head: usize,
    },
    /// When the condition of `held` became true.
    Held(Option<u64>),
    /// Previous value of `rose`/`fell`.
    Edge(bool),
    /// State of `hyst`.
    Hyst(bool),
}

impl Mem {
    /// The initial memory of `kind`.
    pub fn new(kind: MemKind) -> Self {
        match kind {
            MemKind::Avg { slots }
            | MemKind::MinW { slots }
            | MemKind::MaxW { slots }
            | MemKind::Rate { slots } => Self::Window {
                ring: Vec::with_capacity(slots),
                head: 0,
            },
            MemKind::Held { .. } => Self::Held(None),
            MemKind::Rose | MemKind::Fell => Self::Edge(false),
            MemKind::Hyst => Self::Hyst(false),
        }
    }
}

/// A compiled expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// A constant.
    Num(f64),
    /// The current value of a signal.
    Sig(usize),
    /// The current state of a stateful dynamics call.
    State(usize),
    /// `-x`.
    Neg(Box<Node>),
    /// `!x`.
    Not(Box<Node>),
    /// `a op b`.
    Bin(Op, Box<Node>, Box<Node>),
    /// A unit conversion: `x · scale + offset`.
    Lin(f64, f64, Box<Node>),
    /// `if c then a else b`; both branches are always evaluated.
    If(Box<Node>, Box<Node>, Box<Node>),
    /// A stateless function.
    Call(Func, Vec<Node>),
    /// A function with memory: the index of its [`Mem`], and its arguments.
    Mem(usize, MemKind, Vec<Node>),
}

/// What a stateful dynamics call does at each step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StateKind {
    /// `x ← clamp(x + d · step, min, max)`.
    Integrate {
        /// Lower bound.
        min: f64,
        /// Upper bound.
        max: f64,
    },
    /// `x ← x + (u − x) · step / tau`.
    Lag {
        /// Time constant in s.
        tau: f64,
    },
    /// The value of `u` this many steps ago (at least one).
    Delay {
        /// Steps.
        steps: usize,
    },
}

/// A stateful dynamics call.
#[derive(Clone, Debug, PartialEq)]
pub struct StateCall {
    /// The signal whose entry holds the call; its `init` is the initial state.
    pub owner: usize,
    /// Kind.
    pub kind: StateKind,
    /// The argument, evaluated at each step in the unit of the state.
    pub arg: Node,
}

/// One `[dynamics]` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The signal it defines.
    pub signal: usize,
    /// Its expression, in the signal's unit.
    pub expr: Node,
}

/// The compiled `[dynamics]`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Dynamics {
    /// Entries, in evaluation order.
    pub entries: Vec<Entry>,
    /// Stateful calls; all are updated together after each step.
    pub states: Vec<StateCall>,
    /// Functions with memory.
    pub mems: Vec<MemKind>,
}

/// What [`eval`] reads and updates.
#[derive(Debug)]
pub struct Env<'a> {
    /// Values of the signals.
    pub values: &'a [f64],
    /// States of the stateful calls.
    pub states: &'a [f64],
    /// Memory of the functions with memory.
    pub mems: &'a mut [Mem],
    /// Virtual time of the step, in ns since the model started.
    pub t_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sym {
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Comma,
    Assign,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num {
        value: f64,
        text: String,
        unit: Option<String>,
    },
    Ident(String),
    Sel(String),
    Sym(Sym),
}

const AGGREGATIONS: [&str; 6] = ["any", "all", "count", "sum", "maxs", "mins"];

fn unit_start(c: char) -> bool {
    c.is_alphabetic() || matches!(c, 'µ' | '°' | '%')
}

fn unit_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, 'µ' | '°' | '%' | '\u{3a9}' | '\u{2126}')
}

fn path_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

fn lex(src: &str, path: &str) -> Result<Vec<Tok>, ModelError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<Tok> = Vec::new();
    let mut agg_depths: Vec<usize> = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let at_arg_start = agg_depths.last() == Some(&depth)
            && matches!(out.last(), Some(Tok::Sym(Sym::LParen | Sym::Comma)));
        if c.is_whitespace() {
            i += 1;
        } else if at_arg_start && (path_char(c) || c == '*') && {
            let mut j = i;
            while j < chars.len() && (path_char(chars[j]) || chars[j] == '*') {
                j += 1;
            }
            chars[i..j].contains(&'*')
        } {
            let start = i;
            while i < chars.len() && (path_char(chars[i]) || chars[i] == '*') {
                i += 1;
            }
            out.push(Tok::Sel(chars[start..i].iter().collect()));
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || chars[i] == '_'
                    || ((chars[i] == 'e' || chars[i] == 'E')
                        && chars
                            .get(i + 1)
                            .is_some_and(|n| n.is_ascii_digit() || *n == '-' || *n == '+'))
                    || ((chars[i] == '-' || chars[i] == '+') && matches!(chars[i - 1], 'e' | 'E')))
            {
                i += 1;
            }
            let text: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            let value = text
                .parse::<f64>()
                .map_err(|_| err(J0833, path, std::format!("bad number `{text}`")))?;
            let mut j = i;
            if chars.get(j) == Some(&' ') {
                j += 1;
            }
            let mut unit = None;
            if chars.get(j).copied().is_some_and(unit_start) {
                let mut end = j;
                while end < chars.len() {
                    let ch = chars[end];
                    let joins = matches!(ch, '/' | '·')
                        && chars.get(end + 1).copied().is_some_and(unit_start);
                    if unit_char(ch) || joins {
                        end += 1;
                    } else {
                        break;
                    }
                }
                let word: String = chars[j..end].iter().collect();
                if word != "then" && word != "else" {
                    unit = Some(word);
                    i = end;
                }
            }
            out.push(Tok::Num { value, text, unit });
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && path_char(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let mut k = i;
            while chars.get(k).is_some_and(|c| c.is_whitespace()) {
                k += 1;
            }
            if AGGREGATIONS.contains(&word.as_str()) && chars.get(k) == Some(&'(') {
                agg_depths.push(depth + 1);
            }
            out.push(Tok::Ident(word));
        } else {
            let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
            let (sym, len) = match two.as_str() {
                "==" => (Sym::EqEq, 2),
                "!=" => (Sym::NotEq, 2),
                "<=" => (Sym::Le, 2),
                ">=" => (Sym::Ge, 2),
                "&&" => (Sym::AndAnd, 2),
                "||" => (Sym::OrOr, 2),
                _ => match c {
                    '+' => (Sym::Plus, 1),
                    '-' => (Sym::Minus, 1),
                    '*' => (Sym::Star, 1),
                    '/' => (Sym::Slash, 1),
                    '(' => (Sym::LParen, 1),
                    ')' => (Sym::RParen, 1),
                    ',' => (Sym::Comma, 1),
                    '=' => (Sym::Assign, 1),
                    '<' => (Sym::Lt, 1),
                    '>' => (Sym::Gt, 1),
                    '!' => (Sym::Bang, 1),
                    _ => return Err(err(J0833, path, std::format!("unexpected `{c}`"))),
                },
            };
            match sym {
                Sym::LParen => depth += 1,
                Sym::RParen => {
                    if agg_depths.last() == Some(&depth) {
                        agg_depths.pop();
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
            out.push(Tok::Sym(sym));
            i += len;
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq)]
enum Ast {
    Num {
        value: f64,
        text: String,
        unit: Option<String>,
    },
    Name(String),
    Sel(String),
    Neg(Box<Ast>),
    Not(Box<Ast>),
    Bin(Op, Box<Ast>, Box<Ast>),
    If(Box<Ast>, Box<Ast>, Box<Ast>),
    Call(String, Vec<(Option<String>, Ast)>),
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    path: &'a str,
}

const MAX_DEPTH: usize = 64;

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn eat(&mut self, s: Sym) -> bool {
        if self.peek() == Some(&Tok::Sym(s)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn keyword(&mut self, k: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Ident(w)) if w == k) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn fail(&self, m: &str) -> ModelError {
        err(J0833, self.path, m)
    }

    fn expr(&mut self, depth: usize) -> Result<Ast, ModelError> {
        if depth > MAX_DEPTH {
            return Err(self.fail("expression too deep"));
        }
        let mut lhs = self.and(depth)?;
        while self.eat(Sym::OrOr) {
            let rhs = self.and(depth)?;
            lhs = Ast::Bin(Op::Or, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn and(&mut self, depth: usize) -> Result<Ast, ModelError> {
        let mut lhs = self.not(depth)?;
        while self.eat(Sym::AndAnd) {
            let rhs = self.not(depth)?;
            lhs = Ast::Bin(Op::And, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn not(&mut self, depth: usize) -> Result<Ast, ModelError> {
        if self.eat(Sym::Bang) {
            if depth > MAX_DEPTH {
                return Err(self.fail("expression too deep"));
            }
            return Ok(Ast::Not(Box::new(self.not(depth + 1)?)));
        }
        self.cmp(depth)
    }

    fn cmp(&mut self, depth: usize) -> Result<Ast, ModelError> {
        let lhs = self.sum(depth)?;
        let op = match self.peek() {
            Some(Tok::Sym(Sym::EqEq)) => Op::Eq,
            Some(Tok::Sym(Sym::NotEq)) => Op::Ne,
            Some(Tok::Sym(Sym::Lt)) => Op::Lt,
            Some(Tok::Sym(Sym::Le)) => Op::Le,
            Some(Tok::Sym(Sym::Gt)) => Op::Gt,
            Some(Tok::Sym(Sym::Ge)) => Op::Ge,
            _ => return Ok(lhs),
        };
        self.pos += 1;
        let rhs = self.sum(depth)?;
        Ok(Ast::Bin(op, Box::new(lhs), Box::new(rhs)))
    }

    fn sum(&mut self, depth: usize) -> Result<Ast, ModelError> {
        let mut lhs = self.prod(depth)?;
        loop {
            let op = if self.eat(Sym::Plus) {
                Op::Add
            } else if self.eat(Sym::Minus) {
                Op::Sub
            } else {
                return Ok(lhs);
            };
            let rhs = self.prod(depth)?;
            lhs = Ast::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn prod(&mut self, depth: usize) -> Result<Ast, ModelError> {
        let mut lhs = self.unary(depth)?;
        loop {
            let op = if self.eat(Sym::Star) {
                Op::Mul
            } else if self.eat(Sym::Slash) {
                Op::Div
            } else {
                return Ok(lhs);
            };
            let rhs = self.unary(depth)?;
            lhs = Ast::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn unary(&mut self, depth: usize) -> Result<Ast, ModelError> {
        if self.eat(Sym::Minus) {
            if depth > MAX_DEPTH {
                return Err(self.fail("expression too deep"));
            }
            return Ok(Ast::Neg(Box::new(self.unary(depth + 1)?)));
        }
        self.atom(depth)
    }

    fn atom(&mut self, depth: usize) -> Result<Ast, ModelError> {
        match self.peek().cloned() {
            Some(Tok::Num { value, text, unit }) => {
                self.pos += 1;
                Ok(Ast::Num { value, text, unit })
            }
            Some(Tok::Sel(s)) => {
                self.pos += 1;
                Ok(Ast::Sel(s))
            }
            Some(Tok::Ident(name)) if name == "if" => {
                self.pos += 1;
                let c = self.expr(depth + 1)?;
                if !self.keyword("then") {
                    return Err(self.fail("expected `then`"));
                }
                let t = self.expr(depth + 1)?;
                if !self.keyword("else") {
                    return Err(self.fail("expected `else`"));
                }
                let e = self.expr(depth + 1)?;
                Ok(Ast::If(Box::new(c), Box::new(t), Box::new(e)))
            }
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                if !self.eat(Sym::LParen) {
                    return Ok(Ast::Name(name));
                }
                let mut args = Vec::new();
                if !self.eat(Sym::RParen) {
                    loop {
                        let named = match (self.peek().cloned(), self.toks.get(self.pos + 1)) {
                            (Some(Tok::Ident(k)), Some(Tok::Sym(Sym::Assign))) => {
                                self.pos += 2;
                                Some(k)
                            }
                            _ => None,
                        };
                        args.push((named, self.expr(depth + 1)?));
                        if self.eat(Sym::RParen) {
                            break;
                        }
                        if !self.eat(Sym::Comma) {
                            return Err(self.fail("expected `,` or `)`"));
                        }
                    }
                }
                Ok(Ast::Call(name, args))
            }
            Some(Tok::Sym(Sym::LParen)) => {
                self.pos += 1;
                let e = self.expr(depth + 1)?;
                if !self.eat(Sym::RParen) {
                    return Err(self.fail("expected `)`"));
                }
                Ok(e)
            }
            _ => Err(self.fail("expected a value")),
        }
    }
}

fn parse(src: &str, path: &str) -> Result<Ast, ModelError> {
    let mut p = Parser {
        toks: lex(src, path)?,
        pos: 0,
        path,
    };
    let e = p.expr(0)?;
    if p.pos != p.toks.len() {
        return Err(p.fail("unexpected text after the expression"));
    }
    Ok(e)
}

/// The type of a compiled sub-expression.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ty {
    Bool,
    /// A number; `None` is a bare number, which takes the unit around it.
    Num(Option<Unit>),
    Enum(usize),
}

/// Whether `name` matches the selector `pattern` (document 02 §9): `*`
/// inside one segment, `**` for one or more whole segments.
pub fn matches_selector(pattern: &str, name: &str) -> bool {
    fn seg(p: &str, s: &str) -> bool {
        match p.split_once('*') {
            None => p == s,
            Some((head, rest)) => {
                let Some(tail) = s.strip_prefix(head) else {
                    return false;
                };
                (0..=tail.len())
                    .filter(|i| tail.is_char_boundary(*i))
                    .any(|i| seg(rest, &tail[i..]))
            }
        }
    }
    fn segs(p: &[&str], s: &[&str]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(&"**"), _) => (1..=s.len()).any(|k| segs(&p[1..], &s[k..])),
            (Some(ph), Some(sh)) => seg(ph, sh) && segs(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let p: Vec<&str> = pattern.split('.').collect();
    let s: Vec<&str> = name.split('.').collect();
    segs(&p, &s)
}

fn si_unit(dim: [i8; 8]) -> Option<Unit> {
    const BASE: [&str; 8] = ["s", "m", "kg", "A", "K", "mol", "cd", "%"];
    let mut num = String::new();
    let mut den = String::new();
    for (i, e) in dim.iter().enumerate() {
        let part = match e.unsigned_abs() {
            0 => continue,
            1 => String::from(BASE[i]),
            n => std::format!("{}{n}", BASE[i]),
        };
        let side = if *e > 0 { &mut num } else { &mut den };
        if !side.is_empty() {
            side.push('*');
        }
        side.push_str(&part);
    }
    let text = match (num.is_empty(), den.is_empty()) {
        (true, true) => return Some(Unit::ONE),
        (false, true) => num,
        (true, false) => std::format!("1/{den}"),
        (false, false) => std::format!("{num}/{den}"),
    };
    Unit::parse(&text).ok()
}

struct Compiler<'a> {
    signals: &'a [Signal],
    enums: &'a [Enum],
    params: &'a BTreeMap<String, Literal>,
    step_ns: u64,
    owner: usize,
    path: &'a str,
    states: Vec<StateCall>,
    mems: Vec<MemKind>,
    deps: Vec<usize>,
}

impl Compiler<'_> {
    fn fail(&self, code: joker_hal::Code, m: impl Into<String>) -> ModelError {
        err(code, self.path, m)
    }

    fn units_err(&self, e: joker_units::Error, what: &str) -> ModelError {
        let code = match e.kind {
            ErrorKind::IncompatibleKind => J0108,
            ErrorKind::IncompatibleDimension => J0301,
            _ => J0109,
        };
        self.fail(code, std::format!("{what}: {}", e.kind.message()))
    }

    fn convert(&self, node: Node, from: Option<Unit>, to: &Unit) -> Result<Node, ModelError> {
        let Some(from) = from else {
            return Ok(node);
        };
        let c = from
            .conversion_to(to)
            .map_err(|e| self.units_err(e, &std::format!("`{from}` against `{to}`")))?;
        if c.is_identity() {
            return Ok(node);
        }
        Ok(match node {
            Node::Num(v) => Node::Num(c.apply(v)),
            other => Node::Lin(c.scale, c.offset, Box::new(other)),
        })
    }

    fn mismatch(&self, what: &str) -> ModelError {
        self.fail(J0833, std::format!("type mismatch: {what}"))
    }

    fn num(&mut self, ast: &Ast, what: &str) -> Result<(Node, Option<Unit>), ModelError> {
        match self.expr(ast)? {
            (n, Ty::Num(u)) => Ok((n, u)),
            _ => Err(self.mismatch(&std::format!("{what} must be a number"))),
        }
    }

    fn boolean(&mut self, ast: &Ast, what: &str) -> Result<Node, ModelError> {
        match self.expr(ast)? {
            (n, Ty::Bool) => Ok(n),
            _ => Err(self.mismatch(&std::format!("{what} must be a condition"))),
        }
    }

    fn duration(&mut self, ast: &Ast, what: &str) -> Result<u64, ModelError> {
        let bad = |c: &Self, m: &str| c.fail(J0833, std::format!("{what}: {m}"));
        match ast {
            Ast::Num {
                text,
                unit: Some(u),
                ..
            } => Duration::parse(&std::format!("{text}{u}"))
                .map(Duration::as_nanos)
                .map_err(|_| bad(self, "expected a duration such as `10s`")),
            Ast::Name(n) if n.starts_with("param.") => {
                let p = &n["param.".len()..];
                let lit = self
                    .params
                    .get(p)
                    .ok_or_else(|| self.fail(J0836, std::format!("unknown parameter `{p}`")))?;
                lit.duration_ns()
                    .map_err(|e| self.fail(J0836, std::format!("`{p}`: {}", e.message)))
            }
            other => {
                let (node, unit) = self.num(other, what)?;
                let unit = unit.ok_or_else(|| bad(self, "a duration needs a time unit"))?;
                let node = self.convert(node, Some(unit), &Unit::SECOND)?;
                let s = fold(&node).ok_or_else(|| bad(self, "expected a constant"))?;
                if !(s.is_finite() && s >= 0.0) {
                    return Err(bad(self, "expected a non-negative duration"));
                }
                Ok((s * 1e9).round() as u64)
            }
        }
    }

    fn name(&mut self, name: &str) -> Result<(Node, Ty), ModelError> {
        if name == "true" || name == "false" {
            return Ok((Node::Num(f64::from(u8::from(name == "true"))), Ty::Bool));
        }
        if let Ok(i) = self.signals.binary_search_by(|s| s.name.as_str().cmp(name)) {
            let s = &self.signals[i];
            let ty = match s.ty {
                SimType::Bool => Ty::Bool,
                SimType::Enum(e) => Ty::Enum(e),
                SimType::Bits(_) => Ty::Num(Some(Unit::ONE)),
                SimType::String | SimType::Bytes => {
                    return Err(self.mismatch(&std::format!(
                        "`{name}` is a string or bytes signal, which expressions cannot read"
                    )));
                }
                _ => Ty::Num(Some(s.unit)),
            };
            self.deps.push(i);
            return Ok((Node::Sig(i), ty));
        }
        if let Some(p) = name.strip_prefix("param.") {
            let lit = self
                .params
                .get(p)
                .ok_or_else(|| self.fail(J0836, std::format!("unknown parameter `{p}`")))?;
            return match lit {
                Literal::Bool(b) => Ok((Node::Num(f64::from(u8::from(*b))), Ty::Bool)),
                Literal::Number(n) => Ok((Node::Num(*n), Ty::Num(None))),
                Literal::Text(t) => {
                    let (v, u) = literal::quantity(t)
                        .map_err(|e| self.fail(J0836, std::format!("`{p}`: {}", e.message)))?;
                    Ok((Node::Num(v), Ty::Num(u)))
                }
            };
        }
        if let Some((e, variant)) = name.rsplit_once('.')
            && let Some(ix) = self.enums.iter().position(|x| x.name == e)
        {
            let v = self.enums[ix].value(variant).ok_or_else(|| {
                self.fail(J0303, std::format!("enum `{e}` has no variant `{variant}`"))
            })?;
            return Ok((Node::Num(v as f64), Ty::Enum(ix)));
        }
        Err(self.fail(
            J0302,
            std::format!("unknown signal `{name}` (the model's own signals only)"),
        ))
    }

    fn expr(&mut self, ast: &Ast) -> Result<(Node, Ty), ModelError> {
        match ast {
            Ast::Num { value, unit, .. } => {
                let u = unit
                    .as_deref()
                    .map(|u| {
                        Unit::parse(u).map_err(|_| {
                            self.fail(J0109, std::format!("unit `{u}` is not in the catalogue"))
                        })
                    })
                    .transpose()?;
                Ok((Node::Num(*value), Ty::Num(u)))
            }
            Ast::Name(n) => self.name(n),
            Ast::Sel(s) => {
                Err(self.fail(J0833, std::format!("selector `{s}` outside an aggregation")))
            }
            Ast::Neg(a) => match self.expr(a)? {
                (_, Ty::Num(Some(u))) if u.is_affine() => Err(self.fail(
                    J0301,
                    "the negative of an absolute temperature has no meaning",
                )),
                (n, t @ Ty::Num(_)) => Ok((Node::Neg(Box::new(n)), t)),
                _ => Err(self.mismatch("`-` takes a number")),
            },
            Ast::Not(a) => {
                let n = self.boolean(a, "the operand of `!`")?;
                Ok((Node::Not(Box::new(n)), Ty::Bool))
            }
            Ast::Bin(op, a, b) => self.bin(*op, a, b),
            Ast::If(c, t, e) => {
                let c = self.boolean(c, "the condition of `if`")?;
                let (t, tt) = self.expr(t)?;
                let (e, et) = self.expr(e)?;
                let (e, ty) = self.align(e, et, tt)?;
                Ok((Node::If(Box::new(c), Box::new(t), Box::new(e)), ty))
            }
            Ast::Call(name, args) => self.call(name, args),
        }
    }

    fn align(&self, node: Node, ty: Ty, to: Ty) -> Result<(Node, Ty), ModelError> {
        match (ty, to) {
            (Ty::Bool, Ty::Bool) => Ok((node, Ty::Bool)),
            (Ty::Enum(a), Ty::Enum(b)) if a == b => Ok((node, to)),
            (Ty::Num(None), Ty::Enum(_)) | (Ty::Enum(_), Ty::Num(None)) => Ok((node, to)),
            (Ty::Num(from), Ty::Num(Some(t))) => Ok((self.convert(node, from, &t)?, to)),
            (Ty::Num(from), Ty::Num(None)) => Ok((node, Ty::Num(from))),
            _ => Err(self.mismatch("both sides must have the same type")),
        }
    }

    fn bin(&mut self, op: Op, a: &Ast, b: &Ast) -> Result<(Node, Ty), ModelError> {
        let pack = |x: Node, y: Node| Node::Bin(op, Box::new(x), Box::new(y));
        match op {
            Op::And | Op::Or => {
                let x = self.boolean(a, "an operand of `&&`/`||`")?;
                let y = self.boolean(b, "an operand of `&&`/`||`")?;
                Ok((pack(x, y), Ty::Bool))
            }
            Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                let (x, xt) = self.expr(a)?;
                let (y, yt) = self.expr(b)?;
                let ordered = matches!(op, Op::Lt | Op::Le | Op::Gt | Op::Ge);
                let (x, y) = match (xt, yt) {
                    (Ty::Num(None), Ty::Num(Some(u))) => (x, self.convert(y, Some(u), &u)?),
                    (Ty::Num(_), Ty::Num(_)) => {
                        let (y, _) = self.align(y, yt, xt)?;
                        (x, y)
                    }
                    (Ty::Bool, Ty::Bool) | (Ty::Enum(_), _) | (_, Ty::Enum(_)) if !ordered => {
                        let (y, _) = self.align(y, yt, xt)?;
                        (x, y)
                    }
                    _ => return Err(self.mismatch("these operands cannot be compared")),
                };
                Ok((pack(x, y), Ty::Bool))
            }
            Op::Add | Op::Sub => {
                let (x, xu) = self.num(a, "an operand of `+`/`-`")?;
                let (y, yu) = self.num(b, "an operand of `+`/`-`")?;
                let add = op == Op::Add;
                let affine = |u: &Option<Unit>| u.is_some_and(|u| u.is_affine());
                let bad = || {
                    self.fail(
                        J0301,
                        "two absolute temperatures cannot be added, nor one subtracted from a difference",
                    )
                };
                Ok(match (xu, yu) {
                    (None, None) => (pack(x, y), Ty::Num(None)),
                    (Some(u), None) => (pack(x, y), Ty::Num(Some(u))),
                    (None, Some(u)) if !add && u.is_affine() => return Err(bad()),
                    (None, Some(u)) => (pack(x, y), Ty::Num(Some(u))),
                    (Some(p), Some(q)) => match (affine(&xu), affine(&yu)) {
                        (true, true) if add => return Err(bad()),
                        (true, true) => {
                            let y = self.convert(y, Some(q), &p)?;
                            (pack(x, y), Ty::Num(Some(p.difference())))
                        }
                        (true, false) => {
                            let y = self.convert(y, Some(q), &p.difference())?;
                            (pack(x, y), Ty::Num(Some(p)))
                        }
                        (false, true) if add => {
                            let x = self.convert(x, Some(p), &q.difference())?;
                            (pack(x, y), Ty::Num(Some(q)))
                        }
                        (false, true) => return Err(bad()),
                        (false, false) => {
                            let y = self.convert(y, Some(q), &p)?;
                            (pack(x, y), Ty::Num(Some(p)))
                        }
                    },
                })
            }
            Op::Mul | Op::Div => {
                let (x, xu) = self.num(a, "an operand of `*`/`/`")?;
                let (y, yu) = self.num(b, "an operand of `*`/`/`")?;
                let unit = match (xu, yu, op) {
                    (None, None, _) => None,
                    (Some(u), None, _) | (None, Some(u), Op::Mul) => Some(u),
                    (None, Some(u), _) => Some(
                        Unit::ONE
                            .checked_div(&u)
                            .map_err(|e| self.units_err(e, "quotient"))?,
                    ),
                    (Some(p), Some(q), Op::Mul) => Some(
                        p.checked_mul(&q)
                            .map_err(|e| self.units_err(e, "product"))?,
                    ),
                    (Some(p), Some(q), _) => Some(
                        p.checked_div(&q)
                            .map_err(|e| self.units_err(e, "quotient"))?,
                    ),
                };
                Ok((pack(x, y), Ty::Num(unit)))
            }
        }
    }

    fn positional<'b>(
        &self,
        name: &str,
        args: &'b [(Option<String>, Ast)],
        allowed: &[&str],
    ) -> Result<Vec<&'b Ast>, ModelError> {
        if let Some((Some(k), _)) = args
            .iter()
            .find(|(k, _)| k.as_ref().is_some_and(|k| !allowed.contains(&k.as_str())))
        {
            return Err(self.fail(J0833, std::format!("`{k}` is not an argument of `{name}`")));
        }
        Ok(args
            .iter()
            .filter(|(k, _)| k.is_none())
            .map(|(_, a)| a)
            .collect())
    }

    fn arity(&self, name: &str, got: usize, ok: bool) -> Result<(), ModelError> {
        if ok {
            Ok(())
        } else {
            Err(self.fail(
                J0833,
                std::format!("wrong number of arguments for `{name}` ({got})"),
            ))
        }
    }

    fn same_unit(
        &mut self,
        args: &[&Ast],
        what: &str,
    ) -> Result<(Vec<Node>, Option<Unit>), ModelError> {
        let compiled: Vec<(Node, Option<Unit>)> = args
            .iter()
            .map(|a| self.num(a, what))
            .collect::<Result<_, _>>()?;
        let target = compiled.iter().find_map(|(_, u)| *u);
        let mut out = Vec::with_capacity(compiled.len());
        for (n, u) in compiled {
            out.push(match &target {
                Some(t) => self.convert(n, u, t)?,
                None => n,
            });
        }
        Ok((out, target))
    }

    fn mem(&mut self, kind: MemKind, args: Vec<Node>) -> Node {
        self.mems.push(kind);
        Node::Mem(self.mems.len() - 1, kind, args)
    }

    fn window_slots(&mut self, ast: &Ast, name: &str) -> Result<usize, ModelError> {
        let ns = self.duration(ast, &std::format!("the window of `{name}`"))?;
        if ns == 0 {
            return Err(self.fail(J0833, std::format!("the window of `{name}` is zero")));
        }
        if ns > MAX_WINDOW_NS {
            return Err(self.fail(J0305, "windows are limited to 15 min"));
        }
        Ok(ns.div_ceil(self.step_ns) as usize)
    }

    fn aggregate(&mut self, func: Func, args: &[&Ast]) -> Result<(Node, Ty), ModelError> {
        let mut items: Vec<(Node, Ty)> = Vec::new();
        for a in args {
            if let Ast::Sel(s) = a {
                let mut found = 0;
                for i in 0..self.signals.len() {
                    if matches_selector(s, &self.signals[i].name) {
                        let name = self.signals[i].name.clone();
                        items.push(self.name(&name)?);
                        found += 1;
                    }
                }
                if found == 0 {
                    return Err(self.fail(J0155, std::format!("selector `{s}` matches nothing")));
                }
            } else {
                items.push(self.expr(a)?);
            }
        }
        if items.is_empty() {
            return Err(self.fail(J0833, "an aggregation needs arguments"));
        }
        let boolean = matches!(func, Func::Any | Func::All | Func::Count);
        if boolean {
            if items.iter().any(|(_, t)| *t != Ty::Bool) {
                return Err(self.mismatch("`any`, `all` and `count` take conditions"));
            }
            let ty = if func == Func::Count {
                Ty::Num(None)
            } else {
                Ty::Bool
            };
            return Ok((
                Node::Call(func, items.into_iter().map(|(n, _)| n).collect()),
                ty,
            ));
        }
        let target = items.iter().find_map(|(_, t)| match t {
            Ty::Num(u) => *u,
            _ => None,
        });
        if func == Func::Sum && target.is_some_and(|u| u.is_affine()) {
            return Err(self.fail(J0301, "the sum of absolute temperatures has no meaning"));
        }
        let mut nodes = Vec::new();
        for (n, t) in items {
            let Ty::Num(u) = t else {
                return Err(self.mismatch("`sum`, `maxs` and `mins` take numbers"));
            };
            nodes.push(match &target {
                Some(to) => self.convert(n, u, to)?,
                None => n,
            });
        }
        Ok((Node::Call(func, nodes), Ty::Num(target)))
    }

    fn call(
        &mut self,
        name: &str,
        args: &[(Option<String>, Ast)],
    ) -> Result<(Node, Ty), ModelError> {
        let allowed: &[&str] = if name == "integrate" {
            &["min", "max"]
        } else {
            &[]
        };
        let pos = self.positional(name, args, allowed)?;
        let n = pos.len();
        match name {
            "integrate" | "lag" | "delay" => self.stateful(name, args, &pos),
            "abs" => {
                self.arity(name, n, n == 1)?;
                let (x, u) = self.num(pos[0], "the argument of `abs`")?;
                if u.is_some_and(|u| u.is_affine()) {
                    return Err(self.fail(J0301, "`abs` of an absolute temperature"));
                }
                Ok((Node::Call(Func::Abs, std::vec![x]), Ty::Num(u)))
            }
            "min" | "max" | "clamp" => {
                let func = match name {
                    "min" => Func::Min,
                    "max" => Func::Max,
                    _ => Func::Clamp,
                };
                self.arity(name, n, if func == Func::Clamp { n == 3 } else { n >= 2 })?;
                let (nodes, u) = self.same_unit(&pos, "an argument of `min`/`max`/`clamp`")?;
                Ok((Node::Call(func, nodes), Ty::Num(u)))
            }
            "sqrt" => {
                self.arity(name, n, n == 1)?;
                let (x, u) = self.num(pos[0], "the argument of `sqrt`")?;
                let Some(u) = u.filter(|u| !u.is_dimensionless()) else {
                    return Ok((Node::Call(Func::Sqrt, std::vec![x]), Ty::Num(u)));
                };
                let exps = u.dimension().exponents();
                if u.is_affine() || exps.iter().any(|e| e % 2 != 0) {
                    return Err(self.fail(J0301, "`sqrt` needs even dimension exponents"));
                }
                let half = exps.map(|e| e / 2);
                let out = si_unit(half).ok_or_else(|| self.fail(J0301, "unit too long"))?;
                let si = Node::Lin(u.factor(), u.offset(), Box::new(x));
                Ok((Node::Call(Func::Sqrt, std::vec![si]), Ty::Num(Some(out))))
            }
            "pow" => {
                self.arity(name, n, n == 2)?;
                let (x, u) = self.num(pos[0], "the base of `pow`")?;
                let (e, eu) = self.num(pos[1], "the exponent of `pow`")?;
                if eu.is_some_and(|u| !u.is_dimensionless()) {
                    return Err(self.fail(J0301, "the exponent of `pow` is dimensionless"));
                }
                match u.filter(|u| !u.is_dimensionless()) {
                    None => Ok((Node::Call(Func::Pow, std::vec![x, e]), Ty::Num(u))),
                    Some(u) => {
                        let k = fold(&e)
                            .filter(|k| k.fract() == 0.0 && (-8.0..=8.0).contains(k))
                            .ok_or_else(|| {
                                self.fail(
                                    J0833,
                                    "`pow` of a dimensioned base needs a small constant integer exponent",
                                )
                            })?;
                        let out = u
                            .checked_powi(k as i8)
                            .map_err(|_| self.fail(J0301, "`pow` of an absolute temperature"))?;
                        Ok((Node::Call(Func::Pow, std::vec![x, e]), Ty::Num(Some(out))))
                    }
                }
            }
            "bit" => {
                self.arity(name, n, n == 2)?;
                let (x, _) = self.num(pos[0], "the value of `bit`")?;
                let (k, _) = self.num(pos[1], "the index of `bit`")?;
                let k = fold(&k)
                    .filter(|k| k.fract() == 0.0 && (0.0..=63.0).contains(k))
                    .ok_or_else(|| self.fail(J0833, "the index of `bit` is a constant in 0–63"))?;
                Ok((Node::Call(Func::Bit, std::vec![x, Node::Num(k)]), Ty::Bool))
            }
            "avg" | "minw" | "maxw" | "rate" => {
                self.arity(name, n, n == 2)?;
                let (x, u) = self.num(pos[0], &std::format!("the value of `{name}`"))?;
                let slots = self.window_slots(pos[1], name)?;
                let (kind, ty) = match name {
                    "avg" => (MemKind::Avg { slots }, Ty::Num(u)),
                    "minw" => (MemKind::MinW { slots }, Ty::Num(u)),
                    "maxw" => (MemKind::MaxW { slots }, Ty::Num(u)),
                    _ => {
                        let per = u
                            .unwrap_or(Unit::ONE)
                            .difference()
                            .checked_div(&Unit::SECOND)
                            .map_err(|e| self.units_err(e, "rate"))?;
                        (MemKind::Rate { slots }, Ty::Num(Some(per)))
                    }
                };
                Ok((self.mem(kind, std::vec![x]), ty))
            }
            "held" => {
                self.arity(name, n, n == 2)?;
                let c = self.boolean(pos[0], "the condition of `held`")?;
                let ns = self.duration(pos[1], "the time of `held`")?;
                if ns == 0 {
                    return Err(self.fail(J0833, "the time of `held` is zero"));
                }
                Ok((self.mem(MemKind::Held { ns }, std::vec![c]), Ty::Bool))
            }
            "rose" | "fell" => {
                self.arity(name, n, n == 1)?;
                let c = self.boolean(pos[0], &std::format!("the argument of `{name}`"))?;
                let kind = if name == "rose" {
                    MemKind::Rose
                } else {
                    MemKind::Fell
                };
                Ok((self.mem(kind, std::vec![c]), Ty::Bool))
            }
            "hyst" => {
                self.arity(name, n, n == 3)?;
                let (nodes, _) = self.same_unit(&pos, "an argument of `hyst`")?;
                Ok((self.mem(MemKind::Hyst, nodes), Ty::Bool))
            }
            "any" | "all" | "count" | "sum" | "maxs" | "mins" => {
                let func = match name {
                    "any" => Func::Any,
                    "all" => Func::All,
                    "count" => Func::Count,
                    "sum" => Func::Sum,
                    "maxs" => Func::Maxs,
                    _ => Func::Mins,
                };
                self.aggregate(func, &pos)
            }
            "good" | "quality" | "age" => Err(self.fail(
                J0833,
                std::format!(
                    "`{name}` reads the quality of acquired samples; plant values have none"
                ),
            )),
            "own" => Err(self.fail(J0306, "`own` exists only in class methods")),
            other => Err(self.fail(J0833, std::format!("unknown function `{other}`"))),
        }
    }

    fn stateful(
        &mut self,
        name: &str,
        args: &[(Option<String>, Ast)],
        pos: &[&Ast],
    ) -> Result<(Node, Ty), ModelError> {
        let n = pos.len();
        let owner = &self.signals[self.owner];
        let unit = owner.unit;
        let ty = match owner.ty {
            SimType::Bool => Ty::Bool,
            SimType::Enum(e) => Ty::Enum(e),
            _ => Ty::Num(Some(unit)),
        };
        let named = |key: &str| {
            args.iter()
                .find(|(k, _)| k.as_deref() == Some(key))
                .map(|(_, a)| a)
        };
        let saved = core::mem::take(&mut self.deps);
        let result = (|| -> Result<(StateKind, Node), ModelError> {
            Ok(match name {
                "integrate" => {
                    self.arity(name, n, n == 1)?;
                    if ty != Ty::Num(Some(unit)) {
                        return Err(self.mismatch("`integrate` defines a numeric signal"));
                    }
                    let per = unit
                        .difference()
                        .checked_div(&Unit::SECOND)
                        .map_err(|e| self.units_err(e, "integrate"))?;
                    let (d, du) = self.num(pos[0], "the derivative of `integrate`")?;
                    let d = match du {
                        None => d,
                        Some(du) => self.convert(d, Some(du), &per).map_err(|_| {
                            self.fail(
                                J0301,
                                std::format!(
                                    "`integrate` of `{du}` gives `{du}·s`, which is not `{unit}`"
                                ),
                            )
                        })?,
                    };
                    let mut bound = |key: &str, default: f64| -> Result<f64, ModelError> {
                        match named(key) {
                            None => Ok(default),
                            Some(a) => {
                                let (b, bu) = self.num(a, "a bound of `integrate`")?;
                                let b = self.convert(b, bu, &unit)?;
                                fold(&b).ok_or_else(|| {
                                    self.fail(J0833, "the bounds of `integrate` are constants")
                                })
                            }
                        }
                    };
                    let min = bound("min", f64::NEG_INFINITY)?;
                    let max = bound("max", f64::INFINITY)?;
                    (StateKind::Integrate { min, max }, d)
                }
                _ => {
                    self.arity(name, n, n == 2)?;
                    let t = self.duration(pos[1], &std::format!("the time of `{name}`"))?;
                    if t > MAX_WINDOW_NS {
                        return Err(self.fail(J0305, std::format!("`{name}` is limited to 15 min")));
                    }
                    let (u, ut) = self.expr(pos[0])?;
                    let (u, _) = self.align(u, ut, ty)?;
                    if name == "lag" {
                        if ty != Ty::Num(Some(unit)) {
                            return Err(self.mismatch("`lag` defines a numeric signal"));
                        }
                        if t == 0 {
                            return Err(self.fail(J0833, "`lag` needs a positive time constant"));
                        }
                        (
                            StateKind::Lag {
                                tau: t as f64 / 1e9,
                            },
                            u,
                        )
                    } else {
                        let steps = ((t as f64 / self.step_ns as f64).round() as usize).max(1);
                        (StateKind::Delay { steps }, u)
                    }
                }
            })
        })();
        self.deps = saved;
        let (kind, arg) = result?;
        self.states.push(StateCall {
            owner: self.owner,
            kind,
            arg,
        });
        Ok((Node::State(self.states.len() - 1), ty))
    }
}

fn fold(n: &Node) -> Option<f64> {
    Some(match n {
        Node::Num(v) => *v,
        Node::Neg(a) => -fold(a)?,
        Node::Lin(s, o, a) => fold(a)? * s + o,
        Node::Bin(op, a, b) => {
            let (a, b) = (fold(a)?, fold(b)?);
            match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div => a / b,
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// Compiles the `[dynamics]` entries `(signal, expression)`.
pub fn compile(
    signals: &[Signal],
    enums: &[Enum],
    params: &BTreeMap<String, Literal>,
    entries: &[(String, String)],
    step_ns: u64,
    has_init: &[bool],
) -> Result<Dynamics, ModelError> {
    let mut compiled: Vec<(Entry, Vec<usize>)> = Vec::new();
    let mut calls: Vec<StateCall> = Vec::new();
    let mut mems: Vec<MemKind> = Vec::new();
    let mut sorted: Vec<&(String, String)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, src) in sorted {
        let path = std::format!("dynamics.{name}");
        let i = signals
            .binary_search_by(|s| s.name.as_str().cmp(name))
            .map_err(|_| err(J0833, &path, "unknown signal"))?;
        let s = &signals[i];
        if s.direction == SimDirection::Output {
            return Err(err(
                J0832,
                &path,
                "an output is written by the core and has no dynamics",
            ));
        }
        if s.generator.is_some() {
            return Err(err(
                J0832,
                &path,
                "a signal has either a generator or dynamics",
            ));
        }
        if matches!(s.ty, SimType::String | SimType::Bytes) {
            return Err(err(
                J0833,
                &path,
                "string and bytes signals have no dynamics",
            ));
        }
        let ast = parse(src, &path)?;
        let mut c = Compiler {
            signals,
            enums,
            params,
            step_ns,
            owner: i,
            path: &path,
            states: Vec::new(),
            mems: Vec::new(),
            deps: Vec::new(),
        };
        let (node, ty) = c.expr(&ast)?;
        let target = match s.ty {
            SimType::Bool => Ty::Bool,
            SimType::Enum(e) => Ty::Enum(e),
            SimType::Bits(_) => Ty::Num(None),
            _ => Ty::Num(Some(s.unit)),
        };
        let (mut expr, _) = c.align(node, ty, target)?;
        if !c.states.is_empty() && !has_init.get(i).copied().unwrap_or(false) {
            return Err(err(
                J0833,
                &path,
                "a signal with integrate, lag or delay needs `init`",
            ));
        }
        let (sbase, mbase) = (calls.len(), mems.len());
        shift(&mut expr, sbase, mbase);
        for mut call in c.states {
            shift(&mut call.arg, sbase, mbase);
            calls.push(call);
        }
        mems.extend(c.mems);
        let mut deps = c.deps;
        deps.sort_unstable();
        deps.dedup();
        compiled.push((Entry { signal: i, expr }, deps));
    }
    let mut order: Vec<Entry> = Vec::new();
    let mut done: Vec<bool> = std::vec![false; compiled.len()];
    let defined: Vec<usize> = compiled.iter().map(|(e, _)| e.signal).collect();
    while order.len() < compiled.len() {
        let next = (0..compiled.len()).find(|&k| {
            !done[k]
                && compiled[k].1.iter().all(|d| {
                    defined
                        .iter()
                        .position(|s| s == d)
                        .is_none_or(|j| done[j] && j != k)
                })
        });
        let Some(k) = next else {
            let k = (0..compiled.len()).find(|&k| !done[k]).unwrap_or(0);
            let name = &signals[compiled[k].0.signal].name;
            return Err(err(
                J0833,
                &std::format!("dynamics.{name}"),
                "algebraic cycle: break it with integrate, lag or delay",
            ));
        };
        done[k] = true;
        order.push(compiled[k].0.clone());
    }
    Ok(Dynamics {
        entries: order,
        states: calls,
        mems,
    })
}

fn shift(n: &mut Node, states: usize, mems: usize) {
    match n {
        Node::State(i) => *i += states,
        Node::Mem(i, _, args) => {
            *i += mems;
            args.iter_mut().for_each(|a| shift(a, states, mems));
        }
        Node::Neg(a) | Node::Not(a) | Node::Lin(_, _, a) => shift(a, states, mems),
        Node::Bin(_, a, b) => {
            shift(a, states, mems);
            shift(b, states, mems);
        }
        Node::If(c, a, b) => {
            shift(c, states, mems);
            shift(a, states, mems);
            shift(b, states, mems);
        }
        Node::Call(_, args) => args.iter_mut().for_each(|a| shift(a, states, mems)),
        Node::Num(_) | Node::Sig(_) => {}
    }
}

fn truth(v: f64) -> bool {
    v != 0.0
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

/// Evaluates `n`, sampling every function with memory once.
pub fn eval(n: &Node, env: &mut Env<'_>) -> f64 {
    match n {
        Node::Num(v) => *v,
        Node::Sig(i) => env.values.get(*i).copied().unwrap_or(f64::NAN),
        Node::State(i) => env.states.get(*i).copied().unwrap_or(f64::NAN),
        Node::Neg(a) => -eval(a, env),
        Node::Not(a) => flag(!truth(eval(a, env))),
        Node::Lin(s, o, a) => eval(a, env) * s + o,
        Node::If(c, a, b) => {
            let c = eval(c, env);
            let a = eval(a, env);
            let b = eval(b, env);
            if truth(c) { a } else { b }
        }
        Node::Bin(op, a, b) => {
            let (a, b) = (eval(a, env), eval(b, env));
            match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div => a / b,
                Op::Eq => flag(a == b),
                Op::Ne => flag(a != b),
                Op::Lt => flag(a < b),
                Op::Le => flag(a <= b),
                Op::Gt => flag(a > b),
                Op::Ge => flag(a >= b),
                Op::And => flag(truth(a) && truth(b)),
                Op::Or => flag(truth(a) || truth(b)),
            }
        }
        Node::Call(f, args) => {
            let v: Vec<f64> = args.iter().map(|a| eval(a, env)).collect();
            call(*f, &v)
        }
        Node::Mem(i, kind, args) => {
            let v: Vec<f64> = args.iter().map(|a| eval(a, env)).collect();
            let t = env.t_ns;
            match env.mems.get_mut(*i) {
                Some(m) => sample(*kind, m, &v, t),
                None => f64::NAN,
            }
        }
    }
}

fn call(f: Func, v: &[f64]) -> f64 {
    let first = v.first().copied().unwrap_or(f64::NAN);
    match f {
        Func::Abs => first.abs(),
        Func::Min | Func::Mins => v.iter().copied().fold(f64::INFINITY, f64::min),
        Func::Max | Func::Maxs => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        Func::Clamp => match v {
            [x, lo, hi] => x.max(*lo).min(*hi),
            _ => f64::NAN,
        },
        Func::Sqrt => first.sqrt(),
        Func::Pow => libm::pow(first, v.get(1).copied().unwrap_or(f64::NAN)),
        Func::Bit => {
            let k = v.get(1).copied().unwrap_or(0.0) as u32;
            flag((first as i64 as u64) >> k & 1 == 1)
        }
        Func::Any => flag(v.iter().any(|x| truth(*x))),
        Func::All => flag(v.iter().all(|x| truth(*x))),
        Func::Count => v.iter().filter(|x| truth(**x)).count() as f64,
        Func::Sum => v.iter().sum(),
    }
}

fn sample(kind: MemKind, m: &mut Mem, v: &[f64], t: u64) -> f64 {
    let x = v.first().copied().unwrap_or(f64::NAN);
    match (kind, m) {
        (
            MemKind::Avg { slots }
            | MemKind::MinW { slots }
            | MemKind::MaxW { slots }
            | MemKind::Rate { slots },
            Mem::Window { ring, head },
        ) => {
            if ring.len() < slots {
                ring.push((t, x));
            } else if let Some(slot) = ring.get_mut(*head) {
                *slot = (t, x);
                *head = (*head + 1) % slots;
            }
            let ordered = || ring[*head..].iter().chain(ring[..*head].iter());
            match kind {
                MemKind::Avg { .. } => ordered().map(|(_, v)| *v).sum::<f64>() / ring.len() as f64,
                MemKind::MinW { .. } => ordered().map(|(_, v)| *v).fold(f64::INFINITY, f64::min),
                MemKind::MaxW { .. } => {
                    ordered().map(|(_, v)| *v).fold(f64::NEG_INFINITY, f64::max)
                }
                _ => {
                    let (Some(a), Some(b)) = (ordered().next(), ordered().last()) else {
                        return 0.0;
                    };
                    if b.0 <= a.0 {
                        0.0
                    } else {
                        (b.1 - a.1) / ((b.0 - a.0) as f64 / 1e9)
                    }
                }
            }
        }
        (MemKind::Held { ns }, Mem::Held(start)) => {
            if truth(x) {
                let s = *start.get_or_insert(t);
                flag(t - s >= ns)
            } else {
                *start = None;
                0.0
            }
        }
        (MemKind::Rose, Mem::Edge(prev)) => {
            let now = truth(x);
            let out = now && !*prev;
            *prev = now;
            flag(out)
        }
        (MemKind::Fell, Mem::Edge(prev)) => {
            let now = truth(x);
            let out = !now && *prev;
            *prev = now;
            flag(out)
        }
        (MemKind::Hyst, Mem::Hyst(state)) => {
            let (lo, hi) = (
                v.get(1).copied().unwrap_or(f64::NAN),
                v.get(2).copied().unwrap_or(f64::NAN),
            );
            if x > hi {
                *state = true;
            } else if x < lo {
                *state = false;
            }
            flag(*state)
        }
        _ => f64::NAN,
    }
}

/// True when `u` has the dimension of time (for durations given as
/// quantities).
pub fn is_time(u: &Unit) -> bool {
    u.dimension() == joker_units::Dimension::base(BaseDimension::Time)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::literal::unit;

    fn sig(name: &str, u: &str) -> Signal {
        Signal::new(name, SimType::F64, unit(u).unwrap())
    }

    fn entries(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (String::from(*a), String::from(*b)))
            .collect()
    }

    fn one(signals: &[Signal], src: &str, init: bool) -> Result<Dynamics, ModelError> {
        let name = signals[0].name.clone();
        let mut has = std::vec![false; signals.len()];
        has[0] = init;
        compile(
            signals,
            &[],
            &BTreeMap::new(),
            &entries(&[(&name, src)]),
            10_000_000,
            &has,
        )
    }

    fn run(d: &mut Dynamics, values: &[f64], t_ns: u64, mems: &mut Vec<Mem>) -> f64 {
        if mems.is_empty() {
            *mems = d.mems.iter().map(|k| Mem::new(*k)).collect();
        }
        let mut env = Env {
            values,
            states: &[],
            mems,
            t_ns,
        };
        eval(&d.entries[0].expr, &mut env)
    }

    #[test]
    fn lag_of_the_track_compiles_with_units() {
        let signals = [sig("cpu_temp", "°C"), sig("load", "1")];
        let d = one(&signals, "lag(45 °C + 30 K * load, 60s)", true).unwrap();
        assert_eq!(d.states.len(), 1);
        assert_eq!(d.states[0].kind, StateKind::Lag { tau: 60.0 });
        let mut env = Env {
            values: &[0.0, 1.0],
            states: &[],
            mems: &mut [],
            t_ns: 0,
        };
        assert_eq!(eval(&d.states[0].arg, &mut env), 75.0);
        assert_eq!(d.entries[0].expr, Node::State(0));
    }

    #[test]
    fn the_tank_integrates_flows_in_metres() {
        let signals = [sig("inflow", "m3/h"), sig("level", "m"), sig("pump", "%")];
        let level = &signals[1..];
        let mut all: Vec<Signal> = std::vec![level[0].clone()];
        all.extend([signals[0].clone(), signals[2].clone()]);
        all.sort_by(|a, b| a.name.cmp(&b.name));
        let d = compile(
            &all,
            &[],
            &BTreeMap::new(),
            &entries(&[(
                "level",
                "integrate((inflow - 0.05 m3/h/% * pump) / 3.2 m2, min = 0 m, max = 3 m)",
            )]),
            10_000_000,
            &[false, true, false],
        )
        .unwrap();
        assert_eq!(
            d.states[0].kind,
            StateKind::Integrate { min: 0.0, max: 3.0 }
        );
        let mut env = Env {
            values: &[3.6, 0.0, 0.0],
            states: &[],
            mems: &mut [],
            t_ns: 0,
        };
        let rate = eval(&d.states[0].arg, &mut env);
        assert!((rate - 3.6 / 3600.0 / 3.2).abs() < 1e-15, "{rate}");
        let bad = compile(
            &all,
            &[],
            &BTreeMap::new(),
            &entries(&[("level", "integrate(inflow - pump)")]),
            10_000_000,
            &[false, true, false],
        );
        assert_eq!(bad.unwrap_err().code, J0301);
    }

    #[test]
    fn order_follows_dependencies_and_cycles_are_refused() {
        let signals = [sig("a", "1"), sig("b", "1"), sig("c", "s")];
        let d = compile(
            &signals,
            &[],
            &BTreeMap::new(),
            &entries(&[("a", "b + 1"), ("b", "c * 2 / 1s"), ("c", "integrate(1)")]),
            10_000_000,
            &[false, false, true],
        )
        .unwrap();
        let order: Vec<usize> = d.entries.iter().map(|e| e.signal).collect();
        assert_eq!(order, [2, 1, 0]);
        let e = compile(
            &signals,
            &[],
            &BTreeMap::new(),
            &entries(&[("a", "b"), ("b", "a")]),
            10_000_000,
            &[false; 3],
        )
        .unwrap_err();
        assert_eq!(e.code, J0833);
        let ok = compile(
            &signals,
            &[],
            &BTreeMap::new(),
            &entries(&[("a", "lag(b, 1s)"), ("b", "a")]),
            10_000_000,
            &[true, false, false],
        );
        assert!(ok.is_ok());
    }

    #[test]
    fn the_whole_language_evaluates() {
        let signals = [sig("a", "1"), sig("b", "1"), sig("t", "°C")];
        let cases: [(&str, [f64; 3], f64); 14] = [
            ("if b > 2 then 1 else 0", [0.0, 3.0, 0.0], 1.0),
            ("if !(b > 2) || b == 7 then 1 else 0", [0.0, 7.0, 0.0], 1.0),
            (
                "clamp(-b * 2, 0, 10) + max(abs(b), 1, 4) - min(b, 2) / 2",
                [0.0, -3.0, 0.0],
                6.0 + 4.0 + 1.5,
            ),
            ("sqrt(b) + pow(b, 2)", [0.0, 4.0, 0.0], 18.0),
            ("if bit(b, 1) then 1 else 0", [0.0, 2.0, 0.0], 1.0),
            ("count(b > 1, b > 2, true)", [0.0, 2.0, 0.0], 2.0),
            ("sum(b, 2, b)", [0.0, 1.5, 0.0], 5.0),
            ("maxs(b*) + mins(b, 0.5)", [0.0, 1.0, 9.0], 1.5),
            (
                "if any(b > 1, false) && all(true, b > 0) then 1 else 0",
                [0.0, 2.0, 0.0],
                1.0,
            ),
            ("if t > 300 K then 1 else 0", [0.0, 0.0, 30.0], 1.0),
            ("(t - 20 °C) / 1 K", [0.0, 0.0, 25.0], 5.0),
            ("1 + 2 * 3 - 4 / 2", [0.0; 3], 5.0),
            ("2 - -1", [0.0; 3], 3.0),
            ("if 1 m < 500 mm then 1 else 2", [0.0; 3], 2.0),
        ];
        for (src, values, want) in cases {
            let mut d = one(&signals, src, false).unwrap_or_else(|e| panic!("{src}: {e}"));
            let got = run(&mut d, &values, 0, &mut Vec::new());
            assert!((got - want).abs() < 1e-12, "{src}: {got} != {want}");
        }
    }

    #[test]
    fn functions_with_memory_sample_each_step() {
        let signals = [sig("a", "1"), sig("b", "1")];
        let mut d = one(&signals, "avg(b, 30ms)", false).unwrap();
        assert_eq!(d.mems, [MemKind::Avg { slots: 3 }]);
        let mut mems = Vec::new();
        let got: Vec<f64> = (0..5)
            .map(|k| run(&mut d, &[0.0, k as f64], k * 10_000_000, &mut mems))
            .collect();
        assert_eq!(got, [0.0, 0.5, 1.0, 2.0, 3.0]);
        let per_s = [sig("a", "1/s"), sig("b", "1")];
        let mut d = one(&per_s, "rate(b, 1s)", false).unwrap();
        let mut mems = Vec::new();
        run(&mut d, &[0.0, 0.0], 0, &mut mems);
        assert_eq!(run(&mut d, &[0.0, 2.0], 500_000_000, &mut mems), 4.0);
        let mut d = one(&signals, "if held(b > 0, 20ms) then 1 else 0", false).unwrap();
        let mut mems = Vec::new();
        let got: Vec<f64> = [1.0, 1.0, 1.0, 0.0, 1.0]
            .iter()
            .enumerate()
            .map(|(k, b)| run(&mut d, &[0.0, *b], k as u64 * 10_000_000, &mut mems))
            .collect();
        assert_eq!(got, [0.0, 0.0, 1.0, 0.0, 0.0]);
        let mut d = one(
            &signals,
            "if rose(b > 0) then 1 else if fell(b > 0) then 2 else 0",
            false,
        )
        .unwrap();
        let mut mems = Vec::new();
        let got: Vec<f64> = [0.0, 1.0, 1.0, 0.0]
            .iter()
            .map(|b| run(&mut d, &[0.0, *b], 0, &mut mems))
            .collect();
        assert_eq!(got, [0.0, 1.0, 0.0, 2.0]);
        let mut d = one(&signals, "if hyst(b, 1, 2) then 1 else 0", false).unwrap();
        let mut mems = Vec::new();
        let got: Vec<f64> = [1.5, 2.5, 1.5, 0.5]
            .iter()
            .map(|b| run(&mut d, &[0.0, *b], 0, &mut mems))
            .collect();
        assert_eq!(got, [0.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn selectors_match_segments() {
        assert!(matches_selector("inv*.ac.p", "inv1.ac.p"));
        assert!(!matches_selector("inv*.ac.p", "inv1.dc.p"));
        assert!(matches_selector("meter.**", "meter.l1.v"));
        assert!(!matches_selector("meter.**", "meter"));
        assert!(matches_selector("*", "a"));
        assert!(!matches_selector("*", "a.b"));
    }

    #[test]
    fn errors_carry_their_codes() {
        let signals = [sig("a", "1"), sig("b", "1"), sig("t", "°C")];
        let cases = [
            ("sqrt(b", J0833),
            ("lag(b)", J0833),
            ("b +", J0833),
            ("b ^ 2", J0833),
            ("integrate(b, top = 1)", J0833),
            ("nope", J0302),
            ("3 furlongs", J0109),
            ("b + 3 m", J0301),
            ("t + t", J0301),
            ("-t", J0301),
            ("avg(b, 16min)", J0305),
            ("delay(b, 20min)", J0305),
            ("good(b)", J0833),
            ("own(b)", J0306),
            ("frobnicate(b)", J0833),
            ("sum(z*)", J0155),
            ("if b then 1 else 0", J0833),
            ("b && true", J0833),
            ("lag(b, 0s)", J0833),
        ];
        for (src, code) in cases {
            let r = one(&signals, src, true);
            assert_eq!(r.map(|_| ()).unwrap_err().code, code, "{src}");
        }
        assert_eq!(
            one(&signals, "integrate(1)", false).unwrap_err().code,
            J0833
        );
        let mut p = BTreeMap::new();
        p.insert(String::from("k"), Literal::Number(2.0));
        p.insert(String::from("tau"), Literal::Text(String::from("10m")));
        let d = compile(
            &signals,
            &[],
            &p,
            &entries(&[("a", "lag(b * param.k, param.tau)")]),
            10_000_000,
            &[true, false, false],
        )
        .unwrap();
        assert_eq!(d.states[0].kind, StateKind::Lag { tau: 600.0 });
    }
}
