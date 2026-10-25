//! The syntax tree of a `.jkc` file (docs/17 §3.2), with a span on every
//! name, type and expression so each check can point at the text.

use joker_manifest::source::Span;

/// A value and where it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    /// The value.
    pub value: T,
    /// Where it is in the source.
    pub span: Span,
}

impl<T> Spanned<T> {
    /// A value at a span.
    pub const fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }
}

/// A parsed `.jkc` file.
#[derive(Debug, Clone)]
pub struct File {
    /// `package std`.
    pub package: Spanned<String>,
    /// The `use` declarations.
    pub uses: Vec<Use>,
    /// The classes.
    pub classes: Vec<Class>,
    /// The enums.
    pub enums: Vec<EnumDecl>,
    /// The records.
    pub records: Vec<RecordDecl>,
}

/// `use pkg/name(@major)? (as alias)?`.
#[derive(Debug, Clone)]
pub struct Use {
    /// The package of the used item.
    pub package: String,
    /// The class, enum or record name.
    pub name: String,
    /// The major, for a class.
    pub major: Option<u32>,
    /// The local name.
    pub alias: Option<String>,
    /// The whole declaration.
    pub span: Span,
}

impl Use {
    /// The name the file uses for the item.
    pub fn local_name(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

/// `@name(args)`.
#[derive(Debug, Clone)]
pub struct Annotation {
    /// The dotted name, without `@`.
    pub name: Spanned<String>,
    /// The arguments as written, split at top-level commas.
    pub args: Vec<Spanned<String>>,
    /// The whole annotation.
    pub span: Span,
}

/// A semantic version, `major.minor.patch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Major.
    pub major: u32,
    /// Minor.
    pub minor: u32,
    /// Patch.
    pub patch: u32,
}

impl Version {
    /// Parses `M.m.p`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let mut next = || -> Option<u32> {
            let part = parts.next()?;
            let canonical = part == "0" || !part.starts_with('0');
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) || !canonical {
                return None;
            }
            part.parse().ok()
        };
        let version = Self {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        match parts.next() {
            None => Some(version),
            Some(_) => None,
        }
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// `class name@version { … }`.
#[derive(Debug, Clone)]
pub struct Class {
    /// The `///` lines before the class.
    pub doc: Option<String>,
    /// The annotations before `class`.
    pub annotations: Vec<Annotation>,
    /// The name, possibly dotted (`hvac.vav`).
    pub name: Spanned<String>,
    /// The full version.
    pub version: Spanned<Version>,
    /// The items, in declaration order.
    pub items: Vec<Item>,
    /// From `class` to the closing brace.
    pub span: Span,
}

/// An item of a class or of a capability.
#[derive(Debug, Clone)]
pub enum Item {
    /// `in`, `out` or `param`.
    Member(Member),
    /// `member m: pkg/class@major`.
    Nested(Nested),
    /// `capability name { … }`.
    Capability(Capability),
    /// `method`.
    Method(Method),
    /// `query`.
    Query(Query),
    /// `health required(…)`.
    Health(Health),
    /// `event`.
    Event(Event),
    /// `action`.
    Action(Action),
}

/// The direction of a scalar member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    /// `in`.
    In,
    /// `out`.
    Out,
    /// `param`.
    Param,
}

impl Direction {
    /// The keyword.
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Out => "out",
            Self::Param => "param",
        }
    }
}

/// A type as written (docs/17 §3.2 `type` and `rtype`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRef {
    /// `bool`.
    Bool,
    /// `i64`.
    I64,
    /// `u64`.
    U64,
    /// `f64`.
    F64,
    /// `f32`.
    F32,
    /// `string`.
    String,
    /// `bytes`.
    Bytes,
    /// `bits<N>`.
    Bits(u32),
    /// An enum or record, by local name.
    Named(String),
    /// `list<T, N>`.
    List(Box<TypeRef>, u32),
    /// `()`.
    Unit,
}

impl TypeRef {
    /// Whether values of the type are floating point numbers.
    pub const fn is_float(&self) -> bool {
        matches!(self, Self::F64 | Self::F32)
    }

    /// Whether values of the type are numbers that may carry a unit.
    pub const fn is_numeric(&self) -> bool {
        matches!(self, Self::F64 | Self::F32 | Self::I64 | Self::U64)
    }
}

/// `in`, `out` or `param`.
#[derive(Debug, Clone)]
pub struct Member {
    /// `///` lines.
    pub doc: Option<String>,
    /// The direction.
    pub direction: Direction,
    /// The name.
    pub name: Spanned<String>,
    /// The type.
    pub ty: Spanned<TypeRef>,
    /// The unit as written.
    pub unit: Option<Spanned<String>>,
    /// `= literal`.
    pub default: Option<Spanned<String>>,
    /// The annotations after the declaration.
    pub annotations: Vec<Annotation>,
}

/// A class reference: `pkg/name@major`, with `std` implied.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassRef {
    /// The package, `None` when the reference omits it.
    pub package: Option<String>,
    /// The class name.
    pub name: String,
    /// The major.
    pub major: u32,
}

/// `member name: classref ([min..max])?`.
#[derive(Debug, Clone)]
pub struct Nested {
    /// `///` lines.
    pub doc: Option<String>,
    /// The name.
    pub name: Spanned<String>,
    /// The member's class.
    pub class: Spanned<ClassRef>,
    /// `[min..max]` for a collection.
    pub range: Option<Spanned<(u32, u32)>>,
    /// The annotations.
    pub annotations: Vec<Annotation>,
}

/// `capability name { … }`.
#[derive(Debug, Clone)]
pub struct Capability {
    /// The name.
    pub name: Spanned<String>,
    /// The items.
    pub items: Vec<Item>,
}

/// A parameter of a method or an action, or a field of a record or event.
#[derive(Debug, Clone)]
pub struct Field {
    /// `///` lines (records and events).
    pub doc: Option<String>,
    /// The name.
    pub name: Spanned<String>,
    /// The type.
    pub ty: Spanned<TypeRef>,
    /// The unit as written.
    pub unit: Option<Spanned<String>>,
}

/// A method statement.
#[derive(Debug, Clone)]
pub enum Stmt {
    /// `target = expr`; the target may be dotted (`m.x`, which is `JC0204`).
    Assign {
        /// The assigned name.
        target: Spanned<String>,
        /// The expression text.
        expr: Spanned<String>,
    },
    /// `member.method(args)`, or `name(args)` without a member.
    Call {
        /// The fixed class member, when written.
        member: Option<Spanned<String>>,
        /// The called method.
        method: Spanned<String>,
        /// The argument expressions.
        args: Vec<Spanned<String>>,
        /// The whole statement.
        span: Span,
    },
}

/// `(atomic)? method name(params) (order(…))? { … }`.
#[derive(Debug, Clone)]
pub struct Method {
    /// `///` lines.
    pub doc: Option<String>,
    /// `atomic`.
    pub atomic: bool,
    /// The name.
    pub name: Spanned<String>,
    /// The parameters.
    pub params: Vec<Field>,
    /// `order(…)`.
    pub order: Option<Spanned<Vec<Spanned<String>>>>,
    /// The statements.
    pub body: Vec<Stmt>,
    /// The annotations.
    pub annotations: Vec<Annotation>,
}

/// `query name: type unit? = expr`.
#[derive(Debug, Clone)]
pub struct Query {
    /// `///` lines.
    pub doc: Option<String>,
    /// The name.
    pub name: Spanned<String>,
    /// The type.
    pub ty: Spanned<TypeRef>,
    /// The unit as written.
    pub unit: Option<Spanned<String>>,
    /// The expression text.
    pub expr: Spanned<String>,
    /// The annotations.
    pub annotations: Vec<Annotation>,
}

/// `health required(…)`.
#[derive(Debug, Clone)]
pub struct Health {
    /// The required members.
    pub names: Vec<Spanned<String>>,
    /// The whole declaration.
    pub span: Span,
}

/// The payload of an event.
#[derive(Debug, Clone)]
pub enum EventBody {
    /// `{ fields }`.
    Fields(Vec<Field>),
    /// `: Record`.
    Record(Spanned<String>),
}

/// `event name { … }` or `event name: Record`.
#[derive(Debug, Clone)]
pub struct Event {
    /// `///` lines.
    pub doc: Option<String>,
    /// The name.
    pub name: Spanned<String>,
    /// The payload.
    pub body: EventBody,
    /// The annotations.
    pub annotations: Vec<Annotation>,
}

/// `action name(params) -> result annotations`.
#[derive(Debug, Clone)]
pub struct Action {
    /// `///` lines.
    pub doc: Option<String>,
    /// The name.
    pub name: Spanned<String>,
    /// The parameters.
    pub params: Vec<Field>,
    /// The result type.
    pub result: Spanned<TypeRef>,
    /// The annotations.
    pub annotations: Vec<Annotation>,
}

/// `enum Name { variant = n, … }`.
#[derive(Debug, Clone)]
pub struct EnumDecl {
    /// `///` lines.
    pub doc: Option<String>,
    /// The name.
    pub name: Spanned<String>,
    /// The variants and their values, in declaration order.
    pub variants: Vec<(Spanned<String>, i64)>,
}

/// `record Name { fields }`.
#[derive(Debug, Clone)]
pub struct RecordDecl {
    /// `///` lines.
    pub doc: Option<String>,
    /// The name.
    pub name: Spanned<String>,
    /// The fields, in declaration order.
    pub fields: Vec<Field>,
}
