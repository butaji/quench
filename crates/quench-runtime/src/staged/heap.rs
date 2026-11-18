//! The staged heap operations: literal allocation, the host helpers and the dense element
//! write, each over the frame's proof.

use super::*;

/// The heap operations: literal allocation, the host helpers and element access. A frame with
/// no heap for the operation leaves it `Unresolved` for its host path.
#[inline(always)]
pub(super) fn heap_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        allocate @ Operation::Allocate { .. } => allocation_step(stage, frame, allocate),
        define @ Operation::DefineElement { .. } => element_define_step(stage, frame, define),
        helper @ (Operation::IndexSetHelper { .. } | Operation::FieldStoreMiss { .. }) => helper_step(stage, frame, helper),
        site @ (Operation::AddSite { .. } | Operation::NamingSite { .. } | Operation::InstanceOf { .. }) => site_step(stage, frame, site),
        array @ (Operation::DirectArray { .. } | Operation::ArrayLength { .. } | Operation::TypedLength { .. }
            | Operation::ArrayAppend { .. }) => array_step(stage, frame, array),
        index => index_step(stage, frame, index),
    }
}

/// The property-add and naming sites over the frame's miss edge: a hit defines the written word, a
/// decline leaves the record to the VM (`add::site_add`, `naming::site_name`).
#[inline(always)]
fn site_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let (word, done) = match operation {
        Operation::AddSite { records, object, edge, link, form, value } =>
            (value, add::site_add(stage, records, object, (edge, link), form, value, frame.miss())),
        Operation::NamingSite { records, function, naming, form, name } =>
            (name, naming::site_name(stage, records, function, naming, form, name, frame.miss())),
        Operation::InstanceOf { destination, value, constructor } => {
            let (value, constructor) = (frame.tagged(stage, value), frame.tagged(stage, constructor));
            let Some(holds) = frame.instance_of(stage, value, constructor) else { return Step::Unresolved };
            let (boolean, fact) = frame.boolean(stage, holds);
            frame.write(stage, destination, boolean, fact);
            (boolean, Some(()))
        }
        _ => return Step::Unresolved,
    };
    match done {
        Some(()) => Step::Value(word),
        None => Step::Unresolved,
    }
}

/// The array and typed-array operations over a frame's heap: each answer is the frame's
/// (`Frame::direct_array`, `array_length`, `typed_length`, `array_append`), and the default leaves
/// the row to the VM.
#[inline(always)]
fn array_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::DirectArray { source } => match frame.direct_array(stage, source) {
            Some(()) => Step::Value(stage.constant(0)),
            None => Step::Unresolved,
        },
        Operation::ArrayLength { destination, source } => match frame.array_length(stage, source) {
            Some((value, fact)) => {
                frame.write(stage, destination, value, fact);
                Step::Value(value)
            }
            None => Step::Unresolved,
        },
        Operation::TypedLength { destination, source } => match frame.typed_length(stage, source) {
            Some((value, fact)) => {
                frame.write(stage, destination, value, fact);
                Step::Value(value)
            }
            None => Step::Unresolved,
        },
        Operation::ArrayAppend { array, value } => {
            let word = frame.tagged(stage, value);
            match frame.array_append(stage, array, word) {
                Some(()) => Step::Value(word),
                None => Step::Unresolved,
            }
        }
        _ => Step::Unresolved,
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

/// An array literal's element definition over a frame's definer (`Frame::define_element`).
#[inline(always)]
fn element_define_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let Operation::DefineElement { array, index, value } = operation else { return Step::Unresolved };
    let array = frame.tagged(stage, array);
    let value = frame.tagged(stage, value);
    match frame.define_element(stage, array, index, value) {
        Some(()) => Step::Value(value),
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

/// A dense element write over a frame's view proof.
#[inline(always)]
fn index_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::IndexWrite { index, value, proof } => match frame.index_write(stage, index, value, proof) {
            Some(()) => Step::Value(value),
            None => Step::Unresolved,
        },
        _ => Step::Unresolved,
    }
}

