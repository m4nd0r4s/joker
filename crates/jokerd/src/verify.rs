//! Readback verification of outputs (`verify`, docs/01 §7, ADR 0072).
//!
//! After each commit of its owner group, an output with `verify` compares
//! its applied value with the latest sample of its readback input. While
//! they disagree (the readback is not `good`, or it is further from the
//! applied value than `tolerance`) for `within` or longer, the output's
//! slot is `uncertain(not_confirmed)`; the first readback that agrees
//! makes it `good` again, also after a `device-rejected` ack (ref/06
//! §2.9). A new applied value starts a new window. The
//! state is allocated before the first tick, so a tick does not allocate.

use joker_core::envelope::Applied;
use joker_core::slot::Slot;
use joker_core::value::NO_TS_SRC;
use joker_core::{Outputs, Quality, QualityCode, QualityReason, SignalId, ValueType};
use joker_units::Conversion;

use crate::plan::Plan;
use crate::shared::Shared;

#[derive(Debug, Clone, Copy)]
struct Entry {
    group: usize,
    output: SignalId,
    output_ty: ValueType,
    readback: SignalId,
    readback_ty: ValueType,
    conversion: Conversion,
    tolerance: Option<f64>,
    within_ns: u64,
    applied: Option<Applied>,
    disagree_since: Option<u64>,
    unconfirmed: bool,
}

/// The verified outputs of a generation.
#[derive(Debug, Clone, Default)]
pub struct Verifier {
    entries: Vec<Entry>,
}

fn number(ty: ValueType, bits: u64) -> Option<f64> {
    match ty {
        ValueType::Bool | ValueType::Enum => Some(bits as f64),
        _ => ty.bits_to_f64(bits),
    }
}

impl Verifier {
    /// The verified outputs of `plan`, with the index of each one's owner
    /// group in `plan.groups`.
    pub fn new(plan: &Plan) -> Self {
        let entries = plan
            .outputs
            .iter()
            .filter_map(|o| {
                let verify = o.verify.as_ref()?;
                Some(Entry {
                    group: plan.groups.iter().position(|g| g.name == o.group)?,
                    output: plan.id(&o.path)?,
                    output_ty: o.ty,
                    readback: plan.id(&verify.readback)?,
                    readback_ty: plan.signal(&verify.readback)?.ty,
                    conversion: verify.conversion,
                    tolerance: verify.tolerance,
                    within_ns: verify.within_ns,
                    applied: None,
                    disagree_since: None,
                    unconfirmed: false,
                })
            })
            .collect();
        Self { entries }
    }

    /// How many outputs are verified.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no output is verified.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Checks the outputs owned by `group` after its commit at `now`.
    pub fn check(&mut self, group: usize, outputs: &Outputs<'_>, shared: &Shared, now: u64) {
        let slots = &shared.slots;
        for e in self.entries.iter_mut().filter(|e| e.group == group) {
            let was = e.unconfirmed;
            let Some(view) = outputs.view(e.output) else {
                continue;
            };
            if e.applied != Some(view.applied) {
                e.applied = Some(view.applied);
                e.disagree_since = None;
                e.unconfirmed = false;
                if was {
                    shared.set_unconfirmed(e.output, false);
                }
            }
            let Applied::Value(bits) = view.applied else {
                continue;
            };
            let agrees = slots
                .get(e.readback.index())
                .and_then(|s| s.read(8).ok())
                .filter(|s| s.quality().is_good())
                .and_then(|s| number(e.readback_ty, s.transport_bits()))
                .zip(number(e.output_ty, bits))
                .is_some_and(|(read, applied)| {
                    let read = e.conversion.apply(read);
                    match e.tolerance {
                        Some(t) => (read - applied).abs() <= t,
                        None => read == applied,
                    }
                });
            let Some(slot) = slots.get(e.output.index()) else {
                continue;
            };
            if agrees {
                if e.unconfirmed {
                    shared.set_unconfirmed(e.output, false);
                }
                e.disagree_since = None;
                e.unconfirmed = false;
                let held = slot
                    .read(8)
                    .is_ok_and(|s| s.quality().reason() == QualityReason::NotConfirmed);
                if held {
                    mark(slot, Quality::GOOD, now);
                }
                continue;
            }
            let since = *e.disagree_since.get_or_insert(now);
            if !e.unconfirmed && now.saturating_sub(since) >= e.within_ns {
                e.unconfirmed = true;
                shared.set_unconfirmed(e.output, true);
                mark(
                    slot,
                    Quality::new(QualityCode::Uncertain, QualityReason::NotConfirmed),
                    now,
                );
            }
        }
    }
}

fn mark(slot: &Slot, quality: Quality, now: u64) {
    if let Ok(sample) = slot.read(8) {
        slot.write(sample.transport_bits(), quality, NO_TS_SRC, now);
    }
}
