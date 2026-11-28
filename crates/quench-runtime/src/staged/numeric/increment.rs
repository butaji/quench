//! `++`/`--` and ToNumeric over Number operands.

use super::*;

/// `operand`'s int32 word for the int32 fast paths, with the test that makes it an int32 at run
/// time. A program constant that is an int32 is its own word with no test: its decode proved it,
/// so its tag is never read at run time. A register is its low word, tested by its tag unless the
/// frame's type fact already proves it an int32 (then the test is folded away: `None`). A frame that
/// keeps its Numbers unboxed (`tagged_number_fact` is `None`) has no tag to test, and boxing a binary64
/// register to test it would cost more than the binary64 path, so such a register takes that path.
/// `None` where the operand is neither, or a constant that is no int32, which keeps the binary64 path.
#[inline(always)]
pub(super) fn fast_int<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operand: Operand) -> Option<(S::Word, Option<S::Cond>)> {
    match frame.constant_operand(operand) {
        Some(value) => Some((stage.i32_constant(value.as_int()?), None)),
        None => {
            let proven = operand.register_index()
                .and_then(|register| frame.register_type(register))
                .is_some_and(|ty| ty.is_int32());
            if !proven && frame.tagged_number_fact().is_none() {
                return None;
            }
            let bits = frame.operand_word(stage, operand)?;
            let test = (!proven).then(|| super::super::value::is_int(stage, bits));
            Some((stage.narrow(bits), test))
        }
    }
}

/// ToNumeric of a Number `source`: the Number itself, copied with its facts. Any other
/// value leaves (the interpreter converts a BigInt or a primitive by its general path).
#[inline(always)]
pub(super) fn to_numeric<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    match frame.prove_number(stage, source, miss) {
        Some(()) => Step::Value(frame.move_register(stage, destination, source)),
        None => general_update(stage, frame, super::super::CallOut::ToNumeric { source }, destination),
    }
}

/// `source ± 1` (`++`/`--`) of an int32 `source`: the int32 sum or difference with one.
#[inline(always)]
pub(super) fn increment<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register, decrement: bool,
    overflow: Overflow) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let operation = if decrement { NumberArithmetic::Subtract } else { NumberArithmetic::Add };
    let update = super::super::CallOut::IncDec { source, decrement };
    let Some(value) = frame.int_operand(stage, Operand::register(source), miss).map(super::super::Int32::word) else {
        // A stage that found the operand no int32 (the interpreter's, or a tier that knows it a
        // binary64) steps the binary64 by one; any other operand is the VM's update.
        let Some(number) = frame.number_operand(stage, Operand::register(source), miss).map(super::super::Float64::word) else {
            return general_update(stage, frame, update, destination);
        };
        let one = stage.f64_constant(1.0);
        let result = stage.float_arithmetic(operation, number, one);
        let fact = frame.number_fact();
        frame.write(stage, destination, result, fact);
        return Step::Value(result);
    };
    let one = stage.i32_constant(1);
    match int_arithmetic_result(stage, frame, destination, value, one, operation, (overflow, NegativeZero::Exit), miss) {
        // An int32 overflow is the VM's update, which steps it as a double.
        Step::Unresolved => general_update(stage, frame, update, destination),
        step => step,
    }
}

/// The VM's general operation `call` (ToNumeric or an update) defining `destination`, where the frame takes
/// the general operator in place (`NumericFrame::general_operators`); otherwise the row stays unresolved.
#[inline(always)]
fn general_update<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, call: super::super::CallOut,
    destination: Register) -> Step<S::Word, F::Control> {
    if frame.general_operators() { super::super::callout::calling(stage, frame, call, Some(destination)) } else { Step::Unresolved }
}
