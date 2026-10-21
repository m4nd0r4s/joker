//! Errors as `application/problem+json` (RFC 9457, ref/09 §3.4).

use std::fmt;

use serde_json::{Map, Value, json};

/// The media type of a problem.
pub const PROBLEM_JSON: &str = "application/problem+json";

/// One diagnostic of a problem (`errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemError {
    /// Its code.
    pub code: String,
    /// The rendered message.
    pub message: String,
}

/// A refused request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Registry code (`J-E1105`, `J0107`).
    pub code: String,
    /// HTTP status.
    pub status: u16,
    /// The concrete case.
    pub detail: Option<String>,
    /// Extra members (`pointer`, `param`, `help`, …).
    pub extra: Map<String, Value>,
    /// Diagnostics.
    pub errors: Vec<ProblemError>,
}

impl Problem {
    /// A problem with a code, a status and a detail.
    pub fn new(code: &str, status: u16, detail: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            status,
            detail: Some(detail.into()),
            extra: Map::new(),
            errors: Vec::new(),
        }
    }

    /// Adds an extra member.
    #[must_use]
    pub fn with(mut self, key: &str, value: Value) -> Self {
        self.extra.insert(key.to_owned(), value);
        self
    }

    /// The short title of the code, from the registry.
    pub fn title(&self) -> String {
        joker_manifest::codes::Registry::builtin()
            .ok()
            .and_then(|r| r.lookup(&self.code))
            .map_or_else(|| self.code.clone(), |c| c.message().to_owned())
    }

    /// The JSON body.
    pub fn body(&self, request_id: Option<&str>) -> Value {
        let mut m = Map::new();
        m.insert(
            "type".into(),
            json!(format!("urn:joker:code:{}", self.code)),
        );
        m.insert("title".into(), json!(self.title()));
        m.insert("status".into(), json!(self.status));
        m.insert("code".into(), json!(self.code));
        if let Some(d) = &self.detail {
            m.insert("detail".into(), json!(d));
        }
        if let Some(id) = request_id {
            m.insert("instance".into(), json!(format!("urn:joker:request:{id}")));
            m.insert("request_id".into(), json!(id));
        }
        if !self.errors.is_empty() {
            m.insert(
                "errors".into(),
                Value::Array(
                    self.errors
                        .iter()
                        .map(|e| json!({"code": e.code, "severity": "error", "message": e.message}))
                        .collect(),
                ),
            );
        }
        for (k, v) in &self.extra {
            m.insert(k.clone(), v.clone());
        }
        Value::Object(m)
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error[{}]: {}",
            self.code,
            self.detail.as_deref().unwrap_or("")
        )
    }
}

impl std::error::Error for Problem {}
