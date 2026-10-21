//! SPDX licence expressions, as far as the MCU rule needs them.

/// Whether a licence expression lets the crate be used under one of
/// `allowed` alone: some choice of its `OR` alternatives leaves only allowed
/// licences joined by `AND`. The legacy `/` separator counts as `OR`; an
/// expression that does not parse offers nothing.
pub fn offers_one_of(expression: &str, allowed: &[&str]) -> bool {
    let tokens = tokenize(expression);
    let mut parser = Parser {
        tokens: &tokens,
        at: 0,
    };
    match parser.or() {
        Some(expr) if parser.at == tokens.len() => satisfiable(&expr, allowed),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    And,
    Or,
    With,
    Id(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    Licence(String),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

fn tokenize(text: &str) -> Vec<Token> {
    let spaced = text
        .replace('(', " ( ")
        .replace(')', " ) ")
        .replace('/', " OR ");
    spaced
        .split_whitespace()
        .map(|word| match word {
            "(" => Token::Open,
            ")" => Token::Close,
            "AND" => Token::And,
            "OR" => Token::Or,
            "WITH" => Token::With,
            id => Token::Id(id.to_owned()),
        })
        .collect()
}

struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn or(&mut self) -> Option<Expr> {
        let mut terms = vec![self.and()?];
        while self.peek() == Some(&Token::Or) {
            self.at += 1;
            terms.push(self.and()?);
        }
        Some(if terms.len() == 1 {
            terms.remove(0)
        } else {
            Expr::Or(terms)
        })
    }

    fn and(&mut self) -> Option<Expr> {
        let mut terms = vec![self.atom()?];
        while self.peek() == Some(&Token::And) {
            self.at += 1;
            terms.push(self.atom()?);
        }
        Some(if terms.len() == 1 {
            terms.remove(0)
        } else {
            Expr::And(terms)
        })
    }

    fn atom(&mut self) -> Option<Expr> {
        match self.peek()?.clone() {
            Token::Open => {
                self.at += 1;
                let inner = self.or()?;
                if self.peek() != Some(&Token::Close) {
                    return None;
                }
                self.at += 1;
                Some(inner)
            }
            Token::Id(id) => {
                self.at += 1;
                if self.peek() == Some(&Token::With) {
                    self.at += 1;
                    match self.peek()?.clone() {
                        Token::Id(exception) => {
                            self.at += 1;
                            Some(Expr::Licence(format!("{id} WITH {exception}")))
                        }
                        _ => None,
                    }
                } else {
                    Some(Expr::Licence(id))
                }
            }
            _ => None,
        }
    }
}

fn satisfiable(expr: &Expr, allowed: &[&str]) -> bool {
    match expr {
        Expr::Licence(id) => allowed.contains(&id.as_str()),
        Expr::And(terms) => terms.iter().all(|t| satisfiable(t, allowed)),
        Expr::Or(terms) => terms.iter().any(|t| satisfiable(t, allowed)),
    }
}

#[cfg(test)]
mod tests {
    use super::offers_one_of;

    const MCU: &[&str] = &["MIT", "Apache-2.0"];

    #[test]
    fn dual_and_single_permissive_licences_pass() {
        assert!(offers_one_of("MIT", MCU));
        assert!(offers_one_of("MIT OR Apache-2.0", MCU));
        assert!(offers_one_of("Apache-2.0/MIT", MCU));
        assert!(offers_one_of(
            "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
            MCU
        ));
        assert!(offers_one_of("(MIT OR Apache-2.0) AND MIT", MCU));
    }

    #[test]
    fn anything_else_fails() {
        assert!(!offers_one_of("Apache-2.0 WITH LLVM-exception", MCU));
        assert!(!offers_one_of("BSD-3-Clause", MCU));
        assert!(!offers_one_of("(MIT OR Apache-2.0) AND Unicode-3.0", MCU));
        assert!(!offers_one_of("GPL-2.0-only OR LGPL-2.1", MCU));
        assert!(!offers_one_of("MIT OR", MCU));
        assert!(!offers_one_of("(MIT", MCU));
        assert!(!offers_one_of("", MCU));
    }
}
