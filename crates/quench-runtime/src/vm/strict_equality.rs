//! Strict equality of heap values: strings and BigInts are equal by contents, every other
//! heap value only to itself.
use super::*;

impl<H: Host> Vm<H> {
    /// `strict_equal` of two values that are no Numbers and not identical: equal strings or
    /// BigInts, of equal contents.
    #[inline(always)]
    pub(in crate::vm) fn heap_values_strictly_equal(&self, a: Value, b: Value) -> bool {
        a.is_heap() && b.is_heap() && self.distinct_heap_values_equal(a, b)
    }

    /// `heap_values_strictly_equal` of two distinct heap values, as the lane's `===` proved
    /// them before it left a pair of strings or BigInt cells here.
    #[inline(always)]
    pub(in crate::vm) fn distinct_heap_values_equal(&self, a: Value, b: Value) -> bool {
        use crate::value::DistinctHeapPair::{Slots, Strings, Unequal};
        match a.distinct_heap_pair(b) {
            Unequal => false,
            Strings => matches!((self.heap.string(a), self.heap.string(b)), (Some(a), Some(b)) if a == b),
            Slots => matches!((self.heap.get(a), self.heap.get(b)), (Some(Cell::BigInt(a)), Some(Cell::BigInt(b))) if a == b),
        }
    }
}

#[cfg(test)]
#[path = "strict_equality_tests.rs"]
mod tests;
