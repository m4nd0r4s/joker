//! The code registry (docs/16 §9, ref/00 §8).
//!
//! Every diagnostic and runtime error of the Joker carries a code from a single
//! registry, `codes.toml`, embedded in this crate. The registry is data: the
//! message, explanation and remissions of each code are read from the file and
//! never duplicated in code, so `jk explain`, the API `title` and the
//! diagnostics of `jk check` all show the same text.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::RangeInclusive;
use std::str::FromStr;
use std::sync::OnceLock;

use serde::Deserialize;

/// The registry source embedded at build time.
pub const SOURCE: &str = include_str!("../codes.toml");

/// The registry schema version this loader reads.
pub const SCHEMA: u32 = 1;

/// The namespace a code belongs to (docs/16 §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// `Jxxxx`: validation codes of the manifest, lock, node and tooling.
    Manifest,
    /// `J-Exxxx`: runtime errors seen by apps, SDKs, the API and the Joker Micro.
    Runtime,
    /// `JCxxxx`: errors of the class compiler `jkc`.
    ClassError,
    /// `JWxxxx`: warnings of the class compiler `jkc`.
    ClassWarning,
}

impl Family {
    /// The prefix written before the four digits.
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Manifest => "J",
            Self::Runtime => "J-E",
            Self::ClassError => "JC",
            Self::ClassWarning => "JW",
        }
    }

    const fn numbers(self) -> RangeInclusive<u16> {
        match self {
            Self::Manifest => 100..=999,
            Self::Runtime => 1..=2999,
            Self::ClassError | Self::ClassWarning => 1..=9999,
        }
    }

    const ALL: [Self; 4] = [
        Self::Runtime,
        Self::ClassError,
        Self::ClassWarning,
        Self::Manifest,
    ];
}

/// A code identifier such as `J0107` or `J-E0901`.
///
/// Only numbers inside the bands of docs/16 §9 are valid: `J0100`–`J0999`,
/// `J-E0001`–`J-E2999`, and any four digits but `0000` for `JC` and `JW`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CodeId {
    family: Family,
    number: u16,
}

impl CodeId {
    /// Builds an identifier, checking the number against its family's band.
    pub fn new(family: Family, number: u16) -> Result<Self, InvalidCodeId> {
        if family.numbers().contains(&number) {
            Ok(Self { family, number })
        } else {
            Err(InvalidCodeId(format!("{}{number:04}", family.prefix())))
        }
    }

    /// The namespace of the code.
    pub const fn family(self) -> Family {
        self.family
    }

    /// The four-digit number of the code.
    pub const fn number(self) -> u16 {
        self.number
    }
}

impl FromStr for CodeId {
    type Err = InvalidCodeId;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidCodeId(text.to_owned());
        let (family, digits) = Family::ALL
            .iter()
            .find_map(|family| {
                text.strip_prefix(family.prefix())
                    .map(|rest| (*family, rest))
            })
            .ok_or_else(invalid)?;
        if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        let number = digits.parse().map_err(|_| invalid())?;
        Self::new(family, number).map_err(|_| invalid())
    }
}

impl fmt::Display for CodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{:04}", self.family.prefix(), self.number)
    }
}

/// A text that is not a valid code identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidCodeId(pub String);

impl fmt::Display for InvalidCodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid code identifier `{}`", self.0)
    }
}

impl std::error::Error for InvalidCodeId {}

/// The single severity of a code (docs/16 §9: one code, one severity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// Always an error.
    Error,
    /// Always a warning.
    Warning,
    /// An error in `mode = "prod"`, shown as a `W` warning in `mode = "dev"`.
    ErrorInProd,
    /// A trap of a Wasm app; only `J-E09xx` codes are traps.
    Trap,
}

impl Severity {
    /// The registry spelling of the severity.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::ErrorInProd => "error-in-prod",
            Self::Trap => "trap",
        }
    }
}

/// Whether a code is in use, reserved for a later version or retired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    /// The code is in use.
    Active,
    /// The code belongs to a reserved surface (ADR 0028).
    Reserved,
    /// The code is retired and never reused.
    Retired {
        /// The code that now covers the case.
        replaced_by: CodeId,
    },
}

/// The document a remission points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Document {
    /// A numbered document in `docs/`, such as `16`.
    Doc(u8),
    /// A normative reference in `docs/ref/`, such as `ref/04`.
    Reference(u8),
    /// An architecture decision record in `docs/adr/`, such as `ADR 0026`.
    Adr(u16),
}

/// A remission to a document and optionally one of its sections, such as
/// `ref/04 §7` or `03 §15`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Remission {
    document: Document,
    section: Option<String>,
}

impl Remission {
    /// The document the remission points at.
    pub const fn document(&self) -> Document {
        self.document
    }

    /// The section number, such as `10.2`, if the remission names one.
    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }
}

impl FromStr for Remission {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (document, section) = match text.split_once(" §") {
            Some((document, section)) => (document, Some(section)),
            None => (text, None),
        };
        let digits = |s: &str, len: usize| -> Result<u16, ()> {
            if s.len() == len && s.bytes().all(|b| b.is_ascii_digit()) {
                s.parse().map_err(|_| ())
            } else {
                Err(())
            }
        };
        let document = if let Some(n) = document.strip_prefix("ref/") {
            Document::Reference(u8::try_from(digits(n, 2)?).map_err(|_| ())?)
        } else if let Some(n) = document.strip_prefix("ADR ") {
            Document::Adr(digits(n, 4)?)
        } else {
            Document::Doc(u8::try_from(digits(document, 2)?).map_err(|_| ())?)
        };
        let section = match section {
            Some(s)
                if !s.is_empty()
                    && s.split('.').all(|part| {
                        !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
                    }) =>
            {
                Some(s.to_owned())
            }
            Some(_) => return Err(()),
            None => None,
        };
        Ok(Self { document, section })
    }
}

impl fmt::Display for Remission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.document {
            Document::Doc(n) => write!(f, "{n:02}")?,
            Document::Reference(n) => write!(f, "ref/{n:02}")?,
            Document::Adr(n) => write!(f, "ADR {n:04}")?,
        }
        match &self.section {
            Some(section) => write!(f, " §{section}"),
            None => Ok(()),
        }
    }
}

/// A `major.minor` version, used by `since`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// The major version.
    pub major: u16,
    /// The minor version.
    pub minor: u16,
}

impl FromStr for Version {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (major, minor) = text.split_once('.').ok_or(())?;
        let part = |s: &str| -> Result<u16, ()> {
            if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
                s.parse().map_err(|_| ())
            } else {
                Err(())
            }
        };
        Ok(Self {
            major: part(major)?,
            minor: part(minor)?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// One entry of the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    id: CodeId,
    severity: Option<Severity>,
    status: Status,
    message: String,
    explain: Option<String>,
    doc: Remission,
    also: Vec<Remission>,
    since: Option<Version>,
}

impl Code {
    /// The identifier of the code.
    pub const fn id(&self) -> CodeId {
        self.id
    }

    /// The severity; `None` only for a retired code.
    pub const fn severity(&self) -> Option<Severity> {
        self.severity
    }

    /// Whether the code is active, reserved or retired.
    pub const fn status(&self) -> Status {
        self.status
    }

    /// The short message, as read from the registry.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The long explanation shown by `jk explain`, if the registry has one.
    pub fn explain(&self) -> Option<&str> {
        self.explain.as_deref()
    }

    /// The reference that defines the code.
    pub const fn doc(&self) -> &Remission {
        &self.doc
    }

    /// Other references that cite the code.
    pub fn also(&self) -> &[Remission] {
        &self.also
    }

    /// The version in which the code starts to exist, for reserved features.
    pub const fn since(&self) -> Option<Version> {
        self.since
    }

    /// The `W` form shown in `mode = "dev"` for a manifest code of severity
    /// `error-in-prod`, such as `W0205` for `J0205`.
    pub fn dev_warning(&self) -> Option<String> {
        (self.id.family == Family::Manifest && self.severity == Some(Severity::ErrorInProd))
            .then(|| format!("W{:04}", self.id.number))
    }
}

/// A registry that failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// The file is not valid TOML or does not match the entry layout.
    Syntax(toml::de::Error),
    /// The file has no integer `schema`.
    MissingSchema,
    /// The `schema` is not one this loader reads.
    UnsupportedSchema(u32),
    /// An `id` or `replaced_by` is not a valid code identifier.
    InvalidId(InvalidCodeId),
    /// Two entries share an identifier; retired identifiers are never reused.
    Duplicate(CodeId),
    /// An entry has an empty message.
    EmptyMessage(CodeId),
    /// A code that is not retired has no severity.
    MissingSeverity(CodeId),
    /// A code outside `J-E0900`–`J-E0999` has severity `trap`.
    MisplacedTrap(CodeId),
    /// A `doc` or `also` value is not a remission.
    InvalidRemission {
        /// The entry.
        code: CodeId,
        /// The text that failed to parse.
        text: String,
    },
    /// A `since` value is not a `major.minor` version.
    InvalidVersion {
        /// The entry.
        code: CodeId,
        /// The text that failed to parse.
        text: String,
    },
    /// A retired code also declares a `status`.
    RetiredWithStatus(CodeId),
    /// A retired code has no `replaced_by`.
    RetiredWithoutReplacement(CodeId),
    /// A code that is not retired declares `replaced_by`.
    ReplacementWithoutRetirement(CodeId),
    /// A `replaced_by` names a code that is missing, retired or the code itself.
    DanglingReplacement {
        /// The retired entry.
        code: CodeId,
        /// The code it names.
        replaced_by: CodeId,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => write!(f, "code registry does not parse: {error}"),
            Self::MissingSchema => write!(f, "code registry has no `schema`"),
            Self::UnsupportedSchema(schema) => {
                write!(
                    f,
                    "code registry schema {schema} is not supported (expected {SCHEMA})"
                )
            }
            Self::InvalidId(error) => write!(f, "{error}"),
            Self::Duplicate(code) => write!(f, "code {code} is registered twice"),
            Self::EmptyMessage(code) => write!(f, "code {code} has an empty message"),
            Self::MissingSeverity(code) => write!(f, "code {code} has no severity"),
            Self::MisplacedTrap(code) => {
                write!(f, "code {code} is a trap but only J-E09xx codes are traps")
            }
            Self::InvalidRemission { code, text } => {
                write!(f, "code {code} has an invalid remission `{text}`")
            }
            Self::InvalidVersion { code, text } => {
                write!(f, "code {code} has an invalid `since` version `{text}`")
            }
            Self::RetiredWithStatus(code) => {
                write!(f, "retired code {code} must not declare a status")
            }
            Self::RetiredWithoutReplacement(code) => {
                write!(f, "retired code {code} has no `replaced_by`")
            }
            Self::ReplacementWithoutRetirement(code) => {
                write!(f, "code {code} declares `replaced_by` but is not retired")
            }
            Self::DanglingReplacement { code, replaced_by } => write!(
                f,
                "retired code {code} is replaced by {replaced_by}, which is not an active code"
            ),
        }
    }
}

impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Syntax(error) => Some(error),
            Self::InvalidId(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    #[serde(default)]
    code: Vec<RawCode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawStatus {
    Active,
    Reserved,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCode {
    id: String,
    severity: Option<Severity>,
    status: Option<RawStatus>,
    message: String,
    explain: Option<String>,
    doc: String,
    #[serde(default)]
    also: Vec<String>,
    since: Option<String>,
    #[serde(default)]
    retired: bool,
    replaced_by: Option<String>,
}

impl RawCode {
    fn validate(self) -> Result<Code, RegistryError> {
        let id: CodeId = self.id.parse().map_err(RegistryError::InvalidId)?;
        if self.message.trim().is_empty() {
            return Err(RegistryError::EmptyMessage(id));
        }
        let remission = |text: String| {
            text.parse::<Remission>()
                .map_err(|()| RegistryError::InvalidRemission { code: id, text })
        };
        let doc = remission(self.doc)?;
        let also = self
            .also
            .into_iter()
            .map(remission)
            .collect::<Result<Vec<_>, _>>()?;
        let since = self
            .since
            .map(|text| {
                text.parse::<Version>()
                    .map_err(|()| RegistryError::InvalidVersion { code: id, text })
            })
            .transpose()?;
        let replaced_by = self
            .replaced_by
            .map(|text| text.parse::<CodeId>().map_err(RegistryError::InvalidId))
            .transpose()?;
        let status = match (self.retired, self.status, replaced_by) {
            (true, Some(_), _) => return Err(RegistryError::RetiredWithStatus(id)),
            (true, None, Some(replaced_by)) => Status::Retired { replaced_by },
            (true, None, None) => return Err(RegistryError::RetiredWithoutReplacement(id)),
            (false, _, Some(_)) => return Err(RegistryError::ReplacementWithoutRetirement(id)),
            (false, Some(RawStatus::Reserved), None) => Status::Reserved,
            (false, Some(RawStatus::Active) | None, None) => Status::Active,
        };
        match self.severity {
            None if !self.retired => return Err(RegistryError::MissingSeverity(id)),
            Some(Severity::Trap)
                if id.family != Family::Runtime || !(900..=999).contains(&id.number) =>
            {
                return Err(RegistryError::MisplacedTrap(id));
            }
            _ => {}
        }
        Ok(Code {
            id,
            severity: self.severity,
            status,
            message: self.message,
            explain: self.explain,
            doc,
            also,
            since,
        })
    }
}

/// The validated code registry, ordered by identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    codes: BTreeMap<CodeId, Code>,
}

impl Registry {
    /// Parses and validates a registry source.
    pub fn parse(source: &str) -> Result<Self, RegistryError> {
        let mut table: toml::Table = source.parse().map_err(RegistryError::Syntax)?;
        let schema = table
            .remove("schema")
            .and_then(|value| value.as_integer())
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(RegistryError::MissingSchema)?;
        if schema != SCHEMA {
            return Err(RegistryError::UnsupportedSchema(schema));
        }
        let raw: RawRegistry = toml::Value::Table(table)
            .try_into()
            .map_err(RegistryError::Syntax)?;
        let mut codes = BTreeMap::new();
        for entry in raw.code {
            let code = entry.validate()?;
            let id = code.id;
            if codes.insert(id, code).is_some() {
                return Err(RegistryError::Duplicate(id));
            }
        }
        for code in codes.values() {
            if let Status::Retired { replaced_by } = code.status {
                let target_active = codes
                    .get(&replaced_by)
                    .is_some_and(|target| !matches!(target.status, Status::Retired { .. }));
                if replaced_by == code.id || !target_active {
                    return Err(RegistryError::DanglingReplacement {
                        code: code.id,
                        replaced_by,
                    });
                }
            }
        }
        Ok(Self { codes })
    }

    /// The registry embedded in this crate, parsed once.
    pub fn builtin() -> Result<&'static Self, &'static RegistryError> {
        static BUILTIN: OnceLock<Result<Registry, RegistryError>> = OnceLock::new();
        BUILTIN.get_or_init(|| Self::parse(SOURCE)).as_ref()
    }

    /// The entry for an identifier.
    pub fn get(&self, id: CodeId) -> Option<&Code> {
        self.codes.get(&id)
    }

    /// The entry for a code as a person writes it, including the `W` form of a
    /// manifest code of severity `error-in-prod` (`W0205` finds `J0205`).
    pub fn lookup(&self, text: &str) -> Option<&Code> {
        if let Some(digits) = text.strip_prefix('W') {
            let id: CodeId = format!("J{digits}").parse().ok()?;
            return self
                .get(id)
                .filter(|code| code.severity == Some(Severity::ErrorInProd));
        }
        self.get(text.parse().ok()?)
    }

    /// All entries, ordered by identifier.
    pub fn iter(&self) -> impl Iterator<Item = &Code> {
        self.codes.values()
    }

    /// The number of entries, retired ones included.
    pub fn len(&self) -> usize {
        self.codes.len()
    }

    /// Whether the registry has no entries.
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, extra: &str) -> String {
        format!("[[code]]\nid = \"{id}\"\nmessage = \"m\"\ndoc = \"ref/01 §10.1\"\n{extra}\n")
    }

    fn registry(entries: &[String]) -> Result<Registry, RegistryError> {
        Registry::parse(&format!("schema = 1\n{}", entries.concat()))
    }

    #[test]
    fn builtin_registry_is_valid() {
        let registry = Registry::builtin().unwrap();
        assert!(!registry.is_empty());
        let unit = registry.lookup("J0107").unwrap();
        assert_eq!(unit.severity(), Some(Severity::Error));
        assert_eq!(unit.doc().document(), Document::Reference(1));
    }

    #[test]
    fn code_ids_round_trip() {
        for text in ["J0107", "J-E0901", "JC0107", "JW0230", "J-E2512"] {
            assert_eq!(text.parse::<CodeId>().unwrap().to_string(), text);
        }
        let id: CodeId = "J-E0901".parse().unwrap();
        assert_eq!((id.family(), id.number()), (Family::Runtime, 901));
    }

    #[test]
    fn code_ids_outside_their_band_are_invalid() {
        let out_of_band =
            [("J", "0099"), ("J-E", "3000"), ("JC", "0000")].map(|(p, n)| [p, n].concat());
        for text in out_of_band
            .iter()
            .map(String::as_str)
            .chain(["J107", "J01070", "X0107", "J-E09a1", "W0205"])
        {
            assert!(text.parse::<CodeId>().is_err(), "{text}");
        }
    }

    #[test]
    fn remissions_round_trip() {
        for text in [
            "ref/04 §7",
            "03 §15",
            "ADR 0026",
            "ref/03 §2.2.14",
            "ref/01",
        ] {
            assert_eq!(text.parse::<Remission>().unwrap().to_string(), text);
        }
        for text in ["ref/4 §7", "doc 03", "03 §", "03 §a.1", "ADR 26"] {
            assert!(text.parse::<Remission>().is_err(), "{text}");
        }
    }

    #[test]
    fn duplicates_and_reuse_of_retired_ids_are_rejected() {
        let error = registry(&[
            entry("J0140", "severity = \"error\""),
            entry("J0291", "retired = true\nreplaced_by = \"J0140\""),
            entry("J0291", "severity = \"error\""),
        ]);
        assert_eq!(
            error,
            Err(RegistryError::Duplicate("J0291".parse().unwrap()))
        );
    }

    #[test]
    fn retired_codes_need_an_active_replacement() {
        let j0140: CodeId = "J0140".parse().unwrap();
        let j0291: CodeId = "J0291".parse().unwrap();
        assert_eq!(
            registry(&[entry("J0291", "retired = true")]),
            Err(RegistryError::RetiredWithoutReplacement(j0291))
        );
        assert_eq!(
            registry(&[entry("J0291", "retired = true\nreplaced_by = \"J0140\"")]),
            Err(RegistryError::DanglingReplacement {
                code: j0291,
                replaced_by: j0140
            })
        );
        assert_eq!(
            registry(&[entry(
                "J0140",
                "severity = \"error\"\nreplaced_by = \"J0291\""
            )]),
            Err(RegistryError::ReplacementWithoutRetirement(j0140))
        );
        assert_eq!(
            registry(&[entry(
                "J0291",
                "retired = true\nreplaced_by = \"J0291\"\nstatus = \"reserved\""
            )]),
            Err(RegistryError::RetiredWithStatus(j0291))
        );
    }

    #[test]
    fn entries_are_validated() {
        let j0140: CodeId = "J0140".parse().unwrap();
        assert_eq!(
            registry(&[entry("J0140", "")]),
            Err(RegistryError::MissingSeverity(j0140))
        );
        assert_eq!(
            registry(&[entry("J0140", "severity = \"trap\"")]),
            Err(RegistryError::MisplacedTrap(j0140))
        );
        assert!(registry(&[entry("J-E0901", "severity = \"trap\"")]).is_ok());
        assert!(matches!(
            registry(&[entry("J0140", "severity = \"fatal\"")]),
            Err(RegistryError::Syntax(_))
        ));
        assert!(matches!(
            registry(&[entry("J0140", "severity = \"error\"\nsince = \"1\"")]),
            Err(RegistryError::InvalidVersion { .. })
        ));
        assert!(matches!(
            registry(&[entry("J0140", "severity = \"error\"\nalso = [\"ref/1\"]")]),
            Err(RegistryError::InvalidRemission { .. })
        ));
        assert!(matches!(
            registry(&[entry("J0140", "severity = \"error\"\nsevere = true")]),
            Err(RegistryError::Syntax(_))
        ));
        assert_eq!(
            Registry::parse("schema = 2\n"),
            Err(RegistryError::UnsupportedSchema(2))
        );
        assert_eq!(Registry::parse(""), Err(RegistryError::MissingSchema));
        assert!(matches!(
            Registry::parse("schema = 1\nextra = 1\n"),
            Err(RegistryError::Syntax(_))
        ));
    }

    #[test]
    fn reserved_status_and_since_are_read() {
        let registry = registry(&[entry(
            "J0501",
            "severity = \"error\"\nstatus = \"reserved\"\nsince = \"1.1\"",
        )])
        .unwrap();
        let code = registry.lookup("J0501").unwrap();
        assert_eq!(code.status(), Status::Reserved);
        assert_eq!(code.since(), Some(Version { major: 1, minor: 1 }));
    }

    #[test]
    fn dev_warnings_map_to_error_in_prod_manifest_codes() {
        let registry = Registry::builtin().unwrap();
        let code = registry.lookup("W0205").unwrap();
        assert_eq!(code.id().to_string(), "J0205");
        assert_eq!(code.dev_warning().as_deref(), Some("W0205"));
        assert!(registry.lookup(&["W", "0107"].concat()).is_none());
        assert!(registry.lookup("J-E0003").unwrap().dev_warning().is_none());
    }
}
