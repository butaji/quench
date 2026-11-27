//! The named store as a staged row: the plan a site proved decides whether the value goes to
//! an own slot in place (by the first arm whose shape the receiver has), through the slot a
//! loop's entry projected, or to the VM, and which of the VM's entries completes what no arm
//! takes (its own-field hit, through the bound slot first, or its adding entries).

use super::ic::{self, Reached};
use super::property::{base_word, guard_receiver_shape, Owner, PlanSource, PropertyFrame, StoreArm, StoreJoin, StorePlan};
use super::store::{admit_named, own_store, own_store_proved, slot_holds, uncallable};
use super::Stage;
use crate::bytecode::{FieldBase, Register};
use crate::typed_ir::{Representation, Type};

/// `base.atom = value` through cache site `site`: the stored word, or a leave at `miss`.
/// `base` is a register, or the bound `this` for `None`; the `value` register is read tagged,
/// with the fact the frame proved of its type.
#[inline(always)]
pub(super) fn set_field<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F,
    (value, base): (Register, Option<Register>), (atom, site): (u32, u16), miss: S::Miss) -> Option<S::Word> {
    let fact = frame.value_fact(value);
    let tagged = frame.tagged(stage, value);
    store_value(stage, frame, base, (tagged, fact), (atom, site), miss)
}

/// `base.atom = value` of a word no register names, of type `fact`: the word, or a leave at `miss`.
#[inline(always)]
pub(super) fn store_word<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: Option<Register>,
    (value, fact): (S::Word, Type), (atom, site): (u32, u16), miss: S::Miss) -> Option<S::Word> {
    store_value(stage, frame, base, (value, fact), (atom, site), miss)
}

/// The store of `value` through `site`'s plan: in place by its arms, else by the VM's completion,
/// which reads the tagged word from `source`.
#[inline(always)]
fn store_value<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: Option<Register>,
    (value, fact): (S::Word, Type), (atom, site): (u32, u16), miss: S::Miss) -> Option<S::Word> {
    let plans = frame.plans();
    let (arms, join) = match plans.store_plan(site)? {
        StorePlan::Entry => return frame.entry_slot_store(stage, site, value, miss).map(|()| value),
        StorePlan::Own { arms, join } => (arms, join),
        StorePlan::Layout { id, slot, join } => {
            let base = base.map_or(FieldBase::THIS, FieldBase::register);
            let at = (Owner::of(base)?, base_word(stage, frame, base, miss)?);
            frame.stored(stage, at.0, miss,
                #[inline(always)] |stage, frame, miss| layout_store(stage, frame, at, (id, slot), value, miss),
                #[inline(always)] move |stage: &mut S, frame: &mut F, miss: S::Miss| {
                    vm_store(stage, frame, join, at, (atom, site), (value, fact), miss)
                })?;
            return Some(value);
        }
        StorePlan::Hinted { id, slot, narrow, join } => {
            let base = base.map_or(FieldBase::THIS, FieldBase::register);
            let at = (Owner::of(base)?, base_word(stage, frame, base, miss)?);
            if frame.field_store_hinted(stage, at.1, atom, site, value, (id, slot), narrow).is_none() {
                vm_store(stage, frame, join, at, (atom, site), (value, fact), miss)?;
            }
            return Some(value);
        }
        StorePlan::Held { id, slot, held } => {
            // A stage without the slot owner tables makes no tracked store in place (`Stage::SLOT_TABLES`).
            if !S::SLOT_TABLES { return None; }
            let base = base.map_or(FieldBase::THIS, FieldBase::register);
            let at = (Owner::of(base)?, base_word(stage, frame, base, miss)?);
            use super::slot_store::{HELD_INT32, HELD_OBJECT, HELD_OBJECT_OR_NULL, HELD_TAGGED};
            match held {
                HELD_INT32 => held_store::<S, F, HELD_INT32>(stage, frame, at, (id, slot), value, miss),
                HELD_OBJECT => held_store::<S, F, HELD_OBJECT>(stage, frame, at, (id, slot), value, miss),
                HELD_OBJECT_OR_NULL => held_store::<S, F, HELD_OBJECT_OR_NULL>(stage, frame, at, (id, slot), value, miss),
                _ => held_store::<S, F, HELD_TAGGED>(stage, frame, at, (id, slot), value, miss),
            }?;
            return Some(value);
        }
        StorePlan::Polymorphic { id, slot } => {
            let base = base.map_or(FieldBase::THIS, FieldBase::register);
            let at = (Owner::of(base)?, base_word(stage, frame, base, miss)?);
            frame.field_store_polymorphic(stage, at.1, atom, site, value, (id, slot))?;
            return Some(value);
        }
        StorePlan::Add => (&[][..], StoreJoin::Add),
        StorePlan::Host => (&[][..], StoreJoin::Host),
        StorePlan::Bound => (&[][..], StoreJoin::Bound),
    };
    let base = base.map_or(FieldBase::THIS, FieldBase::register);
    let at = (Owner::of(base)?, base_word(stage, frame, base, miss)?);
    let slow = #[inline(always)] move |stage: &mut S, frame: &mut F, miss: S::Miss| {
        vm_store(stage, frame, join, at, (atom, site), (value, fact), miss)
    };
    if arms.is_empty() {
        slow(stage, frame, miss)?;
    } else {
        frame.stored(stage, at.0, miss,
            #[inline(always)] |stage, frame, miss| in_place(stage, frame, at, arms, (value, fact), miss), slow)?;
    }
    Some(value)
}

/// The in-place store of `value` to `owner`'s `object` of layout `id` (`StorePlan::Layout`):
/// the method-cache admission, the receiver's record, then the hinted own store, whose layout
/// compare is the whole shape and form proof.
#[inline(always)]
fn layout_store<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, object): (Owner, S::Word),
    (id, slot): (u32, u16), value: S::Word, miss: S::Miss) -> Option<()> {
    frame.store_admitted(stage, miss)?;
    let at = frame.receiver_record(stage, owner, object, false, miss)?;
    let record_id = super::value::direct_object_id(stage, object);
    own_store(stage, at.record, record_id, id, true, slot, value, miss)
}

/// The tracked store of `value` into `object`'s arena-form record through the hint `(id, slot)`, by
/// a row installed for the slot's held type `HELD` (`StorePlan::Held`): the method cache's admission,
/// the receiver's record, its payload once the layout compare, the slot's type and the barrier's log
/// admit the store (`held_payload`), then the arena's slot. Nothing takes effect before a guard, so a
/// refusal leaves to the plain row, which stores and installs the row of the slot's type now.
#[inline(always)]
fn held_store<S: Stage, F: PropertyFrame<S>, const HELD: u8>(stage: &mut S, frame: &mut F, (owner, object): (Owner, S::Word),
    (id, slot): (u32, u16), value: S::Word, miss: S::Miss) -> Option<()> {
    frame.store_admitted(stage, miss)?;
    let at = frame.receiver_record(stage, owner, object, false, miss)?;
    let record_id = super::value::direct_object_id(stage, object);
    let payload = super::slot_store::held_payload::<S, HELD>(stage, at.record, record_id, (id, slot), value, miss)?;
    super::slot_store::store_flat(stage, payload, slot, (value, Representation::Tagged));
    Some(())
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

/// The in-place store of `value`, of type `fact`, to `owner`'s `object`: the run's admission, the
/// receiver's record, then the arm that holds its shape. A single arm's shape guard is the
/// owner's proof; several are a switch on the shape word. Where a method cache may name the atom,
/// the arm admits the store once its shape is proved, if the store is quiet (`admit_named`).
#[inline(always)]
fn in_place<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, object): (Owner, S::Word),
    arms: &[StoreArm], value: (S::Word, Type), miss: S::Miss) -> Option<()> {
    let named = frame.store_admission(stage, miss)?;
    let at = frame.receiver_record(stage, owner, object, false, miss)?;
    match arms {
        // An ahead image's layout arm: the record's own word proves the shape and the plain arena
        // run, so the store writes the run with no form test.
        [arm] if arm.layout => {
            super::property::guard_receiver_layout(stage, frame, (owner, true), &at, arm.shape, miss)?;
            if let Some(base) = kept_flat(frame, owner, object, *arm) {
                return tagged_through_layout(stage, (base, object), *arm, value, named, miss);
            }
            arm_store_flat(stage, (at.record, object), *arm, value, named, miss)
        }
        [arm] => {
            guard_receiver_shape(stage, frame, (owner, true), &at, arm.shape, miss)?;
            if let Some(base) = kept_flat(frame, owner, object, *arm) {
                return tagged_through_layout(stage, (base, object), *arm, value, named, miss);
            }
            arm_store(stage, (at.record, object), *arm, value, named, miss)
        }
        arms => arm_switch(stage, (&at, object), arms, value, named, miss),
    }
}

/// The flat run a read of `owner`'s object of `arm`'s shape proved and the frame kept, where it
/// holds `arm`'s slot: a store through it needs neither the storage form nor the run tested again.
#[inline(always)]
fn kept_flat<S: Stage, F: PropertyFrame<S>>(frame: &F, owner: Owner, object: S::Word, arm: StoreArm) -> Option<S::Word> {
    match frame.kept_layout(owner, object, arm.shape).filter(|layout| usize::from(arm.slot) < layout.storage_len)?.slots {
        super::object::Slots::Flat(base) => Some(base),
        _ => None,
    }
}

/// The tagged store of `arm` into the kept flat run at `base` of `object`: the arm's type test
/// where the slot's type may not hold the value, the barrier's gate for a reference, the word.
#[inline(always)]
fn tagged_through_layout<S: Stage>(stage: &mut S, (base, object): (S::Word, S::Word), arm: StoreArm,
    (value, fact): (S::Word, Type), named: Option<S::Cond>, miss: S::Miss) -> Option<()> {
    let fact = match arm.holds {
        Some(holds) => {
            let held = slot_holds(stage, value, holds);
            stage.guard(held, miss)?;
            holds
        }
        None => fact,
    };
    let offset = stage.site(u64::from(arm.slot) * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as u64);
    let address = stage.add_imm(base, offset);
    if let Some(named) = named {
        // The kept run holds the slot: a quiet store writes and overwrites no callable.
        let old = stage.load_u64(super::region::Region::ObjectSlots.mutable(), address, 0);
        let (fresh, kept) = (uncallable(stage, value), uncallable(stage, old));
        let quiet = stage.all(fresh, kept);
        let admitted = stage.any(named, quiet);
        stage.guard(admitted, miss)?;
    }
    let id = super::value::direct_object_id(stage, object);
    let fact = super::slot_store::store_fact(stage, value, fact);
    super::slot_store::held_gate_open(stage, id, (value, fact), miss)?;
    super::slot_store::store_word(stage, address, value, crate::typed_ir::Representation::Tagged);
    Some(())
}

/// The first of `arms` whose shape the record `at` has stores; the last arm's shape is the
/// guard of the whole.
#[inline(always)]
fn arm_switch<S: Stage>(stage: &mut S, (at, object): (&Reached<S::Word, S::Cond>, S::Word), arms: &[StoreArm],
    value: (S::Word, Type), named: Option<S::Cond>, miss: S::Miss) -> Option<()> {
    let (arm, rest) = arms.split_first()?;
    let exact = ic::shape_word(stage, at);
    let same = { let shape = stage.shape_site(arm.shape); stage.eq_imm(exact, shape) };
    if rest.is_empty() {
        stage.guard(same, miss)?;
        return arm_store(stage, (at.record, object), *arm, value, named, miss);
    }
    stage.choose_leaving(same,
        #[inline(always)] |stage| arm_store(stage, (at.record, object), *arm, value, named, miss).map(|()| stage.constant(0)),
        #[inline(always)] |stage| arm_switch(stage, (at, object), rest, value, named, miss).map(|()| stage.constant(0))).map(drop)
}

/// `arm_store` of a tagged arm whose layout the caller proved a plain arena run (`StoreArm::layout`):
/// the arm's type test, the method-cache admission and the gate, then the word into the run.
#[inline(always)]
fn arm_store_flat<S: Stage>(stage: &mut S, (record, object): (S::Word, S::Word), arm: StoreArm,
    (value, fact): (S::Word, Type), named: Option<S::Cond>, miss: S::Miss) -> Option<()> {
    let fact = match arm.holds {
        Some(holds) => {
            let held = slot_holds(stage, value, holds);
            stage.guard(held, miss)?;
            holds
        }
        None => fact,
    };
    admit_named(stage, named, (record, arm.slot), value, miss)?;
    let id = super::value::direct_object_id(stage, object);
    super::store::own_store_flat_proved(stage, record, id, arm.slot, (value, fact), miss)
}

/// The store through one arm into `record` (the record of `object`), whose shape the caller
/// proved. A value the slot's type may not hold must be of the arm's `holds` type, else it
/// leaves at `miss` for the VM, which joins the slot's type; the shared own store then runs
/// under the narrowed fact.
#[inline(always)]
fn arm_store<S: Stage>(stage: &mut S, (record, object): (S::Word, S::Word), arm: StoreArm,
    (value, fact): (S::Word, Type), named: Option<S::Cond>, miss: S::Miss) -> Option<()> {
    let fact = match arm.holds {
        Some(holds) => {
            let held = slot_holds(stage, value, holds);
            stage.guard(held, miss)?;
            holds
        }
        None => fact,
    };
    admit_named(stage, named, (record, arm.slot), value, miss)?;
    let id = super::value::direct_object_id(stage, object);
    own_store_proved(stage, record, id, arm.slot, (value, fact), miss)
}
