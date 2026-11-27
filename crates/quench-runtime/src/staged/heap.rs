//! The staged heap operations: literal allocation, the host helpers and the dense element
//! write, each over the frame's proof.

use super::*;

/// The heap operations: literal allocation, the host helpers and element access. A frame with
/// no heap for the operation leaves it `Unresolved` for its host path.
#[inline(always)]
pub fn heap_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        allocate @ Operation::Allocate { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::ALLOCATE]) } { allocation_step_of::<KIND, _, _>(stage, frame, allocate) } else { unreachable!() },
        define @ Operation::DefineElement { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::DEFINE_ELEMENT]) } { element_define_step_of::<KIND, _, _>(stage, frame, define) } else { unreachable!() },
        helper @ (Operation::IndexSetHelper { .. } | Operation::FieldStoreMiss { .. }) => if const { super::op_kind::admits(KIND, &[super::op_kind::INDEX_SET_HELPER, super::op_kind::FIELD_STORE_MISS]) } { helper_step_of::<KIND, _, _>(stage, frame, helper) } else { unreachable!() },
        site @ (Operation::AddSite { .. } | Operation::NamingSite { .. } | Operation::InstanceOf { .. }) => if const { super::op_kind::admits(KIND, &[super::op_kind::ADD_SITE, super::op_kind::NAMING_SITE, super::op_kind::INSTANCE_OF]) } { site_step_of::<KIND, _, _>(stage, frame, site) } else { unreachable!() },
        array @ (Operation::DirectArray { .. } | Operation::ArrayLength { .. } | Operation::TypedLength { .. }
            | Operation::ArrayAppend { .. }) => if const { super::op_kind::admits(KIND, &[super::op_kind::DIRECT_ARRAY, super::op_kind::ARRAY_LENGTH, super::op_kind::TYPED_LENGTH, super::op_kind::ARRAY_APPEND]) } { array_step_of::<KIND, _, _>(stage, frame, array) } else { unreachable!() },
        index => if const { super::op_kind::outside(KIND, &[super::op_kind::ALLOCATE, super::op_kind::DEFINE_ELEMENT, super::op_kind::INDEX_SET_HELPER, super::op_kind::FIELD_STORE_MISS, super::op_kind::ADD_SITE, super::op_kind::NAMING_SITE, super::op_kind::INSTANCE_OF, super::op_kind::DIRECT_ARRAY, super::op_kind::ARRAY_LENGTH, super::op_kind::TYPED_LENGTH, super::op_kind::ARRAY_APPEND]) } { index_step_of::<KIND, _, _>(stage, frame, index) } else { unreachable!() },
    }
}

/// `heap_step_of` for any operation.
#[inline(always)]
pub fn heap_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    heap_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// The property-add and naming sites over the frame's miss edge: a hit defines the written word, a
/// decline leaves the record to the VM (`add::site_add`, `naming::site_name`).
#[inline(always)]
fn site_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let (word, done) = match operation {
        Operation::AddSite { records, object, edge, link, form, value } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::ADD_SITE]) } { (value, add::site_add(stage, records, object, (edge, link), form, value, frame.miss())) } else { unreachable!() },
        Operation::NamingSite { records, function, naming, form, name } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::NAMING_SITE]) } { (name, naming::site_name(stage, records, function, naming, form, name, frame.miss())) } else { unreachable!() },
        Operation::InstanceOf { destination, value, constructor } => if const { super::op_kind::admits(KIND, &[super::op_kind::INSTANCE_OF]) } { {
            let (value, constructor) = (frame.tagged(stage, value), frame.tagged(stage, constructor));
            let Some(holds) = frame.instance_of(stage, value, constructor) else { return Step::Unresolved };
            let (boolean, fact) = frame.boolean(stage, holds);
            frame.write(stage, destination, boolean, fact);
            (boolean, Some(()))
        } } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::ADD_SITE, super::op_kind::NAMING_SITE, super::op_kind::INSTANCE_OF]) } { return Step::Unresolved } else { unreachable!() },
    };
    match done {
        Some(()) => Step::Value(word),
        None => Step::Unresolved,
    }
}

/// `site_step_of` for any operation.
#[inline(always)]
fn site_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    site_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// The array and typed-array operations over a frame's heap: each answer is the frame's
/// (`Frame::direct_array`, `array_length`, `typed_length`, `array_append`), and the default leaves
/// the row to the VM.
#[inline(always)]
fn array_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::DirectArray { source } => if const { super::op_kind::admits(KIND, &[super::op_kind::DIRECT_ARRAY]) } { match frame.direct_array(stage, source) {
            Some(()) => Step::Value(stage.constant(0)),
            None => Step::Unresolved,
        } } else { unreachable!() },
        Operation::ArrayLength { destination, source } => if const { super::op_kind::admits(KIND, &[super::op_kind::ARRAY_LENGTH]) } { match frame.array_length(stage, source) {
            Some((value, fact)) => {
                frame.write(stage, destination, value, fact);
                Step::Value(value)
            }
            None => Step::Unresolved,
        } } else { unreachable!() },
        Operation::TypedLength { destination, source } => if const { super::op_kind::admits(KIND, &[super::op_kind::TYPED_LENGTH]) } { match frame.typed_length(stage, source) {
            Some((value, fact)) => {
                frame.write(stage, destination, value, fact);
                Step::Value(value)
            }
            None => Step::Unresolved,
        } } else { unreachable!() },
        Operation::ArrayAppend { array, value } => if const { super::op_kind::admits(KIND, &[super::op_kind::ARRAY_APPEND]) } { {
            let word = frame.tagged(stage, value);
            match frame.array_append(stage, array, word) {
                Some(()) => Step::Value(word),
                None => Step::Unresolved,
            }
        } } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::DIRECT_ARRAY, super::op_kind::ARRAY_LENGTH, super::op_kind::TYPED_LENGTH, super::op_kind::ARRAY_APPEND]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `array_step_of` for any operation.
#[inline(always)]
fn array_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    array_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// An allocation row over a frame's heap.
#[inline(always)]
fn allocation_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let value = match operation {
        Operation::Allocate { destination, allocation, proof } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::ALLOCATE]) } { frame.allocate_value(stage, destination, allocation, proof) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::ALLOCATE]) } { None } else { unreachable!() },
    };
    match value {
        Some(value) => Step::Value(value),
        None => Step::Unresolved,
    }
}

/// `allocation_step_of` for any operation.
#[inline(always)]
fn allocation_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    allocation_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// An array literal's element definition over a frame's definer (`Frame::define_element`).
#[inline(always)]
fn element_define_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let Operation::DefineElement { array, index, value } = operation else { return Step::Unresolved };
    let array = frame.tagged(stage, array);
    let value = frame.tagged(stage, value);
    match frame.define_element(stage, array, index, value) {
        Some(()) => Step::Value(value),
        None => Step::Unresolved,
    }
}

/// `element_define_step_of` for any operation.
#[inline(always)]
fn element_define_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    element_define_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// A host helper callout (the element store's helper) over the frame's proof.
#[inline(always)]
fn helper_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::IndexSetHelper { base, key, value, proof } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::INDEX_SET_HELPER]) } { match frame.index_set_helper(stage, base, key, value, proof) {
                Some(()) => Step::Value(value),
                None => Step::Unresolved,
            } } else { unreachable!() },
        Operation::FieldStoreMiss { object, atom, site, value, proof } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::FIELD_STORE_MISS]) } { match frame.field_store_miss(stage, object, atom, site, value, proof) {
                Some(()) => Step::Value(value),
                None => Step::Unresolved,
            } } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::INDEX_SET_HELPER, super::op_kind::FIELD_STORE_MISS]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `helper_step_of` for any operation.
#[inline(always)]
fn helper_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    helper_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// A dense element write over a frame's view proof.
#[inline(always)]
fn index_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::IndexWrite { index, value, proof } => if const { super::op_kind::admits(KIND, &[super::op_kind::INDEX_WRITE]) } { match frame.index_write(stage, index, value, proof) {
            Some(()) => Step::Value(value),
            None => Step::Unresolved,
        } } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::INDEX_WRITE]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `index_step_of` for any operation.
#[inline(always)]
fn index_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    index_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

