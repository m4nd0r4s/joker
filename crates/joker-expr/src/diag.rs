use core::fmt;

use joker_units::{Span, Unit};

use crate::types::Ty;

/// What is wrong with an expression. Each kind maps to one registry code
/// (docs/16 §9, ref/01 §3.7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErrorKind {
    /// A character that starts no token.
    UnexpectedChar(char),
    /// A number followed by text that is neither a unit nor an operator.
    MalformedLiteral,
    /// An enum variant not of the form `Enum.variant`.
    MalformedEnumVariant,
    /// Nothing but whitespace.
    EmptyExpression,
    /// A token where it cannot appear.
    UnexpectedToken,
    /// The expression ends too early.
    UnexpectedEnd,
    /// A missing `)`.
    ExpectedCloseParen,
    /// A missing `then`.
    ExpectedThen,
    /// A missing `else`.
    ExpectedElse,
    /// `a < b < c`: comparisons do not chain.
    ChainedComparison,
    /// A dotted name before `(`.
    InvalidFunctionName,
    /// A selector outside the argument list of a call.
    SelectorOutsideCall,
    /// A path that breaks the grammar of ref/01 §3.8.
    InvalidPath,
    /// A selector that breaks the grammar of ref/01 §3.9.
    InvalidSelector,
    /// The source is longer than [`crate::limits::MAX_SOURCE_LEN`].
    TooLong,
    /// The expression has more nodes or arguments than the fixed capacities.
    TooComplex,
    /// The expression nests deeper than [`crate::limits::MAX_DEPTH`].
    TooDeep,
    /// A call with more than [`crate::limits::MAX_CALL_ARGS`] arguments.
    TooManyArguments,
    /// A selector that expands to more than [`crate::limits::MAX_ELEMENTS`] signals.
    TooManyElements,
    /// A malformed quantity or duration literal.
    Literal(joker_units::ErrorKind),
    /// Operands whose dimensions do not match.
    IncompatibleUnits {
        /// Unit of the left operand.
        lhs: Unit,
        /// Unit of the right operand.
        rhs: Unit,
    },
    /// Operands with the same dimension and different quantity kinds.
    IncompatibleKinds {
        /// Unit of the left operand.
        lhs: Unit,
        /// Unit of the right operand.
        rhs: Unit,
    },
    /// An operation that an affine unit (`°C`) does not support.
    AffineOperation {
        /// The affine unit.
        unit: Unit,
    },
    /// A power or root that does not give whole dimension exponents.
    FractionalDimension {
        /// The unit of the base.
        unit: Unit,
    },
    /// A path that names no signal.
    UnknownSignal,
    /// An enum or variant that does not exist.
    UnknownEnumVariant,
    /// An operand of the wrong type.
    TypeMismatch {
        /// What the position needs.
        expected: Expected,
        /// What the operand is.
        found: Ty,
    },
    /// Two enum operands of different enum types.
    EnumMismatch,
    /// A signal whose type expressions cannot read (`string`, `bytes`, `record`).
    UnsupportedSignalType,
    /// A call to a function that does not exist.
    UnknownFunction,
    /// A call with the wrong number of arguments.
    WrongArity {
        /// Fewest arguments accepted.
        min: u8,
        /// Most arguments accepted.
        max: u8,
    },
    /// A selector passed to a function that is not an aggregation.
    SelectorNotAllowed,
    /// An argument that must be a duration literal.
    ExpectedDuration,
    /// An argument that must be a constant.
    ExpectedConstant,
    /// An argument that must be a signal path.
    ExpectedSignal,
    /// A window above 15 minutes.
    WindowTooLong,
    /// A window or hold time of zero.
    ZeroDuration,
    /// `own()` outside a class method.
    OwnOutsideMethod,
    /// A selector that matches no signal.
    EmptySelection,
}

/// What a position expects, for [`ErrorKind::TypeMismatch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expected {
    /// A boolean.
    Bool,
    /// A number.
    Number,
    /// A number or an enum, for an ordering comparison.
    Ordered,
    /// An integer-valued word, for `bit`.
    Integer,
    /// Operands of the same type.
    SameType,
}

impl fmt::Display for Expected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Expected::Bool => "bool",
            Expected::Number => "a number",
            Expected::Ordered => "a number",
            Expected::Integer => "an integer",
            Expected::SameType => "the same type on both sides",
        })
    }
}

impl ErrorKind {
    /// The registry code.
    pub fn code(&self) -> &'static str {
        match self {
            ErrorKind::UnexpectedChar(_)
            | ErrorKind::MalformedLiteral
            | ErrorKind::MalformedEnumVariant
            | ErrorKind::EmptyExpression
            | ErrorKind::UnexpectedToken
            | ErrorKind::UnexpectedEnd
            | ErrorKind::ExpectedCloseParen
            | ErrorKind::ExpectedThen
            | ErrorKind::ExpectedElse
            | ErrorKind::ChainedComparison
            | ErrorKind::InvalidFunctionName
            | ErrorKind::SelectorOutsideCall => "J0158",
            ErrorKind::InvalidPath => "J0153",
            ErrorKind::InvalidSelector => "J0154",
            ErrorKind::TooLong
            | ErrorKind::TooComplex
            | ErrorKind::TooDeep
            | ErrorKind::TooManyArguments
            | ErrorKind::TooManyElements => "J0149",
            ErrorKind::Literal(kind) => kind.code(),
            ErrorKind::IncompatibleUnits { .. }
            | ErrorKind::AffineOperation { .. }
            | ErrorKind::FractionalDimension { .. } => "J0301",
            ErrorKind::IncompatibleKinds { .. } => "J0108",
            ErrorKind::UnknownSignal => "J0302",
            ErrorKind::UnknownEnumVariant => "J0303",
            ErrorKind::TypeMismatch { .. }
            | ErrorKind::EnumMismatch
            | ErrorKind::UnsupportedSignalType => "J0304",
            ErrorKind::WindowTooLong => "J0305",
            ErrorKind::ZeroDuration => "J0152",
            ErrorKind::OwnOutsideMethod => "J0306",
            ErrorKind::UnknownFunction
            | ErrorKind::WrongArity { .. }
            | ErrorKind::SelectorNotAllowed
            | ErrorKind::ExpectedDuration
            | ErrorKind::ExpectedConstant
            | ErrorKind::ExpectedSignal => "J0307",
            ErrorKind::EmptySelection => "J0155",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::UnexpectedChar(c) => write!(f, "unexpected character `{c}`"),
            ErrorKind::MalformedLiteral => f.write_str("malformed literal"),
            ErrorKind::MalformedEnumVariant => {
                f.write_str("malformed enum variant, expected `Enum.variant`")
            }
            ErrorKind::EmptyExpression => f.write_str("empty expression"),
            ErrorKind::UnexpectedToken => f.write_str("unexpected token"),
            ErrorKind::UnexpectedEnd => f.write_str("unexpected end of expression"),
            ErrorKind::ExpectedCloseParen => f.write_str("expected `)`"),
            ErrorKind::ExpectedThen => f.write_str("expected `then`"),
            ErrorKind::ExpectedElse => f.write_str("expected `else`"),
            ErrorKind::ChainedComparison => f.write_str("comparisons cannot be chained"),
            ErrorKind::InvalidFunctionName => f.write_str("a function name has no dots"),
            ErrorKind::SelectorOutsideCall => {
                f.write_str("a selector is only valid as an aggregation argument")
            }
            ErrorKind::InvalidPath => f.write_str("invalid path"),
            ErrorKind::InvalidSelector => f.write_str("invalid selector"),
            ErrorKind::TooLong => f.write_str("expression longer than 4 KiB"),
            ErrorKind::TooComplex => f.write_str("expression too large"),
            ErrorKind::TooDeep => f.write_str("expression nested too deeply"),
            ErrorKind::TooManyArguments => f.write_str("too many arguments"),
            ErrorKind::TooManyElements => f.write_str("selector matches too many signals"),
            ErrorKind::Literal(kind) => f.write_str(kind.message()),
            ErrorKind::IncompatibleUnits { lhs, rhs } => {
                write!(f, "incompatible dimensions: `{lhs}` and `{rhs}`")
            }
            ErrorKind::IncompatibleKinds { lhs, rhs } => write!(
                f,
                "incompatible quantity kinds: `{lhs}` ({}) and `{rhs}` ({})",
                kind_name(lhs),
                kind_name(rhs)
            ),
            ErrorKind::AffineOperation { unit } => write!(
                f,
                "`{unit}` is an affine scale: only differences and shifts by a difference are allowed"
            ),
            ErrorKind::FractionalDimension { unit } => {
                write!(f, "`{unit}` raised to this power has no unit")
            }
            ErrorKind::UnknownSignal => f.write_str("unknown signal"),
            ErrorKind::UnknownEnumVariant => f.write_str("unknown enum variant"),
            ErrorKind::TypeMismatch { expected, found } => {
                write!(f, "expected {expected}, found {found}")
            }
            ErrorKind::EnumMismatch => f.write_str("variants of different enum types"),
            ErrorKind::UnsupportedSignalType => {
                f.write_str("expressions cannot read string, bytes or record signals")
            }
            ErrorKind::UnknownFunction => f.write_str("unknown function"),
            ErrorKind::WrongArity { min, max } if min == max => {
                write!(f, "expected {min} argument(s)")
            }
            ErrorKind::WrongArity { min, max } if *max == u8::MAX => {
                write!(f, "expected at least {min} argument(s)")
            }
            ErrorKind::WrongArity { min, max } => {
                write!(f, "expected {min} to {max} arguments")
            }
            ErrorKind::SelectorNotAllowed => {
                f.write_str("only any, all, count, sum, maxs and mins take a selector")
            }
            ErrorKind::ExpectedDuration => f.write_str("expected a duration literal"),
            ErrorKind::ExpectedConstant => f.write_str("expected a constant"),
            ErrorKind::ExpectedSignal => f.write_str("expected a signal path"),
            ErrorKind::WindowTooLong => f.write_str("window above the maximum (15 min)"),
            ErrorKind::ZeroDuration => f.write_str("duration must be greater than zero"),
            ErrorKind::OwnOutsideMethod => f.write_str("`own()` outside a class method"),
            ErrorKind::EmptySelection => f.write_str("selector matches no signal"),
        }
    }
}

fn kind_name(unit: &Unit) -> &'static str {
    unit.kind().map_or("no kind", |k| k.as_str())
}

/// A compile-time error with the span of the offending text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Diagnostic {
    /// What is wrong.
    pub kind: ErrorKind,
    /// Where, in bytes of the expression source.
    pub span: Span,
}

impl Diagnostic {
    /// Builds a diagnostic.
    pub const fn new(kind: ErrorKind, span: Span) -> Diagnostic {
        Diagnostic { kind, span }
    }

    /// The registry code.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// Writes the diagnostic in rustc style against its source:
    ///
    /// ```text
    /// error[J0301]: incompatible dimensions: `W` and `Hz`
    ///  --> outputs."pcs.p_set".interlock:1:11
    ///   |
    /// 1 | meter.p > 50 Hz
    ///   |           ^^^^^
    /// ```
    pub fn render(&self, src: &str, origin: &str, out: &mut dyn fmt::Write) -> fmt::Result {
        let start = (self.span.start as usize).min(src.len());
        let end = (self.span.end as usize).clamp(start, src.len());
        let line_start = src
            .get(..start)
            .and_then(|s| s.rfind('\n'))
            .map_or(0, |i| i + 1);
        let line_end = src
            .get(start..)
            .and_then(|s| s.find('\n'))
            .map_or(src.len(), |i| start + i);
        let line_no = src.get(..start).map_or(0, |s| s.matches('\n').count()) + 1;
        let line = src.get(line_start..line_end).unwrap_or("");
        let col = src.get(line_start..start).map_or(0, |s| s.chars().count());
        let width = src
            .get(start..end.min(line_end))
            .map_or(0, |s| s.chars().count())
            .max(1);
        let mut digits = 1;
        let mut n = line_no;
        while n >= 10 {
            n /= 10;
            digits += 1;
        }
        writeln!(out, "error[{}]: {}", self.code(), self.kind)?;
        writeln!(out, "{:digits$}--> {origin}:{line_no}:{}", "", col + 1)?;
        writeln!(out, "{:digits$} |", "")?;
        writeln!(out, "{line_no} | {line}")?;
        write!(out, "{:digits$} | {:col$}", "", "")?;
        for _ in 0..width {
            out.write_char('^')?;
        }
        out.write_char('\n')
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]: {}", self.code(), self.kind)
    }
}

impl core::error::Error for Diagnostic {}
