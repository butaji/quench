//! The staged meaning of each operation (`step`): one definition the interpreter runs with
//! `Concrete` and the tiers generate with `Emit`.
use super::*;

#[path = "steps/keyed.rs"]
mod keyed;
#[path = "steps/callee.rs"]
mod callee;
#[path = "steps/values.rs"]
mod values;
#[path = "steps/locals.rs"]
mod locals;
#[path = "steps/wasm_rows.rs"]
mod wasm_rows;
#[path = "steps/flow_rows.rs"]
mod flow_rows;
#[path = "steps/entries.rs"]
mod entries;
pub use keyed::*;
pub use callee::*;
pub use values::*;
pub use locals::*;
pub use wasm_rows::*;
pub use flow_rows::*;
pub use entries::*;

/// `step` out of line, for a generated row whose opcode has no family step (`RowStep::Full`): one instance per
/// stage and frame type rather than the whole interpreter inlined into every such row's instance (`row_step!`).
/// The instance sits in its frame type's shard module (`NumericFrame::STEP_SHARD`): all instances of one function
/// share one codegen unit, and four lane frame instances of ~110 KB each in one unit were one thread for 74 s at
/// the end of a hot numeric edit.
#[inline(always)]
pub fn step_out_of_line<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let shard: fn(&mut S, &mut F, Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> =
        const { match F::STEP_SHARD % 4 { 0 => out_of_line::s0::step, 1 => out_of_line::s1::step, 2 => out_of_line::s2::step, _ => out_of_line::s3::step } };
    shard(stage, frame, operation)
}

mod out_of_line {
    use super::*;
    macro_rules! shards { ($($shard:ident)*) => { $(pub mod $shard {
        use super::*;
        #[inline(never)]
        pub fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
            super::super::step(stage, frame, operation)
        }
    })* }; }
    shards!(s0 s1 s2 s3);
}

/// Run `operation` on `frame`; its word or where control goes.
#[inline(always)]
pub fn step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
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
pub fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// The part of `step` a row of one opcode can reach (`row_step`): a tier instantiating one handler
/// per opcode compiles only that part, chosen by an `if const` on the opcode, rather than every
/// family of `step` in every instantiation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowStep {
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
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
