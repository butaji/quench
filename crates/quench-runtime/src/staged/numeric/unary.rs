//! Unary rows (`+x`, `-x`, `~x`) and the general unary path.

use super::*;

/// `operation` of one Number `source` in `domain`; a negation of `i32::MIN` widens and
/// leaves at the miss edge, since its Number result is no int32.
#[inline(always)]
pub(super) fn unary<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Operand, operation: NumberUnary, domain: Domain,
) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    match domain {
        Domain::Float64 => {
            let Some(value) = frame.number_operand(stage, source, miss).map(super::super::Float64::word) else { return Step::Unresolved };
            let result = match operation {
                NumberUnary::Plus => value,
                NumberUnary::Negate => stage.float_negate(value),
            };
            let fact = frame.number_fact();
            frame.write(stage, destination, result, fact);
            Step::Value(result)
        }
        Domain::Int32 => {
            // A Number operand past the int32 domain (a double, `-0`, `i32::MIN`'s negation) takes the
            // float row before the VM's operator: only a non-Number leaves.
            let Some(value) = frame.int_operand(stage, source, miss).map(super::super::Int32::word) else {
                return float_or_general_unary(stage, frame, destination, source, operation);
            };
            match operation {
                NumberUnary::Plus => {
                    // The int's word as the frame holds an int (`int_result`): the operand is the raw int32.
                    let result = frame.int_result(stage, value);
                    let fact = frame.int_fact();
                    frame.write(stage, destination, result, fact);
                    Step::Value(result)
                }
                NumberUnary::Negate => {
                    // `-0` is no int32: a zero operand leaves, as `i32::MIN`'s overflow does; the VM's negation takes both.
                    let zero_operand = stage.i32_is_zero(value);
                    if stage.guard_false(zero_operand, miss).is_none() {
                        return float_or_general_unary(stage, frame, destination, source, operation);
                    }
                    let zero = stage.i32_constant(0);
                    match int_arithmetic_result(stage, frame, destination, zero, value,
                        NumberArithmetic::Subtract, LEAVES, miss) {
                        Step::Unresolved => float_or_general_unary(stage, frame, destination, source, operation),
                        step => step,
                    }
                }
            }
        }
    }
}

/// `unary` of a Number operand in the float domain, else the VM's operator (`general_unary`).
#[inline(always)]
fn float_or_general_unary<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Operand, operation: NumberUnary,
) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    match frame.number_operand(stage, source, miss).map(super::super::Float64::word) {
        Some(value) => {
            let result = match operation {
                NumberUnary::Plus => value,
                NumberUnary::Negate => stage.float_negate(value),
            };
            let fact = frame.number_fact();
            frame.write(stage, destination, result, fact);
            Step::Value(result)
        }
        None => general_unary(stage, frame, unary_operator(operation), (destination, source)),
    }
}

/// The unary operator `operation` names, which the VM's `unary` takes (`CallOut::Unary`).
#[cfg(not(stencil_build))]
pub(super) fn unary_operator(operation: NumberUnary) -> u32 {
    match operation {
        NumberUnary::Plus => crate::vm::operations::unary_operator::UnaryPlus,
        NumberUnary::Negate => crate::vm::operations::unary_operator::UnaryNegation,
    }
}

/// The stencil build has no parser, and no stencil frame takes the VM's general operators
/// (`NumericFrame::general_operators`), so no operator code reaches a callout there.
#[cfg(stencil_build)]
pub(super) fn unary_operator(_operation: NumberUnary) -> u32 {
    u32::MAX
}

/// The VM's unary `operator` of the register `source`, defining `destination`, where the frame takes
/// the general operator in place (`NumericFrame::general_operators`); otherwise the row stays unresolved.
#[inline(always)]
pub(super) fn general_unary<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operator: u32,
    (destination, source): (Register, Operand)) -> Step<S::Word, F::Control> {
    match source.register_index() {
        Some(value) if frame.general_operators() =>
            super::super::callout::calling(stage, frame, super::super::CallOut::Unary { operator, value }, Some(destination)),
        _ => Step::Unresolved,
    }
}
