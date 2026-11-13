//! Environment storage as staged reads: a function's dense run of captured values. The
//! interpreter reads it on `Concrete`; a specializer emits it.

use super::{Concrete, Stage};
use super::region::Region;
use crate::value::ValueLayout;

/// The address and value of `slot` of the dense run of `len` environment values at `values`:
/// `None` (leaving at `miss`) past the run or where the slot holds an internal marker
/// (`Value::is_internal_marker`: a shared slot or an uninitialized binding), which the caller
/// resolves the long way.
#[inline(always)]
fn dense_slot<S: Stage>(stage: &mut S, values: S::Word, len: S::Word, slot: u16, miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let slot = u64::from(slot);
    // The slot is a per-site operand: one hole for the bounds test, one for the address.
    let bound = stage.site(slot);
    let in_bounds = stage.ugt_imm(len, bound);
    stage.guard(in_bounds, miss)?;
    let offset = stage.site(slot * std::mem::size_of::<crate::Value>() as u64);
    let address = stage.add_imm(values, offset);
    let value = stage.load_u64(Region::Cells.mutable(), address, 0);
    let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
    let ordinary = stage.ne_imm(tag, ValueLayout::INTERNAL_TAG);
    stage.guard(ordinary, miss)?;
    Some((address, value))
}

/// Value `slot` of the dense run of `len` environment values at `values`: its ordinary
/// value, or `None` (leaving at `miss`) past the run or at an internal marker.
#[inline(always)]
pub(crate) fn dense_capture<S: Stage>(stage: &mut S, values: S::Word, len: S::Word, slot: u16,
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

/// Value `slot` of the dense environment storage whose word `storage` holds: the header
/// pointer, a header whose sparse mark is clear, `slot` below its length, an ordinary value.
/// Any other storage or slot leaves at `miss` for the general read
/// (`EnvironmentSlots::dense` then `dense_plain`; `heap.environment_slot`).
#[allow(dead_code)] // consumer: thread 13's Emit lowering of LoadEnvLocal in environment windows
#[inline(always)]
pub(crate) fn environment_slot<S: Stage>(stage: &mut S, storage: S::Word, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let layout = crate::heap::NATIVE_ENVIRONMENT_LAYOUT;
    let header_pointer = stage.load_u64(Region::Cells.mutable(), storage, layout.header_pointer_offset);
    let header = stage.load_u64(Region::Cells.mutable(), header_pointer, 0);
    let marks = stage.and_imm(header, layout.sparse_mark);
    let dense = stage.eq_imm(marks, 0u64);
    stage.guard(dense, miss)?;
    let len = stage.and_imm(header, !layout.sparse_mark);
    let values = stage.add_imm(header_pointer, layout.slots_offset);
    dense_capture(stage, values, len, slot, miss)
}

/// The interpreter's `environment_slot` over the storage at `storage`.
///
/// # Safety
/// `storage` addresses a live `EnvironmentSlots`.
#[allow(dead_code)] // consumer: thread 13's Emit lowering of LoadEnvLocal in environment windows
#[inline(always)]
pub(crate) unsafe fn concrete_environment_slot(storage: *const crate::heap::EnvironmentSlots, slot: u16) -> Option<crate::Value> {
    let mut stage = Concrete::new(std::ptr::null());
    environment_slot(&mut stage, storage as u64, slot, ()).map(crate::Value::from_bits)
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
}
