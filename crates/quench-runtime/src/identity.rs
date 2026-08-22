/// No alignment bits are available for `HeapRef`: it is an arena index, not a
/// pointer. Keeping this explicit prevents future pointer-tagging assumptions.
pub const HEAP_REF_ALIGNMENT_TAG_BITS: u8 = 0;

/// Compact arena index, not a native pointer.
///
/// Alignment tag bits are intentionally unavailable: the value is an index
/// into host-owned tables, so pointer-alignment assumptions would be invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeapRef(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PropertyKeyId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContinuationId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodeId(pub u32);

/// Compact integer program-counter range used by VM continuations.
///
/// `code` selects immutable bytecode storage; `start` and `end` are offsets
/// into that storage. No AST/IR reference participates in a suspended return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodeRange {
    pub code: CodeId,
    pub start: u32,
    pub end: u32,
}

impl CodeRange {
    pub fn new(code: CodeId, start: u32, end: u32) -> Option<Self> {
        (start <= end).then_some(Self { code, start, end })
    }

    pub fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn contains(self, offset: u32) -> bool {
        offset >= self.start && offset < self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvironmentRef(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedCompletion {
    pub tag: u8,

    pub flags: u8,
    pub payload: u32,
    pub aux: u32,
}
#[inline]
pub fn property_key_id(property: &str) -> PropertyKeyId {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    property.hash(&mut hasher);
    PropertyKeyId(hasher.finish() as u32)
}
#[cfg(test)]
mod tests {
    use super::{HeapRef, HEAP_REF_ALIGNMENT_TAG_BITS};

    #[test]
    fn heap_reference_does_not_claim_pointer_tag_bits() {
        assert_eq!(HEAP_REF_ALIGNMENT_TAG_BITS, 0);
        assert_eq!(std::mem::size_of::<HeapRef>(), std::mem::size_of::<u32>());
    }
}
