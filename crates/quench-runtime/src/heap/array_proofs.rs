//! Array storage proofs: what a lane run proved about record arrays' dense element storage
//! (`ArrayStorageProofs`), so that a loop's element accesses read and replace elements
//! through the proof instead of decoding the array at every access (Linear TOD-1247 M2).
//!
//! A proof states, for one array value: where its first element lives, how many elements
//! a read may reach there, and how many a store of a value the write barrier passes may
//! replace in place, which is none when the array is frozen, has indexed descriptors or a
//! mapped `arguments` map, shares its backing, or serves as a watched prototype.
//!
//! The heap is the authority over those facts, and it retires every proof of the running
//! lane at each change that could falsify one (`retire_array_storage_proofs`): structural
//! access to an array's elements (`ArrayObject::elements_mut`, the only path that can
//! replace, grow, shrink or detach a record array's backing), a backing gaining an owner
//! (`ArrayElements::clone`), an object becoming frozen (`Object::set_frozen`, the only
//! writer of the frozen bit) or gaining indexed descriptors or an arguments map, and every
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

use super::{ArrayObject, Heap};
use crate::value::Value;
use std::mem::MaybeUninit;

/// Proof slots of a lane run. A record reads the slot of its base register
/// (`ArrayStorageProofs::slot`): a loop's distinct base registers rarely share one, and the
/// lane gives no proved record to a base whose slot another of its loop's bases takes
/// (`specialize_proved_arrays`). Eight limit a run's bounds to 128 bytes; colliding bases
/// retain generic records, so slot exhaustion changes only specialization coverage.
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
    values: [MaybeUninit<*mut Value>; ARRAY_PROOF_SLOTS],
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
        let value = unsafe { *self.values[slot].assume_init().add(index) };
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
        if unsafe { (*values.add(index)).is_deleted() } {
            return false;
        }
        // SAFETY: the writable proof establishes exclusive backing and an in-bounds slot;
        // replacement also updates the authoritative element-kind count.
        unsafe { super::ArrayElements::replace_in_place(values, index, value) };
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
        let replaces = !array.has_indexed_descriptors() && !array.is_frozen()
            && array.arguments_map().is_none() && !array.was_used_as_prototype();
        let (values, length, writable) = match replaces.then(|| array.storage_in_place()).flatten() {
            Some((values, length)) => (values, length, length),
            None => {
                let values = array.elements().as_slice();
                (values.as_ptr().cast_mut(), values.len(), 0)
            }
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

#[cfg(all(test, not(feature = "profile-aggregate")))]
mod tests {
    use super::*;
    use crate::heap::{Cell, Object};

    fn array(heap: &mut Heap) -> Value {
        heap.alloc(Cell::Array {
            object: ArrayObject::new(Object::new(Value::NULL), [Value::integer(1), Value::integer(2)]),
        })
    }

    #[test]
    fn proved_replacements_preserve_element_kinds_and_copy_on_write() {
        let mut heap = Heap::new();
        let base = array(&mut heap);
        let mut proofs = Box::new(ArrayStorageProofs::new());
        let _scope = LaneProofScope::enter();
        assert!(heap.prove_array_storage(&mut proofs, 0, base));
        for value in [Value::number(0.5), Value::NULL, Value::integer(3)] {
            assert!(proofs.replace(0, base, 0, value));
            assert_eq!(proofs.element(0, base, 0), Some(value));
            assert_eq!(heap.array(base).unwrap().elements().holds_only_int32(), value.as_int().is_some());
        }
        let copy = heap.array(base).unwrap().elements().clone();
        assert!(!proofs.holds(0, base), "sharing retires the writable proof");
        assert!(heap.prove_array_storage(&mut proofs, 0, base));
        assert!(!proofs.replace(0, base, 0, Value::integer(4)));
        heap.array_mut(base).unwrap().elements_mut().make_mut().set(0, Value::number(1.5));
        assert_eq!(copy[0], Value::integer(3));
        assert!(copy.holds_only_int32());
        assert!(!proofs.holds(0, base));
    }

    #[test]
    fn proved_storage_retires_before_mutation_collection_and_nested_lanes() {
        let mut heap = Heap::new();
        let mut proofs = Box::new(ArrayStorageProofs::new());
        let _scope = LaneProofScope::enter();
        let mutations: [fn(&mut ArrayObject); 5] = [
            |array| array.elements_mut().make_mut().truncate(0),
            |array| array.set_frozen(true),
            |array| array.mark_indexed_descriptors(),
            |array| array.set_arguments_map(vec![0]),
            |array| array.mark_as_prototype(),
        ];
        for mutate in mutations {
            let base = array(&mut heap);
            assert!(heap.prove_array_storage(&mut proofs, 0, base));
            assert!(proofs.replace(0, base, 0, Value::integer(2)));
            mutate(heap.array_mut(base).unwrap());
            assert!(!proofs.holds(0, base));
            assert!(!proofs.replace(0, base, 0, Value::integer(3)));
        }
        let base = array(&mut heap);
        assert!(heap.prove_array_storage(&mut proofs, 0, base));
        heap.collect([base]);
        assert!(!proofs.holds(0, base));
        assert!(heap.prove_array_storage(&mut proofs, 0, base));
        {
            let _nested = LaneProofScope::enter();
            assert!(!proofs.holds(0, base));
        }
        assert!(!proofs.holds(0, base));
    }
}
