//! The immutable signal registry of a generation (docs/01 §3.1, docs/16 §6).
//!
//! Signals get dense [`SignalId`]s in the byte order of their canonical paths,
//! so the same set of signals always gives the same ids on every target.
//! Building never allocates: the caller hands in fixed-capacity storage (a
//! static array on an MCU). With the `alloc` feature, [`OwnedRegistry`] does the
//! same with vectors for the Linux builder (ADR 0020).

use crate::hash::Sha256;
use crate::value::ValueType;

/// Dense signal handle, valid only within one generation (ref/00 §5).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SignalId(pub u32);

impl SignalId {
    /// The id as an index into dense tables.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Dense cycle-group handle, in the byte order of the group names.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(pub u32);

impl GroupId {
    /// The id as an index into dense tables.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// The unit a signal is expressed in, as declared (ref/00 §4.1).
///
/// The core never converts units on the tick path: every value is already in
/// the unit of its signal. This is a narrow placeholder that will carry the
/// parsed unit of `joker-units` once that crate exposes it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitRef<'a>(pub &'a str);

impl UnitRef<'static> {
    /// The dimensionless unit `1`.
    pub const ONE: Self = Self("1");
}

/// Direction of a signal.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Direction {
    /// Written by a driver's acquisition stage.
    Input = 0,
    /// Written by the arbiter and envelope of its owner group.
    Output = 1,
    /// Derived or published by a cycle group (`calc.*`, `app.*`, `alarm.*`).
    Derived = 2,
}

/// Metadata of one signal (docs/01 §3.1).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SignalMeta<'a> {
    /// Canonical path (docs/02 §9).
    pub path: &'a str,
    /// Signal type.
    pub ty: ValueType,
    /// Declared unit.
    pub unit: UnitRef<'a>,
    /// Direction.
    pub direction: Direction,
    /// Owner group (outputs and derived signals), by name until the registry
    /// is built.
    pub group: Option<&'a str>,
    /// Resolved owner group, set by the build.
    pub group_id: Option<GroupId>,
    /// `max_age` in ns; `None` never ages.
    pub max_age_ns: Option<u64>,
}

impl<'a> SignalMeta<'a> {
    /// Placeholder used to initialise fixed-capacity storage.
    pub const EMPTY: SignalMeta<'static> = SignalMeta {
        path: "",
        ty: ValueType::F64,
        unit: UnitRef::ONE,
        direction: Direction::Input,
        group: None,
        group_id: None,
        max_age_ns: None,
    };

    /// An input signal with no owner group.
    pub const fn input(path: &'a str, ty: ValueType, unit: UnitRef<'a>) -> Self {
        Self {
            path,
            ty,
            unit,
            direction: Direction::Input,
            group: None,
            group_id: None,
            max_age_ns: None,
        }
    }

    /// An output owned by the named group.
    pub const fn output(path: &'a str, ty: ValueType, unit: UnitRef<'a>, group: &'a str) -> Self {
        Self {
            path,
            ty,
            unit,
            direction: Direction::Output,
            group: Some(group),
            group_id: None,
            max_age_ns: None,
        }
    }
}

/// A cycle group.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GroupMeta<'a> {
    /// Group name (docs/16 P5).
    pub name: &'a str,
    /// Nominal period in ns.
    pub period_ns: u64,
}

impl GroupMeta<'static> {
    /// Placeholder used to initialise fixed-capacity storage.
    pub const EMPTY: Self = Self {
        name: "",
        period_ns: 0,
    };
}

/// A second path for an existing signal (docs/16 P1).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Alias<'a> {
    /// The alias path.
    pub path: &'a str,
    /// The canonical path it stands for.
    pub target: &'a str,
    /// Resolved target, set by the build.
    pub id: SignalId,
}

impl Alias<'static> {
    /// Placeholder used to initialise fixed-capacity storage.
    pub const EMPTY: Self = Self {
        path: "",
        target: "",
        id: SignalId(0),
    };
}

impl<'a> Alias<'a> {
    /// An alias of `target`.
    pub const fn new(path: &'a str, target: &'a str) -> Self {
        Self {
            path,
            target,
            id: SignalId(0),
        }
    }
}

/// Why a registry could not be built.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RegistryError<'a> {
    /// The fixed storage for this kind of entry is full.
    CapacityExceeded {
        /// Which table overflowed.
        table: Table,
        /// Its capacity.
        capacity: usize,
    },
    /// A path breaks the grammar of docs/02 §9 or the limits of ref/00 §10.
    InvalidPath(&'a str),
    /// A group name is not `snake_case` (docs/16 P5).
    InvalidGroupName(&'a str),
    /// The same path (canonical or alias) appears twice.
    DuplicatePath(&'a str),
    /// The same group name appears twice.
    DuplicateGroup(&'a str),
    /// An alias points to a path that is neither a signal nor an alias.
    UnknownAliasTarget(&'a str),
    /// A chain of aliases loops, or is longer than [`MAX_ALIAS_CHAIN`].
    AliasCycle(&'a str),
    /// A signal names a group that does not exist.
    UnknownGroup(&'a str),
    /// More entries than a `u32` id can number.
    TooManyEntries,
}

/// The tables of a registry, for capacity errors.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Table {
    /// Signals.
    Signals,
    /// Aliases.
    Aliases,
    /// Groups.
    Groups,
}

/// Maximum bytes in a path segment (ref/00 §10).
pub const MAX_SEGMENT_LEN: usize = 32;
/// Maximum segments in a path (ref/00 §10).
pub const MAX_SEGMENTS: usize = 8;
/// Maximum links in a chain of aliases (ADR 0041 §6).
pub const MAX_ALIAS_CHAIN: usize = 8;

/// Checks a path against docs/02 §9: `segment ("." segment)*`, each segment
/// `[a-z][a-z0-9_]*` of at most 32 bytes, at most 8 segments.
pub fn is_valid_path(path: &str) -> bool {
    let mut segments = 0;
    for segment in path.split('.') {
        segments += 1;
        if segments > MAX_SEGMENTS || !is_valid_name(segment) {
            return false;
        }
    }
    true
}

/// Checks a `snake_case` name (`[a-z][a-z0-9_]*`, at most 32 bytes).
pub fn is_valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    match bytes.first() {
        Some(b'a'..=b'z') => {}
        _ => return false,
    }
    bytes.len() <= MAX_SEGMENT_LEN
        && bytes
            .iter()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// Collects signals, aliases and groups into caller-provided storage.
#[derive(Debug)]
pub struct RegistryBuilder<'a> {
    signals: &'a mut [SignalMeta<'a>],
    aliases: &'a mut [Alias<'a>],
    groups: &'a mut [GroupMeta<'a>],
    n_signals: usize,
    n_aliases: usize,
    n_groups: usize,
}

impl<'a> RegistryBuilder<'a> {
    /// A builder whose capacities are the lengths of the given storage.
    pub fn new(
        signals: &'a mut [SignalMeta<'a>],
        aliases: &'a mut [Alias<'a>],
        groups: &'a mut [GroupMeta<'a>],
    ) -> Self {
        Self {
            signals,
            aliases,
            groups,
            n_signals: 0,
            n_aliases: 0,
            n_groups: 0,
        }
    }

    /// Adds a signal.
    pub fn signal(&mut self, meta: SignalMeta<'a>) -> Result<(), RegistryError<'a>> {
        push(
            &mut *self.signals,
            &mut self.n_signals,
            meta,
            Table::Signals,
        )
    }

    /// Adds an alias.
    pub fn alias(&mut self, alias: Alias<'a>) -> Result<(), RegistryError<'a>> {
        push(
            &mut *self.aliases,
            &mut self.n_aliases,
            alias,
            Table::Aliases,
        )
    }

    /// Adds a group.
    pub fn group(&mut self, group: GroupMeta<'a>) -> Result<(), RegistryError<'a>> {
        push(&mut *self.groups, &mut self.n_groups, group, Table::Groups)
    }

    /// Sorts, validates and freezes the registry.
    pub fn build(self) -> Result<Registry<'a>, RegistryError<'a>> {
        let signals = &mut self.signals[..self.n_signals];
        let aliases = &mut self.aliases[..self.n_aliases];
        let groups = &mut self.groups[..self.n_groups];
        freeze(signals, aliases, groups)?;
        Ok(Registry {
            signals,
            aliases,
            groups,
        })
    }
}

fn push<'a, T>(
    storage: &mut [T],
    len: &mut usize,
    item: T,
    table: Table,
) -> Result<(), RegistryError<'a>> {
    match storage.get_mut(*len) {
        Some(slot) => {
            *slot = item;
            *len += 1;
            Ok(())
        }
        None => Err(RegistryError::CapacityExceeded {
            table,
            capacity: storage.len(),
        }),
    }
}

fn freeze<'a>(
    signals: &mut [SignalMeta<'a>],
    aliases: &mut [Alias<'a>],
    groups: &mut [GroupMeta<'a>],
) -> Result<(), RegistryError<'a>> {
    if signals.len() > u32::MAX as usize || groups.len() > u32::MAX as usize {
        return Err(RegistryError::TooManyEntries);
    }
    groups.sort_unstable_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
    for (i, g) in groups.iter().enumerate() {
        if !is_valid_name(g.name) {
            return Err(RegistryError::InvalidGroupName(g.name));
        }
        if i > 0 && groups[i - 1].name == g.name {
            return Err(RegistryError::DuplicateGroup(g.name));
        }
    }
    signals.sort_unstable_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    for i in 0..signals.len() {
        let path = signals[i].path;
        if !is_valid_path(path) {
            return Err(RegistryError::InvalidPath(path));
        }
        if i > 0 && signals[i - 1].path == path {
            return Err(RegistryError::DuplicatePath(path));
        }
        signals[i].group_id = match signals[i].group {
            None => None,
            Some(name) => match groups.binary_search_by(|g| g.name.as_bytes().cmp(name.as_bytes()))
            {
                Ok(index) => Some(GroupId(index as u32)),
                Err(_) => return Err(RegistryError::UnknownGroup(name)),
            },
        };
    }
    aliases.sort_unstable_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    for i in 0..aliases.len() {
        let alias = aliases[i];
        if !is_valid_path(alias.path) {
            return Err(RegistryError::InvalidPath(alias.path));
        }
        if (i > 0 && aliases[i - 1].path == alias.path) || find(signals, alias.path).is_some() {
            return Err(RegistryError::DuplicatePath(alias.path));
        }
    }
    for i in 0..aliases.len() {
        let mut target = aliases[i].target;
        let mut resolved = None;
        for _ in 0..MAX_ALIAS_CHAIN {
            if let Some(id) = find(signals, target) {
                resolved = Some(id);
                break;
            }
            match aliases.binary_search_by(|a| a.path.as_bytes().cmp(target.as_bytes())) {
                Ok(j) => target = aliases[j].target,
                Err(_) => return Err(RegistryError::UnknownAliasTarget(target)),
            }
        }
        aliases[i].id = resolved.ok_or(RegistryError::AliasCycle(aliases[i].path))?;
    }
    Ok(())
}

fn find(signals: &[SignalMeta<'_>], path: &str) -> Option<SignalId> {
    signals
        .binary_search_by(|s| s.path.as_bytes().cmp(path.as_bytes()))
        .ok()
        .map(|index| SignalId(index as u32))
}

/// An immutable, sorted registry. Lookups by path are for configuration time
/// only; the tick path uses [`SignalId`]s (docs/01 §3.1).
#[derive(Copy, Clone, Debug)]
pub struct Registry<'a> {
    signals: &'a [SignalMeta<'a>],
    aliases: &'a [Alias<'a>],
    groups: &'a [GroupMeta<'a>],
}

impl<'a> Registry<'a> {
    /// Number of signals; ids are `0..len`.
    pub fn len(&self) -> usize {
        self.signals.len()
    }

    /// True when there are no signals.
    pub fn is_empty(&self) -> bool {
        self.signals.is_empty()
    }

    /// Every signal, in id order.
    pub fn signals(&self) -> &'a [SignalMeta<'a>] {
        self.signals
    }

    /// Every group, in id order.
    pub fn groups(&self) -> &'a [GroupMeta<'a>] {
        self.groups
    }

    /// Every alias, in path order.
    pub fn aliases(&self) -> &'a [Alias<'a>] {
        self.aliases
    }

    /// Metadata of a signal.
    pub fn signal(&self, id: SignalId) -> Option<&'a SignalMeta<'a>> {
        self.signals.get(id.index())
    }

    /// Metadata of a group.
    pub fn group(&self, id: GroupId) -> Option<&'a GroupMeta<'a>> {
        self.groups.get(id.index())
    }

    /// Resolves a canonical path or an alias to its id (docs/16 P1).
    pub fn resolve(&self, path: &str) -> Option<SignalId> {
        find(self.signals, path).or_else(|| {
            self.aliases
                .binary_search_by(|a| a.path.as_bytes().cmp(path.as_bytes()))
                .ok()
                .map(|index| self.aliases[index].id)
        })
    }

    /// Resolves a group name.
    pub fn resolve_group(&self, name: &str) -> Option<GroupId> {
        self.groups
            .binary_search_by(|g| g.name.as_bytes().cmp(name.as_bytes()))
            .ok()
            .map(|index| GroupId(index as u32))
    }

    /// The registry hash of the trace header (docs/16 §7), encoded as ADR 0031
    /// fixes: SHA-256 over `"JKREG" u8:1 u32:n_signals`, then for each signal
    /// in id order `u16:path_len path u8:type u32:owner_group`, with
    /// `0xFFFF_FFFF` for no owner group; integers little-endian.
    pub fn hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"JKREG");
        h.update(&[1]);
        h.update(&(self.signals.len() as u32).to_le_bytes());
        for s in self.signals {
            h.update(&(s.path.len() as u16).to_le_bytes());
            h.update(s.path.as_bytes());
            h.update(&[s.ty as u8]);
            let group = s.group_id.map_or(u32::MAX, |g| g.0);
            h.update(&group.to_le_bytes());
        }
        h.finalize()
    }
}

/// A registry built with vectors, for the Linux builder (ADR 0020).
#[cfg(feature = "alloc")]
#[derive(Clone, Debug, Default)]
pub struct OwnedRegistry<'a> {
    signals: alloc::vec::Vec<SignalMeta<'a>>,
    aliases: alloc::vec::Vec<Alias<'a>>,
    groups: alloc::vec::Vec<GroupMeta<'a>>,
}

#[cfg(feature = "alloc")]
impl<'a> OwnedRegistry<'a> {
    /// Sorts, validates and freezes the given entries.
    pub fn build(
        mut signals: alloc::vec::Vec<SignalMeta<'a>>,
        mut aliases: alloc::vec::Vec<Alias<'a>>,
        mut groups: alloc::vec::Vec<GroupMeta<'a>>,
    ) -> Result<Self, RegistryError<'a>> {
        freeze(&mut signals, &mut aliases, &mut groups)?;
        Ok(Self {
            signals,
            aliases,
            groups,
        })
    }

    /// The borrowed view used everywhere else.
    pub fn as_registry(&self) -> Registry<'_> {
        Registry {
            signals: &self.signals,
            aliases: &self.aliases,
            groups: &self.groups,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KW: UnitRef<'static> = UnitRef("kW");

    fn build_into<'a>(
        signals: &'a mut [SignalMeta<'a>],
        aliases: &'a mut [Alias<'a>],
        groups: &'a mut [GroupMeta<'a>],
        order: &[usize],
    ) -> Result<Registry<'a>, RegistryError<'a>> {
        let all = [
            SignalMeta::output("pcs.p_set", ValueType::F64, KW, "fast"),
            SignalMeta::input("meter.p_total", ValueType::F64, KW),
            SignalMeta::input("meter.f", ValueType::F64, UnitRef("Hz")),
            SignalMeta::input("di.door_open", ValueType::Bool, UnitRef::ONE),
        ];
        let mut b = RegistryBuilder::new(signals, aliases, groups);
        b.group(GroupMeta {
            name: "slow",
            period_ns: 1_000_000_000,
        })?;
        b.group(GroupMeta {
            name: "fast",
            period_ns: 100_000_000,
        })?;
        for &i in order {
            b.signal(all[i])?;
        }
        b.alias(Alias::new("battery.p_set", "pcs.p_set"))?;
        b.build()
    }

    #[test]
    fn ids_follow_path_order_whatever_the_input_order() {
        let mut s1 = [SignalMeta::EMPTY; 8];
        let mut a1 = [Alias::EMPTY; 2];
        let mut g1 = [GroupMeta::EMPTY; 2];
        let r1 = build_into(&mut s1, &mut a1, &mut g1, &[0, 1, 2, 3]).unwrap();
        let mut s2 = [SignalMeta::EMPTY; 8];
        let mut a2 = [Alias::EMPTY; 2];
        let mut g2 = [GroupMeta::EMPTY; 2];
        let r2 = build_into(&mut s2, &mut a2, &mut g2, &[3, 1, 0, 2]).unwrap();
        let paths: [&str; 4] = core::array::from_fn(|i| r1.signals()[i].path);
        assert_eq!(
            paths,
            ["di.door_open", "meter.f", "meter.p_total", "pcs.p_set"]
        );
        assert_eq!(r1.hash(), r2.hash());
        assert_eq!(r1.resolve("pcs.p_set"), Some(SignalId(3)));
        assert_eq!(r1.resolve_group("fast"), Some(GroupId(0)));
        assert_eq!(r1.signal(SignalId(3)).unwrap().group_id, Some(GroupId(0)));
    }

    #[test]
    fn alias_resolves_to_same_id() {
        let mut s = [SignalMeta::EMPTY; 8];
        let mut a = [Alias::EMPTY; 2];
        let mut g = [GroupMeta::EMPTY; 2];
        let r = build_into(&mut s, &mut a, &mut g, &[0, 1, 2, 3]).unwrap();
        assert_eq!(r.resolve("battery.p_set"), r.resolve("pcs.p_set"));
        assert_eq!(r.resolve("battery.q_set"), None);
    }

    #[test]
    fn capacity_overflow_is_an_error() {
        let mut s = [SignalMeta::EMPTY; 3];
        let mut a = [Alias::EMPTY; 2];
        let mut g = [GroupMeta::EMPTY; 2];
        assert_eq!(
            build_into(&mut s, &mut a, &mut g, &[0, 1, 2, 3]).unwrap_err(),
            RegistryError::CapacityExceeded {
                table: Table::Signals,
                capacity: 3
            }
        );
        let mut s = [SignalMeta::EMPTY; 8];
        let mut a: [Alias<'_>; 0] = [];
        let mut g = [GroupMeta::EMPTY; 2];
        assert_eq!(
            build_into(&mut s, &mut a, &mut g, &[0]).unwrap_err(),
            RegistryError::CapacityExceeded {
                table: Table::Aliases,
                capacity: 0
            }
        );
    }

    #[test]
    fn rejects_bad_entries() {
        let mut s = [SignalMeta::EMPTY; 4];
        let mut a = [Alias::EMPTY; 2];
        let mut g = [GroupMeta::EMPTY; 1];
        let mut b = RegistryBuilder::new(&mut s, &mut a, &mut g);
        b.signal(SignalMeta::input("x.a", ValueType::F64, KW))
            .unwrap();
        b.signal(SignalMeta::input("x.a", ValueType::F64, KW))
            .unwrap();
        assert_eq!(b.build().unwrap_err(), RegistryError::DuplicatePath("x.a"));

        let mut s = [SignalMeta::EMPTY; 4];
        let mut a = [Alias::EMPTY; 2];
        let mut g = [GroupMeta::EMPTY; 1];
        let mut b = RegistryBuilder::new(&mut s, &mut a, &mut g);
        b.signal(SignalMeta::input("x.a", ValueType::F64, KW))
            .unwrap();
        b.alias(Alias::new("x.a", "x.a")).unwrap();
        assert_eq!(b.build().unwrap_err(), RegistryError::DuplicatePath("x.a"));

        let mut s = [SignalMeta::EMPTY; 4];
        let mut a = [Alias::EMPTY; 2];
        let mut g = [GroupMeta::EMPTY; 1];
        let mut b = RegistryBuilder::new(&mut s, &mut a, &mut g);
        b.alias(Alias::new("y.a", "x.a")).unwrap();
        assert_eq!(
            b.build().unwrap_err(),
            RegistryError::UnknownAliasTarget("x.a")
        );

        let mut s = [SignalMeta::EMPTY; 4];
        let mut a = [Alias::EMPTY; 3];
        let mut g = [GroupMeta::EMPTY; 1];
        let mut b = RegistryBuilder::new(&mut s, &mut a, &mut g);
        b.signal(SignalMeta::input("esp_io.boot", ValueType::Bool, KW))
            .unwrap();
        b.alias(Alias::new("botao.clicked", "esp_io.boot")).unwrap();
        b.alias(Alias::new("painel.botao.clicked", "botao.clicked"))
            .unwrap();
        let r = b.build().unwrap();
        assert_eq!(r.resolve("painel.botao.clicked"), Some(SignalId(0)));

        let mut s = [SignalMeta::EMPTY; 4];
        let mut a = [Alias::EMPTY; 3];
        let mut g = [GroupMeta::EMPTY; 1];
        let mut b = RegistryBuilder::new(&mut s, &mut a, &mut g);
        b.alias(Alias::new("p.a", "p.b")).unwrap();
        b.alias(Alias::new("p.b", "p.a")).unwrap();
        assert!(matches!(
            b.build().unwrap_err(),
            RegistryError::AliasCycle(_)
        ));

        let mut s = [SignalMeta::EMPTY; 4];
        let mut a = [Alias::EMPTY; 2];
        let mut g = [GroupMeta::EMPTY; 1];
        let mut b = RegistryBuilder::new(&mut s, &mut a, &mut g);
        b.signal(SignalMeta::output("x.o", ValueType::F64, KW, "nope"))
            .unwrap();
        assert_eq!(b.build().unwrap_err(), RegistryError::UnknownGroup("nope"));
    }

    #[test]
    fn path_grammar() {
        assert!(is_valid_path("pump1.run_cmd"));
        assert!(is_valid_path("a.b.c.d.e.f.g.h"));
        assert!(!is_valid_path("a.b.c.d.e.f.g.h.i"));
        assert!(!is_valid_path("Pump.x"));
        assert!(!is_valid_path("1a.x"));
        assert!(!is_valid_path("a..b"));
        assert!(!is_valid_path(""));
        assert!(!is_valid_path("a.b-c"));
        assert!(is_valid_path("abcdefghijklmnopqrstuvwxyz012345"));
        assert!(!is_valid_path("abcdefghijklmnopqrstuvwxyz0123456"));
    }

    #[test]
    fn hash_is_pinned() {
        let mut s = [SignalMeta::EMPTY; 8];
        let mut a = [Alias::EMPTY; 2];
        let mut g = [GroupMeta::EMPTY; 2];
        let r = build_into(&mut s, &mut a, &mut g, &[0, 1, 2, 3]).unwrap();
        let mut expected = Sha256::new();
        expected.update(b"JKREG\x01\x04\x00\x00\x00");
        expected.update(b"\x0c\x00di.door_open\x00\xff\xff\xff\xff");
        expected.update(b"\x07\x00meter.f\x03\xff\xff\xff\xff");
        expected.update(b"\x0d\x00meter.p_total\x03\xff\xff\xff\xff");
        expected.update(b"\x09\x00pcs.p_set\x03\x00\x00\x00\x00");
        assert_eq!(r.hash(), expected.finalize());
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn owned_registry_matches_borrowed() {
        let mut s = [SignalMeta::EMPTY; 8];
        let mut a = [Alias::EMPTY; 2];
        let mut g = [GroupMeta::EMPTY; 2];
        let borrowed = build_into(&mut s, &mut a, &mut g, &[0, 1, 2, 3]).unwrap();
        let owned = OwnedRegistry::build(
            alloc::vec![
                SignalMeta::input("meter.f", ValueType::F64, UnitRef("Hz")),
                SignalMeta::output("pcs.p_set", ValueType::F64, KW, "fast"),
                SignalMeta::input("di.door_open", ValueType::Bool, UnitRef::ONE),
                SignalMeta::input("meter.p_total", ValueType::F64, KW),
            ],
            alloc::vec![Alias::new("battery.p_set", "pcs.p_set")],
            alloc::vec![
                GroupMeta {
                    name: "slow",
                    period_ns: 1_000_000_000,
                },
                GroupMeta {
                    name: "fast",
                    period_ns: 100_000_000,
                },
            ],
        )
        .unwrap();
        assert_eq!(owned.as_registry().hash(), borrowed.hash());
        assert_eq!(
            owned.as_registry().resolve("battery.p_set"),
            Some(SignalId(3))
        );
    }
}
