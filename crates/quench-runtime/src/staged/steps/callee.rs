//! Call and return steps: the callee load, the call and the return of a row.

use super::*;

/// A call, a construction, a return or a suspension: the operations that leave or enter an
/// activation (`step`).
#[inline(always)]
pub(super) fn call_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Call(CallOperation::Site(site)) => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::CALL]) } { super::super::invoke::invoke(stage, frame, frame.miss(), site) } else { unreachable!() },
        Operation::Return { source } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::RETURN]) } { return_step(stage, frame, source) } else { unreachable!() },
        Operation::Call(operation) => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::CALL]) } { super::super::protocol::step(stage, frame.miss(), operation) } else { unreachable!() },
        #[cfg(not(stencil_build))]
        Operation::Suspend { kind, destination, value } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::SUSPEND]) } { super::super::suspend::suspend(stage, frame, kind, destination, value) } else { unreachable!() },
        _ => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::CALL, super::super::op_kind::RETURN, super::super::op_kind::CALL]) } { unreachable!("`step` sends only calls, returns and suspensions") } else { unreachable!() },
    }
}

/// `call_step_of` for any operation.
#[inline(always)]
fn call_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    call_step_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `Return` of `source`: a raw record's word completes through `Frame::return_raw`
/// (`Frame::returns_raw`), any other register through `Frame::ret`. Both leave the function, so
/// both answer `Step::Control`, or `Step::Unresolved` where the frame does not complete them.
#[inline(always)]
pub(super) fn return_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, source: Register) -> Step<S::Word, F::Control> {
    if frame.returns_raw() {
        frame.return_raw(stage, source).map_or(Step::Unresolved, Step::Control)
    } else {
        super::super::invoke::ret(stage, frame, source)
    }
}

/// LoadCallee of `destination`: the frame's function word and fact; where the frame does not hold
/// it, leave at the miss edge, where the VM reads it.
#[inline(always)]
pub(super) fn load_callee<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register) -> Step<S::Word, F::Control> {
    let Some((value, fact)) = frame.callee_value(stage) else { return Step::Unresolved };
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}
