//! A dense array backing's element read as a staged operation: the interpreter reads it on
//! `Concrete`, a specializer emits it.

use super::{Concrete, Stage};

/// Element `index` of the dense backing of `length` values at `values`, with its address:
/// `None` (leaving at `miss`) past the backing or at a hole (`Value::DELETED`), which the
/// caller resolves the long way. `index` and `length` share one width; the address widens
/// the index first, as an in-bounds int32 index's byte offset can pass `u32::MAX`.
#[inline(always)]
pub(crate) fn dense_element<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, index: S::Word,
    miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let in_bounds = stage.ult(index, length);
    stage.guard(in_bounds, miss)?;
    let offset = stage.uextend32(index);
    let offset = stage.mul_imm(offset, std::mem::size_of::<crate::Value>() as u64);
    let address = stage.add(values, offset);
    let value = stage.load_u64(address, 0);
    let present = stage.ne_imm(value, crate::Value::DELETED.to_bits());
    stage.guard(present, miss)?;
    Some((address, value))
}

/// Replaces the present element at `address` of the dense backing at `values`, which held
/// `old`, with `value`: the backing's count of values that are no int32 (its others) follows
/// the write, and a count native code made unknown (`OTHERS_UNKNOWN`) stays unknown. The
/// count subtracts first, so an exact count never overflows on the way.
#[inline(always)]
pub(crate) fn replace_element<S: Stage>(stage: &mut S, values: S::Word, address: S::Word, old: S::Word, value: S::Word) {
    let others = stage.load_u32(values, crate::heap::OTHERS_FROM_VALUES);
    let (added, removed) = (other(stage, value), other(stage, old));
    let others = recount(stage, others, added, removed);
    stage.store_u32(values, crate::heap::OTHERS_FROM_VALUES, others);
    stage.store_u64(address, 0, value);
}

/// A backing's count of other values once a write added `added` and overwrote `removed`;
/// `OTHERS_UNKNOWN` stays unknown. Subtracting first keeps an exact count from overflowing.
#[inline(always)]
fn recount<S: Stage>(stage: &mut S, others: S::Word, added: S::Word, removed: S::Word) -> S::Word {
    let removed_first = stage.sub(others, removed);
    let recounted = stage.add(removed_first, added);
    let unknown = stage.eq_imm(others, u64::from(crate::heap::OTHERS_UNKNOWN));
    stage.select(unknown, others, recounted)
}

/// One where `value` counts among a backing's others (it is no int32), else zero.
#[inline(always)]
fn other<S: Stage>(stage: &mut S, value: S::Word) -> S::Word {
    let tag = stage.and_imm(value, crate::value::ValueLayout::TAG_MASK);
    let int = stage.eq_imm(tag, crate::value::ValueLayout::INT_TAG);
    let (zero, one) = (stage.constant(0), stage.constant(1));
    stage.select(int, zero, one)
}

/// The interpreter's reading of `replace_element` at `index` of the backing whose first value
/// `values` addresses.
///
/// # Safety
/// The backing is its array's own, alive, and holds `index`.
#[inline(always)]
pub(crate) unsafe fn concrete_replace_element(values: *mut crate::Value, index: usize, value: crate::Value) {
    let mut stage = Concrete::new(std::ptr::null());
    // SAFETY: the caller's contract.
    let address = unsafe { values.add(index) };
    let old = unsafe { address.read() };
    replace_element(&mut stage, values as u64, address as u64, old.to_bits(), value.to_bits());
}

/// The interpreter's reading of `other`.
#[inline(always)]
pub(crate) fn concrete_other(value: crate::Value) -> u32 {
    other(&mut Concrete::new(std::ptr::null()), value.to_bits()) as u32
}

/// The interpreter's reading of `recount`.
#[inline(always)]
pub(crate) fn concrete_recount(others: u32, added: u32, removed: u32) -> u32 {
    recount(&mut Concrete::new(std::ptr::null()), u64::from(others), u64::from(added), u64::from(removed)) as u32
}

/// The interpreter's reading of `dense_element` over `elements`.
#[inline(always)]
pub(crate) fn concrete_dense_element(elements: &[crate::Value], index: usize) -> Option<crate::Value> {
    let mut stage = Concrete::new(std::ptr::null());
    dense_element(&mut stage, elements.as_ptr() as u64, elements.len() as u64, index as u64, ())
        .map(|(_, value)| crate::Value::from_bits(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    #[test]
    fn a_dense_element_reads_present_values_and_leaves_past_the_backing_or_at_a_hole() {
        let elements = [Value::number(1.5), Value::DELETED, Value::UNDEFINED];
        let read = |index| concrete_dense_element(&elements, index).map(Value::to_bits);
        assert_eq!(read(0), Some(Value::number(1.5).to_bits()));
        assert_eq!(read(1), None, "a hole leaves");
        assert_eq!(read(2), Some(Value::UNDEFINED.to_bits()));
        assert_eq!(read(3), None, "past the backing leaves");
    }
}
