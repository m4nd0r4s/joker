//! An untyped TOML tree in which every key and value keeps the span of the
//! layer that wrote it, so the merged manifest still knows where each value
//! came from (ref/01 §12: layers are read untyped, then merged).

use std::collections::BTreeMap;

use toml::de::{DeTable, DeValue};

use crate::source::{FileId, Span};

/// A TOML value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A string.
    String(String),
    /// An integer.
    Integer(i64),
    /// A float.
    Float(f64),
    /// A boolean.
    Bool(bool),
    /// A date-time, kept as written.
    Datetime(String),
    /// An array.
    Array(Vec<Node>),
    /// A table.
    Table(Table),
}

impl Value {
    /// The TOML type name, for messages.
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::String(_) => "a string",
            Self::Integer(_) => "an integer",
            Self::Float(_) => "a float",
            Self::Bool(_) => "a boolean",
            Self::Datetime(_) => "a date-time",
            Self::Array(_) => "an array",
            Self::Table(_) => "a table",
        }
    }
}

/// A value with the span where it is written.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// The value.
    pub value: Value,
    /// Where the value is written.
    pub span: Span,
}

impl Node {
    /// The string, if the value is one.
    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The table, if the value is one.
    pub fn as_table(&self) -> Option<&Table> {
        match &self.value {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }
}

/// A table entry: the span of its key and its value.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Where the key is written.
    pub key: Span,
    /// The value.
    pub node: Node,
}

/// A table, ordered by key (invariant 9: no observable hash order).
pub type Table = BTreeMap<String, Entry>;

/// A TOML number that does not fit an `i64` or an `f64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidNumber(pub Span);

/// Converts a parsed TOML table into a [`Table`] with spans in `file`.
pub fn from_toml(file: FileId, table: &DeTable<'_>) -> Result<Table, InvalidNumber> {
    let mut out = Table::new();
    for (key, value) in table.iter() {
        let key_range = key.span();
        out.insert(
            key.get_ref().to_string(),
            Entry {
                key: Span::new(file, key_range.start, key_range.end),
                node: node(file, value.get_ref(), value.span())?,
            },
        );
    }
    Ok(out)
}

fn node(
    file: FileId,
    value: &DeValue<'_>,
    range: std::ops::Range<usize>,
) -> Result<Node, InvalidNumber> {
    let span = Span::new(file, range.start, range.end);
    let value = match value {
        DeValue::String(s) => Value::String(s.to_string()),
        DeValue::Integer(i) => Value::Integer(
            i64::from_str_radix(i.as_str(), i.radix()).map_err(|_| InvalidNumber(span))?,
        ),
        DeValue::Float(f) => Value::Float(float(f.as_str()).ok_or(InvalidNumber(span))?),
        DeValue::Boolean(b) => Value::Bool(*b),
        DeValue::Datetime(d) => Value::Datetime(d.to_string()),
        DeValue::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| node(file, item.get_ref(), item.span()))
                .collect::<Result<_, _>>()?,
        ),
        DeValue::Table(t) => Value::Table(from_toml(file, t)?),
    };
    Ok(Node { value, span })
}

fn float(text: &str) -> Option<f64> {
    let text = text.replace('_', "");
    match text.trim_start_matches(['+', '-']) {
        "inf" | "nan" => text.replace("inf", "infinity").parse().ok(),
        _ => text.parse().ok(),
    }
}
