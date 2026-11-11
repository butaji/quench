//! Environment storage as staged reads: a function's dense run of captured values. The
//! interpreter reads it on `Concrete`; a specializer emits it.

use super::{Concrete, Stage};
use crate::value::ValueLayout;

/// Value `slot` of the dense run of `len` environment values at `values`: its ordinary
/// value, or `None` (leaving at `miss`) past the run or where the slot holds an internal
/// marker (`Value::is_internal_marker`: a shared slot or an uninitialized binding), which
/// the caller resolves the long way.
#[inline(always)]
pub(crate) fn dense_capture<S: Stage>(stage: &mut S, values: S::Word, len: S::Word, slot: u16,
    miss: S::Miss) -> Option<S::Word> {
    let slot = u64::from(slot);
    // The slot is a per-site operand: one hole for the bounds test, one for the address.
    let bound = stage.site(slot);
    let in_bounds = stage.ugt_imm(len, bound);
    stage.guard(in_bounds, miss)?;
    let offset = stage.site(slot * std::mem::size_of::<crate::Value>() as u64);
    let address = stage.add_imm(values, offset);
    let value = stage.load_u64(address, 0);
    let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
    let ordinary = stage.ne_imm(tag, ValueLayout::INTERNAL_TAG);
    stage.guard(ordinary, miss)?;
    Some(value)
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
}
