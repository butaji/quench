/// `HeapRef` is an arena index, never an address. Consequently its numeric
/// representation has no alignment-derived tag bits. Keep this contract next
/// to the type so a future representation change must update the audit and
/// its tests together.
pub const HEAP_REF_ALIGNMENT_TAG_BITS: u8 = 0;

/// Compact arena index, not a native pointer.
///
/// Ownership and lifecycle are provided by the arena that contains the index;
/// invalid indices are rejected by arena lookup. In particular, converting a
/// `HeapRef` to a pointer, or masking its low bits as a pointer tag, is invalid.
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
/// Stable, process-independent identifier for a property name.
///
/// Transition keys are persisted in inline caches, so they must not depend on
/// `DefaultHasher` implementation details (or its per-process seed).
#[inline]
pub fn property_key_id(property: &str) -> PropertyKeyId {
    PropertyKeyId(stable_hash(property.as_bytes()))
}

/// FNV-1a is intentionally small and deterministic; collisions are harmless
/// because the complete property name remains authoritative on the slow path.
#[inline]
fn stable_hash(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{HeapRef, HEAP_REF_ALIGNMENT_TAG_BITS};

    #[test]
    fn heap_reference_does_not_claim_pointer_tag_bits() {
        assert_eq!(HEAP_REF_ALIGNMENT_TAG_BITS, 0);
        assert_eq!(std::mem::size_of::<HeapRef>(), std::mem::size_of::<u32>());
        assert_eq!(std::mem::align_of::<HeapRef>(), std::mem::align_of::<u32>());
    }

    #[test]
    fn heap_reference_keeps_index_bits_intact() {
        let reference = HeapRef(3);
        assert_eq!(reference.0, 3);
    }
}
