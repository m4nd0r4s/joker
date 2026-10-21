use core::fmt;

/// A byte range in the parsed text, `start..end`.
///
/// Errors carry spans so that callers can render rustc-style diagnostics
/// against the original string (ref/00 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    /// First byte of the range.
    pub start: u32,
    /// One past the last byte of the range.
    pub end: u32,
}

impl Span {
    /// Builds a span from byte offsets, saturating at `u32::MAX`.
    pub fn new(start: usize, end: usize) -> Span {
        Span {
            start: u32::try_from(start).unwrap_or(u32::MAX),
            end: u32::try_from(end).unwrap_or(u32::MAX),
        }
    }

    /// The same span moved right by `offset` bytes.
    pub fn shifted(self, offset: u32) -> Span {
        Span {
            start: self.start.saturating_add(offset),
            end: self.end.saturating_add(offset),
        }
    }

    /// The smallest span that covers both.
    pub fn join(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// The span as a `usize` range, for slicing the source text.
    pub fn range(self) -> core::ops::Range<usize> {
        self.start as usize..self.end as usize
    }
}

/// What went wrong while parsing or converting units, quantities or durations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// The text is empty where a unit or literal is required.
    Empty,
    /// A unit symbol that is not in the catalogue of docs/16 §5.
    UnknownUnit,
    /// A prefix on a unit that does not take prefixes (`°C`, `%`, `1`, `rpm`, `gn`).
    PrefixNotAllowed,
    /// A malformed unit expression (a dangling `/`, a bad exponent, `°C` raised to a power).
    InvalidUnitSyntax,
    /// A unit symbol longer than [`crate::MAX_SYMBOL_LEN`] bytes.
    SymbolTooLong,
    /// A dimension exponent outside the `i8` range.
    ExponentOverflow,
    /// A malformed or non-finite number in a quantity literal.
    InvalidNumber,
    /// A number without a unit where a quantity is required.
    MissingUnit,
    /// Two units with different dimensions.
    IncompatibleDimension,
    /// Two units with the same dimension but different quantity kinds (`W` against `var`).
    IncompatibleKind,
    /// A malformed duration literal.
    InvalidDuration,
    /// A duration that does not fit in `u64` nanoseconds.
    DurationOverflow,
}

impl ErrorKind {
    /// The registry code (docs/16 §9) a manifest diagnostic uses for this error.
    pub const fn code(self) -> &'static str {
        match self {
            ErrorKind::UnknownUnit
            | ErrorKind::PrefixNotAllowed
            | ErrorKind::InvalidUnitSyntax
            | ErrorKind::SymbolTooLong
            | ErrorKind::ExponentOverflow => "J0109",
            ErrorKind::Empty | ErrorKind::InvalidNumber | ErrorKind::MissingUnit => "J0151",
            ErrorKind::IncompatibleDimension => "J0107",
            ErrorKind::IncompatibleKind => "J0108",
            ErrorKind::InvalidDuration | ErrorKind::DurationOverflow => "J0152",
        }
    }

    /// A short English description.
    pub const fn message(self) -> &'static str {
        match self {
            ErrorKind::Empty => "empty literal",
            ErrorKind::UnknownUnit => "unit outside the catalogue",
            ErrorKind::PrefixNotAllowed => "this unit does not take a prefix",
            ErrorKind::InvalidUnitSyntax => "malformed unit",
            ErrorKind::SymbolTooLong => "unit symbol too long",
            ErrorKind::ExponentOverflow => "unit exponent out of range",
            ErrorKind::InvalidNumber => "invalid number",
            ErrorKind::MissingUnit => "number without a unit where a quantity is required",
            ErrorKind::IncompatibleDimension => "incompatible unit (dimension)",
            ErrorKind::IncompatibleKind => "incompatible quantity kind",
            ErrorKind::InvalidDuration => "invalid duration",
            ErrorKind::DurationOverflow => "duration out of range",
        }
    }
}

/// An error with the span of the offending text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Error {
    /// What went wrong.
    pub kind: ErrorKind,
    /// Where, in bytes of the parsed text; empty for conversion errors.
    pub span: Span,
}

impl Error {
    /// Builds an error.
    pub const fn new(kind: ErrorKind, span: Span) -> Error {
        Error { kind, span }
    }

    pub(crate) fn at(kind: ErrorKind, start: usize, end: usize) -> Error {
        Error::new(kind, Span::new(start, end))
    }

    /// The registry code of [`Error::kind`].
    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.code(), self.kind.message())
    }
}

impl core::error::Error for Error {}
