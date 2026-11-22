// The value encoding: the tags of a NaN-boxed word, the handle bits of the heap record families, and
// the narrow form an inline property slot packs a value into. Bit encodings and the tables they index,
// nothing else. ONE source: `value.rs` re-exports these, `staged/layout_offsets.rs` aliases them as the
// stencil `words`, and the stencil crate mounts this file by `#[path]` (`stencils/layout.rs`). It names
// nothing but `core`, so it compiles in both builds.
#![allow(dead_code)]

/// The high 16 bits of a word: its tag field.
pub(crate) const TAG_MASK: u64 = 0xffff_0000_0000_0000;
/// The bits below the tag field: a tagged value's payload.
pub(crate) const PAYLOAD_MASK: u64 = 0x0000_ffff_ffff_ffff;
pub(crate) const TAG_UNDEFINED: u64 = 0x7ff9_0000_0000_0000;
pub(crate) const TAG_NULL: u64 = 0x7ffa_0000_0000_0000;
pub(crate) const TAG_BOOL: u64 = 0x7ffb_0000_0000_0000;
pub const TAG_HEAP: u64 = 0x7ffc_0000_0000_0000;
pub(crate) const TAG_INT: u64 = 0x7ffd_0000_0000_0000;
pub(crate) const TAG_DELETED: u64 = 0x7ffe_0000_0000_0000;
pub(crate) const CANONICAL_NAN: u64 = 0x7ff8_0000_0000_0000;
/// The high dword of every int32's word (`Registers::int` compares it in memory).
pub const INT_TAG_HIGH: u32 = (TAG_INT >> 32) as u32;
/// The `true` word: the `false` tag with bit 0 set.
pub(crate) const TAG_TRUE: u64 = TAG_BOOL | 1;
/// A tag field is the bits at and above this shift: the payload's width.
pub(crate) const TAG_SHIFT: u32 = PAYLOAD_MASK.count_ones();
/// The first and last tag words.
pub(crate) const FIRST_TAG: u64 = TAG_UNDEFINED;
pub(crate) const LAST_TAG: u64 = TAG_DELETED;
/// The first tag's field, and the span of the tag fields above it: a value is tagged when its field
/// less `FIRST_TAG_FIELD` is at most `TAG_FIELD_SPAN`. The tags occupy one contiguous range of tag-field
/// values, so a tag test is one range comparison.
pub(crate) const FIRST_TAG_FIELD: u64 = FIRST_TAG >> TAG_SHIFT;
pub(crate) const TAG_FIELD_SPAN: u64 = (LAST_TAG - FIRST_TAG) >> TAG_SHIFT;

const _: () = {
    let tags = [TAG_UNDEFINED, TAG_NULL, TAG_BOOL, TAG_HEAP, TAG_INT, TAG_DELETED];
    let step = 1 << TAG_SHIFT;
    let mut index = 1;
    while index < tags.len() {
        assert!(tags[index] == tags[index - 1] + step);
        index += 1;
    }
    assert!(tags[0] == FIRST_TAG && tags[tags.len() - 1] == LAST_TAG);
};

/// Whether a word's high 16 bits (`TAG_SHIFT` is 48) name a tag rather than a double's bits:
/// `Value::is_tagged` from the tag field alone, for a reader that loads only that half-word.
#[inline(always)]
pub(crate) fn tag_field_is_tagged(field: u16) -> bool {
    const { assert!(TAG_SHIFT == 48) };
    u32::from(field).wrapping_sub((FIRST_TAG >> TAG_SHIFT) as u32) <= ((LAST_TAG - FIRST_TAG) >> TAG_SHIFT) as u32
}

// The handle bits: the payload bits a heap reference's family code takes, above the record index.
pub(crate) const DIRECT_OBJECT_HANDLE_BIT: u64 = 1 << 47;
pub(crate) const DIRECT_ARRAY_HANDLE_BIT: u64 = 1 << 46;
pub(crate) const DIRECT_PROMISE_HANDLE_BIT: u64 = 1 << 45;
/// The Wasm vector record family's code: a bit below `HEAP_FAMILY_BITS`, every combination of which
/// another family names. No narrow upper word sets it, so a vector is never narrow.
pub(crate) const DIRECT_V128_HANDLE_BIT: u64 = 1 << 44;
/// A shared environment slot (`Value::shared_environment_slot`) is a `TAG_DELETED` payload with this bit.
pub(crate) const SHARED_ENVIRONMENT_SLOT_BIT: u64 = DIRECT_OBJECT_HANDLE_BIT;
/// The `TAG_DELETED` payload bit of `Value::environment_index`.
pub(crate) const ENVIRONMENT_INDEX_BIT: u64 = DIRECT_ARRAY_HANDLE_BIT;
/// Both direct-handle bits: the function record family's code.
pub(crate) const DIRECT_FUNCTION_HANDLE_BITS: u64 = DIRECT_OBJECT_HANDLE_BIT | DIRECT_ARRAY_HANDLE_BIT;
/// The await-state record family's code.
pub(crate) const DIRECT_AWAIT_HANDLE_BITS: u64 = DIRECT_PROMISE_HANDLE_BIT | DIRECT_ARRAY_HANDLE_BIT;
/// The iterator record family's code.
pub(crate) const DIRECT_ITERATOR_HANDLE_BITS: u64 = DIRECT_PROMISE_HANDLE_BIT | DIRECT_OBJECT_HANDLE_BIT;
/// The string record family's code: every family bit.
pub(crate) const DIRECT_STRING_HANDLE_BITS: u64 = DIRECT_PROMISE_HANDLE_BIT | DIRECT_OBJECT_HANDLE_BIT | DIRECT_ARRAY_HANDLE_BIT;
/// The payload bits that code the reference families a narrow value packs.
pub(crate) const HEAP_FAMILY_BITS: u64 = DIRECT_FUNCTION_HANDLE_BITS | DIRECT_PROMISE_HANDLE_BIT;
/// The environment record family's code: the vector bit with the promise bit.
pub(crate) const DIRECT_ENVIRONMENT_HANDLE_BITS: u64 = DIRECT_V128_HANDLE_BIT | DIRECT_PROMISE_HANDLE_BIT;
/// The symbol record family's code: the vector bit with the object bit.
pub(crate) const DIRECT_SYMBOL_HANDLE_BITS: u64 = DIRECT_V128_HANDLE_BIT | DIRECT_OBJECT_HANDLE_BIT;
/// The Map and Set record family's code: the vector, promise and object bits.
pub(crate) const DIRECT_COLLECTION_HANDLE_BITS: u64 = DIRECT_V128_HANDLE_BIT | DIRECT_PROMISE_HANDLE_BIT | DIRECT_OBJECT_HANDLE_BIT;
/// The Wasm GC object record family's code: the vector bit with the array bit (no narrow word sets
/// the vector bit, so it names no narrow family; `family_is` compares every family bit exactly).
pub const DIRECT_WASM_GC_HANDLE_BITS: u64 = DIRECT_V128_HANDLE_BIT | DIRECT_ARRAY_HANDLE_BIT;
/// The payload bits that code a heap value's reference family.
pub(crate) const HEAP_REFERENCE_BITS: u64 = HEAP_FAMILY_BITS | DIRECT_V128_HANDLE_BIT;
/// Where a known closure keeps its function id: the payload bits above the 32-bit slot index and below
/// `HEAP_REFERENCE_BITS`.
pub(crate) const KNOWN_CLOSURE_FUNCTION_SHIFT: u32 = u32::BITS;
pub(crate) const KNOWN_CLOSURE_FUNCTION_MASK: u64 = (1 << (DIRECT_V128_HANDLE_BIT.trailing_zeros() - KNOWN_CLOSURE_FUNCTION_SHIFT)) - 1;
pub(crate) const KNOWN_CLOSURE_FUNCTION_BITS: u64 = KNOWN_CLOSURE_FUNCTION_MASK << KNOWN_CLOSURE_FUNCTION_SHIFT;
/// The bits that name a heap value's reference family.
pub const HEAP_FAMILY_MASK: u64 = TAG_MASK | HEAP_REFERENCE_BITS;
/// The shift of the family bits within the payload, to the family's index.
pub(crate) const HEAP_FAMILY_SHIFT: u32 = HEAP_FAMILY_BITS.trailing_zeros();

// Heap tags: the tag and family bits that distinguish each direct record family.
/// Tag and family bits that distinguish a direct ordinary-array record.
pub(crate) const DIRECT_ARRAY_TAG: u64 = TAG_HEAP | DIRECT_ARRAY_HANDLE_BIT;
/// Tag and family bits that distinguish a direct ordinary-object record.
pub(crate) const DIRECT_OBJECT_TAG: u64 = TAG_HEAP | DIRECT_OBJECT_HANDLE_BIT;
/// Tag and family bits that distinguish a direct function record.
pub(crate) const DIRECT_FUNCTION_TAG: u64 = TAG_HEAP | DIRECT_FUNCTION_HANDLE_BITS;
/// Tag and family bits that distinguish a direct promise record.
pub(crate) const DIRECT_PROMISE_TAG: u64 = TAG_HEAP | DIRECT_PROMISE_HANDLE_BIT;
/// Tag and family bits that distinguish a direct iterator record.
pub(crate) const DIRECT_ITERATOR_TAG: u64 = TAG_HEAP | DIRECT_ITERATOR_HANDLE_BITS;
/// Tag and family bits that distinguish a direct collection record (a Map, Set, WeakMap or WeakSet).
pub(crate) const DIRECT_COLLECTION_TAG: u64 = TAG_HEAP | DIRECT_COLLECTION_HANDLE_BITS;
/// Tag and family bits that distinguish a direct string record.
pub(crate) const DIRECT_STRING_TAG: u64 = TAG_HEAP | DIRECT_STRING_HANDLE_BITS;
/// The upper word of a direct object handle, and its narrow code: its family bits, as `to_narrow`'s
/// table names them; `to_narrow_reference` packs it without the lookup.
pub(crate) const DIRECT_OBJECT_UPPER: u64 = TAG_HEAP | DIRECT_OBJECT_HANDLE_BIT;

// Narrow values: a `Value` packed into 32 bits for an object's inline property slot. Bit 0 set holds a
// `NARROW_INTEGER_BITS` integer above it. Bit 0 clear holds a `NARROW_CODE_BITS` code above it, naming the
// value's upper word in `NARROW_UPPER_WORDS`, and the lower word above that, at most `NARROW_INDEX_BITS` wide.
pub(crate) const NARROW_INTEGER_FLAG: u32 = 1;
pub(crate) const NARROW_CODE_BITS: u32 = 4;
pub(crate) const NARROW_CODE_SHIFT: u32 = NARROW_INTEGER_FLAG.trailing_ones();
pub(crate) const NARROW_CODE_MASK: u32 = (1 << NARROW_CODE_BITS) - 1;
pub(crate) const NARROW_INDEX_SHIFT: u32 = NARROW_CODE_SHIFT + NARROW_CODE_BITS;
/// Lower-word width of a narrow non-integer: it leaves the top two bits clear.
pub(crate) const NARROW_INDEX_BITS: u32 = u32::BITS - NARROW_INDEX_SHIFT - 2;
/// Integer width of a narrow integer: its top two bits copy the sign.
pub(crate) const NARROW_INTEGER_BITS: u32 = u32::BITS - NARROW_CODE_SHIFT - 1;
pub(crate) const LOWER_WORD: u64 = u32::MAX as u64;
pub(crate) const NARROW_HEAP_FAMILIES: usize = 1 << HEAP_FAMILY_BITS.count_ones();
/// Codes `to_narrow` can compute: a constant's base plus stray family bits. Codes past the packed field
/// name no upper word any value has.
pub(crate) const NARROW_LOOKUP_CODES: usize = 2 * (1 << NARROW_CODE_BITS);
/// Upper words of narrow non-integers by code: the eight heap reference families in `HEAP_FAMILY_BITS`
/// order, then the tagged constants. Unused codes hold zero, the upper word of a number, which `to_narrow`
/// never looks up.
pub const NARROW_UPPER_WORDS: [u64; NARROW_LOOKUP_CODES] = {
    let mut words = [0; NARROW_LOOKUP_CODES];
    let mut family = 0;
    while family < NARROW_HEAP_FAMILIES {
        words[family] = TAG_HEAP | (family as u64) << HEAP_FAMILY_SHIFT;
        family += 1;
    }
    words[NARROW_HEAP_FAMILIES] = TAG_UNDEFINED;
    words[NARROW_HEAP_FAMILIES + 1] = TAG_NULL;
    words[NARROW_HEAP_FAMILIES + 2] = TAG_BOOL;
    words[NARROW_HEAP_FAMILIES + 3] = TAG_DELETED;
    words
};
/// The position of `tag` in tag order from `FIRST_TAG`.
const fn tag_index(tag: u64) -> usize {
    ((tag - FIRST_TAG) >> TAG_SHIFT) as usize
}
/// First narrow code of each tag, in tag order from `FIRST_TAG`; a value's code adds its family bits. The
/// integer tag's entry is unused: integers pack apart.
pub(crate) const NARROW_TAG_CODES: [usize; ((LAST_TAG - FIRST_TAG) >> TAG_SHIFT) as usize + 1] = {
    let mut codes = [0; ((LAST_TAG - FIRST_TAG) >> TAG_SHIFT) as usize + 1];
    codes[tag_index(TAG_UNDEFINED)] = NARROW_HEAP_FAMILIES;
    codes[tag_index(TAG_NULL)] = NARROW_HEAP_FAMILIES + 1;
    codes[tag_index(TAG_BOOL)] = NARROW_HEAP_FAMILIES + 2;
    codes[tag_index(TAG_HEAP)] = 0;
    codes[tag_index(TAG_DELETED)] = NARROW_HEAP_FAMILIES + 3;
    codes
};
/// `null`'s narrow form, as `to_narrow` computes it.
pub(crate) const NULL_NARROW: u32 = ((NARROW_HEAP_FAMILIES + 1) as u32) << NARROW_CODE_SHIFT;
/// The narrow code of a direct object handle's upper word.
pub(crate) const DIRECT_OBJECT_NARROW_CODE: u32 = ((DIRECT_OBJECT_UPPER & HEAP_FAMILY_BITS) >> HEAP_FAMILY_SHIFT) as u32;

const _: () = assert!(NARROW_HEAP_FAMILIES == 8);
const _: () = assert!(HEAP_FAMILY_BITS >> HEAP_FAMILY_SHIFT == NARROW_HEAP_FAMILIES as u64 - 1);
const _: () = assert!(NARROW_HEAP_FAMILIES + 4 <= 1 << NARROW_CODE_BITS);
const _: () = assert!(NARROW_HEAP_FAMILIES + 3 + NARROW_HEAP_FAMILIES <= NARROW_LOOKUP_CODES);
const _: () = assert!(NARROW_UPPER_WORDS[NARROW_HEAP_FAMILIES + 1] == TAG_NULL);
const _: () = assert!(NARROW_UPPER_WORDS[DIRECT_OBJECT_NARROW_CODE as usize] == DIRECT_OBJECT_UPPER);

/// The encoding facts compiled code tests and builds values with (`Value::number`, `Value::is_tagged`).
pub struct ValueLayout;

impl ValueLayout {
    pub(crate) const TAG_MASK: u64 = TAG_MASK;
    pub(crate) const INT_TAG: u64 = TAG_INT;
    /// Tag shared by TDZ and internal environment-slot markers.
    pub(crate) const INTERNAL_TAG: u64 = TAG_DELETED;
    /// The tag of every heap reference (`Value::heap_index`).
    pub(crate) const HEAP_TAG: u64 = TAG_HEAP;
    pub(crate) const DIRECT_ARRAY_TAG: u64 = DIRECT_ARRAY_TAG;
    pub(crate) const DIRECT_OBJECT_TAG: u64 = DIRECT_OBJECT_TAG;
    pub(crate) const DIRECT_FUNCTION_TAG: u64 = DIRECT_FUNCTION_TAG;
    pub(crate) const DIRECT_PROMISE_TAG: u64 = DIRECT_PROMISE_TAG;
    pub(crate) const DIRECT_ITERATOR_TAG: u64 = DIRECT_ITERATOR_TAG;
    pub(crate) const DIRECT_COLLECTION_TAG: u64 = DIRECT_COLLECTION_TAG;
    pub(crate) const HEAP_FAMILY_MASK: u64 = HEAP_FAMILY_MASK;
    pub(crate) const DIRECT_STRING_TAG: u64 = DIRECT_STRING_TAG;
    pub(crate) const CANONICAL_NAN: u64 = CANONICAL_NAN;
    /// A tag field is the bits at and above this shift.
    pub(crate) const TAG_SHIFT: u32 = TAG_SHIFT;
    /// The first tag's field; a value is tagged when its field less this is at most the span.
    pub(crate) const FIRST_TAG_FIELD: u64 = FIRST_TAG_FIELD;
    pub(crate) const TAG_FIELD_SPAN: u64 = TAG_FIELD_SPAN;
}

/// The narrow-value facts compiled code decodes an inline property slot with, as `Value::from_narrow`
/// does: an integer below the flag, else an upper word by code with the lower word above the code.
pub struct NarrowLayout;

impl NarrowLayout {
    pub(crate) const INTEGER_FLAG: u32 = NARROW_INTEGER_FLAG;
    pub(crate) const INTEGER_BITS: u32 = NARROW_INTEGER_BITS;
    pub(crate) const CODE_SHIFT: u32 = NARROW_CODE_SHIFT;
    pub(crate) const CODE_MASK: u32 = NARROW_CODE_MASK;
    pub(crate) const INDEX_SHIFT: u32 = NARROW_INDEX_SHIFT;
    pub(crate) const INDEX_BITS: u32 = NARROW_INDEX_BITS;
    pub(crate) const DIRECT_OBJECT_UPPER: u64 = DIRECT_OBJECT_UPPER;
    pub(crate) const DIRECT_OBJECT_CODE: u32 = DIRECT_OBJECT_NARROW_CODE;
}
