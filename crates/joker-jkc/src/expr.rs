//! Expressions inside a class: the canonical text that the contract keeps
//! (ADR 0059), the environment `joker-expr` checks them against, and the
//! member paths they read.

use std::collections::BTreeMap;

use joker_expr::ast::{Ast, Node, NodeId};
use joker_expr::lexer::{Lexer, TokenKind};
use joker_expr::{EnumId, Env, Func, SignalInfo};

/// The canonical text of an expression: its tokens, with one space between
/// operands and operators and none inside parentheses, before commas or
/// after a unary operator, and a literal written as `<number> <unit>`.
/// Two spellings that differ only in layout have the same canonical text.
pub fn canonical(src: &str) -> Result<String, joker_expr::Diagnostic> {
    let mut lexer = Lexer::new(src);
    let mut out = String::new();
    let mut prev: Option<TokenKind> = None;
    let mut prev_unary = false;
    loop {
        let token = lexer.next_token()?;
        if token.kind == TokenKind::Eof {
            return Ok(out);
        }
        let text = src
            .get(token.span.start as usize..token.span.end as usize)
            .unwrap_or("");
        let glue = match (prev, token.kind) {
            (None, _) => true,
            (Some(TokenKind::LParen), _) => true,
            (_, TokenKind::RParen | TokenKind::Comma) => true,
            (Some(TokenKind::Path), TokenKind::LParen) => true,
            _ => prev_unary,
        };
        if !glue {
            out.push(' ');
        }
        match token.kind {
            TokenKind::Literal { unit_start } => {
                let split = (unit_start.saturating_sub(token.span.start)) as usize;
                let number = text.get(..split).unwrap_or(text).trim_end();
                let unit = text.get(split..).unwrap_or("").trim();
                out.push_str(number);
                if !unit.is_empty() {
                    out.push(' ');
                    out.push_str(unit);
                }
            }
            _ => out.push_str(text),
        }
        prev_unary = match token.kind {
            TokenKind::Bang => true,
            TokenKind::Minus => is_operand_start(prev),
            _ => false,
        };
        prev = Some(token.kind);
    }
}

fn is_operand_start(prev: Option<TokenKind>) -> bool {
    !matches!(
        prev,
        Some(
            TokenKind::Literal { .. }
                | TokenKind::Path
                | TokenKind::Selector
                | TokenKind::EnumVariant
                | TokenKind::True
                | TokenKind::False
                | TokenKind::RParen
        )
    )
}

/// The names an expression of a class can read, as `joker-expr` sees them.
#[derive(Debug, Default)]
pub struct ClassEnv {
    signals: BTreeMap<String, SignalInfo>,
    enums: BTreeMap<String, (EnumId, BTreeMap<String, i64>)>,
}

impl ClassEnv {
    /// Adds a readable name.
    pub fn signal(&mut self, path: &str, info: SignalInfo) {
        self.signals.insert(path.to_owned(), info);
    }

    /// Adds an enum under the name the file uses for it.
    pub fn enumeration(&mut self, name: &str, id: EnumId, variants: BTreeMap<String, i64>) {
        self.enums.insert(name.to_owned(), (id, variants));
    }

    /// Whether a name is readable.
    pub fn has(&self, path: &str) -> bool {
        self.signals.contains_key(path)
    }
}

impl Env for ClassEnv {
    fn signal(&self, path: &str) -> Option<SignalInfo> {
        self.signals.get(path).copied()
    }

    fn enum_variant(&self, enum_name: &str, variant: &str) -> Option<(EnumId, u64)> {
        let (id, variants) = self.enums.get(enum_name)?;
        let value = variants.get(variant)?;
        Some((*id, u64::try_from(*value).unwrap_or(0)))
    }

    fn select(&self, _selector: &str, _found: &mut dyn FnMut(SignalInfo)) {}
}

/// One path an expression reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    /// The path text.
    pub path: String,
    /// Byte range in the expression source.
    pub range: std::ops::Range<usize>,
    /// Whether the path is a direct argument of an aggregation.
    pub aggregated: bool,
    /// Whether the path is the argument of `own()`.
    pub own: bool,
}

/// The paths an expression reads, in source order.
pub fn reads(ast: &Ast, src: &str) -> Vec<Read> {
    let mut out = Vec::new();
    visit(ast, ast.root(), src, None, &mut out);
    out.sort_by_key(|r| r.range.start);
    out
}

fn visit(ast: &Ast, id: NodeId, src: &str, parent: Option<Func>, out: &mut Vec<Read>) {
    let span = ast.span(id);
    match ast.node(id) {
        Node::Path => out.push(Read {
            path: src
                .get(span.start as usize..span.end as usize)
                .unwrap_or("")
                .to_owned(),
            range: span.start as usize..span.end as usize,
            aggregated: parent.is_some_and(Func::is_aggregation),
            own: parent == Some(Func::Own),
        }),
        Node::Unary { operand, .. } => visit(ast, operand, src, None, out),
        Node::Binary { lhs, rhs, .. } => {
            visit(ast, lhs, src, None, out);
            visit(ast, rhs, src, None, out);
        }
        Node::If {
            cond,
            then,
            otherwise,
        } => {
            visit(ast, cond, src, None, out);
            visit(ast, then, src, None, out);
            visit(ast, otherwise, src, None, out);
        }
        Node::Call { name, first, count } => {
            let func = src
                .get(name.start as usize..name.end as usize)
                .and_then(Func::from_name);
            for arg in ast.args(first, count) {
                visit(ast, *arg, src, func, out);
            }
        }
        Node::Literal { .. } | Node::Bool(_) | Node::Selector | Node::EnumVariant => {}
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn layout_does_not_change_the_canonical_text() {
        let a = canonical("held(run_cmd&&!running,10s)").unwrap();
        let b = canonical("held( run_cmd &&\n   ! running , 10 s )").unwrap();
        assert_eq!(a, "held(run_cmd && !running, 10 s)");
        assert_eq!(a, b);
        assert_eq!(
            canonical("clamp(pct,0 %,100 %)").unwrap(),
            "clamp(pct, 0 %, 100 %)"
        );
        assert_eq!(canonical("a - -b").unwrap(), "a - -b");
        assert_eq!(canonical("-(a) * 2").unwrap(), "-(a) * 2");
        assert_eq!(canonical("12 m3/h").unwrap(), "12 m3/h");
        assert_eq!(
            canonical("if a then 1 else 2").unwrap(),
            "if a then 1 else 2"
        );
    }

    #[test]
    fn reads_mark_aggregations_and_own() {
        let src = "any(pumps.running) && !own(on) && x";
        let ast = joker_expr::parse(src).unwrap();
        let r = reads(&ast, src);
        assert_eq!(r.len(), 3);
        assert!(r[0].aggregated);
        assert!(r[1].own);
        assert_eq!(r[2].path, "x");
    }
}
