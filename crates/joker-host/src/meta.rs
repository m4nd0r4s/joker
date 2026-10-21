//! The `joker:meta` section of an app component (ref/04 §5,
//! `spec/cddl/joker-meta.cddl`): where it is, how its fragments merge, and
//! the keys the host uses at load (ref/04 §5.4).

use joker_core::ValueType;
use wasmparser::{Encoding, Parser, Payload};

use crate::cbor::{self, Item};
use crate::error::{HostError, codes};

/// Name of the custom section.
pub const SECTION: &str = "joker:meta";

/// Kind of an app port (`port-kind` of the CDDL).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PortKind {
    /// `#[input]`.
    Input,
    /// `#[output]`.
    Output,
    /// `#[param]`.
    Param,
    /// `#[internal]`.
    Internal,
    /// `#[publish]` of a `Published<T>`.
    Published,
    /// `#[publish]` of a `Counter`.
    Counter,
    /// `#[input(event)]`.
    EventIn,
    /// `#[output(event)]`.
    EventOut,
    /// `#[action]`.
    Action,
    /// `#[mailbox]`.
    Mailbox,
    /// `#[alarm]`.
    Alarm,
    /// `#[model]` (unstable `ml`).
    Model,
    /// Stream input (unstable `streams`).
    Stream,
    /// Block input (unstable `blocks`).
    Block,
}

impl PortKind {
    fn from_code(code: u64) -> Option<Self> {
        Some(match code {
            0 => Self::Input,
            1 => Self::Output,
            2 => Self::Param,
            3 => Self::Internal,
            4 => Self::Published,
            5 => Self::Counter,
            6 => Self::EventIn,
            7 => Self::EventOut,
            8 => Self::Action,
            9 => Self::Mailbox,
            10 => Self::Alarm,
            11 => Self::Model,
            12 => Self::Stream,
            13 => Self::Block,
            _ => return None,
        })
    }
}

/// A port declared by the app (key 6).
#[derive(Clone, Debug, PartialEq)]
pub struct MetaPort {
    /// Field name.
    pub name: String,
    /// Port kind.
    pub kind: PortKind,
    /// Signal type, for signal-like kinds.
    pub ty: Option<ValueType>,
    /// Declared unit.
    pub unit: Option<String>,
    /// May stay unbound.
    pub optional: bool,
    /// A `many` port.
    pub many: bool,
    /// Parameter default, as an `f64` or a boolean.
    pub default: Option<f64>,
}

/// Kind of an app task (`task-kind` of the CDDL).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TaskKind {
    /// `#[joker::cycle]`.
    Cycle,
    /// `#[joker::task]`.
    Async,
    /// `#[joker::on_action]`.
    Action,
}

/// A task of the app (key 7), in merged order: sorted by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetaTask {
    /// The function name.
    pub name: String,
    /// Task kind.
    pub kind: TaskKind,
    /// Requested period of a cycle task, ns.
    pub period_ns: Option<u64>,
    /// Declared WCET of a cycle task, ns.
    pub wcet_ns: Option<u64>,
    /// Disabled by `overrun = "degrade"`.
    pub optional: bool,
}

/// The merged metadata of an app, with the keys the host uses.
#[derive(Clone, Debug, PartialEq)]
pub struct AppMeta {
    /// App name (key 1).
    pub name: String,
    /// App version (key 2).
    pub version: String,
    /// WIT world the component was built against (key 3).
    pub world: String,
    /// SDK language (key 4.0).
    pub sdk_lang: String,
    /// Cycle ABI version (key 5).
    pub cycle_abi: u64,
    /// Ports (key 6), in the order of the port table.
    pub ports: Vec<MetaPort>,
    /// Tasks (key 7); the index is the task id of `on-cycle`.
    pub tasks: Vec<MetaTask>,
    /// Number of devices (key 8).
    pub devices: usize,
    /// The app implements a class (key 9).
    pub implements: bool,
    /// Unstable features used (key 10).
    pub features: Vec<String>,
    /// Suggested proposal buffer capacity (key 16.0).
    pub proposal_capacity: Option<u64>,
    /// Suggested cycle instance `memory.max` (key 16.1).
    pub cycle_memory_max: Option<u64>,
}

fn invalid(message: impl Into<String>) -> HostError {
    HostError::new(codes::META_INVALID, message)
}

/// Finds and merges the `joker:meta` of a component (ref/04 §5.1–§5.3): the
/// component-level section, which holds one canonical item, wins over the
/// fragments of the embedded core modules.
pub fn read(component: &[u8]) -> Result<AppMeta, HostError> {
    let mut depth = 0usize;
    let mut top: Option<&[u8]> = None;
    let mut fragments: Vec<&[u8]> = Vec::new();
    let mut is_component = false;
    for payload in Parser::new(0).parse_all(component) {
        let payload =
            payload.map_err(|e| HostError::new(codes::COMPONENT_INVALID, format!("{e}")))?;
        match payload {
            Payload::Version { encoding, .. } => {
                if depth == 0 {
                    is_component = encoding == Encoding::Component;
                }
                depth += 1;
            }
            Payload::End(_) => depth = depth.saturating_sub(1),
            Payload::CustomSection(section) if section.name() == SECTION => {
                if depth == 1 && is_component {
                    if top.is_some() {
                        return Err(invalid("two component-level joker:meta sections"));
                    }
                    top = Some(section.data());
                } else {
                    fragments.push(section.data());
                }
            }
            _ => {}
        }
    }
    if !is_component {
        return Err(HostError::new(codes::COMPONENT_INVALID, "not a component"));
    }
    let meta = match top {
        Some(bytes) => {
            let items = cbor::decode_sequence(bytes)
                .map_err(|e| invalid(format!("CBOR at {}", e.offset)))?;
            match items.as_slice() {
                [one] => one.clone(),
                _ => return Err(invalid("the component-level section holds one item")),
            }
        }
        None if fragments.is_empty() => return Err(invalid("no joker:meta section")),
        None => merge(&fragments)?,
    };
    parse(&meta)
}

fn merge(sections: &[&[u8]]) -> Result<Item, HostError> {
    let mut app: Option<Item> = None;
    let mut tasks: Vec<Item> = Vec::new();
    let mut schemas: Vec<Item> = Vec::new();
    for bytes in sections {
        let items =
            cbor::decode_sequence(bytes).map_err(|e| invalid(format!("CBOR at {}", e.offset)))?;
        for fragment in items {
            let body = fragment
                .get(1)
                .cloned()
                .ok_or_else(|| invalid("fragment without body"))?;
            match fragment.get(0).and_then(Item::as_u64) {
                Some(0) if app.is_none() => app = Some(body),
                Some(0) => return Err(invalid("more than one app fragment")),
                Some(1) => tasks.push(body),
                Some(2) => schemas.push(body),
                _ => return Err(invalid("unknown fragment kind")),
            }
        }
    }
    let app = app.ok_or_else(|| invalid("no app fragment"))?;
    tasks.sort_by(|a, b| name_of(a).cmp(name_of(b)));
    if tasks.windows(2).any(|w| name_of(&w[0]) == name_of(&w[1])) {
        return Err(invalid("two tasks with the same name"));
    }
    schemas.sort_by(|a, b| name_of(a).cmp(name_of(b)));
    let mut unique: Vec<Item> = Vec::with_capacity(schemas.len());
    for s in schemas {
        match unique.last() {
            Some(last) if name_of(last) == name_of(&s) && *last != s => {
                return Err(invalid(
                    "two schemas with the same name and different content",
                ));
            }
            Some(last) if name_of(last) == name_of(&s) => {}
            _ => unique.push(s),
        }
    }
    let Item::Map(mut entries) = app else {
        return Err(invalid("the app fragment is not a map"));
    };
    for (key, value) in entries.iter_mut() {
        match key.as_u64() {
            Some(7) => *value = Item::Array(core::mem::take(&mut tasks)),
            Some(11) => *value = Item::Array(core::mem::take(&mut unique)),
            _ => {}
        }
    }
    Ok(Item::Map(entries))
}

fn name_of(item: &Item) -> &[u8] {
    let key = if item.get(0).and_then(Item::as_str).is_some() {
        0
    } else {
        1
    };
    item.get(key)
        .and_then(Item::as_str)
        .map(str::as_bytes)
        .unwrap_or_default()
}

fn text(item: &Item, key: u64, what: &str) -> Result<String, HostError> {
    item.get(key)
        .and_then(Item::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("{what} (key {key}) missing")))
}

fn flag(item: &Item, key: u64) -> bool {
    item.get(key).and_then(Item::as_bool).unwrap_or(false)
}

fn parse(meta: &Item) -> Result<AppMeta, HostError> {
    if meta.get(0).and_then(Item::as_u64) != Some(1) {
        return Err(invalid("unknown joker:meta format version"));
    }
    let name = text(meta, 1, "app name")?;
    if !is_ident(&name) || name.starts_with("joker") {
        return Err(invalid(format!("invalid app name {name:?}")));
    }
    let version = text(meta, 2, "app version")?;
    let world = text(meta, 3, "world")?;
    if parse_world(&world).is_none() {
        return Err(invalid(format!("invalid world {world:?}")));
    }
    let sdk_lang = meta
        .get(4)
        .ok_or_else(|| invalid("sdk-info (key 4) missing"))
        .and_then(|sdk| text(sdk, 0, "sdk language"))?;
    let cycle_abi = meta
        .get(5)
        .and_then(Item::as_u64)
        .ok_or_else(|| invalid("cycle ABI (key 5) missing"))?;
    let list = |key: u64| -> Result<&[Item], HostError> {
        meta.get(key)
            .and_then(Item::as_array)
            .ok_or_else(|| invalid(format!("list (key {key}) missing")))
    };
    let ports = list(6)?
        .iter()
        .map(parse_port)
        .collect::<Result<Vec<_>, _>>()?;
    let tasks = list(7)?
        .iter()
        .map(parse_task)
        .collect::<Result<Vec<_>, _>>()?;
    let devices = list(8)?.len();
    let features = list(10)?
        .iter()
        .map(|f| {
            f.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("feature is not text"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    list(11)?;
    let limits = meta.get(16);
    Ok(AppMeta {
        name,
        version,
        world,
        sdk_lang,
        cycle_abi,
        ports,
        tasks,
        devices,
        implements: meta.get(9).is_some(),
        features,
        proposal_capacity: limits.and_then(|l| l.get(0)).and_then(Item::as_u64),
        cycle_memory_max: limits.and_then(|l| l.get(1)).and_then(Item::as_u64),
    })
}

fn parse_port(item: &Item) -> Result<MetaPort, HostError> {
    let name = text(item, 0, "port name")?;
    let kind = item
        .get(1)
        .and_then(Item::as_u64)
        .and_then(PortKind::from_code)
        .ok_or_else(|| invalid(format!("port {name}: invalid kind")))?;
    let ty = match item.get(2) {
        None => None,
        Some(t) => Some(
            t.as_u64()
                .and_then(|c| u8::try_from(c).ok())
                .and_then(ValueType::from_u8)
                .ok_or_else(|| invalid(format!("port {name}: invalid signal type")))?,
        ),
    };
    let default = match item.get(8) {
        Some(Item::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        Some(other) => other.as_f64(),
        None => None,
    };
    Ok(MetaPort {
        unit: item.get(3).and_then(Item::as_str).map(str::to_owned),
        optional: flag(item, 5),
        many: item.get(6).is_some(),
        name,
        kind,
        ty,
        default,
    })
}

fn parse_task(item: &Item) -> Result<MetaTask, HostError> {
    let name = text(item, 0, "task name")?;
    let kind = match item.get(1).and_then(Item::as_u64) {
        Some(0) => TaskKind::Cycle,
        Some(1) => TaskKind::Async,
        Some(2) => TaskKind::Action,
        _ => return Err(invalid(format!("task {name}: invalid kind"))),
    };
    Ok(MetaTask {
        period_ns: item.get(2).and_then(Item::as_u64),
        wcet_ns: item.get(3).and_then(Item::as_u64),
        optional: flag(item, 4),
        name,
        kind,
    })
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some('a'..='z'))
        && s.len() <= 32
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
}

/// The `(major, minor, patch)` of a `joker:runtime@x.y.z` world reference.
pub fn parse_world(world: &str) -> Option<(u64, u64, u64)> {
    let version = world.strip_prefix("joker:runtime@")?;
    let mut parts = version.split('.').map(|p| p.parse::<u64>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idents_and_worlds() {
        assert!(is_ident("light"));
        assert!(is_ident("peak_shaver2"));
        assert!(!is_ident("Light"));
        assert!(!is_ident("2light"));
        assert!(!is_ident(""));
        assert_eq!(parse_world("joker:runtime@0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_world("joker:runtime@0.1"), None);
        assert_eq!(parse_world("joker:driver@0.1.0"), None);
    }
}
