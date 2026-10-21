//! Conversion table of the v1 catalogue (docs/13 §4, task 2).
//!
//! Each row states `1 <from> = <value> <to>`. The values come from the UCUM
//! definitions (`ucum-essence.xml`: `bar`, `l`, `h`, `d`, `min`, `[g]`,
//! `Cel`, `Ohm`, `Hz`, `Pa`, `N`, `J`, `W`, `V`, `lx`) and from the SI
//! brochure, not from this crate.

#![allow(clippy::unwrap_used, clippy::panic)]

use joker_units::{Dimension, ErrorKind, Quantity, QuantityKind, Unit};

const TABLE: &[(&str, f64, &str)] = &[
    ("kW", 1000.0, "W"),
    ("MW", 1000.0, "kW"),
    ("GW", 1e9, "W"),
    ("mW", 1e-3, "W"),
    ("µW", 1e-6, "W"),
    ("kWh", 3.6e6, "J"),
    ("Wh", 3600.0, "J"),
    ("MWh", 1000.0, "kWh"),
    ("kvarh", 1000.0, "varh"),
    ("Mvar", 1000.0, "kvar"),
    ("kVA", 1000.0, "VA"),
    ("bar", 1e5, "Pa"),
    ("mbar", 100.0, "Pa"),
    ("kPa", 0.01, "bar"),
    ("MPa", 10.0, "bar"),
    ("l", 1e-3, "m3"),
    ("ml", 1e-6, "m3"),
    ("m3", 1000.0, "l"),
    ("m3/h", 1000.0 / 60.0, "l/min"),
    ("l/min", 0.06, "m3/h"),
    ("m3/h", 1.0 / 3600.0, "m3/s"),
    ("h", 3600.0, "s"),
    ("min", 60.0, "s"),
    ("d", 24.0, "h"),
    ("ms", 1e-3, "s"),
    ("µs", 1e-3, "ms"),
    ("kg", 1000.0, "g"),
    ("mg", 1e-3, "g"),
    ("km", 1000.0, "m"),
    ("m2", 1e6, "mm2"),
    ("km/h", 1.0 / 3.6, "m/s"),
    ("gn", 9.80665, "m/s2"),
    ("rpm", 1.0 / 60.0, "Hz"),
    ("Hz", 60.0, "rpm"),
    ("kHz", 1000.0, "1/s"),
    ("Nm", 1.0, "J"),
    ("kNm", 1000.0, "N*m"),
    ("N", 1.0, "kg*m/s2"),
    ("Pa", 1.0, "N/m2"),
    ("J", 1.0, "N*m"),
    ("W", 1.0, "J/s"),
    ("kW", 3600.0, "kJ/h"),
    ("kWh/h", 1.0, "kW"),
    ("V", 1.0, "W/A"),
    ("Ω", 1.0, "V/A"),
    ("kΩ", 1000.0, "Ω"),
    ("Ω", 1.0, "\u{2126}"),
    ("mA", 1e-3, "A"),
    ("kV", 1000.0, "V"),
    ("lx", 1.0, "cd/m2"),
    ("mmol", 1e-3, "mol"),
    ("kW/s", 60.0, "kW/min"),
    ("kW/min", 60.0, "kW/h"),
    ("%/s", 60.0, "%/min"),
    ("°C/min", 1.0 / 60.0, "K/s"),
    ("°C/h", 1.0, "K/h"),
    ("m3/h/%", 1.0, "m3/h/%"),
    ("mK", 1e-3, "K"),
];

fn unit(s: &str) -> Unit {
    Unit::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
}

fn close(a: f64, b: f64) -> bool {
    let scale = a.abs().max(b.abs()).max(1e-300);
    (a - b).abs() / scale < 1e-12
}

#[test]
fn conversion_table() {
    for (from, value, to) in TABLE {
        let got = unit(from).convert(1.0, &unit(to)).unwrap();
        assert!(
            close(got, *value),
            "1 {from} = {got} {to}, expected {value}"
        );
        let back = unit(to).convert(*value, &unit(from)).unwrap();
        assert!(close(back, 1.0), "{value} {to} = {back} {from}, expected 1");
    }
}

#[test]
fn temperatures_are_affine() {
    let c = unit("°C");
    let k = unit("K");
    for (celsius, kelvin) in [
        (0.0, 273.15),
        (-273.15, 0.0),
        (100.0, 373.15),
        (25.0, 298.15),
    ] {
        assert!(close(c.convert(celsius, &k).unwrap() + 1.0, kelvin + 1.0));
        assert!(close(
            k.convert(kelvin, &c).unwrap() + 1000.0,
            celsius + 1000.0
        ));
    }
    let conv = c.conversion_to(&k).unwrap();
    assert_eq!(conv.scale, 1.0);
    assert_eq!(conv.offset, 273.15);
    assert!(close(unit("mK").convert(1500.0, &k).unwrap(), 1.5));
}

#[test]
fn conversion_maps_are_linear() {
    let conv = unit("W").conversion_to(&unit("kW")).unwrap();
    assert_eq!(conv.apply(125_000.0), 125.0);
    assert!(unit("kW").conversion_to(&unit("kW")).unwrap().is_identity());
}

#[test]
fn incompatible_dimensions() {
    for (a, b) in [
        ("W", "Wh"),
        ("kW", "kW/s"),
        ("Hz", "s"),
        ("%", "1"),
        ("%/s", "1/s"),
        ("°C", "W"),
        ("m3/h", "l"),
        ("bar", "N"),
    ] {
        let e = unit(a).convert(1.0, &unit(b)).unwrap_err();
        assert_eq!(e.kind, ErrorKind::IncompatibleDimension, "{a} -> {b}");
        assert_eq!(e.code(), "J0107");
    }
}

#[test]
fn incompatible_kinds() {
    for (a, b) in [
        ("W", "var"),
        ("kW", "kVA"),
        ("var", "VA"),
        ("Wh", "varh"),
        ("kW/s", "kvar/s"),
        ("J/s", "var"),
    ] {
        let e = unit(a).convert(1.0, &unit(b)).unwrap_err();
        assert_eq!(e.kind, ErrorKind::IncompatibleKind, "{a} -> {b}");
        assert_eq!(e.code(), "J0108");
    }
}

#[test]
fn kinds_of_catalogue_units() {
    let cases = [
        ("kW", Some(QuantityKind::ActivePower)),
        ("Mvar", Some(QuantityKind::ReactivePower)),
        ("kVA", Some(QuantityKind::ApparentPower)),
        ("kWh", Some(QuantityKind::Energy)),
        ("J", Some(QuantityKind::Energy)),
        ("kvarh", Some(QuantityKind::ReactiveEnergy)),
        ("°C", Some(QuantityKind::Temperature)),
        ("K", None),
        ("Nm", None),
        ("kW/s", Some(QuantityKind::ActivePower)),
        ("kWh/h", Some(QuantityKind::ActivePower)),
        ("°C/min", Some(QuantityKind::TemperatureDifference)),
        ("kW/m2", None),
        ("kW*h", None),
    ];
    for (s, kind) in cases {
        assert_eq!(unit(s).kind(), kind, "{s}");
    }
}

#[test]
fn every_catalogue_unit_parses() {
    let catalogue = [
        "s", "m", "g", "A", "K", "mol", "cd", "1", "W", "Wh", "J", "V", "Ω", "Hz", "var", "varh",
        "VA", "Pa", "bar", "N", "Nm", "lx", "°C", "%", "rpm", "l", "h", "min", "d", "m2", "m3",
        "m/s", "m/s2", "m3/h", "l/min", "gn", "kW/s", "%/s", "m3/h/%", "°C/min",
    ];
    for s in catalogue {
        assert_eq!(unit(s).symbol(), s);
    }
    for p in ["µ", "m", "k", "M", "G"] {
        for base in [
            "s", "m", "g", "A", "K", "mol", "cd", "W", "Wh", "J", "V", "Ω", "Hz",
        ] {
            let mut buf = [0u8; 16];
            let s = concat(&mut buf, p, base);
            assert!(Unit::parse(s).is_ok(), "{s}");
        }
    }
}

fn concat<'a>(buf: &'a mut [u8; 16], a: &str, b: &str) -> &'a str {
    let (x, y) = buf.split_at_mut(a.len());
    x.copy_from_slice(a.as_bytes());
    y[..b.len()].copy_from_slice(b.as_bytes());
    core::str::from_utf8(&buf[..a.len() + b.len()]).unwrap()
}

#[test]
fn quantity_literals_convert() {
    let q = Quantity::parse("125 kW").unwrap();
    assert_eq!(q.value_in(&unit("W")).unwrap(), 125_000.0);
    let r = Quantity::parse("-20 kW/s").unwrap();
    assert_eq!(r.value_in(&unit("W/s")).unwrap(), -20_000.0);
    let t = Quantity::parse("0.5 °C/min").unwrap();
    assert!(close(t.value_in(&unit("K/s")).unwrap(), 0.5 / 60.0));
    assert_eq!(unit("m/s2").dimension(), unit("gn").dimension());
    assert_ne!(unit("m/s2").dimension(), Dimension::NONE);
}
