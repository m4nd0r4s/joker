use core::fmt;

use joker_units::Unit;

/// The type of a signal, as declared in the registry (ref/00 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalType {
    /// `bool`.
    Bool,
    /// `i64`.
    I64,
    /// `u64`.
    U64,
    /// `f64`.
    F64,
    /// `f32`, promoted to `f64`.
    F32,
    /// `enum<N>`, identified by the registry's enum id.
    Enum(EnumId),
    /// `bits<N>`, read as an unsigned word.
    Bits,
    /// `string`; not usable in expressions.
    String,
    /// `bytes`; not usable in expressions.
    Bytes,
    /// `record`; not usable in expressions.
    Record,
}

/// The identifier of an enum type in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnumId(pub u32);

/// What the checker needs to know about a signal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalInfo {
    /// The dense `SignalId` of this generation.
    pub id: u32,
    /// The declared type.
    pub ty: SignalType,
    /// The declared unit, with the declared quantity kind if any; `None`
    /// for a signal without a unit, which counts as dimensionless.
    pub unit: Option<Unit>,
}

/// The registry as seen by the checker: it resolves paths, enum variants
/// and selectors. The manifest implements it over the generation being
/// built.
pub trait Env {
    /// The signal at a canonical path or alias, if any.
    fn signal(&self, path: &str) -> Option<SignalInfo>;

    /// The enum type and numeric value of `Enum.variant`, if both exist.
    fn enum_variant(&self, enum_name: &str, variant: &str) -> Option<(EnumId, u64)>;

    /// Calls `found` for every signal a selector matches, deduplicated by
    /// `SignalId` and in the lexicographic order of canonical paths
    /// (ref/01 §3.9).
    fn select(&self, selector: &str, found: &mut dyn FnMut(SignalInfo));
}

/// How a number is represented at runtime. Arithmetic is always `f64`;
/// comparisons between two integers of the same signedness are exact
/// (docs/16 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Repr {
    /// `f64`.
    Float,
    /// `i64`.
    Signed,
    /// `u64`.
    Unsigned,
}

/// The static type of a sub-expression.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ty {
    /// A boolean.
    Bool,
    /// A number in a unit (`1` when dimensionless).
    Number {
        /// The unit of the value.
        unit: Unit,
        /// The runtime representation.
        repr: Repr,
    },
    /// A variant of one enum type.
    Enum(EnumId),
}

impl Ty {
    /// A dimensionless float.
    pub const SCALAR: Ty = Ty::Number {
        unit: Unit::ONE,
        repr: Repr::Float,
    };

    /// The unit, for numbers.
    pub fn unit(&self) -> Option<Unit> {
        match self {
            Ty::Number { unit, .. } => Some(*unit),
            _ => None,
        }
    }

    /// The type of a signal in an expression; `None` for types that
    /// expressions cannot use (`string`, `bytes`, `record`).
    pub fn of_signal(info: &SignalInfo) -> Option<Ty> {
        let unit = info.unit.unwrap_or(Unit::ONE);
        let number = |repr| Ty::Number { unit, repr };
        Some(match info.ty {
            SignalType::Bool => Ty::Bool,
            SignalType::I64 => number(Repr::Signed),
            SignalType::U64 | SignalType::Bits => number(Repr::Unsigned),
            SignalType::F64 | SignalType::F32 => number(Repr::Float),
            SignalType::Enum(id) => Ty::Enum(id),
            SignalType::String | SignalType::Bytes | SignalType::Record => return None,
        })
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Bool => f.write_str("bool"),
            Ty::Number { unit, .. } if unit.is_dimensionless() => {
                f.write_str("dimensionless number")
            }
            Ty::Number { unit, .. } => write!(f, "number in `{unit}`"),
            Ty::Enum(_) => f.write_str("enum"),
        }
    }
}
