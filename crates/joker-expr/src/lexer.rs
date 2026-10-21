use joker_units::Span;

use crate::diag::{Diagnostic, ErrorKind};

/// The longest path segment (ref/00 §10).
pub const MAX_SEGMENT_LEN: usize = 32;
/// The most segments in a path (ref/00 §10).
pub const MAX_SEGMENTS: usize = 8;

/// A token kind of the expression language (docs/02 §10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A number, possibly followed by a unit: the whole literal text is the
    /// token, and `unit_start` is where the unit begins (equal to the end
    /// when there is none). Whether the unit is a quantity unit or a
    /// duration unit depends on the position, and is decided by the checker.
    Literal {
        /// Byte offset of the unit, or the end of the token without a unit.
        unit_start: u32,
    },
    /// A signal path (`meter.p_total`) or a bare name (`abs` before `(`).
    Path,
    /// A path with `*` or `**` segments, only valid as an aggregation argument.
    Selector,
    /// An enum variant, `Enum.variant`.
    EnumVariant,
    /// `true`.
    True,
    /// `false`.
    False,
    /// `if`.
    If,
    /// `then`.
    Then,
    /// `else`.
    Else,
    /// `(`.
    LParen,
    /// `)`.
    RParen,
    /// `,`.
    Comma,
    /// `||`.
    OrOr,
    /// `&&`.
    AndAnd,
    /// `!`.
    Bang,
    /// `==`.
    EqEq,
    /// `!=`.
    NotEq,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
    /// `+`.
    Plus,
    /// `-`.
    Minus,
    /// `*`.
    Star,
    /// `/`.
    Slash,
    /// The end of the input.
    Eof,
}

/// A token and its span in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// What the token is.
    pub kind: TokenKind,
    /// Where it is.
    pub span: Span,
}

/// A lexer over one expression; it never allocates.
#[derive(Debug, Clone)]
pub struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    selector_next: bool,
}

const KEYWORDS: [(&str, TokenKind); 5] = [
    ("true", TokenKind::True),
    ("false", TokenKind::False),
    ("if", TokenKind::If),
    ("then", TokenKind::Then),
    ("else", TokenKind::Else),
];

fn is_unit_start(c: char) -> bool {
    c.is_ascii_alphabetic()
        || matches!(
            c,
            '\u{b0}' | '\u{b5}' | '\u{3bc}' | '\u{3a9}' | '\u{2126}' | '%'
        )
}

fn is_unit_char(c: char) -> bool {
    is_unit_start(c) || c.is_ascii_digit() || c == '\u{b7}'
}

fn is_path_char(c: char, selector: bool) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || (selector && c == '*')
}

impl<'a> Lexer<'a> {
    /// A lexer at the start of `src`.
    pub fn new(src: &'a str) -> Lexer<'a> {
        Lexer {
            src,
            pos: 0,
            selector_next: false,
        }
    }

    /// The source being lexed.
    pub fn source(&self) -> &'a str {
        self.src
    }

    fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.src
            .get(self.pos + offset..)
            .and_then(|s| s.chars().next())
    }

    fn span(&self, start: usize) -> Span {
        Span::new(start, self.pos)
    }

    /// Lexes the next token as a possible selector: a run of path
    /// characters and `*` without spaces (`alarm.p1_*.active`, `meter.**`).
    /// The parser asks for this only at the start of an aggregation
    /// argument; everywhere else `*` is multiplication.
    pub fn expect_selector(&mut self) {
        self.selector_next = true;
    }

    /// The next token, or a diagnostic for text that is not a token.
    pub fn next_token(&mut self) -> Result<Token, Diagnostic> {
        let selector = core::mem::replace(&mut self.selector_next, false);
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += self.peek().map_or(1, char::len_utf8);
        }
        let start = self.pos;
        let Some(c) = self.peek() else {
            return Ok(Token {
                kind: TokenKind::Eof,
                span: self.span(start),
            });
        };
        if c.is_ascii_digit() {
            return self.literal(start);
        }
        if c.is_ascii_lowercase() || (selector && c == '*') {
            return self.path(start, selector);
        }
        if c.is_ascii_uppercase() {
            return self.enum_variant(start);
        }
        let two = |a: char, b: char| c == a && self.peek_at(1) == Some(b);
        let (kind, len) = if two('|', '|') {
            (TokenKind::OrOr, 2)
        } else if two('&', '&') {
            (TokenKind::AndAnd, 2)
        } else if two('=', '=') {
            (TokenKind::EqEq, 2)
        } else if two('!', '=') {
            (TokenKind::NotEq, 2)
        } else if two('<', '=') {
            (TokenKind::Le, 2)
        } else if two('>', '=') {
            (TokenKind::Ge, 2)
        } else {
            let kind = match c {
                '!' => TokenKind::Bang,
                '<' => TokenKind::Lt,
                '>' => TokenKind::Gt,
                '+' => TokenKind::Plus,
                '-' => TokenKind::Minus,
                '/' => TokenKind::Slash,
                '*' => TokenKind::Star,
                '(' => TokenKind::LParen,
                ')' => TokenKind::RParen,
                ',' => TokenKind::Comma,
                _ => {
                    self.pos += c.len_utf8();
                    return Err(Diagnostic::new(
                        ErrorKind::UnexpectedChar(c),
                        self.span(start),
                    ));
                }
            };
            (kind, 1)
        };
        self.pos += len;
        Ok(Token {
            kind,
            span: self.span(start),
        })
    }

    fn skip_digits(&mut self) -> usize {
        let n = self.rest().bytes().take_while(u8::is_ascii_digit).count();
        self.pos += n;
        n
    }

    fn literal(&mut self, start: usize) -> Result<Token, Diagnostic> {
        self.skip_digits();
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
            self.skip_digits();
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let save = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            if self.skip_digits() == 0 {
                self.pos = save;
            }
        }
        let number_end = self.pos;
        if self.peek() == Some(' ') && self.peek_at(1).is_some_and(is_unit_start) {
            let word: &str = self
                .src
                .get(self.pos + 1..)
                .unwrap_or("")
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .next()
                .unwrap_or("");
            if !KEYWORDS.iter().any(|(k, _)| *k == word) {
                self.pos += 1;
            }
        }
        let unit_start = self.pos;
        if unit_start != number_end || self.peek().is_some_and(is_unit_start) {
            self.unit();
            if self.pos == unit_start {
                self.pos = number_end;
            }
        }
        let unit_start = if self.pos == number_end {
            number_end
        } else {
            unit_start
        };
        if self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
        {
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
            {
                self.pos += 1;
            }
            return Err(Diagnostic::new(
                ErrorKind::MalformedLiteral,
                self.span(start),
            ));
        }
        Ok(Token {
            kind: TokenKind::Literal {
                unit_start: u32::try_from(unit_start).unwrap_or(u32::MAX),
            },
            span: self.span(start),
        })
    }

    fn unit(&mut self) {
        loop {
            while self.peek().is_some_and(is_unit_char) {
                self.pos += self.peek().map_or(1, char::len_utf8);
            }
            let op = self.peek();
            if matches!(op, Some('/' | '*')) && self.peek_at(1).is_some_and(is_unit_start) {
                self.pos += 1;
                continue;
            }
            break;
        }
    }

    fn path(&mut self, start: usize, allow_star: bool) -> Result<Token, Diagnostic> {
        let mut selector = false;
        let mut segments = 0usize;
        loop {
            let seg_start = self.pos;
            while self.peek().is_some_and(|c| is_path_char(c, allow_star)) {
                self.pos += 1;
            }
            let seg = self.src.get(seg_start..self.pos).unwrap_or("");
            segments += 1;
            if seg.contains('*') {
                selector = true;
                let valid = seg == "**" || !seg.contains("**");
                if !valid {
                    self.finish_path();
                    return Err(Diagnostic::new(
                        ErrorKind::InvalidSelector,
                        self.span(start),
                    ));
                }
            } else if seg.is_empty()
                || (!seg.starts_with(|c: char| c.is_ascii_lowercase()) && !allow_star)
                || seg.starts_with(|c: char| c.is_ascii_digit() || c == '_')
                || seg.len() > MAX_SEGMENT_LEN
            {
                self.finish_path();
                return Err(Diagnostic::new(ErrorKind::InvalidPath, self.span(start)));
            }
            if self.peek() == Some('.')
                && self.peek_at(1).is_some_and(|c| is_path_char(c, allow_star))
            {
                self.pos += 1;
                continue;
            }
            break;
        }
        if !allow_star && self.peek() == Some('.') && self.peek_at(1) == Some('*') {
            self.finish_path();
            return Err(Diagnostic::new(
                ErrorKind::SelectorOutsideCall,
                self.span(start),
            ));
        }
        if self
            .peek()
            .is_some_and(|c| c.is_ascii_uppercase() || c == '.')
        {
            self.finish_path();
            return Err(Diagnostic::new(ErrorKind::InvalidPath, self.span(start)));
        }
        if segments > MAX_SEGMENTS {
            return Err(Diagnostic::new(ErrorKind::InvalidPath, self.span(start)));
        }
        let text = self.src.get(start..self.pos).unwrap_or("");
        if let Some((_, kind)) = KEYWORDS.iter().find(|(k, _)| *k == text) {
            return Ok(Token {
                kind: *kind,
                span: self.span(start),
            });
        }
        Ok(Token {
            kind: if selector {
                TokenKind::Selector
            } else {
                TokenKind::Path
            },
            span: self.span(start),
        })
    }

    fn finish_path(&mut self) {
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '*')
        {
            self.pos += 1;
        }
    }

    fn enum_variant(&mut self, start: usize) -> Result<Token, Diagnostic> {
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            self.pos += 1;
        }
        if self.peek() != Some('.') {
            return Err(Diagnostic::new(
                ErrorKind::MalformedEnumVariant,
                self.span(start),
            ));
        }
        self.pos += 1;
        let variant_start = self.pos;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            self.pos += 1;
        }
        let valid = self.pos > variant_start
            && self
                .src
                .get(variant_start..)
                .is_some_and(|s| s.starts_with(|c: char| c.is_ascii_lowercase()))
            && !self
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
        if !valid {
            self.finish_path();
            return Err(Diagnostic::new(
                ErrorKind::MalformedEnumVariant,
                self.span(start),
            ));
        }
        Ok(Token {
            kind: TokenKind::EnumVariant,
            span: self.span(start),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> [Option<TokenKind>; 12] {
        let mut lexer = Lexer::new(src);
        let mut out = [None; 12];
        for slot in out.iter_mut() {
            let t = lexer.next_token().unwrap();
            *slot = Some(t.kind);
            if t.kind == TokenKind::Eof {
                break;
            }
        }
        out
    }

    fn texts<'a>(src: &'a str, out: &mut [&'a str; 12]) -> usize {
        let mut lexer = Lexer::new(src);
        let mut n = 0;
        loop {
            let t = lexer.next_token().unwrap();
            if t.kind == TokenKind::Eof {
                return n;
            }
            out[n] = &src[t.span.range()];
            n += 1;
        }
    }

    #[test]
    fn operators_and_paths() {
        use TokenKind::*;
        let k = kinds("a.b >= 2 && !c || d != e");
        assert_eq!(
            &k[..11],
            &[
                Some(Path),
                Some(Ge),
                Some(Literal { unit_start: 8 }),
                Some(AndAnd),
                Some(Bang),
                Some(Path),
                Some(OrOr),
                Some(Path),
                Some(NotEq),
                Some(Path),
                Some(Eof)
            ]
        );
    }

    #[test]
    fn literals_with_units() {
        let mut out = [""; 12];
        let n = texts("meter.f > 49.8 Hz && p < 20 kW/s", &mut out);
        assert_eq!(
            &out[..n],
            &["meter.f", ">", "49.8 Hz", "&&", "p", "<", "20 kW/s"]
        );
        let n = texts("avg(x, 10s) > 5 % + 1e3W", &mut out);
        assert_eq!(
            &out[..n],
            &["avg", "(", "x", ",", "10s", ")", ">", "5 %", "+", "1e3W"]
        );
        let n = texts("if c then 5 else 2 °C", &mut out);
        assert_eq!(&out[..n], &["if", "c", "then", "5", "else", "2 °C"]);
        let n = texts("2 * x / 3 s", &mut out);
        assert_eq!(&out[..n], &["2", "*", "x", "/", "3 s"]);
        let n = texts("t > 0.5 °C/min", &mut out);
        assert_eq!(&out[..n], &["t", ">", "0.5 °C/min"]);
    }

    #[test]
    fn unit_start_offsets() {
        let mut lexer = Lexer::new("125 kW");
        let t = lexer.next_token().unwrap();
        assert_eq!(t.kind, TokenKind::Literal { unit_start: 4 });
        let mut lexer = Lexer::new("12");
        let t = lexer.next_token().unwrap();
        assert_eq!(t.kind, TokenKind::Literal { unit_start: 2 });
    }

    #[test]
    fn selectors_only_on_request() {
        let mut lexer = Lexer::new("alarm.p1_*.active meter.** a*b");
        for want in ["alarm.p1_*.active", "meter.**"] {
            lexer.expect_selector();
            let t = lexer.next_token().unwrap();
            assert_eq!(t.kind, TokenKind::Selector);
            assert_eq!(&lexer.source()[t.span.range()], want);
        }
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Path);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Star);
        let mut lexer = Lexer::new("a.b");
        lexer.expect_selector();
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Path);
    }

    #[test]
    fn enums() {
        assert_eq!(
            kinds("s == SiteMode.maintenance")[2],
            Some(TokenKind::EnumVariant)
        );
        assert_eq!(kinds("a*b")[1], Some(TokenKind::Star));
    }

    #[test]
    fn errors() {
        for (src, code) in [
            ("a $ b", "J0158"),
            ("Foo", "J0158"),
            ("Foo.Bar", "J0158"),
            ("a.B", "J0153"),
            ("a.b.c.d.e.f.g.h.i", "J0153"),
            ("12abc.x", "J0158"),
        ] {
            let mut lexer = Lexer::new(src);
            let err = loop {
                match lexer.next_token() {
                    Ok(t) if t.kind == TokenKind::Eof => panic!("{src}: no error"),
                    Ok(_) => continue,
                    Err(e) => break e,
                }
            };
            assert_eq!(err.code(), code, "{src}");
        }
    }
}
