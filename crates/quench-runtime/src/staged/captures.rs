//! Capture access as staged rows: a read of the capture `slot`, `depth` scopes out, and a store
//! to it. The frame owns where captures live (the running function's dense slots, the
//! environment chain the VM walks); the row owns the order of effects, so a store reads its
//! source once and a read defines its destination once. A binding the frame's own access does
//! not take (an uninitialized `let`, a `const`, a shared slot) is the interpreter's exact path.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt};
use super::{NumericFrame, NumericStage, Operation, Step};

/// Run the capture row `operation`: its word, or `Unresolved` where the frame's access declines.
#[inline(always)]
pub fn step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::LoadCapture { destination, depth, slot, root } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_CAPTURE]) } { match frame.capture_slot(stage, depth, slot, root) {
            Some((value, fact)) => {
                frame.write(stage, destination, value, fact);
                Step::Value(value)
            }
            None => Step::Unresolved,
        } } else { unreachable!() },
        Operation::StoreCapture { source, depth, slot } => if const { super::op_kind::admits(KIND, &[super::op_kind::STORE_CAPTURE]) } { {
            let value = frame.tagged(stage, source);
            match frame.store_capture(stage, depth, slot, value) {
                Some(()) => Step::Value(value),
                None => Step::Unresolved,
            }
        } } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::LOAD_CAPTURE, super::op_kind::STORE_CAPTURE]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_of` for any operation.
#[inline(always)]
pub fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

#[cfg(test)]
mod tests;
