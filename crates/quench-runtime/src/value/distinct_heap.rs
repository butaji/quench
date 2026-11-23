//! What a heap value's reference family alone decides: records, and strict equality of two
//! distinct heap values.
use super::*;

/// `Value::distinct_heap_pair`: where two distinct values' strict equality is decided.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DistinctHeapPair {
    /// Never equal: identity is their only equality.
    Unequal,
    /// Two strings, equal by contents.
    Strings,
    /// Two legacy slot cells, equal only as BigInts of one value.
    Slots,
}

impl Value {
    /// Whether the value names an object or array record, which holds only an ordinary
    /// object or an array: never a string, a BigInt or a function.
    #[inline(always)] pub fn is_record(self) -> bool {
        // The whole reference code: a symbol's code shares the object bit.
        self.is_heap()
            && matches!(
                self.0 & HEAP_REFERENCE_BITS,
                DIRECT_OBJECT_HANDLE_BIT | DIRECT_ARRAY_HANDLE_BIT
            )
    }

    /// What strict equality of two distinct values that are not both Numbers leaves to their cells,
    /// decided by the handles' reference families alone (the family bits include the tag, so a
    /// value that is no heap value differs from every heap family): only strings and BigInts
    /// (legacy slot cells) are equal by contents; every other value equals only itself, which
    /// distinctness decided.
    #[inline(always)] pub(crate) fn distinct_heap_pair(self, other: Self) -> DistinctHeapPair {
        debug_assert!(self.is_heap() && other.is_heap() && self != other);
        // Both words carry the heap tag, so their families agree when the xor of their
        // reference bits is zero, and one word's reference bits then name the pair's family.
        if (self.0 ^ other.0) & HEAP_REFERENCE_BITS != 0 {
            return DistinctHeapPair::Unequal;
        }
        match self.0 & HEAP_REFERENCE_BITS {
            0 => DistinctHeapPair::Slots,
            DIRECT_STRING_HANDLE_BITS => DistinctHeapPair::Strings,
            _ => DistinctHeapPair::Unequal,
        }
    }
}
