//! Value-word reads every tier shares: which heap family a value names.

use super::machine::{Binary, Lane, MachineScalar, Relation, Unary};
use super::region::Region;
use super::Stage;
use crate::value::ValueLayout;

/// The family field is the word's top bits, so a family test is one shift and a compare with
/// a small immediate, with no 64-bit mask or tag to load.
const FAMILY_SHIFT: u32 = ValueLayout::HEAP_FAMILY_MASK.trailing_zeros();
const _: () = assert!(ValueLayout::HEAP_FAMILY_MASK == u64::MAX << FAMILY_SHIFT);

#[inline(always)]
fn family_is<S: Stage>(stage: &mut S, bits: S::Word, tag: u64) -> S::Cond {
    let family = stage.shr_imm(bits, FAMILY_SHIFT);
    stage.eq_imm(family, tag >> FAMILY_SHIFT)
}

/// Whether `bits` is a direct object reference, whose low half names its record
/// (`Value::as_direct_object`).
#[inline(always)]
pub(crate) fn is_direct_object<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::DIRECT_OBJECT_TAG)
}

/// The id of the object record a direct object reference names, its low half (`Value::direct_object_index`).
#[inline(always)]
pub(crate) fn direct_object_id<S: Stage>(stage: &mut S, bits: S::Word) -> S::Word {
    stage.and_imm(bits, u64::from(u32::MAX))
}

/// The immediate operations a tag test is written in. A `Stage` is one (its words and conditions);
/// a `MachineScalar` is one through `MachineTags`, whose comparisons are 0-or-1 flag words. Each tag
/// test is written once over this, so a layout change is one definition for every stage and machine.
pub(crate) trait TagOps {
    type Word: Copy;
    /// A comparison's result: a condition at a stage, a flag word at a machine.
    type Flag: Copy;
    fn and_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn shr_imm(&mut self, a: Self::Word, shift: u64) -> Self::Word;
    fn add_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn eq_imm(&mut self, a: Self::Word, imm: u64) -> Self::Flag;
    fn ugt_imm(&mut self, a: Self::Word, imm: u64) -> Self::Flag;
}

/// Every stage is a `TagOps` through its own immediate operations: a `u64` operand converts to the
/// stage's `Imm` (`From<u64>`), so the stage receives the same constant it received before.
impl<S: Stage> TagOps for S {
    type Word = S::Word;
    type Flag = S::Cond;
    #[inline(always)]
    fn and_imm(&mut self, a: S::Word, imm: u64) -> S::Word {
        Stage::and_imm(self, a, imm)
    }
    #[inline(always)]
    fn shr_imm(&mut self, a: S::Word, shift: u64) -> S::Word {
        Stage::shr_imm(self, a, shift)
    }
    #[inline(always)]
    fn add_imm(&mut self, a: S::Word, imm: u64) -> S::Word {
        Stage::add_imm(self, a, imm)
    }
    #[inline(always)]
    fn eq_imm(&mut self, a: S::Word, imm: u64) -> S::Cond {
        Stage::eq_imm(self, a, imm)
    }
    #[inline(always)]
    fn ugt_imm(&mut self, a: S::Word, imm: u64) -> S::Cond {
        Stage::ugt_imm(self, a, imm)
    }
}

/// A `MachineScalar` read as a `TagOps`: its words are its words, and a comparison is its flag word.
/// The immediate is the `i64` with the same bits as the `u64` operand.
struct MachineTags<'a, M>(&'a mut M);

impl<M: MachineScalar> TagOps for MachineTags<'_, M> {
    type Word = M::Word;
    type Flag = M::Word;
    #[inline(always)]
    fn and_imm(&mut self, a: M::Word, imm: u64) -> M::Word {
        self.0.raw_binary_imm(Binary::And, a, imm as i64)
    }
    #[inline(always)]
    fn shr_imm(&mut self, a: M::Word, shift: u64) -> M::Word {
        self.0.raw_binary_imm(Binary::ShiftRightUnsigned, a, shift as i64)
    }
    #[inline(always)]
    fn add_imm(&mut self, a: M::Word, imm: u64) -> M::Word {
        self.0.raw_binary_imm(Binary::Add, a, imm as i64)
    }
    #[inline(always)]
    fn eq_imm(&mut self, a: M::Word, imm: u64) -> M::Word {
        self.0.raw_compare_imm(Relation::Equal, a, imm as i64)
    }
    #[inline(always)]
    fn ugt_imm(&mut self, a: M::Word, imm: u64) -> M::Word {
        self.0.raw_compare_imm(Relation::UnsignedGreater, a, imm as i64)
    }
}

/// Whether `bits` is an int32 (`Value::as_int`); an int32 has one tagged form, so two
/// distinct int-tagged words are two distinct values. At a machine it is a flag word.
#[inline(always)]
pub(crate) fn is_int<T: TagOps>(ops: &mut T, bits: T::Word) -> T::Flag {
    let tag = ops.and_imm(bits, ValueLayout::TAG_MASK);
    ops.eq_imm(tag, ValueLayout::INT_TAG)
}

/// Whether `bits` is a heap reference (`Value::is_heap`).
#[inline(always)]
pub(crate) fn is_heap<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    stage.eq_imm(tag, ValueLayout::HEAP_TAG)
}

/// The language bits of a declared parameter's mask (`quench_bjit::shared_words::language`, the
/// order of `ValueType`'s discriminants; the runtime's `vm/compiled/base_region/declared.rs` asserts
/// they are these types' bits).
pub(crate) mod language {
    pub(crate) const UNDEFINED: u16 = 1 << 0;
    pub(crate) const NULL: u16 = 1 << 1;
    pub(crate) const BOOLEAN: u16 = 1 << 2;
    pub(crate) const NUMBER: u16 = 1 << 3;
    pub(crate) const STRING: u16 = 1 << 4;
    pub(crate) const OBJECT: u16 = 1 << 7;
    pub(crate) const FUNCTION: u16 = 1 << 8;
    pub(crate) const ARRAY: u16 = 1 << 9;
    pub(crate) const OBJECT_PLAIN: u16 = 1 << 10;
}

/// The language bits (`language`) a word's tag alone sets: a Number (an int32 or a double), a Boolean,
/// `null`, `undefined`, a direct object family record (`OBJECT`, and `OBJECT_PLAIN` for a plain one), a
/// direct array (`ARRAY`, with the `OBJECT` bit, as the family test the object family makes), a direct
/// function (`FUNCTION`), a direct string (`STRING`, the exact family test `is_direct_string` makes). A heap value
/// no tag decides (a rope or heap string, a symbol, a BigInt) sets none.
#[inline(always)]
pub(crate) fn tag_languages<S: Stage>(stage: &mut S, bits: S::Word) -> S::Word {
    let plain = family_is(stage, bits, ValueLayout::DIRECT_OBJECT_TAG);
    let array = family_is(stage, bits, ValueLayout::DIRECT_ARRAY_TAG);
    let function = family_is(stage, bits, ValueLayout::DIRECT_FUNCTION_TAG);
    let string = is_direct_string(stage, bits);
    let mut object = stage.any(plain, array);
    object = stage.any(object, function);
    for tag in [ValueLayout::DIRECT_PROMISE_TAG, ValueLayout::DIRECT_ITERATOR_TAG] {
        let family = family_is(stage, bits, tag);
        object = stage.any(object, family);
    }
    let int = is_int(stage, bits);
    let double = is_double(stage, bits);
    let number = stage.any(int, double);
    let unflagged = stage.and_imm(bits, !(crate::Value::TRUE.to_bits() ^ crate::Value::FALSE.to_bits()));
    let boolean = stage.eq_imm(unflagged, crate::Value::FALSE.to_bits());
    let null = stage.eq_imm(bits, crate::Value::NULL.to_bits());
    let undefined = stage.eq_imm(bits, crate::Value::UNDEFINED.to_bits());
    let plain = family_is(stage, bits, ValueLayout::DIRECT_OBJECT_TAG);
    let array = family_is(stage, bits, ValueLayout::DIRECT_ARRAY_TAG);
    let function = family_is(stage, bits, ValueLayout::DIRECT_FUNCTION_TAG);
    let string = is_direct_string(stage, bits);
    let classes = [(number, language::NUMBER), (boolean, language::BOOLEAN), (null, language::NULL), (undefined, language::UNDEFINED),
        (object, language::OBJECT), (plain, language::OBJECT_PLAIN), (array, language::ARRAY), (function, language::FUNCTION),
        (string, language::STRING)];
    let mut languages = stage.constant(0);
    for (hit, bit) in classes {
        let flag = stage.constant(u64::from(bit));
        let none = stage.constant(0);
        let added = stage.select(hit, flag, none);
        languages = stage.or(languages, added);
    }
    languages
}

/// Whether `bits` is `true` or `false` (`Value::as_bool`).
#[inline(always)]
pub(crate) fn is_boolean<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let (truthy, falsy) = (stage.eq_imm(bits, crate::Value::TRUE.to_bits()), stage.eq_imm(bits, crate::Value::FALSE.to_bits()));
    stage.any(truthy, falsy)
}

/// Whether `bits` is `undefined`.
#[inline(always)]
pub(crate) fn is_undefined<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    stage.eq_imm(bits, crate::Value::UNDEFINED.to_bits())
}

/// Whether `bits` is `null`.
#[inline(always)]
pub(crate) fn is_null<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    stage.eq_imm(bits, crate::Value::NULL.to_bits())
}

/// The value word of an int32 whose 64-bit `payload` is its zero-extended bits (`Value::int`).
#[inline(always)]
pub(crate) fn int_word<S: Stage>(stage: &mut S, payload: S::Word) -> S::Word {
    stage.or_imm(payload, ValueLayout::INT_TAG)
}

/// Whether `bits` is a direct string reference (`Value::as_direct_string`).
#[inline(always)]
pub(crate) fn is_direct_string<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let family = stage.and_imm(bits, ValueLayout::HEAP_FAMILY_MASK);
    stage.eq_imm(family, ValueLayout::DIRECT_STRING_TAG)
}

/// The int32 value word of a `length`: `None` (leaving at `miss`) past int32, which
/// `Value::length` holds as a double.
#[inline(always)]
pub(crate) fn length_value<S: Stage>(stage: &mut S, length: S::Word, miss: S::Miss) -> Option<S::Word> {
    let beyond = stage.ugt_imm(length, i32::MAX as u64);
    let within = stage.not(beyond);
    stage.guard(within, miss)?;
    Some(stage.or_imm(length, ValueLayout::INT_TAG))
}

/// Whether `bits` is a direct array reference (`Value::as_direct_array`).
#[inline(always)]
pub(crate) fn is_direct_array<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::DIRECT_ARRAY_TAG)
}

/// Whether `bits` is a legacy slot reference (`Value::legacy_heap_index`): a heap value of no
/// record family, whose low half is its slot.
#[inline(always)]
pub(crate) fn is_legacy_slot<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::HEAP_TAG)
}

/// Whether `bits` is a direct function reference (`Value::as_direct_function`).
#[inline(always)]
pub(crate) fn is_direct_function<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::DIRECT_FUNCTION_TAG)
}

/// Whether `bits` is a binary64 Number: past the span of the tag fields the immediates use. At a
/// machine it is a flag word.
#[inline(always)]
pub(crate) fn is_double<T: TagOps>(ops: &mut T, bits: T::Word) -> T::Flag {
    let field = ops.shr_imm(bits, u64::from(ValueLayout::TAG_SHIFT));
    let offset = ops.add_imm(field, ValueLayout::FIRST_TAG_FIELD.wrapping_neg());
    ops.ugt_imm(offset, ValueLayout::TAG_FIELD_SPAN)
}

/// The states `tagged_truthiness` sorts a word into: undecided by the word, falsy, or truthy.
const UNDECIDED: u64 = 0;
const FALSY: u64 = 1;
const TRUTHY: u64 = 2;

/// ToBoolean of `bits` as far as its own word decides it: `(undecided, truthy)`. A heap value
/// other than an object or array record is undecided, its cell deciding (a string's length, a
/// BigInt, the IsHTMLDDA object); `truthy` holds for any other truthy value: a record, a nonzero
/// int32 or Number, or an immediate but `null`, `undefined`, `false` and the deleted marker.
/// `Value::truthiness` is this at `Concrete`, so every stage decides the same cases.
#[inline(always)]
pub(crate) fn tagged_truthiness<S: Stage>(stage: &mut S, bits: S::Word) -> (S::Cond, S::Cond) {
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
    // The word's truth state: a heap word is decided by its record's test alone (its record is truthy,
    // else undecided), so the record tests and the immediate and Number ladder below run only in their own
    // arm. The arms compute the same truth as before.
    let state = stage.choose(heap, #[inline(always)] |stage| {
        let (object, array) = (is_direct_object(stage, bits), is_direct_array(stage, bits));
        let record = stage.any(object, array);
        let (truthy, undecided) = (stage.constant(TRUTHY), stage.constant(UNDECIDED));
        stage.select(record, truthy, undecided)
    }, #[inline(always)] |stage| {
        let integer = stage.eq_imm(tag, ValueLayout::INT_TAG);
        let payload = stage.and_imm(bits, u64::from(u32::MAX));
        let integer_truthy = stage.ne_imm(payload, 0u32);
        let past = is_double(stage, bits);
        let tagged = stage.not(past);
        // Written out, not an iterator fold: a stencil inlines every step (no call to `reduce` or `expect`).
        let null = stage.eq_imm(bits, crate::Value::NULL.to_bits());
        let undefined = stage.eq_imm(bits, crate::Value::UNDEFINED.to_bits());
        let deleted = stage.eq_imm(bits, crate::Value::DELETED.to_bits());
        let false_word = stage.eq_imm(bits, crate::Value::FALSE.to_bits());
        let nullish = stage.any(null, undefined);
        let absent = stage.any(deleted, false_word);
        let falsy = stage.any(nullish, absent);
        let immediate_truthy = stage.not(falsy);
        let double_truthy = stage.number_truthy(bits);
        let tagged_truthy = stage.pick(integer, integer_truthy, immediate_truthy);
        let unheaped_truthy = stage.pick(tagged, tagged_truthy, double_truthy);
        let (yes, no) = (stage.constant(TRUTHY), stage.constant(FALSY));
        stage.select(unheaped_truthy, yes, no)
    });
    (stage.eq_imm(state, UNDECIDED), stage.eq_imm(state, TRUTHY))
}

/// ToBoolean of a Boolean's `bits`: `true` alone is truthy.
#[inline(always)]
pub(crate) fn boolean_truthy<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    stage.eq_imm(bits, crate::Value::TRUE.to_bits())
}

/// ToBoolean of an integer word (an int32 or a 64-bit integer, as held): nonzero.
#[inline(always)]
pub(crate) fn int_truthy<S: Stage>(stage: &mut S, word: S::Word) -> S::Cond {
    stage.ne_imm(word, 0u32)
}

#[cfg(not(stencil_build))]
use crate::bytecode::typeof_name_index as name_index;
// The stencil build mounts the same self-contained file as `crate::bytecode`'s `typeof_names`.
#[cfg(stencil_build)]
#[path = "../bytecode/typeof_names.rs"]
mod typeof_names;
#[cfg(stencil_build)]
use typeof_names::typeof_name_index as name_index;

/// The index in `bytecode::TYPEOF_NAMES` of a name some value's `typeof` is.
const fn typeof_name_index(name: &str) -> u64 {
    match name_index(name) {
        Some(index) => index as u64,
        None => panic!("a typeof name"),
    }
}

/// The indexes in `bytecode::TYPEOF_NAMES` of the names a value's tag alone decides: `typeof` of
/// `undefined`, of `null` (an object) and a boolean and a Number.
pub(crate) const TYPEOF_UNDEFINED: u64 = typeof_name_index("undefined");
pub(crate) const TYPEOF_OBJECT: u64 = typeof_name_index("object");
pub(crate) const TYPEOF_BOOLEAN: u64 = typeof_name_index("boolean");
pub(crate) const TYPEOF_NUMBER: u64 = typeof_name_index("number");

/// The index in `bytecode::TYPEOF_NAMES` of the one name `typeof` yields for every value of the language
/// set `languages` (the `language` bits), where the set decides one: a set of functions is `function`, a
/// set of Numbers is `number`, and a set of direct objects and arrays (or none) is `object`. A set that
/// may name either (a generic object, or a mix of kinds) has none, so a fold built on it leaves at its test.
pub(crate) const fn language_typeof_index(languages: u16) -> Option<u64> {
    if languages == language::FUNCTION {
        Some(typeof_name_index("function"))
    } else if languages == language::NUMBER {
        Some(TYPEOF_NUMBER)
    } else if languages & !(language::OBJECT_PLAIN | language::ARRAY) == 0 {
        Some(TYPEOF_OBJECT)
    } else {
        None
    }
}

/// The tag classes `typeof` reads from a value's own word: a Number (int32 or double), a boolean,
/// and `null`. Every `typeof` row selects its name from these.
#[inline(always)]
pub(crate) fn typeof_classes<S: Stage>(stage: &mut S, bits: S::Word) -> (S::Cond, S::Cond, S::Cond) {
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    let integer = stage.eq_imm(tag, ValueLayout::INT_TAG);
    let double = is_double(stage, bits);
    let number = stage.any(integer, double);
    let boolean = stage.eq_imm(tag, crate::Value::TRUE.to_bits() & ValueLayout::TAG_MASK);
    let null = stage.eq_imm(bits, crate::Value::NULL.to_bits());
    (number, boolean, null)
}

/// The kind a heap value's `typeof` name follows from its cell (a string, a BigInt, a symbol, an
/// object or a function), as its index in `bytecode::TYPEOF_NAMES`: what `Frame::type_of` proves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct TypeofKind(u64);

impl TypeofKind {
    /// The kind whose `typeof` name is `bytecode::TYPEOF_NAMES[index]`.
    pub(crate) const fn of_index(index: u64) -> Self {
        Self(index)
    }

    /// The index in `bytecode::TYPEOF_NAMES` of this kind's name.
    pub(crate) const fn index(self) -> u64 {
        self.0
    }
}

/// The index in `bytecode::TYPEOF_NAMES` of `typeof` of the value `bits` where its tag alone decides
/// it (`undefined` and the deleted marker, `null`, a boolean, an int32 or a double): a heap value,
/// whose cell decides (a string, BigInt, symbol, object or function), or an internal marker leaves
/// at `miss`.
#[inline(always)]
pub(crate) fn immediate_typeof_index<S: Stage>(stage: &mut S, bits: S::Word, miss: S::Miss) -> Option<S::Word> {
    let (number, boolean, null) = typeof_classes(stage, bits);
    let unset = [crate::Value::UNDEFINED, crate::Value::DELETED].map(|value| stage.eq_imm(bits, value.to_bits()));
    let decided = [number, boolean, null, unset[0], unset[1]].into_iter().reduce(|any, each| stage.any(any, each)).expect("five tag classes");
    stage.guard(decided, miss)?;
    let [undefined, object, boolean_name, number_name] = [TYPEOF_UNDEFINED, TYPEOF_OBJECT, TYPEOF_BOOLEAN, TYPEOF_NUMBER].map(|index| stage.constant(index));
    let word = stage.select(boolean, boolean_name, undefined);
    let word = stage.select(null, object, word);
    Some(stage.select(number, number_name, word))
}

/// The `typeof` result string of the value `bits` where its tag alone decides it: the entry of `names`,
/// the address of the table of the eight result strings (`Vm::typeof_names`, interned when the heap is
/// made and indexed as `bytecode::TYPEOF_NAMES`), at the value's index; a heap value leaves at `miss`.
#[inline(always)]
pub(crate) fn immediate_typeof_string<S: Stage>(stage: &mut S, bits: S::Word, names: S::Word, miss: S::Miss) -> Option<S::Word> {
    let index = immediate_typeof_index(stage, bits, miss)?;
    let offset = stage.mul_imm(index, core::mem::size_of::<crate::Value>() as u64);
    let entry = stage.add(names, offset);
    Some(stage.load_table(Region::HeapMeta.readonly(), entry))
}

/// The binary64 value of the int32 whose tagged word is `bits`: its low half, sign-extended and
/// converted (`Value::as_int` read as a Number). Both int32 unboxing rows share it.
#[inline(always)]
pub(crate) fn int32_as_f64<M: MachineScalar>(machine: &mut M, bits: M::Word) -> M::Word {
    let low = machine.raw_unary(Unary::Narrow(Lane::I32), bits);
    machine.raw_unary(Unary::FromSigned(Lane::F64), low)
}

/// The exact int32 projection of the binary64 `number` (`Value::number`'s integer test): its
/// saturating truncation, and whether that truncation round-trips to the same bits, which rejects
/// fractions, nonfinite values, saturation and -0. Returns `(integer, exact)`.
#[inline(always)]
pub(crate) fn exact_int32<M: MachineScalar>(machine: &mut M, number: M::Word) -> (M::Word, M::Word) {
    let integer = machine.raw_unary(Unary::ToSignedSaturating(Lane::I32), number);
    let round_trip = machine.raw_unary(Unary::FromSigned(Lane::F64), integer);
    let bits = machine.raw_unary(Unary::Bitcast(Lane::I64), number);
    let round_trip_bits = machine.raw_unary(Unary::Bitcast(Lane::I64), round_trip);
    let exact = machine.raw_compare(Relation::Equal, bits, round_trip_bits);
    (integer, exact)
}

/// The tagged word of the binary64 `number` under `Value::number`'s encoding, without a branch: an
/// exact int32 takes the int tag, a NaN the canonical NaN, and anything else its own bits.
#[inline(always)]
pub(crate) fn number_word<M: MachineScalar>(machine: &mut M, number: M::Word) -> M::Word {
    let (integer, is_integer) = exact_int32(machine, number);
    let bits = machine.raw_unary(Unary::Bitcast(Lane::I64), number);
    let integer_bits = machine.raw_unary(Unary::ZeroExtend(Lane::I64), integer);
    let int_tag = machine.raw_const(Lane::I64, ValueLayout::INT_TAG as i64);
    let integer_bits = machine.raw_binary(Binary::Or, integer_bits, int_tag);
    let encoded = machine.raw_select(is_integer, integer_bits, bits);
    let is_nan = machine.raw_compare(Relation::FloatUnordered, number, number);
    let canonical_nan = machine.raw_const(Lane::I64, ValueLayout::CANONICAL_NAN as i64);
    machine.raw_select(is_nan, canonical_nan, encoded)
}

/// The binary64 value of a Number the caller proved numeric, whose tagged word is `bits`: an int32
/// converts (`int32_as_f64`), any other word is its own double.
#[inline(always)]
pub(crate) fn known_number_f64<M: MachineScalar>(machine: &mut M, bits: M::Word) -> M::Word {
    let integer = is_int(&mut MachineTags(&mut *machine), bits);
    let int = int32_as_f64(machine, bits);
    let double = machine.raw_unary(Unary::Bitcast(Lane::F64), bits);
    machine.raw_select(integer, int, double)
}

/// The tagged Number test and unboxing of `bits`: `(decided, value)`. A non-NaN word is its own
/// double; a NaN-space word is an int32 (its tag) or a double NaN, and any other tag is not a Number
/// (`decided` is false). `value` is meaningful where `decided` holds.
#[inline(always)]
pub(crate) fn tagged_number_f64<M: MachineScalar>(machine: &mut M, bits: M::Word) -> (M::Word, M::Word) {
    let number = machine.raw_unary(Unary::Bitcast(Lane::F64), bits);
    let ordered = machine.raw_compare(Relation::FloatEqual, number, number);
    let nan = machine.raw_compare_imm(Relation::Equal, ordered, 0);
    let integer = is_int(&mut MachineTags(&mut *machine), bits);
    let double = is_double(&mut MachineTags(&mut *machine), bits);
    let tagged = machine.raw_binary(Binary::Or, integer, double);
    let decided = machine.raw_binary(Binary::Or, ordered, tagged);
    let integer_nan = machine.raw_binary(Binary::And, nan, integer);
    let int = int32_as_f64(machine, bits);
    let value = machine.raw_select(integer_nan, int, number);
    (decided, value)
}

/// The double bits of `number` and whether they are its cold encoding: a whole, signed-zero or NaN
/// result takes `number_word` (`truncated == number`, or unordered). Returns `(cold, bits)`.
#[inline(always)]
pub(crate) fn number_bits_unless_whole<M: MachineScalar>(machine: &mut M, number: M::Word) -> (M::Word, M::Word) {
    let bits = machine.raw_unary(Unary::Bitcast(Lane::I64), number);
    let truncated = machine.raw_unary(Unary::Truncate, number);
    let cold = machine.raw_compare(Relation::FloatUnorderedEqual, truncated, number);
    (cold, bits)
}

#[cfg(not(stencil_build))]
/// The ToInt32 candidate of the binary64 `number` by the exact I64 truncation: the low 32 bits of its
/// saturating I64 truncation, and whether `number` lies inside the exact range
/// (`EXACT_I64_TRUNCATION_LIMIT`); a caller leaves when the range does not hold. Returns `(int32, exact)`.
#[inline(always)]
pub(crate) fn truncate_int32<M: MachineScalar>(machine: &mut M, number: M::Word) -> (M::Word, M::Word) {
    let magnitude = machine.raw_unary(Unary::FloatAbsolute, number);
    let bound = machine.raw_const(Lane::F64, crate::value::EXACT_I64_TRUNCATION_LIMIT.to_bits() as i64);
    let exact = machine.raw_compare(Relation::FloatLess, magnitude, bound);
    let wide = machine.raw_unary(Unary::ToSignedSaturating(Lane::I64), number);
    (machine.raw_unary(Unary::Narrow(Lane::I32), wide), exact)
}

#[cfg(not(stencil_build))]
/// The interpreter's `immediate_typeof_index` of `value`.
#[inline(always)]
pub(crate) fn concrete_immediate_typeof_index(value: crate::Value) -> Option<usize> {
    let mut stage = crate::staged::Concrete::new(std::ptr::null());
    immediate_typeof_index(&mut stage, value.to_bits(), ()).map(|index| index as usize)
}

#[cfg(test)]
mod typeof_tests {
    use super::*;
    use crate::Value;
    use crate::bytecode::TYPEOF_NAMES;

    /// The tag classes answer the index of their `typeof` name; a heap value or an internal marker has none.
    #[test]
    fn the_tag_classes_name_their_typeof_index() {
        let named = |value: Value| concrete_immediate_typeof_index(value).map(|index| TYPEOF_NAMES[index]);
        assert_eq!(named(Value::UNDEFINED), Some("undefined"));
        assert_eq!(named(Value::DELETED), Some("undefined"));
        assert_eq!(named(Value::NULL), Some("object"));
        assert_eq!([named(Value::TRUE), named(Value::FALSE)], [Some("boolean"); 2]);
        assert_eq!([named(Value::integer(-3)), named(Value::number(2.5)), named(Value::number(f64::NAN))], [Some("number"); 3]);
        assert_eq!(TYPEOF_NAMES[TYPEOF_UNDEFINED as usize], "undefined");
        assert_eq!(TYPEOF_NAMES[TYPEOF_OBJECT as usize], "object");
        assert_eq!(TYPEOF_NAMES[TYPEOF_BOOLEAN as usize], "boolean");
        assert_eq!(TYPEOF_NAMES[TYPEOF_NUMBER as usize], "number");
    }
}
