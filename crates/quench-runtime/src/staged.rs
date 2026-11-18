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

use crate::bytecode::{Decoder, FieldBase, FieldLookup, Op, Operand, Register, WideInstruction};
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
pub(crate) use steps::{step, row_step, RowStep, step_flow, step_wasm_int, step_local, step_number, step_value, step_names, step_property, step_keyed, step_generic, step_wasm_float, step_wasm_jump, step_wasm_state, step_wasm_access};
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
/// `FUSED_ROWS` mode: `A`'s result stays an unboxed word for `B`. Never boxed in the fused record.
pub(crate) const FUSED_RAW: u8 = 2;
/// The mode of a window pair or chain `fuse_rows` installs for a window local's (`FLAG_WINDOW`) first row: both
/// rows run at the window's flag, reading the window's locals. Not a table mode; `fuse_rows` names it.
pub(crate) const FUSED_WINDOW: u8 = 3;
/// `FUSED_ROWS` mode: `B` runs on the record at `A`'s jump target (reserved; not yet implemented in `fused_rows`).
pub(crate) const FUSED_TARGET: u8 = 4;

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

/// The opcode whose row answers a `Binary` record: a register pair without result flags decodes
/// here (`decode_binary`); any other record runs the general step.
pub(crate) const BINARY_ROW: u16 = Op::Binary as u16;
/// The opcode of a conditional jump on a comparison of two operands (`Operation::CompareJump`),
/// the record the default comparison handler and its register forms are told apart by.
pub(crate) const COMPARE_JUMP_ROW: u16 = Op::JumpBinaryFalse as u16;

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
pub(crate) const WASM_JUMP_ZERO_ROW: u16 = Op::WasmJumpI32Zero as u16;
pub(crate) const WASM_JUMP_NONZERO_ROW: u16 = Op::WasmJumpI32NonZero as u16;
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
pub(crate) const STORE_ENV_LOCAL_ROW: u16 = Op::StoreEnvLocal as u16;

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

/// Whether `op` is an increment or decrement (`IncDec`), the update a fused index triple starts with.
pub(crate) const fn increments(op: Op) -> bool {
    matches!(op, Op::IncDec)
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
/// Writes a captured slot (`StoreCapture`).
pub(crate) const fn writes_capture(op: Op) -> bool { matches!(op, Op::StoreCapture) }
/// Reads a binding by name (`LoadName`).
pub(crate) const fn reads_name(op: Op) -> bool { matches!(op, Op::LoadName) }
/// Reads an environment slot (`LoadEnvLocal`).
pub(crate) const fn reads_environment_slot(op: Op) -> bool { matches!(op, Op::LoadEnvLocal) }
/// Defines an element of an array literal (`DefineArrayElement`).
pub(crate) const fn defines_array_element(op: Op) -> bool { matches!(op, Op::DefineArrayElement) }
/// Refuses `null` and `undefined` (`RequireObjectCoercible`).
pub(crate) const fn requires_object_coercible(op: Op) -> bool { matches!(op, Op::RequireObjectCoercible) }
/// Copies a register into another (`Move`).
pub(crate) const fn moves(op: Op) -> bool { matches!(op, Op::Move) }
/// Jumps on a register's truthiness (`JumpFalse`, `JumpTrue`).
pub(crate) const fn tests_truthiness(op: Op) -> bool { matches!(op, Op::JumpFalse | Op::JumpTrue) }
/// Writes `this` as a field (`SetThisField`).
pub(crate) const fn writes_this_field(op: Op) -> bool { matches!(op, Op::SetThisField) }

/// An instruction `step` runs, with the operands a tier decoded.
#[derive(Clone, Copy)]
pub(crate) enum Operation<W, P> {
    /// Copy register `source` into `destination` with its facts.
    Move { destination: Register, source: Register },
    /// Define `destination` from `constant`.
    LoadConst { destination: Register, constant: Constant },
    /// Define `destination` from local slot `slot`.
    LoadLocalPlain { destination: Register, slot: u16 },
    /// Write `source` to local slot `slot`, and to `result` where the instruction has one.
    StoreLocalPlain { source: Register, slot: u16, result: Option<Register> },
    /// Define `destination` from local slot `slot` where its binding is initialized (LoadLocal): a
    /// binding in its temporal dead zone, or a numeric local target (`numeric`), leaves at the
    /// frame's miss edge.
    LoadLocalChecked { destination: Register, slot: u16, numeric: bool },
    /// Write `source` to local slot `slot` where its binding is initialized, or whatever its state
    /// where `initializing` (a declaration's own store), and to `result` where the instruction has
    /// one (StoreLocal). An assignment to a binding in its temporal dead zone leaves at the miss edge.
    StoreLocalChecked { source: Register, slot: u16, result: Option<Register>, initializing: bool },
    /// Open the temporal dead zone of local slot `slot` by writing the deleted marker (InitializeTdz).
    InitializeLocal { slot: u16 },
    /// Continue at the jump target.
    Jump,
    /// Continue at the jump target when register `register` is truthy, or when it is falsy
    /// where `taken` is false; otherwise continue after this instruction.
    JumpIf { register: Register, taken: bool },
    /// Define `destination` with the activation's `this`; where the activation does not
    /// hold it, leave at the frame's miss edge.
    LoadThis { destination: Register },
    /// Define `destination` with the function the running activation runs (`Frame::callee`); where
    /// the frame does not hold it, leave at the frame's miss edge.
    LoadCallee { destination: Register },
    /// A Number operation (`numeric`).
    Number(NumberOperation),
    /// Continue where register `register` is neither `null` nor `undefined`; those leave at
    /// the frame's miss edge, where the interpreter throws the TypeError
    /// (RequireObjectCoercible).
    RequireObjectCoercible { register: Register },
    /// Continue where register `register` holds an object or array record, which
    /// `RequireIteratorResult` accepts as it is; any other value leaves at the frame's miss edge,
    /// where the VM raises or reads it.
    RequireIteratorResult { register: Register },
    /// Define `destination` with the logical negation of register `source`'s truthiness.
    LogicalNot { destination: Register, source: Register },
    /// Define `destination` with the `typeof` name index (int32) of register `source`'s value
    /// where its own word decides it; a heap value leaves at the frame's miss edge.
    TypeofIndex { destination: Register, source: Register },
    /// Define `destination` with whether `typeof source` is the name at `index` in `TYPEOF_NAMES`,
    /// negated where `negated` (`UNARY_TYPEOF_IS`); a heap value leaves at the frame's miss edge.
    TypeofTest { destination: Register, source: Register, index: u64, negated: bool },
    /// Store `value` at dense array element `index`, under `proof`.
    IndexWrite { index: W, value: W, proof: P },
    /// The empty object, object or array literal, constant array, regexp literal, closure,
    /// literal template or string concatenation `allocation` makes, defined into `destination`; a due collection leaves
    /// at the frame's miss edge (`Frame::allocate_value`).
    Allocate { destination: Register, allocation: Allocation, proof: P },
    /// Define element `index` of the literal array in `array` as `value` (`DefineArrayElement`), a
    /// fresh hole of that array; the frame's element definer answers it (`Frame::define_element`).
    DefineElement { array: Register, index: u32, value: Register },
    /// Continue unless an interrupt or a due collection is requested (`Frame::safepoint`).
    Safepoint { proof: P },
    /// The element store's host helper for `base[key] = value`, under `proof`; the frame's
    /// miss edge takes a failed store.
    IndexSetHelper { base: W, key: W, value: W, proof: P },
    /// The name read of `atom` through cache `site` into `destination`, and `undefined` into
    /// `receiver` where it is a call's: the frame's facts about the global (`GlobalFrame::global_binding`)
    /// decide it, else the VM's read (`CallOut::LoadName`). A call's receiver is the VM's where
    /// a `with` scope may bind the name, so a read the frame does not resolve leaves.
    LoadName { destination: Register, atom: u32, site: u16, receiver: Option<Register>, proof: P },
    /// Define `destination` as `undefined` (`void`).
    Void { destination: Register },
    /// ToPropertyKey of a string `source`, which is its own key; any other value leaves at the
    /// miss edge, where the VM converts it (`Vm::to_property_key`: a Number allocates, an object
    /// runs guest code).
    PropertyKey { destination: Register, source: Register },
    /// `left == right` (or `===`; negated for `!=`, `!==`) where the two words decide it: the
    /// same word, two int32s, or `null` and `undefined` under `==`. Any other pair leaves at the
    /// miss edge, where the VM compares (strings, BigInts, conversions that run guest code).
    Equality { destination: Register, left: Operand, right: Operand, strict: bool, negated: bool },
    /// Any other `Binary` of two operands, each a register or a program constant (a string or BigInt
    /// relation, an arithmetic of other than Numbers, `instanceof`, `in`): the VM's own operation, a
    /// callout.
    Binary { destination: Register, left: Operand, right: Operand, operator: u32 },
    /// Define `destination` from environment slot `slot` (`environment::environment_slot`); a
    /// sparse storage or an internal marker leaves at the frame's miss edge.
    LoadEnvLocal { destination: Register, slot: u16 },
    /// Write `source` to environment slot `slot`, and to `result` where the instruction has one;
    /// `initialize` where the store is the binding's declaration.
    StoreEnvLocal { source: Register, slot: u16, result: Option<Register>, initialize: bool },
    /// Define `destination` with the length of the arguments binding in local `slot`.
    ArgumentsLength { destination: Register, slot: u16 },
    /// Define `destination` with element `index` of the arguments binding in local `slot`: a keeping
    /// activation's kept value, else the frame's or the VM's read (`CallOut::ArgumentGet`).
    ArgumentGet { destination: Register, index: Register, slot: u16 },
    /// Clone the running activation's environment by environment clone plan `plan`: a frame's own
    /// environment, or an environment window's (`CallOut::CloneEnv`). Defines no register.
    CloneEnv { plan: u32 },
    /// Define `destination` with the `typeof` name of the global `atom` through cache `site`
    /// (the lane's own read); an unresolvable name is `undefined`, not an error.
    GlobalTypeof { destination: Register, atom: u32, site: u16 },
    /// The global-name store of `source` to `atom` through cache `site`, an initialization
    /// where `initialize`; a binding the lane's store does not take leaves at the miss edge.
    GlobalWrite { source: Register, atom: u32, site: u16, initialize: bool },
    /// An iteration or spread step (`Protocol`) over the `operands` registers, defining
    /// `destination` where it has one; the generic protocol runs guest code, a callout.
    Protocol { step: Protocol, destination: Option<Register>, operands: [Option<Register>; 2] },
    /// Define `destination` with the next key of the for-in state in register `state`; continue
    /// at the jump target once none is left.
    ForInNext { destination: Register, state: Register },
    /// `base[key]` in the body of the for-in whose state register is `state`: the key's listed
    /// slot where the state's memo answers (`iteration::for_in_keyed_get`), else the element read.
    EnumeratedGet { destination: Register, state: Register, base: Register, key: Register },
    /// The VM field-store helper for `object.atom = value` through cache `site`.
    #[cfg_attr(not(test), allow(dead_code))] // no tier builds it until the call-out rows land
    FieldStoreMiss { object: W, atom: u32, site: u16, value: W, proof: P },
    /// Push the iterator in register `iterator` with its `done` register on the running cleanup
    /// stack (`Frame::iterator_push`), which closes it where the loop exits abruptly.
    IteratorCleanupPush { iterator: Register, done: Register },
    /// Pop the running cleanup stack's top entry (`Frame::iterator_pop`).
    IteratorCleanupPop,
    /// The index of the value in register `source` among the `count` program constants from `start`
    /// (`Frame::case_index`), defined into `destination`: the switch's string-case selector.
    StringCaseIndex { destination: Register, source: Register, start: u32, count: u16 },
    /// Suspend the promise job the run is rooted in on the value of register `source`, resuming into
    /// `destination` (`Frame::suspend_await`); the record leaves the lane run (`Edge::Leave`).
    Await { destination: Register, source: Register },
    /// The frame's `yield` of the value in register `value` (`Frame::suspend_yield`): the running
    /// generator's suspension, which the frame transfers control for (`Edge::Transfer`).
    Yield { value: Register },
    /// The frame's `yield*` of the iterable in register `iterable` (`Frame::suspend_yield_star`), which
    /// transfers control the same way.
    YieldStar { iterable: Register },
    /// The value `base.name` reads through `lookup`'s plan (`property::read_with`); the tier
    /// defines the result register.
    GetField { base: FieldBase, lookup: FieldLookup },
    /// `base.atom = value` through cache `site`'s store plan (`property_write::set_field`);
    /// `base` is `this` for `None`.
    SetField { value: Register, base: Option<Register>, atom: u32, site: u16 },
    /// `base.atom = value` of a word no register names, of type `fact`, through cache site
    /// `site`'s store plan (`property_write::store_word`); `base` is `this` for `None`.
    StoreField { base: Option<Register>, value: W, fact: Type, atom: u32, site: u16 },
    /// The VM's read of `atom` of `object`, a value no register names, through `site`
    /// (`PropertyFrame::field_probe`).
    FieldGetHost { object: W, atom: u32, site: u16 },
    /// The VM's adding store of `value` to `atom` of `object` (`PropertyFrame::field_add`).
    #[cfg_attr(not(test), allow(dead_code))]
    FieldAdd { object: W, atom: u32, site: u16, value: W },
    /// `delete base[key]` into `destination`, throwing on a refusal where `strict`
    /// (`PropertyFrame::delete_property`, else `CallOut::Delete`).
    Delete { destination: Register, base: Register, key: Register, strict: bool },
    /// `base[key]` past the dense and typed element reads (`PropertyFrame::index_get_host`).
    #[cfg_attr(not(test), allow(dead_code))]
    IndexGetHost { base: W, key: W },
    /// A step of the call protocol: admission, `this` binding, the callee's window, completion
    /// (`protocol`).
    Call(CallOperation<W>),
    /// A call whose one argument register holds an iterable to spread or an array to apply
    /// (`CallSpread`, `CallArray`): the VM's call, a callout (`spread`).
    SpreadCall { destination: Register, callee: Register, receiver: Register, arguments: Arguments, passing: Passing },
    /// `super(...)`: the VM's construction with the activation's `new.target`, a callout (`spread`).
    SuperCall { destination: Register, callee: Register, arguments: Arguments, passing: Passing },
    /// Complete the activation with the value of `source` (`invoke::ret`).
    Return { source: Register },
    /// Raise the value of register `source` (Throw). No staged row raises it: `step` leaves it
    /// `Unresolved`, so each tier keeps its own throw path.
    Throw { source: Register },
    /// The value `base[key]` reads, a present dense element where the array and the key's int32
    /// are proved (`element_rows`), else the VM's read; the tier defines the result register.
    ElementRead { base: Register, key: Operand },
    /// `base[key] = value`, replacing a present dense element in place where the array owns its
    /// backing alone and the value needs no barrier log, else the VM's store.
    ElementWrite { value: Register, base: Register, key: Register },
    /// Define `destination` from the capture `slot`, `depth` scopes out, or from the root
    /// environment's projected binding where `root` (`Frame::capture_slot`). The decode never
    /// names a root: only a tier that found the walk end at one does.
    LoadCapture { destination: Register, depth: u16, slot: u16, root: bool },
    /// Store `source` to the capture `slot`, `depth` scopes out (`Frame::store_capture`).
    StoreCapture { source: Register, depth: u16, slot: u16 },
    /// A Wasm float, float conversion or select (`wasm_float`); the frame's Wasm words decide it, and
    /// a trap or an operand the frame does not hold leaves at the miss edge.
    WasmFloat(wasm_float::WasmFloatOperation),
    /// `left` compared with `right` (`Comparison`), jumping to the target when the comparison
    /// holds, or fails where `negate` (`JumpBinaryFalse`); otherwise continue after it.
    CompareJump { comparison: Comparison, left: Operand, right: Operand, negate: bool },
    /// A Wasm i32 or i64 register or immediate operator (`wasm_int`), which the row answers from its words.
    WasmInt(wasm_int::WasmIntOperation),
    /// A Wasm i32 conditional jump or fused add-and-jump over int32 operands (`wasm_jump`).
    WasmJump(WasmJump),
    /// A Wasm branch table: control goes to the entry its index picks (`Edge::Entry`, `wasm_jump`).
    BranchTable(BranchTable),
    /// A Wasm global's load or store, or a register window's fill (`wasm_state`).
    WasmState(wasm_state::WasmStateOperation),
    /// A Wasm direct load or store of the function's memory (`wasm_access`).
    WasmAccess(wasm_access::WasmAccessOperation),
    /// A Wasm SIMD operator or byte shuffle on the lane's vector registers (`wasm_simd`).
    WasmVector(wasm_simd::VectorOperation),
    /// A property add of `value` to the object at the record table `records`, through a site's
    /// transition `edge` (and the prototype `link` a chained site must hold), storing it in `form`
    /// (`add::site_add`). The tier names the object word and the site's words: a register's or the
    /// activation's receiver, so one variant serves the register and `this` add rows. No opcode
    /// decodes to it; a stencil row builds it. Leaves at the miss edge where the add declines.
    AddSite { records: W, object: W, edge: add::Edge<W>, link: Option<W>, form: Form, value: W },
    /// A function's inferred name `name` written to the function word `function`'s empty name slot
    /// through a site's naming edge (`naming::site_name`, the `(edge, empty)` pair). No opcode decodes
    /// to it; a stencil row builds it. Leaves at the miss edge where the naming declines.
    NamingSite { records: W, function: W, naming: (add::Edge<W>, W), form: Form, name: W },
    /// `value instanceof constructor` into `destination`, decided by the frame's words of the value
    /// and the constructor (`Frame::instance_of`, the walk `object::instanceof_by_words`); the frame's
    /// default leaves it to the VM's `Binary` callout. Leaves at the miss edge where the walk declines.
    InstanceOf { destination: Register, value: Register, constructor: Register },
    /// Continue where register `source` holds a direct array (the array identity test, `Frame::direct_array`);
    /// any other value leaves at the miss edge.
    DirectArray { source: Register },
    /// Define `destination` with the `length` of the direct array in register `source` (`Frame::array_length`).
    ArrayLength { destination: Register, source: Register },
    /// Define `destination` with the `length` of the typed array in register `source` (`Frame::typed_length`).
    TypedLength { destination: Register, source: Register },
    /// Append the value in register `value` to the array in register `array`, where its backing has
    /// room and is writable (`Frame::array_append`); a value the frame does not append leaves at the miss edge.
    ArrayAppend { array: Register, value: Register },
}
impl<W, P> Operation<W, P> {
    /// The staged operation `instruction` denotes, with the operands the catalog fixes and the
    /// tables of `context` its operands index, by the decoder the catalog names for its opcode
    /// (`Op::staged`). `None` where the opcode has no staged operation, or where its constant or
    /// method site is not available to this tier: the caller then takes its own exact path. The
    /// heap operations are the frame's (`Frame::field_read`); a tier decodes those itself.
    pub(crate) fn decode(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
        match instruction.op().staged()? {
            Decoder::Flow => Self::decode_flow(instruction, context),
            Decoder::Local => Self::decode_local(instruction),
            Decoder::Generic => Self::decode_generic(instruction),
            Decoder::Value => Self::decode_value(instruction),
            Decoder::Property => Self::decode_property(instruction),
            Decoder::Element => Self::decode_element(instruction),
            Decoder::Call => Self::decode_call(instruction, context),
            Decoder::Names => names::decode(instruction),
            Decoder::WasmFloat => Self::decode_wasm_float(instruction),
            // Under the tier's proof of the allocation (`decode_literal`), so none here.
            Decoder::Proved | Decoder::Literal => None,
            Decoder::WasmInt => Self::decode_wasm_int(instruction),
            Decoder::WasmJump => Self::decode_wasm_jump(instruction),
            Decoder::WasmState => Self::decode_wasm_state(instruction, context),
            Decoder::WasmMemory => Self::decode_wasm_access(instruction, context),
            Decoder::WasmVector => Self::decode_wasm_vector(instruction, context),
            // A lane row is the lane's own (`decode_lane`): the plan keeps its generic or exit route for it.
            Decoder::Lane => None,
        }
    }

    /// The Wasm i32 jumps of `decode` (`Decoder::WasmJump`), from any record's wide fields
    /// (`Fields::wide`).
    #[inline(always)]
    pub(crate) fn decode_wasm_jump(instruction: WideInstruction) -> Option<Self> {
        WasmJump::decode(instruction).map(Self::WasmJump).or_else(|| BranchTable::decode(instruction).map(Self::BranchTable))
    }

    /// The environment, global and iteration operations of `decode` (`Decoder::Names`), from a
    /// record's wide fields (`Fields::wide`).
    #[inline(always)]
    pub(crate) fn decode_names(instruction: WideInstruction) -> Option<Self> {
        names::decode(instruction)
    }

    /// The literal operations of `decode` (`Decoder::Literal`) under the tier's `proof` of the
    /// allocation: the allocation a literal, closure or constant array record makes
    /// (`Allocation::decode`, which an array literal's element definitions do not change: the
    /// array is a hole array and each definition is `DefineElement`), and an element definition.
    #[inline(always)]
    pub(crate) fn decode_literal(instruction: WideInstruction, proof: P) -> Option<Self> {
        match instruction.op() {
            Op::DefineArrayElement => Some(Self::DefineElement {
                array: instruction.register_b(), index: instruction.imm(), value: instruction.register_a() }),
            _ => {
                let (destination, allocation) = Allocation::decode(instruction, &[])?;
                Some(Self::Allocate { destination, allocation, proof })
            }
        }
    }

    /// The register moves and control transfers of `decode`, from any record a tier holds
    /// (`Fields`): the one flow decoder, the bytecode's and the lane's alike.
    #[inline(always)]
    pub(crate) fn decode_flow<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        Some(match instruction.op() {
            Op::Move => Self::Move {
                destination: instruction.result_register(), source: instruction.register_b() },
            Op::LoadConst => Self::LoadConst {
                destination: instruction.result_register(),
                constant: instruction.constant(context)? },
            Op::Jump => Self::Jump,
            Op::JumpFalse => Self::JumpIf {
                register: instruction.register_a(), taken: false },
            Op::JumpTrue => Self::JumpIf {
                register: instruction.register_a(), taken: true },
            Op::Throw => Self::Throw { source: instruction.register_a() },
            _ => Self::Return { source: instruction.register_a() },
        })
    }

    /// The call operations of `decode`: a call or construction through its forms (`Invocation`),
    /// except `super(...)`, whose arguments the VM gathers (`spread`).
    fn decode_call(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
        if instruction.op() == Op::Construct && instruction.is_super_construct() {
            return spread::decode(instruction);
        }
        Some(Self::Call(CallOperation::Site(Invocation::of(instruction, context)?)))
    }

    /// The property operations of `decode`: a plain named read, a named store and `delete`.
    fn decode_property(instruction: WideInstruction) -> Option<Self> {
        match instruction.op() {
            Op::GetField => Self::decode_get_field(instruction),
            Op::Delete => Some(Self::Delete { destination: instruction.result_register(), base: instruction.register_b(),
                key: instruction.register_c(), strict: instruction.boolean_flag()? }),
            op if op.is_field_store() => Some(Self::SetField { value: instruction.register_a(),
                base: instruction.field_store_register(), atom: instruction.atom_index(),
                site: instruction.cache_site_index() }),
            _ => None,
        }
    }

    /// A named read that completes no frame and writes no `this`; a nested lookup's base is the
    /// tier's own (`FieldBase::NESTED`).
    fn decode_get_field(instruction: WideInstruction) -> Option<Self> {
        if instruction.result_flags() != 0 { return None; }
        let lookup = instruction.field_lookup()?;
        let base = match lookup {
            FieldLookup::Atom { base, .. } => base,
            FieldLookup::Site(_) => FieldBase(FieldBase::NESTED),
        };
        Some(Self::GetField { base, lookup })
    }

    /// The value-level operations of `decode`.
    #[inline(always)]
    pub(crate) fn decode_value(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::LoadThis => Self::LoadThis {
                destination: instruction.result_register() },
            Op::LoadCallee => Self::LoadCallee {
                destination: instruction.result_register() },
            Op::RequireObjectCoercible => Self::RequireObjectCoercible {
                register: instruction.register_b() },
            Op::RequireIteratorResult => Self::RequireIteratorResult {
                register: instruction.register_b() },
            Op::ToNumeric => Self::Number(NumberOperation::ToNumeric {
                destination: instruction.result_register(), source: instruction.register_b() }),
            Op::IncDec if instruction.numeric_local_target().is_none() => Self::Number(NumberOperation::IncDec {
                destination: instruction.result_register(), source: instruction.register_b(),
                decrement: instruction.boolean_flag()?, overflow: Overflow::Exit }),
            Op::LoadCapture if instruction.result_flags() == 0 => Self::LoadCapture {
                destination: instruction.result_register(), depth: instruction.capture_depth(), slot: instruction.capture_slot(),
                root: false },
            Op::StoreCapture => Self::StoreCapture {
                source: instruction.register_a(), depth: instruction.capture_depth(), slot: instruction.capture_slot() },
            Op::Unary => return Self::decode_unary(instruction),
            _ => return None,
        })
    }

    /// The unary operators of `decode_value`.
    #[inline(always)]
    fn decode_unary(instruction: WideInstruction) -> Option<Self> {
        let (destination, source) = (instruction.result_register(), instruction.register_b());
        let operator = instruction.unary_operator();
        if let Some((index, negated)) = crate::bytecode::typeof_test(operator) {
            return Some(Self::TypeofTest { destination, source, index: index as u64, negated });
        }
        Some(match operator {
            _ if operator == oxc_ast::ast::UnaryOperator::LogicalNot as u32 => Self::LogicalNot { destination, source },
            _ if operator == crate::bytecode::UNARY_TYPEOF_INDEX => Self::TypeofIndex { destination, source },
            _ if operator == oxc_ast::ast::UnaryOperator::BitwiseNot as u32 =>
                Self::Number(NumberOperation::BitwiseNot { destination, source: Operand::register(source) }),
            _ if operator == oxc_ast::ast::UnaryOperator::UnaryNegation as u32 =>
                Self::number_unary(destination, source, crate::vm::operations::NumberUnary::Negate),
            _ if operator == oxc_ast::ast::UnaryOperator::UnaryPlus as u32 =>
                Self::number_unary(destination, source, crate::vm::operations::NumberUnary::Plus),
            _ if operator == oxc_ast::ast::UnaryOperator::Void as u32 && !instruction.returns_from_frame() =>
                Self::Void { destination },
            _ => return None,
        })
    }

    /// `operation` of a Number `source` in the int32 domain: an int32 operand decides it, a double
    /// or a value no Number leaves to the frame's miss edge (`Unary`'s arm).
    #[inline(always)]
    fn number_unary(destination: Register, source: Register, operation: crate::vm::operations::NumberUnary) -> Self {
        Self::Number(NumberOperation::Unary { destination, source: Operand::register(source), operation, domain: Domain::Int32 })
    }

    /// The Wasm integer operations of `decode` (`Decoder::WasmInt`), from a record's wide fields.
    #[inline(always)]
    pub(crate) fn decode_wasm_int(instruction: WideInstruction) -> Option<Self> {
        wasm_int::decode(instruction).map(Self::WasmInt)
    }

    /// The local-slot operations of `decode`.
    #[inline(always)]
    pub(crate) fn decode_local(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::LoadLocalPlain => Self::LoadLocalPlain {
                destination: instruction.result_register(),
                slot: window_slot(instruction.imm()) },
            Op::StoreLocalPlain => Self::StoreLocalPlain {
                source: instruction.register_a(),
                slot: window_slot(instruction.imm()),
                result: instruction.optional_register_b() },
            Op::LoadLocal => Self::LoadLocalChecked {
                destination: instruction.result_register(),
                slot: window_slot(instruction.imm()),
                numeric: instruction.result_flags() & crate::bytecode::NUMERIC_LOCAL_TARGET != 0 },
            Op::StoreLocal => Self::StoreLocalChecked {
                source: instruction.register_a(),
                slot: window_slot(instruction.imm()),
                result: instruction.optional_register_b(),
                initializing: instruction.c() != 0 },
            Op::InitializeTdz => Self::InitializeLocal {
                slot: window_slot(instruction.imm()) },
            _ => return None,
        })
    }
}

impl<W, P> Operation<W, P> {
    /// The operation as a root read where it is a `LoadCapture` (`root`), else unchanged: a row whose
    /// record a walk settled on a root binding decodes its read so (`FLAG_ROOT`).
    pub(crate) fn rooted(self) -> Self {
        match self {
            Self::LoadCapture { destination, depth, slot, .. } => Self::LoadCapture { destination, depth, slot, root: true },
            other => other,
        }
    }
}

impl Operation<u64, ()> {
    /// The operation `instruction` denotes, decoded with no tables (`DecodeContext::EMPTY`): what a
    /// tier asks an instruction's meaning by, where the row's operands need no table.
    pub(crate) fn of(instruction: WideInstruction) -> Option<Self> {
        Self::decode(instruction, DecodeContext::EMPTY)
    }

    /// The lane's own rows (`Decoder::Lane`): a string-case index over the constants the record names, and an
    /// `await` the lane's frame suspends.
    pub(crate) fn decode_lane(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::StringCaseIndex => Self::StringCaseIndex { destination: instruction.result_register(),
                source: instruction.register_c(), start: instruction.imm(), count: instruction.element_count() },
            Op::Await => Self::Await { destination: instruction.result_register(), source: instruction.register_b() },
            Op::Yield => Self::Yield { value: instruction.register_b() },
            Op::YieldStar => Self::YieldStar { iterable: instruction.register_c() },
            _ => return None,
        })
    }

    /// Whether `instruction` does nothing: a marker that defines and writes nothing.
    pub(crate) fn is_nop(instruction: WideInstruction) -> bool {
        instruction.op() == Op::Nop
    }

    /// Whether `instruction` is a Number binary operation (`NumericBinary`): its operator is the
    /// Number arithmetic, relation or bitwise operation the tier reads from its operands.
    pub(crate) fn is_number_binary(instruction: WideInstruction) -> bool {
        instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary
    }

    /// Whether `instruction` is a Number unary operation (`NumericUnary`), whose operator the tier
    /// reads from the instruction's unary operator field.
    pub(crate) fn is_number_unary(instruction: WideInstruction) -> bool {
        instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericUnary
    }

    /// Whether `instruction` is a call through its callee register (`Call`, including the eval
    /// forms that decode to no operation), which an inlined unit plans and admits as a call.
    pub(crate) fn is_plain_call(instruction: WideInstruction) -> bool {
        instruction.op() == Op::Call
    }

    /// Whether `instruction` constructs (`Construct`), with its callee and arguments in registers.
    pub(crate) fn is_construction(instruction: WideInstruction) -> bool {
        instruction.op() == Op::Construct
    }

    /// Whether `instruction` throws: its decoded operation is `Throw`, which leaves at the miss edge.
    pub(crate) fn is_throw(instruction: WideInstruction) -> bool {
        matches!(Self::of(instruction), Some(Self::Throw { .. }))
    }

    /// Whether `instruction` returns from its frame: its decoded operation is `Return`.
    pub(crate) fn is_return(instruction: WideInstruction) -> bool {
        matches!(Self::of(instruction), Some(Self::Return { .. }))
    }

    /// Whether `instruction` is a Number, equality or general `Binary` operation: the operation a
    /// `Binary` record decodes to, whose operator the tier reads from its operand field.
    pub(crate) fn is_binary(instruction: WideInstruction) -> bool {
        matches!(Self::of(instruction), Some(Self::Number(_) | Self::Equality { .. } | Self::Binary { .. }))
    }

    /// Whether `instruction` yields a value from a generator (`Yield`, a lane row). The interpreter
    /// frame answers no suspension (`Frame::suspend_yield`), so its yield is the general loop's step.
    pub(crate) fn is_yield(instruction: WideInstruction) -> bool {
        matches!(Self::decode_lane(instruction), Some(Self::Yield { .. }))
    }

    /// Whether `instruction` awaits a value (`Await`, a lane row). The interpreter frame answers no
    /// suspension (`Frame::suspend_await`), so an async frame's await is the general loop's step.
    pub(crate) fn is_await(instruction: WideInstruction) -> bool {
        matches!(Self::decode_lane(instruction), Some(Self::Await { .. }))
    }

    /// Whether `instruction` tests a private name's brand (`PrivateIn`), which no staged row answers.
    pub(crate) fn is_private_in(instruction: WideInstruction) -> bool {
        instruction.op() == Op::PrivateIn
    }

    /// Whether `instruction` is a named field read (`GetField`), whatever its result flags.
    pub(crate) fn is_field_read(instruction: WideInstruction) -> bool {
        instruction.op() == Op::GetField
    }

    /// Whether `instruction` is a named field store (`SetField`, `SetThisField`, or a store of a
    /// field of `this`), whatever its operand.
    pub(crate) fn is_field_store(instruction: WideInstruction) -> bool {
        instruction.op().is_field_store()
    }

    /// Whether `instruction` is the named store `SetField` itself (not a store to `this`).
    pub(crate) fn is_set_field(instruction: WideInstruction) -> bool {
        instruction.op() == Op::SetField
    }

    /// Whether `instruction` reads a capture (`LoadCapture`), whatever its result flags.
    pub(crate) fn is_capture_read(instruction: WideInstruction) -> bool {
        instruction.op() == Op::LoadCapture
    }

    /// Whether `instruction` is an ordinary call (`Call`, `CallArray`), which a native tier
    /// profiles and dispatches as a call.
    pub(crate) fn is_ordinary_call(instruction: WideInstruction) -> bool {
        instruction.op().is_ordinary_call()
    }

    /// The field cache sites `instruction` reads (`true`) or stores through (`false`), as the
    /// program's field sites name them: a named read's lookup (its site, or the sites of a
    /// nested lookup), and a field store's cache site.
    pub(crate) fn field_cache_sites(instruction: WideInstruction, field_sites: &[crate::bytecode::FieldSite]) -> Vec<(u16, bool)> {
        if Self::is_field_read(instruction) {
            return match instruction.field_lookup() {
                Some(FieldLookup::Atom { cache_site, .. }) => vec![(cache_site, true)],
                Some(FieldLookup::Site(index)) => field_sites.get(index).map_or_else(Vec::new, |site| {
                    let reads = [Some(site.first), site.second].into_iter().flatten().map(|(_, cache)| (cache, true));
                    reads.chain(site.sink.map(|(_, cache)| (cache, false))).collect()
                }),
                None => Vec::new(),
            };
        }
        if Self::is_field_store(instruction) { vec![(instruction.cache_site_index(), false)] } else { Vec::new() }
    }

    /// Whether a unit holds `instruction` by its own rewrite or an exit rather than by renaming
    /// (the catalog's `UNIT_REWRITTEN` trait).
    pub(crate) fn is_unit_rewritten(instruction: WideInstruction) -> bool {
        instruction.op().is_unit_rewritten()
    }
}

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
