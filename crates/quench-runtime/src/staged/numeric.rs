//! Number x Number semantics over `Stage`/`Frame`, shared by the interpreter and native tiers.

use super::{Frame, Stage, Step};
use crate::bytecode::{Operand, Register};
use crate::vm::operations::{
    CheckedInt32Stage, NumberArithmetic, NumberArithmeticStage, NumberF64Stage, NumberRelation,
    PartialOrderStage,
};

/// The representation both operands and the result of a Number operation have.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Domain {
    Int32,
    Float64,
}

/// What an int32 operation does at an overflow or a negative zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Overflow {
    /// Leave at the frame's miss edge.
    Exit,
    /// Keep the wrapping int32 result.
    Wrap,
    /// Continue as the binary64 result.
    Widen,
}

/// Machine primitives for the two Number representations: int32 words hold the unsigned
/// low 32 bits, float64 words hold their IEEE bits.
pub(crate) trait NumericStage: Stage {
    /// `relation` of two words in `domain`.
    fn relation(&mut self, relation: NumberRelation, domain: Domain,
        left: Self::Word, right: Self::Word) -> Self::Cond;
    /// `operation` of two binary64 words.
    fn float_arithmetic(&mut self, operation: NumberArithmetic,
        left: Self::Word, right: Self::Word) -> Self::Word;
    /// `operation` of two int32 words: the wrapped result and whether it overflowed.
    fn int_arithmetic(&mut self, operation: NumberArithmetic,
        left: Self::Word, right: Self::Word) -> (Self::Word, Self::Cond);
    /// The int32 word holding `value`.
    fn i32_constant(&mut self, value: i32) -> Self::Word;
    fn i32_to_f64(&mut self, value: Self::Word) -> Self::Word;
    fn i32_is_zero(&mut self, value: Self::Word) -> Self::Cond;
    fn i32_is_negative(&mut self, value: Self::Word) -> Self::Cond;
    fn both(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    fn either(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    fn cond_word(&mut self, cond: Self::Cond) -> Self::Word;
    /// Continue only where `cond` is false.
    fn guard_false(&mut self, cond: Self::Cond, miss: Self::Miss) -> Option<()>;
}

/// A frame whose operands a numeric operation proves in its representation and whose result
/// it defines with that representation's fact.
pub(crate) trait NumericFrame<S: Stage>: Frame<S> {
    fn int_operand(&mut self, stage: &mut S, operand: Operand, miss: S::Miss) -> S::Word;
    fn number_operand(&mut self, stage: &mut S, operand: Operand, miss: S::Miss) -> S::Word;
    /// Continue where `register` holds a Number; `None` where it does not, so nothing runs
    /// past it. A specializer may prove a narrower representation its facts favor (an
    /// untyped register taken for an int32), leaving at `miss` for the rest.
    fn prove_number(&mut self, stage: &mut S, register: Register, miss: S::Miss) -> Option<()>;
    fn miss(&self) -> S::Miss;
    fn int_fact(&self) -> Self::Fact;
    fn number_fact(&self) -> Self::Fact;
}

/// A Number operation `step` runs, with the operands a tier decoded.
#[derive(Clone, Copy)]
pub(crate) enum NumberOperation {
    /// Number x Number arithmetic with both operands in `domain`.
    Arithmetic { destination: Register, left: Operand, right: Operand,
        operation: NumberArithmetic, domain: Domain, overflow: Overflow },
    /// Number x Number relation, producing its predicate word.
    Relation { left: Operand, right: Operand, relation: NumberRelation, domain: Domain },
    /// `source ± 1` (`++`, or `--` where `decrement`) of an int32 `source`.
    IncDec { destination: Register, source: Register, decrement: bool, overflow: Overflow },
    /// ToNumeric of a Number `source` into `destination`; any other value leaves.
    ToNumeric { destination: Register, source: Register },
}

/// Run the Number `operation` on `frame`; its word, or `Unresolved` where it left.
#[inline(always)]
pub(crate) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: NumberOperation) -> Step<S::Word, F::Control> {
    match operation {
        NumberOperation::Arithmetic { destination, left, right, operation, domain, overflow } =>
            arithmetic(stage, frame, destination, left, right, operation, domain, overflow),
        NumberOperation::Relation { left, right, relation, domain } =>
            self::relation(stage, frame, left, right, relation, domain),
        NumberOperation::IncDec { destination, source, decrement, overflow } =>
            increment(stage, frame, destination, source, decrement, overflow),
        NumberOperation::ToNumeric { destination, source } => to_numeric(stage, frame, destination, source),
    }
}

/// A relation of two operands in `domain`; its predicate word.
fn relation<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand, relation: NumberRelation, domain: Domain) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let (left, right) = match domain {
        Domain::Int32 => (frame.int_operand(stage, left, miss), frame.int_operand(stage, right, miss)),
        Domain::Float64 => (frame.number_operand(stage, left, miss), frame.number_operand(stage, right, miss)),
    };
    let condition = stage.relation(relation, domain, left, right);
    Step::Value(stage.cond_word(condition))
}

/// Arithmetic of two operands in `domain`; the result in `domain` or widened to binary64.
fn arithmetic<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand,
    operation: NumberArithmetic, domain: Domain, overflow: Overflow) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    match domain {
        Domain::Float64 => {
            let left = frame.number_operand(stage, left, miss);
            let right = frame.number_operand(stage, right, miss);
            let result = stage.float_arithmetic(operation, left, right);
            let fact = frame.number_fact();
            frame.write(stage, destination, result, fact);
            Step::Value(result)
        }
        Domain::Int32 => {
            let left = frame.int_operand(stage, left, miss);
            let right = frame.int_operand(stage, right, miss);
            int_arithmetic_result(stage, frame, destination, left, right, operation, overflow, miss)
        }
    }
}

/// ToNumeric of a Number `source`: the Number itself, copied with its facts. Any other
/// value leaves (the interpreter converts a BigInt or a primitive by its general path).
fn to_numeric<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    match frame.prove_number(stage, source, miss) {
        Some(()) => Step::Value(frame.move_register(stage, destination, source)),
        None => Step::Unresolved,
    }
}

/// `source ± 1` (`++`/`--`) of an int32 `source`: the int32 sum or difference with one.
fn increment<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register, decrement: bool,
    overflow: Overflow) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let value = frame.int_operand(stage, Operand::register(source), miss);
    let one = stage.i32_constant(1);
    let operation = if decrement { NumberArithmetic::Subtract } else { NumberArithmetic::Add };
    int_arithmetic_result(stage, frame, destination, value, one, operation, overflow, miss)
}

#[allow(clippy::too_many_arguments)]
fn int_arithmetic_result<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left_value: S::Word, right_value: S::Word,
    operation: NumberArithmetic, overflow: Overflow, miss: S::Miss) -> Step<S::Word, F::Control> {
    let (result, overflowed) = stage.int_arithmetic(operation, left_value, right_value);
    if overflow == Overflow::Exit && stage.guard_false(overflowed, miss).is_none() {
        return Step::Unresolved;
    }
    let negative_zero = negative_zero(stage, operation, left_value, right_value, result);
    if overflow == Overflow::Widen {
        let widen = match negative_zero {
            Some(negative_zero) => stage.either(overflowed, negative_zero),
            None => overflowed,
        };
        let number = stage.choose(widen, |stage| {
            let left = stage.i32_to_f64(left_value);
            let right = stage.i32_to_f64(right_value);
            stage.float_arithmetic(operation, left, right)
        }, |stage| stage.i32_to_f64(result));
        let fact = frame.number_fact();
        frame.write(stage, destination, number, fact);
        return Step::Value(number);
    }
    if let Some(negative_zero) = negative_zero
        && stage.guard_false(negative_zero, miss).is_none() {
        return Step::Unresolved;
    }
    let fact = frame.int_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `-0` a zero int32 product yields, which no int32 holds: a zero result with a negative
/// sign on either factor.
fn negative_zero<S: NumericStage>(stage: &mut S, operation: NumberArithmetic,
    left: S::Word, right: S::Word, result: S::Word) -> Option<S::Cond> {
    (operation == NumberArithmetic::Multiply).then(|| {
        let zero = stage.i32_is_zero(result);
        let signs = stage.or(left, right);
        let negative = stage.i32_is_negative(signs);
        stage.both(zero, negative)
    })
}

fn int32(word: u64) -> i32 { word as u32 as i32 }

/// The interpreter's wrapping int32 result, from the same rows as every arithmetic stage.
struct WrappingInt32Stage;

macro_rules! wrapping_int32_arithmetic {
    ($($operation:ident => $operator:ident, $method:ident, $instruction:ident, $symbol:tt, $float:ident, $checked:ident, $overflow:ident;)+) => {
        impl NumberArithmeticStage<i32> for WrappingInt32Stage {
            type Output = i32;
            $(
                #[inline(always)]
                fn $method(&mut self, left: i32, right: i32) -> Self::Output {
                    wrapping_int32_arithmetic!(@wrapped left, right, $symbol)
                }
            )+
        }
    };
    (@wrapped $left:ident, $right:ident, +) => { $left.wrapping_add($right) };
    (@wrapped $left:ident, $right:ident, -) => { $left.wrapping_sub($right) };
    (@wrapped $left:ident, $right:ident, *) => { $left.wrapping_mul($right) };
    (@wrapped $left:ident, $right:ident, /) => {
        { let _ = ($left, $right); unreachable!("division has a binary64 result") }
    };
}
crate::vm::operations::number_arithmetic_rows!(wrapping_int32_arithmetic);

impl NumericStage for super::Concrete {
    fn relation(&mut self, relation: NumberRelation, domain: Domain,
        left: u64, right: u64) -> bool {
        match domain {
            Domain::Int32 => relation.apply(&mut PartialOrderStage, int32(left), int32(right)),
            Domain::Float64 => relation.apply(&mut PartialOrderStage,
                f64::from_bits(left), f64::from_bits(right)),
        }
    }
    fn float_arithmetic(&mut self, operation: NumberArithmetic, left: u64, right: u64) -> u64 {
        operation.apply(&mut NumberF64Stage,
            f64::from_bits(left), f64::from_bits(right)).to_bits()
    }
    fn int_arithmetic(&mut self, operation: NumberArithmetic,
        left: u64, right: u64) -> (u64, bool) {
        let (left, right) = (int32(left), int32(right));
        let checked = operation.apply(&mut CheckedInt32Stage, left, right);
        let overflowed = checked.is_none();
        let result = checked.unwrap_or_else(|| operation.apply(&mut WrappingInt32Stage, left, right));
        (u64::from(result as u32), overflowed)
    }
    fn i32_constant(&mut self, value: i32) -> u64 { u64::from(value as u32) }
    fn i32_to_f64(&mut self, value: u64) -> u64 { f64::from(int32(value)).to_bits() }
    fn i32_is_zero(&mut self, value: u64) -> bool { int32(value) == 0 }
    fn i32_is_negative(&mut self, value: u64) -> bool { int32(value) < 0 }
    fn both(&mut self, a: bool, b: bool) -> bool { a && b }
    fn either(&mut self, a: bool, b: bool) -> bool { a || b }
    fn cond_word(&mut self, cond: bool) -> u64 { u64::from(cond) }
    fn guard_false(&mut self, cond: bool, _miss: ()) -> Option<()> { (!cond).then_some(()) }
}

#[cfg(test)]
mod tests;
