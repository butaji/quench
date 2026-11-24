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
//! replace, grow, shrink or detach a record array's backing), a unique backing gaining an owner
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

use super::{ArrayObject, Heap};
#[cfg(test)]
use super::ArrayElements;
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
pub struct ArrayStorageProofs {
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
    #[cfg(test)]
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

    #[cfg(test)]
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

    /// No proofs.
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            readable: [0; ARRAY_PROOF_SLOTS],
            writable: [0; ARRAY_PROOF_SLOTS],
            // Initialized, so `dense` reads a retired or never proved slot's words without a branch.
            bases: [MaybeUninit::zeroed(); ARRAY_PROOF_SLOTS],
            values: [MaybeUninit::new(NonNull::dangling()); ARRAY_PROOF_SLOTS],
        }
    }

    /// The slot of base register `register`.
    #[inline(always)]
    pub fn slot(register: u16) -> usize {
        usize::from(register) % ARRAY_PROOF_SLOTS
    }

    /// Whether `register`'s slot proves `base` at a bound of `bound`, which is nonzero.
    #[inline(always)]
    fn proves(&self, slot: usize, bound: usize, base: Value) -> bool {
        // SAFETY: a nonzero bound was written by `prove` with the slot's base.
        bound != 0 && unsafe { self.bases[slot].assume_init() } == base
    }

    /// The first element and the elements a read through the proof in `register`'s slot may reach,
    /// for a staged element row (`ElementFrame::proved_view`); `None` where the slot does not prove `base`.
    #[inline(always)]
    pub fn read_span(&self, register: u16, base: Value) -> Option<(NonNull<Value>, usize)> {
        let slot = Self::slot(register);
        let bound = self.readable[slot];
        // SAFETY: a proved slot's values were written by `prove` with its bound.
        self.proves(slot, bound, base).then(|| (unsafe { self.values[slot].assume_init() }, bound))
    }

    /// `read_span` for a store: the elements the proof in `register`'s slot lets a store replace in
    /// place; `None` where the slot proves no in-place replacement of `base`.
    #[inline(always)]
    pub fn write_span(&self, register: u16, base: Value) -> Option<(NonNull<Value>, usize)> {
        let slot = Self::slot(register);
        let bound = self.writable[slot];
        // SAFETY: as in `read_span`.
        self.proves(slot, bound, base).then(|| (unsafe { self.values[slot].assume_init() }, bound))
    }

    /// The dense storage `register`'s slot proves of `base`: its first element and the elements an
    /// access reaches there, the readable bound for a read and the writable one for a store that
    /// replaces in place. `(null, 0)` where the slot does not prove `base` at that bound, so the
    /// access reaches no element.
    // merge: HEAD's `dense` kept, since lane_frame.rs still calls it; the closing brace below is shared.
    #[inline(always)]
    pub(crate) fn dense(&self, register: u16, base: Value, writable: bool) -> (*mut Value, usize) {
        let slot = Self::slot(register);
        let bound = if writable { self.writable[slot] } else { self.readable[slot] };
        // SAFETY: every slot's base and values are initialized (`new`), and a nonzero bound was
        // written by `prove` with them. A slot of another base selects no element, with no branch.
        let proved = unsafe { self.bases[slot].assume_init() } == base;
        (unsafe { self.values[slot].assume_init() }.as_ptr(), if proved { bound } else { 0 })
    }

    /// Whether `register`'s slot proves `base`, whether or not an access through it hit.
    #[inline(always)]
    pub fn holds(&self, register: u16, base: Value) -> bool {
        let slot = Self::slot(register);
        self.proves(slot, self.readable[slot], base)
    }

    /// Proves `array`, the record array `base` names, in `register`'s slot.
    fn prove(&mut self, register: u16, base: Value, array: &mut ArrayObject) {
        let slot = Self::slot(register);
        let replaces = !array.has_indexed_descriptors() && !array.is_frozen() && array.arguments_map().is_none()
            && !array.was_used_as_prototype();
        // A bound below 2^31 lets an int32 key reach `element` and `replace` as its u32 bits
        // (`ElementFrame::index_offset`): a negative key lands at or above 2^31, past
        // every bound, so the bounds test is also the sign test. No dense backing is that long.
        // A narrow backing proves no index (`ArrayElements::is_narrow`): reads take the long way.
        let length = if array.elements().is_encoded() { 0 } else { array.elements().len().min(i32::MAX as usize) };
        let (values, writable) = match array.unique_values().filter(|_| replaces) {
            Some(values) => (values, length),
            None => match array.elements().wide_values() {
                // SAFETY: a slice's pointer is never null.
                Some(wide) => (unsafe { NonNull::new_unchecked(wide.as_ptr().cast_mut()) }, 0),
                None => (NonNull::dangling(), 0),
            },
        };
        self.readable[slot] = length;
        self.writable[slot] = writable;
        self.bases[slot] = MaybeUninit::new(base);
        self.values[slot] = MaybeUninit::new(values);
    }

    /// Proves nothing, until the next `prove`.
    pub fn retire(&mut self) {
        self.readable = [0; ARRAY_PROOF_SLOTS];
        self.writable = [0; ARRAY_PROOF_SLOTS];
    }
}

impl Heap {
    /// Whether a store of `stored` into the record array `base` names logs nothing in the write
    /// barrier, as `record_array_store_mut_unlogged` admits it: the gate of a proved heap-value
    /// replacement (`ElementFrame::array_store_gate`).
    #[inline(always)]
    pub fn array_store_unlogged(&self, base: Value, stored: Value) -> bool {
        base.direct_array_index().is_some_and(|record_id| !self.array_records.store_logs(record_id, stored))
    }


    /// Proves the storage of `base` in `register`'s slot of `proofs` when `base` names a
    /// record array; `false` for any other value.
    /// Widens the narrow backing of the record array `base` (`ArrayObject::widen_elements`),
    /// as a loop entry that reads it in place needs; any other value is left as it is.
    pub fn widen_array_elements(&mut self, base: Value) {
        let Some(record_id) = base.direct_array_index() else {
            return;
        };
        // SAFETY: direct-array Values name live records; `&mut self` is exclusive; widening
        // writes no reference into the record, so no barrier applies.
        let array = unsafe { self.array_records.get_live_mut_unbarriered(record_id) };
        if array.elements().is_encoded() {
            retire_array_storage_proofs();
            array.widen_elements();
        }
    }

    pub fn prove_array_storage(&mut self, proofs: &mut ArrayStorageProofs, register: u16, base: Value) -> bool {
        let Some(record_id) = base.direct_array_index() else {
            return false;
        };
        // SAFETY: direct-array Values name live records; `&mut self` is exclusive; proving
        // writes nothing into the record, so no barrier applies.
        let array = unsafe { self.array_records.get_live_mut_unbarriered(record_id) };
        // A proof's element reads read `Value` words, which a double backing's are not: such
        // an array keeps its kind and takes the interpreter's element rows, which decode it.
        // Widening it here would lose the kind for good (`KEEP_WIDE`).
        if array.elements().is_double() {
            return false;
        }
        // An array a lane proves is hot: its reads take the proof's wide values. Widening
        // copies the same values.
        array.widen_elements();
        let armed: *mut ArrayStorageProofs = proofs;
        if ARMED_PROOFS.get() != armed {
            retire_array_storage_proofs();
            ARMED_PROOFS.set(armed);
        }
        proofs.prove(register, base, array);
        true
    }
}

impl Heap {
    /// The dense storage of the record array `base` names, as `ArrayStorageProofs::prove`
    /// would prove it: its first element, the elements a read may reach, and those a store
    /// of a Number may replace in place (zero where none may). `None` for any other value,
    /// Valid until the next change that retires proofs
    /// (`retire_array_storage_proofs`): a loop kernel holds it across no call or allocation.
    pub fn array_storage_span(&mut self, base: Value) -> Option<(NonNull<Value>, usize, usize)> {
        let record_id = base.direct_array_index()?;
        // SAFETY: as in `prove_array_storage`.
        let array = unsafe { self.array_records.get_live_mut_unbarriered(record_id) };
        // The span is read and written as wide values: a double backing is no span (its words
        // are Numbers), and taking one would widen it for good.
        if array.elements().is_double() {
            return None;
        }
        array.widen_elements();
        let replaces = !array.has_indexed_descriptors() && !array.is_frozen() && array.arguments_map().is_none()
            && !array.was_used_as_prototype();
        let length = array.elements().len().min(i32::MAX as usize);
        Some(match array.unique_values().filter(|_| replaces) {
            Some(values) => (values, length, length),
            // SAFETY: a slice's pointer is never null.
            None => (unsafe { NonNull::new_unchecked(array.elements().as_slice().as_ptr().cast_mut()) }, length, 0),
        })
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

/// The address of the thread's armed-proofs word (`ARMED_PROOFS`), which a stencil row reads
/// through `JsStepDispatch::armed_array_proofs`: the word is null while no lane run's proofs
/// are armed. The address is stable for the thread's life, so the row sees each arm and
/// retirement as it happens.
pub fn armed_proofs_address() -> usize {
    ARMED_PROOFS.as_ptr().addr()
}

/// The word that holds no index-free chain link: the mirror's value while no proof covers the
/// chain of `Array.prototype`.
pub const NO_INDEX_FREE_CHAIN_LINK: u64 = 0;

/// The encoded link of `Array.prototype` while the held index-free chain proof covers its chain,
/// else `NO_INDEX_FREE_CHAIN_LINK`: the word a push row matches a dense array's prototype link
/// against (`JsStepDispatch::array_chain_index_free_address`). It mirrors the proof, so it is
/// written only with that proof: `prove_index_free_chain` sets it with the proof it records, and
/// `Proofs::retire` clears it with the proof. A `#[thread_local]` static like `ARMED_PROOFS`.
#[thread_local]
static INDEX_FREE_CHAIN_LINK: std::cell::Cell<u64> = std::cell::Cell::new(NO_INDEX_FREE_CHAIN_LINK);

/// The address of the thread's index-free chain link word (`INDEX_FREE_CHAIN_LINK`), which a
/// stencil row reads through `JsStepDispatch::array_chain_index_free_address`. The address is
/// stable for the thread's life, so the row sees each set and clear as it happens.
pub fn index_free_chain_link_address() -> usize {
    INDEX_FREE_CHAIN_LINK.as_ptr().addr()
}

/// Sets the index-free chain link word to `link`, an encoded prototype link, or to
/// `NO_INDEX_FREE_CHAIN_LINK`.
pub fn set_index_free_chain_link(link: u64) {
    INDEX_FREE_CHAIN_LINK.set(link);
}

/// Retires the armed proofs (`ARMED_PROOFS`): a change that may falsify one is about to
/// happen, or the lane run that made them enters a nested run or ends. Where no proof is
/// held this only reads the thread's pointer.
#[inline(always)]
pub fn retire_array_storage_proofs() {
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
pub struct LaneProofScope(());

impl LaneProofScope {
    #[inline]
    pub fn enter() -> Self {
        retire_array_storage_proofs();
        Self(())
    }
}

impl Drop for LaneProofScope {
    #[inline]
    fn drop(&mut self) {
        retire_array_storage_proofs();
    }
}

#[cfg(all(test, not(feature = "profile-aggregate")))]
mod tests {
    use super::*;
    use crate::heap::Object;

    fn array(heap: &mut Heap) -> Value {
        heap.alloc_array(ArrayObject::new(Object::new(Value::NULL), [Value::integer(1), Value::integer(2)]))
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
    fn further_shared_owners_preserve_read_proofs_until_detachment() {
        let mut heap = Heap::new();
        let base = array(&mut heap);
        let mut proofs = Box::new(ArrayStorageProofs::new());
        let _scope = LaneProofScope::enter();
        let first = heap.array(base).unwrap().elements().clone();
        assert!(heap.prove_array_storage(&mut proofs, 0, base));
        let second = first.clone();
        assert!(proofs.holds(0, base));
        assert_eq!(proofs.element(0, base, 0), Some(Value::integer(1)));
        assert!(!proofs.replace(0, base, 0, Value::integer(9)));
        drop(first);
        drop(second);
        // Becoming unique cannot upgrade an existing read-only proof to writable.
        assert!(!proofs.replace(0, base, 0, Value::integer(9)));
        assert!(heap.prove_array_storage(&mut proofs, 0, base));
        assert!(proofs.replace(0, base, 0, Value::integer(9)));
        let copy = heap.array(base).unwrap().elements().clone();
        assert!(!proofs.holds(0, base));
        assert!(heap.prove_array_storage(&mut proofs, 0, base));
        heap.array_mut(base).unwrap().elements_mut().make_mut().set(0, Value::integer(4));
        assert!(!proofs.holds(0, base));
        assert_eq!(copy[0], Value::integer(9));
        assert_eq!(heap.array(base).unwrap().elements()[0], Value::integer(4));
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
