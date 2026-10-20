use core::fmt;
use core::str::FromStr;

use crate::catalog::{self, Atom, PREFIXES};
use crate::dimension::{BaseDimension, Dimension};
use crate::error::{Error, ErrorKind, Span};
use crate::kind::QuantityKind;

/// The longest unit symbol a [`Unit`] can hold, in bytes.
pub const MAX_SYMBOL_LEN: usize = 32;

/// A unit symbol stored inline, so that [`Unit`] is `Copy` and never allocates.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Symbol {
    bytes: [u8; MAX_SYMBOL_LEN],
    len: u8,
}

impl Symbol {
    const fn from_static(s: &'static str) -> Symbol {
        let src = s.as_bytes();
        let mut bytes = [0u8; MAX_SYMBOL_LEN];
        let mut i = 0;
        while i < src.len() && i < MAX_SYMBOL_LEN {
            bytes[i] = src[i];
            i += 1;
        }
        Symbol {
            bytes,
            len: i as u8,
        }
    }

    fn new(s: &str) -> Option<Symbol> {
        let mut out = Symbol {
            bytes: [0; MAX_SYMBOL_LEN],
            len: 0,
        };
        out.push(s)?;
        Some(out)
    }

    fn push(&mut self, s: &str) -> Option<()> {
        let start = self.len as usize;
        let end = start.checked_add(s.len())?;
        self.bytes
            .get_mut(start..end)?
            .copy_from_slice(s.as_bytes());
        self.len = u8::try_from(end).ok()?;
        Some(())
    }

    fn as_str(&self) -> &str {
        let bytes = self.bytes.get(..self.len as usize).unwrap_or(&[]);
        core::str::from_utf8(bytes).unwrap_or("")
    }

    fn is_compound(&self) -> bool {
        self.as_str().contains(['/', '*', '\u{b7}'])
    }
}

/// A linear map between two units of the same dimension: `to = from * scale + offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conversion {
    /// Multiplier applied first.
    pub scale: f64,
    /// Added after the multiplier (non-zero only around `°C`).
    pub offset: f64,
}

impl Conversion {
    /// The identity conversion.
    pub const IDENTITY: Conversion = Conversion {
        scale: 1.0,
        offset: 0.0,
    };

    /// Applies the conversion to a value.
    pub fn apply(&self, value: f64) -> f64 {
        value * self.scale + self.offset
    }

    /// True when the conversion leaves every value unchanged.
    pub fn is_identity(&self) -> bool {
        self.scale == 1.0 && self.offset == 0.0
    }
}

/// A unit of the v1 catalogue: an atom, a prefixed atom or a compound of
/// them (docs/16 §5).
///
/// A unit reduces to a [`Dimension`], a factor and an offset to coherent SI,
/// plus an optional [`QuantityKind`]. Equality compares the reduced form,
/// not the spelling: `"kW"` equals `"kJ/s"` except for the kind.
#[derive(Clone, Copy)]
pub struct Unit {
    dimension: Dimension,
    factor: f64,
    offset: f64,
    kind: Option<QuantityKind>,
    symbol: Symbol,
}

impl PartialEq for Unit {
    fn eq(&self, other: &Unit) -> bool {
        self.dimension == other.dimension
            && self.factor == other.factor
            && self.offset == other.offset
            && self.kind == other.kind
    }
}

impl fmt::Debug for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Unit")
            .field("symbol", &self.symbol.as_str())
            .field("dimension", &self.dimension)
            .field("factor", &self.factor)
            .field("offset", &self.offset)
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol.as_str())
    }
}

impl FromStr for Unit {
    type Err = Error;

    fn from_str(s: &str) -> Result<Unit, Error> {
        Unit::parse(s)
    }
}

impl Unit {
    /// The dimensionless unit `1`.
    pub const ONE: Unit = Unit::from_atom_const("1", Dimension::NONE, 1.0, None);
    /// The second, `s`.
    pub const SECOND: Unit = Unit::from_atom_const(
        "s",
        Dimension::from_exponents([1, 0, 0, 0, 0, 0, 0, 0]),
        1.0,
        None,
    );
    /// The kelvin, `K`, which is also the unit of a `°C` difference.
    pub const KELVIN: Unit = Unit::from_atom_const(
        "K",
        Dimension::from_exponents([0, 0, 0, 0, 1, 0, 0, 0]),
        1.0,
        None,
    );
    /// The percent, `%`.
    pub const PERCENT: Unit = Unit::from_atom_const(
        "%",
        Dimension::from_exponents([0, 0, 0, 0, 0, 0, 0, 1]),
        1.0,
        None,
    );

    const fn from_atom_const(
        symbol: &'static str,
        dimension: Dimension,
        factor: f64,
        kind: Option<QuantityKind>,
    ) -> Unit {
        Unit {
            dimension,
            factor,
            offset: 0.0,
            kind,
            symbol: Symbol::from_static(symbol),
        }
    }

    /// Parses a unit: an atom of the catalogue, optionally prefixed and
    /// raised to a digit exponent (`m3`, `s2`), combined with `/`, `*` or
    /// `·` and grouped with parentheses, left to right (`m3/h/%`).
    ///
    /// A `°C` inside a compound is read as a temperature difference
    /// (`°C/min` is `K/min`, ref/01 §3.3).
    pub fn parse(text: &str) -> Result<Unit, Error> {
        if text.is_empty() {
            return Err(Error::at(ErrorKind::Empty, 0, 0));
        }
        if text.len() > MAX_SYMBOL_LEN {
            return Err(Error::at(ErrorKind::SymbolTooLong, 0, text.len()));
        }
        let mut parser = Parser { text, pos: 0 };
        let mut unit = parser.compound()?;
        if parser.pos != text.len() {
            return Err(Error::at(
                ErrorKind::InvalidUnitSyntax,
                parser.pos,
                text.len(),
            ));
        }
        unit.symbol =
            Symbol::new(text).ok_or(Error::at(ErrorKind::SymbolTooLong, 0, text.len()))?;
        Ok(unit)
    }

    /// The symbol as written (or as composed by [`Unit::checked_mul`] and friends).
    pub fn symbol(&self) -> &str {
        self.symbol.as_str()
    }

    /// The dimension.
    pub fn dimension(&self) -> Dimension {
        self.dimension
    }

    /// The factor to coherent SI.
    pub fn factor(&self) -> f64 {
        self.factor
    }

    /// The offset to coherent SI (only `°C` has one).
    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// The quantity kind, if the unit carries one.
    pub fn kind(&self) -> Option<QuantityKind> {
        self.kind
    }

    /// The same unit with a different quantity kind, for signals that declare one.
    pub fn with_kind(mut self, kind: Option<QuantityKind>) -> Unit {
        self.kind = kind;
        self
    }

    /// True for an affine scale (`°C`), whose sums and products are restricted
    /// (docs/16 §4).
    pub fn is_affine(&self) -> bool {
        self.offset != 0.0
    }

    /// True for the dimensionless `1` (and not for `%`).
    pub fn is_dimensionless(&self) -> bool {
        self.dimension.is_dimensionless()
    }

    /// True when the unit is a pure time unit (`s`, `ms`, `min`, `h`, `d`).
    pub fn is_time(&self) -> bool {
        self.dimension == Dimension::base(BaseDimension::Time) && self.kind.is_none()
    }

    /// The unit of a difference between two values in this unit: `K` for
    /// `°C`, the unit itself otherwise.
    pub fn difference(&self) -> Unit {
        if !self.is_affine() {
            return *self;
        }
        Unit {
            dimension: self.dimension,
            factor: self.factor,
            offset: 0.0,
            kind: Some(QuantityKind::TemperatureDifference),
            symbol: Symbol::from_static("K"),
        }
    }

    /// The fraction unit for a `%` unit (`%` → `1`, `%/s` → `1/s`), for
    /// bindings declared `as = "fraction"` (docs/02 §3). `None` if the unit
    /// is not linear in `%`.
    pub fn as_fraction(&self) -> Option<Unit> {
        let exps = self.dimension.exponents();
        if self.dimension.exponent(BaseDimension::Percent) != 1 || self.is_affine() {
            return None;
        }
        let mut stripped = exps;
        stripped[BaseDimension::Percent as usize] = 0;
        let mut symbol = Symbol::new("")?;
        let s = self.symbol.as_str();
        if s == "%" {
            symbol.push("1")?;
        } else if let Some(rest) = s.strip_prefix("%/") {
            symbol.push("1/")?;
            symbol.push(rest)?;
        } else {
            symbol.push("(")?;
            symbol.push(s)?;
            symbol.push(")/%")?;
        }
        Some(Unit {
            dimension: Dimension::from_exponents(stripped),
            factor: self.factor * 0.01,
            offset: 0.0,
            kind: self.kind,
            symbol,
        })
    }

    /// Checks that a value in `self` can be bound to or compared with `other`:
    /// same dimension (`J0107`) and, when both carry one, same kind (`J0108`).
    pub fn check_compatible(&self, other: &Unit) -> Result<(), Error> {
        if self.dimension != other.dimension {
            return Err(Error::new(
                ErrorKind::IncompatibleDimension,
                Span::default(),
            ));
        }
        match (self.kind, other.kind) {
            (Some(a), Some(b)) if !kinds_compatible(a, b) => {
                Err(Error::new(ErrorKind::IncompatibleKind, Span::default()))
            }
            _ => Ok(()),
        }
    }

    /// The linear map from values in `self` to values in `to`, after
    /// [`Unit::check_compatible`].
    pub fn conversion_to(&self, to: &Unit) -> Result<Conversion, Error> {
        self.check_compatible(to)?;
        Ok(Conversion {
            scale: self.factor / to.factor,
            offset: (self.offset - to.offset) / to.factor,
        })
    }

    /// Converts a value from `self` to `to`.
    pub fn convert(&self, value: f64, to: &Unit) -> Result<f64, Error> {
        Ok(self.conversion_to(to)?.apply(value))
    }

    /// The value in coherent SI units.
    pub fn to_si(&self, value: f64) -> f64 {
        value * self.factor + self.offset
    }

    /// The value in this unit, from coherent SI.
    pub fn from_si(&self, value: f64) -> f64 {
        (value - self.offset) / self.factor
    }

    /// The product of two units; affine operands count as differences.
    /// The result has no kind.
    pub fn checked_mul(&self, other: &Unit) -> Result<Unit, Error> {
        self.combine(other, Op::Mul)
    }

    /// The quotient of two units; affine operands count as differences.
    /// Dividing by a pure time unit keeps the kind (`kvar/s`), except that
    /// energy per time is power (`kWh/h`, `kJ/s`).
    pub fn checked_div(&self, other: &Unit) -> Result<Unit, Error> {
        self.combine(other, Op::Div)
    }

    /// The unit raised to an integer power; `°C` cannot be raised.
    pub fn checked_powi(&self, n: i8) -> Result<Unit, Error> {
        if n == 1 {
            return Ok(*self);
        }
        if n == 0 {
            return Ok(Unit::ONE);
        }
        if self.is_affine() {
            return Err(Error::new(ErrorKind::InvalidUnitSyntax, Span::default()));
        }
        let dimension = self
            .dimension
            .checked_pow(n)
            .ok_or(Error::new(ErrorKind::ExponentOverflow, Span::default()))?;
        let mut symbol = Symbol::new("").ok_or(too_long())?;
        if n < 0 {
            symbol.push("1/").ok_or(too_long())?;
        }
        let wrap = self.symbol.is_compound();
        if wrap {
            symbol.push("(").ok_or(too_long())?;
        }
        symbol.push(self.symbol.as_str()).ok_or(too_long())?;
        if wrap {
            symbol.push(")").ok_or(too_long())?;
        }
        if n.unsigned_abs() != 1 {
            symbol
                .push(itoa_u8(n.unsigned_abs()).as_str())
                .ok_or(too_long())?;
        }
        Ok(Unit {
            dimension,
            factor: powi(self.factor, n),
            offset: 0.0,
            kind: None,
            symbol,
        })
    }

    /// The square root of a unit whose dimension exponents are all even
    /// (`m2` → `m`); `°C` and odd exponents have no root.
    pub fn checked_sqrt(&self) -> Result<Unit, Error> {
        if self.is_affine() {
            return Err(Error::new(ErrorKind::InvalidUnitSyntax, Span::default()));
        }
        let mut exponents = self.dimension.exponents();
        for e in exponents.iter_mut() {
            if *e % 2 != 0 {
                return Err(Error::new(ErrorKind::InvalidUnitSyntax, Span::default()));
            }
            *e /= 2;
        }
        let mut symbol = Symbol::new("sqrt(").ok_or(too_long())?;
        symbol.push(self.symbol.as_str()).ok_or(too_long())?;
        symbol.push(")").ok_or(too_long())?;
        Ok(Unit {
            dimension: Dimension::from_exponents(exponents),
            factor: sqrt(self.factor),
            offset: 0.0,
            kind: None,
            symbol,
        })
    }

    fn combine(&self, other: &Unit, op: Op) -> Result<Unit, Error> {
        let lhs = self.difference();
        let rhs = other.difference();
        let dimension = match op {
            Op::Mul => lhs.dimension.checked_mul(rhs.dimension),
            Op::Div => lhs.dimension.checked_div(rhs.dimension),
        }
        .ok_or(Error::new(ErrorKind::ExponentOverflow, Span::default()))?;
        let factor = match op {
            Op::Mul => lhs.factor * rhs.factor,
            Op::Div => lhs.factor / rhs.factor,
        };
        let kind = match op {
            Op::Div if rhs.is_time() => lhs.kind.map(per_time),
            _ => None,
        };
        let mut symbol = Symbol::new(self.symbol.as_str()).ok_or(too_long())?;
        symbol
            .push(match op {
                Op::Mul => "*",
                Op::Div => "/",
            })
            .ok_or(too_long())?;
        let wrap = other.symbol.is_compound();
        if wrap {
            symbol.push("(").ok_or(too_long())?;
        }
        symbol.push(other.symbol.as_str()).ok_or(too_long())?;
        if wrap {
            symbol.push(")").ok_or(too_long())?;
        }
        Ok(Unit {
            dimension,
            factor,
            offset: 0.0,
            kind,
            symbol,
        })
    }

    fn from_atom(atom: &Atom, prefix: f64) -> Unit {
        Unit {
            dimension: atom.dimension,
            factor: atom.factor * prefix,
            offset: atom.offset,
            kind: atom.kind,
            symbol: Symbol::from_static(atom.symbol),
        }
    }
}

fn per_time(kind: QuantityKind) -> QuantityKind {
    match kind {
        QuantityKind::Energy => QuantityKind::ActivePower,
        QuantityKind::ReactiveEnergy => QuantityKind::ReactivePower,
        QuantityKind::Temperature => QuantityKind::TemperatureDifference,
        other => other,
    }
}

fn kinds_compatible(a: QuantityKind, b: QuantityKind) -> bool {
    a == b
}

fn too_long() -> Error {
    Error::new(ErrorKind::SymbolTooLong, Span::default())
}

fn sqrt(value: f64) -> f64 {
    if value <= 0.0 || !value.is_finite() {
        return value;
    }
    let mut x = if value > 1.0 { value / 2.0 } else { 1.0 };
    for _ in 0..128 {
        let next = 0.5 * (x + value / x);
        if next == x {
            break;
        }
        x = next;
    }
    x
}

fn powi(base: f64, n: i8) -> f64 {
    let mut out = 1.0;
    for _ in 0..n.unsigned_abs() {
        out *= base;
    }
    if n < 0 { 1.0 / out } else { out }
}

struct SmallStr {
    bytes: [u8; 3],
    len: usize,
}

impl SmallStr {
    fn as_str(&self) -> &str {
        core::str::from_utf8(self.bytes.get(..self.len).unwrap_or(&[])).unwrap_or("")
    }
}

fn itoa_u8(v: u8) -> SmallStr {
    let mut out = SmallStr {
        bytes: [0; 3],
        len: 0,
    };
    for (i, digit) in [v / 100, (v / 10) % 10, v % 10].into_iter().enumerate() {
        if digit == 0 && out.len == 0 && i < 2 {
            continue;
        }
        if let Some(slot) = out.bytes.get_mut(out.len) {
            *slot = b'0' + digit;
            out.len += 1;
        }
    }
    out
}

#[derive(Clone, Copy)]
enum Op {
    Mul,
    Div,
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        self.text.get(self.pos..).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn compound(&mut self) -> Result<Unit, Error> {
        let start = self.pos;
        let (mut unit, mut terms) = (self.term()?, 1u32);
        loop {
            let op = match self.peek() {
                Some('/') => Op::Div,
                Some('*') | Some('\u{b7}') => Op::Mul,
                _ => break,
            };
            self.pos += self.peek().map_or(1, char::len_utf8);
            let rhs = self.term()?;
            terms += 1;
            unit = unit.combine(&rhs, op).map_err(|e| Error {
                span: Span::new(start, self.pos),
                ..e
            })?;
        }
        if terms > 1 && unit.kind == Some(QuantityKind::Temperature) {
            unit.kind = Some(QuantityKind::TemperatureDifference);
        }
        Ok(unit)
    }

    fn term(&mut self) -> Result<Unit, Error> {
        let start = self.pos;
        let base = if self.peek() == Some('(') {
            self.pos += 1;
            let inner = self.compound()?;
            if self.peek() != Some(')') {
                return Err(Error::at(ErrorKind::InvalidUnitSyntax, start, self.pos));
            }
            self.pos += 1;
            inner
        } else {
            self.atom()?
        };
        let exp_start = self.pos;
        let digits = self.rest().bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return Ok(base);
        }
        self.pos += digits;
        let exp = self
            .text
            .get(exp_start..self.pos)
            .and_then(|d| d.parse::<i8>().ok())
            .filter(|e| *e >= 1)
            .ok_or(Error::at(ErrorKind::ExponentOverflow, exp_start, self.pos))?;
        if base.is_affine() || base.symbol.as_str() == "1" {
            return Err(Error::at(ErrorKind::InvalidUnitSyntax, start, self.pos));
        }
        let mut unit = base.checked_powi(exp).map_err(|e| Error {
            span: Span::new(start, self.pos),
            ..e
        })?;
        unit.kind = None;
        Ok(unit)
    }

    fn atom(&mut self) -> Result<Unit, Error> {
        let start = self.pos;
        let rest = self.rest();
        if rest.starts_with('1') {
            self.pos += 1;
            return Ok(Unit::ONE);
        }
        let len: usize = rest
            .char_indices()
            .find(|(_, c)| matches!(c, '/' | '*' | '\u{b7}' | '(' | ')') || c.is_ascii_digit())
            .map_or(rest.len(), |(i, _)| i);
        let name = rest.get(..len).unwrap_or("");
        let end = start + len;
        if name.is_empty() {
            return Err(Error::at(
                ErrorKind::InvalidUnitSyntax,
                start,
                end.max(start + 1).min(self.text.len()),
            ));
        }
        self.pos = end;
        if let Some(atom) = catalog::find_atom(name) {
            return Ok(Unit::from_atom(atom, 1.0));
        }
        for prefix in &PREFIXES {
            let Some(base) = name.strip_prefix(prefix.symbol) else {
                continue;
            };
            if let Some(atom) = catalog::find_atom(base) {
                if !atom.prefixable {
                    return Err(Error::at(ErrorKind::PrefixNotAllowed, start, end));
                }
                return Ok(Unit::from_atom(atom, prefix.factor));
            }
        }
        Err(Error::at(ErrorKind::UnknownUnit, start, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Unit {
        Unit::parse(s).unwrap()
    }

    #[test]
    fn atoms_and_prefixes() {
        assert_eq!(u("kW").factor(), 1e3);
        assert_eq!(u("kW").kind(), Some(QuantityKind::ActivePower));
        assert_eq!(u("MWh").factor(), 3.6e9);
        assert_eq!(u("µA").factor(), 1e-6);
        assert_eq!(u("\u{3bc}A").factor(), 1e-6);
        assert_eq!(u("kg").factor(), 1.0);
        assert_eq!(u("min").factor(), 60.0);
        assert_eq!(u("mol").dimension(), Dimension::base(BaseDimension::Amount));
        assert_eq!(u("mm").factor(), 1e-3);
        assert_eq!(u("kΩ").factor(), 1e3);
        assert_eq!(u("kvarh").kind(), Some(QuantityKind::ReactiveEnergy));
    }

    #[test]
    fn exclusions() {
        for s in ["k°C", "m%", "krpm", "mgn"] {
            let e = Unit::parse(s).unwrap_err();
            assert_eq!(e.kind, ErrorKind::PrefixNotAllowed, "{s}");
            assert_eq!(e.code(), "J0109");
        }
        assert_eq!(Unit::parse("k1").unwrap_err().code(), "J0109");
        for s in ["ft", "psi", "kWW", "Ohm", "cW", "dam", "°F", "kw"] {
            let e = Unit::parse(s).unwrap_err();
            assert_eq!(e.kind, ErrorKind::UnknownUnit, "{s}");
            assert_eq!(e.code(), "J0109");
        }
    }

    #[test]
    fn compounds() {
        let v = u("m3/h");
        assert_eq!(
            v.dimension(),
            Dimension::from_exponents([-1, 3, 0, 0, 0, 0, 0, 0])
        );
        assert!((v.factor() - 1.0 / 3600.0).abs() < 1e-18);
        assert_eq!(u("kW/s").kind(), Some(QuantityKind::ActivePower));
        assert_eq!(u("kW/m2").kind(), None);
        assert_eq!(u("m/s2"), u("m/(s*s)"));
        assert_eq!(u("m3/h/%").dimension().exponent(BaseDimension::Percent), -1);
        let r = u("°C/min");
        assert!(!r.is_affine());
        assert_eq!(r.kind(), Some(QuantityKind::TemperatureDifference));
        assert_eq!(u("V·A").dimension(), u("VA").dimension());
        assert_eq!(u("1/s").dimension(), u("Hz").dimension());
    }

    #[test]
    fn malformed() {
        for s in ["kW/", "/s", "(kW", "kW)", "°C2", "m0", "m 3", "k W"] {
            assert!(Unit::parse(s).is_err(), "{s}");
        }
        assert_eq!(Unit::parse("").unwrap_err().kind, ErrorKind::Empty);
        let long = "kW/kW/kW/kW/kW/kW/kW/kW/kW/kW/kW/kW";
        assert_eq!(
            Unit::parse(long).unwrap_err().kind,
            ErrorKind::SymbolTooLong
        );
    }

    #[test]
    fn spans_point_at_the_bad_atom() {
        let e = Unit::parse("kW/foo").unwrap_err();
        assert_eq!(e.span.range(), 3..6);
    }

    #[test]
    fn compatibility() {
        assert!(u("W").check_compatible(&u("kW")).is_ok());
        assert_eq!(
            u("W").check_compatible(&u("var")).unwrap_err().kind,
            ErrorKind::IncompatibleKind
        );
        assert_eq!(
            u("W").check_compatible(&u("var")).unwrap_err().code(),
            "J0108"
        );
        assert_eq!(
            u("W").check_compatible(&u("Wh")).unwrap_err().code(),
            "J0107"
        );
        assert_eq!(
            u("%").check_compatible(&Unit::ONE).unwrap_err().code(),
            "J0107"
        );
        assert!(u("kW").check_compatible(&u("kJ/s")).is_ok());
        assert!(u("J").check_compatible(&u("Nm")).is_ok());
    }

    #[test]
    fn fractions() {
        let f = u("%").as_fraction().unwrap();
        assert_eq!(f.dimension(), Dimension::NONE);
        assert_eq!(f.factor(), 0.01);
        assert_eq!(f.symbol(), "1");
        assert_eq!(u("%/s").as_fraction().unwrap().symbol(), "1/s");
        assert!(u("kW").as_fraction().is_none());
    }

    #[test]
    fn affine_difference() {
        let c = u("°C");
        assert!(c.is_affine());
        let d = c.difference();
        assert!(!d.is_affine());
        assert_eq!(d.kind(), Some(QuantityKind::TemperatureDifference));
        assert_eq!(d.symbol(), "K");
        assert_eq!(u("kW").difference(), u("kW"));
    }

    #[test]
    fn composed_symbols() {
        let kw = u("kW");
        assert_eq!(kw.checked_div(&Unit::SECOND).unwrap().symbol(), "kW/s");
        assert_eq!(kw.checked_mul(&u("h")).unwrap().symbol(), "kW*h");
        assert_eq!(kw.checked_div(&u("m/s")).unwrap().symbol(), "kW/(m/s)");
        assert_eq!(u("m/s").checked_powi(2).unwrap().symbol(), "(m/s)2");
        assert_eq!(u("m").checked_powi(-1).unwrap().symbol(), "1/m");
        assert_eq!(u("s").checked_powi(-2).unwrap().symbol(), "1/s2");
        assert_eq!(u("1/s2"), u("s").checked_powi(-2).unwrap());
        assert_eq!(kw.checked_mul(&u("h")).unwrap().kind(), None);
        assert!(u("°C").checked_powi(2).is_err());
        let side = u("km2").checked_sqrt().unwrap();
        assert_eq!(side.dimension(), u("m").dimension());
        assert_eq!(side.factor(), 1000.0);
        assert_eq!(side.symbol(), "sqrt(km2)");
        assert_eq!(u("1").checked_sqrt().unwrap().factor(), 1.0);
        assert!(u("m3").checked_sqrt().is_err());
        assert!(u("°C").checked_sqrt().is_err());
        assert_eq!(u("mm2").checked_sqrt().unwrap().factor(), 1e-3);
    }
}
