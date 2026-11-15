//! The Number operations of `vm::operations` as machine primitives, written once for every
//! machine: each operation's row (`NumberRelation`, `NumberArithmetic`, `NumberBitwise`) maps to
//! the primitive rows of `machine` here, and a backend only supplies those
//! (`vm/compiled/emit_machine`). A Number is a binary64 word, or an int32 word where `Domain`
//! says so.

use super::machine::{Binary, Lane, MachineAccess, MachineScalar, Relation, Unary};
use super::Domain;
use crate::vm::operations::{
    number_remainder, number_truthiness, NumberArithmetic, NumberBitwise, NumberRelation, NumberTruthinessStage,
    WORD32_SHIFT_MASK,
};

/// The comparison `relation` makes of two Numbers of `domain`: signed for int32, IEEE order for
/// binary64; its flag is a word that holds 0 or 1.
pub(crate) fn number_relation<M: MachineScalar>(machine: &mut M, relation: NumberRelation, left: M::Word,
    right: M::Word, domain: Domain) -> M::Word {
    let (integer, float) = match relation {
        NumberRelation::Equal => (Relation::Equal, Relation::FloatEqual),
        NumberRelation::NotEqual => (Relation::NotEqual, Relation::FloatNotEqual),
        NumberRelation::LessThan => (Relation::SignedLess, Relation::FloatLess),
        NumberRelation::LessEqual => (Relation::SignedLessEqual, Relation::FloatLessEqual),
        NumberRelation::GreaterThan => (Relation::SignedGreater, Relation::FloatGreater),
        NumberRelation::GreaterEqual => (Relation::SignedGreaterEqual, Relation::FloatGreaterEqual),
    };
    machine.raw_compare(match domain { Domain::Int32 => integer, Domain::Float64 => float }, left, right)
}

/// `operation` of two binary64 Numbers.
pub(crate) fn float_arithmetic<M: MachineScalar>(machine: &mut M, operation: NumberArithmetic, left: M::Word,
    right: M::Word) -> M::Word {
    let row = match operation {
        NumberArithmetic::Add => Binary::FloatAdd,
        NumberArithmetic::Subtract => Binary::FloatSub,
        NumberArithmetic::Multiply => Binary::FloatMul,
        NumberArithmetic::Divide => Binary::FloatDiv,
    };
    machine.raw_binary(row, left, right)
}

/// The integer row of an operation whose int32 result is an int32: not a division.
fn integer_row(operation: NumberArithmetic) -> Binary {
    match operation {
        NumberArithmetic::Add => Binary::Add,
        NumberArithmetic::Subtract => Binary::Sub,
        NumberArithmetic::Multiply => Binary::Mul,
        NumberArithmetic::Divide => unreachable!("division has a Float64 result"),
    }
}

/// `operation` of two int32 words: its wrapping result and whether it overflowed.
pub(crate) fn checked_int32_arithmetic<M: MachineScalar>(machine: &mut M, operation: NumberArithmetic,
    left: M::Word, right: M::Word) -> (M::Word, M::Word) {
    machine.raw_binary_overflow(integer_row(operation), left, right)
}

/// `operation` of two integer words of one lane as their wrapping bits: int32 words, where a caller
/// that proved projecting the Number result commutes with wrapping arithmetic selects it, or 64-bit
/// words, where a caller that proved the exact result inside the lane does.
pub(crate) fn wrapping_int32_arithmetic<M: MachineScalar>(machine: &mut M, operation: NumberArithmetic,
    left: M::Word, right: M::Word) -> M::Word {
    machine.raw_binary(integer_row(operation), left, right)
}

/// The int32 bits `operation` makes of two int32 words; a shift's count is its low five bits.
pub(crate) fn int32_bitwise<M: MachineScalar>(machine: &mut M, operation: NumberBitwise, left: M::Word,
    right: M::Word) -> M::Word {
    let (row, counted) = match operation {
        NumberBitwise::And => (Binary::And, false),
        NumberBitwise::Or => (Binary::Or, false),
        NumberBitwise::Xor => (Binary::Xor, false),
        NumberBitwise::ShiftLeft => (Binary::ShiftLeft, true),
        NumberBitwise::ShiftRight => (Binary::ShiftRightSigned, true),
        NumberBitwise::ShiftRightUnsigned => (Binary::ShiftRightUnsigned, true),
    };
    let right = if counted {
        let lane = machine.raw_lane(right);
        let mask = machine.raw_const(lane, i64::from(WORD32_SHIFT_MASK));
        machine.raw_binary(Binary::And, right, mask)
    } else {
        right
    };
    machine.raw_binary(row, left, right)
}

/// `int32_bitwise` as the binary64 Number it denotes: signed, but for a zero-fill shift, whose
/// bits are unsigned.
pub(crate) fn number_bitwise<M: MachineScalar>(machine: &mut M, operation: NumberBitwise, left: M::Word,
    right: M::Word) -> M::Word {
    let bits = int32_bitwise(machine, operation, left, right);
    if operation == NumberBitwise::ShiftRightUnsigned {
        let unsigned = machine.raw_unary(Unary::ZeroExtend(Lane::I64), bits);
        machine.raw_unary(Unary::FromUnsigned(Lane::F64), unsigned)
    } else {
        machine.raw_unary(Unary::FromSigned(Lane::F64), bits)
    }
}

/// The parameter lanes of the remainder leaf: its dividend and its divisor.
const REMAINDER_PARAMETERS: [Lane; 2] = [Lane::F64, Lane::F64];

/// The remainder of two binary64 Numbers: a call of the shared leaf, which touches no VM state.
pub(crate) fn float_remainder<M: MachineAccess>(machine: &mut M, left: M::Word, right: M::Word) -> M::Word {
    machine.call_leaf_lanes(number_remainder as *const () as usize, (REMAINDER_PARAMETERS.as_slice(), Lane::F64), &[left, right])
}

/// The ToBoolean stage of a machine: `number_truthiness`' predicates as comparison and bitwise rows.
struct Truthiness<'a, M>(&'a mut M);

impl<M: MachineScalar> NumberTruthinessStage<M::Word> for Truthiness<'_, M> {
    type Predicate = M::Word;

    fn zero(&mut self) -> M::Word {
        self.0.raw_const(Lane::F64, 0.0_f64.to_bits() as i64)
    }

    fn equal(&mut self, left: M::Word, right: M::Word) -> M::Word {
        self.0.raw_compare(Relation::FloatEqual, left, right)
    }

    fn not_equal(&mut self, left: M::Word, right: M::Word) -> M::Word {
        self.0.raw_compare(Relation::FloatNotEqual, left, right)
    }

    fn and(&mut self, left: M::Word, right: M::Word) -> M::Word {
        self.0.raw_binary(Binary::And, left, right)
    }
}

/// ToBoolean of a Number the caller proved numeric: the one formula (`number_truthiness`) on a
/// machine's words.
pub(crate) fn number_truthy<M: MachineScalar>(machine: &mut M, number: M::Word) -> M::Word {
    number_truthiness(&mut Truthiness(machine), number)
}
