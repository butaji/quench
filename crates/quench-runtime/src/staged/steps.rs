//! The staged meaning of each operation (`step`): one definition the interpreter runs with
//! `Concrete` and the tiers generate with `Emit`.
use super::*;

/// Run `operation` on `frame`; its word or where control goes.
#[inline(always)]
pub(crate) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Jump => Step::Control(frame.jump(stage)),
        Operation::JumpIf { register, taken } => jump_if(stage, frame, register, taken),
        Operation::Number(operation) => numeric::step(stage, frame, operation),
        property @ (Operation::GetField { .. } | Operation::SetField { .. } | Operation::StoreField { .. } | Operation::FieldGetHost { .. }
            | Operation::FieldAdd { .. } | Operation::IndexGetHost { .. }) => super::property::property_step(stage, frame, property),
        keyed @ (Operation::Delete { .. } | Operation::ElementRead { .. } | Operation::ElementWrite { .. }) => keyed_step(stage, frame, keyed),
        Operation::Void { .. } | Operation::PropertyKey { .. } | Operation::Equality { .. } | Operation::Binary { .. }
            | Operation::CompareJump { .. } => super::generic::step(stage, frame, operation),
        call @ (Operation::Call(_) | Operation::Return { .. } | Operation::Suspend { .. }) => call_step(stage, frame, call),
        Operation::Move { .. } | Operation::LoadConst { .. } | Operation::LoadLocalPlain { .. } | Operation::StoreLocalPlain { .. }
            | Operation::LogicalNot { .. } | Operation::Safepoint { .. } => register_step(stage, frame, operation),
        Operation::LoadEnvLocal { .. } | Operation::StoreEnvLocal { .. } | Operation::ArgumentsLength { .. }
            | Operation::GlobalTypeof { .. } | Operation::GlobalWrite { .. } | Operation::LoadName { .. } | Operation::Protocol { .. }
            | Operation::ForInNext { .. } | Operation::SpreadCall { .. } | Operation::SuperCall { .. }
            | Operation::LoadCapture { .. } | Operation::StoreCapture { .. } => super::names::step(stage, frame, operation),
        value => value_step(stage, frame, value),
    }
}

/// A delete or an element access: the keyed operations (`step`).
#[inline(always)]
fn keyed_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Delete { destination, base, key, strict } => super::delete::step(stage, frame, destination, [base, key], strict),
        element => super::element_rows::step(stage, frame, element),
    }
}

/// A call, a construction, a return or a suspension: the operations that leave or enter an
/// activation (`step`).
#[inline(always)]
fn call_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Call(CallOperation::Site(site)) => super::invoke::invoke(stage, frame, frame.miss(), site),
        Operation::Return { source } => super::invoke::ret(stage, frame, source),
        Operation::Call(operation) => super::protocol::step(stage, frame.miss(), operation),
        Operation::Suspend { kind, destination, value } => super::suspend::suspend(stage, frame, kind, destination, value),
        _ => unreachable!("`step` sends only calls, returns and suspensions"),
    }
}

/// The value-level operations, then the heap's (`step`).
#[inline(always)]
fn value_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::RequireObjectCoercible { register } => require_object_coercible(stage, frame, register),
        Operation::LoadThis { destination } => load_this(stage, frame, destination, frame.miss()),
        Operation::RequireIteratorResult { register } => super::iteration::require_iterator_result(stage, frame, register, frame.miss()),
        Operation::TypeofIndex { destination, source } => super::control::typeof_index(stage, frame, destination, source),
        Operation::LoadEnvLocal { .. } | Operation::StoreEnvLocal { .. } | Operation::ArgumentsLength { .. }
            | Operation::GlobalTypeof { .. } | Operation::GlobalWrite { .. } | Operation::Protocol { .. }
            | Operation::ForInNext { .. } => super::names::step(stage, frame, operation),
        heap => super::heap::heap_step(stage, frame, heap),
    }
}

/// The operations that only move values between registers, locals and the stage.
#[inline(always)]
fn register_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Move { destination, source } => Step::Value(frame.move_register(stage, destination, source)),
        Operation::LoadConst { destination, constant } => {
            let (value, fact) = frame.constant(stage, constant);
            frame.write(stage, destination, value, fact);
            Step::Value(value)
        }
        Operation::LoadLocalPlain { .. } | Operation::StoreLocalPlain { .. } => local_step(stage, frame, operation),
        Operation::LogicalNot { destination, source } => logical_not(stage, frame, destination, source),
        Operation::Safepoint { proof } => match frame.safepoint(stage, proof) {
            Some(()) => Step::Value(stage.constant(0)),
            None => Step::Unresolved,
        },
        _ => Step::Unresolved,
    }
}

/// The operations that read or write a local through the frame.
#[inline(always)]
fn local_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::LoadLocalPlain { destination, slot } => match read_local(stage, frame, destination, slot, frame.local_miss()) {
            Some(value) => Step::Value(value),
            None => Step::Unresolved,
        },
        Operation::StoreLocalPlain { source, slot, result } => {
            let Some(value) = write_local(stage, frame, source, slot, frame.local_miss()) else { return Step::Unresolved };
            if let Some(result) = result {
                let fact = frame.fact(source);
                frame.write(stage, result, value, fact);
            }
            Step::Value(value)
        }
        _ => Step::Unresolved,
    }
}

/// RequireObjectCoercible of `register`: its tagged word where it is neither `null` nor
/// `undefined`; those leave at the frame's miss edge.
#[inline(always)]
fn require_object_coercible<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    register: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let value = frame.tagged(stage, register);
    let null = stage.eq_imm(value, crate::Value::NULL.to_bits());
    let undefined = stage.eq_imm(value, crate::Value::UNDEFINED.to_bits());
    let nullish = stage.either(null, undefined);
    match stage.guard_false(nullish, miss) {
        Some(()) => Step::Value(value),
        None => Step::Unresolved,
    }
}
