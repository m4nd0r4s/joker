use crate::dimension::Dimension;
use crate::kind::QuantityKind;

/// One unit symbol of the v1 catalogue (docs/16 §5), before prefixes and
/// compounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Atom {
    /// The symbol as written (`"kWh"` is the prefix `k` on the atom `Wh`).
    pub symbol: &'static str,
    /// Exponents in `[T, L, M, I, Θ, N, J, %]` order.
    pub dimension: Dimension,
    /// Multiply by this to get the coherent SI value (`kg`, not `g`).
    pub factor: f64,
    /// Add this after the factor to get the SI value (`273.15` for `°C`).
    pub offset: f64,
    /// The quantity kind the atom carries, if any.
    pub kind: Option<QuantityKind>,
    /// Whether the SI prefixes of [`PREFIXES`] apply.
    pub prefixable: bool,
}

/// An SI prefix accepted by the catalogue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prefix {
    /// The prefix as written.
    pub symbol: &'static str,
    /// Its multiplier.
    pub factor: f64,
}

/// The prefixes of docs/16 §5. `µ` is accepted both as U+00B5 (micro sign)
/// and U+03BC (Greek mu).
pub const PREFIXES: [Prefix; 6] = [
    Prefix {
        symbol: "\u{b5}",
        factor: 1e-6,
    },
    Prefix {
        symbol: "\u{3bc}",
        factor: 1e-6,
    },
    Prefix {
        symbol: "m",
        factor: 1e-3,
    },
    Prefix {
        symbol: "k",
        factor: 1e3,
    },
    Prefix {
        symbol: "M",
        factor: 1e6,
    },
    Prefix {
        symbol: "G",
        factor: 1e9,
    },
];

const fn dim(exponents: [i8; 8]) -> Dimension {
    Dimension::from_exponents(exponents)
}

const fn atom(
    symbol: &'static str,
    dimension: Dimension,
    factor: f64,
    kind: Option<QuantityKind>,
    prefixable: bool,
) -> Atom {
    Atom {
        symbol,
        dimension,
        factor,
        offset: 0.0,
        kind,
        prefixable,
    }
}

const POWER: Dimension = dim([-3, 2, 1, 0, 0, 0, 0, 0]);
const ENERGY: Dimension = dim([-2, 2, 1, 0, 0, 0, 0, 0]);

/// The atoms of the v1 catalogue. Compounds (`m3/h`, `kW/s`) are built by
/// the unit parser from these.
pub const ATOMS: [Atom; 31] = [
    atom("s", dim([1, 0, 0, 0, 0, 0, 0, 0]), 1.0, None, true),
    atom("m", dim([0, 1, 0, 0, 0, 0, 0, 0]), 1.0, None, true),
    atom("g", dim([0, 0, 1, 0, 0, 0, 0, 0]), 1e-3, None, true),
    atom("A", dim([0, 0, 0, 1, 0, 0, 0, 0]), 1.0, None, true),
    atom("K", dim([0, 0, 0, 0, 1, 0, 0, 0]), 1.0, None, true),
    atom("mol", dim([0, 0, 0, 0, 0, 1, 0, 0]), 1.0, None, true),
    atom("cd", dim([0, 0, 0, 0, 0, 0, 1, 0]), 1.0, None, true),
    atom("1", Dimension::NONE, 1.0, None, false),
    atom("W", POWER, 1.0, Some(QuantityKind::ActivePower), true),
    atom("Wh", ENERGY, 3600.0, Some(QuantityKind::Energy), true),
    atom("J", ENERGY, 1.0, Some(QuantityKind::Energy), true),
    atom("V", dim([-3, 2, 1, -1, 0, 0, 0, 0]), 1.0, None, true),
    atom("\u{3a9}", dim([-3, 2, 1, -2, 0, 0, 0, 0]), 1.0, None, true),
    atom("\u{2126}", dim([-3, 2, 1, -2, 0, 0, 0, 0]), 1.0, None, true),
    atom("Hz", dim([-1, 0, 0, 0, 0, 0, 0, 0]), 1.0, None, true),
    atom("var", POWER, 1.0, Some(QuantityKind::ReactivePower), true),
    atom(
        "varh",
        ENERGY,
        3600.0,
        Some(QuantityKind::ReactiveEnergy),
        true,
    ),
    atom("VA", POWER, 1.0, Some(QuantityKind::ApparentPower), true),
    atom("Pa", dim([-2, -1, 1, 0, 0, 0, 0, 0]), 1.0, None, true),
    atom("bar", dim([-2, -1, 1, 0, 0, 0, 0, 0]), 1e5, None, true),
    atom("N", dim([-2, 1, 1, 0, 0, 0, 0, 0]), 1.0, None, true),
    atom("Nm", ENERGY, 1.0, None, true),
    atom("lx", dim([0, -2, 0, 0, 0, 0, 1, 0]), 1.0, None, true),
    Atom {
        symbol: "\u{b0}C",
        dimension: dim([0, 0, 0, 0, 1, 0, 0, 0]),
        factor: 1.0,
        offset: 273.15,
        kind: Some(QuantityKind::Temperature),
        prefixable: false,
    },
    atom("%", dim([0, 0, 0, 0, 0, 0, 0, 1]), 1.0, None, false),
    atom(
        "rpm",
        dim([-1, 0, 0, 0, 0, 0, 0, 0]),
        1.0 / 60.0,
        None,
        false,
    ),
    atom("l", dim([0, 3, 0, 0, 0, 0, 0, 0]), 1e-3, None, true),
    atom("h", dim([1, 0, 0, 0, 0, 0, 0, 0]), 3600.0, None, true),
    atom("min", dim([1, 0, 0, 0, 0, 0, 0, 0]), 60.0, None, true),
    atom("d", dim([1, 0, 0, 0, 0, 0, 0, 0]), 86400.0, None, true),
    atom("gn", dim([-2, 1, 0, 0, 0, 0, 0, 0]), 9.80665, None, false),
];

/// Looks up an atom by its exact symbol.
pub fn find_atom(symbol: &str) -> Option<&'static Atom> {
    ATOMS.iter().find(|a| a.symbol == symbol)
}
