//! The named store as a staged row: the plan a site proved decides whether the value lands in
//! an own slot by the shared `store::own_store`, or goes to the VM's adding or general store.

use super::property::{base_word, receiver, PlanSource, PropertyFrame, StorePlan};
use super::{store, Stage};
use crate::bytecode::{FieldBase, Register};

/// `base.atom = value` through cache site `site`: the stored word, or a leave at `miss`.
/// `base` is a register, or the bound `this` for `None`; the `value` register is read tagged.
#[inline(always)]
pub(super) fn set_field<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F,
    (value, base): (Register, Option<Register>), (atom, site): (u32, u16), miss: S::Miss) -> Option<S::Word> {
    let plan = frame.plans().store_plan(site)?;
    let base = base.map_or(FieldBase::THIS, FieldBase::register);
    let object = base_word(stage, frame, base, miss)?;
    let value = frame.tagged(stage, value);
    match plan {
        StorePlan::Own { shape, slot } => {
            let (at, id) = receiver(stage, frame, object, miss)?;
            store::own_store(stage, at.record, id, shape, slot, value, miss)?;
        }
        StorePlan::Add => frame.field_add(stage, object, atom, site, value, miss)?,
        StorePlan::Host => frame.field_store_host(stage, object, atom, site, value, miss)?,
    }
    Some(value)
}
