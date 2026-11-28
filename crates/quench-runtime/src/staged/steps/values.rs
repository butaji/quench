//! Value steps: the value row and the string case index it switches on.

use super::*;

/// The value-level operations, then the heap's (`step`).
#[inline(always)]
pub(super) fn value_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::RequireObjectCoercible { register } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::REQUIRE_OBJECT_COERCIBLE]) } { require_object_coercible(stage, frame, register) } else { unreachable!() },
        Operation::LoadThis { destination } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::LOAD_THIS]) } { load_this(stage, frame, destination, frame.miss()) } else { unreachable!() },
        Operation::LoadCallee { destination } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::LOAD_CALLEE]) } { load_callee(stage, frame, destination) } else { unreachable!() },
        Operation::StringCaseIndex { destination, source, start, count } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::STRING_CASE_INDEX]) } { string_case_index(stage, frame, destination, source, start, count) } else { unreachable!() },
        Operation::RequireIteratorResult { register } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::REQUIRE_ITERATOR_RESULT]) } { super::super::iteration::require_iterator_result(stage, frame, register, frame.miss()) } else { unreachable!() },
        Operation::TypeofIndex { destination, source } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::TYPEOF_INDEX]) } { super::super::control::typeof_index(stage, frame, destination, source) } else { unreachable!() },
        Operation::TypeofTest { destination, source, index, negated } =>
            if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::TYPEOF_TEST]) } { super::super::control::typeof_test(stage, frame, destination, source, index, negated) } else { unreachable!() },
        Operation::TypeofString { destination, source } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::TYPEOF_STRING]) } { super::super::control::typeof_string(stage, frame, destination, source) } else { unreachable!() },
        Operation::LoadEnvLocal { .. } | Operation::StoreEnvLocal { .. } | Operation::ArgumentsLength { .. } | Operation::ArgumentGet { .. }
            | Operation::GlobalTypeof { .. } | Operation::GlobalWrite { .. } | Operation::GlobalLexicalLoad { .. }
            | Operation::GlobalLexicalStore { .. } | Operation::Protocol { .. }
            | Operation::ForInNext { .. } | Operation::EnumeratedGet { .. } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::LOAD_ENV_LOCAL, super::super::op_kind::STORE_ENV_LOCAL, super::super::op_kind::ARGUMENTS_LENGTH, super::super::op_kind::ARGUMENT_GET, super::super::op_kind::GLOBAL_TYPEOF, super::super::op_kind::GLOBAL_WRITE, super::super::op_kind::GLOBAL_LEXICAL_LOAD, super::super::op_kind::GLOBAL_LEXICAL_STORE, super::super::op_kind::PROTOCOL, super::super::op_kind::FOR_IN_NEXT, super::super::op_kind::ENUMERATED_GET]) } { super::super::names::step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        heap => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::REQUIRE_OBJECT_COERCIBLE, super::super::op_kind::LOAD_THIS, super::super::op_kind::LOAD_CALLEE, super::super::op_kind::STRING_CASE_INDEX, super::super::op_kind::REQUIRE_ITERATOR_RESULT, super::super::op_kind::TYPEOF_INDEX, super::super::op_kind::TYPEOF_TEST, super::super::op_kind::TYPEOF_STRING, super::super::op_kind::LOAD_ENV_LOCAL, super::super::op_kind::STORE_ENV_LOCAL, super::super::op_kind::ARGUMENTS_LENGTH, super::super::op_kind::ARGUMENT_GET, super::super::op_kind::GLOBAL_TYPEOF, super::super::op_kind::GLOBAL_WRITE, super::super::op_kind::GLOBAL_LEXICAL_LOAD, super::super::op_kind::GLOBAL_LEXICAL_STORE, super::super::op_kind::PROTOCOL, super::super::op_kind::FOR_IN_NEXT, super::super::op_kind::ENUMERATED_GET]) } { super::super::heap::heap_step_of::<KIND, _, _>(stage, frame, heap) } else { unreachable!() },
    }
}

/// `value_step_of` for any operation.
#[inline(always)]
fn value_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    value_step_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `StringCaseIndex`: the frame's case-table index of `source` among the constants `start..start + count`,
/// defined into `destination`; a frame with no case table leaves it to the general step.
#[inline(always)]
fn string_case_index<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    source: Register, start: u32, count: u16) -> Step<S::Word, F::Control> {
    match frame.case_index(stage, source, start, count) {
        Some((value, fact)) => {
            frame.write(stage, destination, value, fact);
            Step::Value(value)
        }
        None => Step::Unresolved,
    }
}
