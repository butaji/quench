//! The named store as a staged row: the plan a site proved decides whether the value goes to
//! an own slot in place (by the first arm whose shape the receiver has), to the VM's adding
//! store, or to its general one, and what the VM completes where no arm takes the store.

use super::ic::{self, Reached};
use super::property::{base_word, guard_receiver_shape, Owner, PlanSource, PropertyFrame, StoreArm, StoreJoin, StorePlan};
use super::store::{own_store_proved, slot_holds};
use super::Stage;
use crate::bytecode::{FieldBase, Register};
use crate::typed_ir::Type;

/// `base.atom = value` through cache site `site`: the stored word, or a leave at `miss`.
/// `base` is a register, or the bound `this` for `None`; the `value` register is read tagged,
/// with the fact the frame proved of its type.
#[inline(always)]
pub(super) fn set_field<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F,
    (value, base): (Register, Option<Register>), (atom, site): (u32, u16), miss: S::Miss) -> Option<S::Word> {
    let plans = frame.plans();
    let plan = plans.store_plan(site)?;
    let base = base.map_or(FieldBase::THIS, FieldBase::register);
    let object = base_word(stage, frame, base, miss)?;
    let owner = Owner::of(base)?;
    let fact = frame.value_fact(value);
    let value = frame.tagged(stage, value);
    match plan {
        StorePlan::Own { arms, join } => frame.stored(stage, owner, miss,
            |stage, frame, miss| in_place(stage, frame, (owner, object), arms, atom, (value, fact), miss),
            |stage, frame, miss| vm_store(stage, frame, join, object, (atom, site), value, miss))?,
        StorePlan::Add => vm_store(stage, frame, StoreJoin::Add, object, (atom, site), value, miss)?,
        StorePlan::Host => vm_store(stage, frame, StoreJoin::Host, object, (atom, site), value, miss)?,
    }
    Some(value)
}

/// The VM's completion of the store of `value`: its own-field hit, or its adding entries.
#[inline(always)]
fn vm_store<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, join: StoreJoin, object: S::Word,
    (atom, site): (u32, u16), value: S::Word, miss: S::Miss) -> Option<()> {
    match join {
        StoreJoin::Host => frame.field_store_host(stage, object, atom, site, value, miss),
        StoreJoin::Add => frame.field_add(stage, object, atom, site, value, miss),
    }
}

/// The in-place store of `value`, of type `fact`, to `owner`'s `object`: the method-cache
/// admission, the receiver's record, then the arm that holds its shape. A single arm's shape
/// guard is the owner's proof; several are a switch on the shape word.
#[inline(always)]
fn in_place<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, object): (Owner, S::Word),
    arms: &[StoreArm], atom: u32, value: (S::Word, Type), miss: S::Miss) -> Option<()> {
    frame.store_admitted(stage, atom, miss)?;
    let at = frame.receiver_record(stage, owner, object, false, miss)?;
    match arms {
        [arm] => {
            guard_receiver_shape(stage, frame, (owner, true), &at, arm.shape, miss)?;
            arm_store(stage, (at.record, object), *arm, value, miss)
        }
        arms => arm_switch(stage, (&at, object), arms, value, miss),
    }
}

/// The first of `arms` whose shape the record `at` has stores; the last arm's shape is the
/// guard of the whole.
fn arm_switch<S: Stage>(stage: &mut S, (at, object): (&Reached<S::Word, S::Cond>, S::Word), arms: &[StoreArm],
    value: (S::Word, Type), miss: S::Miss) -> Option<()> {
    let (arm, rest) = arms.split_first()?;
    let exact = ic::shape_word(stage, at);
    let same = { let shape = stage.site(u64::from(arm.shape)); stage.eq_imm(exact, shape) };
    if rest.is_empty() {
        stage.guard(same, miss)?;
        return arm_store(stage, (at.record, object), *arm, value, miss);
    }
    stage.choose_leaving(same,
        |stage| arm_store(stage, (at.record, object), *arm, value, miss).map(|()| stage.constant(0)),
        |stage| arm_switch(stage, (at, object), rest, value, miss).map(|()| stage.constant(0))).map(drop)
}

/// The store through one arm into `record` (the record of `object`), whose shape the caller
/// proved. A value the slot's type may not hold must be of the arm's `holds` type, else it
/// leaves at `miss` for the VM, which joins the slot's type; the shared own store then runs
/// under the narrowed fact.
#[inline(always)]
fn arm_store<S: Stage>(stage: &mut S, (record, object): (S::Word, S::Word), arm: StoreArm,
    (value, fact): (S::Word, Type), miss: S::Miss) -> Option<()> {
    let fact = match arm.holds {
        Some(holds) => {
            let held = slot_holds(stage, value, holds);
            stage.guard(held, miss)?;
            holds
        }
        None => fact,
    };
    let id = stage.and_imm(object, u64::from(u32::MAX));
    own_store_proved(stage, record, id, arm.slot, (value, fact), miss)
}
