use joker_units::Span;

use crate::ast::{Ast, BinaryOp, Node, NodeId, UnaryOp};
use crate::check::Func;
use crate::diag::{Diagnostic, ErrorKind};
use crate::lexer::{Lexer, Token, TokenKind};
use crate::limits::{MAX_CALL_ARGS, MAX_DEPTH, MAX_SOURCE_LEN};

/// Parses an expression into a syntax tree (docs/02 §10).
///
/// Syntax errors are `J0158`, invalid paths `J0153`, invalid selectors
/// `J0154` and limit violations `J0149`, each with the span to underline.
pub fn parse(src: &str) -> Result<Ast, Diagnostic> {
    if src.len() > MAX_SOURCE_LEN {
        return Err(Diagnostic::new(
            ErrorKind::TooLong,
            Span::new(MAX_SOURCE_LEN, src.len()),
        ));
    }
    let mut parser = Parser {
        lexer: Lexer::new(src),
        current: Token {
            kind: TokenKind::Eof,
            span: Span::default(),
        },
        ast: Ast::empty(),
        depth: 0,
    };
    parser.advance()?;
    if parser.current.kind == TokenKind::Eof {
        return Err(Diagnostic::new(
            ErrorKind::EmptyExpression,
            parser.current.span,
        ));
    }
    let root = parser.expr()?;
    if parser.current.kind != TokenKind::Eof {
        return Err(Diagnostic::new(
            ErrorKind::UnexpectedToken,
            parser.current.span,
        ));
    }
    parser.ast.set_root(root);
    Ok(parser.ast)
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token,
    ast: Ast,
    depth: usize,
}

impl Parser<'_> {
    fn advance(&mut self) -> Result<Token, Diagnostic> {
        let previous = self.current;
        self.current = self.lexer.next_token()?;
        Ok(previous)
    }

    fn expect(&mut self, kind: TokenKind, error: ErrorKind) -> Result<Token, Diagnostic> {
        if self.current.kind == kind {
            self.advance()
        } else {
            Err(Diagnostic::new(error, self.current.span))
        }
    }

    fn push(&mut self, node: Node, span: Span) -> Result<NodeId, Diagnostic> {
        self.ast
            .push(node, span)
            .ok_or(Diagnostic::new(ErrorKind::TooComplex, span))
    }

    fn enter(&mut self) -> Result<(), Diagnostic> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Diagnostic::new(ErrorKind::TooDeep, self.current.span));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn binary(&mut self, op: BinaryOp, lhs: NodeId, rhs: NodeId) -> Result<NodeId, Diagnostic> {
        let span = self.ast.span(lhs).join(self.ast.span(rhs));
        self.push(Node::Binary { op, lhs, rhs }, span)
    }

    fn expr(&mut self) -> Result<NodeId, Diagnostic> {
        self.enter()?;
        let result = self.or();
        self.leave();
        result
    }

    fn or(&mut self) -> Result<NodeId, Diagnostic> {
        let mut lhs = self.and()?;
        while self.current.kind == TokenKind::OrOr {
            self.advance()?;
            let rhs = self.and()?;
            lhs = self.binary(BinaryOp::Or, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn and(&mut self) -> Result<NodeId, Diagnostic> {
        let mut lhs = self.not()?;
        while self.current.kind == TokenKind::AndAnd {
            self.advance()?;
            let rhs = self.not()?;
            lhs = self.binary(BinaryOp::And, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn not(&mut self) -> Result<NodeId, Diagnostic> {
        if self.current.kind != TokenKind::Bang {
            return self.cmp();
        }
        let bang = self.advance()?;
        self.enter()?;
        let operand = self.not();
        self.leave();
        let operand = operand?;
        let span = bang.span.join(self.ast.span(operand));
        self.push(
            Node::Unary {
                op: UnaryOp::Not,
                operand,
            },
            span,
        )
    }

    fn cmp(&mut self) -> Result<NodeId, Diagnostic> {
        let lhs = self.sum()?;
        let op = match self.current.kind {
            TokenKind::EqEq => BinaryOp::Eq,
            TokenKind::NotEq => BinaryOp::Ne,
            TokenKind::Lt => BinaryOp::Lt,
            TokenKind::Le => BinaryOp::Le,
            TokenKind::Gt => BinaryOp::Gt,
            TokenKind::Ge => BinaryOp::Ge,
            _ => return Ok(lhs),
        };
        self.advance()?;
        let rhs = self.sum()?;
        if matches!(
            self.current.kind,
            TokenKind::EqEq
                | TokenKind::NotEq
                | TokenKind::Lt
                | TokenKind::Le
                | TokenKind::Gt
                | TokenKind::Ge
        ) {
            return Err(Diagnostic::new(
                ErrorKind::ChainedComparison,
                self.current.span,
            ));
        }
        self.binary(op, lhs, rhs)
    }

    fn sum(&mut self) -> Result<NodeId, Diagnostic> {
        let mut lhs = self.prod()?;
        loop {
            let op = match self.current.kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => return Ok(lhs),
            };
            self.advance()?;
            let rhs = self.prod()?;
            lhs = self.binary(op, lhs, rhs)?;
        }
    }

    fn prod(&mut self) -> Result<NodeId, Diagnostic> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.current.kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                _ => return Ok(lhs),
            };
            self.advance()?;
            let rhs = self.unary()?;
            lhs = self.binary(op, lhs, rhs)?;
        }
    }

    fn unary(&mut self) -> Result<NodeId, Diagnostic> {
        if self.current.kind != TokenKind::Minus {
            return self.atom();
        }
        let minus = self.advance()?;
        self.enter()?;
        let operand = self.unary();
        self.leave();
        let operand = operand?;
        let span = minus.span.join(self.ast.span(operand));
        self.push(
            Node::Unary {
                op: UnaryOp::Neg,
                operand,
            },
            span,
        )
    }

    fn atom(&mut self) -> Result<NodeId, Diagnostic> {
        let token = self.current;
        match token.kind {
            TokenKind::Literal { unit_start } => {
                self.advance()?;
                self.push(Node::Literal { unit_start }, token.span)
            }
            TokenKind::True | TokenKind::False => {
                self.advance()?;
                self.push(Node::Bool(token.kind == TokenKind::True), token.span)
            }
            TokenKind::EnumVariant => {
                self.advance()?;
                self.push(Node::EnumVariant, token.span)
            }
            TokenKind::Path => {
                self.advance()?;
                if self.current.kind == TokenKind::LParen {
                    self.call(token)
                } else {
                    self.push(Node::Path, token.span)
                }
            }
            TokenKind::Selector => Err(Diagnostic::new(ErrorKind::SelectorOutsideCall, token.span)),
            TokenKind::LParen => {
                self.advance()?;
                let inner = self.expr()?;
                self.expect(TokenKind::RParen, ErrorKind::ExpectedCloseParen)?;
                Ok(inner)
            }
            TokenKind::If => self.if_expr(),
            TokenKind::Eof => Err(Diagnostic::new(ErrorKind::UnexpectedEnd, token.span)),
            _ => Err(Diagnostic::new(ErrorKind::UnexpectedToken, token.span)),
        }
    }

    fn if_expr(&mut self) -> Result<NodeId, Diagnostic> {
        let start = self.advance()?.span;
        let cond = self.expr()?;
        self.expect(TokenKind::Then, ErrorKind::ExpectedThen)?;
        let then = self.expr()?;
        self.expect(TokenKind::Else, ErrorKind::ExpectedElse)?;
        let otherwise = self.expr()?;
        let span = start.join(self.ast.span(otherwise));
        self.push(
            Node::If {
                cond,
                then,
                otherwise,
            },
            span,
        )
    }

    fn call(&mut self, name: Token) -> Result<NodeId, Diagnostic> {
        if self.lexer_text(name.span).contains('.') {
            return Err(Diagnostic::new(ErrorKind::InvalidFunctionName, name.span));
        }
        let aggregation =
            Func::from_name(self.lexer_text(name.span)).is_some_and(Func::is_aggregation);
        if aggregation {
            self.lexer.expect_selector();
        }
        self.advance()?;
        let mut args = [NodeId(0); MAX_CALL_ARGS];
        let mut count = 0usize;
        if self.current.kind != TokenKind::RParen {
            loop {
                let arg = if self.current.kind == TokenKind::Selector {
                    let token = self.advance()?;
                    if !matches!(self.current.kind, TokenKind::Comma | TokenKind::RParen) {
                        return Err(Diagnostic::new(
                            ErrorKind::UnexpectedToken,
                            self.current.span,
                        ));
                    }
                    self.push(Node::Selector, token.span)?
                } else {
                    self.expr()?
                };
                let slot = args.get_mut(count).ok_or(Diagnostic::new(
                    ErrorKind::TooManyArguments,
                    self.ast.span(arg),
                ))?;
                *slot = arg;
                count += 1;
                if self.current.kind != TokenKind::Comma {
                    break;
                }
                if aggregation {
                    self.lexer.expect_selector();
                }
                self.advance()?;
            }
        }
        let close = self.expect(TokenKind::RParen, ErrorKind::ExpectedCloseParen)?;
        let span = name.span.join(close.span);
        let (first, count) = self
            .ast
            .push_args(args.get(..count).unwrap_or(&[]))
            .ok_or(Diagnostic::new(ErrorKind::TooComplex, span))?;
        self.push(
            Node::Call {
                name: name.span,
                first,
                count,
            },
            span,
        )
    }

    fn lexer_text(&self, span: Span) -> &str {
        self.lexer.source().get(span.range()).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::format;
    use std::string::String;

    fn show(ast: &Ast, src: &str, id: NodeId) -> String {
        let text = |id: NodeId| &src[ast.span(id).range()];
        match ast.node(id) {
            Node::Literal { .. } | Node::Path | Node::Selector | Node::EnumVariant => {
                String::from(text(id))
            }
            Node::Bool(b) => format!("{b}"),
            Node::Unary { op, operand } => {
                let o = show(ast, src, operand);
                match op {
                    UnaryOp::Neg => format!("(-{o})"),
                    UnaryOp::Not => format!("(!{o})"),
                }
            }
            Node::Binary { op, lhs, rhs } => format!(
                "({} {} {})",
                show(ast, src, lhs),
                op.symbol(),
                show(ast, src, rhs)
            ),
            Node::If {
                cond,
                then,
                otherwise,
            } => format!(
                "(if {} then {} else {})",
                show(ast, src, cond),
                show(ast, src, then),
                show(ast, src, otherwise)
            ),
            Node::Call { name, first, count } => {
                let mut s = format!("{}(", &src[name.range()]);
                for (i, a) in ast.args(first, count).iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(&show(ast, src, *a));
                }
                s.push(')');
                s
            }
        }
    }

    fn p(src: &str) -> String {
        let ast = parse(src).unwrap();
        show(&ast, src, ast.root())
    }

    #[test]
    fn precedence() {
        assert_eq!(p("a || b && c"), "(a || (b && c))");
        assert_eq!(p("!a == b"), "(!(a == b))");
        assert_eq!(p("a + b * c - d"), "((a + (b * c)) - d)");
        assert_eq!(p("-a * b"), "((-a) * b)");
        assert_eq!(p("a / b / c"), "((a / b) / c)");
        assert_eq!(p("(a + b) * c"), "((a + b) * c)");
        assert_eq!(p("!!a"), "(!(!a))");
        assert_eq!(p("--a"), "(-(-a))");
    }

    #[test]
    fn atoms() {
        assert_eq!(p("meter.f > 49.8 Hz"), "(meter.f > 49.8 Hz)");
        assert_eq!(p("true && !false"), "(true && (!false))");
        assert_eq!(
            p("s.mode == SiteMode.maintenance"),
            "(s.mode == SiteMode.maintenance)"
        );
        assert_eq!(
            p("if a then 1 kW else -2 kW"),
            "(if a then 1 kW else (-2 kW))"
        );
        assert_eq!(p("clamp(x, 0, 1)"), "clamp(x, 0, 1)");
        assert_eq!(p("any(alarm.p1_*.active)"), "any(alarm.p1_*.active)");
        assert_eq!(p("sum(a*b, c * 2)"), "sum(a*b, (c * 2))");
        assert_eq!(p("max(a*b, c)"), "max((a * b), c)");
        assert_eq!(p("count(**)"), "count(**)");
        assert_eq!(p("f()"), "f()");
        assert_eq!(p("avg(x, 10s) > max(a, b)"), "(avg(x, 10s) > max(a, b))");
    }

    #[test]
    fn spans_cover_the_operands() {
        let src = "a.b  +  c";
        let ast = parse(src).unwrap();
        assert_eq!(ast.span(ast.root()).range(), 0..9);
    }

    #[test]
    fn errors() {
        for (src, code, at) in [
            ("", "J0158", 0..0),
            ("a <", "J0158", 3..3),
            ("a < b < c", "J0158", 6..7),
            ("(a", "J0158", 2..2),
            ("a b", "J0158", 2..3),
            ("if a then b", "J0158", 11..11),
            ("if a b", "J0158", 5..6),
            ("f(a,)", "J0158", 4..5),
            ("a.b(1)", "J0158", 0..3),
            ("x * meter.*", "J0158", 4..11),
            ("any(a.**b)", "J0154", 4..9),
            ("sum(a.* + 1)", "J0158", 8..9),
            ("max(a.*)", "J0158", 4..7),
        ] {
            let e = parse(src).unwrap_err();
            assert_eq!((e.code(), e.span.range()), (code, at), "{src}");
        }
    }

    #[test]
    fn limits() {
        let deep = format!("{}a{}", "(".repeat(100), ")".repeat(100));
        assert_eq!(parse(&deep).unwrap_err().code(), "J0149");
        let long = "a + ".repeat(1100) + "a";
        assert_eq!(parse(&long).unwrap_err().code(), "J0149");
        let wide = "a + ".repeat(300) + "a";
        assert_eq!(parse(&wide).unwrap_err().code(), "J0149");
        let args = format!("f({}a)", "a, ".repeat(20));
        assert_eq!(parse(&args).unwrap_err().code(), "J0149");
        let nots = format!("{}a", "!".repeat(200));
        assert_eq!(parse(&nots).unwrap_err().code(), "J0149");
    }
}
