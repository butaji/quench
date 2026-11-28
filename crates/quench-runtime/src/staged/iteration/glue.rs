#![allow(unused_imports)]
use super::*;
use crate::staged::{Concrete, Stage};

/// The interpreter's reading of `for_in_next`: the key the step over `state` yields, among the array
/// records of `heap`, in collection epoch `epoch`; `None` leaves the step to the VM's full
/// one, which also advances nothing the row did not.
#[inline(always)]
pub fn concrete_for_in_next(heap: &crate::heap::Heap, state: crate::Value, epoch: u64) -> Option<crate::Value> {
    let arrays = heap.array_record_base()?;
    let mut stage = Concrete::over(heap);
    for_in_next(&mut stage, arrays as u64, state.to_bits(), epoch, ()).map(crate::Value::from_bits)
}
