//! The named store as a staged row: the plan a site proved decides whether the value goes to
//! an own slot in place (by the first arm whose shape the receiver has), through the slot a
//! loop's entry projected, or to the VM, and which of the VM's entries completes what no arm
//! takes (its own-field hit, through the bound slot first, or its adding entries).

use super::ic::{self, Reached};
use super::property::{base_word, guard_receiver_shape, Owner, PlanSource, PropertyFrame, StoreArm, StoreJoin, StorePlan};
use super::store::{admit_named, own_store, own_store_proved, own_store_raw, slot_holds, uncallable};
use super::Stage;
use crate::bytecode::{FieldBase, Register};
use crate::typed_ir::{Representation, Type};

/// A stored value: its tagged word (`None` where no arm needs it before the VM's completion,
/// which reads it from the register), its type, and its binary64 word where the frame holds one.
type Stored<W> = (Option<W>, Type, Option<W>);

/// `base.atom = value` through cache site `site`: the stored word, or a leave at `miss`.
/// `base` is a register, or the bound `this` for `None`; the `value` register is read tagged,
/// with the fact the frame proved of its type. Where every arm stores into raw doubles, the
/// register's binary64 word goes in as it is and only the VM's completion boxes it: the word
/// answered is then that binary64 word.
#[inline(always)]
pub(super) fn set_field<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F,
    (value, base): (Register, Option<Register>), (atom, site): (u32, u16), miss: S::Miss) -> Option<S::Word> {
    let fact = frame.value_fact(value);
    let (raw, tagged) = match frame.plans().store_plan(site) {
        Some(StorePlan::Own { arms, .. }) => (arms.iter().any(|arm| arm.raw_double), arms.iter().any(|arm| !arm.raw_double)),
        Some(StorePlan::Entry) if frame.entry_slot_raw() => (true, false),
        _ => (false, true),
    };
    let number = if raw { frame.stored_number(stage, value, miss) } else { None };
    match number {
        Some(number) if !tagged => store_value(stage, frame, base, (None, fact, Some(number)), Source::Register(value, number), (atom, site), miss),
        _ => {
            let tagged = frame.tagged(stage, value);
            store_value(stage, frame, base, (Some(tagged), fact, number), value_source(tagged), (atom, site), miss)
        }
    }
}

/// `base.atom = value` of a word no register names, of type `fact`: the word, or a leave at `miss`.
#[inline(always)]
pub(super) fn store_word<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: Option<Register>,
    (value, fact): (S::Word, Type), (atom, site): (u32, u16), miss: S::Miss) -> Option<S::Word> {
    store_value(stage, frame, base, (Some(value), fact, None), value_source(value), (atom, site), miss)
}

/// Where the VM's completion takes the tagged word: the word itself, or the register (whose
/// binary64 word the in-place arms stored), read on that path alone.
#[derive(Clone, Copy)]
enum Source<W> {
    Word(W),
    Register(Register, W),
}

#[inline(always)]
fn value_source<W>(word: W) -> Source<W> { Source::Word(word) }

impl<W: Copy> Source<W> {
    /// The word the store answers: the tagged word, or the stored binary64 word.
    #[inline(always)]
    fn answer(self) -> W {
        match self { Self::Word(word) | Self::Register(_, word) => word }
    }

    #[inline(always)]
    fn tagged<S: Stage<Word = W>, F: PropertyFrame<S>>(self, stage: &mut S, frame: &mut F) -> W {
        match self { Self::Word(word) => word, Self::Register(register, _) => frame.tagged(stage, register) }
    }
}

/// The store of `value` through `site`'s plan: in place by its arms, else by the VM's completion,
/// which reads the tagged word from `source`.
#[inline(always)]
fn store_value<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: Option<Register>,
    (value, fact, number): Stored<S::Word>, source: Source<S::Word>, (atom, site): (u32, u16), miss: S::Miss) -> Option<S::Word> {
    let plans = frame.plans();
    let (arms, join) = match plans.store_plan(site)? {
        StorePlan::Entry => {
            // A raw-double slot takes the binary64 word; a frame with no view of it leaves.
            if frame.entry_slot_raw() {
                let number = number.or_else(#[inline(always)] || { let zero = stage.constant(0); let never = stage.ne_imm(zero, 0u32); stage.guard(never, miss).map(|()| zero) })?;
                return frame.entry_slot_store(stage, site, number, miss).map(|()| number);
            }
            let value = source.tagged(stage, frame);
            return frame.entry_slot_store(stage, site, value, miss).map(|()| value);
        }
        StorePlan::Own { arms, join } => (arms, join),
        StorePlan::Layout { id, slot, join } => {
            let base = base.map_or(FieldBase::THIS, FieldBase::register);
            let at = (Owner::of(base)?, base_word(stage, frame, base, miss)?);
            let tagged = value;
            frame.stored(stage, at.0, miss,
                #[inline(always)] |stage, frame, miss| layout_store(stage, frame, at, (id, slot), tagged?, miss),
                #[inline(always)] move |stage: &mut S, frame: &mut F, miss: S::Miss| {
                    let value = source.tagged(stage, frame);
                    vm_store(stage, frame, join, at, (atom, site), (value, fact), miss)
                })?;
            return Some(source.answer());
        }
        StorePlan::Hinted { id, slot, narrow, join } => {
            let base = base.map_or(FieldBase::THIS, FieldBase::register);
            let at = (Owner::of(base)?, base_word(stage, frame, base, miss)?);
            let tagged = source.tagged(stage, frame);
            let slow = #[inline(always)] move |stage: &mut S, frame: &mut F, miss: S::Miss| {
                if frame.field_store_hinted(stage, at.1, atom, site, tagged, (id, slot), narrow).is_none() {
                    vm_store(stage, frame, join, at, (atom, site), (tagged, fact), miss)?;
                }
                Some(())
            };
            if S::SLOT_TABLES && super::slot_store::hinted_arena(id, narrow) {
                frame.stored(stage, at.0, miss,
                    #[inline(always)] |stage, frame, miss| hinted_arena_store(stage, frame, at, (id, slot), tagged, miss), slow)?;
            } else {
                slow(stage, frame, miss)?;
            }
            return Some(source.answer());
        }
        StorePlan::Polymorphic { id, slot } => {
            let base = base.map_or(FieldBase::THIS, FieldBase::register);
            let at = (Owner::of(base)?, base_word(stage, frame, base, miss)?);
            let tagged = source.tagged(stage, frame);
            frame.field_store_polymorphic(stage, at.1, atom, site, tagged, (id, slot))?;
            return Some(source.answer());
        }
        StorePlan::Add => (&[][..], StoreJoin::Add),
        StorePlan::Host => (&[][..], StoreJoin::Host),
        StorePlan::Bound => (&[][..], StoreJoin::Bound),
    };
    let base = base.map_or(FieldBase::THIS, FieldBase::register);
    let at = (Owner::of(base)?, base_word(stage, frame, base, miss)?);
    let slow = #[inline(always)] move |stage: &mut S, frame: &mut F, miss: S::Miss| {
        let value = source.tagged(stage, frame);
        vm_store(stage, frame, join, at, (atom, site), (value, fact), miss)
    };
    if arms.is_empty() {
        slow(stage, frame, miss)?;
    } else {
        frame.stored(stage, at.0, miss,
            #[inline(always)] |stage, frame, miss| in_place(stage, frame, at, arms, (value, fact, number), miss), slow)?;
    }
    Some(source.answer())
}

/// The in-place store of `value` to `owner`'s `object` of layout `id` (`StorePlan::Layout`):
/// the method-cache admission, the receiver's record, then the hinted own store, whose layout
/// compare is the whole shape and form proof.
#[inline(always)]
fn layout_store<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, object): (Owner, S::Word),
    (id, slot): (u32, u16), value: S::Word, miss: S::Miss) -> Option<()> {
    frame.store_admitted(stage, miss)?;
    let at = frame.receiver_record(stage, owner, object, false, miss)?;
    let record_id = stage.and_imm(object, u64::from(u32::MAX));
    own_store(stage, at.record, record_id, id, true, slot, value, miss)
}

/// The tracked store of `value` into `object`'s arena-form record through the hint `(id, slot)`,
/// which `hinted_arena` admits: the method cache's admission, the receiver's record, its payload
/// once the layout compare, the slot owner's held type and the barrier's log admit the store
/// (`held_payload`), then the arena's slot. Nothing
/// takes effect before a guard, so a refusal leaves to the frame's VM store, the host path's store.
#[inline(always)]
fn hinted_arena_store<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, object): (Owner, S::Word),
    (id, slot): (u32, u16), value: S::Word, miss: S::Miss) -> Option<()> {
    frame.store_admitted(stage, miss)?;
    let at = frame.receiver_record(stage, owner, object, false, miss)?;
    let record_id = stage.and_imm(object, u64::from(u32::MAX));
    let payload = super::slot_store::held_payload(stage, at.record, record_id, (id, slot), value, miss)?;
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
    arms: &[StoreArm], value: Stored<S::Word>, miss: S::Miss) -> Option<()> {
    let named = frame.store_admission(stage, miss)?;
    let at = frame.receiver_record(stage, owner, object, false, miss)?;
    match arms {
        [arm] => {
            let shaped = guard_receiver_shape(stage, frame, (owner, true), &at, arm.shape, miss)?;
            if let (true, Some(number)) = (arm.raw_double, value.2) {
                return raw_through_layout(stage, frame, (owner, &Reached { shaped: Some(shaped), ..at }), *arm, number, miss);
            }
            if let Some(base) = kept_flat(frame, owner, *arm) {
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
fn kept_flat<S: Stage, F: PropertyFrame<S>>(frame: &F, owner: Owner, arm: StoreArm) -> Option<S::Word> {
    match frame.kept_layout(owner, arm.shape).filter(|layout| usize::from(arm.slot) < layout.storage_len)?.slots {
        super::object::Slots::Flat(base) => Some(base),
        _ => None,
    }
}

/// The tagged store of `arm` into the kept flat run at `base` of `object`: the arm's type test
/// where the slot's type may not hold the value, the barrier's gate for a reference, the word.
#[inline(always)]
fn tagged_through_layout<S: Stage>(stage: &mut S, (base, object): (S::Word, S::Word), arm: StoreArm,
    (value, fact, _): Stored<S::Word>, named: Option<S::Cond>, miss: S::Miss) -> Option<()> {
    let value = value?;
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
    let id = stage.and_imm(object, u64::from(u32::MAX));
    super::slot_store::held_gate_open(stage, id, (value, fact), miss)?;
    super::slot_store::store_word(stage, address, value, crate::typed_ir::Representation::Tagged);
    Some(())
}

/// The binary64 word `number` into the raw-double slot of `arm` of `owner`'s record `at`, whose
/// shape the caller proved: through the layout the frame kept of the object (a read of it proved
/// its run wide), else the one this store proves and keeps, as a read does (`property::kept_slot`).
#[inline(always)]
fn raw_through_layout<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, at): (Owner, &Reached<S::Word, S::Cond>),
    arm: StoreArm, number: S::Word, miss: S::Miss) -> Option<()> {
    let storage_len = usize::from(arm.slot) + 1;
    let layout = match frame.kept_layout(owner, arm.shape).filter(|layout| usize::from(arm.slot) < layout.storage_len) {
        Some(layout) => layout,
        None => {
            let layout = ic::slot_layout(stage, at, super::Form::Arena, storage_len, miss)?;
            frame.keep_layout(owner, (arm.shape, at.shaped.expect("a slot store follows the guard of its shape").fact()), layout);
            layout
        }
    };
    let super::object::Slots::Flat(base) = layout.slots else { return own_store_raw(stage, at.record, arm.slot, number, miss) };
    let offset = stage.site(u64::from(arm.slot) * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as u64);
    let address = stage.add_imm(base, offset);
    super::slot_store::store_word(stage, address, number, crate::typed_ir::Representation::Float64);
    Some(())
}

/// The first of `arms` whose shape the record `at` has stores; the last arm's shape is the
/// guard of the whole.
#[inline(always)]
fn arm_switch<S: Stage>(stage: &mut S, (at, object): (&Reached<S::Word, S::Cond>, S::Word), arms: &[StoreArm],
    value: Stored<S::Word>, named: Option<S::Cond>, miss: S::Miss) -> Option<()> {
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

/// The store through one arm into `record` (the record of `object`), whose shape the caller
/// proved. A value the slot's type may not hold must be of the arm's `holds` type, else it
/// leaves at `miss` for the VM, which joins the slot's type; the shared own store then runs
/// under the narrowed fact.
#[inline(always)]
fn arm_store<S: Stage>(stage: &mut S, (record, object): (S::Word, S::Word), arm: StoreArm,
    (value, fact, number): Stored<S::Word>, named: Option<S::Cond>, miss: S::Miss) -> Option<()> {
    if arm.raw_double {
        // The arm's slot holds raw doubles: a frame with no binary64 view of the value leaves.
        let Some(number) = number else {
            let zero = stage.constant(0);
            let never = stage.ne_imm(zero, 0u32);
            stage.guard(never, miss)?;
            return Some(());
        };
        if arm.unformed {
            // The site left the form open: a packed record holds no run to store into.
            let inline = super::object::inline_storage(stage, record);
            let flat = stage.not(inline);
            stage.guard(flat, miss)?;
        }
        return own_store_raw(stage, record, arm.slot, number, miss);
    }
    let value = value?;
    let fact = match arm.holds {
        Some(holds) => {
            let held = slot_holds(stage, value, holds);
            stage.guard(held, miss)?;
            holds
        }
        None => fact,
    };
    admit_named(stage, named, (record, arm.slot), value, miss)?;
    let id = stage.and_imm(object, u64::from(u32::MAX));
    own_store_proved(stage, record, id, arm.slot, (value, fact), miss)
}
