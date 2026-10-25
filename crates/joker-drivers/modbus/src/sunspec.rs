//! SunSpec discovery over Modbus (M2, ref/03 §2.2.15, ref/06 §2.11, ADR
//! 0077).
//!
//! A SunSpec device announces itself with the marker `SunS` in two holding
//! registers at one of a few base addresses, followed by a chain of models:
//! each model is a header of two registers (its id and the length of its
//! body) and the body, and the chain ends with the id `0xFFFF`. Discovery
//! looks for the marker at each base of the profile's `[discovery]`, walks
//! the chain, and reads the body of every model this module knows, which
//! gives the identity of the equipment (model 1) and the scale factors of
//! the inverter (101–103) and meter (201–204) models. It reads only, with
//! FC 3.
//!
//! Everything here is pure: [`Walk`] says which registers to read next and
//! takes the answers, so `modbus-rtu`, `modbus-tcp` and the tests drive the
//! same walk. [`Found`] is the result: the effective map the driver reports
//! at `connect` (and whose digest the lock pins), the entry of
//! `[discovered.<device>]`, and a profile generated from the map in the
//! format of `jk profile import` (ref/03 §2.2.19). [`Image`] builds the
//! register image of a simulated SunSpec device.

use std::collections::BTreeMap;
use std::format;
use std::string::{String, ToString};
use std::vec;
use std::vec::Vec;

use joker_hal::cbor::Value;
use joker_hal::{BatchFailure, Cbor, ConnectOut, DeviceInfo, DriverError, code};
use sha2::{Digest, Sha256};

/// `SunS` in two registers.
pub const MARKER: [u16; 2] = [0x5375, 0x6E53];

/// The id that ends the chain.
pub const END: u16 = 0xFFFF;

/// Where the marker is looked for when `[discovery]` has no `bases`.
pub const DEFAULT_BASES: [u16; 3] = [40000, 50000, 0];

/// Models walked at most when `[discovery]` has no `max_models`.
pub const DEFAULT_MAX_MODELS: usize = 64;

/// Registers one read asks for at most (Modbus application protocol §6.3).
pub const MAX_READ: u16 = 125;

/// The `[discovery]` of a profile, as it reaches the driver in the key
/// `discovery` of the device's quirks (ref/06 §2.4.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovery {
    /// Where to look for the marker, in order.
    pub bases: Vec<u16>,
    /// The accepted model ids; empty accepts every model.
    pub models: Vec<u16>,
    /// Models walked at most.
    pub max_models: usize,
}

impl Default for Discovery {
    fn default() -> Self {
        Self {
            bases: DEFAULT_BASES.to_vec(),
            models: Vec::new(),
            max_models: DEFAULT_MAX_MODELS,
        }
    }
}

fn u16_list(v: Cbor<'_>, what: &'static str) -> Result<Vec<u16>, &'static str> {
    let mut out = Vec::new();
    for item in v.array().map_err(|_| what)? {
        let n = item
            .and_then(|i| i.as_u64())
            .ok()
            .and_then(|n| u16::try_from(n).ok())
            .ok_or(what)?;
        out.push(n);
    }
    Ok(out)
}

impl Discovery {
    /// Reads `{ kind = "sunspec", bases?, models?, max_models? }`. `None`
    /// when the quirks carry no `discovery`.
    pub fn parse(quirks: Cbor<'_>) -> Result<Option<Self>, &'static str> {
        let Some(table) = quirks
            .get("discovery")
            .map_err(|_| "quirks are not a table")?
        else {
            return Ok(None);
        };
        let mut d = Self::default();
        let mut kind = None;
        for entry in table.entries().map_err(|_| "`discovery` is not a table")? {
            let (k, v) = entry.map_err(|_| "`discovery` is not a table")?;
            match k {
                "kind" => kind = v.as_str().ok(),
                "bases" => d.bases = u16_list(v, "`bases` is a list of addresses")?,
                "models" => d.models = u16_list(v, "`models` is a list of model ids")?,
                "max_models" => {
                    d.max_models = v
                        .as_u64()
                        .ok()
                        .and_then(|n| usize::try_from(n).ok())
                        .filter(|n| (1..=1024).contains(n))
                        .ok_or("`max_models` is 1–1024")?;
                }
                _ => return Err("unknown key of `discovery`"),
            }
        }
        match kind {
            Some("sunspec") => {}
            Some(_) => return Err("`discovery.kind` is \"sunspec\" in this runtime"),
            None => return Err("`discovery.kind` is required"),
        }
        if d.bases.is_empty() {
            return Err("`bases` is empty");
        }
        Ok(Some(d))
    }

    fn accepts(&self, id: u16) -> bool {
        self.models.is_empty() || self.models.contains(&id)
    }
}

/// One model of the chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelRef {
    /// The model id.
    pub id: u16,
    /// The address of its id register, base 0.
    pub at: u16,
    /// The registers of its body.
    pub len: u16,
}

impl ModelRef {
    /// The address of the first register of the body.
    pub fn body(&self) -> u16 {
        self.at.saturating_add(2)
    }
}

/// Why discovery failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryError {
    /// No base holds the marker.
    NoMarker,
    /// The chain is longer than `max_models`.
    TooManyModels,
    /// A model runs past address 65535.
    Malformed,
    /// A read failed for another reason than an exception.
    Read(BatchFailure),
}

impl DiscoveryError {
    /// A message for the `connect` failure.
    pub fn message(&self) -> &'static str {
        match self {
            Self::NoMarker => "no SunSpec marker at any base of `discovery.bases`",
            Self::TooManyModels => "the SunSpec chain has more models than `max_models`",
            Self::Malformed => "a SunSpec model runs past address 65535",
            Self::Read(_) => "a read of the SunSpec chain failed",
        }
    }
}

/// How one read of the walk went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The device answered with an exception (an address it does not
    /// have): the marker is not at this base, or the chain ends here.
    Exception,
    /// The read failed in another way (a timeout, a broken link).
    Failed(BatchFailure),
}

/// What the walk asks for next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Read `count` holding registers from `at` with FC 3.
    Read {
        /// First register.
        at: u16,
        /// Registers.
        count: u16,
    },
    /// The walk is over; [`Walk::found`] has the result.
    Done,
    /// The walk failed.
    Failed(DiscoveryError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Marker(usize),
    Header(u16),
    Body(usize),
    Done,
    Failed(DiscoveryError),
}

/// The discovery of one device: say what to read with [`Walk::step`],
/// answer with [`Walk::feed`] until [`Step::Done`].
#[derive(Clone, Debug)]
pub struct Walk<'d> {
    discovery: &'d Discovery,
    phase: Phase,
    found: Found,
}

impl<'d> Walk<'d> {
    /// A walk over `discovery`.
    pub fn new(discovery: &'d Discovery) -> Self {
        Self {
            discovery,
            phase: Phase::Marker(0),
            found: Found::default(),
        }
    }

    /// The next read, or the end.
    pub fn step(&self) -> Step {
        match self.phase {
            Phase::Marker(i) => match self.discovery.bases.get(i) {
                Some(&at) => Step::Read { at, count: 2 },
                None => Step::Failed(DiscoveryError::NoMarker),
            },
            Phase::Header(at) => Step::Read { at, count: 2 },
            Phase::Body(i) => match self.found.models.get(i) {
                Some(m) => Step::Read {
                    at: m.body(),
                    count: m.len,
                },
                None => Step::Done,
            },
            Phase::Done => Step::Done,
            Phase::Failed(e) => Step::Failed(e),
        }
    }

    /// The answer to the read of the last [`Walk::step`].
    pub fn feed(&mut self, answer: Result<&[u16], ReadError>) {
        self.phase = match (self.phase, answer) {
            (Phase::Done | Phase::Failed(_), _) => self.phase,
            (_, Err(ReadError::Failed(f))) => Phase::Failed(DiscoveryError::Read(f)),
            (Phase::Marker(i), Ok(words)) if words == MARKER => {
                let base = self.discovery.bases.get(i).copied().unwrap_or(0);
                self.found.base = base;
                match base.checked_add(2) {
                    Some(at) => Phase::Header(at),
                    None => Phase::Failed(DiscoveryError::Malformed),
                }
            }
            (Phase::Marker(i), _) => Phase::Marker(i + 1),
            (Phase::Header(_), Err(ReadError::Exception)) => self.next_body(0),
            (Phase::Header(at), Ok(&[id, len])) => self.header(at, id, len),
            (Phase::Header(_), Ok(_)) => Phase::Failed(DiscoveryError::Malformed),
            (Phase::Body(i), Ok(words)) => {
                if let Some(slot) = self.found.bodies.get_mut(i) {
                    *slot = Some(words.to_vec());
                }
                self.next_body(i + 1)
            }
            (Phase::Body(i), Err(ReadError::Exception)) => self.next_body(i + 1),
        };
    }

    fn header(&mut self, at: u16, id: u16, len: u16) -> Phase {
        if id == END {
            return self.next_body(0);
        }
        if self.found.chain.len() >= self.discovery.max_models {
            return Phase::Failed(DiscoveryError::TooManyModels);
        }
        let model = ModelRef { id, at, len };
        self.found.chain.push(model);
        if self.discovery.accepts(id) {
            self.found.models.push(model);
            self.found.bodies.push(None);
        } else {
            self.found.ignored.push(id);
        }
        let next = u32::from(at) + 2 + u32::from(len);
        if next + 1 > 65_535 {
            return Phase::Failed(DiscoveryError::Malformed);
        }
        Phase::Header(next as u16)
    }

    fn next_body(&self, from: usize) -> Phase {
        let next = self
            .found
            .models
            .iter()
            .enumerate()
            .skip(from)
            .find(|(_, m)| definition(m.id).is_some() && m.len <= MAX_READ)
            .map(|(i, _)| i);
        match next {
            Some(i) => Phase::Body(i),
            None => Phase::Done,
        }
    }

    /// The result, once [`Walk::step`] is [`Step::Done`].
    pub fn found(self) -> Found {
        self.found
    }
}

/// What discovery found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// The base where the marker was.
    pub base: u16,
    /// Every model of the chain, in order.
    pub chain: Vec<ModelRef>,
    /// The accepted models, in chain order: the map.
    pub models: Vec<ModelRef>,
    /// The ids of the models `[discovery] models` did not accept.
    pub ignored: Vec<u16>,
    /// The body of each accepted model this module knows, as read.
    pub bodies: Vec<Option<Vec<u16>>>,
}

/// The identity in model 1.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    /// `Mn`.
    pub vendor: String,
    /// `Md`.
    pub model: String,
    /// `Vr`.
    pub firmware: String,
    /// `SN`.
    pub serial: String,
}

/// The text of SunSpec string registers: two characters per register, the
/// first in the high byte, padded with NUL.
pub fn text(words: &[u16]) -> String {
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

/// A scale factor: `0x8000` is "not implemented".
const NOT_IMPLEMENTED: i16 = i16::MIN;

impl Found {
    /// The identity of model 1, if the chain has it and its body was read.
    pub fn identity(&self) -> Option<Identity> {
        let i = self.models.iter().position(|m| m.id == 1)?;
        let body = self.bodies.get(i)?.as_ref()?;
        let field = |from: usize, n: usize| text(body.get(from..from + n).unwrap_or(&[]));
        Some(Identity {
            vendor: field(0, 16),
            model: field(16, 16),
            firmware: field(40, 8),
            serial: field(48, 16),
        })
    }

    /// The path prefix of each accepted model: `m<id>`, and `m<id>_<n>`
    /// for the n-th repetition (n ≥ 2) of the same id.
    pub fn prefixes(&self) -> Vec<String> {
        let mut seen: BTreeMap<u16, usize> = BTreeMap::new();
        self.models
            .iter()
            .map(|m| {
                let n = seen.entry(m.id).or_insert(0);
                *n += 1;
                if *n == 1 {
                    format!("m{}", m.id)
                } else {
                    format!("m{}_{}", m.id, n)
                }
            })
            .collect()
    }

    /// The scale factors read, by `<prefix>.<point>` of their point.
    pub fn scales(&self) -> BTreeMap<String, i16> {
        let mut out = BTreeMap::new();
        for ((m, body), prefix) in self.models.iter().zip(&self.bodies).zip(self.prefixes()) {
            let (Some(def), Some(body)) = (definition(m.id), body) else {
                continue;
            };
            for p in def.points.iter().filter(|p| p.kind == Kind::ScaleFactor) {
                if let Some(raw) = body.get(usize::from(p.offset)) {
                    out.insert(format!("{prefix}.{}", snake(p.name)), *raw as i16);
                }
            }
        }
        out
    }

    /// The effective map the driver reports at `connect`: the accepted
    /// models in chain order and the scale factors read, as deterministic
    /// CBOR (ref/06 §2.11, ADR 0077).
    pub fn map(&self) -> Vec<u8> {
        let models = self
            .models
            .iter()
            .map(|m| {
                Value::map([
                    ("id", Value::Uint(u64::from(m.id))),
                    ("at", Value::Uint(u64::from(m.at))),
                    ("len", Value::Uint(u64::from(m.len))),
                ])
            })
            .collect();
        let scales = self
            .scales()
            .into_iter()
            .map(|(k, v)| (k, Value::Int(i64::from(v))))
            .collect();
        Value::map([
            ("models", Value::Array(models)),
            ("scales", Value::Map(scales)),
        ])
        .encode()
    }

    /// `sha256:<hex>` of [`Found::map`]: the `digest` of
    /// `[discovered.<device>]`.
    pub fn digest(&self) -> String {
        digest_of(&self.map())
    }

    /// The `[discovered.<device>]` entry of the lock (ref/02 §2.1.11) for
    /// `device`, discovered by `driver` with `profile` at `at` (RFC 3339).
    pub fn lock_entry(&self, device: &str, driver: &str, profile: &str, at: &str) -> String {
        let mut out = format!("[discovered.{device}]\n");
        out += &format!("driver = {}\n", quoted(driver));
        out += &format!("profile = {}\n", quoted(profile));
        out += &format!("at = {}\n", quoted(at));
        if let Some(id) = self.identity() {
            for (key, value) in [
                ("vendor", &id.vendor),
                ("model", &id.model),
                ("serial", &id.serial),
                ("firmware", &id.firmware),
            ] {
                if !value.is_empty() {
                    out += &format!("{key} = {}\n", quoted(value));
                }
            }
        }
        out += &format!("digest = {}\n", quoted(&self.digest()));
        out += "models = [\n";
        for m in &self.models {
            out += &format!("  {{ id = {}, at = {}, len = {} }},\n", m.id, m.at, m.len);
        }
        out += "]\n";
        out
    }
}

/// `sha256:<hex>` of `bytes`.
pub fn digest_of(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut out = String::from("sha256:");
    for b in hash {
        out += &format!("{b:02x}");
    }
    out
}

/// How a discovered map differs from the one pinned in the lock (ref/02
/// §3.1.5, `J0401 kind=map`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapDrift {
    /// The digest pinned.
    pub locked: String,
    /// The digest found.
    pub actual: String,
}

/// Compares a discovered map with the map pinned in the lock
/// (`device-spec.discovered`); `None` when they are the same map.
pub fn drift(pinned: Cbor<'_>, found: &Found) -> Option<MapDrift> {
    let locked = digest_of(pinned.as_bytes());
    let actual = found.digest();
    (locked != actual).then_some(MapDrift { locked, actual })
}

/// Reports what discovery found through `connect` (ref/06 §2.11): the
/// identity of model 1 and the effective map. When the lock pins a map
/// (`device-spec.discovered`) and the one found differs, `connect` fails
/// with `J0401 kind=map`: the addresses the plan reads come from the
/// pinned map, and reading them on a different chain would read other
/// registers (ADR 0077).
pub fn report(
    found: &Found,
    pinned: Option<&[u8]>,
    out: &mut ConnectOut<'_>,
) -> Result<(), DriverError> {
    let identity = found.identity().unwrap_or_default();
    let map = found.map();
    let info = DeviceInfo {
        vendor: present(&identity.vendor),
        model: present(&identity.model),
        serial: present(&identity.serial),
        firmware: present(&identity.firmware),
        hw_revision: None,
        cert_sha256: None,
        extra: Cbor::EMPTY_MAP,
    };
    out.device(0, &info, Some(Cbor::new(&map)));
    match pinned.and_then(|p| drift(Cbor::new(p), found)) {
        Some(_) => Err(DriverError::new(
            code!("J0401"),
            "kind=map: the SunSpec chain is not the one pinned in the lock",
        )),
        None => Ok(()),
    }
}

fn present(s: &str) -> Option<&str> {
    (!s.is_empty()).then_some(s)
}

fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            c if c.is_control() => out += &format!("\\u{:04X}", u32::from(c)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What a point of a model is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A value, scaled by `sf` when it has one.
    Value,
    /// A scale factor (`sunssf`): the power of ten of the points that name
    /// it.
    ScaleFactor,
    /// Text, `len` registers; not a signal of the generated profile.
    Text,
}

/// One point of a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointDef {
    /// The SunSpec name.
    pub name: &'static str,
    /// Its register in the body.
    pub offset: u16,
    /// The raw type (ref/03 §2.2.7).
    pub ty: &'static str,
    /// The unit; empty for a number without unit.
    pub unit: &'static str,
    /// The scale factor point.
    pub sf: Option<&'static str>,
    /// What the point is.
    pub kind: Kind,
    /// The enum of the point, in `[enums]` of the generated profile.
    pub enumeration: Option<&'static str>,
}

/// A model this module knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelDef {
    /// The model id.
    pub id: u16,
    /// Its name in the SunSpec information model.
    pub label: &'static str,
    /// Its fixed points, in register order.
    pub points: &'static [PointDef],
}

const fn value(
    name: &'static str,
    offset: u16,
    ty: &'static str,
    unit: &'static str,
    sf: &'static str,
) -> PointDef {
    PointDef {
        name,
        offset,
        ty,
        unit,
        sf: Some(sf),
        kind: Kind::Value,
        enumeration: None,
    }
}

const fn plain(name: &'static str, offset: u16, ty: &'static str) -> PointDef {
    PointDef {
        name,
        offset,
        ty,
        unit: "",
        sf: None,
        kind: Kind::Value,
        enumeration: None,
    }
}

const fn sf(name: &'static str, offset: u16) -> PointDef {
    PointDef {
        name,
        offset,
        ty: "i16",
        unit: "",
        sf: None,
        kind: Kind::ScaleFactor,
        enumeration: None,
    }
}

const fn string(name: &'static str, offset: u16) -> PointDef {
    PointDef {
        name,
        offset,
        ty: "string",
        unit: "",
        sf: None,
        kind: Kind::Text,
        enumeration: None,
    }
}

/// The enum of the operating state of models 101–103.
pub const INVERTER_STATE: (&str, &[(&str, u16)]) = (
    "InverterState",
    &[
        ("off", 1),
        ("sleeping", 2),
        ("starting", 3),
        ("mppt", 4),
        ("throttled", 5),
        ("shutting_down", 6),
        ("fault", 7),
        ("standby", 8),
    ],
);

const COMMON: &[PointDef] = &[
    string("Mn", 0),
    string("Md", 16),
    string("Opt", 32),
    string("Vr", 40),
    string("SN", 48),
    plain("DA", 64, "u16"),
];

const INVERTER: &[PointDef] = &[
    value("A", 0, "u16", "A", "A_SF"),
    value("AphA", 1, "u16", "A", "A_SF"),
    value("AphB", 2, "u16", "A", "A_SF"),
    value("AphC", 3, "u16", "A", "A_SF"),
    sf("A_SF", 4),
    value("PPVphAB", 5, "u16", "V", "V_SF"),
    value("PPVphBC", 6, "u16", "V", "V_SF"),
    value("PPVphCA", 7, "u16", "V", "V_SF"),
    value("PhVphA", 8, "u16", "V", "V_SF"),
    value("PhVphB", 9, "u16", "V", "V_SF"),
    value("PhVphC", 10, "u16", "V", "V_SF"),
    sf("V_SF", 11),
    value("W", 12, "i16", "W", "W_SF"),
    sf("W_SF", 13),
    value("Hz", 14, "u16", "Hz", "Hz_SF"),
    sf("Hz_SF", 15),
    value("VA", 16, "i16", "VA", "VA_SF"),
    sf("VA_SF", 17),
    value("VAr", 18, "i16", "var", "VAr_SF"),
    sf("VAr_SF", 19),
    value("PF", 20, "i16", "%", "PF_SF"),
    sf("PF_SF", 21),
    value("WH", 22, "u32", "Wh", "WH_SF"),
    sf("WH_SF", 24),
    value("DCA", 25, "u16", "A", "DCA_SF"),
    sf("DCA_SF", 26),
    value("DCV", 27, "u16", "V", "DCV_SF"),
    sf("DCV_SF", 28),
    value("DCW", 29, "i16", "W", "DCW_SF"),
    sf("DCW_SF", 30),
    value("TmpCab", 31, "i16", "\u{b0}C", "Tmp_SF"),
    value("TmpSnk", 32, "i16", "\u{b0}C", "Tmp_SF"),
    value("TmpTrns", 33, "i16", "\u{b0}C", "Tmp_SF"),
    value("TmpOt", 34, "i16", "\u{b0}C", "Tmp_SF"),
    sf("Tmp_SF", 35),
    PointDef {
        enumeration: Some(INVERTER_STATE.0),
        ..plain("St", 36, "u16")
    },
    plain("StVnd", 37, "u16"),
    plain("Evt1", 38, "bits32"),
    plain("Evt2", 40, "bits32"),
    plain("EvtVnd1", 42, "bits32"),
    plain("EvtVnd2", 44, "bits32"),
    plain("EvtVnd3", 46, "bits32"),
    plain("EvtVnd4", 48, "bits32"),
];

const METER: &[PointDef] = &[
    value("A", 0, "i16", "A", "A_SF"),
    value("AphA", 1, "i16", "A", "A_SF"),
    value("AphB", 2, "i16", "A", "A_SF"),
    value("AphC", 3, "i16", "A", "A_SF"),
    sf("A_SF", 4),
    value("PhV", 5, "i16", "V", "V_SF"),
    value("PhVphA", 6, "i16", "V", "V_SF"),
    value("PhVphB", 7, "i16", "V", "V_SF"),
    value("PhVphC", 8, "i16", "V", "V_SF"),
    value("PPV", 9, "i16", "V", "V_SF"),
    value("PhVphAB", 10, "i16", "V", "V_SF"),
    value("PhVphBC", 11, "i16", "V", "V_SF"),
    value("PhVphCA", 12, "i16", "V", "V_SF"),
    sf("V_SF", 13),
    value("Hz", 14, "i16", "Hz", "Hz_SF"),
    sf("Hz_SF", 15),
    value("W", 16, "i16", "W", "W_SF"),
    value("WphA", 17, "i16", "W", "W_SF"),
    value("WphB", 18, "i16", "W", "W_SF"),
    value("WphC", 19, "i16", "W", "W_SF"),
    sf("W_SF", 20),
    value("VA", 21, "i16", "VA", "VA_SF"),
    value("VAphA", 22, "i16", "VA", "VA_SF"),
    value("VAphB", 23, "i16", "VA", "VA_SF"),
    value("VAphC", 24, "i16", "VA", "VA_SF"),
    sf("VA_SF", 25),
    value("VAR", 26, "i16", "var", "VAR_SF"),
    value("VARphA", 27, "i16", "var", "VAR_SF"),
    value("VARphB", 28, "i16", "var", "VAR_SF"),
    value("VARphC", 29, "i16", "var", "VAR_SF"),
    sf("VAR_SF", 30),
    value("PF", 31, "i16", "%", "PF_SF"),
    value("PFphA", 32, "i16", "%", "PF_SF"),
    value("PFphB", 33, "i16", "%", "PF_SF"),
    value("PFphC", 34, "i16", "%", "PF_SF"),
    sf("PF_SF", 35),
    value("TotWhExp", 36, "u32", "Wh", "TotWh_SF"),
    value("TotWhExpPhA", 38, "u32", "Wh", "TotWh_SF"),
    value("TotWhExpPhB", 40, "u32", "Wh", "TotWh_SF"),
    value("TotWhExpPhC", 42, "u32", "Wh", "TotWh_SF"),
    value("TotWhImp", 44, "u32", "Wh", "TotWh_SF"),
    value("TotWhImpPhA", 46, "u32", "Wh", "TotWh_SF"),
    value("TotWhImpPhB", 48, "u32", "Wh", "TotWh_SF"),
    value("TotWhImpPhC", 50, "u32", "Wh", "TotWh_SF"),
    sf("TotWh_SF", 52),
];

/// The models this module knows: the common model, the integer inverter
/// models and the fixed head of the integer meter models (ADR 0077).
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: 1,
        label: "common",
        points: COMMON,
    },
    ModelDef {
        id: 101,
        label: "inverter (single phase)",
        points: INVERTER,
    },
    ModelDef {
        id: 102,
        label: "inverter (split phase)",
        points: INVERTER,
    },
    ModelDef {
        id: 103,
        label: "inverter (three phase)",
        points: INVERTER,
    },
    ModelDef {
        id: 201,
        label: "meter (single phase)",
        points: METER,
    },
    ModelDef {
        id: 202,
        label: "meter (split phase)",
        points: METER,
    },
    ModelDef {
        id: 203,
        label: "meter (wye, three phase)",
        points: METER,
    },
    ModelDef {
        id: 204,
        label: "meter (delta, three phase)",
        points: METER,
    },
];

/// The definition of model `id`, if this module knows it.
pub fn definition(id: u16) -> Option<&'static ModelDef> {
    MODELS.iter().find(|m| m.id == id)
}

/// A SunSpec point name in `snake_case`: `PPVphAB` is `pp_vph_ab`, `W_SF`
/// is `w_sf`, `TotWhExpPhA` is `tot_wh_exp_ph_a`.
pub fn snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(char::is_ascii_lowercase);
            let boundary = prev.is_ascii_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_ascii_uppercase() && next_lower);
            if boundary && !out.ends_with('_') {
                out.push('_');
            }
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn scale_literal(sf: i16) -> String {
    if sf >= 0 {
        format!("{}", 10u64.pow(u32::from(sf.unsigned_abs())))
    } else {
        format!("0.{}1", "0".repeat(usize::from(sf.unsigned_abs()) - 1))
    }
}

/// The profile generated from a discovered map (ADR 0077), in the format
/// of `jk profile import` (ref/03 §2.2.19): a normal `modbus` profile with
/// one input per point of each model this module knows, at its absolute
/// address, with its raw type and unit. The scale factors read at
/// discovery are written as each point's `scale`, because `scale_factor`
/// is not read by this runtime (ADR 0064); a change of a scale factor
/// changes the map's digest, so it is a drift of the map. `names` renames
/// points (`"<model>.<point>" → path`, ref/03 §2.2.15). What discovery
/// cannot decide is left in `# REVIEW` lines.
pub fn profile(found: &Found, name: &str, names: &BTreeMap<String, String>) -> String {
    let identity = found.identity().unwrap_or_default();
    let scales = found.scales();
    let mut out = String::new();
    out += &format!(
        "# Generated by SunSpec discovery: marker at {}, models {}. Resolve the \"# REVIEW\" lines and run jk profile test.\n",
        found.base,
        found
            .models
            .iter()
            .map(|m| m.id.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    out += "schema = 1\n\n[profile]\n";
    out += &format!("name    = {}\n", quoted(name));
    out += "version = \"1.0.0\"\n";
    out += "driver  = \"modbus\"\n";
    if !identity.vendor.is_empty() {
        out += &format!("vendor  = {}\n", quoted(&identity.vendor));
    }
    if !identity.model.is_empty() {
        out += &format!("model   = {}\n", quoted(&identity.model));
    }
    out += &format!(
        "# REVIEW: firmware {} and serial {} were read at discovery; the lock pins them in [discovered]\n",
        quoted(&identity.firmware),
        quoted(&identity.serial)
    );
    out += "# REVIEW: quirks: jk profile test measures the latency and suggests quirks.latency\n";
    out += "\n[defaults]\npoll = \"1s\"   # REVIEW: read rate\n";
    let needs_state = found
        .models
        .iter()
        .any(|m| matches!(m.id, 101..=103) && definition(m.id).is_some());
    if needs_state {
        out += "\n[enums]\n";
        let (enum_name, variants) = INVERTER_STATE;
        let body = variants
            .iter()
            .map(|(v, n)| format!("{v} = {n}"))
            .collect::<Vec<_>>()
            .join(", ");
        out += &format!("{enum_name} = {{ {body} }}\n");
    }
    out += "\n[signals]\n";
    for (m, prefix) in found.models.iter().zip(found.prefixes()) {
        let Some(def) = definition(m.id) else {
            out += &format!(
                "# REVIEW: model {} at {} ({} registers) has no built-in definition; map its points by hand\n",
                m.id, m.at, m.len
            );
            continue;
        };
        out += &format!("# model {} ({}) at {}\n", m.id, def.label, m.at);
        for p in def.points {
            if p.kind == Kind::Text || p.offset >= m.len {
                continue;
            }
            let key = format!("{}.{}", m.id, p.name);
            let path = names
                .get(&key)
                .cloned()
                .unwrap_or_else(|| format!("{prefix}.{}", snake(p.name)));
            let mut fields = vec![
                format!("at = {}", u32::from(m.body()) + u32::from(p.offset)),
                "fc = 3".to_string(),
                format!("type = {}", quoted(p.ty)),
            ];
            if let Some(e) = p.enumeration {
                fields.push(format!("enum = {}", quoted(e)));
            }
            let mut review = None;
            if p.kind == Kind::Value
                && let Some(sf_name) = p.sf
            {
                match scales.get(&format!("{prefix}.{}", snake(sf_name))) {
                    Some(&NOT_IMPLEMENTED) => {
                        review = Some(format!("{sf_name} is not implemented by the equipment"));
                    }
                    Some(&s) if (-10..=10).contains(&s) => {
                        if s != 0 {
                            fields.push(format!("scale = {}", scale_literal(s)));
                        }
                        review = Some(format!("scale from {sf_name} = {s} at discovery"));
                    }
                    Some(&s) => review = Some(format!("{sf_name} = {s} is not a usable scale")),
                    None => review = Some(format!("{sf_name} was not read")),
                }
            }
            if !p.unit.is_empty() {
                fields.push(format!("unit = {}", quoted(p.unit)));
            }
            fields.push(format!("label = {}", quoted(p.name)));
            out += &format!("{} = {{ {} }}", quoted(&path), fields.join(", "));
            if let Some(r) = review {
                out += &format!("   # REVIEW: {r}");
            }
            out.push('\n');
        }
    }
    if !found.ignored.is_empty() {
        out += &format!(
            "# models not accepted by [discovery] models: {}\n",
            found
                .ignored
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    out += "\n# REVIEW: [fallback]: does the equipment have a communication fallback? (ref/03 §2.2.11)\n";
    out
}

/// The register image of a simulated SunSpec device: the marker at `base`,
/// then each model as added, then the end of the chain.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Image {
    next: u16,
    /// The holding registers, by address.
    pub registers: BTreeMap<u16, u16>,
}

impl Image {
    /// An image with the marker at `base`.
    pub fn new(base: u16) -> Self {
        let mut registers = BTreeMap::new();
        registers.insert(base, MARKER[0]);
        registers.insert(base.wrapping_add(1), MARKER[1]);
        Self {
            next: base.wrapping_add(2),
            registers,
        }
    }

    /// Adds model `id` with `body`; returns where its id register is.
    pub fn model(&mut self, id: u16, body: &[u16]) -> u16 {
        let at = self.next;
        self.registers.insert(at, id);
        self.registers.insert(at.wrapping_add(1), body.len() as u16);
        for (i, w) in body.iter().enumerate() {
            self.registers.insert(at.wrapping_add(2 + i as u16), *w);
        }
        self.next = at.wrapping_add(2 + body.len() as u16);
        at
    }

    /// Ends the chain.
    pub fn end(&mut self) {
        self.registers.insert(self.next, END);
        self.registers.insert(self.next.wrapping_add(1), 0);
    }

    /// The registers `at..at + count`, or `None` when one is missing (the
    /// answer of an equipment is then exception 2).
    pub fn read(&self, at: u16, count: u16) -> Option<Vec<u16>> {
        (0..count)
            .map(|i| self.registers.get(&at.checked_add(i)?).copied())
            .collect()
    }
}

/// Text in `n` SunSpec string registers.
pub fn text_registers(s: &str, n: usize) -> Vec<u16> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.resize(2 * n, 0);
    bytes
        .chunks(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect()
}

/// The body of a model 1 with this identity (66 registers).
pub fn common_body(vendor: &str, model: &str, firmware: &str, serial: &str) -> Vec<u16> {
    let mut body = text_registers(vendor, 16);
    body.extend(text_registers(model, 16));
    body.extend(text_registers("", 8));
    body.extend(text_registers(firmware, 8));
    body.extend(text_registers(serial, 16));
    body.extend([1, 0]);
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> Image {
        let mut image = Image::new(40000);
        image.model(1, &common_body("ACME", "PCS-125", "4.12.0", "A1B2C3"));
        let mut inverter = vec![0u16; 50];
        inverter[12] = 1234;
        inverter[13] = (-1i16) as u16;
        inverter[4] = (-2i16) as u16;
        image.model(103, &inverter);
        image.end();
        image
    }

    fn run(image: &Image, d: &Discovery) -> Result<(Found, Vec<(u16, u16)>), DiscoveryError> {
        let mut walk = Walk::new(d);
        let mut reads = Vec::new();
        loop {
            match walk.step() {
                Step::Read { at, count } => {
                    reads.push((at, count));
                    let words = image.read(at, count);
                    walk.feed(words.as_deref().ok_or(ReadError::Exception));
                }
                Step::Done => return Ok((walk.found(), reads)),
                Step::Failed(e) => return Err(e),
            }
        }
    }

    #[test]
    fn the_walk_finds_the_marker_and_the_chain() {
        let (found, reads) = run(&device(), &Discovery::default()).unwrap();
        assert_eq!(found.base, 40000);
        assert_eq!(
            found.models,
            [
                ModelRef {
                    id: 1,
                    at: 40002,
                    len: 66
                },
                ModelRef {
                    id: 103,
                    at: 40070,
                    len: 50
                }
            ]
        );
        assert_eq!(
            reads,
            [
                (40000, 2),
                (40002, 2),
                (40070, 2),
                (40122, 2),
                (40004, 66),
                (40072, 50)
            ]
        );
        let id = found.identity().unwrap();
        assert_eq!(
            (
                id.vendor.as_str(),
                id.model.as_str(),
                id.firmware.as_str(),
                id.serial.as_str()
            ),
            ("ACME", "PCS-125", "4.12.0", "A1B2C3")
        );
        assert_eq!(found.scales().get("m103.w_sf"), Some(&-1));
        assert_eq!(found.scales().get("m103.a_sf"), Some(&-2));
    }

    #[test]
    fn the_marker_is_looked_for_at_each_base_in_order() {
        let mut image = Image::new(0);
        image.model(103, &[0; 50]);
        image.end();
        let (found, reads) = run(&image, &Discovery::default()).unwrap();
        assert_eq!(found.base, 0);
        assert_eq!(&reads[..3], [(40000, 2), (50000, 2), (0, 2)]);
        assert_eq!(
            run(&Image::default(), &Discovery::default()).unwrap_err(),
            DiscoveryError::NoMarker
        );
    }

    #[test]
    fn a_chain_without_its_end_ends_at_the_first_missing_header() {
        let mut image = Image::new(40000);
        image.model(103, &[0; 50]);
        let (found, _) = run(&image, &Discovery::default()).unwrap();
        assert_eq!(found.models.len(), 1);
    }

    #[test]
    fn models_outside_the_accepted_list_are_ignored_and_max_models_bounds_the_walk() {
        let d = Discovery {
            models: vec![103],
            ..Discovery::default()
        };
        let (found, _) = run(&device(), &d).unwrap();
        assert_eq!(found.models.len(), 1);
        assert_eq!(found.ignored, [1]);
        assert_eq!(found.chain.len(), 2);
        let d = Discovery {
            max_models: 1,
            ..Discovery::default()
        };
        assert_eq!(
            run(&device(), &d).unwrap_err(),
            DiscoveryError::TooManyModels
        );
    }

    #[test]
    fn a_failed_read_fails_the_walk() {
        let d = Discovery::default();
        let mut walk = Walk::new(&d);
        walk.feed(Err(ReadError::Failed(BatchFailure::Timeout)));
        assert_eq!(
            walk.step(),
            Step::Failed(DiscoveryError::Read(BatchFailure::Timeout))
        );
    }

    #[test]
    fn the_map_and_its_digest_change_with_the_chain_and_the_scales() {
        let (a, _) = run(&device(), &Discovery::default()).unwrap();
        let (b, _) = run(&device(), &Discovery::default()).unwrap();
        assert_eq!(a.digest(), b.digest());
        assert!(drift(Cbor::new(&a.map()), &b).is_none());
        let mut shifted = Image::new(40000);
        shifted.model(1, &common_body("ACME", "PCS-125", "4.12.0", "A1B2C3"));
        shifted.model(120, &[0; 26]);
        let mut inverter = vec![0u16; 50];
        inverter[13] = (-1i16) as u16;
        inverter[4] = (-2i16) as u16;
        shifted.model(103, &inverter);
        shifted.end();
        let (c, _) = run(&shifted, &Discovery::default()).unwrap();
        let d = drift(Cbor::new(&a.map()), &c).unwrap();
        assert_eq!(d.locked, a.digest());
        assert_eq!(d.actual, c.digest());
        let mut rescaled = device();
        rescaled.registers.insert(40072 + 13, 0);
        let (e, _) = run(&rescaled, &Discovery::default()).unwrap();
        assert!(drift(Cbor::new(&a.map()), &e).is_some(), "W_SF changed");
    }

    #[test]
    fn the_lock_entry_pins_identity_digest_and_models() {
        let (found, _) = run(&device(), &Discovery::default()).unwrap();
        let entry = found.lock_entry(
            "pcs",
            "modbus-tcp",
            "joker/sunspec@1",
            "2026-10-25T10:00:00Z",
        );
        let expected = format!(
            "[discovered.pcs]\ndriver = \"modbus-tcp\"\nprofile = \"joker/sunspec@1\"\nat = \"2026-10-25T10:00:00Z\"\nvendor = \"ACME\"\nmodel = \"PCS-125\"\nserial = \"A1B2C3\"\nfirmware = \"4.12.0\"\ndigest = \"{}\"\nmodels = [\n  {{ id = 1, at = 40002, len = 66 }},\n  {{ id = 103, at = 40070, len = 50 }},\n]\n",
            found.digest()
        );
        assert_eq!(entry, expected);
        assert!(found.digest().starts_with("sha256:"));
        assert_eq!(found.digest().len(), 7 + 64);
    }

    #[test]
    fn discovery_reads_its_quirks() {
        let q = Value::map([(
            "discovery",
            Value::map([
                ("kind", Value::text("sunspec")),
                ("bases", Value::Array(vec![Value::Uint(50000)])),
                ("models", Value::Array(vec![Value::Uint(103)])),
                ("max_models", Value::Uint(8)),
            ]),
        )])
        .encode();
        assert_eq!(
            Discovery::parse(Cbor::new(&q)),
            Ok(Some(Discovery {
                bases: vec![50000],
                models: vec![103],
                max_models: 8
            }))
        );
        let none = Value::map([("max_regs", Value::Uint(10))]).encode();
        assert_eq!(Discovery::parse(Cbor::new(&none)), Ok(None));
        let other = Value::map([(
            "discovery",
            Value::map([("kind", Value::text("opcua_browse"))]),
        )])
        .encode();
        assert!(Discovery::parse(Cbor::new(&other)).is_err());
    }

    #[test]
    fn names_become_snake_case() {
        assert_eq!(snake("PPVphAB"), "pp_vph_ab");
        assert_eq!(snake("W_SF"), "w_sf");
        assert_eq!(snake("TotWhExpPhA"), "tot_wh_exp_ph_a");
        assert_eq!(snake("AphA"), "aph_a");
        assert_eq!(snake("DCA"), "dca");
        assert_eq!(snake("TmpCab"), "tmp_cab");
        assert_eq!(snake("Evt1"), "evt1");
        assert_eq!(scale_literal(-1), "0.1");
        assert_eq!(scale_literal(-3), "0.001");
        assert_eq!(scale_literal(2), "100");
    }

    #[test]
    fn the_generated_profile_scales_and_names_the_points() {
        let (found, _) = run(&device(), &Discovery::default()).unwrap();
        let mut names = BTreeMap::new();
        names.insert("103.Hz".to_string(), "ac.f".to_string());
        let text = profile(&found, "local/pcs-125", &names);
        assert!(text.contains("vendor  = \"ACME\""), "{text}");
        assert!(
            text.contains("\"m103.w\" = { at = 40084, fc = 3, type = \"i16\", scale = 0.1, unit = \"W\", label = \"W\" }   # REVIEW: scale from W_SF = -1 at discovery"),
            "{text}"
        );
        assert!(
            text.contains(
                "\"m103.a\" = { at = 40072, fc = 3, type = \"u16\", scale = 0.01, unit = \"A\""
            ),
            "{text}"
        );
        assert!(text.contains("\"ac.f\" = { at = 40086"), "{text}");
        assert!(
            text.contains(
                "\"m103.st\" = { at = 40108, fc = 3, type = \"u16\", enum = \"InverterState\""
            ),
            "{text}"
        );
        assert!(text.contains("InverterState = { off = 1,"), "{text}");
        assert!(
            !text.contains("m1.mn"),
            "text points are identity, not signals"
        );
        assert!(text.contains("\"m1.da\""), "{text}");
    }
}
