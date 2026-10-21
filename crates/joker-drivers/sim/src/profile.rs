//! What a simulation model takes from a device profile with `[model]
//! profile` (ref/03 §2.4.2): the signals' type, enum, unit, quantity,
//! direction, limits and named bits (§2.2.4, §2.2.7, §2.2.8), the `[enums]`
//! and the `[implements]`. The rest of the profile does not apply to a `sim`
//! device and is not read; the profile itself is validated by `jk check`.

use std::string::String;
use std::vec::Vec;

use toml::{Table, Value};

use crate::codes::{J0803, J0806, J0809, J0814, J0815};
use crate::model::{ModelError, err};

/// The signal type a profile point gives (ref/03 §2.2.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PointType {
    /// A signal type of document 02 §2 by name (`bool`, `u64`, `bits16`…).
    Plain(String),
    /// `enum<Name>`.
    Enum(String),
}

/// One point of the profile, as the model sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfilePoint {
    /// Relative path.
    pub name: String,
    /// Signal type.
    pub ty: PointType,
    /// Unit, `1` without one.
    pub unit: String,
    /// Quantity kind.
    pub quantity: Option<String>,
    /// `input`, `output` or `param`.
    pub direction: String,
    /// Physical limits, as written.
    pub limits: Option<Value>,
    /// Named bits: index and name.
    pub bits: Vec<(u8, String)>,
}

/// The parts of a profile a model reuses.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ProfileParts {
    /// Points, in path order.
    pub points: Vec<ProfilePoint>,
    /// `[enums]`.
    pub enums: Option<Table>,
    /// `[implements]`.
    pub implements: Option<Table>,
}

fn key<'a>(point: &'a Table, defaults: &'a Table, k: &str) -> Option<&'a Value> {
    point.get(k).or_else(|| defaults.get(k))
}

fn text(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}

/// The signal type of a raw type, per ref/03 §2.2.7.
fn signal_type(
    raw: &str,
    point: &Table,
    defaults: &Table,
    path: &str,
) -> Result<String, ModelError> {
    let scaled = ["scale", "offset", "scale_factor"]
        .iter()
        .any(|k| point.contains_key(*k) || defaults.contains_key(*k))
        || text(key(point, defaults, "unit")).is_some_and(|u| u != "1");
    let derived = match raw {
        "bool" => "bool",
        "u16" | "u32" | "u64" if scaled => "f64",
        "i16" | "i32" | "i64" if scaled => "f64",
        "u16" | "u32" | "u64" => "u64",
        "i16" | "i32" | "i64" => "i64",
        "f32" | "f64" => "f64",
        "bits16" | "bits32" | "bits64" | "string" | "bytes" => raw,
        other => {
            return Err(err(
                J0809,
                path,
                std::format!("raw type `{other}` is not one of ref/03 §2.2.7"),
            ));
        }
    };
    match text(key(point, defaults, "signal_type")) {
        None => Ok(String::from(derived)),
        Some(forced @ ("bool" | "i64" | "u64" | "f64" | "f32")) => {
            let ok = match forced {
                "f32" => matches!(raw, "f32" | "f64"),
                "bool" => raw == "bool",
                _ => raw != "bool" && !raw.starts_with("bits") && raw != "string" && raw != "bytes",
            };
            if ok {
                Ok(String::from(forced))
            } else {
                Err(err(
                    J0815,
                    path,
                    std::format!("`signal_type = \"{forced}\"` does not fit raw type `{raw}`"),
                ))
            }
        }
        Some(other) => Err(err(
            J0815,
            path,
            std::format!("unknown `signal_type` `{other}`"),
        )),
    }
}

/// Reads the parts of `source` a model reuses. A file without `[profile]`
/// is not a profile (`J0803`).
pub fn read(source: &str) -> Result<ProfileParts, ModelError> {
    let doc: Table = source
        .parse()
        .map_err(|e: toml::de::Error| err(J0806, "model.profile", e.message().to_owned()))?;
    if !doc.get("profile").is_some_and(Value::is_table) {
        return Err(err(
            J0803,
            "model.profile",
            "the referenced file has no `[profile]`: it is not a device profile",
        ));
    }
    let empty = Table::new();
    let defaults = doc
        .get("defaults")
        .and_then(Value::as_table)
        .unwrap_or(&empty);
    let mut points = Vec::new();
    if let Some(signals) = doc.get("signals").and_then(Value::as_table) {
        for (name, p) in signals {
            let path = std::format!("model.profile.signals.{name}");
            let Some(p) = p.as_table() else {
                return Err(err(J0806, &path, "a point is a table"));
            };
            let raw = text(key(p, defaults, "type"))
                .ok_or_else(|| err(J0809, &path, "missing `type`"))?;
            let ty = match text(p.get("enum")) {
                Some(e) => PointType::Enum(String::from(e)),
                None => PointType::Plain(signal_type(raw, p, defaults, &path)?),
            };
            let mut bits = Vec::new();
            if let Some(b) = p.get("bits") {
                let Some(b) = b.as_table() else {
                    return Err(err(J0814, &path, "`bits` is a table index → name"));
                };
                for (index, bit) in b {
                    let i: u8 = index
                        .parse()
                        .map_err(|_| err(J0814, &path, "a bit index is an integer"))?;
                    let n = bit
                        .as_str()
                        .ok_or_else(|| err(J0814, &path, "a bit name is a string"))?;
                    bits.push((i, String::from(n)));
                }
            }
            points.push(ProfilePoint {
                name: name.clone(),
                ty,
                unit: String::from(text(key(p, defaults, "unit")).unwrap_or("1")),
                quantity: text(p.get("quantity")).map(String::from),
                direction: String::from(text(key(p, defaults, "direction")).unwrap_or("input")),
                limits: p.get("limits").cloned(),
                bits,
            });
        }
    }
    Ok(ProfileParts {
        points,
        enums: doc.get("enums").and_then(Value::as_table).cloned(),
        implements: doc.get("implements").and_then(Value::as_table).cloned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_types_follow_the_raw_type() {
        let src = r#"schema = 1
[profile]
name = "local/x"
version = "1.0.0"
driver = "modbus"
[defaults]
direction = "input"
[signals]
p = { at = 1, type = "i16", unit = "kW" }
n = { at = 2, type = "u16" }
f = { at = 3, type = "f32", signal_type = "f32" }
st = { at = 4, type = "bits16", bits = { 0 = "running", 3 = "fault" } }
mode = { at = 5, type = "u16", enum = "Mode", direction = "output" }
[enums]
Mode = { off = 0, on = 1 }
"#;
        let parts = read(src).unwrap();
        let ty = |n: &str| {
            parts
                .points
                .iter()
                .find(|p| p.name == n)
                .map(|p| p.ty.clone())
                .unwrap()
        };
        assert_eq!(ty("p"), PointType::Plain("f64".into()));
        assert_eq!(ty("n"), PointType::Plain("u64".into()));
        assert_eq!(ty("f"), PointType::Plain("f32".into()));
        assert_eq!(ty("st"), PointType::Plain("bits16".into()));
        assert_eq!(ty("mode"), PointType::Enum("Mode".into()));
        assert_eq!(
            parts
                .points
                .iter()
                .find(|p| p.name == "st")
                .unwrap()
                .bits
                .len(),
            2
        );
        assert!(parts.enums.is_some());
        assert_eq!(read("schema = 1\n[signals]\n").unwrap_err().code, J0803);
    }
}
