//! `[dynamics]` of a simulation model (ref/03 §2.4.6): parsing, the order of
//! evaluation and the stateful functions `integrate`, `lag` and `delay`.
//!
//! M0 takes the subset of the expression language the track's models use
//! (ADR 0051): numbers with units, the model's signals, `param.<name>`,
//! `+ - * /`, unary minus, parentheses, `abs`, `min`, `max`, `clamp` and the
//! three stateful functions. Everything else is `J0833`. Arithmetic is IEEE
//! 754 `f64` without fused operations.

use std::boxed::Box;
use std::collections::BTreeMap;
use std::string::String;
use std::vec::Vec;

use crate::literal::{KNOWN_UNITS, Literal, Quantity, convert};
use crate::model::{J0832, J0833, J0836, ModelError, Signal, SimDirection, err};

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
}

/// A stateless function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Func {
    /// `abs(x)`.
    Abs,
    /// `min(a, b)`.
    Min,
    /// `max(a, b)`.
    Max,
    /// `clamp(x, lo, hi)`.
    Clamp,
}

/// A compiled expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// A constant.
    Num(f64),
    /// The current value of a signal.
    Sig(usize),
    /// `-x`.
    Neg(Box<Node>),
    /// `a op b`.
    Bin(Op, Box<Node>, Box<Node>),
    /// A stateless function.
    Func(Func, Vec<Node>),
    /// The current state of a stateful call.
    State(usize),
}

/// What a stateful call does at each step.
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
    /// The value of `u` this many steps ago.
    Delay {
        /// Steps.
        steps: usize,
    },
}

/// A stateful call.
#[derive(Clone, Debug, PartialEq)]
pub struct StateCall {
    /// The signal whose entry holds the call; its `init` is the initial state.
    pub owner: usize,
    /// Kind.
    pub kind: StateKind,
    /// The argument evaluated at each step.
    pub arg: Node,
}

/// One `[dynamics]` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The signal it defines.
    pub signal: usize,
    /// Its expression.
    pub expr: Node,
}

/// The compiled `[dynamics]`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Dynamics {
    /// Entries, in evaluation order.
    pub entries: Vec<Entry>,
    /// Stateful calls, in signal-path order (their update order).
    pub states: Vec<StateCall>,
}

/// The longest `delay` (ref/03 §2.4.6).
pub const MAX_DELAY_NS: u64 = 15 * 60 * 1_000_000_000;

#[derive(Clone, Debug, PartialEq)]
enum Ast {
    Num(f64, Option<String>),
    Name(String),
    Neg(Box<Ast>),
    Bin(Op, Box<Ast>, Box<Ast>),
    Call(String, Vec<(Option<String>, Ast)>),
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Unit(String),
    Sym(char),
}

fn lex(src: &str, path: &str) -> Result<Vec<Tok>, ModelError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
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
            let n = text
                .parse()
                .map_err(|_| err(J0833, path, std::format!("bad number `{text}`")))?;
            out.push(Tok::Num(n));
            let mut j = i;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let k = j;
            let mut end = j;
            while end < chars.len()
                && (chars[end].is_alphanumeric() || chars[end] == '°' || chars[end] == '%')
            {
                end += 1;
            }
            let word: String = chars[k..end].iter().collect();
            if KNOWN_UNITS.contains(&word.as_str())
                && !chars.get(end).is_some_and(|c| *c == '_' || *c == '.')
            {
                out.push(Tok::Unit(word));
                i = end;
            }
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if "+-*/(),=".contains(c) {
            out.push(Tok::Sym(c));
            i += 1;
        } else {
            return Err(err(J0833, path, std::format!("unexpected `{c}`")));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    path: &'a str,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Tok::Sym(c)) {
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
        if depth > 64 {
            return Err(self.fail("expression too deep"));
        }
        let mut lhs = self.term(depth)?;
        loop {
            let op = if self.eat('+') {
                Op::Add
            } else if self.eat('-') {
                Op::Sub
            } else {
                return Ok(lhs);
            };
            let rhs = self.term(depth)?;
            lhs = Ast::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn term(&mut self, depth: usize) -> Result<Ast, ModelError> {
        let mut lhs = self.unary(depth)?;
        loop {
            let op = if self.eat('*') {
                Op::Mul
            } else if self.eat('/') {
                Op::Div
            } else {
                return Ok(lhs);
            };
            let rhs = self.unary(depth)?;
            lhs = Ast::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn unary(&mut self, depth: usize) -> Result<Ast, ModelError> {
        if self.eat('-') {
            return Ok(Ast::Neg(Box::new(self.unary(depth + 1)?)));
        }
        self.primary(depth)
    }

    fn primary(&mut self, depth: usize) -> Result<Ast, ModelError> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.pos += 1;
                if let Some(Tok::Unit(u)) = self.peek().cloned() {
                    self.pos += 1;
                    return Ok(Ast::Num(n, Some(u)));
                }
                Ok(Ast::Num(n, None))
            }
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                if !self.eat('(') {
                    return Ok(Ast::Name(name));
                }
                let mut args = Vec::new();
                if !self.eat(')') {
                    loop {
                        let named = match (self.peek().cloned(), self.toks.get(self.pos + 1)) {
                            (Some(Tok::Ident(k)), Some(Tok::Sym('='))) => {
                                self.pos += 2;
                                Some(k)
                            }
                            _ => None,
                        };
                        args.push((named, self.expr(depth + 1)?));
                        if self.eat(')') {
                            break;
                        }
                        if !self.eat(',') {
                            return Err(self.fail("expected `,` or `)`"));
                        }
                    }
                }
                Ok(Ast::Call(name, args))
            }
            Some(Tok::Sym('(')) => {
                self.pos += 1;
                let e = self.expr(depth + 1)?;
                if !self.eat(')') {
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

struct Compiler<'a> {
    signals: &'a [Signal],
    params: &'a BTreeMap<String, Literal>,
    step_s: f64,
    owner: usize,
    path: &'a str,
    states: Vec<StateCall>,
    deps: Vec<usize>,
}

impl Compiler<'_> {
    fn constant(&mut self, ast: &Ast, unit: &str) -> Result<f64, ModelError> {
        let node = self.node(ast, unit, false)?;
        fold(&node).ok_or_else(|| err(J0833, self.path, "expected a constant"))
    }

    fn node(&mut self, ast: &Ast, unit: &str, stateful_ok: bool) -> Result<Node, ModelError> {
        Ok(match ast {
            Ast::Num(n, u) => {
                let q = Quantity {
                    value: *n,
                    unit: u.as_deref(),
                };
                Node::Num(convert(&q, unit).map_err(|e| err(J0833, self.path, e.0))?)
            }
            Ast::Name(name) => {
                if let Some(p) = name.strip_prefix("param.") {
                    let lit = self.params.get(p).ok_or_else(|| {
                        err(J0836, self.path, std::format!("unknown parameter `{p}`"))
                    })?;
                    Node::Num(lit.number(unit).map_err(|e| err(J0836, self.path, e.0))?)
                } else {
                    let i = self
                        .signals
                        .binary_search_by(|s| s.name.as_str().cmp(name))
                        .map_err(|_| {
                            err(J0833, self.path, std::format!("unknown signal `{name}`"))
                        })?;
                    self.deps.push(i);
                    Node::Sig(i)
                }
            }
            Ast::Neg(a) => Node::Neg(Box::new(self.node(a, unit, stateful_ok)?)),
            Ast::Bin(op, a, b) => {
                let (ua, ub) = match op {
                    Op::Add | Op::Sub => (unit, unit),
                    Op::Mul | Op::Div => (unit, "1"),
                };
                let a = self.node(a, ua, stateful_ok)?;
                let b = self.node(b, ub, stateful_ok)?;
                Node::Bin(*op, Box::new(a), Box::new(b))
            }
            Ast::Call(name, args) => self.call(name, args, unit, stateful_ok)?,
        })
    }

    fn call(
        &mut self,
        name: &str,
        args: &[(Option<String>, Ast)],
        unit: &str,
        stateful_ok: bool,
    ) -> Result<Node, ModelError> {
        let positional: Vec<&Ast> = args
            .iter()
            .filter(|(k, _)| k.is_none())
            .map(|(_, a)| a)
            .collect();
        let named = |key: &str| {
            args.iter()
                .find(|(k, _)| k.as_deref() == Some(key))
                .map(|(_, a)| a)
        };
        if let Some((Some(k), _)) = args.iter().find(|(k, _)| {
            k.as_ref()
                .is_some_and(|k| !(name == "integrate" && (k == "min" || k == "max")))
        }) {
            return Err(err(
                J0833,
                self.path,
                std::format!("`{k}` is not an argument of `{name}`"),
            ));
        }
        let stateful = matches!(name, "integrate" | "lag" | "delay");
        if stateful && !stateful_ok {
            return Err(err(
                J0833,
                self.path,
                std::format!("`{name}` cannot be nested in another stateful call"),
            ));
        }
        let arity = match name {
            "abs" | "integrate" => 1,
            "min" | "max" | "lag" | "delay" => 2,
            "clamp" => 3,
            other => {
                return Err(err(
                    J0833,
                    self.path,
                    std::format!("`{other}` is not supported by the M0 simulator"),
                ));
            }
        };
        if positional.len() != arity {
            return Err(err(
                J0833,
                self.path,
                std::format!("`{name}` takes {arity} argument(s)"),
            ));
        }
        let kind = match name {
            "integrate" => StateKind::Integrate {
                min: named("min")
                    .map(|a| self.constant(a, unit))
                    .transpose()?
                    .unwrap_or(f64::NEG_INFINITY),
                max: named("max")
                    .map(|a| self.constant(a, unit))
                    .transpose()?
                    .unwrap_or(f64::INFINITY),
            },
            "lag" => {
                let tau = self.constant(positional[1], "s")?;
                if tau <= 0.0 {
                    return Err(err(
                        J0833,
                        self.path,
                        "`lag` needs a positive time constant",
                    ));
                }
                StateKind::Lag { tau }
            }
            "delay" => {
                let t = self.constant(positional[1], "s")?;
                if !(0.0..=MAX_DELAY_NS as f64 / 1e9).contains(&t) {
                    return Err(err(J0305, self.path, "`delay` is limited to 15 min"));
                }
                StateKind::Delay {
                    steps: (t / self.step_s).round() as usize,
                }
            }
            _ => {
                let func = match name {
                    "abs" => Func::Abs,
                    "min" => Func::Min,
                    "max" => Func::Max,
                    _ => Func::Clamp,
                };
                let args = positional
                    .iter()
                    .map(|a| self.node(a, unit, stateful_ok))
                    .collect::<Result<_, _>>()?;
                return Ok(Node::Func(func, args));
            }
        };
        let saved = core::mem::take(&mut self.deps);
        let arg_unit = if name == "integrate" { "1" } else { unit };
        let arg = self.node(positional[0], arg_unit, false)?;
        self.deps = saved;
        self.states.push(StateCall {
            owner: self.owner,
            kind,
            arg,
        });
        Ok(Node::State(self.states.len() - 1))
    }
}

/// Window or delay above 15 min.
const J0305: joker_hal::Code = joker_hal::code!("J0305");

fn fold(n: &Node) -> Option<f64> {
    Some(match n {
        Node::Num(v) => *v,
        Node::Neg(a) => -fold(a)?,
        Node::Bin(op, a, b) => {
            let (a, b) = (fold(a)?, fold(b)?);
            match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div => a / b,
            }
        }
        _ => return None,
    })
}

/// Compiles the `[dynamics]` entries `(signal, expression)`.
pub fn compile(
    signals: &[Signal],
    params: &BTreeMap<String, Literal>,
    entries: &[(String, String)],
    step_ns: u64,
    has_init: &[bool],
) -> Result<Dynamics, ModelError> {
    let mut compiled: Vec<(Entry, Vec<usize>)> = Vec::new();
    let mut calls: Vec<StateCall> = Vec::new();
    let mut sorted: Vec<&(String, String)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, src) in sorted {
        let path = std::format!("dynamics.{name}");
        let i = signals
            .binary_search_by(|s| s.name.as_str().cmp(name))
            .map_err(|_| err(J0833, &path, "unknown signal"))?;
        let s = &signals[i];
        if s.direction != SimDirection::Input {
            return Err(err(J0833, &path, "only `input` signals have dynamics"));
        }
        if s.generator.is_some() {
            return Err(err(
                J0832,
                &path,
                "a signal has either a generator or dynamics",
            ));
        }
        let ast = parse(src, &path)?;
        let mut c = Compiler {
            signals,
            params,
            step_s: step_ns as f64 / 1e9,
            owner: i,
            path: &path,
            states: Vec::new(),
            deps: Vec::new(),
        };
        let mut expr = c.node(&ast, &s.unit, true)?;
        if !c.states.is_empty() && !has_init.get(i).copied().unwrap_or(false) {
            return Err(err(
                J0833,
                &path,
                "a signal with integrate, lag or delay needs `init`",
            ));
        }
        let base = calls.len();
        shift_states(&mut expr, base);
        for mut call in c.states {
            shift_states(&mut call.arg, base);
            calls.push(call);
        }
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
                "algebraic cycle",
            ));
        };
        done[k] = true;
        order.push(compiled[k].0.clone());
    }
    Ok(Dynamics {
        entries: order,
        states: calls,
    })
}

fn shift_states(n: &mut Node, base: usize) {
    match n {
        Node::State(i) => *i += base,
        Node::Neg(a) => shift_states(a, base),
        Node::Bin(_, a, b) => {
            shift_states(a, base);
            shift_states(b, base);
        }
        Node::Func(_, args) => args.iter_mut().for_each(|a| shift_states(a, base)),
        Node::Num(_) | Node::Sig(_) => {}
    }
}

/// Evaluates `n` with signal `values` and call `states`.
pub fn eval(n: &Node, values: &[f64], states: &[f64]) -> f64 {
    match n {
        Node::Num(v) => *v,
        Node::Sig(i) => values.get(*i).copied().unwrap_or(f64::NAN),
        Node::State(i) => states.get(*i).copied().unwrap_or(f64::NAN),
        Node::Neg(a) => -eval(a, values, states),
        Node::Bin(op, a, b) => {
            let (a, b) = (eval(a, values, states), eval(b, values, states));
            match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div => a / b,
            }
        }
        Node::Func(f, args) => {
            let v: Vec<f64> = args.iter().map(|a| eval(a, values, states)).collect();
            match (f, v.as_slice()) {
                (Func::Abs, [x]) => x.abs(),
                (Func::Min, [a, b]) => a.min(*b),
                (Func::Max, [a, b]) => a.max(*b),
                (Func::Clamp, [x, lo, hi]) => x.max(*lo).min(*hi),
                _ => f64::NAN,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SimType;

    fn sig(name: &str, unit: &str) -> Signal {
        Signal {
            name: String::from(name),
            ty: SimType::F64,
            unit: String::from(unit),
            direction: SimDirection::Input,
            init: 0.0,
            generator: None,
            noise: None,
            resolution: None,
            panel: None,
        }
    }

    fn entries(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (String::from(*a), String::from(*b)))
            .collect()
    }

    #[test]
    fn lag_of_the_track_compiles_with_units() {
        let signals = [sig("cpu_temp", "°C"), sig("load", "1")];
        let d = compile(
            &signals,
            &BTreeMap::new(),
            &entries(&[("cpu_temp", "lag(45 °C + 30 K * load, 60s)")]),
            10_000_000,
            &[true, false],
        )
        .unwrap();
        assert_eq!(d.states.len(), 1);
        assert_eq!(d.states[0].kind, StateKind::Lag { tau: 60.0 });
        assert_eq!(eval(&d.states[0].arg, &[0.0, 1.0], &[]), 75.0);
        assert_eq!(d.entries[0].expr, Node::State(0));
    }

    #[test]
    fn order_follows_dependencies_and_cycles_are_refused() {
        let signals = [sig("a", "1"), sig("b", "1"), sig("c", "1")];
        let d = compile(
            &signals,
            &BTreeMap::new(),
            &entries(&[("a", "b + 1"), ("b", "c * 2"), ("c", "integrate(1)")]),
            10_000_000,
            &[false, false, true],
        )
        .unwrap();
        let order: Vec<usize> = d.entries.iter().map(|e| e.signal).collect();
        assert_eq!(order, [2, 1, 0]);
        let e = compile(
            &signals,
            &BTreeMap::new(),
            &entries(&[("a", "b"), ("b", "a")]),
            10_000_000,
            &[false; 3],
        )
        .unwrap_err();
        assert_eq!(e.code, J0833);
        let ok = compile(
            &signals,
            &BTreeMap::new(),
            &entries(&[("a", "lag(b, 1s)"), ("b", "a")]),
            10_000_000,
            &[true, false, false],
        );
        assert!(ok.is_ok());
    }

    #[test]
    fn unsupported_forms_are_j0833() {
        let signals = [sig("a", "1"), sig("b", "1")];
        for src in [
            "sqrt(b)",
            "lag(b)",
            "b +",
            "lag(lag(b, 1s), 1s)",
            "b ^ 2",
            "integrate(b, top = 1)",
            "nope",
            "3 m2",
        ] {
            let r = compile(
                &signals,
                &BTreeMap::new(),
                &entries(&[("a", src)]),
                10_000_000,
                &[true, false],
            );
            assert_eq!(r.map(|_| ()).unwrap_err().code, J0833, "{src}");
        }
        let r = compile(
            &signals,
            &BTreeMap::new(),
            &entries(&[("a", "integrate(1)")]),
            10_000_000,
            &[false, false],
        );
        assert_eq!(r.unwrap_err().code, J0833);
        let mut p = BTreeMap::new();
        p.insert(String::from("k"), Literal::Number(2.0));
        let d = compile(
            &signals,
            &p,
            &entries(&[(
                "a",
                "clamp(-b * param.k, 0, 10) + max(abs(b), 1) - min(b, 2) / 2",
            )]),
            10_000_000,
            &[false, false],
        )
        .unwrap();
        assert_eq!(eval(&d.entries[0].expr, &[0.0, -3.0], &[]), 6.0 + 3.0 + 1.5);
    }
}
