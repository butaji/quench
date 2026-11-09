//! The staged meaning of each operation (`step`): one definition the interpreter runs with
//! `Concrete` and the tiers generate with `Emit`.
use super::*;

/// Run `operation` on `frame`; its word or where control goes.
#[inline(always)]
pub(crate) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Move { destination, source } => Step::Value(frame.move_register(stage, destination, source)),
        Operation::LoadConst { destination, constant } => {
            let (value, fact) = frame.constant(stage, constant);
            frame.write(stage, destination, value, fact);
            Step::Value(value)
        }
        Operation::Jump => Step::Control(frame.jump(stage)),
        Operation::JumpIf { register, taken } => jump_if(stage, frame, register, taken),
        Operation::Number(operation) => numeric::step(stage, frame, operation),
        Operation::RequireObjectCoercible { register } => require_object_coercible(stage, frame, register),
        Operation::LoadThis { destination } => load_this(stage, frame, destination),
        heap => heap_step(stage, frame, heap),
    }
}

/// The heap operations: a field read or write. A frame with no heap for the operation leaves
/// it `Unresolved` for its host path.
#[inline(always)]
fn heap_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        field @ (Operation::ReadField { .. } | Operation::WriteField { .. }) =>
            field_step(stage, frame, field),
        literal @ (Operation::MakeObject { .. } | Operation::MakeArray { .. }) =>
            allocation_step(stage, frame, literal),
        index => index_step(stage, frame, index),
    }
}

/// A field read or write over a frame's object proof.
#[inline(always)]
fn field_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::ReadField { base, slot, ty, proof } => match frame.field_read(stage, base, slot, ty, proof) {
            Some((value, _)) => Step::Value(value),
            None => Step::Unresolved,
        },
        Operation::WriteField { base, slot, ty, value, proof } =>
            match frame.field_write(stage, base, slot, ty, value, proof) {
                Some(()) => Step::Value(value),
                None => Step::Unresolved,
            },
        _ => Step::Unresolved,
    }
}

/// An object or array literal allocation over a frame's heap.
#[inline(always)]
fn allocation_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let value = match operation {
        Operation::MakeObject { destination, proof } =>
            frame.allocate_object(stage, destination, proof),
        Operation::MakeArray { destination, length, elements, proof } =>
            frame.allocate_array(stage, destination, length, elements, proof),
        _ => None,
    };
    match value {
        Some(value) => Step::Value(value),
        None => Step::Unresolved,
    }
}

/// A dense or typed element read or write over a frame's view proof.
#[inline(always)]
fn index_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::IndexRead { index, proof } => match frame.index_read(stage, index, proof) {
            Some(value) => Step::Value(value),
            None => Step::Unresolved,
        },
        Operation::IndexWrite { index, value, proof } => match frame.index_write(stage, index, value, proof) {
            Some(()) => Step::Value(value),
            None => Step::Unresolved,
        },
        Operation::TypedIndexRead { index, proof } => match frame.typed_index_read(stage, index, proof) {
            Some(value) => Step::Value(value),
            None => Step::Unresolved,
        },
        Operation::TypedIndexWrite { index, value, proof } => match frame.typed_index_write(stage, index, value, proof) {
            Some(()) => Step::Value(value),
            None => Step::Unresolved,
        },
        _ => Step::Unresolved,
    }
}

/// A conditional jump on the register's truthiness, taken when it is `taken`; unresolved where
/// the stage cannot tell its truthiness.
fn jump_if<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, register: Register, taken: bool)
    -> Step<S::Word, F::Control> {
    let Some(condition) = frame.truthiness(stage, register) else { return Step::Unresolved };
    let condition = if taken { condition } else { stage.not(condition) };
    Step::Control(frame.branch(stage, condition))
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
