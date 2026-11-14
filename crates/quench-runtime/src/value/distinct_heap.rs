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
    #[inline(always)] pub(crate) fn is_record(self) -> bool {
        self.is_heap()
            && matches!(
                self.0 & HEAP_FAMILY_BITS,
                DIRECT_OBJECT_HANDLE_BIT | DIRECT_ARRAY_HANDLE_BIT
            )
    }

    /// What strict equality of two distinct values that are not both Numbers leaves to their cells,
    /// decided by the handles' reference families alone (the family bits include the tag, so a
    /// value that is no heap value differs from every heap family): only strings and BigInts
    /// (legacy slot cells) are equal by contents; every other value equals only itself, which
    /// distinctness decided.
    #[inline(always)] pub(crate) fn distinct_heap_pair(self, other: Self) -> DistinctHeapPair {
        debug_assert!(self != other);
        let (family, other_family) = (self.0 & HEAP_FAMILY_MASK, other.0 & HEAP_FAMILY_MASK);
        if family != other_family {
            DistinctHeapPair::Unequal
        } else if family == TAG_HEAP | DIRECT_STRING_HANDLE_BITS {
            DistinctHeapPair::Strings
        } else if family == TAG_HEAP {
            DistinctHeapPair::Slots
        } else {
            DistinctHeapPair::Unequal
        }
    }
}
