//! The staged meaning of each operation (`step`): one definition the interpreter runs with
//! `Concrete` and the tiers generate with `Emit`.
use super::*;

/// Run `operation` on `frame`; its word or where control goes.
#[inline(always)]
pub(crate) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Jump => Step::Control(frame.jump(stage)),
        Operation::JumpIf { register, taken } => super::heap::jump_if(stage, frame, register, taken),
        Operation::Number(operation) => numeric::step(stage, frame, operation),
        Operation::RequireObjectCoercible { register } => require_object_coercible(stage, frame, register),
        Operation::LoadThis { destination } => load_this(stage, frame, destination),
        Operation::Void { .. } | Operation::PropertyKey { .. } | Operation::Equality { .. } => super::generic::step(stage, frame, operation),
        Operation::Move { .. } | Operation::LoadConst { .. } | Operation::LoadLocalPlain { .. } | Operation::StoreLocalPlain { .. }
            | Operation::LogicalNot { .. } | Operation::Safepoint { .. } => register_step(stage, frame, operation),
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
        Operation::LogicalNot { destination, source } => match frame.truthiness(stage, source) {
            Some(truthy) => {
                let falsy = stage.not(truthy);
                let (value, fact) = frame.boolean(stage, falsy);
                frame.write(stage, destination, value, fact);
                Step::Value(value)
            }
            None => Step::Unresolved,
        },
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
        Operation::LoadLocalPlain { destination, slot } => {
            match frame.load_local(stage, slot, frame.local_miss()) {
                Some((value, fact)) => {
                    frame.write(stage, destination, value, fact);
                    Step::Value(value)
                }
                None => Step::Unresolved,
            }
        }
        Operation::StoreLocalPlain { source, slot, result } => {
            let value = frame.tagged(stage, source);
            if !frame.store_local(stage, slot, value, frame.local_miss()) { return Step::Unresolved; }
            if let Some(result) = result {
                let fact = frame.fact(source);
                frame.write(stage, result, value, fact);
            }
            Step::Value(value)
        }
        _ => Step::Unresolved,
    }
}

/// The activation's `this` into `destination`, where the activation holds it.
#[inline(always)]
fn load_this<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let this = frame.this(stage);
    let bound = stage.ne_imm(this, crate::Value::DELETED.to_bits());
    match stage.guard(bound, miss) {
        Some(()) => {
            frame.define_this(stage, destination, this);
            Step::Value(this)
        }
        None => Step::Unresolved,
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
