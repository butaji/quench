//! The staged meaning of each operation (`step`): one definition the interpreter runs with
//! `Concrete` and the tiers generate with `Emit`.
use super::*;

/// Run `operation` on `frame`; its word or where control goes.
#[inline(always)]
pub(crate) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Jump => Step::Control(frame.jump(stage)),
        Operation::JumpIf { register, taken } => jump_if(stage, frame, register, taken),
        Operation::Throw { source } => raise_step(stage, frame, source),
        Operation::Number(operation) => numeric::step(stage, frame, operation),
        property @ (Operation::GetField { .. } | Operation::SetField { .. } | Operation::StoreField { .. } | Operation::FieldGetHost { .. }
            | Operation::FieldAdd { .. } | Operation::IndexGetHost { .. }) => super::property::property_step(stage, frame, property),
        keyed @ (Operation::Delete { .. } | Operation::ElementRead { .. } | Operation::ElementReadOperand { .. } | Operation::ElementWrite { .. }) => keyed_step(stage, frame, keyed),
        Operation::IncDecLocal { local, source, decrement } => inc_dec_local(stage, frame, local, source, decrement),
        Operation::Void { .. } | Operation::Unary { .. } | Operation::PropertyKey { .. } | Operation::Equality { .. }
            | Operation::Binary { .. } | Operation::CompareJump { .. } => super::generic::step(stage, frame, operation),
        Operation::WasmJump(jump) => super::wasm_jump::step(stage, frame, jump),
        Operation::BranchTable(table) => super::wasm_jump::branch_table(stage, frame, table),
        call @ (Operation::Call(_) | Operation::Return { .. } | Operation::Suspend { .. }) => call_step(stage, frame, call),
        Operation::Move { .. } | Operation::LoadConst { .. } | Operation::LoadLocalPlain { .. } | Operation::StoreLocalPlain { .. }
            | Operation::LoadLocalChecked { .. } | Operation::StoreLocalChecked { .. } | Operation::InitializeLocal { .. }
            | Operation::LogicalNot { .. } | Operation::Safepoint { .. } | Operation::Nop => register_step(stage, frame, operation),
        Operation::LoadEnvLocal { .. } | Operation::StoreEnvLocal { .. } | Operation::ArgumentsLength { .. }
            | Operation::GlobalTypeof { .. } | Operation::GlobalWrite { .. } | Operation::GlobalLexicalLoad { .. }
            | Operation::GlobalLexicalStore { .. } | Operation::LoadName { .. } | Operation::Protocol { .. }
            | Operation::ForInNext { .. } | Operation::EnumeratedGet { .. } | Operation::SpreadCall { .. } | Operation::SuperCall { .. }
            | Operation::LoadCapture { .. } | Operation::StoreCapture { .. } | Operation::CloneEnv { .. }
            | Operation::IteratorCleanupPush { .. } | Operation::IteratorCleanupPop | Operation::SetFunctionName { .. }
            => super::names::step(stage, frame, operation),
        Operation::SuperConstArrayObject2(pair) => super::superinstructions::step(stage, frame, pair),
        guard @ (Operation::GuardIdentity { .. } | Operation::GuardLive | Operation::GuardRealm { .. } | Operation::GuardDeclared { .. }
            | Operation::GuardPrototype { .. } | Operation::GuardShape { .. } | Operation::GuardStores { .. } | Operation::BackEdge { .. }) => guard_step(stage, frame, guard),
        value => value_step(stage, frame, value),
    }
}

/// A tier guard, or a taken back edge's poll and count (`step`): `Step::Value` where the proof holds,
/// `Step::Unresolved` where the row leaves.
#[inline(always)]
/// The control a frame answers (`Frame::raise`), or `Unresolved` where it answers none.
#[inline(always)]
fn suspended<W, C>(control: Option<C>) -> Step<W, C> {
    match control {
        Some(control) => Step::Control(control),
        None => Step::Unresolved,
    }
}

fn guard_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let held = match operation {
        Operation::GuardIdentity { source, expected } => super::guard::identity(stage, source, expected, miss),
        Operation::GuardLive => super::guard::live(stage, frame, miss),
        Operation::GuardRealm { expected } => super::guard::realm(stage, frame, expected, miss),
        Operation::GuardDeclared { source, mask } => super::guard::declared(stage, source, mask, miss),
        Operation::GuardPrototype { source, records, shape, link } => super::guard::prototype(stage, records, source, (shape, link), miss),
        Operation::GuardShape { source, records, shape, form } => super::guard::shape(stage, records, source, (shape, form), miss),
        Operation::GuardStores { specialized } => super::guard::stores(stage, specialized, miss),
        Operation::BackEdge { proof } => super::guard::back_edge(stage, frame, proof),
        _ => unreachable!("`guard_step` sends only guards and back edges"),
    };
    match held {
        Some(()) => Step::Value(stage.constant(0)),
        None => Step::Unresolved,
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
        Operation::Return { source } => return_step(stage, frame, source),
        Operation::Call(operation) => super::protocol::step(stage, frame.miss(), operation),
        Operation::Suspend { kind, destination, value } => super::suspend::suspend(stage, frame, kind, destination, value),
        _ => unreachable!("`step` sends only calls, returns and suspensions"),
    }
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
fn value_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::RequireObjectCoercible { register } => require_object_coercible(stage, frame, register),
        Operation::LoadThis { destination } => load_this(stage, frame, destination, frame.miss()),
        Operation::LoadCallee { destination } => load_callee(stage, frame, destination),
        Operation::RequireIteratorResult { register } => super::iteration::require_iterator_result(stage, frame, register, frame.miss()),
        Operation::TypeofIndex { destination, source } => super::control::typeof_index(stage, frame, destination, source),
        Operation::LoadEnvLocal { .. } | Operation::StoreEnvLocal { .. } | Operation::ArgumentsLength { .. }
            | Operation::GlobalTypeof { .. } | Operation::GlobalWrite { .. } | Operation::GlobalLexicalLoad { .. }
            | Operation::GlobalLexicalStore { .. } | Operation::Protocol { .. }
            | Operation::ForInNext { .. } | Operation::EnumeratedGet { .. } => super::names::step(stage, frame, operation),
        Operation::StringCaseIndex { destination, source, start, count } => string_case_index(stage, frame, destination, source, start, count),
        heap => super::heap::heap_step(stage, frame, heap),
    }
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
fn register_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Move { destination, source } => Step::Value(frame.move_register(stage, destination, source)),
        Operation::LoadConst { destination, constant } => {
            let (value, fact) = frame.constant(stage, constant);
            frame.write(stage, destination, value, fact);
            Step::Value(value)
        }
        Operation::LoadLocalPlain { .. } | Operation::StoreLocalPlain { .. } | Operation::LoadLocalChecked { .. }
            | Operation::StoreLocalChecked { .. } | Operation::InitializeLocal { .. } => local_step(stage, frame, operation),
        Operation::LogicalNot { destination, source } => logical_not(stage, frame, destination, source),
        Operation::Nop => Step::Value(stage.constant(0)),
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
        // A keeping activation's `arguments` binding is the frame's to answer (`Frame::arguments_binding`); any
        // other slot, or a frame that declines, is the slot's own read.
        Operation::LoadLocalPlain { destination, slot } => {
            let value = match read_arguments_binding(stage, frame, destination, slot) {
                Some(value) => Some(value),
                None => read_local(stage, frame, destination, slot, frame.local_miss()),
            };
            match value {
                Some(value) => Step::Value(value),
                None => Step::Unresolved,
            }
        }
        Operation::StoreLocalPlain { source, slot, result } => {
            let Some(value) = write_local(stage, frame, source, slot, frame.local_miss()) else { return Step::Unresolved };
            if let Some(result) = result {
                let fact = frame.fact(source);
                frame.write(stage, result, value, fact);
            }
            Step::Value(value)
        }
        Operation::LoadLocalChecked { destination, slot, numeric } => {
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
        }
        Operation::StoreLocalChecked { source, slot, result, initializing } => {
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
        }
        Operation::InitializeLocal { slot } => {
            let hole = stage.constant(crate::Value::DELETED.to_bits());
            let miss = frame.local_miss();
            if frame.store_local(stage, slot, hole, miss) { Step::Value(hole) } else { Step::Unresolved }
        }
        _ => Step::Unresolved,
    }
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
        Some(Decoder::Generic) => if matches!(op, Op::Unary | Op::Binary) { RowStep::Arith } else { RowStep::Generic },
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
pub(crate) fn step_flow<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Jump => Step::Control(frame.jump(stage)),
        Operation::JumpIf { register, taken } => jump_if(stage, frame, register, taken),
        Operation::Move { .. } | Operation::LoadConst { .. } | Operation::Nop => register_step(stage, frame, operation),
        Operation::Throw { source } => raise_step(stage, frame, source),
        Operation::Return { source } => return_step(stage, frame, source),
        _ => Step::Unresolved,
    }
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
pub(crate) fn step_local<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    local_step(stage, frame, operation)
}

/// `step` of a numeric row (`RowStep::Number`).
#[inline(always)]
pub(crate) fn step_number<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Number(operation) => numeric::step(stage, frame, operation),
        _ => Step::Unresolved,
    }
}

/// `step` of a value row (`RowStep::Value`).
#[inline(always)]
pub(crate) fn step_value<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::RequireObjectCoercible { register } => require_object_coercible(stage, frame, register),
        Operation::LoadThis { destination } => load_this(stage, frame, destination, frame.miss()),
        Operation::LoadCallee { destination } => load_callee(stage, frame, destination),
        Operation::RequireIteratorResult { register } => super::iteration::require_iterator_result(stage, frame, register, frame.miss()),
        _ => Step::Unresolved,
    }
}

/// `step` of a names row (`RowStep::Names`).
#[inline(always)]
pub(crate) fn step_names<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    super::names::step(stage, frame, operation)
}

/// `step` of a property row (`RowStep::Property`).
#[inline(always)]
pub(crate) fn step_property<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    super::property::property_step(stage, frame, operation)
}

/// `step` of a keyed row (`RowStep::Keyed`).
#[inline(always)]
pub(crate) fn step_keyed<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    keyed_step(stage, frame, operation)
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
pub(crate) fn step_arith<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Number(operation) => numeric::step(stage, frame, operation),
        Operation::Void { .. } | Operation::Unary { .. } | Operation::PropertyKey { .. } | Operation::Equality { .. }
            | Operation::Binary { .. } | Operation::CompareJump { .. } => super::generic::step(stage, frame, operation),
        Operation::LogicalNot { .. } => register_step(stage, frame, operation),
        Operation::TypeofIndex { destination, source } => super::control::typeof_index(stage, frame, destination, source),
        Operation::TypeofTest { destination, source, index, negated } =>
            super::control::typeof_test(stage, frame, destination, source, index, negated),
        Operation::TypeofString { destination, source } => super::control::typeof_string(stage, frame, destination, source),
        _ => Step::Unresolved,
    }
}

/// `step` of a generic row (`RowStep::Generic`).
#[inline(always)]
pub(crate) fn step_generic<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    super::generic::step(stage, frame, operation)
}

/// `step` of a Wasm i32 jump row (`RowStep::WasmJump`).
#[inline(always)]
pub(crate) fn step_wasm_jump<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::WasmJump(jump) => super::wasm_jump::step(stage, frame, jump),
        Operation::BranchTable(table) => super::wasm_jump::branch_table(stage, frame, table),
        _ => Step::Unresolved,
    }
}
