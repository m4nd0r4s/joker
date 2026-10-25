//! The `.jkc` parser (docs/17 §3.2): a recursive-descent reader over the
//! text, with expressions and literals kept as text (and spans) for the
//! checker, which hands them to `joker-expr` and `joker-units`.
//!
//! A syntax error stops the file with `JC0101`.

use joker_manifest::Diagnostic;
use joker_manifest::source::{FileId, Span};

use crate::ast::{
    Action, Annotation, Capability, Class, ClassRef, Direction, EnumDecl, Event, EventBody, Field,
    File, Health, Item, Member, Method, Nested, Query, RecordDecl, Spanned, Stmt, TypeRef, Use,
    Version,
};

/// The code of a syntax error.
pub const SYNTAX: &str = "JC0101";

/// Parses one `.jkc` file.
pub fn parse(file: FileId, src: &str) -> Result<File, Diagnostic> {
    let mut parser = Parser { src, pos: 0, file };
    parser.file()
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    file: FileId,
}

const STOP: &[char] = &[',', ')', '=', '@', ';', '{', '}', '(', '['];

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl Parser<'_> {
    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file, start, end)
    }

    fn error(&self, start: usize, title: impl Into<String>) -> Diagnostic {
        let end = self.src[start.min(self.src.len())..]
            .char_indices()
            .nth(1)
            .map_or(self.src.len(), |(i, _)| start + i);
        Diagnostic::new(SYNTAX, title).at(self.span(start, end.max(start)))
    }

    fn rest(&self) -> &str {
        self.src.get(self.pos..).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.rest().chars().nth(n)
    }

    fn bump(&mut self) {
        if let Some(c) = self.peek() {
            self.pos += c.len_utf8();
        }
    }

    fn skip(&mut self, newlines: bool) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r') => self.bump(),
                Some('\n') if newlines => self.bump(),
                Some('/') if self.rest().starts_with("//") && !self.rest().starts_with("///") => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                }
                _ => return,
            }
        }
    }

    fn skip_all(&mut self) {
        loop {
            self.skip(true);
            if self.rest().starts_with("///") {
                while self.peek().is_some_and(|c| c != '\n') {
                    self.bump();
                }
            } else {
                return;
            }
        }
    }

    fn docs(&mut self) -> Option<String> {
        let mut lines: Vec<String> = Vec::new();
        loop {
            self.skip(true);
            if !self.rest().starts_with("///") {
                break;
            }
            self.pos += 3;
            let start = self.pos;
            while self.peek().is_some_and(|c| c != '\n') {
                self.bump();
            }
            let line = &self.src[start..self.pos];
            let line = line.strip_prefix(' ').unwrap_or(line).trim_end();
            lines.push(line.to_owned());
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: char) -> Result<(), Diagnostic> {
        self.skip(true);
        if self.eat(c) {
            Ok(())
        } else {
            Err(self.error(self.pos, format!("expected `{c}`")))
        }
    }

    fn ident(&mut self) -> Result<Spanned<String>, Diagnostic> {
        self.skip(true);
        let start = self.pos;
        if !self.peek().is_some_and(is_ident_start) {
            return Err(self.error(start, "expected a name"));
        }
        while self.peek().is_some_and(is_ident_char) {
            self.bump();
        }
        Ok(Spanned::new(
            self.src[start..self.pos].to_owned(),
            self.span(start, self.pos),
        ))
    }

    fn dotted(&mut self) -> Result<Spanned<String>, Diagnostic> {
        let first = self.ident()?;
        let start = first.span.start;
        while self.peek() == Some('.') && self.peek_at(1).is_some_and(is_ident_start) {
            self.bump();
            self.ident()?;
        }
        Ok(Spanned::new(
            self.src[start..self.pos].to_owned(),
            self.span(start, self.pos),
        ))
    }

    fn peek_word(&mut self) -> Option<&str> {
        self.skip(true);
        let rest = self.rest();
        let len = rest.find(|c: char| !is_ident_char(c)).unwrap_or(rest.len());
        (len > 0).then(|| &rest[..len])
    }

    fn keyword(&mut self, word: &str) -> Result<(), Diagnostic> {
        let start = {
            self.skip(true);
            self.pos
        };
        let found = self.ident()?;
        if found.value == word {
            Ok(())
        } else {
            Err(self.error(start, format!("expected `{word}`")))
        }
    }

    fn integer(&mut self) -> Result<Spanned<i64>, Diagnostic> {
        self.skip(true);
        let start = self.pos;
        if self.peek() == Some('-') {
            self.bump();
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        self.src[start..self.pos]
            .parse()
            .map(|v| Spanned::new(v, self.span(start, self.pos)))
            .map_err(|_| self.error(start, "expected an integer"))
    }

    fn natural(&mut self) -> Result<Spanned<u32>, Diagnostic> {
        let n = self.integer()?;
        u32::try_from(n.value)
            .map(|v| Spanned::new(v, n.span))
            .map_err(|_| Diagnostic::new(SYNTAX, "expected a non-negative integer").at(n.span))
    }

    fn file(&mut self) -> Result<File, Diagnostic> {
        self.skip_all();
        self.keyword("package")?;
        let package = self.package_path()?;
        let mut file = File {
            package,
            uses: Vec::new(),
            classes: Vec::new(),
            enums: Vec::new(),
            records: Vec::new(),
        };
        loop {
            let doc = self.docs();
            self.skip(true);
            if self.peek().is_none() {
                return Ok(file);
            }
            let annotations = self.annotations()?;
            let start = self.pos;
            let word = self.peek_word().map(str::to_owned);
            match word.as_deref() {
                Some("use") if annotations.is_empty() => file.uses.push(self.use_decl()?),
                Some("class") => file.classes.push(self.class(doc, annotations)?),
                Some("enum") if annotations.is_empty() => file.enums.push(self.enum_decl(doc)?),
                Some("record") if annotations.is_empty() => {
                    file.records.push(self.record_decl(doc)?);
                }
                _ => return Err(self.error(start, "expected `use`, `class`, `enum` or `record`")),
            }
        }
    }

    fn package_path(&mut self) -> Result<Spanned<String>, Diagnostic> {
        let first = self.ident()?;
        let start = first.span.start;
        while self.peek() == Some('/') {
            self.bump();
            self.ident()?;
        }
        Ok(Spanned::new(
            self.src[start..self.pos].to_owned(),
            self.span(start, self.pos),
        ))
    }

    fn segments(&mut self) -> Result<(Vec<String>, usize), Diagnostic> {
        let first = self.dotted()?;
        let start = first.span.start;
        let mut segments = vec![first.value];
        while self.peek() == Some('/') {
            self.bump();
            segments.push(self.dotted()?.value);
        }
        Ok((segments, start))
    }

    fn use_decl(&mut self) -> Result<Use, Diagnostic> {
        self.keyword("use")?;
        let (mut segments, start) = self.segments()?;
        if segments.len() < 2 {
            return Err(self.error(start, "expected `use <package>/<name>`"));
        }
        let name = segments.pop().unwrap_or_default();
        let major = if self.eat('@') {
            Some(self.natural()?.value)
        } else {
            None
        };
        let save = self.pos;
        let alias = if self.peek_word() == Some("as") {
            self.ident()?;
            Some(self.ident()?.value)
        } else {
            self.pos = save;
            None
        };
        Ok(Use {
            package: segments.join("/"),
            name,
            major,
            alias,
            span: self.span(start, self.pos),
        })
    }

    fn class_ref(&mut self) -> Result<Spanned<ClassRef>, Diagnostic> {
        self.skip(true);
        let (mut segments, start) = self.segments()?;
        let name = segments.pop().unwrap_or_default();
        if !self.eat('@') {
            return Err(self.error(self.pos, "expected `@<major>` after the class name"));
        }
        let major = self.natural()?.value;
        let package = (!segments.is_empty()).then(|| segments.join("/"));
        Ok(Spanned::new(
            ClassRef {
                package,
                name,
                major,
            },
            self.span(start, self.pos),
        ))
    }

    fn annotations(&mut self) -> Result<Vec<Annotation>, Diagnostic> {
        let mut out = Vec::new();
        loop {
            let save = self.pos;
            self.skip(true);
            if self.peek() != Some('@') {
                self.pos = save;
                return Ok(out);
            }
            let start = self.pos;
            self.bump();
            if !self.peek().is_some_and(is_ident_start) {
                return Err(self.error(start, "expected an annotation name after `@`"));
            }
            let name = self.dotted()?;
            let args = if self.peek() == Some('(') {
                self.bump();
                self.args()?
            } else {
                Vec::new()
            };
            out.push(Annotation {
                name,
                args,
                span: self.span(start, self.pos),
            });
        }
    }

    fn skip_string(&mut self) -> Result<(), Diagnostic> {
        let start = self.pos;
        self.bump();
        loop {
            match self.peek() {
                None | Some('\n') => return Err(self.error(start, "unterminated string")),
                Some('\\') => {
                    self.bump();
                    self.bump();
                }
                Some('"') => {
                    self.bump();
                    return Ok(());
                }
                Some(_) => self.bump(),
            }
        }
    }

    fn args(&mut self) -> Result<Vec<Spanned<String>>, Diagnostic> {
        let open = self.pos.saturating_sub(1);
        let mut out = Vec::new();
        let mut depth = 0usize;
        let mut start = self.pos;
        loop {
            match self.peek() {
                None => return Err(self.error(open, "unclosed `(`")),
                Some('"') => self.skip_string()?,
                Some('(' | '[') => {
                    depth += 1;
                    self.bump();
                }
                Some(')') if depth == 0 => {
                    self.push_arg(&mut out, start, self.pos, open)?;
                    self.bump();
                    return Ok(out);
                }
                Some(')' | ']') => {
                    depth = depth.saturating_sub(1);
                    self.bump();
                }
                Some(',') if depth == 0 => {
                    self.push_arg(&mut out, start, self.pos, open)?;
                    self.bump();
                    start = self.pos;
                }
                Some(_) => self.bump(),
            }
        }
    }

    fn push_arg(
        &self,
        out: &mut Vec<Spanned<String>>,
        start: usize,
        end: usize,
        open: usize,
    ) -> Result<(), Diagnostic> {
        let text = &self.src[start..end];
        let lead = text.len() - text.trim_start().len();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            if out.is_empty() && self.src[open + 1..end].trim().is_empty() {
                return Ok(());
            }
            return Err(self.error(start, "empty argument"));
        }
        let s = start + lead;
        out.push(Spanned::new(
            trimmed.to_owned(),
            self.span(s, s + trimmed.len()),
        ));
        Ok(())
    }

    fn raw(&mut self, what: &str) -> Result<Spanned<String>, Diagnostic> {
        self.skip(false);
        let start = self.pos;
        let mut depth = 0usize;
        loop {
            match self.peek() {
                None => break,
                Some('"') => self.skip_string()?,
                Some('(' | '[') => {
                    depth += 1;
                    self.bump();
                }
                Some(')' | ']') if depth > 0 => {
                    depth -= 1;
                    self.bump();
                }
                Some('\n') if depth > 0 => self.bump(),
                Some('/') if self.rest().starts_with("//") => break,
                Some('\n' | ';' | '}' | '@' | ')' | ']') => break,
                Some(_) => self.bump(),
            }
        }
        let text = self.src[start..self.pos].trim_end();
        if text.is_empty() {
            return Err(self.error(start, format!("expected {what}")));
        }
        Ok(Spanned::new(
            text.to_owned(),
            self.span(start, start + text.len()),
        ))
    }

    fn ty(&mut self, allow_unit: bool) -> Result<Spanned<TypeRef>, Diagnostic> {
        self.skip(true);
        let start = self.pos;
        if self.peek() == Some('(') {
            self.bump();
            self.expect(')')?;
            if !allow_unit {
                return Err(self.error(start, "`()` is only a result type"));
            }
            return Ok(Spanned::new(TypeRef::Unit, self.span(start, self.pos)));
        }
        let word = self.ident()?;
        let ty = match word.value.as_str() {
            "bool" => TypeRef::Bool,
            "i64" => TypeRef::I64,
            "u64" => TypeRef::U64,
            "f64" => TypeRef::F64,
            "f32" => TypeRef::F32,
            "string" => TypeRef::String,
            "bytes" => TypeRef::Bytes,
            "bits" => {
                self.expect('<')?;
                let n = self.natural()?.value;
                self.expect('>')?;
                TypeRef::Bits(n)
            }
            "list" => {
                self.expect('<')?;
                let inner = self.ty(false)?.value;
                self.expect(',')?;
                let n = self.natural()?.value;
                self.expect('>')?;
                TypeRef::List(Box::new(inner), n)
            }
            _ => TypeRef::Named(word.value),
        };
        Ok(Spanned::new(ty, self.span(start, self.pos)))
    }

    fn unit(&mut self) -> Option<Spanned<String>> {
        let save = self.pos;
        self.skip(false);
        let start = self.pos;
        let starts = self
            .peek()
            .is_some_and(|c| !c.is_whitespace() && !STOP.contains(&c))
            && !self.rest().starts_with("//")
            && !self.rest().starts_with("->");
        if !starts {
            self.pos = save;
            return None;
        }
        while self
            .peek()
            .is_some_and(|c| !c.is_whitespace() && !STOP.contains(&c))
        {
            self.bump();
        }
        Some(Spanned::new(
            self.src[start..self.pos].to_owned(),
            self.span(start, self.pos),
        ))
    }

    fn field(&mut self, doc: Option<String>, allow_named: bool) -> Result<Field, Diagnostic> {
        let name = self.ident()?;
        self.expect(':')?;
        let ty = self.ty(false)?;
        if !allow_named && matches!(ty.value, TypeRef::List(..)) {
            return Err(Diagnostic::new(SYNTAX, "`list` is not a member type").at(ty.span));
        }
        let unit = if ty.value.is_numeric() {
            self.unit()
        } else {
            None
        };
        Ok(Field {
            doc,
            name,
            ty,
            unit,
        })
    }

    fn params(&mut self) -> Result<Vec<Field>, Diagnostic> {
        self.expect('(')?;
        let mut out = Vec::new();
        self.skip(true);
        if self.eat(')') {
            return Ok(out);
        }
        loop {
            out.push(self.field(None, true)?);
            self.skip(true);
            if self.eat(')') {
                return Ok(out);
            }
            self.expect(',')?;
        }
    }

    fn fields_block(&mut self) -> Result<Vec<Field>, Diagnostic> {
        self.expect('{')?;
        let mut out = Vec::new();
        loop {
            let doc = self.docs();
            self.skip(true);
            if self.eat('}') {
                return Ok(out);
            }
            out.push(self.field(doc, true)?);
            self.skip(false);
            if !self.eat(',') && !matches!(self.peek(), Some('\n' | '}')) {
                return Err(self.error(self.pos, "expected `,`, a new line or `}`"));
            }
        }
    }

    fn class(
        &mut self,
        doc: Option<String>,
        annotations: Vec<Annotation>,
    ) -> Result<Class, Diagnostic> {
        self.skip(true);
        let start = self.pos;
        self.keyword("class")?;
        let name = self.dotted()?;
        if !self.eat('@') {
            return Err(self.error(self.pos, "expected `@<version>` after the class name"));
        }
        let vstart = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            self.bump();
        }
        let vspan = self.span(vstart, self.pos);
        let version = Version::parse(&self.src[vstart..self.pos]).ok_or_else(|| {
            Diagnostic::new(SYNTAX, "expected a full version `major.minor.patch`").at(vspan)
        })?;
        self.expect('{')?;
        let items = self.items(true)?;
        Ok(Class {
            doc,
            annotations,
            name,
            version: Spanned::new(version, vspan),
            items,
            span: self.span(start, self.pos),
        })
    }

    fn items(&mut self, top: bool) -> Result<Vec<Item>, Diagnostic> {
        let mut items = Vec::new();
        loop {
            let doc = self.docs();
            self.skip(true);
            if self.eat('}') {
                return Ok(items);
            }
            let start = self.pos;
            let Some(word) = self.peek_word().map(str::to_owned) else {
                return Err(self.error(start, "expected a class item"));
            };
            let item = match word.as_str() {
                "in" | "out" | "param" => Item::Member(self.member(doc)?),
                "member" => Item::Nested(self.nested(doc)?),
                "capability" if top => Item::Capability(self.capability()?),
                "method" | "atomic" => Item::Method(self.method(doc)?),
                "query" => Item::Query(self.query(doc)?),
                "health" if top => Item::Health(self.health()?),
                "event" => Item::Event(self.event(doc)?),
                "action" => Item::Action(self.action(doc)?),
                _ => return Err(self.error(start, "expected a class item")),
            };
            items.push(item);
        }
    }

    fn member(&mut self, doc: Option<String>) -> Result<Member, Diagnostic> {
        let keyword = self.ident()?;
        let direction = match keyword.value.as_str() {
            "in" => Direction::In,
            "out" => Direction::Out,
            _ => Direction::Param,
        };
        let name = self.ident()?;
        self.expect(':')?;
        let ty = self.ty(false)?;
        let unit = if ty.value.is_numeric() {
            self.unit()
        } else {
            None
        };
        let save = self.pos;
        self.skip(false);
        let default = if self.peek() == Some('=') {
            self.bump();
            Some(self.raw("a literal")?)
        } else {
            self.pos = save;
            None
        };
        let annotations = self.annotations()?;
        Ok(Member {
            doc,
            direction,
            name,
            ty,
            unit,
            default,
            annotations,
        })
    }

    fn nested(&mut self, doc: Option<String>) -> Result<Nested, Diagnostic> {
        self.keyword("member")?;
        let name = self.ident()?;
        self.expect(':')?;
        let class = self.class_ref()?;
        let range = if self.peek() == Some('[') {
            let start = self.pos;
            self.bump();
            let min = self.natural()?.value;
            self.skip(false);
            if !self.rest().starts_with("..") {
                return Err(self.error(self.pos, "expected `..`"));
            }
            self.pos += 2;
            let max = self.natural()?.value;
            self.expect(']')?;
            Some(Spanned::new((min, max), self.span(start, self.pos)))
        } else {
            None
        };
        let annotations = self.annotations()?;
        Ok(Nested {
            doc,
            name,
            class,
            range,
            annotations,
        })
    }

    fn capability(&mut self) -> Result<Capability, Diagnostic> {
        self.keyword("capability")?;
        let name = self.ident()?;
        self.expect('{')?;
        let items = self.items(false)?;
        Ok(Capability { name, items })
    }

    fn method(&mut self, doc: Option<String>) -> Result<Method, Diagnostic> {
        let atomic = self.peek_word() == Some("atomic");
        if atomic {
            self.ident()?;
        }
        self.keyword("method")?;
        let name = self.ident()?;
        let params = self.params()?;
        let order = if self.peek_word() == Some("order") {
            let start = self.pos;
            self.ident()?;
            self.expect('(')?;
            let mut names = Vec::new();
            loop {
                names.push(self.ident()?);
                self.skip(true);
                if self.eat(')') {
                    break;
                }
                self.expect(',')?;
            }
            Some(Spanned::new(names, self.span(start, self.pos)))
        } else {
            None
        };
        let annotations = self.annotations()?;
        self.expect('{')?;
        let mut body = Vec::new();
        loop {
            self.skip(true);
            while self.eat(';') {
                self.skip(true);
            }
            if self.eat('}') {
                break;
            }
            body.push(self.stmt()?);
            self.skip(false);
            if !matches!(self.peek(), Some('\n' | ';' | '}')) {
                return Err(self.error(self.pos, "expected `;`, a new line or `}`"));
            }
        }
        Ok(Method {
            doc,
            atomic,
            name,
            params,
            order,
            body,
            annotations,
        })
    }

    fn stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let path = self.dotted()?;
        self.skip(false);
        if self.peek() == Some('=') && self.peek_at(1) != Some('=') {
            self.bump();
            let expr = self.raw("an expression")?;
            return Ok(Stmt::Assign { target: path, expr });
        }
        if self.peek() == Some('(') {
            self.bump();
            let args = self.args()?;
            let span = Span::new(self.file, path.span.start, self.pos);
            let mut parts: Vec<&str> = path.value.split('.').collect();
            let method_name = parts.pop().unwrap_or_default().to_owned();
            let method_start = path.span.end - method_name.len();
            let method = Spanned::new(method_name, self.span(method_start, path.span.end));
            let member = match parts.len() {
                0 => None,
                1 => Some(Spanned::new(
                    parts.join("."),
                    self.span(path.span.start, method_start.saturating_sub(1)),
                )),
                _ => {
                    return Err(Diagnostic::new(
                        SYNTAX,
                        "a call names one fixed class member and one of its methods",
                    )
                    .at(path.span));
                }
            };
            return Ok(Stmt::Call {
                member,
                method,
                args,
                span,
            });
        }
        Err(self.error(self.pos, "expected `=` or `(`"))
    }

    fn query(&mut self, doc: Option<String>) -> Result<Query, Diagnostic> {
        self.keyword("query")?;
        let name = self.ident()?;
        self.expect(':')?;
        let ty = self.ty(false)?;
        let unit = if ty.value.is_numeric() {
            self.unit()
        } else {
            None
        };
        self.expect('=')?;
        let expr = self.raw("an expression")?;
        let annotations = self.annotations()?;
        Ok(Query {
            doc,
            name,
            ty,
            unit,
            expr,
            annotations,
        })
    }

    fn health(&mut self) -> Result<Health, Diagnostic> {
        self.skip(true);
        let start = self.pos;
        self.keyword("health")?;
        self.keyword("required")?;
        self.expect('(')?;
        let mut names = Vec::new();
        loop {
            names.push(self.ident()?);
            self.skip(true);
            if self.eat(')') {
                break;
            }
            self.expect(',')?;
        }
        Ok(Health {
            names,
            span: self.span(start, self.pos),
        })
    }

    fn event(&mut self, doc: Option<String>) -> Result<Event, Diagnostic> {
        self.keyword("event")?;
        let name = self.ident()?;
        self.skip(false);
        let body = if self.peek() == Some(':') {
            self.bump();
            EventBody::Record(self.ident()?)
        } else {
            EventBody::Fields(self.fields_block()?)
        };
        let annotations = self.annotations()?;
        Ok(Event {
            doc,
            name,
            body,
            annotations,
        })
    }

    fn action(&mut self, doc: Option<String>) -> Result<Action, Diagnostic> {
        self.keyword("action")?;
        let name = self.ident()?;
        let params = self.params()?;
        self.skip(true);
        if !self.rest().starts_with("->") {
            return Err(self.error(self.pos, "expected `->` and the result type"));
        }
        self.pos += 2;
        let result = self.ty(true)?;
        let annotations = self.annotations()?;
        Ok(Action {
            doc,
            name,
            params,
            result,
            annotations,
        })
    }

    fn enum_decl(&mut self, doc: Option<String>) -> Result<EnumDecl, Diagnostic> {
        self.keyword("enum")?;
        let name = self.ident()?;
        self.expect('{')?;
        let mut variants = Vec::new();
        let mut next = 0i64;
        loop {
            self.skip_all();
            if self.eat('}') {
                break;
            }
            let variant = self.ident()?;
            self.skip(false);
            let value = if self.eat('=') {
                self.integer()?.value
            } else {
                next
            };
            next = value.saturating_add(1);
            variants.push((variant, value));
            self.skip(false);
            if !self.eat(',') && !matches!(self.peek(), Some('\n' | '}')) {
                return Err(self.error(self.pos, "expected `,`, a new line or `}`"));
            }
        }
        Ok(EnumDecl {
            doc,
            name,
            variants,
        })
    }

    fn record_decl(&mut self, doc: Option<String>) -> Result<RecordDecl, Diagnostic> {
        self.keyword("record")?;
        let name = self.ident()?;
        let fields = self.fields_block()?;
        Ok(RecordDecl { doc, name, fields })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;
    use joker_manifest::Sources;

    fn parse_text(text: &str) -> Result<File, Diagnostic> {
        let mut sources = Sources::default();
        let id = sources.add("t.jkc".into(), text.to_owned());
        parse(id, text)
    }

    #[test]
    fn the_pump_of_document_17_parses() {
        let text = r#"package std

/// Pump with start/stop and status feedback.
@aligns("CiA 402 when the actuator is a drive")
class pump@1.0.0 {

    /// Run command.
    out run_cmd: bool            @safe_hint(false)

    in  running: bool
    in  fault:   bool
    in  fault_code: u64          @optional

    param rated_flow: f64 m3/h   @optional

    event tripped { code: u64 }

    action reset() -> ()         @permit_hint(fault && !running) @timeout(5 s)

    method start()               { run_cmd = true }
    method stop()                { run_cmd = false }

    query failed_to_start: bool = held(run_cmd && !running, 10 s)

    capability variable_speed {
        out speed_set: f64 %     @safe_hint(0 %) @range_hint(0 %, 100 %)
        atomic method run_at(pct: f64 %) order(speed_set, run_cmd) {
            speed_set = clamp(pct, 0 %, 100 %)
            run_cmd   = pct > 0 %
        }
    }

    health required(running, fault)
}
"#;
        let file = parse_text(text).unwrap();
        let class = &file.classes[0];
        assert_eq!(class.name.value, "pump");
        assert_eq!(class.version.value.to_string(), "1.0.0");
        assert_eq!(
            class.annotations[0].args[0].value,
            "\"CiA 402 when the actuator is a drive\""
        );
        assert_eq!(class.items.len(), 12);
        let Item::Member(m) = &class.items[4] else {
            panic!()
        };
        assert_eq!(m.unit.as_ref().unwrap().value, "m3/h");
        let Item::Action(a) = &class.items[6] else {
            panic!()
        };
        assert_eq!(a.annotations[0].args[0].value, "fault && !running");
        let Item::Capability(c) = &class.items[10] else {
            panic!()
        };
        let Item::Method(m) = &c.items[1] else {
            panic!()
        };
        assert!(m.atomic);
        assert_eq!(m.body.len(), 2);
        let Stmt::Assign { expr, .. } = &m.body[0] else {
            panic!()
        };
        assert_eq!(expr.value, "clamp(pct, 0 %, 100 %)");
    }

    #[test]
    fn calls_semicolons_and_collections_parse() {
        let text = "package local\nuse std/pump@1 as P\nclass c@1.2.3 {\n member drive: std/drive@1\n member pumps: pump@1[1..8] @requires(pressure)\n method go(pct: f64 %) { drive.run_at(pct); drive.stop() }\n}\nenum Mode { off = 0, on }\nrecord R {\n a: f64 kWh\n b: list<u64, 4>\n}\n";
        let file = parse_text(text).unwrap();
        assert_eq!(file.uses[0].alias.as_deref(), Some("P"));
        assert_eq!(file.enums[0].variants[1].1, 1);
        assert_eq!(file.records[0].fields.len(), 2);
        let Item::Nested(n) = &file.classes[0].items[1] else {
            panic!()
        };
        assert_eq!(n.range.as_ref().unwrap().value, (1, 8));
        assert_eq!(n.class.value.package, None);
        let Item::Method(m) = &file.classes[0].items[2] else {
            panic!()
        };
        assert_eq!(m.body.len(), 2);
    }

    #[test]
    fn syntax_errors_are_jc0101() {
        let e = parse_text("package std\nclass x@1.0 {}").unwrap_err();
        assert_eq!(e.code, "JC0101");
        let e = parse_text("package std\nclass x@1.0.0 { wat }").unwrap_err();
        assert_eq!(e.code, "JC0101");
    }
}
