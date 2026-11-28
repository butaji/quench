//! Number words: the double and int32 classes, the exact int32 and long readings, and the binary64 words.

use super::*;

/// Whether `bits` is a binary64 Number: past the span of the tag fields the immediates use. At a
/// machine it is a flag word.
#[inline(always)]
pub fn is_double<T: TagOps>(ops: &mut T, bits: T::Word) -> T::Flag {
    let field = ops.shr_imm(bits, u64::from(ValueLayout::TAG_SHIFT));
    let offset = ops.add_imm(field, ValueLayout::FIRST_TAG_FIELD.wrapping_neg());
    ops.ugt_imm(offset, ValueLayout::TAG_FIELD_SPAN)
}

/// Both Number classes of `bits` from one shifted tag field, as `Value::as_number` reads them:
/// `(double, int)`. The int test compares the same field the double test offsets, so a stage
/// shifts the word once for both.
#[inline(always)]
pub fn number_classes<T: TagOps>(ops: &mut T, bits: T::Word) -> (T::Flag, T::Flag) {
    let field = ops.shr_imm(bits, u64::from(ValueLayout::TAG_SHIFT));
    let offset = ops.add_imm(field, ValueLayout::FIRST_TAG_FIELD.wrapping_neg());
    let double = ops.ugt_imm(offset, ValueLayout::TAG_FIELD_SPAN);
    let int = ops.eq_imm(field, ValueLayout::INT_TAG >> ValueLayout::TAG_SHIFT);
    (double, int)
}

/// The binary64 value of the int32 whose tagged word is `bits`: its low half, sign-extended and
/// converted (`Value::as_int` read as a Number). Both int32 unboxing rows share it.
#[inline(always)]
pub fn int32_as_f64<M: MachineScalar>(machine: &mut M, bits: M::Word) -> M::Word {
    let low = machine.raw_unary(Unary::Narrow(Lane::I32), bits);
    machine.raw_unary(Unary::FromSigned(Lane::F64), low)
}

/// The exact int32 projection of the binary64 `number` (`Value::number`'s integer test): its
/// saturating truncation, and whether that truncation round-trips to the same bits, which rejects
/// fractions, nonfinite values, saturation and -0. Returns `(integer, exact)`.
#[inline(always)]
pub fn exact_int32<M: MachineScalar>(machine: &mut M, number: M::Word) -> (M::Word, M::Word) {
    exact_integer(machine, number, Lane::I32)
}

/// The exact signed 64-bit integer of the binary64 `number`, by `exact_int32`'s test at the word's width.
/// A Number of magnitude 2^63 or more saturates and can still round-trip, so the caller bounds the
/// integer within `IntRange::EXACT` as well.
#[inline(always)]
pub fn exact_long<M: MachineScalar>(machine: &mut M, number: M::Word) -> (M::Word, M::Word) {
    exact_integer(machine, number, Lane::I64)
}

fn exact_integer<M: MachineScalar>(machine: &mut M, number: M::Word, lane: Lane) -> (M::Word, M::Word) {
    let integer = machine.raw_unary(Unary::ToSignedSaturating(lane), number);
    let round_trip = machine.raw_unary(Unary::FromSigned(Lane::F64), integer);
    let bits = machine.raw_unary(Unary::Bitcast(Lane::I64), number);
    let round_trip_bits = machine.raw_unary(Unary::Bitcast(Lane::I64), round_trip);
    let exact = machine.raw_compare(Relation::Equal, bits, round_trip_bits);
    (integer, exact)
}

/// The tagged word of the binary64 `number` under `Value::number`'s encoding, without a branch: an
/// exact int32 takes the int tag, a NaN the canonical NaN, and anything else its own bits.
#[inline(always)]
pub fn number_word<M: MachineScalar>(machine: &mut M, number: M::Word) -> M::Word {
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
pub fn known_number_f64<M: MachineScalar>(machine: &mut M, bits: M::Word) -> M::Word {
    let integer = is_int(&mut MachineTags(&mut *machine), bits);
    let int = int32_as_f64(machine, bits);
    let double = machine.raw_unary(Unary::Bitcast(Lane::F64), bits);
    machine.raw_select(integer, int, double)
}

/// The tagged Number test and unboxing of `bits`: `(decided, value)`. A non-NaN word is its own
/// double; a NaN-space word is an int32 (its tag) or a double NaN, and any other tag is not a Number
/// (`decided` is false). `value` is meaningful where `decided` holds.
#[inline(always)]
pub fn tagged_number_f64<M: MachineScalar>(machine: &mut M, bits: M::Word) -> (M::Word, M::Word) {
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
pub fn number_bits_unless_whole<M: MachineScalar>(machine: &mut M, number: M::Word) -> (M::Word, M::Word) {
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
pub fn truncate_int32<M: MachineScalar>(machine: &mut M, number: M::Word) -> (M::Word, M::Word) {
    let magnitude = machine.raw_unary(Unary::FloatAbsolute, number);
    let bound = machine.raw_const(Lane::F64, crate::value::EXACT_I64_TRUNCATION_LIMIT.to_bits() as i64);
    let exact = machine.raw_compare(Relation::FloatLess, magnitude, bound);
    let wide = machine.raw_unary(Unary::ToSignedSaturating(Lane::I64), number);
    (machine.raw_unary(Unary::Narrow(Lane::I32), wide), exact)
}
