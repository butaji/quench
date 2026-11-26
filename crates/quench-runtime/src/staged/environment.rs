//! Environment storage as staged reads: a function's dense run of captured values. The
//! interpreter reads it on `Concrete`; a specializer emits it.

use super::{Concrete, Stage};
use super::region::Region;
use crate::value::ValueLayout;

/// The address of `slot` of the dense run of `len` environment values at `values`, or `None` (leaving at `miss`) past
/// the run.
#[inline(always)]
pub fn slot_address<S: Stage>(stage: &mut S, values: S::Word, len: S::Word, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let slot = u64::from(slot);
    // The slot is a per-site operand: one hole for the bounds test, one for the address.
    let bound = stage.site(slot);
    let in_bounds = stage.ugt_imm(len, bound);
    stage.guard(in_bounds, miss)?;
    let offset = stage.site(slot * std::mem::size_of::<crate::Value>() as u64);
    Some(stage.add_imm(values, offset))
}

/// The address and value of `slot` of the dense run of `len` environment values at `values`:
/// `None` (leaving at `miss`) past the run or where the slot holds an internal marker
/// (`Value::is_internal_marker`: a shared slot or an uninitialized binding), which the caller
/// resolves the long way.
#[inline(always)]
fn dense_slot<S: Stage>(stage: &mut S, values: S::Word, len: S::Word, slot: u16, miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let address = slot_address(stage, values, len, slot, miss)?;
    Some((address, capture_at(stage, address, miss)?))
}

/// The ordinary value of the environment slot at `address`, or `None` (leaving at `miss`) where it holds
/// an internal marker: the read every capture access makes once it has its slot's address, which an
/// inlined callee's capture (its closure fixed, so its slot's address too) makes alone.
#[inline(always)]
pub(crate) fn capture_at<S: Stage>(stage: &mut S, address: S::Word, miss: S::Miss) -> Option<S::Word> {
    let value = stage.load_u64(Region::Cells.mutable(), address, 0);
    let ordinary = ordinary_slot(stage, value);
    stage.guard(ordinary, miss)?;
    Some(value)
}

/// Whether the environment slot word `value` is an ordinary value: not an internal marker.
#[inline(always)]
fn ordinary_slot<S: Stage>(stage: &mut S, value: S::Word) -> S::Cond {
    let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
    stage.ne_imm(tag, ValueLayout::INTERNAL_TAG)
}

/// Value `slot` of the dense run of `len` environment values at `values`: its ordinary
/// value, or `None` (leaving at `miss`) past the run or at an internal marker.
#[inline(always)]
pub fn dense_capture<S: Stage>(stage: &mut S, values: S::Word, len: S::Word, slot: u16,
    miss: S::Miss) -> Option<S::Word> {
    dense_slot(stage, values, len, slot, miss).map(|(_, value)| value)
}

/// Stores `value` into the initialized slot `slot` of the dense run of `len` values at `values`
/// of an environment that admits every direct store (`admitted`, nonzero: the run's tag,
/// `dispatch_fast::CAPTURE_STORES`), whose barrier its admission took once. `None` (leaving at
/// `miss`) where it does not, past the run, or at an uninitialized binding, which the caller
/// resolves the long way, nothing written.
#[inline(always)]
pub(crate) fn dense_capture_store<S: Stage>(stage: &mut S, (values, len, admitted): (S::Word, S::Word, S::Word), slot: u16,
    value: S::Word, miss: S::Miss) -> Option<()> {
    let direct = stage.ne_imm(admitted, 0u32);
    stage.guard(direct, miss)?;
    let (address, _) = dense_slot(stage, values, len, slot, miss)?;
    stage.store_u64(Region::Cells.mutable(), address, 0, value);
    Some(())
}

/// `dense_capture_store` of a `value` that is no heap reference, which no barrier records whatever
/// the environment's generation: a heap reference leaves at `miss`, nothing written.
#[inline(always)]
pub fn dense_immediate_capture_store<S: Stage>(stage: &mut S, slots: (S::Word, S::Word, S::Word), slot: u16,
    value: S::Word, miss: S::Miss) -> Option<()> {
    let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
    let immediate = stage.ne_imm(tag, ValueLayout::HEAP_TAG);
    stage.guard(immediate, miss)?;
    dense_capture_store(stage, slots, slot, value, miss)
}

#[cfg(test)]
/// The interpreter's `environment_slot_open` over the storage at `storage`: whether it wrote.
///
/// # Safety
/// As `concrete_environment_slot_store`.
#[inline(always)]
pub(crate) unsafe fn concrete_environment_slot_open(storage: *mut crate::heap::EnvironmentSlots, slot: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    environment_slot_open(&mut stage, storage as u64, slot, ()).is_some()
}

/// The interpreter's `environment_slot_define` over the storage at `storage`: whether it wrote.
///
/// # Safety
/// As `concrete_environment_slot_store`.
#[inline(always)]
pub unsafe fn concrete_environment_slot_define(storage: *mut crate::heap::EnvironmentSlots, slot: u16, value: crate::Value) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    environment_slot_define(&mut stage, storage as u64, slot, value.to_bits(), ()).is_some()
}

/// The interpreter's reading of `dense_capture` over a run whose elements start at `values`.
///
/// # Safety
/// `values` addresses `len` live environment values.
#[inline(always)]
pub(crate) unsafe fn concrete_dense_capture(values: *const crate::Value, len: usize, slot: u16) -> Option<crate::Value> {
    let mut stage = Concrete::new(std::ptr::null());
    dense_capture(&mut stage, values as u64, len as u64, slot, ()).map(crate::Value::from_bits)
}

/// The interpreter's `dense_capture_store` over a run whose elements start at `values`, for a caller
/// that admitted the store and took its barrier (`CAPTURE_STORES`): whether it wrote.
///
/// # Safety
/// `values` addresses `len` live environment values, none borrowed, and the environment's
/// write authorization holds for this store.
#[inline(always)]
pub(crate) unsafe fn concrete_dense_capture_store(values: *mut crate::Value, len: usize, slot: u16, value: crate::Value) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    dense_capture_store(&mut stage, (values as u64, len as u64, u64::from(true)), slot, value.to_bits(), ()).is_some()
}

/// The values and length of the dense run of the environment storage whose word `storage` holds: the header
/// pointer, a header whose sparse mark is clear. Any other storage leaves at `miss` for the general read
/// (`EnvironmentSlots::dense` then `dense_plain`; `heap.environment_slot`).
#[inline(always)]
fn storage_run<S: Stage>(stage: &mut S, storage: S::Word, miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let layout = crate::heap::NATIVE_ENVIRONMENT_LAYOUT;
    let header_pointer = stage.load_u64(Region::Cells.mutable(), storage, layout.header_pointer_offset);
    let header = stage.load_u64(Region::Cells.mutable(), header_pointer, 0);
    let marks = stage.and_imm(header, layout.sparse_mark);
    let dense = stage.eq_imm(marks, 0u64);
    stage.guard(dense, miss)?;
    let len = stage.and_imm(header, !layout.sparse_mark);
    Some((stage.add_imm(header_pointer, layout.slots_offset), len))
}

/// Value `slot` of the dense environment storage whose word `storage` holds (`storage_run`): an ordinary value,
/// else a leave at `miss`.
#[inline(always)]
pub fn environment_slot<S: Stage>(stage: &mut S, storage: S::Word, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let (values, len) = storage_run(stage, storage, miss)?;
    dense_capture(stage, values, len, slot, miss)
}

/// Stores `value` into the initialized slot `slot` of the dense environment storage whose word `storage` holds
/// (`storage_run`), a leave at `miss` otherwise with nothing written. The caller owns the write barrier: a storage the
/// window holds in place has none, and an environment's takes none for a value that is no heap reference.
#[inline(always)]
pub fn environment_slot_store<S: Stage>(stage: &mut S, storage: S::Word, slot: u16, value: S::Word, miss: S::Miss) -> Option<()> {
    let (values, len) = storage_run(stage, storage, miss)?;
    let (address, _) = dense_slot(stage, values, len, slot, miss)?;
    stage.store_u64(Region::Cells.mutable(), address, 0, value);
    Some(())
}

/// Defines slot `slot` of the dense environment storage whose word `storage` holds (`storage_run`) as `value`: the
/// first value of a `let`, `const` or class binding, over its dead zone (the hole `Value::DELETED`) or the value an
/// earlier iteration of its scope left, which a scope's `InitializeTdz` and its declaration's store make. A leave at
/// `miss` past the run or at a slot another environment shares (any other internal marker), nothing written. The store
/// has no barrier of its own, as `environment_slot_store`'s: an activation's environment lives only within a lane run,
/// which collects nothing, so it is young or remembered (`dispatch_fast::window_records`'s `FLAG_WINDOW` store asserts it).
#[inline(always)]
pub fn environment_slot_define<S: Stage>(stage: &mut S, storage: S::Word, slot: u16, value: S::Word, miss: S::Miss) -> Option<()> {
    let (values, len) = storage_run(stage, storage, miss)?;
    let address = slot_address(stage, values, len, slot, miss)?;
    let current = stage.load_u64(Region::Cells.mutable(), address, 0);
    let ordinary = ordinary_slot(stage, current);
    let hole = stage.eq_imm(current, crate::Value::DELETED.to_bits());
    let definable = stage.any(ordinary, hole);
    stage.guard(definable, miss)?;
    stage.store_u64(Region::Cells.mutable(), address, 0, value);
    Some(())
}

/// Opens the dead zone of slot `slot` of the dense environment storage whose word `storage` holds (`InitializeTdz`):
/// the hole defined as its value (`environment_slot_define`).
#[inline(always)]
pub fn environment_slot_open<S: Stage>(stage: &mut S, storage: S::Word, slot: u16, miss: S::Miss) -> Option<()> {
    let hole = stage.constant(crate::Value::DELETED.to_bits());
    environment_slot_define(stage, storage, slot, hole, miss)
}

#[cfg(test)]
/// The interpreter's `environment_slot` over the storage at `storage`.
///
/// # Safety
/// `storage` addresses a live `EnvironmentSlots`.
#[inline(always)]
pub(crate) unsafe fn concrete_environment_slot(storage: *const crate::heap::EnvironmentSlots, slot: u16) -> Option<crate::Value> {
    let mut stage = Concrete::new(std::ptr::null());
    environment_slot(&mut stage, storage as u64, slot, ()).map(crate::Value::from_bits)
}

/// The interpreter's `environment_slot_store` over the storage at `storage`: whether it wrote.
///
/// # Safety
/// `storage` addresses a live `EnvironmentSlots` no other reference reads, and the caller owns the store's barrier.
#[inline(always)]
pub unsafe fn concrete_environment_slot_store(storage: *mut crate::heap::EnvironmentSlots, slot: u16, value: crate::Value) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    environment_slot_store(&mut stage, storage as u64, slot, value.to_bits(), ()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    #[test]
    fn a_dense_capture_reads_ordinary_values_and_leaves_past_the_run_or_at_a_marker() {
        let values = [Value::UNDEFINED, Value::DELETED, Value::number(2.5)];
        // SAFETY: `values` holds three live values.
        let read = |slot| unsafe { concrete_dense_capture(values.as_ptr(), values.len(), slot) };
        assert_eq!(read(0).map(Value::to_bits), Some(Value::UNDEFINED.to_bits()));
        assert_eq!(read(1).map(Value::to_bits), None, "an internal marker leaves");
        assert_eq!(read(2).map(Value::to_bits), Some(Value::number(2.5).to_bits()));
        assert_eq!(read(3).map(Value::to_bits), None, "past the run leaves");
    }

    /// The capture row names its slot only through holes: two slots, one op trace.
    #[test]
    fn capture_slot_is_a_hole_not_part_of_the_trace() {
        let trace = |slot: u16| {
            let mut stage = crate::staged::trace::Trace::default();
            let (values, len) = (stage.constant(0), stage.constant(0));
            dense_capture(&mut stage, values, len, slot, ()).expect("a trace never misses");
            stage
        };
        let (first, second) = (trace(2), trace(9));
        assert_eq!(first.ops, second.ops);
        assert_eq!((first.holes, second.holes), (vec![2, 16], vec![9, 72]));
    }

    /// The store writes where `dense_plain_mut` finds a place, and leaves a marker or a slot past the run as it is.
    #[test]
    fn an_environment_slot_store_writes_initialized_slots_only() {
        use crate::heap::EnvironmentSlots;
        let mut slots = EnvironmentSlots::from_values([Value::number(7.0), Value::DELETED].into_iter());
        // SAFETY: `slots` is live and unborrowed.
        let store = |slots: &mut EnvironmentSlots, slot| unsafe { concrete_environment_slot_store(slots, slot, Value::number(9.0)) };
        assert!(store(&mut slots, 0));
        assert!(!store(&mut slots, 1), "a dead zone is the helper's");
        assert!(!store(&mut slots, 2), "past the run leaves");
        let dense = slots.dense().expect("a new storage is dense");
        assert_eq!(dense.dense_plain(0).map(Value::to_bits), Some(Value::number(9.0).to_bits()));
        assert_eq!(dense.dense_plain(1).map(Value::to_bits), None);
    }

    /// The layout constants and the row read what `EnvironmentSlots::dense_plain` reads.
    #[test]
    fn an_environment_slot_reads_as_the_dense_storage_does() {
        use crate::heap::EnvironmentSlots;
        let slots = EnvironmentSlots::from_values([Value::number(7.0), Value::DELETED, Value::UNDEFINED].into_iter());
        let dense = slots.dense().expect("a new storage is dense");
        for slot in 0..4u16 {
            // SAFETY: `slots` is live.
            let read = unsafe { concrete_environment_slot(&slots, slot) };
            assert_eq!(read.map(Value::to_bits), dense.dense_plain(usize::from(slot)).map(Value::to_bits), "slot {slot}");
        }
    }

    /// A binding's first value defines over its dead zone or an earlier iteration's value; a slot another environment shares
    /// or past the run leaves, nothing written; opening the dead zone writes the hole.
    #[test]
    fn an_environment_slot_defines_over_a_hole_or_a_value_and_opens_its_dead_zone() {
        use crate::heap::EnvironmentSlots;
        // Slot 2 shares with the environment the copy was made from; the fresh slots own.
        let source = EnvironmentSlots::from_values([Value::DELETED, Value::number(7.0), Value::number(1.0)].into_iter());
        let owner = Value::from_bits(crate::value::ValueLayout::HEAP_TAG | 1);
        let mut slots = source.iteration_copy(owner, &[0, 1], crate::bytecode::Resets::listed(&[]), |_, _| None).expect("a copy");
        // SAFETY: `slots` is live and unborrowed.
        let define = |slots: &mut EnvironmentSlots, slot| unsafe { concrete_environment_slot_define(slots, slot, Value::number(9.0)) };
        assert!(define(&mut slots, 0), "a dead zone takes the first value");
        assert!(define(&mut slots, 1), "an earlier iteration's value is replaced");
        assert!(!define(&mut slots, 2), "a shared slot is the helper's");
        assert!(!define(&mut slots, 3), "past the run leaves");
        // SAFETY: as above.
        assert!(unsafe { concrete_environment_slot_open(&mut slots, 0) });
        let dense = slots.dense().expect("a new storage is dense");
        assert_eq!(dense.dense_plain(0).map(Value::to_bits), None, "the dead zone is open");
        assert_eq!(dense.dense_plain(1).map(Value::to_bits), Some(Value::number(9.0).to_bits()));
    }
}
