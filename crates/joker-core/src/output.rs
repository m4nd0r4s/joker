//! Output and writer configuration shared by the arbiter and the envelope
//! (docs/01 §6–§7, docs/16 §1–§2, ref/01 §5.10–§5.12).

use core::fmt;

use joker_expr::Program;

use crate::image::WriterId;
use crate::registry::{GroupId, SignalId};
use crate::value::ValueType;

/// One second in ns.
pub const SECOND_NS: u64 = 1_000_000_000;
/// Default `max_lease` of an output: 15 min (docs/16 A9).
pub const DEFAULT_MAX_LEASE_NS: u64 = 15 * 60 * SECOND_NS;
/// Default `max_manual` of an output: 8 h (docs/16 A10).
pub const DEFAULT_MAX_MANUAL_NS: u64 = 8 * 3600 * SECOND_NS;
/// Default writer lease, in periods of the owner group (docs/16 A9).
pub const DEFAULT_LEASE_PERIODS: u64 = 3;

/// The named bands of the 16 arbiter levels (docs/01 §6, ref/01 §3.11).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Band {
    /// Levels 1–2.
    Safety,
    /// Levels 3–5, only for people (CLI, API, HMI).
    Manual,
    /// Levels 6–8.
    Supervisor,
    /// Levels 9–12.
    Control,
    /// Levels 13–15.
    Optimizer,
    /// Level 16.
    Schedule,
}

impl Band {
    /// Every band, from the highest priority.
    pub const ALL: [Self; 6] = [
        Self::Safety,
        Self::Manual,
        Self::Supervisor,
        Self::Control,
        Self::Optimizer,
        Self::Schedule,
    ];

    /// The canonical name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Safety => "safety",
            Self::Manual => "manual",
            Self::Supervisor => "supervisor",
            Self::Control => "control",
            Self::Optimizer => "optimizer",
            Self::Schedule => "schedule",
        }
    }

    /// The inclusive range of levels of this band.
    pub const fn levels(self) -> (u8, u8) {
        match self {
            Self::Safety => (1, 2),
            Self::Manual => (3, 5),
            Self::Supervisor => (6, 8),
            Self::Control => (9, 12),
            Self::Optimizer => (13, 15),
            Self::Schedule => (16, 16),
        }
    }

    /// Parses a band name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.name() == name)
    }
}

/// An arbiter level, 1 (highest priority) to 16 (ref/00 §5).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Level(u8);

impl Level {
    /// Number of levels (ref/00 §10).
    pub const COUNT: usize = 16;
    /// `manual:4`, the default level of a manual write (ref/09 §5.6).
    pub const MANUAL_DEFAULT: Self = Self(4);

    /// A level from its number, if within 1–16.
    pub const fn new(level: u8) -> Option<Self> {
        if level >= 1 && level <= 16 {
            Some(Self(level))
        } else {
            None
        }
    }

    /// The level number, 1–16.
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Index 0–15 into per-level tables.
    pub const fn index(self) -> usize {
        (self.0 - 1) as usize
    }

    /// The band the level belongs to.
    pub const fn band(self) -> Band {
        match self.0 {
            1..=2 => Band::Safety,
            3..=5 => Band::Manual,
            6..=8 => Band::Supervisor,
            9..=12 => Band::Control,
            13..=15 => Band::Optimizer,
            _ => Band::Schedule,
        }
    }

    /// Parses `"<band>:<level>"` (ref/00 §5) or a bare band, which means the
    /// band's default: its lowest priority (ref/01 §3.11). A bare `manual`
    /// has no default.
    pub fn parse(text: &str) -> Option<Self> {
        match text.split_once(':') {
            Some((band, number)) => {
                let band = Band::parse(band)?;
                let level = number.parse::<u8>().ok()?;
                let (low, high) = band.levels();
                (low..=high).contains(&level).then_some(Self(level))
            }
            None => match Band::parse(text)? {
                Band::Manual => None,
                band => Some(Self(band.levels().1)),
            },
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.band().name(), self.0)
    }
}

/// Writer kinds with the numeric codes of ref/00 §6. The cycle and async
/// instances of one app are one writer (docs/16 A1); the kind only says
/// where its proposals are staged.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WriterKind {
    /// An app writing from its cycle instance.
    Cycle = 0,
    /// An app writing from its async instance.
    Async = 1,
    /// A person through the CLI, the API or an HMI.
    Manual = 2,
    /// A bridge.
    Bridge = 3,
    /// An implementation of a virtual instance.
    Impl = 4,
}

impl WriterKind {
    /// Parses the canonical name; `proposal` is reserved for 1.1 and refused.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "cycle" => Self::Cycle,
            "async" => Self::Async,
            "manual" => Self::Manual,
            "bridge" => Self::Bridge,
            "impl" => Self::Impl,
            _ => return None,
        })
    }
}

/// The clock quality a writer requires (`requires_clock`, ref/01 §5.12).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClockRequirement {
    /// Any clock.
    Any,
    /// `holdover` or better.
    Holdover,
    /// `synced` only.
    Synced,
}

/// A declared writer (docs/16 A1).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WriterConfig {
    /// Kind of writer.
    pub kind: WriterKind,
    /// The level it occupies on every output it writes.
    pub level: Level,
    /// Lease in ns, or `None` for 3 periods of each output's owner group.
    /// For a manual writer this is its TTL, which is mandatory (docs/16 A10).
    pub lease_ns: Option<u64>,
    /// Clock quality the writer requires (docs/16 E1).
    pub requires_clock: ClockRequirement,
}

impl WriterConfig {
    /// Placeholder used to initialise fixed-capacity storage.
    pub const EMPTY: Self = Self {
        kind: WriterKind::Cycle,
        level: Level(12),
        lease_ns: None,
        requires_clock: ClockRequirement::Any,
    };
}

/// The safe state of an output (ADR 0006).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SafeValue {
    /// A value, as bits in the output's type and unit.
    Value(u64),
    /// Hand control back to the device's own logic (docs/03 §6.1).
    Relinquish,
}

/// What the range step does with an out-of-range value (ref/01 §5.10).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OnRange {
    /// Clamp to the nearest limit: `clamped_range`.
    Clamp,
    /// Refuse the value: `rejected(range)`.
    Reject,
    /// Trip the output: `tripped`.
    Trip,
}

/// What a failed permissive or quality step applies (docs/16 E3).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OnInterlock {
    /// Apply the safe state.
    Safe,
    /// Keep the last applied value.
    Hold,
}

/// Which writers may write an output.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Writers<'a> {
    /// Every declared writer (conformance fixtures declare writers once).
    Any,
    /// Only these writers. Manual writers may always write (ref/09 §5.6).
    Only(&'a [WriterId]),
}

impl Writers<'_> {
    /// True when `writer`, of kind `kind`, may write.
    pub fn allows(&self, writer: WriterId, kind: WriterKind) -> bool {
        match self {
            Self::Any => true,
            Self::Only(list) => kind == WriterKind::Manual || list.contains(&writer),
        }
    }
}

/// One limit of an expression range (docs/16 E6).
#[derive(Copy, Clone, Debug)]
pub enum Bound<'a> {
    /// A constant, in the output's unit.
    Const(f64),
    /// An expression evaluated in every tick, in the output's unit.
    Expr(&'a Program),
}

/// An actuation budget: at most `count` actuations in any `window_ns`
/// (`max_actuations`, ref/01 §5.10, ADR 0038).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Allowed actuations.
    pub count: u32,
    /// Sliding window in ns.
    pub window_ns: u64,
}

/// How an atomic group applies the rate step (ref/01 §5.16).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OnRate {
    /// Each member keeps its own rate step.
    Independent,
    /// Every member is scaled to the smallest fraction (ADR 0039 §4).
    Proportional,
}

/// An atomic group of outputs (M20, docs/02 §7.3, ref/01 §5.16).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AtomicGroup<'a> {
    /// The members, at least two.
    pub outputs: &'a [SignalId],
    /// Bus order for the driver; a permutation of `outputs`.
    pub order: &'a [SignalId],
    /// Rate behaviour.
    pub on_rate: OnRate,
    /// Commit register and its value, written after the group's writes.
    pub commit: Option<(SignalId, u64)>,
}

/// The envelope and arbiter settings of one output (ref/01 §5.10).
#[derive(Copy, Clone, Debug)]
pub struct OutputConfig<'a> {
    /// The output signal.
    pub signal: SignalId,
    /// Its type.
    pub ty: ValueType,
    /// Valid variant values when `ty` is `Enum` (docs/16 E10).
    pub enum_values: &'a [u64],
    /// Owner group (docs/16 A11).
    pub group: GroupId,
    /// Safe state (ADR 0006).
    pub safe: SafeValue,
    /// Rate limit when going to `safe`, in units per second; `None` jumps.
    pub safe_ramp: Option<f64>,
    /// Constant range limits, in the output's unit.
    pub range: Option<(f64, f64)>,
    /// Range step action.
    pub on_range: OnRange,
    /// Rate limit in units per second.
    pub rate: Option<f64>,
    /// Permissive and quality failure action (docs/16 E3, E4).
    pub on_interlock: OnInterlock,
    /// Upper bound of any writer's lease (docs/16 A9).
    pub max_lease_ns: u64,
    /// Upper bound of a manual TTL (docs/16 A10).
    pub max_manual_ns: u64,
    /// Who may write.
    pub writers: Writers<'a>,
    /// Levels delegated to the Joker Link on an MCU, as a mask with bit
    /// `n-1` for level `n` (`remote_levels`, docs/16 A12); 0 on Linux.
    pub remote_levels: u16,
    /// Permissive (step 2), a boolean expression.
    pub interlock: Option<&'a Program>,
    /// `Uncertain` passes the permissive and expression range limits.
    pub permit_uncertain: bool,
    /// Signals that must be `Good` (step 3, docs/16 E4).
    pub requires_good: &'a [SignalId],
    /// How long `requires_good` may fail before the step fails, in ns.
    pub grace_ns: u64,
    /// Expression range limits (docs/16 E6); replaces `range` when set.
    pub range_exprs: Option<(Bound<'a>, Bound<'a>)>,
    /// Condition that ends a trip (docs/16 E7).
    pub reset_when: Option<&'a Program>,
    /// Changes strictly smaller than this are suppressed (step 6).
    pub deadband: Option<f64>,
    /// Minimum time between actuations, in ns (step 6).
    pub min_interval_ns: u64,
    /// Actuation budget (step 6).
    pub max_actuations: Option<Budget>,
}

impl<'a> OutputConfig<'a> {
    /// An output with only its safe state and the defaults of docs/16 A9–A10.
    pub const fn new(signal: SignalId, ty: ValueType, group: GroupId, safe: SafeValue) -> Self {
        Self {
            signal,
            ty,
            enum_values: &[],
            group,
            safe,
            safe_ramp: None,
            range: None,
            on_range: OnRange::Clamp,
            rate: None,
            on_interlock: OnInterlock::Safe,
            max_lease_ns: DEFAULT_MAX_LEASE_NS,
            max_manual_ns: DEFAULT_MAX_MANUAL_NS,
            writers: Writers::Any,
            remote_levels: 0,
            interlock: None,
            permit_uncertain: false,
            requires_good: &[],
            grace_ns: 0,
            range_exprs: None,
            reset_when: None,
            deadband: None,
            min_interval_ns: 0,
            max_actuations: None,
        }
    }

    /// The expression programs of this output, in evaluation order
    /// (ADR 0038 §1): interlock, low limit, high limit, `reset_when`.
    pub fn programs(&self) -> [Option<&'a Program>; 4] {
        let bound = |b: Bound<'a>| match b {
            Bound::Expr(p) => Some(p),
            Bound::Const(_) => None,
        };
        let (low, high) = match self.range_exprs {
            Some((low, high)) => (bound(low), bound(high)),
            None => (None, None),
        };
        [self.interlock, low, high, self.reset_when]
    }

    /// Words of state arena this output needs: the state of its programs
    /// and its actuation budget.
    pub fn state_words(&self) -> usize {
        let programs: usize = self
            .programs()
            .iter()
            .flatten()
            .map(|p| p.state_words())
            .sum();
        programs + self.max_actuations.map_or(0, |b| 1 + b.count as usize)
    }

    /// True when `level` is delegated to the link (docs/16 A12).
    pub const fn delegates(&self, level: Level) -> bool {
        self.remote_levels & (1 << level.index()) != 0
    }

    /// True when `bits` is a value this output can take (docs/16 E10): not
    /// NaN or infinite, `bool` 0 or 1, an existing enum variant.
    pub fn accepts(&self, bits: u64) -> bool {
        match self.ty {
            ValueType::F64 | ValueType::F32 => f64::from_bits(bits).is_finite(),
            ValueType::Bool => bits <= 1,
            ValueType::Enum => self.enum_values.contains(&bits),
            ValueType::I64 | ValueType::U64 | ValueType::Bits => true,
            ValueType::String | ValueType::Bytes | ValueType::Record => false,
        }
    }
}

/// A configuration error, with its code from the registry (docs/16 §9).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ConfigError {
    /// The code, such as `"J0213"`.
    pub code: &'static str,
    /// The output concerned.
    pub output: Option<SignalId>,
    /// The writer concerned.
    pub writer: Option<WriterId>,
}

/// The owner group of an output (docs/16 A11): the declared `group`; else
/// the group of the highest-priority app that writes it from a cycle task;
/// else the implicit `default` group. `cycle_writers` lists (level, group)
/// of those apps.
pub fn owner_group(
    declared: Option<GroupId>,
    cycle_writers: &[(Level, GroupId)],
    default: GroupId,
) -> GroupId {
    declared
        .or_else(|| {
            cycle_writers
                .iter()
                .min_by_key(|(level, group)| (*level, *group))
                .map(|&(_, group)| group)
        })
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::ToString;

    #[test]
    fn levels_parse_and_print() {
        assert_eq!(Level::parse("optimizer:13"), Level::new(13));
        assert_eq!(Level::parse("manual:4"), Some(Level::MANUAL_DEFAULT));
        assert_eq!(Level::parse("control"), Level::new(12));
        assert_eq!(Level::parse("safety"), Level::new(2));
        assert_eq!(Level::parse("manual"), None);
        assert_eq!(Level::parse("control:13"), None);
        assert_eq!(Level::parse("nope:1"), None);
        assert_eq!(Level::new(13).unwrap().to_string(), "optimizer:13");
        assert_eq!(Level::new(0), None);
        assert_eq!(Level::new(17), None);
        for n in 1..=16 {
            let level = Level::new(n).unwrap();
            assert_eq!(Level::parse(&level.to_string()), Some(level));
        }
    }

    #[test]
    fn accepts_follows_e10() {
        let mut out =
            OutputConfig::new(SignalId(0), ValueType::F64, GroupId(0), SafeValue::Value(0));
        assert!(out.accepts(1.5f64.to_bits()));
        assert!(!out.accepts(f64::NAN.to_bits()));
        assert!(!out.accepts(f64::INFINITY.to_bits()));
        out.ty = ValueType::Bool;
        assert!(out.accepts(1));
        assert!(!out.accepts(2));
        out.ty = ValueType::Enum;
        out.enum_values = &[0, 2];
        assert!(out.accepts(2));
        assert!(!out.accepts(1));
    }

    #[test]
    fn owner_group_follows_a11() {
        let l = |n| Level::new(n).unwrap();
        assert_eq!(
            owner_group(Some(GroupId(3)), &[(l(9), GroupId(1))], GroupId(0)),
            GroupId(3)
        );
        assert_eq!(
            owner_group(None, &[(l(13), GroupId(1)), (l(9), GroupId(2))], GroupId(0)),
            GroupId(2)
        );
        assert_eq!(owner_group(None, &[], GroupId(0)), GroupId(0));
    }
}
