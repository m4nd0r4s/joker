//! A contract as the manifest checks see a class (`joker_manifest::context::
//! ClassInfo`): what `jk check` and the node use to validate `[instances]`
//! and `[implements]` (ref/01 §5.13, ref/03 §2.2.14).

use std::collections::BTreeMap;

use joker_manifest::context::{ClassInfo, ClassMember, Member, MemberKind, SignalType};
use joker_units::Unit;

use crate::ast::Direction;
use crate::contract::{ContractFile, Ty};

fn signal_type(file: &ContractFile, ty: &Ty) -> SignalType {
    match ty {
        Ty::Bool => SignalType::Bool,
        Ty::I64 | Ty::U64 | Ty::Bits(_) => SignalType::Integer,
        Ty::F64 | Ty::F32 => SignalType::Float,
        Ty::String => SignalType::Text,
        Ty::Bytes => SignalType::Bytes,
        Ty::Enum(name) => {
            let mut variants: Vec<(i64, String)> = file
                .contract
                .enums
                .iter()
                .find(|e| &e.name == name)
                .map(|e| e.variants.iter().map(|(n, v)| (*v, n.clone())).collect())
                .unwrap_or_default();
            variants.sort();
            SignalType::Enum {
                name: name.rsplit('/').next().unwrap_or(name).to_owned(),
                variants: variants.into_iter().map(|(_, n)| n).collect(),
            }
        }
        Ty::Record(_) | Ty::List(..) | Ty::Unit => SignalType::Unknown,
    }
}

fn unit(text: Option<&String>) -> Option<Unit> {
    text.and_then(|u| Unit::parse(u).ok())
}

/// The class as the manifest checks see it. A method with the name of a
/// member (`led@1`'s `on`) is left out: the member is what a `map` binds.
pub fn class_info(file: &ContractFile) -> ClassInfo {
    let c = &file.contract;
    let mut members: BTreeMap<String, Member> = BTreeMap::new();
    for m in &c.members {
        let kind = match m.direction {
            Direction::In => MemberKind::In,
            Direction::Out => MemberKind::Out,
            Direction::Param => MemberKind::Param,
        };
        let mut member = Member::new(kind, signal_type(file, &m.ty));
        member.unit = unit(m.unit.as_ref());
        member.optional = m.optional;
        member.capability = m.capability.clone();
        members.insert(m.name.clone(), member);
    }
    for n in &c.class_members {
        let mut member = Member::new(MemberKind::Class, SignalType::Unknown);
        member.optional = n.optional;
        member.capability = n.capability.clone();
        member.class = Some(ClassMember {
            class: n.class.clone(),
            min: n.min,
            max: n.max,
            fixed: !n.collection,
            requires: n.requires.clone(),
        });
        members.insert(n.name.clone(), member);
    }
    for q in &c.queries {
        let mut member = Member::new(MemberKind::Query, signal_type(file, &q.ty));
        member.unit = unit(q.unit.as_ref());
        member.capability = q.capability.clone();
        members.insert(q.name.clone(), member);
    }
    for e in &c.events {
        let mut member = Member::new(MemberKind::Event, SignalType::Unknown);
        member.capability = e.capability.clone();
        members.insert(e.name.clone(), member);
    }
    for a in &c.actions {
        let mut member = Member::new(MemberKind::Action, SignalType::Unknown);
        member.capability = a.capability.clone();
        members.insert(a.name.clone(), member);
    }
    for m in &c.methods {
        if members.contains_key(&m.name) {
            continue;
        }
        let mut member = Member::new(MemberKind::Method, SignalType::Unknown);
        member.capability = m.capability.clone();
        members.insert(m.name.clone(), member);
    }
    ClassInfo {
        members,
        capabilities: c.capabilities.iter().cloned().collect(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn led_keeps_its_out_member_under_the_method_name() {
        let contracts = crate::stdlib::contracts();
        let led = class_info(&contracts["std/led@1"]);
        assert_eq!(led.members["on"].kind, MemberKind::Out);
        assert_eq!(led.members["toggle"].kind, MemberKind::Method);
        assert_eq!(
            led.members["brightness"].capability.as_deref(),
            Some("dimmable")
        );
        assert!(led.capabilities.contains("dimmable"));
        let analog = class_info(&contracts["std/analog_in@1"]);
        assert_eq!(analog.members["value"].unit.unwrap().symbol(), "1");
        assert!(analog.members["reading"].unit.is_none());
        assert_eq!(analog.members["smooth"].kind, MemberKind::Query);
    }
}
