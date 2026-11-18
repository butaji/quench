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
        Operation::WasmJump(jump) => super::wasm_jump::step(stage, frame, jump),
        call @ (Operation::Call(_) | Operation::Return { .. }) => call_step(stage, frame, call),
        Operation::Move { .. } | Operation::LoadConst { .. } | Operation::LoadLocalPlain { .. } | Operation::StoreLocalPlain { .. }
            | Operation::LoadLocalChecked { .. } | Operation::StoreLocalChecked { .. } | Operation::InitializeLocal { .. }
            | Operation::LogicalNot { .. } | Operation::Safepoint { .. } => register_step(stage, frame, operation),
        Operation::LoadEnvLocal { .. } | Operation::StoreEnvLocal { .. } | Operation::ArgumentsLength { .. }
            | Operation::GlobalTypeof { .. } | Operation::GlobalWrite { .. } | Operation::LoadName { .. } | Operation::Protocol { .. }
            | Operation::ForInNext { .. } | Operation::EnumeratedGet { .. } | Operation::SpreadCall { .. } | Operation::SuperCall { .. }
            | Operation::LoadCapture { .. } | Operation::StoreCapture { .. } | Operation::CloneEnv { .. } => super::names::step(stage, frame, operation),
        Operation::WasmInt(operation) => wasm_int::step(stage, frame, operation),
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

/// A call, a construction or a return: the call protocol's operations (`step`).
#[inline(always)]
fn call_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Call(CallOperation::Site(site)) => super::invoke::invoke(stage, frame, frame.miss(), site),
        Operation::Return { source } => super::invoke::ret(stage, frame, source),
        Operation::Call(operation) => super::protocol::step(stage, frame.miss(), operation),
        _ => unreachable!("`step` sends only calls and returns"),
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
            | Operation::GlobalTypeof { .. } | Operation::GlobalWrite { .. } | Operation::Protocol { .. }
            | Operation::ForInNext { .. } | Operation::EnumeratedGet { .. } => super::names::step(stage, frame, operation),
        heap => super::heap::heap_step(stage, frame, heap),
    }
}

/// LoadCallee of `destination`: the frame's function word and fact; where the frame does not hold
/// it, leave at the miss edge, where the VM reads it.
#[inline(always)]
fn load_callee<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register) -> Step<S::Word, F::Control> {
    let Some((value, fact)) = frame.callee(stage) else { return Step::Unresolved };
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
        Operation::LoadLocalChecked { destination, slot, numeric } => {
            if numeric {
                return Step::Unresolved;
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
    /// Any operation: the whole of `step`.
    Full,
}

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
        Some(Decoder::Generic) => if matches!(op, Op::Unary) { RowStep::Full } else { RowStep::Generic },
        Some(Decoder::WasmJump) => RowStep::WasmJump,
        Some(Decoder::Value) => match op {
            Op::LoadCapture | Op::StoreCapture => RowStep::Names,
            Op::IncDec | Op::ToNumeric => RowStep::Number,
            Op::LoadThis | Op::LoadCallee | Op::RequireObjectCoercible | Op::RequireIteratorResult => RowStep::Value,
            _ => RowStep::Full,
        },
        _ => RowStep::Full,
    }
}

/// `step` of a Wasm integer row (`RowStep::WasmInt`).
#[inline(always)]
pub(crate) fn step_wasm_int<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
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
        Operation::Move { .. } | Operation::LoadConst { .. } => register_step(stage, frame, operation),
        _ => Step::Unresolved,
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

/// `step` of a Wasm float row (`RowStep::WasmFloat`) on the interpreter's stage and frame; any
/// other operation is unresolved.
#[inline(always)]
pub(crate) fn step_wasm_float<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    match operation {
        Operation::WasmFloat(operation) => super::wasm_float::step(stage, frame, operation),
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
        _ => Step::Unresolved,
    }
}
