//! Array storage proofs: what a lane run proved about record arrays' dense element storage, and
//! typed arrays' element bytes
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

use super::{ArrayElements, ArrayObject, Cell, Heap, TypedArrayKind};
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
    /// Elements of the typed array a slot proves (`prove_typed_storage`): its length, zero
    /// for an unproved slot.
    typed_bounds: [usize; ARRAY_PROOF_SLOTS],
    /// The proved typed array, its first element and its form (`TYPED_WRITABLE`).
    typed_bases: [MaybeUninit<Value>; ARRAY_PROOF_SLOTS],
    typed_data: [MaybeUninit<NonNull<u8>>; ARRAY_PROOF_SLOTS],
    typed_forms: [u8; ARRAY_PROOF_SLOTS],
}

/// The form byte's flag: the proved view's buffer is owned by it alone, so that a store may
/// write its bytes in place (`BufferBytes::lane_mut`). The rest is the element kind, as its
/// `TYPED_ARRAYS` row.
const TYPED_WRITABLE: u8 = 1 << (u8::BITS - 1);

/// A typed array a proof covers: `bound` elements from `data`.
#[derive(Clone, Copy)]
pub(crate) struct ProvedElements {
    pub(crate) bound: usize,
    pub(crate) data: NonNull<u8>,
    pub(crate) writable: bool,
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
            typed_bounds: [0; ARRAY_PROOF_SLOTS],
            typed_bases: [MaybeUninit::uninit(); ARRAY_PROOF_SLOTS],
            typed_data: [MaybeUninit::uninit(); ARRAY_PROOF_SLOTS],
            typed_forms: [0; ARRAY_PROOF_SLOTS],
        }
    }

    /// The elements of the typed array `base`, of the kind `KIND` (a `TYPED_ARRAYS` row),
    /// through the proof in `register`'s slot; `None` where the slot does not prove `base` of
    /// that kind. The proof holds until the next retirement (module documentation), so the
    /// view's bytes are where `data` names them, and `bound` of them are the view's.
    #[inline(always)]
    pub(crate) fn typed_elements<const KIND: usize>(&self, register: u16, base: Value) -> Option<ProvedElements> {
        let slot = Self::slot(register);
        let bound = self.typed_bounds[slot];
        let form = self.typed_forms[slot];
        // SAFETY: a nonzero bound was written by `prove_typed` with the slot's base and data.
        if bound == 0 || unsafe { self.typed_bases[slot].assume_init() } != base || form & !TYPED_WRITABLE != KIND as u8 {
            return None;
        }
        Some(ProvedElements { bound, data: unsafe { self.typed_data[slot].assume_init() }, writable: form & TYPED_WRITABLE != 0 })
    }

    /// Proves the typed array `base` of the kind row `kind` in `register`'s slot.
    fn prove_typed(&mut self, register: u16, base: Value, kind: usize, elements: ProvedElements) {
        let slot = Self::slot(register);
        self.typed_forms[slot] = kind as u8 | if elements.writable { TYPED_WRITABLE } else { 0 };
        self.typed_bases[slot] = MaybeUninit::new(base);
        self.typed_data[slot] = MaybeUninit::new(elements.data);
        self.typed_bounds[slot] = elements.bound;
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
        self.typed_bounds = [0; ARRAY_PROOF_SLOTS];
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
        arm(proofs);
        proofs.prove(register, base, array);
        true
    }

    /// Proves the elements of the typed array `base` in `register`'s slot of `proofs`: a view
    /// of Number elements (not BigInt or Float16) over a fixed, attached buffer that holds its
    /// bytes in place, which then cannot move, shrink or detach without dropping or copying
    /// them (`BufferBytes` retires). Writable where the view owns them alone and the buffer is
    /// not immutable. `false` for any other value or buffer.
    pub(crate) fn prove_typed_storage(&mut self, proofs: &mut ArrayStorageProofs, register: u16, base: Value) -> bool {
        let Some(Cell::TypedArray { kind, object, buffer, length }) = self.get(base) else {
            return false;
        };
        let (kind, offset, buffer, length) = (*kind, object.offset, *buffer, length.get());
        if kind.is_bigint() || kind == TypedArrayKind::Float16 {
            return false;
        }
        let Some(Cell::ArrayBuffer { bytes, detached: false, resizable: false, immutable, .. }) = self.buffer_bytes_mut(buffer) else {
            return false;
        };
        let immutable = *immutable;
        let (data, held, writable) = match bytes.lane_mut() {
            Some(store) if !immutable => (store.as_mut_ptr(), store.len(), true),
            _ => match bytes.owned() {
                Some(store) => (store.as_ptr().cast_mut(), store.len(), false),
                None => return false,
            },
        };
        let Some(bound) = length.filter(|&length| length != 0 && offset.checked_add(length * kind.width()).is_some_and(|end| end <= held)) else {
            return false;
        };
        arm(proofs);
        // SAFETY: the offset lies within the store, which is not empty.
        let data = unsafe { NonNull::new_unchecked(data.add(offset)) };
        proofs.prove_typed(register, base, kind as usize, ProvedElements { bound, data, writable });
        true
    }
}

/// Arms `proofs` as the thread's, retiring those it replaces.
fn arm(proofs: &mut ArrayStorageProofs) {
    let armed: *mut ArrayStorageProofs = proofs;
    if ARMED_PROOFS.get() != armed {
        retire_array_storage_proofs();
        ARMED_PROOFS.set(armed);
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
    use crate::heap::Object;

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

    const INT32: usize = TypedArrayKind::Int32 as usize;

    fn buffer(heap: &mut Heap, bytes: Vec<u8>, resizable: bool, immutable: bool) -> Value {
        heap.alloc(Cell::ArrayBuffer {
            object: Box::new(Object::new(Value::NULL)),
            bytes: crate::heap::BufferBytes::from(bytes),
            shared: false,
            detached: false,
            max_byte_length: 0,
            resizable,
            immutable,
        })
    }

    fn view(heap: &mut Heap, kind: TypedArrayKind, buffer: Value, offset: usize, length: usize) -> Value {
        let object = crate::heap::BufferViewObject { object: Object::new(Value::NULL), offset };
        heap.alloc(Cell::TypedArray { kind, object: Box::new(object), buffer, length: crate::heap::ViewLength::fixed(length) })
    }

    fn int32s(heap: &mut Heap) -> (Value, Value) {
        let bytes = [1i32, 2, 3].iter().flat_map(|element| element.to_ne_bytes()).collect();
        let buffer = buffer(heap, bytes, false, false);
        (buffer, view(heap, TypedArrayKind::Int32, buffer, 4, 2))
    }

    #[test]
    fn typed_proofs_cover_a_fixed_owned_view_and_name_its_bytes() {
        let mut heap = Heap::new();
        let mut proofs = Box::new(ArrayStorageProofs::new());
        let _scope = LaneProofScope::enter();
        let (_, base) = int32s(&mut heap);
        assert!(proofs.typed_elements::<INT32>(0, base).is_none());
        assert!(heap.prove_typed_storage(&mut proofs, 0, base));
        let elements = proofs.typed_elements::<INT32>(0, base).unwrap();
        assert!(elements.writable);
        assert_eq!(elements.bound, 2);
        assert_eq!(unsafe { elements.data.as_ptr().cast::<i32>().read_unaligned() }, 2);
        assert!(proofs.typed_elements::<{ TypedArrayKind::Uint8 as usize }>(0, base).is_none(), "a proof states its kind");
        assert!(proofs.typed_elements::<INT32>(1, base).is_none(), "a proof is in its register's slot");
    }

    #[test]
    fn typed_proofs_decline_what_they_cannot_hold() {
        let mut heap = Heap::new();
        let mut proofs = Box::new(ArrayStorageProofs::new());
        let _scope = LaneProofScope::enter();
        let bytes = || vec![0; 16];
        let (fixed, resizable, immutable) = (buffer(&mut heap, bytes(), false, false), buffer(&mut heap, bytes(), true, false), buffer(&mut heap, bytes(), false, true));
        let declined = [
            view(&mut heap, TypedArrayKind::BigInt64, fixed, 0, 2),
            view(&mut heap, TypedArrayKind::Float16, fixed, 0, 2),
            view(&mut heap, TypedArrayKind::Int32, resizable, 0, 2),
            view(&mut heap, TypedArrayKind::Int32, fixed, 0, 0),
            view(&mut heap, TypedArrayKind::Int32, fixed, 8, 4),
        ];
        for base in declined {
            assert!(!heap.prove_typed_storage(&mut proofs, 0, base));
            assert!(proofs.typed_elements::<INT32>(0, base).is_none());
        }
        let read_only = view(&mut heap, TypedArrayKind::Int32, immutable, 0, 4);
        assert!(heap.prove_typed_storage(&mut proofs, 0, read_only));
        assert!(!proofs.typed_elements::<INT32>(0, read_only).unwrap().writable, "an immutable buffer proves no store");
        assert!(!heap.prove_typed_storage(&mut proofs, 0, fixed), "an ArrayBuffer is no typed array");
    }

    #[test]
    fn typed_proofs_retire_when_the_buffer_gains_an_owner_unshares_detaches_or_is_collected() {
        let mut heap = Heap::new();
        let mut proofs = Box::new(ArrayStorageProofs::new());
        let _scope = LaneProofScope::enter();
        let (buffer, base) = int32s(&mut heap);
        let prove = |heap: &mut Heap, proofs: &mut ArrayStorageProofs| {
            assert!(heap.prove_typed_storage(proofs, 0, base));
            assert!(proofs.typed_elements::<INT32>(0, base).is_some());
        };
        let bytes = |heap: &mut Heap| match heap.get_mut(buffer) {
            Some(Cell::ArrayBuffer { bytes, .. }) => bytes as *mut crate::heap::BufferBytes,
            _ => unreachable!("an ArrayBuffer"),
        };
        prove(&mut heap, &mut proofs);
        let shared = unsafe { (*bytes(&mut heap)).clone() };
        assert!(proofs.typed_elements::<INT32>(0, base).is_none(), "a second owner retires the proof");
        prove(&mut heap, &mut proofs);
        assert!(!proofs.typed_elements::<INT32>(0, base).unwrap().writable, "a shared store proves no store");
        unsafe { (*bytes(&mut heap)).make_mut() }[0] = 9;
        assert!(proofs.typed_elements::<INT32>(0, base).is_none(), "unsharing retires the proof");
        drop(shared);
        prove(&mut heap, &mut proofs);
        assert!(proofs.typed_elements::<INT32>(0, base).unwrap().writable);
        // DetachArrayBuffer: the bytes are taken and dropped.
        if let Some(Cell::ArrayBuffer { bytes, detached, .. }) = heap.get_mut(buffer) {
            drop(std::mem::take(bytes));
            *detached = true;
        }
        assert!(proofs.typed_elements::<INT32>(0, base).is_none(), "detaching retires the proof");
        assert!(!heap.prove_typed_storage(&mut proofs, 0, base), "a detached buffer proves nothing");
        let (_, other) = int32s(&mut heap);
        assert!(heap.prove_typed_storage(&mut proofs, 0, other));
        heap.collect([other]);
        assert!(proofs.typed_elements::<INT32>(0, other).is_none(), "a collection retires the proof");
    }
}
