//! The named store as a staged row: the plan a site proved decides whether the value goes to
//! an own slot in place (by the first arm whose shape the receiver has), through the slot a
//! loop's entry projected, or to the VM, and which of the VM's entries completes what no arm
//! takes (its own-field hit, through the bound slot first, or its adding entries).

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
    let fact = frame.value_fact(value);
    let value = frame.tagged(stage, value);
    store_word(stage, frame, base, (value, fact), (atom, site), miss)
}

/// `base.atom = value` of a word no register names, of type `fact`: the word, or a leave at `miss`.
#[inline(always)]
pub(super) fn store_word<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: Option<Register>,
    (value, fact): (S::Word, Type), (atom, site): (u32, u16), miss: S::Miss) -> Option<S::Word> {
    let plans = frame.plans();
    let (arms, join) = match plans.store_plan(site)? {
        StorePlan::Entry => return frame.entry_slot_store(stage, site, value, miss).map(|()| value),
        StorePlan::Own { arms, join } => (arms, join),
        StorePlan::Add => (&[][..], StoreJoin::Add),
        StorePlan::Host => (&[][..], StoreJoin::Host),
        StorePlan::Bound => (&[][..], StoreJoin::Bound),
    };
    let base = base.map_or(FieldBase::THIS, FieldBase::register);
    let at = (Owner::of(base)?, base_word(stage, frame, base, miss)?);
    if arms.is_empty() {
        vm_store(stage, frame, join, at, (atom, site), (value, fact), miss)?;
    } else {
        frame.stored(stage, at.0, miss,
            |stage, frame, miss| in_place(stage, frame, at, arms, (value, fact), miss),
            |stage, frame, miss| vm_store(stage, frame, join, at, (atom, site), (value, fact), miss))?;
    }
    Some(value)
}

/// The VM's completion of the store of `value`: its own-field hit, through the bound slot
/// first where the plan says the entry bound one, or its adding entries.
#[inline(always)]
fn vm_store<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, join: StoreJoin, (owner, object): (Owner, S::Word),
    (atom, site): (u32, u16), (value, fact): (S::Word, Type), miss: S::Miss) -> Option<()> {
    match join {
        StoreJoin::Host => frame.field_store_host(stage, object, atom, site, value, miss),
        StoreJoin::Bound => frame.field_store_bound(stage, owner, object, (atom, site), (value, fact), miss),
        StoreJoin::Add => frame.field_add(stage, object, atom, site, value, miss),
    }
}

/// The in-place store of `value`, of type `fact`, to `owner`'s `object`: the method-cache
/// admission, the receiver's record, then the arm that holds its shape. A single arm's shape
/// guard is the owner's proof; several are a switch on the shape word.
#[inline(always)]
fn in_place<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, object): (Owner, S::Word),
    arms: &[StoreArm], value: (S::Word, Type), miss: S::Miss) -> Option<()> {
    frame.store_admitted(stage, miss)?;
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
