//! Flow rows of the staged meaning: the flow step and the raise it may take.

use super::*;

/// `step` of a flow row (`RowStep::Flow`); any other operation is unresolved.
#[inline(always)]
pub fn step_flow_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Jump => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::JUMP]) } { Step::Control(frame.jump(stage)) } else { unreachable!() },
        Operation::JumpIf { register, taken } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::JUMP_IF]) } { jump_if(stage, frame, register, taken) } else { unreachable!() },
        Operation::Move { .. } | Operation::LoadConst { .. } | Operation::Nop => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::MOVE, super::super::op_kind::LOAD_CONST, super::super::op_kind::NOP]) } { register_step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        Operation::Throw { source } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::THROW]) } { raise_step(stage, frame, source) } else { unreachable!() },
        Operation::Return { source } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::RETURN]) } { return_step(stage, frame, source) } else { unreachable!() },
        _ => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::JUMP, super::super::op_kind::JUMP_IF, super::super::op_kind::MOVE, super::super::op_kind::LOAD_CONST, super::super::op_kind::NOP, super::super::op_kind::THROW, super::super::op_kind::RETURN]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_flow_of` for any operation.
#[inline(always)]
pub fn step_flow<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_flow_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `Throw` of register `source`: the frame's raise as its control, where the frame raises it; a frame
/// that does not leaves it to the general step.
#[inline(always)]
pub(super) fn raise_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, source: Register) -> Step<S::Word, F::Control> {
    match frame.raise(stage, source) {
        Some(control) => Step::Control(control),
        None => Step::Unresolved,
    }
}
