#![allow(unused_imports)]
use super::*;
use crate::staged::{Concrete, Stage};
use crate::staged::region::{Access, Region};

#[cfg(test)]
/// The interpreter's reading of `replace_element` at `index` of the backing whose first value
/// `values` addresses.
///
/// # Safety
/// The backing is its array's own, alive, and holds `index`.
#[inline(always)]
pub unsafe fn concrete_replace_element(values: *mut crate::Value, index: usize, value: crate::Value) {
    let mut stage = Concrete::pure();
    // SAFETY: the caller's contract.
    let address = unsafe { values.add(index) };
    let old = unsafe { address.read() };
    // A value of the old one's kind (both int32, or both others) leaves the count as it is,
    // exact or unknown, so only the element is written: a double over a double (V8-v7
    // NavierStokes) skips the count's load, recount and store.
    if other(&mut stage, value.to_bits()) == other(&mut stage, old.to_bits()) {
        replace_uncounted_element(&mut stage, address as u64, value.to_bits());
        return;
    }
    replace_element(&mut stage, values as u64, address as u64, old.to_bits(), value.to_bits());
}

/// The interpreter's reading of `append_in_room`: `value` appended to the direct array `array` among
/// the array records at `record_base` where its backing is writable and has room; false (changing
/// nothing) leaves the append to the VM, which grows the backing. A reference passes the write
/// barrier and a hole is no element, so neither is a word the row appends.
#[inline(always)]
pub fn concrete_append_in_room(record_base: *const crate::heap::ArrayObject, array: crate::Value, value: crate::Value) -> bool {
    if value.heap_index().is_some() || value.is_deleted() {
        return false;
    }
    let mut stage = Concrete::pure();
    array_backing(&mut stage, record_base as u64, array.to_bits(), ()).is_some_and(|backing| {
        writable_backing(&mut stage, backing, ()).is_some() && append_in_room(&mut stage, backing, value.to_bits(), ()).is_some()
    })
}

/// The interpreter's reading of `dense_element` over `elements`.
#[cfg(test)]
#[inline(always)]
pub fn concrete_dense_element(elements: &[crate::Value], index: usize) -> Option<crate::Value> {
    let mut stage = Concrete::pure();
    dense_element(&mut stage, elements.as_ptr() as u64, elements.len() as u64, index as u64, ())
        .map(|(_, value)| crate::Value::from_bits(value))
}

/// The interpreter's reading of `array_length`: the `length` of the direct array `base` among the
/// array records at `record_base`, as the int32 it reads as.
#[inline(always)]
pub fn concrete_array_length(record_base: *const crate::heap::ArrayObject, base: crate::Value) -> Option<crate::Value> {
    let mut stage = Concrete::pure();
    array_length(&mut stage, record_base as u64, base.to_bits(), ()).map(crate::Value::from_bits)
}

/// `any_backing_element` on `Concrete`: the interpreter's read of element `index` of `base`.
#[inline(always)]
pub fn concrete_record_element(record_base: *const crate::heap::ArrayObject, base: crate::Value, index: usize) -> Option<crate::Value> {
    let mut stage = Concrete::pure();
    any_backing_element(&mut stage, record_base as u64, base.to_bits(), index as u64, ()).map(crate::Value::from_bits)
}
