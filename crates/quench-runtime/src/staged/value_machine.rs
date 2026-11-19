//! The value rows on the machine layer (`MachineScalar`): the number conversions and boxing
//! the u-jit and the interpreter share. Kept apart from `value` because the b-jit stencil build
//! includes `value` without the machine layer.

#![allow(dead_code)]

use super::machine::{Binary, Lane, MachineScalar, Relation, Unary};
use crate::value::ValueLayout;

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
    let integer = machine_is_int(machine, bits);
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
    let integer = machine_is_int(machine, bits);
    let double = machine_is_double(machine, bits);
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

/// Whether the word `bits` is an int32 by its tag (`is_int`), as a machine flag.
#[inline(always)]
fn machine_is_int<M: MachineScalar>(machine: &mut M, bits: M::Word) -> M::Word {
    let tag = machine.raw_binary_imm(Binary::And, bits, ValueLayout::TAG_MASK as i64);
    machine.raw_compare_imm(Relation::Equal, tag, ValueLayout::INT_TAG as i64)
}

/// Whether the word `bits` is a binary64 Number by its tag field (`is_double`), as a machine flag.
#[inline(always)]
fn machine_is_double<M: MachineScalar>(machine: &mut M, bits: M::Word) -> M::Word {
    let field = machine.raw_binary_imm(Binary::ShiftRightUnsigned, bits, i64::from(ValueLayout::TAG_SHIFT));
    let offset = machine.raw_binary_imm(Binary::Add, field, ValueLayout::FIRST_TAG_FIELD.wrapping_neg() as i64);
    machine.raw_compare_imm(Relation::UnsignedGreater, offset, ValueLayout::TAG_FIELD_SPAN as i64)
}
