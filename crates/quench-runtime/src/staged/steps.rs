//! The staged meaning of each operation (`step`): one definition the interpreter runs with
//! `Concrete` and the tiers generate with `Emit`.
use super::*;

/// `step` out of line, for a generated row whose opcode has no family step (`RowStep::Full`): one instance per
/// stage and frame type rather than the whole interpreter inlined into every such row's instance (`row_step!`).
#[inline(never)]
pub(crate) fn step_out_of_line<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step(stage, frame, operation)
}

/// Run `operation` on `frame`; its word or where control goes.
#[inline(always)]
pub(crate) fn step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Jump => if const { super::op_kind::admits(KIND, &[super::op_kind::JUMP]) } { Step::Control(frame.jump(stage)) } else { unreachable!() },
        Operation::JumpIf { register, taken } => if const { super::op_kind::admits(KIND, &[super::op_kind::JUMP_IF]) } { jump_if(stage, frame, register, taken) } else { unreachable!() },
        Operation::Throw { source } => if const { super::op_kind::admits(KIND, &[super::op_kind::THROW]) } { raise_step(stage, frame, source) } else { unreachable!() },
        Operation::Number(operation) => if const { super::op_kind::admits(KIND, &[super::op_kind::NUMBER]) } { numeric::step(stage, frame, operation) } else { unreachable!() },
        property @ (Operation::GetField { .. } | Operation::SetField { .. } | Operation::StoreField { .. } | Operation::FieldGetHost { .. }
            | Operation::FieldAdd { .. } | Operation::IndexGetHost { .. }) => if const { super::op_kind::admits(KIND, &[super::op_kind::GET_FIELD, super::op_kind::SET_FIELD, super::op_kind::STORE_FIELD, super::op_kind::FIELD_GET_HOST, super::op_kind::FIELD_ADD, super::op_kind::INDEX_GET_HOST]) } { super::property::property_step_of::<KIND, _, _>(stage, frame, property) } else { unreachable!() },
        keyed @ (Operation::Delete { .. } | Operation::ElementRead { .. } | Operation::ElementReadOperand { .. } | Operation::ElementWrite { .. }) => if const { super::op_kind::admits(KIND, &[super::op_kind::DELETE, super::op_kind::ELEMENT_READ, super::op_kind::ELEMENT_READ_OPERAND, super::op_kind::ELEMENT_WRITE]) } { keyed_step_of::<KIND, _, _>(stage, frame, keyed) } else { unreachable!() },
        Operation::IncDecLocal { local, source, decrement } => if const { super::op_kind::admits(KIND, &[super::op_kind::INC_DEC_LOCAL]) } { inc_dec_local(stage, frame, local, source, decrement) } else { unreachable!() },
        Operation::Void { .. } | Operation::Unary { .. } | Operation::PropertyKey { .. } | Operation::Equality { .. }
            | Operation::Binary { .. } | Operation::CompareJump { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::VOID, super::op_kind::UNARY, super::op_kind::PROPERTY_KEY, super::op_kind::EQUALITY, super::op_kind::BINARY, super::op_kind::COMPARE_JUMP]) } { super::generic::step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        #[cfg(not(stencil_build))]
        Operation::WasmJump(jump) => if const { super::op_kind::admits(KIND, &[super::op_kind::WASM_JUMP]) } { super::wasm_jump::step(stage, frame, jump) } else { unreachable!() },
        #[cfg(not(stencil_build))]
        Operation::BranchTable(table) => if const { super::op_kind::admits(KIND, &[super::op_kind::BRANCH_TABLE]) } { super::wasm_jump::branch_table(stage, frame, table) } else { unreachable!() },
        call @ (Operation::Call(_) | Operation::Return { .. }) => if const { super::op_kind::admits(KIND, &[super::op_kind::CALL, super::op_kind::RETURN]) } { call_step_of::<KIND, _, _>(stage, frame, call) } else { unreachable!() },
        #[cfg(not(stencil_build))]
        call @ Operation::Suspend { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::SUSPEND]) } { call_step_of::<KIND, _, _>(stage, frame, call) } else { unreachable!() },
        Operation::Move { .. } | Operation::LoadConst { .. } | Operation::LoadLocalPlain { .. } | Operation::StoreLocalPlain { .. }
            | Operation::LoadLocalChecked { .. } | Operation::StoreLocalChecked { .. } | Operation::InitializeLocal { .. }
            | Operation::LogicalNot { .. } | Operation::Safepoint { .. } | Operation::Nop => if const { super::op_kind::admits(KIND, &[super::op_kind::MOVE, super::op_kind::LOAD_CONST, super::op_kind::LOAD_LOCAL_PLAIN, super::op_kind::STORE_LOCAL_PLAIN, super::op_kind::LOAD_LOCAL_CHECKED, super::op_kind::STORE_LOCAL_CHECKED, super::op_kind::INITIALIZE_LOCAL, super::op_kind::LOGICAL_NOT, super::op_kind::SAFEPOINT, super::op_kind::NOP]) } { register_step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        Operation::LoadEnvLocal { .. } | Operation::StoreEnvLocal { .. } | Operation::ArgumentsLength { .. } | Operation::ArgumentGet { .. }
            | Operation::GlobalTypeof { .. } | Operation::GlobalWrite { .. } | Operation::GlobalLexicalLoad { .. }
            | Operation::GlobalLexicalStore { .. } | Operation::LoadName { .. } | Operation::Protocol { .. }
            | Operation::ForInNext { .. } | Operation::EnumeratedGet { .. } | Operation::SpreadCall { .. } | Operation::SuperCall { .. }
            | Operation::LoadCapture { .. } | Operation::StoreCapture { .. } | Operation::CloneEnv { .. }
            | Operation::IteratorCleanupPush { .. } | Operation::IteratorCleanupPop | Operation::SetFunctionName { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_ENV_LOCAL, super::op_kind::STORE_ENV_LOCAL, super::op_kind::ARGUMENTS_LENGTH, super::op_kind::ARGUMENT_GET, super::op_kind::GLOBAL_TYPEOF, super::op_kind::GLOBAL_WRITE, super::op_kind::GLOBAL_LEXICAL_LOAD, super::op_kind::GLOBAL_LEXICAL_STORE, super::op_kind::LOAD_NAME, super::op_kind::PROTOCOL, super::op_kind::FOR_IN_NEXT, super::op_kind::ENUMERATED_GET, super::op_kind::SPREAD_CALL, super::op_kind::SUPER_CALL, super::op_kind::LOAD_CAPTURE, super::op_kind::STORE_CAPTURE, super::op_kind::CLONE_ENV, super::op_kind::ITERATOR_CLEANUP_PUSH, super::op_kind::ITERATOR_CLEANUP_POP, super::op_kind::SET_FUNCTION_NAME]) } { super::names::step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        #[cfg(not(stencil_build))]
        Operation::SuperConstArrayObject2(pair) => if const { super::op_kind::admits(KIND, &[super::op_kind::SUPER_CONST_ARRAY_OBJECT2]) } { super::superinstructions::step(stage, frame, pair) } else { unreachable!() },
        guard @ (Operation::GuardIdentity { .. } | Operation::GuardLive | Operation::GuardRealm { .. } | Operation::GuardDeclared { .. }
            | Operation::GuardPrototype { .. } | Operation::GuardShape { .. } | Operation::GuardStores { .. } | Operation::BackEdge { .. }) => if const { super::op_kind::admits(KIND, &[super::op_kind::GUARD_IDENTITY, super::op_kind::GUARD_LIVE, super::op_kind::GUARD_REALM, super::op_kind::GUARD_DECLARED, super::op_kind::GUARD_PROTOTYPE, super::op_kind::GUARD_SHAPE, super::op_kind::GUARD_STORES, super::op_kind::BACK_EDGE]) } { guard_step_of::<KIND, _, _>(stage, frame, guard) } else { unreachable!() },
        value => if const { super::op_kind::outside(KIND, &[super::op_kind::JUMP, super::op_kind::JUMP_IF, super::op_kind::THROW, super::op_kind::NUMBER, super::op_kind::GET_FIELD, super::op_kind::SET_FIELD, super::op_kind::STORE_FIELD, super::op_kind::FIELD_GET_HOST, super::op_kind::FIELD_ADD, super::op_kind::INDEX_GET_HOST, super::op_kind::DELETE, super::op_kind::ELEMENT_READ, super::op_kind::ELEMENT_READ_OPERAND, super::op_kind::ELEMENT_WRITE, super::op_kind::INC_DEC_LOCAL, super::op_kind::VOID, super::op_kind::UNARY, super::op_kind::PROPERTY_KEY, super::op_kind::EQUALITY, super::op_kind::BINARY, super::op_kind::COMPARE_JUMP, super::op_kind::CALL, super::op_kind::RETURN, super::op_kind::MOVE, super::op_kind::LOAD_CONST, super::op_kind::LOAD_LOCAL_PLAIN, super::op_kind::STORE_LOCAL_PLAIN, super::op_kind::LOAD_LOCAL_CHECKED, super::op_kind::STORE_LOCAL_CHECKED, super::op_kind::INITIALIZE_LOCAL, super::op_kind::LOGICAL_NOT, super::op_kind::SAFEPOINT, super::op_kind::NOP, super::op_kind::LOAD_ENV_LOCAL, super::op_kind::STORE_ENV_LOCAL, super::op_kind::ARGUMENTS_LENGTH, super::op_kind::ARGUMENT_GET, super::op_kind::GLOBAL_TYPEOF, super::op_kind::GLOBAL_WRITE, super::op_kind::GLOBAL_LEXICAL_LOAD, super::op_kind::GLOBAL_LEXICAL_STORE, super::op_kind::LOAD_NAME, super::op_kind::PROTOCOL, super::op_kind::FOR_IN_NEXT, super::op_kind::ENUMERATED_GET, super::op_kind::SPREAD_CALL, super::op_kind::SUPER_CALL, super::op_kind::LOAD_CAPTURE, super::op_kind::STORE_CAPTURE, super::op_kind::CLONE_ENV, super::op_kind::ITERATOR_CLEANUP_PUSH, super::op_kind::ITERATOR_CLEANUP_POP, super::op_kind::SET_FUNCTION_NAME, super::op_kind::GUARD_IDENTITY, super::op_kind::GUARD_LIVE, super::op_kind::GUARD_REALM, super::op_kind::GUARD_DECLARED, super::op_kind::GUARD_PROTOTYPE, super::op_kind::GUARD_SHAPE, super::op_kind::GUARD_STORES, super::op_kind::BACK_EDGE]) } { value_step_of::<KIND, _, _>(stage, frame, value) } else { unreachable!() },
    }
}

/// `step_of` for any operation.
#[inline(always)]
pub(crate) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// The control a frame answers (`Frame::raise`), or `Unresolved` where it answers none.
#[inline(always)]
fn suspended<W, C>(control: Option<C>) -> Step<W, C> {
    match control {
        Some(control) => Step::Control(control),
        None => Step::Unresolved,
    }
}

/// A tier guard, or a taken back edge's poll and count (`step`): `Step::Value` where the proof holds,
/// `Step::Unresolved` where the row leaves.
#[inline(always)]
fn guard_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let held = match operation {
        Operation::GuardIdentity { source, expected } => if const { super::op_kind::admits(KIND, &[super::op_kind::GUARD_IDENTITY]) } { super::guard::identity(stage, source, expected, miss) } else { unreachable!() },
        Operation::GuardLive => if const { super::op_kind::admits(KIND, &[super::op_kind::GUARD_LIVE]) } { super::guard::live(stage, frame, miss) } else { unreachable!() },
        Operation::GuardRealm { expected } => if const { super::op_kind::admits(KIND, &[super::op_kind::GUARD_REALM]) } { super::guard::realm(stage, frame, expected, miss) } else { unreachable!() },
        Operation::GuardDeclared { source, mask } => if const { super::op_kind::admits(KIND, &[super::op_kind::GUARD_DECLARED]) } { super::guard::declared(stage, source, mask, miss) } else { unreachable!() },
        Operation::GuardPrototype { source, records, shape, link } => if const { super::op_kind::admits(KIND, &[super::op_kind::GUARD_PROTOTYPE]) } { super::guard::prototype(stage, records, source, (shape, link), miss) } else { unreachable!() },
        Operation::GuardShape { source, records, shape, form } => if const { super::op_kind::admits(KIND, &[super::op_kind::GUARD_SHAPE]) } { super::guard::shape(stage, records, source, (shape, form), miss) } else { unreachable!() },
        Operation::GuardStores { specialized } => if const { super::op_kind::admits(KIND, &[super::op_kind::GUARD_STORES]) } { super::guard::stores(stage, specialized, miss) } else { unreachable!() },
        Operation::BackEdge { proof } => if const { super::op_kind::admits(KIND, &[super::op_kind::BACK_EDGE]) } { super::guard::back_edge(stage, frame, proof) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::GUARD_IDENTITY, super::op_kind::GUARD_LIVE, super::op_kind::GUARD_REALM, super::op_kind::GUARD_DECLARED, super::op_kind::GUARD_PROTOTYPE, super::op_kind::GUARD_SHAPE, super::op_kind::GUARD_STORES, super::op_kind::BACK_EDGE]) } { unreachable!("`guard_step` sends only guards and back edges") } else { unreachable!() },
    };
    match held {
        Some(()) => Step::Value(stage.constant(0)),
        None => Step::Unresolved,
    }
}

/// `guard_step_of` for any operation.
#[inline(always)]
fn guard_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    guard_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// A delete or an element access: the keyed operations (`step`).
#[inline(always)]
fn keyed_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Delete { destination, base, key, strict } => if const { super::op_kind::admits(KIND, &[super::op_kind::DELETE]) } { super::delete::step(stage, frame, destination, [base, key], strict) } else { unreachable!() },
        element => if const { super::op_kind::outside(KIND, &[super::op_kind::DELETE]) } { super::element_rows::step_of::<KIND, _, _>(stage, frame, element) } else { unreachable!() },
    }
}

/// `keyed_step_of` for any operation.
#[inline(always)]
fn keyed_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    keyed_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// A call, a construction, a return or a suspension: the operations that leave or enter an
/// activation (`step`).
#[inline(always)]
fn call_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Call(CallOperation::Site(site)) => if const { super::op_kind::admits(KIND, &[super::op_kind::CALL]) } { super::invoke::invoke(stage, frame, frame.miss(), site) } else { unreachable!() },
        Operation::Return { source } => if const { super::op_kind::admits(KIND, &[super::op_kind::RETURN]) } { return_step(stage, frame, source) } else { unreachable!() },
        Operation::Call(operation) => if const { super::op_kind::admits(KIND, &[super::op_kind::CALL]) } { super::protocol::step(stage, frame.miss(), operation) } else { unreachable!() },
        #[cfg(not(stencil_build))]
        Operation::Suspend { kind, destination, value } => if const { super::op_kind::admits(KIND, &[super::op_kind::SUSPEND]) } { super::suspend::suspend(stage, frame, kind, destination, value) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::CALL, super::op_kind::RETURN, super::op_kind::CALL]) } { unreachable!("`step` sends only calls, returns and suspensions") } else { unreachable!() },
    }
}

/// `call_step_of` for any operation.
#[inline(always)]
fn call_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    call_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `Return` of `source`: a raw record's word completes through `Frame::return_raw`
/// (`Frame::returns_raw`), any other register through `Frame::ret`. Both leave the function, so
/// both answer `Step::Control`, or `Step::Unresolved` where the frame does not complete them.
#[inline(always)]
fn return_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, source: Register) -> Step<S::Word, F::Control> {
    if frame.returns_raw() {
        frame.return_raw(stage, source).map_or(Step::Unresolved, Step::Control)
    } else {
        super::invoke::ret(stage, frame, source)
    }
}

/// The value-level operations, then the heap's (`step`).
#[inline(always)]
fn value_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::RequireObjectCoercible { register } => if const { super::op_kind::admits(KIND, &[super::op_kind::REQUIRE_OBJECT_COERCIBLE]) } { require_object_coercible(stage, frame, register) } else { unreachable!() },
        Operation::LoadThis { destination } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_THIS]) } { load_this(stage, frame, destination, frame.miss()) } else { unreachable!() },
        Operation::LoadCallee { destination } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_CALLEE]) } { load_callee(stage, frame, destination) } else { unreachable!() },
        Operation::StringCaseIndex { destination, source, start, count } => if const { super::op_kind::admits(KIND, &[super::op_kind::STRING_CASE_INDEX]) } { string_case_index(stage, frame, destination, source, start, count) } else { unreachable!() },
        Operation::RequireIteratorResult { register } => if const { super::op_kind::admits(KIND, &[super::op_kind::REQUIRE_ITERATOR_RESULT]) } { super::iteration::require_iterator_result(stage, frame, register, frame.miss()) } else { unreachable!() },
        Operation::TypeofIndex { destination, source } => if const { super::op_kind::admits(KIND, &[super::op_kind::TYPEOF_INDEX]) } { super::control::typeof_index(stage, frame, destination, source) } else { unreachable!() },
        Operation::TypeofTest { destination, source, index, negated } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::TYPEOF_TEST]) } { super::control::typeof_test(stage, frame, destination, source, index, negated) } else { unreachable!() },
        Operation::TypeofString { destination, source } => if const { super::op_kind::admits(KIND, &[super::op_kind::TYPEOF_STRING]) } { super::control::typeof_string(stage, frame, destination, source) } else { unreachable!() },
        Operation::LoadEnvLocal { .. } | Operation::StoreEnvLocal { .. } | Operation::ArgumentsLength { .. } | Operation::ArgumentGet { .. }
            | Operation::GlobalTypeof { .. } | Operation::GlobalWrite { .. } | Operation::GlobalLexicalLoad { .. }
            | Operation::GlobalLexicalStore { .. } | Operation::Protocol { .. }
            | Operation::ForInNext { .. } | Operation::EnumeratedGet { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_ENV_LOCAL, super::op_kind::STORE_ENV_LOCAL, super::op_kind::ARGUMENTS_LENGTH, super::op_kind::ARGUMENT_GET, super::op_kind::GLOBAL_TYPEOF, super::op_kind::GLOBAL_WRITE, super::op_kind::GLOBAL_LEXICAL_LOAD, super::op_kind::GLOBAL_LEXICAL_STORE, super::op_kind::PROTOCOL, super::op_kind::FOR_IN_NEXT, super::op_kind::ENUMERATED_GET]) } { super::names::step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        heap => if const { super::op_kind::outside(KIND, &[super::op_kind::REQUIRE_OBJECT_COERCIBLE, super::op_kind::LOAD_THIS, super::op_kind::LOAD_CALLEE, super::op_kind::STRING_CASE_INDEX, super::op_kind::REQUIRE_ITERATOR_RESULT, super::op_kind::TYPEOF_INDEX, super::op_kind::TYPEOF_TEST, super::op_kind::TYPEOF_STRING, super::op_kind::LOAD_ENV_LOCAL, super::op_kind::STORE_ENV_LOCAL, super::op_kind::ARGUMENTS_LENGTH, super::op_kind::ARGUMENT_GET, super::op_kind::GLOBAL_TYPEOF, super::op_kind::GLOBAL_WRITE, super::op_kind::GLOBAL_LEXICAL_LOAD, super::op_kind::GLOBAL_LEXICAL_STORE, super::op_kind::PROTOCOL, super::op_kind::FOR_IN_NEXT, super::op_kind::ENUMERATED_GET]) } { super::heap::heap_step_of::<KIND, _, _>(stage, frame, heap) } else { unreachable!() },
    }
}

/// `value_step_of` for any operation.
#[inline(always)]
fn value_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    value_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
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

/// LoadCallee of `destination`: the frame's function word and fact; where the frame does not hold
/// it, leave at the miss edge, where the VM reads it.
#[inline(always)]
fn load_callee<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register) -> Step<S::Word, F::Control> {
    let Some((value, fact)) = frame.callee_value(stage) else { return Step::Unresolved };
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}

/// The operations that only move values between registers, locals and the stage.
#[inline(always)]
fn register_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Move { destination, source } => if const { super::op_kind::admits(KIND, &[super::op_kind::MOVE]) } { Step::Value(frame.move_register(stage, destination, source)) } else { unreachable!() },
        Operation::LoadConst { destination, constant } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_CONST]) } { {
            let (value, fact) = frame.constant(stage, constant);
            frame.write(stage, destination, value, fact);
            Step::Value(value)
        } } else { unreachable!() },
        Operation::LoadLocalPlain { .. } | Operation::StoreLocalPlain { .. } | Operation::LoadLocalChecked { .. }
            | Operation::StoreLocalChecked { .. } | Operation::InitializeLocal { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_LOCAL_PLAIN, super::op_kind::STORE_LOCAL_PLAIN, super::op_kind::LOAD_LOCAL_CHECKED, super::op_kind::STORE_LOCAL_CHECKED, super::op_kind::INITIALIZE_LOCAL]) } { local_step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        Operation::LogicalNot { destination, source } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOGICAL_NOT]) } { logical_not(stage, frame, destination, source) } else { unreachable!() },
        Operation::Nop => if const { super::op_kind::admits(KIND, &[super::op_kind::NOP]) } { Step::Value(stage.constant(0)) } else { unreachable!() },
        Operation::Safepoint { proof } => if const { super::op_kind::admits(KIND, &[super::op_kind::SAFEPOINT]) } { match frame.safepoint(stage, proof) {
            Some(()) => Step::Value(stage.constant(0)),
            None => Step::Unresolved,
        } } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::MOVE, super::op_kind::LOAD_CONST, super::op_kind::LOAD_LOCAL_PLAIN, super::op_kind::STORE_LOCAL_PLAIN, super::op_kind::LOAD_LOCAL_CHECKED, super::op_kind::STORE_LOCAL_CHECKED, super::op_kind::INITIALIZE_LOCAL, super::op_kind::LOGICAL_NOT, super::op_kind::NOP, super::op_kind::SAFEPOINT]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `register_step_of` for any operation.
#[inline(always)]
fn register_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    register_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// The operations that read or write a local through the frame.
#[inline(always)]
fn local_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        // A keeping activation's `arguments` binding is the frame's to answer (`Frame::arguments_binding`); any
        // other slot, or a frame that declines, is the slot's own read.
        Operation::LoadLocalPlain { destination, slot } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_LOCAL_PLAIN]) } { {
            let value = match read_arguments_binding(stage, frame, destination, slot) {
                Some(value) => Some(value),
                None => read_local(stage, frame, destination, slot, frame.local_miss()),
            };
            match value {
                Some(value) => Step::Value(value),
                None => Step::Unresolved,
            }
        } } else { unreachable!() },
        Operation::StoreLocalPlain { source, slot, result } => if const { super::op_kind::admits(KIND, &[super::op_kind::STORE_LOCAL_PLAIN]) } { {
            let Some(value) = write_local(stage, frame, source, slot, frame.local_miss()) else { return Step::Unresolved };
            if let Some(result) = result {
                let fact = frame.fact(source);
                frame.write(stage, result, value, fact);
            }
            Step::Value(value)
        } } else { unreachable!() },
        Operation::LoadLocalChecked { destination, slot, numeric } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_LOCAL_CHECKED]) } { {
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
        Operation::StoreLocalChecked { source, slot, result, initializing } => if const { super::op_kind::admits(KIND, &[super::op_kind::STORE_LOCAL_CHECKED]) } { {
            let miss = frame.miss();
            let stored = if initializing {
                write_local(stage, frame, source, slot, Some(miss))
            } else {
                write_initialized_local(stage, frame, source, slot, miss)
            };
            let Some(value) = stored else { return Step::Unresolved };
            if let Some(result) = result {
                let fact = frame.fact(source);
                frame.write(stage, result, value, fact);
            }
            Step::Value(value)
        } } else { unreachable!() },
        Operation::InitializeLocal { slot } => if const { super::op_kind::admits(KIND, &[super::op_kind::INITIALIZE_LOCAL]) } { {
            let hole = stage.constant(crate::Value::DELETED.to_bits());
            let miss = frame.local_miss();
            if frame.store_local(stage, slot, hole, miss) { Step::Value(hole) } else { Step::Unresolved }
        } } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::LOAD_LOCAL_PLAIN, super::op_kind::STORE_LOCAL_PLAIN, super::op_kind::LOAD_LOCAL_CHECKED, super::op_kind::STORE_LOCAL_CHECKED, super::op_kind::INITIALIZE_LOCAL]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `local_step_of` for any operation.
#[inline(always)]
fn local_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    local_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `IncDecLocal`: the `++` (or `--`) of `source` written to frame local `local`, by the Number step of an
/// int32 or binary64 operand, else the VM's update (`CallOut::IncDec`). Defines the updated value.
#[inline(always)]
fn inc_dec_local<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, local: Register, source: Register,
    decrement: bool) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let operation = if decrement { crate::vm::operations::NumberArithmetic::Subtract } else { crate::vm::operations::NumberArithmetic::Add };
    let operand = crate::bytecode::Operand::register(source);
    let (word, fact) = match frame.int_operand(stage, operand, miss) {
        Some(value) => {
            let one = stage.i32_constant(1);
            let (sum, overflowed) = stage.int_arithmetic(operation, value, one);
            match stage.guard_false(overflowed, miss) {
                Some(()) => (frame.int_result(stage, sum), frame.int_fact()),
                None => return inc_dec_local_by_vm(stage, frame, local, source, decrement),
            }
        }
        None => match frame.number_operand(stage, operand, miss) {
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
    match frame.vm_step(stage, super::CallOut::IncDec { source, decrement }, None) {
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
fn require_object_coercible<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
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

/// The part of `step` a row of one opcode can reach (`row_step`): a tier instantiating one handler
/// per opcode compiles only that part, chosen by an `if const` on the opcode, rather than every
/// family of `step` in every instantiation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowStep {
    /// Jumps, register moves and constants (`Decoder::Flow`).
    Flow,
    /// Plain local reads and writes (`Decoder::Local`).
    Local,
    /// The numeric operations (`NumberOperation`).
    Number,
    /// `this`, coercibility, `!` and `typeof` index.
    Value,
    /// The environment, global, capture and iteration operations (`names::step`).
    Names,
    /// Named property reads and writes.
    Property,
    /// Deletes and element accesses.
    Keyed,
    /// Comparisons, binaries, `void` and property keys (`generic::step`).
    Generic,
    /// The Wasm i32 and i64 integer operators (`Decoder::WasmInt`, `wasm_int::step`).
    WasmInt,
    /// Wasm float, float conversion and select operations (`wasm_float`).
    WasmFloat,
    /// The Wasm i32 conditional jumps and fused add-and-jump (`wasm_jump::step`).
    WasmJump,
    /// The Wasm instance's global loads and stores, and register fills (`wasm_state::step`).
    WasmState,
    /// The Wasm direct loads and stores of the function's memory (`wasm_access::step`).
    WasmMemory,
    /// The Wasm SIMD operators and byte shuffles on the lane's vector registers (`wasm_simd::step`).
    WasmVector,
    /// The activation entries of a lane's synthetic prologue records (`prologue::step`).
    Entry,
    /// A `Binary` or `Unary`: the Number operations, the generic comparisons and call-outs,
    /// `!` and the `typeof` forms (`step_arith`), not the whole of `step`.
    Arith,
    /// Any operation: the whole of `step`.
    Full,
}

#[cfg(not(stencil_build))]
/// The part of `step` opcode `op`'s row reaches.
pub(crate) const fn row_step(op: Op) -> RowStep {
    match op.staged() {
        Some(Decoder::Flow) => RowStep::Flow,
        Some(Decoder::Local) => RowStep::Local,
        Some(Decoder::WasmInt) => RowStep::WasmInt,
        Some(Decoder::Names) | Some(Decoder::Proved) => RowStep::Names,
        Some(Decoder::Property) => if matches!(op, Op::Delete) { RowStep::Keyed } else { RowStep::Property },
        Some(Decoder::Element) => RowStep::Keyed,
        Some(Decoder::WasmFloat) => RowStep::WasmFloat,
        Some(Decoder::Generic) => if matches!(op, Op::Unary | Op::Binary | Op::NumericAdd | Op::NumericMultiply) { RowStep::Arith } else { RowStep::Generic },
        Some(Decoder::WasmJump) => RowStep::WasmJump,
        Some(Decoder::WasmState) => RowStep::WasmState,
        Some(Decoder::WasmMemory) => RowStep::WasmMemory,
        Some(Decoder::WasmVector) => RowStep::WasmVector,
        Some(Decoder::Lane) => match op {
            Op::TierPrologue | Op::BodyEntry => RowStep::Entry,
            _ => RowStep::Full,
        },
        Some(Decoder::Value) => match op {
            Op::LoadCapture | Op::StoreCapture => RowStep::Names,
            Op::IncDec | Op::ToNumeric => RowStep::Number,
            // A unary decodes to the operations the arithmetic step runs (`decode_unary`): a `typeof`
            // test or name, `!`, a Number operator or the VM's.
            Op::Unary => RowStep::Arith,
            Op::LoadThis | Op::LoadCallee | Op::RequireObjectCoercible | Op::RequireIteratorResult => RowStep::Value,
            _ => RowStep::Full,
        },
        _ => RowStep::Full,
    }
}

#[cfg(not(stencil_build))]
/// `step` of a Wasm integer row (`RowStep::WasmInt`) on the interpreter's stage and frame; any
/// other operation is unresolved.
#[inline(always)]
pub(crate) fn step_wasm_int<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    match operation {
        Operation::WasmInt(operation) => super::wasm_int::step(stage, frame, operation),
        _ => Step::Unresolved,
    }
}

/// `step` of a flow row (`RowStep::Flow`); any other operation is unresolved.
#[inline(always)]
pub(crate) fn step_flow_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Jump => if const { super::op_kind::admits(KIND, &[super::op_kind::JUMP]) } { Step::Control(frame.jump(stage)) } else { unreachable!() },
        Operation::JumpIf { register, taken } => if const { super::op_kind::admits(KIND, &[super::op_kind::JUMP_IF]) } { jump_if(stage, frame, register, taken) } else { unreachable!() },
        Operation::Move { .. } | Operation::LoadConst { .. } | Operation::Nop => if const { super::op_kind::admits(KIND, &[super::op_kind::MOVE, super::op_kind::LOAD_CONST, super::op_kind::NOP]) } { register_step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        Operation::Throw { source } => if const { super::op_kind::admits(KIND, &[super::op_kind::THROW]) } { raise_step(stage, frame, source) } else { unreachable!() },
        Operation::Return { source } => if const { super::op_kind::admits(KIND, &[super::op_kind::RETURN]) } { return_step(stage, frame, source) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::JUMP, super::op_kind::JUMP_IF, super::op_kind::MOVE, super::op_kind::LOAD_CONST, super::op_kind::NOP, super::op_kind::THROW, super::op_kind::RETURN]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_flow_of` for any operation.
#[inline(always)]
pub(crate) fn step_flow<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_flow_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `Throw` of register `source`: the frame's raise as its control, where the frame raises it; a frame
/// that does not leaves it to the general step.
#[inline(always)]
fn raise_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, source: Register) -> Step<S::Word, F::Control> {
    match frame.raise(stage, source) {
        Some(control) => Step::Control(control),
        None => Step::Unresolved,
    }
}

/// `step` of a local row (`RowStep::Local`).
#[inline(always)]
pub(crate) fn step_local_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    local_step_of::<KIND, _, _>(stage, frame, operation)
}

/// `step_local_of` for any operation.
#[inline(always)]
pub(crate) fn step_local<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_local_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a numeric row (`RowStep::Number`).
#[inline(always)]
pub(crate) fn step_number_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Number(operation) => if const { super::op_kind::admits(KIND, &[super::op_kind::NUMBER]) } { numeric::step(stage, frame, operation) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::NUMBER]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_number_of` for any operation.
#[inline(always)]
pub(crate) fn step_number<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_number_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a value row (`RowStep::Value`).
#[inline(always)]
pub(crate) fn step_value_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::RequireObjectCoercible { register } => if const { super::op_kind::admits(KIND, &[super::op_kind::REQUIRE_OBJECT_COERCIBLE]) } { require_object_coercible(stage, frame, register) } else { unreachable!() },
        Operation::LoadThis { destination } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_THIS]) } { load_this(stage, frame, destination, frame.miss()) } else { unreachable!() },
        Operation::LoadCallee { destination } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_CALLEE]) } { load_callee(stage, frame, destination) } else { unreachable!() },
        Operation::RequireIteratorResult { register } => if const { super::op_kind::admits(KIND, &[super::op_kind::REQUIRE_ITERATOR_RESULT]) } { super::iteration::require_iterator_result(stage, frame, register, frame.miss()) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::REQUIRE_OBJECT_COERCIBLE, super::op_kind::LOAD_THIS, super::op_kind::LOAD_CALLEE, super::op_kind::REQUIRE_ITERATOR_RESULT]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_value_of` for any operation.
#[inline(always)]
pub(crate) fn step_value<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_value_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a names row (`RowStep::Names`).
#[inline(always)]
pub(crate) fn step_names_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    super::names::step_of::<KIND, _, _>(stage, frame, operation)
}

/// `step_names_of` for any operation.
#[inline(always)]
pub(crate) fn step_names<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_names_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a property row (`RowStep::Property`).
#[inline(always)]
pub(crate) fn step_property_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    super::property::property_step_of::<KIND, _, _>(stage, frame, operation)
}

/// `step_property_of` for any operation.
#[inline(always)]
pub(crate) fn step_property<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_property_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a keyed row (`RowStep::Keyed`).
#[inline(always)]
pub(crate) fn step_keyed_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    keyed_step_of::<KIND, _, _>(stage, frame, operation)
}

/// `step_keyed_of` for any operation.
#[inline(always)]
pub(crate) fn step_keyed<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_keyed_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

#[cfg(not(stencil_build))]
/// `step` of a Wasm float row (`RowStep::WasmFloat`) on the interpreter's stage and frame; any
/// other operation is unresolved.
#[inline(always)]
pub(crate) fn step_wasm_float<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    match operation {
        Operation::WasmFloat(operation) => super::wasm_float::step(stage, frame, operation),
        _ => Step::Unresolved,
    }
}

#[cfg(not(stencil_build))]
/// `step` of an instance-state row (`RowStep::WasmState`) on the interpreter's stage and frame.
#[inline(always)]
pub(crate) fn step_wasm_state<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    match operation {
        Operation::WasmState(operation) => super::wasm_state::step(stage, frame, operation),
        _ => Step::Unresolved,
    }
}

#[cfg(not(stencil_build))]
/// `step` of a direct memory row (`RowStep::WasmMemory`) on the interpreter's stage and frame.
#[inline(always)]
pub(crate) fn step_wasm_access<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    match operation {
        Operation::WasmAccess(operation) => super::wasm_access::step(stage, frame, operation),
        _ => Step::Unresolved,
    }
}

#[cfg(not(stencil_build))]
/// `step` of a Wasm vector row (`RowStep::WasmVector`) on the interpreter's stage and frame; any
/// other operation is unresolved.
#[inline(always)]
pub(crate) fn step_wasm_vector<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    match operation {
        Operation::WasmVector(operation) => super::wasm_simd::step(stage, frame, operation),
        _ => Step::Unresolved,
    }
}

/// `step` of a `Binary` or `Unary` row (`RowStep::Arith`): only the operations those decode to.
#[inline(always)]
pub(crate) fn step_arith_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Number(operation) => if const { super::op_kind::admits(KIND, &[super::op_kind::NUMBER]) } { numeric::step(stage, frame, operation) } else { unreachable!() },
        Operation::Void { .. } | Operation::Unary { .. } | Operation::PropertyKey { .. } | Operation::Equality { .. }
            | Operation::Binary { .. } | Operation::CompareJump { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::VOID, super::op_kind::UNARY, super::op_kind::PROPERTY_KEY, super::op_kind::EQUALITY, super::op_kind::BINARY, super::op_kind::COMPARE_JUMP]) } { super::generic::step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        Operation::LogicalNot { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOGICAL_NOT]) } { register_step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        Operation::TypeofIndex { destination, source } => if const { super::op_kind::admits(KIND, &[super::op_kind::TYPEOF_INDEX]) } { super::control::typeof_index(stage, frame, destination, source) } else { unreachable!() },
        Operation::TypeofTest { destination, source, index, negated } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::TYPEOF_TEST]) } { super::control::typeof_test(stage, frame, destination, source, index, negated) } else { unreachable!() },
        Operation::TypeofString { destination, source } => if const { super::op_kind::admits(KIND, &[super::op_kind::TYPEOF_STRING]) } { super::control::typeof_string(stage, frame, destination, source) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::NUMBER, super::op_kind::VOID, super::op_kind::UNARY, super::op_kind::PROPERTY_KEY, super::op_kind::EQUALITY, super::op_kind::BINARY, super::op_kind::COMPARE_JUMP, super::op_kind::LOGICAL_NOT, super::op_kind::TYPEOF_INDEX, super::op_kind::TYPEOF_TEST, super::op_kind::TYPEOF_STRING]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_arith_of` for any operation.
#[inline(always)]
pub(crate) fn step_arith<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_arith_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `step` of a generic row (`RowStep::Generic`).
#[inline(always)]
pub(crate) fn step_generic_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    super::generic::step_of::<KIND, _, _>(stage, frame, operation)
}

/// `step_generic_of` for any operation.
#[inline(always)]
pub(crate) fn step_generic<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_generic_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

#[cfg(not(stencil_build))]
/// `step` of a Wasm i32 jump row (`RowStep::WasmJump`).
#[inline(always)]
pub(crate) fn step_wasm_jump_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::WasmJump(jump) => if const { super::op_kind::admits(KIND, &[super::op_kind::WASM_JUMP]) } { super::wasm_jump::step(stage, frame, jump) } else { unreachable!() },
        Operation::BranchTable(table) => if const { super::op_kind::admits(KIND, &[super::op_kind::BRANCH_TABLE]) } { super::wasm_jump::branch_table(stage, frame, table) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::WASM_JUMP, super::op_kind::BRANCH_TABLE]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_wasm_jump_of` for any operation.
#[cfg(not(stencil_build))]
#[inline(always)]
pub(crate) fn step_wasm_jump<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_wasm_jump_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}
