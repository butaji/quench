//! Register and local steps: the register and local rows and their increments.

use super::*;

/// The operations that only move values between registers, locals and the stage.
#[inline(always)]
pub(super) fn register_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Move { destination, source } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::MOVE]) } { Step::Value(frame.move_register(stage, destination, source)) } else { unreachable!() },
        Operation::LoadConst { destination, constant } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::LOAD_CONST]) } { {
            let (value, fact) = frame.constant(stage, constant);
            frame.write(stage, destination, value, fact);
            Step::Value(value)
        } } else { unreachable!() },
        Operation::LoadLocalPlain { .. } | Operation::StoreLocalPlain { .. } | Operation::LoadLocalChecked { .. }
            | Operation::StoreLocalChecked { .. } | Operation::InitializeLocal { .. } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::LOAD_LOCAL_PLAIN, super::super::op_kind::STORE_LOCAL_PLAIN, super::super::op_kind::LOAD_LOCAL_CHECKED, super::super::op_kind::STORE_LOCAL_CHECKED, super::super::op_kind::INITIALIZE_LOCAL]) } { local_step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        Operation::LogicalNot { destination, source } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::LOGICAL_NOT]) } { logical_not(stage, frame, destination, source) } else { unreachable!() },
        Operation::Nop => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::NOP]) } { Step::Value(stage.constant(0)) } else { unreachable!() },
        Operation::Safepoint { proof } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::SAFEPOINT]) } { match frame.safepoint(stage, proof) {
            Some(()) => Step::Value(stage.constant(0)),
            None => Step::Unresolved,
        } } else { unreachable!() },
        _ => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::MOVE, super::super::op_kind::LOAD_CONST, super::super::op_kind::LOAD_LOCAL_PLAIN, super::super::op_kind::STORE_LOCAL_PLAIN, super::super::op_kind::LOAD_LOCAL_CHECKED, super::super::op_kind::STORE_LOCAL_CHECKED, super::super::op_kind::INITIALIZE_LOCAL, super::super::op_kind::LOGICAL_NOT, super::super::op_kind::NOP, super::super::op_kind::SAFEPOINT]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `register_step_of` for any operation.
#[inline(always)]
fn register_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    register_step_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// The operations that read or write a local through the frame.
#[inline(always)]
pub(super) fn local_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        // A keeping activation's `arguments` binding is the frame's to answer (`Frame::arguments_binding`); any
        // other slot, or a frame that declines, is the slot's own read.
        Operation::LoadLocalPlain { destination, slot } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::LOAD_LOCAL_PLAIN]) } { {
            let value = match read_arguments_binding(stage, frame, destination, slot) {
                Some(value) => Some(value),
                None => read_local(stage, frame, destination, slot, frame.local_miss()),
            };
            match value {
                Some(value) => Step::Value(value),
                None => Step::Unresolved,
            }
        } } else { unreachable!() },
        Operation::StoreLocalPlain { source, slot, result } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::STORE_LOCAL_PLAIN]) } { {
            let Some(value) = write_local(stage, frame, source, slot, frame.local_miss()) else { return Step::Unresolved };
            if let Some(result) = result {
                let fact = frame.tagged_fact(source);
                frame.write(stage, result, value, fact);
            }
            Step::Value(value)
        } } else { unreachable!() },
        Operation::LoadLocalChecked { destination, slot, numeric } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::LOAD_LOCAL_CHECKED]) } { {
            if numeric {
                return Step::Unresolved;
            }
            // An `arguments` binding is initialized from entry, so the frame's answer skips the check.
            if let Some(value) = read_arguments_binding(stage, frame, destination, slot) {
                return Step::Value(value);
            }
            match read_initialized_local(stage, frame, destination, slot, frame.miss()) {
                Some(value) => Step::Value(value),
                None => Step::Unresolved,
            }
        } } else { unreachable!() },
        Operation::StoreLocalChecked { source, slot, result, initializing } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::STORE_LOCAL_CHECKED]) } { {
            let miss = frame.miss();
            let stored = if initializing {
                write_local(stage, frame, source, slot, Some(miss))
            } else {
                write_initialized_local(stage, frame, source, slot, miss)
            };
            let Some(value) = stored else { return Step::Unresolved };
            if let Some(result) = result {
                let fact = frame.tagged_fact(source);
                frame.write(stage, result, value, fact);
            }
            Step::Value(value)
        } } else { unreachable!() },
        Operation::InitializeLocal { slot } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::INITIALIZE_LOCAL]) } { {
            let hole = stage.constant(crate::Value::DELETED.to_bits());
            let miss = frame.local_miss();
            if frame.store_local(stage, slot, hole, miss) { Step::Value(hole) } else { Step::Unresolved }
        } } else { unreachable!() },
        _ => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::LOAD_LOCAL_PLAIN, super::super::op_kind::STORE_LOCAL_PLAIN, super::super::op_kind::LOAD_LOCAL_CHECKED, super::super::op_kind::STORE_LOCAL_CHECKED, super::super::op_kind::INITIALIZE_LOCAL]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `local_step_of` for any operation.
#[inline(always)]
fn local_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    local_step_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `IncDecLocal`: the `++` (or `--`) of `source` written to frame local `local`, by the Number step of an
/// int32 or binary64 operand, else the VM's update (`CallOut::IncDec`). Defines the updated value.
#[inline(always)]
pub(super) fn inc_dec_local<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, local: Register, source: Register,
    decrement: bool) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let operation = if decrement { crate::vm::operations::NumberArithmetic::Subtract } else { crate::vm::operations::NumberArithmetic::Add };
    let operand = crate::bytecode::Operand::register(source);
    let (word, fact) = match frame.int_operand(stage, operand, miss).map(super::super::Int32::word) {
        Some(value) => {
            let one = stage.i32_constant(1);
            let (sum, overflowed) = stage.int_arithmetic(operation, value, one);
            match stage.guard_false(overflowed, miss) {
                Some(()) => (frame.int_result(stage, sum), frame.int_fact()),
                None => return inc_dec_local_by_vm(stage, frame, local, source, decrement),
            }
        }
        None => match frame.number_operand(stage, operand, miss).map(super::super::Float64::word) {
            Some(number) => {
                let one = stage.f64_constant(1.0);
                (stage.float_arithmetic(operation, number, one), frame.number_fact())
            }
            None => return inc_dec_local_by_vm(stage, frame, local, source, decrement),
        },
    };
    if frame.store_numeric_local(stage, local, word, fact) { Step::Value(word) } else { Step::Unresolved }
}

/// The VM's update of `source` written to local `local` (`inc_dec_local`'s other operands).
#[inline(always)]
fn inc_dec_local_by_vm<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, local: Register, source: Register,
    decrement: bool) -> Step<S::Word, F::Control> {
    if !frame.general_operators() {
        return Step::Unresolved;
    }
    match frame.vm_step(stage, super::super::CallOut::IncDec { source, decrement }, None) {
        // The update has run, so the local must take it: a frame that answers an update keeps its locals.
        Some((word, fact)) => {
            assert!(frame.store_numeric_local(stage, local, word, fact), "a frame that answers an update keeps its numeric local");
            Step::Value(word)
        }
        None => Step::Unresolved,
    }
}

/// RequireObjectCoercible of `register`: its tagged word where it is neither `null` nor
/// `undefined`; those leave at the frame's miss edge.
#[inline(always)]
pub(super) fn require_object_coercible<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    register: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let value = frame.tagged(stage, register);
    let null = stage.eq_imm(value, crate::Value::NULL.to_bits());
    let undefined = stage.eq_imm(value, crate::Value::UNDEFINED.to_bits());
    let nullish = stage.either(null, undefined);
    match stage.guard_false(nullish, miss) {
        Some(()) => Step::Value(value),
        None => {
            frame.object_coercion_failed(stage, value);
            Step::Unresolved
        }
    }
}
