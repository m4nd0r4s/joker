use core::fmt;
use core::str::FromStr;

/// A quantity kind: tells apart units that share a dimension (docs/16 §5).
///
/// `W` and `var` have the same dimension but binding one to the other is
/// `J0108`. A unit without a kind (`K`, `Nm`, most compounds) is checked by
/// dimension only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QuantityKind {
    /// `W` and its prefixed forms.
    ActivePower,
    /// `var` and its prefixed forms.
    ReactivePower,
    /// `VA` and its prefixed forms.
    ApparentPower,
    /// `Wh` and `J`.
    Energy,
    /// `varh`.
    ReactiveEnergy,
    /// An absolute temperature (`°C`, or `K` when a signal declares it).
    Temperature,
    /// A temperature difference (`K` against `°C`, `°C − °C`).
    TemperatureDifference,
}

impl QuantityKind {
    /// Every kind, in declaration order.
    pub const ALL: [QuantityKind; 7] = [
        QuantityKind::ActivePower,
        QuantityKind::ReactivePower,
        QuantityKind::ApparentPower,
        QuantityKind::Energy,
        QuantityKind::ReactiveEnergy,
        QuantityKind::Temperature,
        QuantityKind::TemperatureDifference,
    ];

    /// The `snake_case` name used in TOML and JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            QuantityKind::ActivePower => "active_power",
            QuantityKind::ReactivePower => "reactive_power",
            QuantityKind::ApparentPower => "apparent_power",
            QuantityKind::Energy => "energy",
            QuantityKind::ReactiveEnergy => "reactive_energy",
            QuantityKind::Temperature => "temperature",
            QuantityKind::TemperatureDifference => "temperature_difference",
        }
    }
}

impl fmt::Display for QuantityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The name is not one of the quantity kinds of docs/16 §5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownQuantityKind;

impl fmt::Display for UnknownQuantityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown quantity kind")
    }
}

impl FromStr for QuantityKind {
    type Err = UnknownQuantityKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        QuantityKind::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or(UnknownQuantityKind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for k in QuantityKind::ALL {
            assert_eq!(k.as_str().parse::<QuantityKind>(), Ok(k));
        }
        assert!("power".parse::<QuantityKind>().is_err());
    }
}
