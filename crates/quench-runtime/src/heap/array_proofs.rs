//! Array storage proofs: what a lane run proved about record arrays' dense element storage
//! (`ArrayStorageProofs`), so that a loop's element accesses read and replace elements
//! through the proof instead of decoding the array at every access (Linear TOD-1247 M2).
//!
//! A proof states, for one array value: where its first element lives, how many elements
//! a read may reach there, and how many a store of a value the write barrier passes may
//! replace in place (`ArrayElements::replace_through`, which keeps the backing's count of
//! non-int32 values, the one authority over it), which is none when the array is frozen,
//! has indexed descriptors or a mapped `arguments` map, is a prototype (whose dependents
//! a replacement must retire), or shares its backing.
//!
//! The heap is the authority over those facts, and it retires every proof of the running
//! lane at each change that could falsify one (`retire_array_storage_proofs`): structural
//! access to an array's elements (`ArrayObject::elements_mut`, the only path that can
//! replace, grow, shrink or detach a record array's backing), a backing gaining an owner
//! (`ArrayElements::clone`), an object becoming frozen (`Object::set_frozen`, the only
//! writer of the frozen bit), a prototype (`Object::mark_as_prototype`) or gaining indexed
//! descriptors or an arguments map, and every
//! collection, whose sweep may free an array and reuse its record. An array's identity is
//! its value: a live value names one record, and only a collection frees one. A proof
//! therefore holds, whatever ran since it was made, until the next retirement, and a record
//! whose base register holds the proved value needs no other guard. Retirement zeroes the
//! bounds, so a retired slot proves nothing and its accesses take their generic records,
//! which are exact. Native loop code appends within capacity without retiring
//! (`LENGTH_FROM_VALUES`): a proof then bounds fewer elements than the array has, which
//! only declines more accesses. The lane retires its proofs itself where its own state
//! could carry one past a change: entering a nested run, ending its own, and restoring a
//! context a native's callback copied.

use super::{ArrayElements, ArrayObject, Heap};
use crate::value::Value;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

/// Proof slots of a lane run. A record reads the slot of its base register
/// (`ArrayStorageProofs::slot`): a loop's distinct base registers rarely share one, and the
/// lane gives no proved record to a base whose slot another of its loop's bases takes
/// (`specialize_proved_arrays`). Eight keep a run's bounds in two 64-byte lines.
pub(crate) const ARRAY_PROOF_SLOTS: usize = 8;

/// A lane run's array storage proofs, by slot. The bounds come first and are the only words
/// a run starts by writing; a slot's base and values are read only below a nonzero bound,
/// which `ArrayStorageProofs::prove` writes with them.
#[repr(C)]
pub(crate) struct ArrayStorageProofs {
    /// Elements a read may reach: the array's length, zero for an unproved slot.
    readable: [usize; ARRAY_PROOF_SLOTS],
    /// Elements a store may replace in place: the length, or zero where the array admits
    /// no in-place replacement.
    writable: [usize; ARRAY_PROOF_SLOTS],
    /// The proved array value.
    bases: [MaybeUninit<Value>; ARRAY_PROOF_SLOTS],
    /// Its first element.
    values: [MaybeUninit<NonNull<Value>>; ARRAY_PROOF_SLOTS],
}

impl ArrayStorageProofs {
    /// No proofs.
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self {
            readable: [0; ARRAY_PROOF_SLOTS],
            writable: [0; ARRAY_PROOF_SLOTS],
            bases: [MaybeUninit::uninit(); ARRAY_PROOF_SLOTS],
            values: [MaybeUninit::uninit(); ARRAY_PROOF_SLOTS],
        }
    }

    /// The slot of base register `register`.
    #[inline(always)]
    pub(crate) fn slot(register: u16) -> usize {
        usize::from(register) % ARRAY_PROOF_SLOTS
    }

    /// Whether `register`'s slot proves `base` at a bound of `bound`, which is nonzero.
    #[inline(always)]
    fn proves(&self, slot: usize, bound: usize, base: Value) -> bool {
        // SAFETY: a nonzero bound was written by `prove` with the slot's base.
        bound != 0 && unsafe { self.bases[slot].assume_init() } == base
    }

    /// The present element `base[index]` through the proof in `register`'s slot; `None`
    /// where the slot does not prove `base`, past the length, or at a hole.
    #[inline(always)]
    pub(crate) fn element(&self, register: u16, base: Value, index: usize) -> Option<Value> {
        let slot = Self::slot(register);
        let bound = self.readable[slot];
        if index >= bound || !self.proves(slot, bound, base) {
            return None;
        }
        // SAFETY: the proof holds (module documentation): `index` is below the length of
        // the backing whose first element `values` names.
        let value = unsafe { *self.values[slot].assume_init().as_ptr().add(index) };
        (!value.is_deleted()).then_some(value)
    }

    /// Replaces the present element `base[index]` with `value` through the proof in
    /// `register`'s slot, as `Vm::try_replace_record_element` would; `false` stores nothing:
    /// a slot that does not prove `base` writable, an index past the length, a hole, or a
    /// value the write barrier might log.
    #[inline(always)]
    pub(crate) fn replace(&mut self, register: u16, base: Value, index: usize, value: Value) -> bool {
        let slot = Self::slot(register);
        let bound = self.writable[slot];
        if value.heap_index().is_some() || index >= bound || !self.proves(slot, bound, base) {
            return false;
        }
        // SAFETY: as in `element`; a nonzero writable bound proves the backing this array's
        // own and in-place replacement its whole store.
        let values = unsafe { self.values[slot].assume_init() };
        if unsafe { *values.as_ptr().add(index) }.is_deleted() {
            return false;
        }
        // SAFETY: as above.
        unsafe { ArrayElements::replace_through(values, index, value) };
        true
    }

    /// Whether `register`'s slot proves `base`, whether or not an access through it hit.
    #[inline(always)]
    pub(crate) fn holds(&self, register: u16, base: Value) -> bool {
        let slot = Self::slot(register);
        self.proves(slot, self.readable[slot], base)
    }

    /// Proves `array`, the record array `base` names, in `register`'s slot.
    fn prove(&mut self, register: u16, base: Value, array: &mut ArrayObject) {
        let slot = Self::slot(register);
        let replaces = !array.has_indexed_descriptors() && !array.is_frozen() && array.arguments_map().is_none()
            && !array.was_used_as_prototype();
        let length = array.elements().len();
        let (values, writable) = match array.unique_values().filter(|_| replaces) {
            Some(values) => (values, length),
            // SAFETY: a slice's pointer is never null.
            None => (unsafe { NonNull::new_unchecked(array.elements().as_slice().as_ptr().cast_mut()) }, 0),
        };
        self.readable[slot] = length;
        self.writable[slot] = writable;
        self.bases[slot] = MaybeUninit::new(base);
        self.values[slot] = MaybeUninit::new(values);
    }

    /// Proves nothing, until the next `prove`.
    pub(crate) fn retire(&mut self) {
        self.readable = [0; ARRAY_PROOF_SLOTS];
        self.writable = [0; ARRAY_PROOF_SLOTS];
    }
}

impl Heap {
    /// Proves the storage of `base` in `register`'s slot of `proofs` when `base` names a
    /// record array; `false` for any other value.
    pub(crate) fn prove_array_storage(&mut self, proofs: &mut ArrayStorageProofs, register: u16, base: Value) -> bool {
        let Some(record_id) = base.direct_array_index() else {
            return false;
        };
        // SAFETY: direct-array Values name live records; `&mut self` is exclusive; proving
        // writes nothing into the record, so no barrier applies.
        let array = unsafe { self.array_records.get_live_mut_unbarriered(record_id) };
        let armed: *mut ArrayStorageProofs = proofs;
        if ARMED_PROOFS.get() != armed {
            retire_array_storage_proofs();
            ARMED_PROOFS.set(armed);
        }
        proofs.prove(register, base, array);
        true
    }
}

/// The proofs that hold any proof on this thread (`ArrayStorageProofs::prove` arms
/// them), null when none do: only a lane run's proofs prove, and only between its entry
/// and exit, each of which retires (`LaneProofScope`). A `#[thread_local]` static rather
/// than `thread_local!`, whose access stayed an out-of-line call at each retirement (V8-v7
/// DeltaBlue, Callgrind: 215K calls, 0.23% of its instructions with the push growth path
/// it kept out of line).
#[thread_local]
static ARMED_PROOFS: std::cell::Cell<*mut ArrayStorageProofs> = std::cell::Cell::new(std::ptr::null_mut());

/// Retires the armed proofs (`ARMED_PROOFS`): a change that may falsify one is about to
/// happen, or the lane run that made them enters a nested run or ends. Where no proof is
/// held this only reads the thread's pointer.
#[inline(always)]
pub(crate) fn retire_array_storage_proofs() {
    let proofs = ARMED_PROOFS.get();
    if !proofs.is_null() {
        retire_armed(proofs);
    }
}

#[cold]
#[inline(never)]
fn retire_armed(proofs: *mut ArrayStorageProofs) {
    ARMED_PROOFS.set(std::ptr::null_mut());
    // SAFETY: armed proofs belong to the running lane, live until it retires them at its
    // exit, and no reference to them is held across the operation retiring them.
    unsafe { (*proofs).retire() };
}

/// A lane run's span for its proofs: entering retires the enclosing run's, which does not
/// see what changes while this one runs, and leaving (also by unwinding) retires the run's
/// own, which end with it. Declared after the run's context, so it leaves first.
pub(crate) struct LaneProofScope(());

impl LaneProofScope {
    pub(crate) fn enter() -> Self {
        retire_array_storage_proofs();
        Self(())
    }
}

impl Drop for LaneProofScope {
    fn drop(&mut self) {
        retire_array_storage_proofs();
    }
}
