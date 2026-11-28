//! The family entry points of the staged meaning: local, number, value, names, property, keyed and arithmetic steps.

use super::*;

/// `step` of a local row (`RowStep::Local`).
#[inline(always)]
pub fn step_local_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    local_step_of::<KIND, _, _>(stage, frame, operation)
}

/// `step_local_of` for any operation.
#[inline(always)]
pub fn step_local<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_local_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a numeric row (`RowStep::Number`).
#[inline(always)]
pub fn step_number_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Number(operation) => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::NUMBER]) } { numeric::step(stage, frame, operation) } else { unreachable!() },
        _ => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::NUMBER]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_number_of` for any operation.
#[inline(always)]
pub fn step_number<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_number_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a value row (`RowStep::Value`).
#[inline(always)]
pub fn step_value_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::RequireObjectCoercible { register } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::REQUIRE_OBJECT_COERCIBLE]) } { require_object_coercible(stage, frame, register) } else { unreachable!() },
        Operation::LoadThis { destination } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::LOAD_THIS]) } { load_this(stage, frame, destination, frame.miss()) } else { unreachable!() },
        Operation::LoadCallee { destination } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::LOAD_CALLEE]) } { load_callee(stage, frame, destination) } else { unreachable!() },
        Operation::RequireIteratorResult { register } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::REQUIRE_ITERATOR_RESULT]) } { super::super::iteration::require_iterator_result(stage, frame, register, frame.miss()) } else { unreachable!() },
        _ => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::REQUIRE_OBJECT_COERCIBLE, super::super::op_kind::LOAD_THIS, super::super::op_kind::LOAD_CALLEE, super::super::op_kind::REQUIRE_ITERATOR_RESULT]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_value_of` for any operation.
#[inline(always)]
pub fn step_value<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_value_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a names row (`RowStep::Names`).
#[inline(always)]
pub fn step_names_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    super::super::names::step_of::<KIND, _, _>(stage, frame, operation)
}

/// `step_names_of` for any operation.
#[inline(always)]
pub fn step_names<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_names_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a property row (`RowStep::Property`).
#[inline(always)]
pub fn step_property_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    super::super::property::property_step_of::<KIND, _, _>(stage, frame, operation)
}

/// `step_property_of` for any operation.
#[inline(always)]
pub fn step_property<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_property_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a keyed row (`RowStep::Keyed`).
#[inline(always)]
pub fn step_keyed_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    keyed_step_of::<KIND, _, _>(stage, frame, operation)
}

/// `step_keyed_of` for any operation.
#[inline(always)]
pub fn step_keyed<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_keyed_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a `Binary` or `Unary` row (`RowStep::Arith`): only the operations those decode to.
#[inline(always)]
pub fn step_arith_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Number(operation) => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::NUMBER]) } { numeric::step(stage, frame, operation) } else { unreachable!() },
        Operation::Void { .. } | Operation::Unary { .. } | Operation::PropertyKey { .. } | Operation::Equality { .. }
            | Operation::Binary { .. } | Operation::CompareJump { .. } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::VOID, super::super::op_kind::UNARY, super::super::op_kind::PROPERTY_KEY, super::super::op_kind::EQUALITY, super::super::op_kind::BINARY, super::super::op_kind::COMPARE_JUMP]) } { super::super::generic::step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        Operation::LogicalNot { .. } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::LOGICAL_NOT]) } { register_step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        Operation::TypeofIndex { destination, source } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::TYPEOF_INDEX]) } { super::super::control::typeof_index(stage, frame, destination, source) } else { unreachable!() },
        Operation::TypeofTest { destination, source, index, negated } =>
            if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::TYPEOF_TEST]) } { super::super::control::typeof_test(stage, frame, destination, source, index, negated) } else { unreachable!() },
        Operation::TypeofString { destination, source } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::TYPEOF_STRING]) } { super::super::control::typeof_string(stage, frame, destination, source) } else { unreachable!() },
        _ => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::NUMBER, super::super::op_kind::VOID, super::super::op_kind::UNARY, super::super::op_kind::PROPERTY_KEY, super::super::op_kind::EQUALITY, super::super::op_kind::BINARY, super::super::op_kind::COMPARE_JUMP, super::super::op_kind::LOGICAL_NOT, super::super::op_kind::TYPEOF_INDEX, super::super::op_kind::TYPEOF_TEST, super::super::op_kind::TYPEOF_STRING]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_arith_of` for any operation.
#[inline(always)]
pub fn step_arith<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_arith_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a generic row (`RowStep::Generic`).
#[inline(always)]
pub fn step_generic_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    super::super::generic::step_of::<KIND, _, _>(stage, frame, operation)
}

/// `step_generic_of` for any operation.
#[inline(always)]
pub fn step_generic<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_generic_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}
