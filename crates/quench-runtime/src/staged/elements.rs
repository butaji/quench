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
