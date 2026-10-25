use std::fmt;

const TAG_MASK: u64 = 0xffff_0000_0000_0000;
const PAYLOAD_MASK: u64 = 0x0000_ffff_ffff_ffff;
const TAG_UNDEFINED: u64 = 0x7ff9_0000_0000_0000;
const TAG_NULL: u64 = 0x7ffa_0000_0000_0000;
const TAG_BOOL: u64 = 0x7ffb_0000_0000_0000;
const TAG_HEAP: u64 = 0x7ffc_0000_0000_0000;
const TAG_INT: u64 = 0x7ffd_0000_0000_0000;
const TAG_DELETED: u64 = 0x7ffe_0000_0000_0000;
const CANONICAL_NAN: u64 = 0x7ff8_0000_0000_0000;
/// The tags occupy one contiguous range of tag-field values, so a tag test
/// is one range comparison.
const FIRST_TAG: u64 = TAG_UNDEFINED;
/// The tag field's position: above the payload.
const TAG_SHIFT: u32 = PAYLOAD_MASK.count_ones();
const LAST_TAG: u64 = TAG_DELETED;
const _: () = {
    let tags = [
        TAG_UNDEFINED,
        TAG_NULL,
        TAG_BOOL,
        TAG_HEAP,
        TAG_INT,
        TAG_DELETED,
    ];
    let step = 1 << TAG_SHIFT;
    let mut index = 1;
    while index < tags.len() {
        assert!(tags[index] == tags[index - 1] + step);
        index += 1;
    }
    assert!(tags[0] == FIRST_TAG && tags[tags.len() - 1] == LAST_TAG);
};
const DIRECT_OBJECT_HANDLE_BIT: u64 = 1 << 47;
/// The `TAG_DELETED` payload bit of `Value::shared_environment_slot`. Environments are legacy
/// slot cells, whose payload never sets the direct-handle bits.
const SHARED_ENVIRONMENT_SLOT_BIT: u64 = DIRECT_OBJECT_HANDLE_BIT;
/// The `TAG_DELETED` payload bit of `Value::environment_index`, distinct from the shared-slot
/// bit and from the `DELETED`/`UNMATERIALIZED_ARGUMENTS` sentinels.
const ENVIRONMENT_INDEX_BIT: u64 = DIRECT_ARRAY_HANDLE_BIT;
const DIRECT_ARRAY_HANDLE_BIT: u64 = 1 << 46;
/// Both direct-handle bits: the function record family's code.
const DIRECT_FUNCTION_HANDLE_BITS: u64 = DIRECT_OBJECT_HANDLE_BIT | DIRECT_ARRAY_HANDLE_BIT;
/// The promise record family's code, below the other direct-handle bits. Legacy slot and
/// record indices stay below it (`MAX_HEAP_CELLS`, `RECORD_SPACE_CAPACITY`).
const DIRECT_PROMISE_HANDLE_BIT: u64 = 1 << 45;
/// The await-state record family's code.
const DIRECT_AWAIT_HANDLE_BITS: u64 = DIRECT_PROMISE_HANDLE_BIT | DIRECT_ARRAY_HANDLE_BIT;
/// The iterator record family's code.
const DIRECT_ITERATOR_HANDLE_BITS: u64 = DIRECT_PROMISE_HANDLE_BIT | DIRECT_OBJECT_HANDLE_BIT;
/// The string record family's code: every family bit.
const DIRECT_STRING_HANDLE_BITS: u64 =
    DIRECT_PROMISE_HANDLE_BIT | DIRECT_OBJECT_HANDLE_BIT | DIRECT_ARRAY_HANDLE_BIT;
/// The payload bits that code the reference families a narrow value packs.
const HEAP_FAMILY_BITS: u64 = DIRECT_FUNCTION_HANDLE_BITS | DIRECT_PROMISE_HANDLE_BIT;
/// The Wasm vector record family's code: a bit below `HEAP_FAMILY_BITS`, every combination
/// of which another family names. Record and legacy slot indices stay below it
/// (`RECORD_SPACE_CAPACITY`, `MAX_HEAP_CELLS`). No narrow upper word sets it, so a vector is
/// never narrow.
const DIRECT_V128_HANDLE_BIT: u64 = 1 << 44;
/// The payload bits that code a heap value's reference family (`Value::heap_reference`).
const HEAP_REFERENCE_BITS: u64 = HEAP_FAMILY_BITS | DIRECT_V128_HANDLE_BIT;
/// The bits that name a heap value's reference family.
const HEAP_FAMILY_MASK: u64 = TAG_MASK | HEAP_REFERENCE_BITS;

/// A narrow value: a `Value` packed into 32 bits for an object's inline property slot. Bit 0
/// set holds a `NARROW_INTEGER_BITS` integer above it. Bit 0 clear holds a `NARROW_CODE_BITS` code above it,
/// naming the value's upper word in `NARROW_UPPER_WORDS`, and the lower word above that, at
/// most `NARROW_INDEX_BITS` wide. Decoding rebuilds the exact bits, so a value is narrow
/// exactly when it round-trips. The top two bits of a narrow value are equal, so a payload word
/// whose upper half is a narrow value never carries an extras tag.
pub(crate) const NARROW_INTEGER_FLAG: u32 = 1;
const NARROW_CODE_BITS: u32 = 4;
const NARROW_CODE_SHIFT: u32 = NARROW_INTEGER_FLAG.trailing_ones();
const NARROW_INDEX_SHIFT: u32 = NARROW_CODE_SHIFT + NARROW_CODE_BITS;
/// Lower-word width of a narrow non-integer: it leaves the top two bits clear.
const NARROW_INDEX_BITS: u32 = u32::BITS - NARROW_INDEX_SHIFT - 2;
/// Integer width of a narrow integer: its top two bits copy the sign.
const NARROW_INTEGER_BITS: u32 = u32::BITS - NARROW_CODE_SHIFT - 1;
const LOWER_WORD: u64 = u32::MAX as u64;
/// Upper words of narrow non-integers by code: the eight heap reference families in
/// `HEAP_FAMILY_BITS` order, then the tagged constants. Unused codes hold zero, the upper word
/// of a number, which `to_narrow` never looks up.
const NARROW_UPPER_WORDS: [u64; NARROW_LOOKUP_CODES] = {
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
const HEAP_FAMILY_SHIFT: u32 = HEAP_FAMILY_BITS.trailing_zeros();
/// The position of `tag` in tag order from `FIRST_TAG`.
const fn tag_index(tag: u64) -> usize {
    ((tag - FIRST_TAG) >> TAG_SHIFT) as usize
}
/// First narrow code of each tag, in tag order from `FIRST_TAG`; a value's code adds its
/// family bits. The integer tag's entry is unused: integers pack apart.
const NARROW_TAG_CODES: [usize; ((LAST_TAG - FIRST_TAG) >> TAG_SHIFT) as usize + 1] = {
    let mut codes = [0; ((LAST_TAG - FIRST_TAG) >> TAG_SHIFT) as usize + 1];
    codes[tag_index(TAG_UNDEFINED)] = NARROW_HEAP_FAMILIES;
    codes[tag_index(TAG_NULL)] = NARROW_HEAP_FAMILIES + 1;
    codes[tag_index(TAG_BOOL)] = NARROW_HEAP_FAMILIES + 2;
    codes[tag_index(TAG_HEAP)] = 0;
    codes[tag_index(TAG_DELETED)] = NARROW_HEAP_FAMILIES + 3;
    codes
};
/// Codes `to_narrow` can compute: a constant's base plus stray family bits. Codes past the
/// packed field name no upper word any value has.
const NARROW_LOOKUP_CODES: usize = 2 * (1 << NARROW_CODE_BITS);
const NARROW_HEAP_FAMILIES: usize = 1 << HEAP_FAMILY_BITS.count_ones();
const _: () = assert!(HEAP_FAMILY_BITS >> HEAP_FAMILY_SHIFT == NARROW_HEAP_FAMILIES as u64 - 1);
const _: () = assert!(NARROW_HEAP_FAMILIES + 4 <= 1 << NARROW_CODE_BITS);
const _: () = assert!(NARROW_HEAP_FAMILIES + 3 + NARROW_HEAP_FAMILIES <= NARROW_LOOKUP_CODES);

impl Value {
    /// This value as a narrow value, when it has one.
    #[inline(always)]
    pub(crate) fn to_narrow(self) -> Option<u32> {
        let upper = self.0 & !LOWER_WORD;
        let lower = self.0 as u32;
        if upper == TAG_INT {
            let integer = lower as i32;
            let limit = 1i32 << (NARROW_INTEGER_BITS - 1);
            return (-limit..limit)
                .contains(&integer)
                .then_some(((integer as u32) << NARROW_CODE_SHIFT) | NARROW_INTEGER_FLAG);
        }
        // The code of the value's tag plus its family bits; only an upper word the code names
        // exactly is narrow, which also rejects numbers and family bits on a constant.
        let tag = (self.0 >> TAG_SHIFT).wrapping_sub(FIRST_TAG >> TAG_SHIFT) as usize;
        let base = *NARROW_TAG_CODES.get(tag)?;
        let code = base + ((self.0 & HEAP_FAMILY_BITS) >> HEAP_FAMILY_SHIFT) as usize;
        (NARROW_UPPER_WORDS[code] == upper && lower >> NARROW_INDEX_BITS == 0)
            .then_some(lower << NARROW_INDEX_SHIFT | (code as u32) << NARROW_CODE_SHIFT)
    }

    /// The value `narrow` packs (`to_narrow`).
    #[inline(always)]
    pub(crate) fn from_narrow(narrow: u32) -> Self {
        if narrow & NARROW_INTEGER_FLAG != 0 {
            return Self::integer((narrow as i32) >> NARROW_CODE_SHIFT);
        }
        let code = (narrow >> NARROW_CODE_SHIFT) as usize & ((1 << NARROW_CODE_BITS) - 1);
        Self(NARROW_UPPER_WORDS[code] | u64::from(narrow >> NARROW_INDEX_SHIFT))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeapReference {
    LegacySlot(u32),
    ObjectRecord(u32),
    ArrayRecord(u32),
    FunctionRecord(u32),
    PromiseRecord(u32),
    AwaitRecord(u32),
    /// A Wasm vector's bytes (`Heap::alloc_v128`).
    V128Record(u32),
    IteratorRecord(u32),
    StringRecord(u32),
}

/// `value` truncated toward zero when it fits `i32`; any other result is some `i32` whose
/// conversion back differs from `value`. x86-64 truncates directly (out-of-range and NaN
/// inputs give `i32::MIN`), skipping the saturation a Rust cast adds.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn truncate_to_i32(value: f64) -> i32 {
    use std::arch::x86_64::{_mm_cvttsd_si32, _mm_set_sd};
    // SAFETY: SSE2 is part of the x86-64 baseline.
    #[allow(unused_unsafe)]
    unsafe {
        _mm_cvttsd_si32(_mm_set_sd(value))
    }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline(always)]
fn truncate_to_i32(value: f64) -> i32 {
    value as i32
}

#[cfg(feature = "profile-aggregate")]
#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum ProfileKind {
    Undefined,
    Null,
    Boolean,
    Integer,
    Double,
    Heap,
}

#[cfg(feature = "profile-aggregate")]
impl ProfileKind {
    pub(crate) const COUNT: usize = Self::Heap as usize + 1;
    pub(crate) const NAMES: [&'static str; Self::COUNT] =
        ["undefined", "null", "boolean", "integer", "double", "heap"];
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Value(u64);

/// The integer tag as a value. Interpreter code that keeps it in a machine
/// register (passing it along like any other argument) encodes an integer
/// with one bitwise or instead of first materializing the tag constant.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(crate) struct IntegerEncoding(u64);

/// The tag bits an encoded i32 carries; compiled code encodes in place.
pub(crate) const INTEGER_TAG_BITS: u64 = TAG_INT;

/// The encoding facts compiled code tests and builds values with (`Value::number`,
/// `Value::is_tagged`).
pub(crate) struct ValueLayout;

impl ValueLayout {
    pub(crate) const TAG_MASK: u64 = TAG_MASK;
    pub(crate) const INT_TAG: u64 = TAG_INT;
    /// Tag and family bits that distinguish a direct ordinary-array record.
    pub(crate) const DIRECT_ARRAY_TAG: u64 = TAG_HEAP | DIRECT_ARRAY_HANDLE_BIT;
    /// Tag and family bits that distinguish a direct ordinary-object record.
    pub(crate) const DIRECT_OBJECT_TAG: u64 = TAG_HEAP | DIRECT_OBJECT_HANDLE_BIT;
    pub(crate) const HEAP_FAMILY_MASK: u64 = HEAP_FAMILY_MASK;
    pub(crate) const CANONICAL_NAN: u64 = CANONICAL_NAN;
    /// A tag field is the bits at and above this shift.
    pub(crate) const TAG_SHIFT: u32 = TAG_SHIFT;
    /// The first tag's field; a value is tagged when its field less this is at most the span.
    pub(crate) const FIRST_TAG_FIELD: u64 = FIRST_TAG >> TAG_SHIFT;
    pub(crate) const TAG_FIELD_SPAN: u64 = (LAST_TAG - FIRST_TAG) >> TAG_SHIFT;
}

impl IntegerEncoding {
    pub(crate) const TAG: Self = Self(TAG_INT);

    #[inline(always)]
    pub(crate) fn encode(self, value: i32) -> Value {
        Value(self.0 | u64::from(value as u32))
    }
}

impl Value {
    pub const UNDEFINED: Self = Self(TAG_UNDEFINED);
    pub const NULL: Self = Self(TAG_NULL);
    pub const FALSE: Self = Self(TAG_BOOL);
    pub const TRUE: Self = Self(TAG_BOOL | 1);

    /// The Boolean `value`.
    #[inline(always)]
    pub const fn boolean(value: bool) -> Self {
        if value { Self::TRUE } else { Self::FALSE }
    }
    pub(crate) const DELETED: Self = Self(TAG_DELETED);
    /// The canonical NaN `number` stores every NaN as.
    pub(crate) const NAN: Self = Self(CANONICAL_NAN);
    /// An activation's `arguments` binding whose object does not exist yet
    /// (`CallPlan::forwards_arguments`). Not a heap reference, and distinct from every value a
    /// guest operation produces.
    pub(crate) const UNMATERIALIZED_ARGUMENTS: Self = Self(TAG_DELETED | 1);

    /// An environment slot that shares its owner environment's slot of the same index
    /// (`EnvironmentSlot::shared`): a `TAG_DELETED` payload of `SHARED_ENVIRONMENT_SLOT_BIT`
    /// and the owner's heap payload, which no guest operation produces.
    pub(crate) fn shared_environment_slot(owner: Self) -> Self {
        debug_assert!(owner.is_heap() && owner.0 & SHARED_ENVIRONMENT_SLOT_BIT == 0);
        Self(TAG_DELETED | SHARED_ENVIRONMENT_SLOT_BIT | (owner.0 & PAYLOAD_MASK))
    }

    /// A slot index or length inside a sparse environment's slot storage
    /// (`EnvironmentSlots`): a `TAG_DELETED` payload of `ENVIRONMENT_INDEX_BIT` and the number,
    /// which no guest operation produces.
    pub(crate) fn environment_index(index: u32) -> Self {
        Self(TAG_DELETED | ENVIRONMENT_INDEX_BIT | u64::from(index))
    }

    /// How the `environment_index` word `self` orders against the number `index`: the
    /// number fills the low payload bits under one fixed tag, so the words order as their
    /// numbers do and a search needs no decoding.
    #[inline(always)]
    pub(crate) fn cmp_environment_index(self, index: u32) -> std::cmp::Ordering {
        debug_assert!(self.as_environment_index().is_some());
        self.0.cmp(&Self::environment_index(index).0)
    }

    /// The number of an `environment_index`.
    #[inline(always)]
    pub(crate) fn as_environment_index(self) -> Option<u32> {
        (self.0 & (TAG_MASK | ENVIRONMENT_INDEX_BIT) == TAG_DELETED | ENVIRONMENT_INDEX_BIT)
            .then_some(self.0 as u32)
    }

    /// The owner environment of a `shared_environment_slot`.
    #[inline(always)]
    pub(crate) fn shared_environment_owner(self) -> Option<Self> {
        (self.0 & (TAG_MASK | SHARED_ENVIRONMENT_SLOT_BIT) == TAG_DELETED | SHARED_ENVIRONMENT_SLOT_BIT)
            .then(|| Self(TAG_HEAP | (self.0 & PAYLOAD_MASK & !SHARED_ENVIRONMENT_SLOT_BIT)))
    }

    /// A Number: integral values in `i32` range other than -0 are stored as integers, NaN
    /// canonically. A value is integral exactly when its truncation converts back to the
    /// same bits, which also excludes -0 (+0's bits differ) and NaN.
    pub fn number(value: f64) -> Self {
        let integer = truncate_to_i32(value);
        if f64::from(integer).to_bits() == value.to_bits() {
            return Self::integer(integer);
        }
        Self(if value.is_nan() {
            CANONICAL_NAN
        } else {
            value.to_bits()
        })
    }

    #[inline(always)]
    pub(crate) fn integer(value: i32) -> Self {
        IntegerEncoding::TAG.encode(value)
    }

    /// A count or index as a Number, as `number` stores it: an integer within `i32`, without
    /// the float round trip `number` makes to find that out.
    #[inline(always)]
    pub(crate) fn length(value: usize) -> Self {
        i32::try_from(value).map_or_else(|_| Self::number(value as f64), Self::integer)
    }

    /// A 64-bit Wasm payload held in place. Every payload outside the tag
    /// space already reads as an untagged value, so only tag-space payloads
    /// need a heap box (`Cell::WasmBits64`); a static Wasm type tells the
    /// two encodings of a slot apart from its 32-bit integer encoding.
    #[inline(always)]
    pub(crate) fn from_bits64(bits: u64) -> Option<Self> {
        let value = Self(bits);
        (!value.is_tagged()).then_some(value)
    }

    /// The value's encoding, which a snapshot writes as is: it restores every heap index the
    /// value names at the same index (`crate::snapshot`).
    pub(crate) fn to_bits(self) -> u64 {
        self.0
    }

    /// A value `to_bits` gave, read back from a snapshot of the same heap layout.
    pub(crate) fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// The 64-bit Wasm payload a value holds in place.
    #[inline(always)]
    pub(crate) fn bits64(self) -> Option<u64> {
        (!self.is_tagged()).then_some(self.0)
    }

    #[inline(always)]
    pub(crate) fn as_int(self) -> Option<i32> {
        ((self.0 & TAG_MASK) == TAG_INT).then_some(self.0 as u32 as i32)
    }

    /// The bits of a slot whose Wasm type is statically f64, for an operator
    /// whose result depends on a NaN operand only through being a NaN (float
    /// arithmetic and comparison). A boxed payload's bits are a quiet NaN, so
    /// such an operator sees a NaN exactly where the payload is one.
    #[inline(always)]
    pub(crate) const fn float_bits64(self) -> u64 {
        self.0
    }

    /// A float arithmetic result held in place. A NaN whose bits fall in the
    /// tag space becomes the canonical NaN: arithmetic may produce any
    /// arithmetic NaN, and the canonical one is.
    #[inline(always)]
    pub(crate) fn from_float_bits64(bits: u64) -> Self {
        let value = Self(bits);
        if value.is_tagged() {
            // Only a NaN whose payload collides with a tag: a predicted
            // branch keeps the check off the result's dependency chain,
            // where a conditional move would lengthen every float chain.
            std::hint::cold_path();
            Self(CANONICAL_NAN)
        } else {
            value
        }
    }

    /// The 32-bit payload of a slot whose Wasm type is statically i32 or f32.
    #[inline(always)]
    pub(crate) const fn wasm_bits32(self) -> u32 {
        self.0 as u32
    }

    #[inline(always)]
    pub(crate) fn int_pair(left: Self, right: Self) -> Option<(i32, i32)> {
        let tags = (left.0 ^ TAG_INT) | (right.0 ^ TAG_INT);
        (tags & TAG_MASK == 0).then_some((left.0 as u32 as i32, right.0 as u32 as i32))
    }

    pub(crate) fn heap(index: u32) -> Self {
        Self(TAG_HEAP | u64::from(index))
    }

    #[inline(always)]
    pub(crate) fn direct_object(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_OBJECT_HANDLE_BIT | u64::from(index))
    }

    #[inline(always)]
    pub(crate) fn direct_array(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_ARRAY_HANDLE_BIT | u64::from(index))
    }

    #[inline(always)]
    pub(crate) fn direct_function(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_FUNCTION_HANDLE_BITS | u64::from(index))
    }

    #[inline(always)]
    pub(crate) fn direct_iterator(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_ITERATOR_HANDLE_BITS | u64::from(index))
    }

    #[inline(always)]
    pub(crate) fn direct_string(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_STRING_HANDLE_BITS | u64::from(index))
    }

    #[inline(always)]
    pub(crate) fn direct_await(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_AWAIT_HANDLE_BITS | u64::from(index))
    }

    #[inline(always)]
    pub(crate) fn direct_v128(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_V128_HANDLE_BIT | u64::from(index))
    }

    #[inline(always)]
    pub(crate) fn direct_promise(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_PROMISE_HANDLE_BIT | u64::from(index))
    }

    /// The Number this value holds. Doubles are tested first: arithmetic that reaches this
    /// conversion has already handled the integer pair.
    #[inline(always)]
    pub fn as_number(self) -> Option<f64> {
        if !self.is_tagged() {
            Some(f64::from_bits(self.0))
        } else {
            self.as_int().map(f64::from)
        }
    }

    /// Both operands as Numbers.
    #[inline(always)]
    pub(crate) fn number_pair(left: Self, right: Self) -> Option<(f64, f64)> {
        Some((left.as_number()?, right.as_number()?))
    }

    pub fn as_bool(self) -> Option<bool> {
        ((self.0 & TAG_MASK) == TAG_BOOL).then_some(self.0 & 1 != 0)
    }

    #[inline(always)]
    pub(crate) fn heap_index(self) -> Option<u32> {
        ((self.0 & TAG_MASK) == TAG_HEAP).then_some((self.0 & PAYLOAD_MASK) as u32)
    }

    #[inline(always)]
    pub(crate) fn heap_reference(self) -> Option<HeapReference> {
        if self.0 & TAG_MASK != TAG_HEAP {
            return None;
        }
        let index = self.0 as u32;
        Some(
            match self.0 & HEAP_REFERENCE_BITS {
                0 => HeapReference::LegacySlot(index),
                DIRECT_OBJECT_HANDLE_BIT => HeapReference::ObjectRecord(index),
                DIRECT_ARRAY_HANDLE_BIT => HeapReference::ArrayRecord(index),
                DIRECT_FUNCTION_HANDLE_BITS => HeapReference::FunctionRecord(index),
                DIRECT_PROMISE_HANDLE_BIT => HeapReference::PromiseRecord(index),
                DIRECT_AWAIT_HANDLE_BITS => HeapReference::AwaitRecord(index),
                DIRECT_ITERATOR_HANDLE_BITS => HeapReference::IteratorRecord(index),
                DIRECT_STRING_HANDLE_BITS => HeapReference::StringRecord(index),
                DIRECT_V128_HANDLE_BIT => HeapReference::V128Record(index),
                _ => return None,
            },
        )
    }

    #[inline(always)]
    pub(crate) fn legacy_heap_index(self) -> Option<u32> {
        match self.heap_reference()? {
            HeapReference::LegacySlot(index) => Some(index),
            HeapReference::ObjectRecord(_)
            | HeapReference::ArrayRecord(_)
            | HeapReference::FunctionRecord(_)
            | HeapReference::PromiseRecord(_)
            | HeapReference::AwaitRecord(_)
            | HeapReference::IteratorRecord(_)
            | HeapReference::StringRecord(_)
            | HeapReference::V128Record(_) => None,
        }
    }

    #[inline(always)]
    pub(crate) fn direct_v128_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_V128_HANDLE_BIT).then_some(self.0 as u32)
    }

    #[inline(always)]
    pub(crate) fn direct_object_index(self) -> Option<u32> {
        // `heap_reference` as one family test: hot reads ask for this family alone.
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_OBJECT_HANDLE_BIT).then_some(self.0 as u32)
    }

    #[inline(always)]
    pub(crate) fn direct_array_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_ARRAY_HANDLE_BIT).then_some(self.0 as u32)
    }

    #[inline(always)]
    pub(crate) fn direct_iterator_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_ITERATOR_HANDLE_BITS).then_some(self.0 as u32)
    }

    #[inline(always)]
    pub(crate) fn direct_string_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_STRING_HANDLE_BITS).then_some(self.0 as u32)
    }

    #[inline(always)]
    pub(crate) fn direct_await_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_AWAIT_HANDLE_BITS).then_some(self.0 as u32)
    }

    #[inline(always)]
    pub(crate) fn direct_promise_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_PROMISE_HANDLE_BIT).then_some(self.0 as u32)
    }

    #[inline(always)]
    pub(crate) fn direct_function_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_FUNCTION_HANDLE_BITS).then_some(self.0 as u32)
    }

    /// Whether the value names a direct record of an object family: an ordinary object,
    /// array, function, promise or iterator. Every such value is an object; strings and
    /// await states, the other direct families, are not.
    #[inline(always)]
    pub(crate) fn is_direct_object_family(self) -> bool {
        self.direct_object_index().is_some()
            || self.direct_array_index().is_some()
            || self.direct_function_index().is_some()
            || self.direct_promise_index().is_some()
            || self.direct_iterator_index().is_some()
    }

    pub fn is_undefined(self) -> bool {
        self.0 == TAG_UNDEFINED
    }

    pub fn is_null(self) -> bool {
        self.0 == TAG_NULL
    }

    /// Whether the value names an object or array record, which holds only an ordinary
    /// object or an array: never a string, a BigInt or a function.
    #[inline(always)]
    pub(crate) fn is_record(self) -> bool {
        self.is_heap()
            && matches!(
                self.0 & HEAP_FAMILY_BITS,
                DIRECT_OBJECT_HANDLE_BIT | DIRECT_ARRAY_HANDLE_BIT
            )
    }

    /// ToNumber where the value alone decides it: a Number, a Boolean, `null` or `undefined`
    /// (the deleted marker reads as `undefined`). `None` for a heap value, whose conversion
    /// reads its cell and may run guest code.
    #[inline(always)]
    pub(crate) fn immediate_number(self) -> Option<f64> {
        if let Some(number) = self.as_number() {
            return Some(number);
        }
        if let Some(value) = self.as_bool() {
            return Some(if value { 1.0 } else { 0.0 });
        }
        if self.is_null() {
            return Some(0.0);
        }
        (self.is_undefined() || self.is_deleted()).then_some(f64::NAN)
    }

    /// ToBoolean where the value alone decides it: Numbers, the primitive tags, and object
    /// and array records, which hold only ordinary objects and arrays and so never "", 0n or
    /// the IsHTMLDDA function. `None` leaves another heap value to its cell.
    #[inline(always)]
    pub(crate) fn truthiness(self) -> Option<bool> {
        // A boolean first, in one test: conditions mostly test comparison results.
        if self.0 & !(Self::TRUE.0 ^ Self::FALSE.0) == Self::FALSE.0 {
            return Some(self == Self::TRUE);
        }
        if let Some(number) = self.as_number() {
            return Some(crate::vm::number_is_truthy(number));
        }
        if self.is_heap() {
            return self.is_record().then_some(true);
        }
        Some(!(self.is_null() || self.is_undefined() || self.is_deleted() || self == Self::FALSE))
    }

    pub fn is_heap(self) -> bool {
        (self.0 & TAG_MASK) == TAG_HEAP
    }

    pub(crate) fn is_deleted(self) -> bool {
        self.0 == TAG_DELETED
    }

    /// Whether this is any `TAG_DELETED` marker: `DELETED`, a shared environment slot, an
    /// environment index or unmaterialized arguments. None is a value a program observes.
    #[inline(always)]
    pub(crate) fn is_internal_marker(self) -> bool {
        self.0 & TAG_MASK == TAG_DELETED
    }

    #[inline(always)]
    fn is_tagged(self) -> bool {
        // The tag field, shifted down, compared as one small range.
        let field = (self.0 >> TAG_SHIFT) as u32;
        field.wrapping_sub((FIRST_TAG >> TAG_SHIFT) as u32)
            <= ((LAST_TAG - FIRST_TAG) >> TAG_SHIFT) as u32
    }

    #[cfg(feature = "profile-aggregate")]
    pub(crate) fn profile_kind(self) -> ProfileKind {
        if self.is_undefined() {
            ProfileKind::Undefined
        } else if self.is_null() {
            ProfileKind::Null
        } else if self.as_bool().is_some() {
            ProfileKind::Boolean
        } else if self.as_int().is_some() {
            ProfileKind::Integer
        } else if self.as_number().is_some() {
            ProfileKind::Double
        } else {
            ProfileKind::Heap
        }
    }
}

/// Magnitudes below 2^63 truncate exactly into an `i64`, whose low 32 bits are the value
/// modulo 2^32.
const EXACT_I64_TRUNCATION_LIMIT: f64 = 9_223_372_036_854_775_808.0;

/// ToUint32 (ToInt32 is its bits reinterpreted as `i32`).
#[inline]
pub(crate) fn number_to_u32(value: f64) -> u32 {
    if value.abs() < EXACT_I64_TRUNCATION_LIMIT {
        // NaN fails the comparison; -0 and small fractions truncate to 0.
        return value as i64 as u32;
    }
    if !value.is_finite() {
        return 0;
    }
    // At or above 2^63 the value is an integer: its significand times a power of two of
    // at least 2^11, whose low 32 bits are the significand's shifted, or none past 2^32.
    let bits = value.to_bits();
    let exponent = ((bits >> F64_FRACTION_BITS) & F64_EXPONENT_MASK) as i64
        - i64::from(F64_EXPONENT_BIAS)
        - i64::from(F64_FRACTION_BITS);
    let significand = (bits & F64_FRACTION_MASK) | (1 << F64_FRACTION_BITS);
    let low = if exponent >= i64::from(u32::BITS) { 0 } else { (significand << exponent) as u32 };
    if value.is_sign_negative() { low.wrapping_neg() } else { low }
}

/// The explicit fraction bits of a binary64.
const F64_FRACTION_BITS: u32 = f64::MANTISSA_DIGITS - 1;
const F64_FRACTION_MASK: u64 = (1 << F64_FRACTION_BITS) - 1;
/// The biased exponent field above the fraction, below the sign bit.
const F64_EXPONENT_MASK: u64 = (1 << (u64::BITS - 1 - F64_FRACTION_BITS)) - 1;
const F64_EXPONENT_BIAS: i32 = f64::MAX_EXP - 1;

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(number) = self.as_number() {
            write!(f, "{number}")
        } else if let Some(boolean) = self.as_bool() {
            write!(f, "{boolean}")
        } else if self.is_null() {
            f.write_str("null")
        } else if self.is_undefined() {
            f.write_str("undefined")
        } else {
            write!(f, "heap({})", self.heap_index().unwrap_or_default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Value;
    use std::mem::size_of;

    #[test]
    fn to_uint32_of_large_magnitudes_is_the_value_modulo_two_to_the_32() {
        const MODULUS: f64 = 4_294_967_296.0;
        let reference = |value: f64| value.trunc().rem_euclid(MODULUS) as u32;
        let mut bits: u64 = 0x2545_F491_4F6C_DD1D;
        for _ in 0..100_000 {
            bits ^= bits << 13;
            bits ^= bits >> 7;
            bits ^= bits << 17;
            let value = f64::from_bits(bits);
            if value.is_finite() && value.abs() >= 9_223_372_036_854_775_808.0 {
                assert_eq!(super::number_to_u32(value), reference(value), "{value:e}");
            }
        }
        for value in [9.223372036854776e18, -9.223372036854776e18, 1.8446744073709552e19, 3e19, -3e19, f64::MAX, -f64::MAX] {
            assert_eq!(super::number_to_u32(value), reference(value), "{value:e}");
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(super::number_to_u32(value), 0);
        }
    }

    #[test]
    fn narrow_values_round_trip_exactly() {
        let narrow = [
            Value::integer(0),
            Value::integer(-1),
            Value::integer((1 << 29) - 1),
            Value::integer(-(1 << 29)),
            Value::UNDEFINED,
            Value::NULL,
            Value::TRUE,
            Value::FALSE,
            Value::DELETED,
            Value::heap(7),
            Value::direct_object(0),
            Value::direct_object((1 << 25) - 1),
            Value::direct_string(3),
        ];
        for value in narrow {
            let packed = value.to_narrow().expect("narrow");
            assert_eq!(Value::from_narrow(packed), value);
            assert!(packed >> 30 == 0 || packed >> 30 == 3, "top bits copy each other");
        }
        let wide = [
            Value::integer(1 << 30),
            Value::integer(i32::MIN),
            Value::number(1.5),
            Value::number(-0.0),
            Value::direct_object(1 << 25),
            Value::heap(1 << 26),
        ];
        for value in wide {
            assert_eq!(value.to_narrow(), None, "{value:?}");
        }
    }

    #[test]
    fn value_is_one_machine_word() {
        assert_eq!(size_of::<Value>(), size_of::<usize>());
    }

    #[test]
    fn values_round_trip() {
        for value in [0.0, -0.0, 1.5, f64::INFINITY, f64::NAN] {
            let round = Value::number(value).as_number().unwrap();
            assert!(round == value || round.is_nan() && value.is_nan());
        }
        assert_eq!(Value::TRUE.as_bool(), Some(true));
        assert_eq!(Value::heap(42).heap_index(), Some(42));
        assert_eq!(
            Value::heap(42).heap_reference(),
            Some(super::HeapReference::LegacySlot(42))
        );
        let direct_object = Value::direct_object(42);
        assert_eq!(direct_object.heap_index(), Some(42));
        assert_eq!(direct_object.direct_object_index(), Some(42));
        assert_eq!(
            direct_object.heap_reference(),
            Some(super::HeapReference::ObjectRecord(42))
        );
        assert_ne!(direct_object, Value::heap(42));
        let direct_array = Value::direct_array(42);
        assert_eq!(direct_array.heap_index(), Some(42));
        assert_eq!(direct_array.direct_array_index(), Some(42));
        assert_eq!(
            direct_array.heap_reference(),
            Some(super::HeapReference::ArrayRecord(42))
        );
        assert_ne!(direct_array, direct_object);
        let direct_function = Value::direct_function(42);
        assert_eq!(direct_function.direct_function_index(), Some(42));
        assert_eq!(direct_function.direct_object_index(), None);
        assert_eq!(direct_function.direct_array_index(), None);
        assert_eq!(direct_function.legacy_heap_index(), None);
        assert_eq!(
            direct_function.heap_reference(),
            Some(super::HeapReference::FunctionRecord(42))
        );
        // A function may be the IsHTMLDDA object, so truthiness never answers for one.
        assert!(!direct_function.is_record());
        assert!(direct_object.is_record() && direct_array.is_record());
        let direct_promise = Value::direct_promise(42);
        assert_eq!(direct_promise.direct_promise_index(), Some(42));
        assert_eq!(direct_promise.direct_object_index(), None);
        assert_eq!(direct_promise.direct_function_index(), None);
        assert_eq!(
            direct_promise.heap_reference(),
            Some(super::HeapReference::PromiseRecord(42))
        );
        let direct_iterator = Value::direct_iterator(42);
        assert_eq!(direct_iterator.direct_iterator_index(), Some(42));
        assert_eq!(direct_iterator.direct_object_index(), None);
        assert_eq!(direct_iterator.direct_promise_index(), None);
        assert!(!direct_iterator.is_record());
        let direct_await = Value::direct_await(42);
        assert_eq!(direct_await.direct_await_index(), Some(42));
        assert_eq!(direct_await.direct_promise_index(), None);
        assert_eq!(direct_await.direct_array_index(), None);
        assert_eq!(
            direct_await.heap_reference(),
            Some(super::HeapReference::AwaitRecord(42))
        );
        assert_eq!(
            Value::int_pair(Value::integer(-2), Value::integer(3)),
            Some((-2, 3))
        );
        assert_eq!(Value::int_pair(Value::integer(1), Value::TRUE), None);
    }

    #[test]
    fn numbers_preserve_signed_zero_and_canonicalize_nan() {
        let negative_zero = Value::number(-0.0).as_number().unwrap();
        assert!(negative_zero == 0.0 && negative_zero.is_sign_negative());
        assert_eq!(
            Value::number(f64::NAN),
            Value::number(f64::from_bits(0x7ff0_0000_0000_0001))
        );
    }
}
