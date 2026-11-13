//! The staged heap operations: literal allocation, the host helpers and dense/typed element
//! access, each over the frame's proof.

use super::*;

/// The heap operations: literal allocation, the host helpers and element access. A frame with
/// no heap for the operation leaves it `Unresolved` for its host path.
#[inline(always)]
pub(super) fn heap_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        allocate @ Operation::Allocate { .. } => allocation_step(stage, frame, allocate),
        helper @ (Operation::IndexSetHelper { .. } | Operation::FieldStoreMiss { .. }) => helper_step(stage, frame, helper),
        index => index_step(stage, frame, index),
    }
}

/// An allocation row over a frame's heap.
#[inline(always)]
fn allocation_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let value = match operation {
        Operation::Allocate { destination, allocation, proof } =>
            frame.allocate_value(stage, destination, allocation, proof),
        _ => None,
    };
    match value {
        Some(value) => Step::Value(value),
        None => Step::Unresolved,
    }
}

/// A host helper callout (the element store's helper) over the frame's proof.
#[inline(always)]
fn helper_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::IndexSetHelper { base, key, value, proof } =>
            match frame.index_set_helper(stage, base, key, value, proof) {
                Some(()) => Step::Value(value),
                None => Step::Unresolved,
            },
        Operation::FieldStoreMiss { object, atom, site, value, proof } =>
            match frame.field_store_miss(stage, object, atom, site, value, proof) {
                Some(()) => Step::Value(value),
                None => Step::Unresolved,
            },
        _ => Step::Unresolved,
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
pub(super) fn jump_if<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, register: Register, taken: bool)
    -> Step<S::Word, F::Control> {
    let Some(condition) = frame.truthiness(stage, register) else { return Step::Unresolved };
    let condition = if taken { condition } else { stage.not(condition) };
    Step::Control(frame.branch(stage, condition))
}

