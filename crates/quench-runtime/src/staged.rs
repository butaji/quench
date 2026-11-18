//! Operation semantics written once, over a binding-time domain (`Stage`).
//!
//! This is Futamura's first projection made literal through Ershov's mixed computation. Each
//! operation below is one Rust function, generic over the stage that runs it:
//!
//! - The interpreter runs it on machine words (`Concrete`). Every condition is known there, so
//!   only the arm taken runs.
//! - A specializer runs it on IR values, which emits the residual code (`Emit` in
//!   `vm/compiled/object_slots.rs`). A condition it cannot know becomes a guard, or both arms.
//!
//! A plain Rust argument (a slot, a storage form a site proved) is static, so a branch on it
//! leaves no code. A rule written here is the rule of every tier, which is what makes the
//! native tiers the interpreter specialized, not a second implementation of it
//! (`docs/v2/specialization.md`). `staged_coverage` in this module's tests keeps hand-written
//! decodes of the same layouts from reappearing elsewhere.

use crate::bytecode::{Decoder, Op, Register, WideInstruction};
#[allow(unused_imports)] // the leaves' `use super::*` take these names
use crate::bytecode::{FieldBase, FieldLookup, Operand};
#[allow(unused_imports)]
use crate::typed_ir::Type;

mod concrete;
pub(crate) mod numeric;
pub(crate) mod flow;
pub(crate) mod machine;
#[cfg_attr(not(feature = "jit"), allow(dead_code))] // a specializer's composition
pub(crate) mod barrier;
#[cfg_attr(not(feature = "jit"), allow(dead_code))] // a specializer's operations; the interpreter has native Numbers
pub(crate) mod number_machine;
mod heap;
mod global;
#[cfg_attr(not(test), allow(dead_code))] // the division is data the specializers will ask; today only its tests do
pub(crate) mod division;
pub(crate) mod math;
mod names;
mod callout;
mod spread;
mod admission;
mod delete;
mod operation;
pub(crate) use operation::Operation;
mod decode;
mod captures;
pub(crate) mod compare;
pub(crate) use global::{lexical_read, GlobalBinding, GlobalFrame};
pub(crate) use names::Protocol;
pub(crate) use callout::{CallOut, OperandValues};
pub(crate) use spread::Passing;
pub(crate) use decode::{DecodeContext, Fields};
pub(crate) use compare::{Comparison, Nullish};
pub(crate) use concrete::Concrete;
pub(crate) use numeric::{Domain, NegativeZero, NumberOperation, NumericFrame, NumericStage, Overflow};
pub(crate) mod builtins;
pub(crate) mod add;
pub(crate) mod add_plan;
pub(crate) mod elements;
pub(crate) mod element_rows;
pub(crate) mod typed_elements;
pub(crate) mod typed_view;
mod element_frame;
pub(crate) use element_frame::ElementFrame;
pub(crate) mod entry;
pub(crate) mod environment;
pub(crate) mod ic;
pub(crate) mod object;
mod narrow;
pub(crate) mod store;
pub(crate) mod slot_store;
pub(crate) mod strings;
pub(crate) mod string_length;
pub(crate) mod holder;
pub(crate) mod iteration;
pub(crate) mod naming;
pub(crate) mod call; pub(crate) mod frame;
pub(crate) mod construct;
pub(crate) mod forms;
pub(crate) mod protocol;
pub(crate) use forms::{Arguments, CallFacts, Callee, FunctionId, Invocation, Receiver, RegisterList};
pub(crate) use protocol::CallOperation;
pub(crate) mod invoke;
pub(crate) mod plan;
mod native_call;
pub(crate) use plan::{plan_entry, CalleeKind, CallEntry, DirectPlan, Identity, Resolved};
#[cfg(test)]
pub(crate) use plan::Bound;
pub(crate) use invoke::{CallFrame, Entered, Room};
mod allocation;
pub(crate) use allocation::Allocation;
pub(crate) mod plan_fields;
pub(crate) mod property;
mod property_write;
pub(crate) use property::{NoPlans, PropertyFrame};
pub(crate) mod value;
mod steps;
mod control;
mod generic;
pub(crate) use steps::{step, row_step, RowStep, step_flow, step_wasm_int, step_local, step_number, step_value, step_names, step_property, step_keyed, step_generic, step_wasm_float, step_wasm_jump, step_wasm_state, step_wasm_access, step_wasm_vector, step_arith};
pub(crate) mod effects;
#[allow(dead_code)] // the K-PLAN builder; its first consumer is the u-jit lowering (P4, TOD-1700)
pub(crate) mod function_plan;
pub(crate) mod region;
pub(crate) mod wasm;
pub(crate) mod wasm_float;
pub(crate) mod wasm_int;
mod wasm_concrete;
pub(crate) mod wasm_call;
pub(crate) mod wasm_flow;
pub(crate) mod wasm_instance;
pub(crate) mod wasm_machine;
pub(crate) mod wasm_memory;
pub(crate) mod wasm_operation;
pub(crate) mod wasm_simd;
pub(crate) mod wasm_simd_concrete;
pub(crate) mod wasm_step;
pub(crate) mod wasm_jump;
pub(crate) use wasm_jump::{BranchTable, WasmJump};
pub(crate) mod wasm_state;
pub(crate) mod wasm_access;
pub(crate) use wasm_concrete::ConcreteWasm;
#[cfg(test)]
pub(crate) mod trace;
#[allow(unused_imports)] // the Stage v2 vocabulary: stencil and UltraJIT stages use the rest
pub(crate) use effects::{Effects, Region, RegionSet};
#[allow(unused_imports)]
pub(crate) use region::Access;
pub(crate) use ic::{HINTED_FORM, concrete_hinted_own_field_hit, concrete_own_field_hit, concrete_site_own_chain, concrete_site_own_read, concrete_site_store};
pub(crate) use object::{Form, Layout, Slots, concrete_own_read, arena_extras_layout};

mod stage;
pub(crate) use stage::{fill_by_rows, Passed, RecordSpace, Stage, Table, word_bytes, word_offset};

mod register_frame;
pub(crate) use register_frame::{Constant, Frame, Step, WasmRep, bound_this, jump_if, load_this, logical_not, read_initialized_local, read_local, write_initialized_local, write_local};

/// Where a row goes when its frame cannot decide a step: the VM's general step; for a capture
/// access whose scopes the frame does not hold, the walk over the scopes from the activation's
/// environment (a read) or the store into the scope it finds (a write); for a truth test of a
/// heap value, its cell's answer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Miss {
    General,
    WalkScopes,
    StoreThroughScopes,
    /// A conditional jump on a heap value whose cell decides its truthiness, jumping where the
    /// truthiness is `taken`.
    CellTruth { taken: bool },
    /// An `arguments` count the frame does not hold: the binding's own count, read from the
    /// frame or window that holds it.
    ArgumentsBinding,
    /// An `arguments` element the frame does not hold: the binding's element, read from the frame or
    /// window that holds it (`lane_frame_arguments` with its element flag).
    ArgumentsElement,
    /// An environment read the frame holds no storage for (an uncaptured or root frame, a
    /// mapped `arguments`, an environment window): the lane's local read (`load_local_miss`).
    EnvironmentLoad,
    /// An environment write the frame cannot make in place: the lane's local store
    /// (`store_local_miss`), which takes the binding rules.
    EnvironmentStore,
    /// A global-name store the frame does not hold (its hint misses): the name's first-run
    /// resolution, which resolves the store and holds the record on its row.
    StoreNameFirst,
    /// A comparison jump the frame leaves (a pair that is no Number relation or word test): the
    /// lane's comparison chain decides the relations it can without guest code.
    CompareChain,
    /// A `yield` the frame does not suspend in place (no generator runs the lane): the job root's
    /// suspension, or the general step (`yield_miss`).
    Yield,
    /// A `yield*` the frame does not suspend in place: the general step (`yield_star_miss`).
    YieldStar,
    /// A `Binary` whose pure value declines (a heap operand, BigInt, a numeric local target): the
    /// general operation made in place (`lane_binary_general`).
    Binary,
    /// `ToPropertyKey` of a value the frame leaves (an integer's decimal key, a symbol): the
    /// lane's own answer for it, else the general step.
    PropertyKeyValue,
    /// A checked local read the frame does not hold in its own locals (a temporal dead zone, a
    /// numeric target, a root body, an environment, a mapped `arguments`): the lane's local read
    /// (`load_local_miss`), which takes the binding rules.
    LocalLoad,
    /// A checked local store the frame does not hold in its own locals: the lane's local store
    /// (`store_local_miss`).
    LocalStore,
    /// An InitializeTdz the frame does not hold in its own locals: the lane's dead-zone opening
    /// (`lane_initialize_tdz`), which finds the binding wherever the frame keeps it.
    LocalInitialize,
    /// A by-name load the frame does not answer: its first run resolves the name, and a later
    /// run whose binding misses takes that binding's residual (`NameLoad`).
    NameLoad(NameLoad),
    /// A generic Wasm memory load the row leaves (a vector load, a memory the frame does not
    /// address directly, an out-of-bounds access): the lane's hand handler of the load, which
    /// completes it or raises its trap.
    WasmMemoryLoad,
    /// A generic Wasm memory store the row leaves: the lane's hand handler of the store.
    WasmMemoryStore,
    /// A generic Wasm effective address the row leaves: the lane's hand handler of the address.
    WasmMemoryAddress,
    /// A `Unary` the frame leaves (an operand its value declines): the lane's unary arm
    /// (`lane_unary`), which answers the pure operators and otherwise makes the operation in place.
    Unary,
    /// A `StringConcat` whose parts the pure concatenation declines (a part that converts with guest
    /// code, or a result past the string length limit): the lane's own concatenation of the parts
    /// (`denoted::string_concat_miss`), else the general step.
    Concat,
}

/// The by-name loads a `LoadName` family row answers: a plain read, a call's read with its
/// receiver (`LoadNameCall`), and `typeof`'s read of an absent name as `undefined`
/// (`LoadNameTypeof`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameLoad {
    Read,
    Call,
    Typeof,
}

/// The by-name load `op` is, or `None` for any other opcode.
pub(crate) const fn name_load(op: Op) -> Option<NameLoad> {
    match op {
        Op::LoadName => Some(NameLoad::Read),
        Op::LoadNameCall => Some(NameLoad::Call),
        Op::LoadNameTypeof => Some(NameLoad::Typeof),
        _ => None,
    }
}

/// The row of `op`'s miss route (`Miss`).
pub(crate) const fn miss(op: Op) -> Miss {
    match op {
        Op::LoadCapture => Miss::WalkScopes,
        Op::StoreCapture => Miss::StoreThroughScopes,
        Op::ArgumentsLength => Miss::ArgumentsBinding,
        Op::ArgumentGet => Miss::ArgumentsElement,
        Op::LoadEnvLocal => Miss::EnvironmentLoad,
        Op::StoreEnvLocal => Miss::EnvironmentStore,
        Op::StoreName => Miss::StoreNameFirst,
        Op::JumpBinaryFalse => Miss::CompareChain,
        Op::Binary => Miss::Binary,
        Op::ToPropertyKey => Miss::PropertyKeyValue,
        Op::LoadLocal => Miss::LocalLoad,
        Op::StoreLocal => Miss::LocalStore,
        Op::InitializeTdz => Miss::LocalInitialize,
        Op::WasmMemoryLoad => Miss::WasmMemoryLoad,
        Op::WasmMemoryStore => Miss::WasmMemoryStore,
        Op::WasmMemoryAddress => Miss::WasmMemoryAddress,
        Op::Unary => Miss::Unary,
        Op::StringConcat => Miss::Concat,
        Op::Yield => Miss::Yield,
        Op::YieldStar => Miss::YieldStar,
        _ => match name_load(op) {
            Some(load) => Miss::NameLoad(load),
            None => match truth_jump(op) {
                Some(taken) => Miss::CellTruth { taken },
                None => Miss::General,
            },
        },
    }
}

/// The adjacent opcode pairs a tier runs as one generated record (`dispatch_fast::fused_rows`):
/// row `.0` then row `.1`, with no dispatch between them. Each is a pair of generated rows the
/// lane meets back to back in loops: a local store or a move then the loop's back jump, a local
/// read then the test on it. No flagged row (`LaneFields::FLAG_ROLE`) is a member.
///
/// Each entry is `(A, B, MODE)`. `MODE` is one of the `FUSED_*` constants below.
pub(crate) const FUSED_ROWS: [(u16, u16, u8); 88] = [
    // An i32 add of a constant then the direct load at its sum (the load reads the register).
    (Op::WasmI32AddImmediate as u16, Op::WasmI32Load as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI64Load as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmF32Load as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmF64Load as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI32Load8S as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI32Load8U as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI32Load16S as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI32Load16U as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI64Load8S as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI64Load8U as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI64Load16S as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI64Load16U as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI64Load32S as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI64Load32U as u16, FUSED_PLAIN),
    // Two i32 adds: a strided loop's pointer and counter updates (the second may be the loop's step and test).
    (Op::WasmI32Add as u16, Op::WasmI32AddImmediateJumpNonZero as u16, FUSED_PLAIN),
    (Op::WasmI32Add as u16, Op::WasmI32AddImmediate as u16, FUSED_PLAIN),
    (Op::WasmI32Add as u16, Op::WasmI32Add as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI32AddImmediateJumpNonZero as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI32AddImmediate as u16, FUSED_PLAIN),
    (Op::WasmI32AddImmediate as u16, Op::WasmI32Add as u16, FUSED_PLAIN),
    // An i32 `and` with an immediate, then the register comparison jump on its result.
    (Op::WasmI32AndImmediate as u16, Op::WasmJumpI32Equal as u16, FUSED_PLAIN),
    (Op::WasmI32AndImmediate as u16, Op::WasmJumpI32NotEqual as u16, FUSED_PLAIN),
    // A constant then the direct Wasm store of it (the store reads the constant's register).
    (Op::LoadConst as u16, Op::WasmI32Store as u16, FUSED_PLAIN),
    (Op::LoadConst as u16, Op::WasmI64Store as u16, FUSED_PLAIN),
    (Op::LoadConst as u16, Op::WasmF32Store as u16, FUSED_PLAIN),
    (Op::LoadConst as u16, Op::WasmF64Store as u16, FUSED_PLAIN),
    (Op::LoadConst as u16, Op::WasmI32Store8 as u16, FUSED_PLAIN),
    (Op::LoadConst as u16, Op::WasmI32Store16 as u16, FUSED_PLAIN),
    (Op::LoadConst as u16, Op::WasmI64Store8 as u16, FUSED_PLAIN),
    (Op::LoadConst as u16, Op::WasmI64Store16 as u16, FUSED_PLAIN),
    (Op::LoadConst as u16, Op::WasmI64Store32 as u16, FUSED_PLAIN),
    (Op::StoreLocalPlain as u16, Op::Jump as u16, FUSED_PLAIN),
    (Op::Move as u16, Op::Jump as u16, FUSED_PLAIN),
    // An i32 add of a constant then the unconditional jump after it (a loop latch).
    (Op::WasmI32AddImmediate as u16, Op::Jump as u16, FUSED_PLAIN),
    (Op::LoadLocalPlain as u16, Op::JumpFalse as u16, FUSED_PLAIN),
    (Op::LoadLocalPlain as u16, Op::JumpTrue as u16, FUSED_PLAIN),
    (Op::ToNumeric as u16, Op::IncDec as u16, FUSED_PLAIN),
    (Op::IncDec as u16, Op::Jump as u16, FUSED_PLAIN),
    (Op::Binary as u16, Op::JumpFalse as u16, FUSED_PLAIN),
    (Op::Binary as u16, Op::JumpTrue as u16, FUSED_PLAIN),
    // Two arithmetic or bitwise operators, the second reading the first's result: `(a + b) * c`,
    // `(a + b) | 0`. Both rows run in full (`FUSED_PLAIN`); the hand pair's dead-write skip and its
    // raw result are dropped.
    (Op::Binary as u16, Op::Binary as u16, FUSED_PLAIN),
    // A `typeof` test then the branch on it (`if (typeof x === "number")`).
    (Op::Unary as u16, Op::JumpFalse as u16, FUSED_PLAIN),
    // `!(x instanceof C)` tests also take the `Binary` then `JumpTrue` pair above.
    // `c = c + 1` of a capture: the store, then the read of the same capture.
    (Op::StoreCapture as u16, Op::LoadCapture as u16, FUSED_PLAIN),
    // A bit-field extraction then an i32 operator (another extraction, a multiply, or an xor).
    (Op::WasmI32ShiftRightUnsignedAndImmediate as u16, Op::WasmI32ShiftRightUnsignedAndImmediate as u16, FUSED_PLAIN),
    (Op::WasmI32ShiftRightUnsignedAndImmediate as u16, Op::WasmI32Multiply as u16, FUSED_PLAIN),
    (Op::WasmI32ShiftRightUnsignedAndImmediate as u16, Op::WasmI32XorImmediate as u16, FUSED_PLAIN),
    // A scaled index: a shift left by a constant, then the add that takes it.
    (Op::WasmI32ShiftLeftImmediate as u16, Op::WasmI32Add as u16, FUSED_PLAIN),
    // A plain i32 store, then a move.
    (Op::WasmI32Store as u16, Op::Move as u16, FUSED_PLAIN),
    (Op::WasmI32Store8 as u16, Op::Move as u16, FUSED_PLAIN),
    (Op::WasmI32Store16 as u16, Op::Move as u16, FUSED_PLAIN),
    // Two moves in a row (a parallel copy resolved in order).
    (Op::Move as u16, Op::Move as u16, FUSED_PLAIN),
    // A move then a Wasm i32 zero test, which reads the register the move wrote.
    (Op::Move as u16, Op::WasmJumpI32Zero as u16, FUSED_PLAIN),
    (Op::Move as u16, Op::WasmJumpI32NonZero as u16, FUSED_PLAIN),
    // A move whose moved value the next i32 load takes as its address.
    (Op::Move as u16, Op::WasmI32Load as u16, FUSED_PLAIN),
    (Op::Move as u16, Op::WasmI32Load8S as u16, FUSED_PLAIN),
    (Op::Move as u16, Op::WasmI32Load8U as u16, FUSED_PLAIN),
    (Op::Move as u16, Op::WasmI32Load16S as u16, FUSED_PLAIN),
    (Op::Move as u16, Op::WasmI32Load16U as u16, FUSED_PLAIN),
    // An i32 load whose value the next branch table selects on (the table reads the register).
    (Op::WasmI32Load as u16, Op::WasmBranchTable as u16, FUSED_PLAIN),
    (Op::WasmI32Load8S as u16, Op::WasmBranchTable as u16, FUSED_PLAIN),
    (Op::WasmI32Load8U as u16, Op::WasmBranchTable as u16, FUSED_PLAIN),
    (Op::WasmI32Load16S as u16, Op::WasmBranchTable as u16, FUSED_PLAIN),
    (Op::WasmI32Load16U as u16, Op::WasmBranchTable as u16, FUSED_PLAIN),
    // A local or capture read then a register comparison jump (`JumpBinaryFalse`).
    (Op::LoadLocalPlain as u16, Op::JumpBinaryFalse as u16, FUSED_PLAIN),
    (Op::LoadCapture as u16, Op::JumpBinaryFalse as u16, FUSED_PLAIN),
    // A direct load then the direct store of its value, of the same width.
    (Op::WasmI32Load as u16, Op::WasmI32Store as u16, FUSED_PLAIN),
    (Op::WasmI32Load8U as u16, Op::WasmI32Store8 as u16, FUSED_PLAIN),
    (Op::WasmI64Load as u16, Op::WasmI64Store as u16, FUSED_PLAIN),
    // A pointer chase: an i32 load, then the i32 load its value addresses.
    (Op::WasmI32Load as u16, Op::WasmI32Load as u16, FUSED_PLAIN),
    (Op::WasmI32Load as u16, Op::WasmI32Load8S as u16, FUSED_PLAIN),
    (Op::WasmI32Load as u16, Op::WasmI32Load8U as u16, FUSED_PLAIN),
    (Op::WasmI32Load as u16, Op::WasmI32Load16S as u16, FUSED_PLAIN),
    (Op::WasmI32Load as u16, Op::WasmI32Load16U as u16, FUSED_PLAIN),
    // An i32 load then the zero test of its value (`WasmJumpI32Zero`, `WasmJumpI32NonZero`).
    (Op::WasmI32Load as u16, Op::WasmJumpI32Zero as u16, FUSED_PLAIN),
    (Op::WasmI32Load as u16, Op::WasmJumpI32NonZero as u16, FUSED_PLAIN),
    (Op::WasmI32Load8S as u16, Op::WasmJumpI32Zero as u16, FUSED_PLAIN),
    (Op::WasmI32Load8S as u16, Op::WasmJumpI32NonZero as u16, FUSED_PLAIN),
    (Op::WasmI32Load8U as u16, Op::WasmJumpI32Zero as u16, FUSED_PLAIN),
    (Op::WasmI32Load8U as u16, Op::WasmJumpI32NonZero as u16, FUSED_PLAIN),
    (Op::WasmI32Load16S as u16, Op::WasmJumpI32Zero as u16, FUSED_PLAIN),
    (Op::WasmI32Load16S as u16, Op::WasmJumpI32NonZero as u16, FUSED_PLAIN),
    (Op::WasmI32Load16U as u16, Op::WasmJumpI32Zero as u16, FUSED_PLAIN),
    (Op::WasmI32Load16U as u16, Op::WasmJumpI32NonZero as u16, FUSED_PLAIN),
    // `a[++i]`: an update then the keyed read it indexes.
    (Op::IncDec as u16, Op::GetIndex as u16, FUSED_PLAIN),
    // `x[t = i]`: a register copy then the keyed read of the copy's element.
    (Op::Move as u16, Op::GetIndex as u16, FUSED_PLAIN),
    // `t + K` then the element read or store its result addresses.
    (Op::Binary as u16, Op::GetIndex as u16, FUSED_PLAIN),
    (Op::Binary as u16, Op::SetIndex as u16, FUSED_PLAIN),
    // `a[i] = v; t = v` of a register copy of the stored value.
    (Op::SetIndex as u16, Op::Move as u16, FUSED_PLAIN),
];

/// Runs of three or four generated rows the lane runs as one record (`chain_rows`): each
/// entry's links in record order, `NO_LINK` padding a three-link chain. Only the last link may
/// be a jump. A chain at a record wins over a `FUSED_ROWS` pair there.
pub(crate) const FUSED_CHAINS: [[u16; 4]; 40] = [
    // A local or capture read, then a register comparison, then its branch.
    [Op::LoadLocalPlain as u16, Op::Binary as u16, Op::JumpFalse as u16, NO_LINK],
    [Op::LoadLocalPlain as u16, Op::Binary as u16, Op::JumpTrue as u16, NO_LINK],
    [Op::LoadCapture as u16, Op::Binary as u16, Op::JumpFalse as u16, NO_LINK],
    [Op::LoadCapture as u16, Op::Binary as u16, Op::JumpTrue as u16, NO_LINK],
    // An i32 load, an i32 operator on the loaded value, then the i32 store of its result.
    [Op::WasmI32Load as u16, Op::WasmI32Add as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32Subtract as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32Multiply as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32And as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32Or as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32Xor as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32ShiftLeft as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32ShiftRightSigned as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32ShiftRightUnsigned as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32Equal as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32NotEqual as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32LessSigned as u16, Op::WasmI32Store as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32LessUnsigned as u16, Op::WasmI32Store as u16, NO_LINK],
    // An i32 add of a constant, an f64 load at its sum, then an f64 operator on the loaded value.
    [Op::WasmI32AddImmediate as u16, Op::WasmF64Load as u16, Op::WasmF64Binary as u16, NO_LINK],
    // `c = (c + 1) | 0` of a capture: its read, the operator, the truncation, then the store.
    [Op::LoadCapture as u16, Op::Binary as u16, Op::Binary as u16, Op::StoreCapture as u16],
    // A move pair then the unconditional jump after it (a parallel copy at a loop's latch).
    [Op::Move as u16, Op::Move as u16, Op::Jump as u16, NO_LINK],
    // `(c ± k) & m` then an immediate comparison jump on the masked value (`c - '0'` digit tests).
    [Op::WasmI32AddImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32EqualImmediate as u16, NO_LINK],
    [Op::WasmI32AddImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32NotEqualImmediate as u16, NO_LINK],
    [Op::WasmI32AddImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32LessUnsignedImmediate as u16, NO_LINK],
    [Op::WasmI32AddImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32LessEqualUnsignedImmediate as u16, NO_LINK],
    [Op::WasmI32AddImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32GreaterUnsignedImmediate as u16, NO_LINK],
    [Op::WasmI32AddImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32GreaterEqualUnsignedImmediate as u16, NO_LINK],
    [Op::WasmI32SubtractImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32EqualImmediate as u16, NO_LINK],
    [Op::WasmI32SubtractImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32NotEqualImmediate as u16, NO_LINK],
    [Op::WasmI32SubtractImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32LessUnsignedImmediate as u16, NO_LINK],
    [Op::WasmI32SubtractImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32LessEqualUnsignedImmediate as u16, NO_LINK],
    [Op::WasmI32SubtractImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32GreaterUnsignedImmediate as u16, NO_LINK],
    [Op::WasmI32SubtractImmediate as u16, Op::WasmI32AndImmediate as u16, Op::WasmJumpI32GreaterEqualUnsignedImmediate as u16, NO_LINK],
    // Two i32 loads of one width, whose values one i32 operator combines.
    [Op::WasmI32Load as u16, Op::WasmI32Load as u16, Op::WasmI32Add as u16, NO_LINK],
    [Op::WasmI32Load as u16, Op::WasmI32Load as u16, Op::WasmI32Multiply as u16, NO_LINK],
    [Op::WasmI32Load16S as u16, Op::WasmI32Load16S as u16, Op::WasmI32Multiply as u16, NO_LINK],
    [Op::WasmI32Load16U as u16, Op::WasmI32Load16U as u16, Op::WasmI32Multiply as u16, NO_LINK],
    [Op::WasmI32Load8U as u16, Op::WasmI32Load8U as u16, Op::WasmI32Add as u16, NO_LINK],
    // `a[++i + K]`: the update, the keyed read it indexes, then the Number operation on the read
    //.
    [Op::IncDec as u16, Op::GetIndex as u16, Op::Binary as u16, NO_LINK],
    // `heap[(p + K) >> S]` and `heap[p + K]`: the address sum, its shift, then the element read or
    // store it addresses.
    [Op::Binary as u16, Op::Binary as u16, Op::GetIndex as u16, NO_LINK],
    [Op::Binary as u16, Op::Binary as u16, Op::SetIndex as u16, NO_LINK],
];

/// The absent fourth link of a three-link `FUSED_CHAINS` entry.
pub(crate) const NO_LINK: u16 = u16::MAX;

/// `FUSED_ROWS` mode: both rows run in full, `A`'s result is written to its register.
pub(crate) const FUSED_PLAIN: u8 = 0;
/// `FUSED_ROWS` mode: `A`'s result register is dead after the pair, and `B` does not name it, so
/// the fused record skips `A`'s register write. `fuse_rows` installs it only where liveness
/// proves that; otherwise the pair runs as `FUSED_PLAIN`.
pub(crate) const FUSED_DEAD_FIRST: u8 = 1;
/// The mode of a window pair or chain `fuse_rows` installs for a window local's (`FLAG_WINDOW`) first row: both
/// rows run at the window's flag, reading the window's locals. Not a table mode; `fuse_rows` names it.
pub(crate) const FUSED_WINDOW: u8 = 3;

/// The opcode whose row answers an arguments binding's count (`Miss::ArgumentsBinding`), which a
/// tier names to give the row a fact it proved at view time.
pub(crate) const ARGUMENTS_COUNT_ROW: u16 = Op::ArgumentsLength as u16;
/// The opcode whose row answers an `arguments` element (`Frame::argument`), which the lane's record
/// names its register base for (`LaneFrame::kept`).
pub(crate) const ARGUMENT_GET_ROW: u16 = Op::ArgumentGet as u16;
/// The opcode whose row stores an environment local (`Operation::StoreEnvLocal`), which a window whose names
/// resolve through its scope runs with its scope's store first (`Frame::scoped_local_store`).
pub(crate) const STORE_ENV_LOCAL_ROW: u16 = Op::StoreEnvLocal as u16;

/// The opcode whose row answers a `Unary` record: `+x`, `-x`, `!x`, `~x` and the index form of
/// `typeof` decode here; a `typeof` test of a name runs the general step.
pub(crate) const UNARY_ROW: u16 = Op::Unary as u16;
/// The opcode whose row stores a global name (`GlobalFrame::global_write`), which a tier names to
/// give the row its record's hint.
pub(crate) const GLOBAL_WRITE_ROW: u16 = Op::StoreName as u16;
/// The opcode whose row reads the running function (`Frame::callee`), which a tier names to give
/// the row the function it runs.
pub(crate) const LOAD_CALLEE_ROW: u16 = Op::LoadCallee as u16;
/// The opcode whose row clones the running activation's environment (`Operation::CloneEnv`), which
/// a tier names to give the row its environment window.
pub(crate) const CLONE_ENV_ROW: u16 = Op::CloneEnv as u16;
/// The opcode whose row reads a capture (`Operation::LoadCapture`), which a tier names to find the
/// captures a loop reads.
pub(crate) const LOAD_CAPTURE_ROW: u16 = Op::LoadCapture as u16;
/// The opcode whose row suspends a promise job on its awaited value (`Operation::Await`), which a lane
/// frame answers and a plan leaves to its suspend exit.
pub(crate) const AWAIT_ROW: u16 = Op::Await as u16;
/// The opcode whose row yields from the running generator (`Operation::Yield`), which a lane frame
/// answers in place and otherwise leaves to its miss.
pub(crate) const YIELD_ROW: u16 = Op::Yield as u16;
/// The opcode whose row delegates to an iterable from the running generator (`Operation::YieldStar`).
pub(crate) const YIELD_STAR_ROW: u16 = Op::YieldStar as u16;
/// The opcode whose row answers a string-case selector (`Operation::StringCaseIndex`), which a tier
/// names to instantiate the row of a selector it filtered (`string_case_filter::specialize`).
pub(crate) const STRING_CASE_INDEX_ROW: u16 = Op::StringCaseIndex as u16;
/// The opcode whose row is an unconditional jump (`Operation::Jump`).
pub(crate) const JUMP_ROW: u16 = Op::Jump as u16;
/// The opcode whose row defines a register from a constant (`Operation::LoadConst`), which the
/// interpreter's constant table answers where the decode does not hold it.
pub(crate) const LOAD_CONST_ROW: u16 = Op::LoadConst as u16;
/// The opcode whose row writes a local slot from a register (`Operation::StoreLocalPlain`), which the
/// interpreter names where it records the local write of a fused binary.
pub(crate) const STORE_LOCAL_PLAIN_ROW: u16 = Op::StoreLocalPlain as u16;
/// The opcode whose row makes a closure (`Operation::Allocate` over `Allocation::Closure`).
pub(crate) const MAKE_CLOSURE_ROW: u16 = Op::MakeClosure as u16;

/// Whether `op` is a conditional jump whose row answers its control edge from a comparison of
/// two operands (`Operation::CompareJump`): a jump the lane runs as its generated row.
pub(crate) const fn compare_jump(op: Op) -> bool {
    matches!(op, Op::JumpBinaryFalse)
}

/// Whether `op` is a Wasm i32 conditional jump or fused add-and-jump (`Operation::WasmJump`): a
/// jump the lane runs as its generated row.
pub(crate) const fn wasm_i32_jump(op: Op) -> bool {
    matches!(op.staged(), Some(Decoder::WasmJump))
}

/// Whether `op` is a Wasm branch table (`Operation::BranchTable`): a jump the lane runs as its
/// generated row, whose edge is the entry its index picks (`Edge::Entry`).
pub(crate) const fn wasm_branch_table(op: Op) -> bool {
    matches!(op, Op::WasmBranchTable)
}

/// The Wasm record forms module validation checks against the module's declarations, one per
/// record meaning (`vm::program_store`'s admission of a module's functions).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum WasmRecordCheck {
    /// A function reference (`ref.func`), which names a declared signature.
    RefFunc,
    /// An indirect call target, which names a type signature.
    IndirectTarget,
    /// A GC descriptor read (`ref.get_desc`), which names a descriptor type.
    RefGetDesc,
    /// An array segment construction (`array.new_data`, `array.new_elem`).
    ArraySegment,
    /// An array construction (`array.new`, its defaulted and fixed forms).
    ArrayNew,
    /// A descriptor test or cast, which names a reference target's descriptor.
    DescriptorReference,
    /// A reference test or cast, which names a reference target's reference type.
    ReferenceTest,
    /// Any other record: nothing module validation checks.
    Other,
}

/// The validation form of `op` (`WasmRecordCheck`): the Wasm GC and reference records whose
/// immediate and registers the module's declarations must admit.
pub(crate) const fn wasm_record_check(op: Op) -> WasmRecordCheck {
    match op {
        Op::WasmRefFunc => WasmRecordCheck::RefFunc,
        Op::WasmIndirectTarget => WasmRecordCheck::IndirectTarget,
        Op::WasmRefGetDesc => WasmRecordCheck::RefGetDesc,
        Op::WasmArrayNewData | Op::WasmArrayNewElem => WasmRecordCheck::ArraySegment,
        Op::WasmArrayNew | Op::WasmArrayNewDefault | Op::WasmArrayNewFixed => WasmRecordCheck::ArrayNew,
        Op::WasmDescriptorTest | Op::WasmDescriptorCast => WasmRecordCheck::DescriptorReference,
        Op::WasmRefTest | Op::WasmRefCast => WasmRecordCheck::ReferenceTest,
        _ => WasmRecordCheck::Other,
    }
}

/// Whether `op` builds a defaulted Wasm array (`array.new_default`), whose element type must be
/// defaultable.
pub(crate) const fn wasm_array_new_defaulted(op: Op) -> bool {
    matches!(op, Op::WasmArrayNewDefault)
}

/// Whether `op` builds a fixed-length Wasm array (`array.new_fixed`), whose elements are a
/// register window of the function.
pub(crate) const fn wasm_array_new_fixed(op: Op) -> bool {
    matches!(op, Op::WasmArrayNewFixed)
}

/// Whether `instruction` makes a constant array (`Allocation::ConstArray`), which a superinstruction
/// fuses at its head.
pub(crate) fn makes_const_array(instruction: WideInstruction) -> bool {
    matches!(Allocation::decode(instruction, &[]), Some((_, Allocation::ConstArray { .. })))
}

/// Whether `instruction` makes an object of two fields (`Allocation::ObjectPair`), which a
/// superinstruction fuses at its tail.
pub(crate) fn makes_object_pair(instruction: WideInstruction) -> bool {
    matches!(Allocation::decode(instruction, &[]), Some((_, Allocation::ObjectPair { .. })))
}

/// Whether `op` is a generic binary operation (`Binary`), the record a fused pair is built from.
pub(crate) const fn binary(op: Op) -> bool {
    matches!(op, Op::Binary)
}

/// Whether `op` is a conditional jump on a truthy or falsy value (`JumpTrue`, `JumpFalse`) or on a
/// comparison (`JumpBinaryFalse`): the guards a function's census counts.
pub(crate) const fn conditional_jump(op: Op) -> bool {
    matches!(op, Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse)
}

/// Whether `op` copies a register (`Move`).
pub(crate) const fn copies_register(op: Op) -> bool {
    matches!(op, Op::Move)
}

/// Whether `op` is a Wasm SIMD operator (`WasmSimd`) on the lane's vector registers.
pub(crate) const fn wasm_simd(op: Op) -> bool {
    matches!(op, Op::WasmSimd)
}

/// Whether `op` is a Wasm `i8x16.shuffle` (`WasmSimdShuffle`), whose lane indices are its constant.
pub(crate) const fn wasm_simd_shuffle(op: Op) -> bool {
    matches!(op, Op::WasmSimdShuffle)
}

/// Whether `op` is a Wasm effective address (`WasmMemoryAddress`) of the function's memory.
pub(crate) const fn wasm_memory_address(op: Op) -> bool {
    matches!(op, Op::WasmMemoryAddress)
}

/// Whether `op` is a generic Wasm memory load (`WasmMemoryLoad`) of the function's memory.
pub(crate) const fn wasm_memory_load(op: Op) -> bool {
    matches!(op, Op::WasmMemoryLoad)
}

/// Whether `op` is a generic Wasm memory store (`WasmMemoryStore`) of the function's memory.
pub(crate) const fn wasm_memory_store(op: Op) -> bool {
    matches!(op, Op::WasmMemoryStore)
}

/// The opcodes of the Wasm i32 zero tests, which the lane names to give a zero test's row the
/// handler of its direction (`jump_row`).
pub(crate) const WASM_I32_ADD_IMMEDIATE_ROW: u16 = Op::WasmI32AddImmediate as u16;
/// The opcodes of the by-name load rows, one per `NameLoad`, which a tier names to instantiate
/// the row of a binding it resolved (`lane_frame::name_row`).
pub(crate) const LOAD_NAME_ROW: u16 = Op::LoadName as u16;
pub(crate) const LOAD_NAME_CALL_ROW: u16 = Op::LoadNameCall as u16;
pub(crate) const LOAD_NAME_TYPEOF_ROW: u16 = Op::LoadNameTypeof as u16;
/// The opcodes whose rows are the checked local accesses (`checked_local`), which a window's
/// specialization recognizes by their row.
pub(crate) const LOCAL_LOAD_ROW: u16 = Op::LoadLocal as u16;
pub(crate) const LOCAL_STORE_ROW: u16 = Op::StoreLocal as u16;
pub(crate) const LOCAL_INITIALIZE_ROW: u16 = Op::InitializeTdz as u16;
/// The opcodes whose rows read and write the running captured environment's slots (`LoadEnvLocal`,
/// `StoreEnvLocal`), which a frame's specialization rewrites where the slot is not captured.
pub(crate) const LOAD_ENV_LOCAL_ROW: u16 = Op::LoadEnvLocal as u16;

/// The local slot a local-access record names. An environment window's record carries its register
/// count above the slot (`dispatch_fast::specialize_environment_window`), so the slot is the low half
/// of its immediate; every local decoder reads it through here.
pub(crate) const fn window_slot(imm: u32) -> u16 { (imm & u16::MAX as u32) as u16 }

/// The register base of the environment window a local-access record names, where its immediate
/// carries one (`window_slot` names the slot above it); `0` where none is named.
pub(crate) const fn window_base(imm: u32) -> u16 { (imm >> u16::BITS) as u16 }

/// Whether `op` is a checked local access: its row reads and writes the frame's own locals in
/// place, and leaves to its hand handler (`Miss`) for any slot those locals do not hold.
pub(crate) const fn checked_local(op: Op) -> bool {
    matches!(op, Op::LoadLocal | Op::StoreLocal | Op::InitializeTdz)
}


/// Whether `op`'s row writes its result register itself when it answers a value. A keyed for-in
/// read (`EnumeratedGet`) answers the word it read and leaves the write to the tier; a global
/// store has no result register, and answers the source word it stored; a store's result is its
/// `b` register and an InitializeTdz has none.
pub(crate) const fn row_writes_result(op: Op) -> bool {
    // A Wasm row writes its result register only where its record has one: a store, a global
    // store and a fill define none.
    if matches!(op.staged(), Some(Decoder::WasmState | Decoder::WasmMemory)) {
        return matches!(op.field_layout(crate::bytecode::InstructionField::A), crate::bytecode::FieldLayout::ResultRegister);
    }
    // A keyed read's answer is written by the lane (its record's result register, as every
    // producer writes it beside the carried value).
    !matches!(op, Op::GetEnumerated | Op::GetIndex | Op::StoreName | Op::StoreLocal | Op::InitializeTdz | Op::DefineArrayElement
        | Op::IteratorCleanupPush | Op::IteratorCleanupPop)
}

/// The truthiness on which `op` jumps, for a jump on its register's truthiness
/// (`Operation::JumpIf`'s `taken`).
pub(crate) const fn truth_jump(op: Op) -> Option<bool> {
    match op {
        Op::JumpFalse => Some(false),
        Op::JumpTrue => Some(true),
        _ => None,
    }
}

/// The `Op` of the `*_ROW` constant `row`, the bridge from a row constant to the opcode a record is
/// built with (`WideInstruction::new`). Panics on a constant no opcode carries.
pub(crate) const fn row_op(row: u16) -> Op {
    match Op::from_index(row as usize) {
        Some(op) => op,
        None => panic!("a staged row constant names no opcode"),
    }
}

/// The `*_ROW` constants of the records a tier builds: a call, a move, a named field read and its
/// stores. `row_op` gives the record's `Op`.
pub(crate) const CALL_ROW: u16 = Op::Call as u16;
pub(crate) const MOVE_ROW: u16 = Op::Move as u16;
pub(crate) const GET_FIELD_ROW: u16 = Op::GetField as u16;
pub(crate) const SET_FIELD_ROW: u16 = Op::SetField as u16;
pub(crate) const SET_THIS_FIELD_ROW: u16 = Op::SetThisField as u16;

// Meaning queries: what the staged rows say `op` does, one query per meaning a tier lowers. A
// tier asks the query instead of naming opcodes; each answers exactly its row family.

/// Reads a named field (`GetField`).
pub(crate) const fn reads_named_field(op: Op) -> bool { matches!(op, Op::GetField) }
/// Reads an element by index (`GetIndex`).
pub(crate) const fn reads_element(op: Op) -> bool { matches!(op, Op::GetIndex) }
/// Stores an element by index (`SetIndex`).
pub(crate) const fn writes_element(op: Op) -> bool { matches!(op, Op::SetIndex) }
/// Accesses an element through its object register as an indexed receiver (`GetIndex`,
/// `SetIndex`, `RequireObjectCoercible`).
pub(crate) const fn indexes_receiver(op: Op) -> bool { matches!(op, Op::GetIndex | Op::SetIndex | Op::RequireObjectCoercible) }
/// Computes a binary operator of two registers (`Binary`).
pub(crate) const fn computes_binary(op: Op) -> bool { matches!(op, Op::Binary) }
/// Computes an arithmetic binary operator (`Binary`, `NumericAdd`, `NumericMultiply`).
pub(crate) const fn computes_arithmetic(op: Op) -> bool { matches!(op, Op::Binary | Op::NumericAdd | Op::NumericMultiply) }
/// Computes a unary operator (`Unary`).
pub(crate) const fn computes_unary(op: Op) -> bool { matches!(op, Op::Unary) }
/// Defines a register from a constant (`LoadConst`).
pub(crate) const fn loads_constant(op: Op) -> bool { matches!(op, Op::LoadConst) }
/// Converts a value to a Number (`ToNumeric`).
pub(crate) const fn converts_numeric(op: Op) -> bool { matches!(op, Op::ToNumeric) }
/// Increments or decrements a Number (`IncDec`).
pub(crate) const fn increments(op: Op) -> bool { matches!(op, Op::IncDec) }
/// Makes a call (`Call`).
pub(crate) const fn calls(op: Op) -> bool { matches!(op, Op::Call) }
/// Makes a call of an argument array (`CallArray`).
pub(crate) const fn calls_array(op: Op) -> bool { matches!(op, Op::CallArray) }
/// Makes an ordinary call (`Call`, `CallArray`).
pub(crate) const fn calls_ordinary(op: Op) -> bool { matches!(op, Op::Call | Op::CallArray) }
/// Continues at the jump target (`Jump`).
pub(crate) const fn jumps(op: Op) -> bool { matches!(op, Op::Jump) }
/// Completes the activation (`Return`).
pub(crate) const fn returns(op: Op) -> bool { matches!(op, Op::Return) }
/// Reads a captured slot (`LoadCapture`).
pub(crate) const fn reads_capture(op: Op) -> bool { matches!(op, Op::LoadCapture) }
/// Reads a binding by name (`LoadName`).
pub(crate) const fn reads_name(op: Op) -> bool { matches!(op, Op::LoadName) }
/// Reads an environment slot (`LoadEnvLocal`).
pub(crate) const fn reads_environment_slot(op: Op) -> bool { matches!(op, Op::LoadEnvLocal) }
/// Defines an element of an array literal (`DefineArrayElement`).
pub(crate) const fn defines_array_element(op: Op) -> bool { matches!(op, Op::DefineArrayElement) }
/// Refuses `null` and `undefined` (`RequireObjectCoercible`).
pub(crate) const fn requires_object_coercible(op: Op) -> bool { matches!(op, Op::RequireObjectCoercible) }
/// Writes `this` as a field (`SetThisField`).
pub(crate) const fn writes_this_field(op: Op) -> bool { matches!(op, Op::SetThisField) }


/// The instructions a tier emits when it lowers a unit. Staged names their opcodes, so a lowering
/// builds the record of a row here and never names an opcode itself.
pub(crate) mod emit {
    use crate::bytecode::{Op, Register, WideInstruction};

    /// A marker that does nothing: an inlined call's admission point.
    pub(crate) fn nop() -> WideInstruction { WideInstruction::new(Op::Nop, 0, 0, 0, 0) }

    /// Continue at `target`.
    pub(crate) fn jump(target: u32) -> WideInstruction { WideInstruction::new(Op::Jump, 0, 0, 0, target) }

    /// Copy register `source` into `destination`.
    pub(crate) fn move_register(destination: Register, source: Register) -> WideInstruction {
        WideInstruction::new(Op::Move, destination, source, 0, 0)
    }

    /// A call through its callee register `b`, with the window and immediate of its arguments.
    pub(crate) fn call(a: Register, b: Register, c: Register, imm: u32) -> WideInstruction {
        WideInstruction::new(Op::Call, a, b, c, imm)
    }

    /// A construction with its callee in `b` and its arguments' window in `c`.
    pub(crate) fn construct(a: Register, b: Register, c: Register, imm: u32) -> WideInstruction {
        WideInstruction::new(Op::Construct, a, b, c, imm)
    }

    /// A named read of the lookup `imm` (its cache site and atom) into `a`, from base `b`.
    pub(crate) fn get_field(a: Register, b: Register, c: Register, imm: u32) -> WideInstruction {
        WideInstruction::new(Op::GetField, a, b, c, imm)
    }
}

/// Which of an instruction's two control edges it takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Edge {
    /// The jump target.
    Target,
    /// The record leaves the lane run: its activation suspended into its job (`Frame::suspend_await`).
    Leave,
    /// The record switched the running register file: its frame made a suspension that transfers
    /// control to another activation (`Frame::suspend_yield`), which the frame holds for the lane to resume.
    Transfer,
    /// The instruction after this one.
    Following,
    /// The entry a branch table's index picks (`k`, clamped to the default entry): control goes to
    /// the jump record `k` places after the table record.
    Entry(u32),
}

#[cfg(test)]
mod test_words;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod coverage_tests;
#[cfg(test)]
mod capture_return_tests;
#[cfg(test)]
mod generic_tests;
#[cfg(test)]
mod element_tests;
#[cfg(test)]
mod primitive_tests;
#[cfg(test)]
mod typed_element_tests;
#[cfg(test)]
mod wasm_tests;
#[cfg(test)]
mod wasm_float_tests;
#[cfg(test)]
mod wasm_instance_tests;
#[cfg(test)]
mod wasm_memory_tests;
#[cfg(test)]
mod wasm_flow_tests;
#[cfg(test)]
mod wasm_call_tests;
#[cfg(test)]
mod wasm_simd_tests;
#[cfg(test)]
mod instanceof_tests;
