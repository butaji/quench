use std::fmt;

// The encoding core (tags, handle bits, narrow layout) is one file shared with the stencils.
pub use crate::staged::value_encoding::*;

#[cfg(all(test, feature = "bench-primitives"))]
#[path = "value/encoding_bench.rs"]
mod encoding_bench;
mod distinct_heap;
pub(crate) use distinct_heap::DistinctHeapPair;
mod number_conversion;
pub use number_conversion::{number_to_i32, number_to_u32, number_to_uint8_clamp};
pub(crate) use number_conversion::{exact_int32, exact_uint32};
#[quench_macros::test_visible]
pub(crate) use number_conversion::EXACT_I64_TRUNCATION_LIMIT;
use number_conversion::truncate_to_i32;
pub use crate::vm::operations::WORD32_SHIFT_MASK;

/// The table `NARROW_UPPER_WORDS` sits in at a fixed address, which compiled code indexes by code.
static NARROW_UPPER_WORD_TABLE: [u64; NARROW_LOOKUP_CODES] = NARROW_UPPER_WORDS;

/// The narrow-value facts compiled code decodes an inline property slot with (`NarrowLayout`'s
/// constants live in `staged::value_encoding`): the runtime's own table reads.
impl NarrowLayout {
    /// Only codes selected by the decoder belong in a native image.
    pub(crate) fn decode_words() -> &'static [u64] {
        &NARROW_UPPER_WORD_TABLE[..=Self::CODE_MASK as usize]
    }

    /// The address of the upper words by code, which lives as long as the process.
    #[inline(always)] pub fn upper_words() -> *const u64 {
        NARROW_UPPER_WORD_TABLE.as_ptr()
    }
}

impl Value {
    #[quench_macros::test_visible]
    /// This value as a narrow value, when it has one.
    #[inline(always)] pub(crate) fn to_narrow(self) -> Option<u32> {
        let upper = self.0 & !LOWER_WORD;
        let lower = self.0 as u32;
        if upper == TAG_INT {
            return Self::narrow_integer(lower as i32);
        }
        if self.0 == TAG_NULL {
            return Some(NULL_NARROW);
        }
        // The code of the value's tag plus its family bits; only an upper word the code names
        // exactly is narrow, which also rejects numbers and family bits on a constant.
        let tag = (self.0 >> TAG_SHIFT).wrapping_sub(FIRST_TAG >> TAG_SHIFT) as usize;
        let base = *NARROW_TAG_CODES.get(tag)?;
        let code = base + ((self.0 & HEAP_FAMILY_BITS) >> HEAP_FAMILY_SHIFT) as usize;
        (NARROW_UPPER_WORDS[code] == upper && lower >> NARROW_INDEX_BITS == 0)
            .then_some(lower << NARROW_INDEX_SHIFT | (code as u32) << NARROW_CODE_SHIFT)
    }

    /// The narrow form of the integer `integer`, when it has one (`to_narrow`).
    #[inline(always)] pub(crate) fn narrow_integer(integer: i32) -> Option<u32> {
        let limit = 1i32 << (NARROW_INTEGER_BITS - 1);
        (-limit..limit)
            .contains(&integer)
            .then_some(((integer as u32) << NARROW_CODE_SHIFT) | NARROW_INTEGER_FLAG)
    }

    /// `to_narrow`, testing a direct object handle first: a new object's values
    /// (`Object::pack_inline`) are mostly references, where field stores, which test an
    /// integer first, keep `to_narrow`.
    #[inline(always)] pub(crate) fn to_narrow_reference(self) -> Option<u32> {
        if self.0 & !LOWER_WORD != DIRECT_OBJECT_UPPER {
            // `null`, a list's end, the other common value of a new object's field, takes
            // `to_narrow`'s first test past the integer's.
            return self.to_narrow();
        }
        let lower = self.0 as u32;
        (lower >> NARROW_INDEX_BITS == 0).then_some(lower << NARROW_INDEX_SHIFT | DIRECT_OBJECT_NARROW_CODE << NARROW_CODE_SHIFT)
    }

    /// Whether `narrow` packs a heap reference (`is_heap`), read from its code alone: heap
    /// families take the codes below `NARROW_HEAP_FAMILIES`, constants the codes above.
    #[inline(always)] pub(crate) fn narrow_is_heap(narrow: u32) -> bool {
        narrow & NARROW_INTEGER_FLAG == 0
            && (narrow >> NARROW_CODE_SHIFT) & ((1 << NARROW_CODE_BITS) - 1) < NARROW_HEAP_FAMILIES as u32
    }

    #[quench_macros::test_visible]
    /// The value `narrow` packs (`to_narrow`).
    #[inline(always)] pub(crate) fn from_narrow(narrow: u32) -> Self {
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
    /// An environment (`Heap::alloc_environment`).
    EnvironmentRecord(u32),
    IteratorRecord(u32),
    StringRecord(u32),
    /// A Symbol's data (`Heap::alloc_symbol`).
    SymbolRecord(u32),
    /// A Map or a Set (`Heap::alloc_map`, `Heap::alloc_set`).
    CollectionRecord(u32),
    /// A Wasm GC struct or array (`Heap::alloc_wasm_gc`), whose head holds its payload.
    WasmGcRecord(u32),
}

/// An ordinary object reference: the low half of a direct object `Value`. Today it equals the
/// object's record id; after R4-B2b it is a class-space granule. Read the record id only
/// through `Heap::record_id` (Concrete) or `staged::object::object_record_of` (staged).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ObjectRef32(pub(crate) u32);

impl ObjectRef32 {
    /// The raw low half, for code that cannot reach the heap yet. Temporary: R4-B2b deletes it.
    #[inline(always)] pub const fn raw(self) -> u32 { self.0 }
    /// The stub granule this reference names: the identity an object link carries (`ProtoLink`).
    #[inline(always)] pub(crate) const fn granule(self) -> u32 { self.0 }
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
pub struct IntegerEncoding(u64);

impl IntegerEncoding {
    pub const TAG: Self = Self(TAG_INT);

    #[inline(always)] pub fn encode(self, value: i32) -> Value {
        Value(self.0 | u64::from(value as u32))
    }
}

/// Two operands classified by one pass over their tags: both int32, both Numbers (an int32 widened), or not both Numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumericPair { Ints(i32, i32), Numbers(f64, f64), Other }

impl NumericPair {
    /// Both operands as doubles: an int32 widened (exact), a double as it is. `None` for
    /// `Other`, whose operands are not both Numbers. Matches `number_pair` on the same operands.
    #[inline(always)] pub(crate) fn as_doubles(self) -> Option<(f64, f64)> {
        match self {
            Self::Ints(left, right) => Some((f64::from(left), f64::from(right))),
            Self::Numbers(left, right) => Some((left, right)),
            Self::Other => None,
        }
    }
}

impl Value {
    pub const UNDEFINED: Self = Self(TAG_UNDEFINED);
    pub const NULL: Self = Self(TAG_NULL);
    pub const FALSE: Self = Self(TAG_BOOL);
    pub const TRUE: Self = Self(TAG_TRUE);

    /// The Boolean `value`.
    #[inline(always)]
    pub const fn boolean(value: bool) -> Self {
        if value { Self::TRUE } else { Self::FALSE }
    }
    pub const DELETED: Self = Self(TAG_DELETED);
    /// The canonical NaN `number` stores every NaN as.
    pub(crate) const NAN: Self = Self(CANONICAL_NAN);
    /// An activation's `arguments` binding whose object does not exist yet
    /// (`CallPlan::forwards_arguments`). Not a heap reference, and distinct from every value a
    /// guest operation produces.
    pub const UNMATERIALIZED_ARGUMENTS: Self = Self(TAG_DELETED | 1);

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
    #[inline(always)] pub(crate) fn cmp_environment_index(self, index: u32) -> std::cmp::Ordering {
        debug_assert!(self.as_environment_index().is_some());
        self.0.cmp(&Self::environment_index(index).0)
    }

    /// The number of an `environment_index`.
    #[inline(always)] pub(crate) fn as_environment_index(self) -> Option<u32> {
        (self.0 & (TAG_MASK | ENVIRONMENT_INDEX_BIT) == TAG_DELETED | ENVIRONMENT_INDEX_BIT)
            .then_some(self.0 as u32)
    }

    /// A call-only closure of `function` over `environment`, made without a function object
    /// (`Function::prototype_unobservable` names the functions whose closure value only ever
    /// reaches a binding read as a callee): the environment's record reference with the
    /// function id in the payload bits between the record index and the reference family, which
    /// no other heap value sets. The collector reads it as the environment it names; a call
    /// enters `function` of the running program over that environment, and any other consumer
    /// first makes the real closure (`Vm::known_closure_function`). `None` when the
    /// value is not an environment or the id does not fit.
    #[inline(always)] pub fn known_closure(function: u32, environment: Self) -> Option<Self> {
        let fits = function != 0 && u64::from(function) <= KNOWN_CLOSURE_FUNCTION_MASK;
        (fits && environment.0 & (TAG_MASK | HEAP_REFERENCE_BITS | KNOWN_CLOSURE_FUNCTION_BITS) == TAG_HEAP | DIRECT_ENVIRONMENT_HANDLE_BITS)
            .then(|| Self(environment.0 | (u64::from(function) << KNOWN_CLOSURE_FUNCTION_SHIFT)))
    }

    /// The function id and environment of a `known_closure`.
    #[inline(always)] pub fn as_known_closure(self) -> Option<(u32, Self)> {
        let function = (self.0 >> KNOWN_CLOSURE_FUNCTION_SHIFT) & KNOWN_CLOSURE_FUNCTION_MASK;
        (self.0 & (TAG_MASK | HEAP_REFERENCE_BITS) == TAG_HEAP | DIRECT_ENVIRONMENT_HANDLE_BITS && function != 0)
            .then(|| (function as u32, Self(self.0 & !KNOWN_CLOSURE_FUNCTION_BITS)))
    }

    /// The owner environment of a `shared_environment_slot`.
    /// An environment slot word's edge for a collection, branch-free: the owner a shared
    /// slot names, else the word itself; and whether that edge is a heap value.
    #[inline(always)] pub(crate) fn environment_slot_edge(self) -> (Self, bool) {
        // A shared slot is `TAG_DELETED | SHARED_ENVIRONMENT_SLOT_BIT | payload`; its owner is
        // `TAG_HEAP | payload`, one xor away.
        const SHARED_PATTERN: u64 = TAG_DELETED | SHARED_ENVIRONMENT_SLOT_BIT;
        const TO_OWNER: u64 = TAG_DELETED ^ TAG_HEAP ^ SHARED_ENVIRONMENT_SLOT_BIT;
        let shared = self.0 & (TAG_MASK | SHARED_ENVIRONMENT_SLOT_BIT) == SHARED_PATTERN;
        let edge = self.0 ^ (TO_OWNER & (shared as u64).wrapping_neg());
        (Self(edge), edge & TAG_MASK == TAG_HEAP)
    }

    /// Whether this word is a shared environment slot (`shared_environment_slot`).
    #[inline(always)] pub(crate) fn is_shared_environment_slot(self) -> bool {
        self.0 & (TAG_MASK | SHARED_ENVIRONMENT_SLOT_BIT) == TAG_DELETED | SHARED_ENVIRONMENT_SLOT_BIT
    }
    #[inline(always)] pub(crate) fn shared_environment_owner(self) -> Option<Self> {
        (self.0 & (TAG_MASK | SHARED_ENVIRONMENT_SLOT_BIT) == TAG_DELETED | SHARED_ENVIRONMENT_SLOT_BIT)
            .then_some(Self(TAG_HEAP | (self.0 & PAYLOAD_MASK & !SHARED_ENVIRONMENT_SLOT_BIT)))
    }

    /// A Number: integral values in `i32` range other than -0 are stored as integers, NaN
    /// canonically. A value is integral exactly when its truncation converts back to the
    /// same bits, which also excludes -0 (+0's bits differ) and NaN.
    #[inline] pub fn number(value: f64) -> Self { Self::number_inline(value) }

    /// `number`, inlined where it is the hot path's own boxing (a typed or double element read). Its int32
    /// rule is `exact_int32`'s (truncation that converts back to the same bits), ordered for the boxing's
    /// branches: a fraction leaves first, then NaN, then `-0`.
    #[inline(always)] pub(crate) fn number_inline(value: f64) -> Self {
        let integer = truncate_to_i32(value);
        let back = f64::from(integer);
        // Ordered and unequal (`fcmp one`, one `ucomisd` and `jne`): a non-integral double,
        // never NaN, stored as its bits with no further test. Equal or unordered is left.
        if back < value || back > value {
            return Self(value.to_bits());
        }
        if value.is_nan() {
            std::hint::cold_path();
            return Self(CANONICAL_NAN);
        }
        // Integral; -0 (which equals +0) keeps its sign as a double.
        if integer != 0 || value.to_bits() == 0 {
            return Self::integer(integer);
        }
        Self(value.to_bits())
    }

    /// `number` without branches, for loops over many values: the same encoding, chosen by
    /// selects so that a column of mixed integral and fractional doubles costs the same per value.
    #[inline(always)]
    pub fn number_unpredictable(value: f64) -> Self {
        let integer = truncate_to_i32(value);
        // `&` and `|`, not `&&` and `||`: no branch between the tests.
        let integral = (f64::from(integer) == value) & ((integer != 0) | (value.to_bits() == 0));
        let double = std::hint::select_unpredictable(value.is_nan(), CANONICAL_NAN, value.to_bits());
        Self(std::hint::select_unpredictable(integral, Self::integer(integer).0, double))
    }

    /// `number` of a value an `i32` operation overflowed to: outside the `i32` integers and
    /// never NaN, so `number` stores its bits as they are; inline, where `number` is a call.
    #[inline(always)] pub(crate) fn overflowed_integer(value: f64) -> Self {
        debug_assert_eq!(Self::number(value).0, value.to_bits());
        Self(value.to_bits())
    }

    #[inline(always)] pub fn integer(value: i32) -> Self {
        IntegerEncoding::TAG.encode(value)
    }

    /// A count or index as a Number, as `number` stores it: an integer within `i32`, without
    /// the float round trip `number` makes to find that out.
    #[inline(always)] pub fn length(value: usize) -> Self {
        match i32::try_from(value) {
            Ok(value) => Self::integer(value),
            Err(_) => Self::long_length(value),
        }
    }

    /// `length` of a count past the `i32` integers, out of line so that `length` stays a
    /// tag `or` wherever it is mapped over an option.
    #[cold]
    #[inline(never)]
    fn long_length(value: usize) -> Self {
        Self::number(value as f64)
    }

    /// A 64-bit Wasm payload held in place. Every payload outside the tag
    /// space already reads as an untagged value, so only tag-space payloads
    /// need a heap box (`Cell::WasmBits64`); a static Wasm type tells the
    /// two encodings of a slot apart from its 32-bit integer encoding.
    #[inline(always)] pub fn from_bits64(bits: u64) -> Option<Self> {
        let value = Self(bits);
        (!value.is_tagged()).then_some(value)
    }

    /// The value's encoding, which a snapshot writes as is: it restores every heap index the
    /// value names at the same index (`crate::snapshot`).
    #[inline(always)] pub fn to_bits(self) -> u64 { self.0 }

    /// A value `to_bits` gave, read back from a snapshot of the same heap layout.
    #[inline(always)] pub fn from_bits(bits: u64) -> Self { Self(bits) }

    /// The 64-bit Wasm payload a value holds in place.
    #[inline(always)] pub fn bits64(self) -> Option<u64> {
        (!self.is_tagged()).then_some(self.0)
    }

    /// Whether the value is a Number stored as a double: no tag claims its bits.
    #[inline(always)] pub fn is_double(self) -> bool { !self.is_tagged() }

    #[inline(always)] pub fn as_int(self) -> Option<i32> {
        ((self.0 & TAG_MASK) == TAG_INT).then_some(self.0 as u32 as i32)
    }

    /// The bits of a slot whose Wasm type is statically f64, for an operator
    /// whose result depends on a NaN operand only through being a NaN (float
    /// arithmetic and comparison). A boxed payload's bits are a quiet NaN, so
    /// such an operator sees a NaN exactly where the payload is one.
    #[inline(always)] pub const fn float_bits64(self) -> u64 {
        self.0
    }

    /// A float arithmetic result held in place. A NaN whose bits fall in the
    /// tag space becomes the canonical NaN: arithmetic may produce any
    /// arithmetic NaN, and the canonical one is.
    #[inline(always)] pub fn from_float_bits64(bits: u64) -> Self {
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
    #[inline(always)] pub const fn wasm_bits32(self) -> u32 {
        self.0 as u32
    }

    /// The word's distance above the int32 tag, as an index: an int32 `k >= 0` gives `k`; a
    /// negative int32 gives at least 2^31; any other value gives at least 2^32 or wraps past
    /// 2^48, since no double's bits fall in the int32 tag's range (NaN is canonical). So one
    /// unsigned compare against a bound below 2^31 is the tag test, the sign test and the
    /// bounds test together.
    #[inline(always)] pub(crate) fn int_key_offset(self) -> usize {
        self.0.wrapping_sub(TAG_INT) as usize
    }

    #[inline(always)] pub fn int_pair(left: Self, right: Self) -> Option<(i32, i32)> {
        let tags = (left.0 ^ TAG_INT) | (right.0 ^ TAG_INT);
        (tags & TAG_MASK == 0).then_some((left.0 as u32 as i32, right.0 as u32 as i32))
    }

    #[quench_macros::test_visible]
    #[inline]
    pub(crate) fn heap(index: u32) -> Self {
        Self(TAG_HEAP | u64::from(index))
    }

    #[inline(always)] pub fn direct_object(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_OBJECT_HANDLE_BIT | u64::from(index))
    }

    #[inline(always)] pub(crate) fn direct_array(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_ARRAY_HANDLE_BIT | u64::from(index))
    }

    #[inline(always)] pub(crate) fn direct_function(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_FUNCTION_HANDLE_BITS | u64::from(index))
    }

    #[inline(always)] pub(crate) fn direct_iterator(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_ITERATOR_HANDLE_BITS | u64::from(index))
    }

    #[inline(always)] pub(crate) fn direct_collection(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_COLLECTION_HANDLE_BITS | u64::from(index))
    }

    #[quench_macros::test_visible]
    #[inline(always)] pub(crate) fn direct_string(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_STRING_HANDLE_BITS | u64::from(index))
    }

    #[inline(always)] pub(crate) fn direct_await(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_AWAIT_HANDLE_BITS | u64::from(index))
    }

    #[inline(always)] pub(crate) fn direct_v128(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_V128_HANDLE_BIT | u64::from(index))
    }

    #[inline(always)] pub(crate) fn direct_wasm_gc(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_WASM_GC_HANDLE_BITS | u64::from(index))
    }

    #[inline(always)] pub(crate) fn direct_promise(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_PROMISE_HANDLE_BIT | u64::from(index))
    }

    /// Whether the value is an int32, tested on the shifted tag field as `as_number` tests
    /// it, so that after an `as_number` of the same value LLVM reuses that test (a double's
    /// field already failed it) instead of masking the word again.
    #[inline(always)] pub(crate) fn has_int_tag_field(self) -> bool {
        (self.0 >> TAG_SHIFT) as u32 == (TAG_INT >> TAG_SHIFT) as u32
    }

    /// The Number this value holds. Doubles are tested first: arithmetic that reaches this
    /// conversion has already handled the integer pair.
    #[inline(always)]
    pub fn as_number(self) -> Option<f64> {
        // One shifted tag field for both tests: a mask-and-compare integer test beside the
        // range test let LLVM evaluate both with flag materializations and a select (14
        // instructions per operand) where a double needs four.
        let field = (self.0 >> TAG_SHIFT) as u32;
        if !self.is_tagged() {
            Some(f64::from_bits(self.0))
        } else if field == (TAG_INT >> TAG_SHIFT) as u32 {
            Some(f64::from(self.0 as u32 as i32))
        } else {
            None
        }
    }

    /// `as_number` without branches, for loops over many values: whether the value is a
    /// Number, whether it is an int32 (whose payload is its low 32 bits), and its bits as a
    /// double (the Number itself when it is a double).
    #[inline(always)]
    pub fn number_parts(self) -> (bool, bool, f64) {
        let int = self.has_int_tag_field();
        (int || !self.is_tagged(), int, f64::from_bits(self.0))
    }

    #[quench_macros::test_visible]
    /// Both operands as Numbers.
    #[inline(always)] pub(crate) fn number_pair(left: Self, right: Self) -> Option<(f64, f64)> {
        Some((left.as_number()?, right.as_number()?))
    }

    /// The operands' classification as `int_pair` and then `number_pair` would give it, with
    /// one tag test per operand: the combined int32 test first (`int_pair`), and only when
    /// it fails, each operand classified once by `number_parts`. An int32 operand's double
    /// is its integer value, the same `as_number` makes of it, so `Numbers` carries the
    /// bits `number_pair` returns.
    #[inline(always)] pub fn numeric_pair(left: Self, right: Self) -> NumericPair {
        if let Some((a, b)) = Self::int_pair(left, right) {
            return NumericPair::Ints(a, b);
        }
        let (left_number, left_int, left_bits) = left.number_parts();
        let (right_number, right_int, right_bits) = right.number_parts();
        if !(left_number && right_number) {
            return NumericPair::Other;
        }
        NumericPair::Numbers(
            if left_int { f64::from(left.0 as u32 as i32) } else { left_bits },
            if right_int { f64::from(right.0 as u32 as i32) } else { right_bits },
        )
    }

    #[inline] pub fn as_bool(self) -> Option<bool> {
        ((self.0 & TAG_MASK) == TAG_BOOL).then_some(self.0 & 1 != 0)
    }

    #[inline(always)] pub fn heap_index(self) -> Option<u32> {
        ((self.0 & TAG_MASK) == TAG_HEAP).then_some((self.0 & PAYLOAD_MASK) as u32)
    }

    #[inline(always)] pub(crate) fn heap_reference(self) -> Option<HeapReference> {
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
                DIRECT_ENVIRONMENT_HANDLE_BITS => HeapReference::EnvironmentRecord(index),
                DIRECT_SYMBOL_HANDLE_BITS => HeapReference::SymbolRecord(index),
                DIRECT_COLLECTION_HANDLE_BITS => HeapReference::CollectionRecord(index),
                DIRECT_WASM_GC_HANDLE_BITS => HeapReference::WasmGcRecord(index),
                _ => return None,
            },
        )
    }

    #[inline(always)] pub(crate) fn legacy_heap_index(self) -> Option<u32> {
        match self.heap_reference()? {
            HeapReference::LegacySlot(index) => Some(index),
            HeapReference::ObjectRecord(_)
            | HeapReference::ArrayRecord(_)
            | HeapReference::FunctionRecord(_)
            | HeapReference::PromiseRecord(_)
            | HeapReference::AwaitRecord(_)
            | HeapReference::IteratorRecord(_)
            | HeapReference::StringRecord(_)
            | HeapReference::V128Record(_)
            | HeapReference::EnvironmentRecord(_)
            | HeapReference::SymbolRecord(_)
            | HeapReference::WasmGcRecord(_)
            | HeapReference::CollectionRecord(_) => None,
        }
    }

    #[inline(always)] pub(crate) fn direct_environment(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_ENVIRONMENT_HANDLE_BITS | u64::from(index))
    }

    #[inline(always)] pub(crate) fn direct_environment_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_ENVIRONMENT_HANDLE_BITS).then_some(self.0 as u32)
    }

    #[inline(always)] pub(crate) fn direct_symbol(index: u32) -> Self {
        Self(TAG_HEAP | DIRECT_SYMBOL_HANDLE_BITS | u64::from(index))
    }

    #[inline(always)] pub(crate) fn direct_symbol_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_SYMBOL_HANDLE_BITS).then_some(self.0 as u32)
    }

    #[inline(always)] pub(crate) fn direct_v128_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_V128_HANDLE_BIT).then_some(self.0 as u32)
    }

    #[inline(always)] pub(crate) fn direct_wasm_gc_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_WASM_GC_HANDLE_BITS).then_some(self.0 as u32)
    }

    #[inline(always)] pub fn direct_object_index(self) -> Option<ObjectRef32> {
        // `heap_reference` as one family test: hot reads ask for this family alone.
        crate::staged::value::is_direct_object(&mut crate::staged::Concrete::pure(), self.0).then_some(ObjectRef32(self.0 as u32))
    }

    #[inline(always)] pub(crate) fn direct_array_index(self) -> Option<u32> {
        crate::staged::value::is_direct_array(&mut crate::staged::Concrete::pure(), self.0).then_some(self.0 as u32)
    }

    #[inline(always)] pub(crate) fn direct_iterator_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_ITERATOR_HANDLE_BITS).then_some(self.0 as u32)
    }

    #[inline(always)] pub(crate) fn direct_collection_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_COLLECTION_HANDLE_BITS).then_some(self.0 as u32)
    }

    #[inline(always)] pub fn direct_string_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_STRING_HANDLE_BITS).then_some(self.0 as u32)
    }

    #[inline(always)] pub(crate) fn direct_await_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_AWAIT_HANDLE_BITS).then_some(self.0 as u32)
    }

    #[inline(always)] pub(crate) fn direct_promise_index(self) -> Option<u32> {
        (self.0 & HEAP_FAMILY_MASK == TAG_HEAP | DIRECT_PROMISE_HANDLE_BIT).then_some(self.0 as u32)
    }

    #[inline(always)] pub(crate) fn direct_function_index(self) -> Option<u32> {
        crate::staged::value::is_direct_function(&mut crate::staged::Concrete::pure(), self.0).then_some(self.0 as u32)
    }

    /// Whether the value names a direct record of an object family: an ordinary object,
    /// array, function, promise, iterator or collection. Every such value is an object; strings
    /// and await states, the other direct families, are not.
    #[inline(always)] pub(crate) fn is_direct_object_family(self) -> bool {
        self.direct_object_index().is_some()
            || self.direct_array_index().is_some()
            || self.direct_function_index().is_some()
            || self.direct_promise_index().is_some()
            || self.direct_iterator_index().is_some()
            || self.direct_collection_index().is_some()
    }

    #[inline] pub fn is_undefined(self) -> bool {
        self.0 == TAG_UNDEFINED
    }

    #[inline] pub fn is_null(self) -> bool {
        self.0 == TAG_NULL
    }

    /// ToNumber where the value alone decides it: a Number, a Boolean, `null` or `undefined`
    /// (the deleted marker reads as `undefined`). `None` for a heap value, whose conversion
    /// reads its cell and may run guest code.
    #[inline(always)] pub(crate) fn immediate_number(self) -> Option<f64> {
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
    #[inline(always)] pub fn truthiness(self) -> Option<bool> {
        // The common jump operands first, each one compare and no decode: the two booleans, then
        // an int32 (its low word is nonzero). Everything else takes the general test.
        if self.0 == Self::TRUE.0 { return Some(true); }
        if self.0 == Self::FALSE.0 { return Some(false); }
        if (self.0 & TAG_MASK) == TAG_INT { return Some(self.0 as u32 != 0); }
        let (undecided, truthy) = crate::staged::value::tagged_truthiness(
            &mut crate::staged::Concrete::pure(),
            self.0,
        );
        (!undecided).then_some(truthy)
    }

    #[inline(always)] pub fn is_heap(self) -> bool {
        (self.0 & TAG_MASK) == TAG_HEAP
    }

    /// Appends the heap values of `values` to `out`, in order: a branch-free compaction that
    /// writes every value into reserved space and advances past the heap ones only.
    #[inline(always)] pub(crate) fn extend_heap_values(out: &mut Vec<Value>, values: &[Value]) {
        out.reserve(values.len());
        let start = out.len();
        let mut kept = 0;
        for &value in values {
            // SAFETY: `kept` never passes the values read so far, so every write lands
            // within the `values.len()` slots reserved past `start`.
            unsafe { out.as_mut_ptr().add(start + kept).write(value) };
            kept += usize::from(value.is_heap());
        }
        // SAFETY: the first `kept` slots past `start` were written above.
        unsafe { out.set_len(start + kept) };
    }

    #[inline] pub fn is_deleted(self) -> bool {
        self.0 == TAG_DELETED
    }

    /// Whether this is any `TAG_DELETED` marker: `DELETED`, a shared environment slot, an
    /// environment index or unmaterialized arguments. None is a value a program observes.
    #[inline(always)] pub fn is_internal_marker(self) -> bool {
        self.0 & TAG_MASK == TAG_DELETED
    }

    #[inline(always)] pub(crate) fn is_tagged(self) -> bool { Self::tagged_field((self.0 >> TAG_SHIFT) as u32) }

    /// Whether a tag field (`bits >> TAG_SHIFT`) lies in the tag range: one small range test.
    #[inline(always)] fn tagged_field(field: u32) -> bool {
        field.wrapping_sub((FIRST_TAG >> TAG_SHIFT) as u32) <= ((LAST_TAG - FIRST_TAG) >> TAG_SHIFT) as u32
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
    fn a_reference_narrowing_of_null_is_the_general_one() {
        assert_eq!(Value::NULL.to_narrow_reference(), Value::NULL.to_narrow());
        assert_eq!(Value::from_narrow(Value::NULL.to_narrow_reference().unwrap()), Value::NULL);
    }

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
        assert_eq!(direct_object.direct_object_index(), Some(super::ObjectRef32(42)));
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
        let direct_symbol = Value::direct_symbol(42);
        assert_eq!(direct_symbol.direct_symbol_index(), Some(42));
        assert_eq!(direct_symbol.direct_object_index(), None);
        assert_eq!(direct_symbol.direct_v128_index(), None);
        assert_eq!(direct_symbol.legacy_heap_index(), None);
        assert_eq!(direct_symbol.to_narrow(), None);
        assert!(!direct_symbol.is_record());
        assert_eq!(
            direct_symbol.heap_reference(),
            Some(super::HeapReference::SymbolRecord(42))
        );
        let direct_collection = Value::direct_collection(43);
        assert_eq!(direct_collection.direct_collection_index(), Some(43));
        assert_eq!(direct_collection.direct_iterator_index(), None);
        assert!(direct_collection.is_direct_object_family());
        assert_eq!(direct_collection.to_narrow(), None);
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

    #[test]
    fn length_encoding_matches_number_at_representation_boundaries() {
        fn check(value: usize) {
            let encoded = Value::length(value);
            assert_eq!(encoded, Value::number(value as f64));
            assert_eq!(
                encoded.as_number().unwrap().to_bits(),
                (value as f64).to_bits(),
            );
            assert_eq!(encoded.as_int().is_some(), i32::try_from(value).is_ok());
        }
        for value in [
            0,
            1,
            i32::MAX as usize,
            i32::MAX as usize + 1,
            u32::MAX as usize,
            usize::MAX,
        ] {
            check(value);
        }
        for exponent in 1..usize::BITS {
            let power = 1_usize << exponent;
            for value in [power - 1, power, power + 1] {
                check(value);
            }
        }
        const SAMPLE_COUNT: usize = 100_000;
        const SAMPLE_STEP: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut value = 0_u64;
        for _ in 0..SAMPLE_COUNT {
            value = value.wrapping_add(SAMPLE_STEP);
            check(value as usize);
            check(value as u32 as usize);
        }
    }

    #[test]
    fn number_encoding_preserves_arbitrary_float_bits_and_integer_boundaries() {
        fn check(number: f64) {
            let encoded = Value::number(number);
            let decoded = encoded.as_number().unwrap();
            if number.is_nan() {
                assert_eq!(encoded.to_bits(), super::CANONICAL_NAN);
            } else {
                assert_eq!(decoded.to_bits(), number.to_bits());
                let integer = number.fract() == 0.0
                    && number >= f64::from(i32::MIN)
                    && number <= f64::from(i32::MAX)
                    && !(number == 0.0 && number.is_sign_negative());
                assert_eq!(encoded.as_int().is_some(), integer, "{number:?}");
            }
        }
        for number in [
            0.0,
            -0.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            -f64::from_bits(1),
            f64::MAX,
            -f64::MAX,
        ] {
            check(number);
        }
        for exponent in 0..=f64::MANTISSA_DIGITS {
            let power = 2_f64.powi(exponent as i32);
            for sign in [-1.0, 1.0] {
                for offset in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                    let number = sign * power + offset;
                    check(number);
                    check(number.next_up());
                    check(number.next_down());
                }
            }
        }
        const SAMPLE_COUNT: usize = 100_000;
        // An odd Weyl step covers the whole word space before repeating; bound the
        // sample count so representation checks stay cheap in the regular suite.
        const SAMPLE_STEP: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut bits = 0_u64;
        for _ in 0..SAMPLE_COUNT {
            bits = bits.wrapping_add(SAMPLE_STEP);
            check(f64::from_bits(bits));
            check(f64::from(bits as i32));
        }
    }
}
