//! Number arithmetic (`+`, `-`, `*`, `/`) with the int32 fast path and the overflow rules.

use super::*;

/// Arithmetic of two operands in `domain`; the result in `domain` or widened to binary64.
#[inline(always)]
pub(super) fn arithmetic<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand,
    operation: NumberArithmetic, domain: Domain, overflow: (Overflow, NegativeZero)) -> Step<S::Word, F::Control> {
    match domain {
        Domain::Float64 => match number_arithmetic(stage, frame, destination, (left, right), operation) {
            Some(result) => Step::Value(result),
            None => Step::Unresolved,
        },
        Domain::Int32 => {
            let Some((left, right)) = operands(stage, frame, left, right, domain) else { return Step::Unresolved };
            let miss = frame.miss();
            int_arithmetic_result(stage, frame, destination, left, right, operation, overflow, miss)
        }
    }
}

/// `left operation right` of two Number operands, defining `destination` and returning its Number word.
/// A frame that keeps Numbers tagged (`tagged_number_fact`) takes the int32 fast path: where both operands
/// are int32 at run time and the operation is integral, the result is boxed directly as an int32 when it
/// neither overflows nor is `-0` (`int_fast_result`), and every other pair is the binary64 result boxed
/// (`box_number`). The tags are tested first, so a Number that is no int32 computes no int32 result. The
/// register is written once, with the tagged fact, so no arm defines it in another representation. Any
/// other frame defines the binary64 result with `number_fact`. `None` where an operand is no Number, which
/// leaves the row.
#[inline(always)]
pub(super) fn number_arithmetic<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, (left, right): (Operand, Operand), operation: NumberArithmetic) -> Option<S::Word> {
    let int_operands = match frame.tagged_number_fact() {
        Some(fact) if operation.integral() => match (fast_int(stage, frame, left), fast_int(stage, frame, right)) {
            (Some(left), Some(right)) => Some((fact, left, right)),
            _ => None,
        },
        _ => None,
    };
    let Some((fact, (left_int, left_test), (right_int, right_test))) = int_operands else {
        let result = float_operation(stage, frame, (left, right), operation)?;
        let fact = frame.number_fact();
        frame.write(stage, destination, result, fact);
        return Some(result);
    };
    // A pair whose tags are not both int32 settles on its tags alone: its double operands reach the float
    // operation with no int32 arithmetic spent on them first.
    let packed = match both_tests(stage, [left_test, right_test]) {
        Some(tests) => stage.choose(tests,
            #[inline(always)] |stage| int_fast_result(stage, operation, left_int, right_int),
            #[inline(always)] |stage| stage.constant(NOT_INT32)),
        None => int_fast_result(stage, operation, left_int, right_int),
    };
    let taken = stage.ne_imm(packed, NOT_INT32);
    let boxed_int = frame.int_result(stage, packed);
    let boxed = stage.choose_leaving(taken, |_| Some(boxed_int), #[inline(always)] |stage| {
        let number = float_operation(stage, frame, (left, right), operation)?;
        Some(stage.box_number(super::super::Float64::of_row(number)))
    })?;
    frame.write(stage, destination, boxed, fact);
    Some(boxed)
}

/// The word no int32 fast path yields. An int32 word is its low 32 bits, so a word above them names no
/// int32, and the row takes the binary64 path.
const NOT_INT32: u64 = 1 << u32::BITS;

/// `left operation right` of two int32 words, as the int32 result where it neither overflows nor is `-0`
/// (`negative_zero`); `NOT_INT32` otherwise. Only integral operations (add, subtract, multiply) reach it.
#[inline(always)]
fn int_fast_result<S: NumericStage>(stage: &mut S, operation: NumberArithmetic, left: S::Word, right: S::Word) -> S::Word {
    let (result, overflowed) = stage.int_arithmetic(operation, left, right);
    let mut fast = stage.not(overflowed);
    if let Some(zero) = negative_zero(stage, operation, left, right, result) {
        let not_zero = stage.not(zero);
        fast = stage.both(fast, not_zero);
    }
    let sentinel = stage.constant(NOT_INT32);
    stage.select(fast, result, sentinel)
}

/// `left operation right` of two Number operands as binary64 (`float_word`); `None` where an operand is no Number.
#[inline(always)]
fn float_operation<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    (left, right): (Operand, Operand), operation: NumberArithmetic) -> Option<S::Word> {
    let (left, right) = operands(stage, frame, left, right, Domain::Float64)?;
    Some(stage.float_arithmetic(operation, left, right))
}

/// The conjunction of the int32 tests `fast_int` returned for the register operands, or `None` where
/// both operands are constants that need none.
#[inline(always)]
pub(super) fn both_tests<S: NumericStage>(stage: &mut S, tests: [Option<S::Cond>; 2]) -> Option<S::Cond> {
    match tests {
        [Some(a), Some(b)] => Some(stage.both(a, b)),
        [Some(a), None] | [None, Some(a)] => Some(a),
        [None, None] => None,
    }
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(super) fn int_arithmetic_result<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left_value: S::Word, right_value: S::Word,
    operation: NumberArithmetic, (overflow, zero): (Overflow, NegativeZero), miss: S::Miss) -> Step<S::Word, F::Control> {
    let (result, overflowed) = stage.int_arithmetic(operation, left_value, right_value);
    if overflow == Overflow::Exit && stage.guard_false(overflowed, miss).is_none() {
        return Step::Unresolved;
    }
    let negative_zero = negative_zero(stage, operation, left_value, right_value, result)
        .filter(|_| zero == NegativeZero::Exit || overflow == Overflow::Widen);
    if overflow == Overflow::Widen {
        let widen = match negative_zero {
            Some(negative_zero) => stage.either(overflowed, negative_zero),
            None => overflowed,
        };
        // A stage that knows whether the result widens (the interpreter's) writes the int32 or
        // the binary64 it is, rather than the binary64 of either.
        match stage.known(widen) {
            Some(false) => {
                let result = frame.int_result(stage, result);
                let fact = frame.int_fact();
                frame.write(stage, destination, result, fact);
                return Step::Value(result);
            }
            Some(true) => {
                let left = stage.i32_to_f64(left_value);
                let right = stage.i32_to_f64(right_value);
                let number = stage.float_arithmetic(operation, left, right);
                let fact = frame.number_fact();
                frame.write(stage, destination, number, fact);
                return Step::Value(number);
            }
            None => {}
        }
        let number = stage.choose(widen, #[inline(always)] |stage| {
            let left = stage.i32_to_f64(left_value);
            let right = stage.i32_to_f64(right_value);
            stage.float_arithmetic(operation, left, right)
        }, #[inline(always)] |stage| stage.i32_to_f64(result));
        let fact = frame.number_fact();
        frame.write(stage, destination, number, fact);
        return Step::Value(number);
    }
    if let Some(negative_zero) = negative_zero
        && stage.guard_false(negative_zero, miss).is_none() {
        return Step::Unresolved;
    }
    let result = frame.int_result(stage, result);
    let fact = frame.int_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `-0` a zero int32 product yields, which no int32 holds: a zero result with a negative
/// sign on either factor.
#[inline(always)]
pub(super) fn negative_zero<S: NumericStage>(stage: &mut S, operation: NumberArithmetic,
    left: S::Word, right: S::Word, result: S::Word) -> Option<S::Cond> {
    (operation == NumberArithmetic::Multiply).then(|| {
        let zero = stage.i32_is_zero(result);
        let signs = stage.or(left, right);
        let negative = stage.i32_is_negative(signs);
        stage.both(zero, negative)
    })
}
