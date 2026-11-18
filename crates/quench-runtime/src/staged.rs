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
/// The stencil layout numbers, one source for the stencils (`#[path]`) and the runtime's const checks.
pub(crate) mod layout_offsets;
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
pub(crate) mod protocol_literals;
pub(crate) mod protocol_classes;
pub(crate) mod protocol_values;
pub(crate) mod protocol_owned;
pub(crate) mod protocol_wasm;
mod callout;
mod spread;
mod admission;
mod delete;
mod operation;
pub(crate) use operation::Operation;

impl<W, P> Operation<W, P> {
    /// The destination and constant index of an instruction `decode_flow` reads as `LoadConst`,
    /// for a tier that holds its constants in a table of its own (the b-jit's Numbers): the
    /// same decode, with the constant left for the tier to look up.
    #[inline(always)]
    pub(crate) fn decode_constant_load(instruction: WideInstruction) -> Option<(Register, usize)> {
        (instruction.op() == Op::LoadConst).then(|| (instruction.result_register(), instruction.constant_index()))
    }
}
mod decode;
pub(crate) mod meaning_fast;
pub(crate) mod meaning;
mod captures;
pub(crate) mod compare;
pub(crate) mod meaning_dispatch;
pub(crate) mod frame_arm;
pub(crate) use global::{lexical_read, GlobalBinding, GlobalFrame};
pub(crate) use names::Protocol;
pub(crate) use callout::{CallOut, OperandValues};
pub(crate) use spread::Passing;
pub(crate) use decode::{DecodeContext, Fields};
pub(crate) use compare::{nullish_of, Comparison};
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
pub(crate) mod code_units;
pub(crate) mod parse_int;
pub(crate) mod holder;
pub(crate) mod iteration;
pub(crate) mod naming;
pub(crate) mod call; pub(crate) mod frame;
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
mod suspend;
pub(crate) use suspend::Suspension;
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
pub(crate) mod wasm_concrete;
pub(crate) mod wasm_gc;
// merge: wasm_concrete stays pub(crate) (vm/wasm_gc and vm/wasm_gc/stage.rs reach it); wasm_int is theirs'.
pub(crate) mod wasm_int;
pub(crate) mod wasm_call;
pub(crate) mod wasm_flow;
pub(crate) mod wasm_instance;
pub(crate) mod wasm_machine;
pub(crate) mod wasm_memory;
pub(crate) mod wasm_table_ops;
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
pub(crate) use ic::{HINTED_FORM, TAGGED_SLOTS, concrete_hinted_own_field_hit, concrete_own_field_hit, concrete_site_own_chain, concrete_site_own_read, concrete_site_store};
pub(crate) use object::{Form, Layout, Slots, concrete_own_read, arena_extras_layout};

mod stage;
pub(crate) use stage::{fill_by_rows, Passed, StagedRecordSpace, Stage, Table, word_bytes, word_offset};

mod register_frame;
pub(crate) use register_frame::{Constant, Frame, Step, WasmRep, bound_this, jump_if, load_this, logical_not, read_initialized_local, read_local, write_initialized_local, write_local};

/// Where a row goes when its frame cannot decide a step: the VM's general step (`General`, which
/// runs the interpreter's own step in place for every opcode the general path covers); for a
/// capture read whose scopes the frame does not hold, the walk over the scopes from the
/// activation's environment (`WalkScopes`); for a by-name load or a store, the name's first-run
/// resolution or the scope's rules; for a comparison jump the frame leaves, the lane's comparison
/// chain; for a `yield`, the suspension the root takes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Miss {
    General,
    /// A capture read the frame does not answer: the walk over the scopes, which requickens the
    /// record to the root binding it finds (`load_capture_walk_miss`).
    WalkScopes,
    /// A global-name store the frame does not hold (its hint misses): the name's first-run
    /// resolution, which resolves the store and holds the record on its row.
    StoreNameFirst,
    /// A comparison jump the frame leaves (a pair that is no Number relation or word test): the
    /// lane's comparison chain decides the relations it can without guest code, and keeps the
    /// operands' feedback (`binary_false_chain_miss`).
    CompareChain,
    /// A `yield` the frame does not suspend in place (no generator runs the lane): the job root's
    /// suspension, or the general step (`yield_miss`).
    Yield,
    /// A `yield*` the frame does not suspend in place: the general path (`lane_exit`), since the
    /// general step of a suspending operation does not continue in place.
    YieldStar,
    /// A by-name load the frame does not answer: its first run resolves the name, and a later
    /// run whose binding misses takes that binding's residual (`NameLoad`).
    NameLoad(NameLoad),
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
        Op::StoreName => Miss::StoreNameFirst,
        Op::JumpBinaryFalse => Miss::CompareChain,
        Op::Yield => Miss::Yield,
        Op::YieldStar => Miss::YieldStar,
        _ => match name_load(op) {
            Some(load) => Miss::NameLoad(load),
            None => Miss::General,
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
    // A local read then a branch on it.
    (Op::LoadLocalPlain as u16, Op::JumpFalse as u16, FUSED_PLAIN),
    (Op::LoadLocalPlain as u16, Op::JumpTrue as u16, FUSED_PLAIN),
    // `x++` / `x--` on a Number (`ToNumeric` then the update). Subsumes `specialize_updates`
    // (`lane_postfix_update`).
    (Op::ToNumeric as u16, Op::IncDec as u16, FUSED_PLAIN),
    // An update then the unconditional jump after it. Subsumes `specialize_update_jumps`
    // (`lane_inc_dec_jump`) and the update of `specialize_update_back_edges` where it sits
    // directly before the back jump (its loop-test variant `lane_inc_dec_loop` is not kept).
    (Op::IncDec as u16, Op::Jump as u16, FUSED_PLAIN),
    // A Number operation then a branch on its result. Subsumes `specialize_value_jumps`
    // (`lane_value_jump`, the value's write kept: the dead-value variant is not kept).
    (Op::Binary as u16, Op::JumpFalse as u16, FUSED_PLAIN),
    (Op::Binary as u16, Op::JumpTrue as u16, FUSED_PLAIN),
    // Two arithmetic or bitwise operators, the second reading the first's result: `(a + b) * c`,
    // `(a + b) | 0`. Both rows run in full (`FUSED_PLAIN`); the hand pair's dead-write skip and its
    // raw result are dropped.
    (Op::Binary as u16, Op::Binary as u16, FUSED_PLAIN),
    // A `typeof` test then the branch on it (`if (typeof x === "number")`).
    (Op::Unary as u16, Op::JumpFalse as u16, FUSED_PLAIN),
    // `!(x instanceof C)` tests also take the `Binary` then `JumpTrue` pair above.
    // `c = c + 1` of a capture: the store, then the read of the same capture. Subsumes
    // `specialize_capture_updates`' store-then-read form (`lane_store_capture_load`).
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
    // A local or capture read then a register comparison jump (`JumpBinaryFalse`). Subsumes
    // `specialize_load_branch_pairs` (`load_relation_jump_handler`), both directions of the jump.
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
    // `a[++i]`: an update then the keyed read it indexes. Subsumes `specialize_updated_index_reads`
    // (`lane_update_get_index`).
    (Op::IncDec as u16, Op::GetIndex as u16, FUSED_PLAIN),
    // `x[t = i]`: a register copy then the keyed read of the copy's element. Subsumes
    // `specialize_moved_index_reads` (`lane_move_get_index_proved`; the proof is the read's flag).
    (Op::Move as u16, Op::GetIndex as u16, FUSED_PLAIN),
    // `t + K` then the element read or store its result addresses. Subsumes the operator pairs
    // (`specialize_operator_pairs`, `lane_operator_pair` and its constant form) into an element access.
    (Op::Binary as u16, Op::GetIndex as u16, FUSED_PLAIN),
    (Op::Binary as u16, Op::SetIndex as u16, FUSED_PLAIN),
    // `a[i] = v; t = v` of a register copy of the stored value. Subsumes `specialize_store_moves`
    // (`lane_set_index_move`).
    (Op::SetIndex as u16, Op::Move as u16, FUSED_PLAIN),
];

/// Runs of three or four generated rows the lane runs as one record (`chain_rows`): each
/// entry's links in record order, `NO_LINK` padding a three-link chain. Only the last link may
/// be a jump. A chain at a record wins over a `FUSED_ROWS` pair there.
pub(crate) const FUSED_CHAINS: [[u16; 4]; 41] = [
    // A local or capture read, then a register comparison, then its branch. Subsumes the value
    // jump of `specialize_value_jumps` (`lane_value_jump`) after a read.
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
    // Subsumes the int32 capture update (`specialize_capture_updates`, `lane_update_capture_int32`).
    [Op::LoadCapture as u16, Op::Binary as u16, Op::Binary as u16, Op::StoreCapture as u16],
    // `c = c + x` / `c -= x` of a capture: its read, the numeric operation, then the store. Subsumes
    // the untruncated capture update (`specialize_capture_updates`, `lane_store_capture_load`).
    [Op::LoadCapture as u16, Op::Binary as u16, Op::StoreCapture as u16, NO_LINK],
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
    // `a[++i + K]`: the update, the keyed read it indexes, then the Number operation on the read.
    // Subsumes `specialize_updated_index_operators` (`lane_update_get_index_operator`).
    [Op::IncDec as u16, Op::GetIndex as u16, Op::Binary as u16, NO_LINK],
    // `heap[(p + K) >> S]` and `heap[p + K]`: the address sum, its shift, then the element read or
    // store it addresses.
    [Op::Binary as u16, Op::Binary as u16, Op::GetIndex as u16, NO_LINK],
    [Op::Binary as u16, Op::Binary as u16, Op::SetIndex as u16, NO_LINK],
];

/// The absent fourth link of a three-link `FUSED_CHAINS` entry.
pub(crate) const NO_LINK: u16 = u16::MAX;
/// The opcode an absent chain link stands for, where its untaken branch still names `NO_LINK`.
pub(crate) const NO_LINK_OP: Op = Op::Nop;

/// `FUSED_ROWS` mode: both rows run in full, `A`'s result is written to its register.
pub(crate) const FUSED_PLAIN: u8 = 0;
/// `FUSED_ROWS` mode: `A`'s result register is dead after the pair, and `B` does not name it, so
/// the fused record skips `A`'s register write. `fuse_rows` installs it only where liveness
/// proves that; otherwise the pair runs as `FUSED_PLAIN`. `B` must be a value row: a jump's
/// target liveness is not checked, so no jump pair names this mode.
pub(crate) const FUSED_DEAD_FIRST: u8 = 1;
/// The mode of a window pair or chain `fuse_rows` installs for a window local's (`FLAG_WINDOW`) first row: both
/// rows run at the window's flag, reading the window's locals. Not a table mode; `fuse_rows` names it.
pub(crate) const FUSED_WINDOW: u8 = 3;
/// `fuse_rows` mode for a backward jump (`B`, the row after `A`) whose target record holds a
/// conditional test (`jump_target_test`, one of `FUSED_TARGET_ROWS`): the fused record runs
/// `A`, then the jump, then the target's test over the target's own fields in the same dispatch,
/// and continues at the test's taken or fall-through edge. The target record keeps its own row
/// for every other entry. The back edge's fuel check runs before the test, as the jump's own does.
/// It subsumes `lane_inc_dec_loop` and `lane_inc_dec_small_loop` (an update pair whose back edge
/// enters a register test or a constant test; `specialize_update_jumps`). A back edge into a loaded
/// bound (`lane_inc_dec_load_loop`) or into an update, and a lone back jump (`lane_jump_back_nullish`),
/// are not pairs and are not this mode.
pub(crate) const FUSED_TARGET: u8 = 4;

/// The opcode whose row answers an arguments binding's count (`Frame::arguments_length`), which a
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

/// Whether `op` is a conditional test a backward jump may run in its target's place
/// (`FUSED_TARGET`): a truthiness jump, or a comparison jump (`JumpBinaryFalse`, whose
/// comparison may be the nullish test of `x == null`). Wasm jumps are not admitted.
pub(crate) const fn jump_target_test(op: Op) -> bool {
    conditional_jump(op)
}

/// The conditional tests `FUSED_TARGET` instantiates a fused back edge for. `fuse_target_backedges`
/// matches a target record's handler against each one's row; a target outside the list keeps its own row.
/// Each entry passes `jump_target_test` (checked at compile time below).
pub(crate) const FUSED_TARGET_ROWS: [u16; 3] = [Op::JumpFalse as u16, Op::JumpTrue as u16, Op::JumpBinaryFalse as u16];

const _: () = {
    let mut k = 0;
    while k < FUSED_TARGET_ROWS.len() {
        assert!(jump_target_test(row_op(FUSED_TARGET_ROWS[k])), "a FUSED_TARGET_ROWS entry is not a conditional test");
        k += 1;
    }
};

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
pub(crate) const LOAD_LOCAL_PLAIN_ROW: u16 = Op::LoadLocalPlain as u16;
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

/// The row that runs a window's local access `op` (a load, store or initialization of a frame local
/// or an environment slot), or `None` where `op` is no local access.
pub(crate) const fn local_access_row(op: Op) -> Option<u16> {
    match op {
        Op::LoadLocal | Op::LoadEnvLocal => Some(LOCAL_LOAD_ROW),
        Op::LoadLocalPlain => Some(LOAD_LOCAL_PLAIN_ROW),
        Op::StoreLocal | Op::StoreEnvLocal => Some(LOCAL_STORE_ROW),
        Op::StoreLocalPlain => Some(STORE_LOCAL_PLAIN_ROW),
        Op::InitializeTdz => Some(LOCAL_INITIALIZE_ROW),
        _ => None,
    }
}

/// Whether `op` is a checked local access: its row reads and writes the frame's own locals in
/// place, and leaves to the general step (`Miss::General`) for any slot those locals do not hold.
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
/// Makes a closure (`MakeClosure`).
pub(crate) const fn makes_closure(op: Op) -> bool { matches!(op, Op::MakeClosure) }
/// Clones the environment of an iteration (`CloneEnv`).
pub(crate) const fn clones_environment(op: Op) -> bool { matches!(op, Op::CloneEnv) }
/// Accesses a local slot of its frame or its environment: a load, a store or an initialization
/// (`LoadLocal`, `LoadLocalPlain`, `LoadEnvLocal`, `StoreLocal`, `StoreLocalPlain`, `StoreEnvLocal`,
/// `InitializeTdz`).
pub(crate) const fn accesses_local_slot(op: Op) -> bool {
    matches!(op, Op::LoadLocal | Op::LoadLocalPlain | Op::LoadEnvLocal | Op::StoreLocal | Op::StoreLocalPlain
        | Op::StoreEnvLocal | Op::InitializeTdz)
}
/// Stores a local slot of its frame or its environment (`StoreLocal`, `StoreLocalPlain`, `StoreEnvLocal`).
pub(crate) const fn stores_local_slot(op: Op) -> bool { matches!(op, Op::StoreLocal | Op::StoreLocalPlain | Op::StoreEnvLocal) }
/// Stores an environment slot (`StoreEnvLocal`).
pub(crate) const fn stores_environment_slot(op: Op) -> bool { matches!(op, Op::StoreEnvLocal) }
/// Loads or stores an environment slot (`LoadEnvLocal`, `StoreEnvLocal`).
pub(crate) const fn addresses_environment_slot(op: Op) -> bool { matches!(op, Op::LoadEnvLocal | Op::StoreEnvLocal) }
/// Stores a captured slot (`StoreCapture`).
pub(crate) const fn writes_capture(op: Op) -> bool { matches!(op, Op::StoreCapture) }
/// Jumps on a falsy register (`JumpFalse`).
pub(crate) const fn jumps_when_false(op: Op) -> bool { matches!(op, Op::JumpFalse) }
/// Does nothing (`Nop`): a marker that a lane skips.
pub(crate) const fn does_nothing(op: Op) -> bool { matches!(op, Op::Nop) }
/// Stores a function's `var` binding (`StoreVarBinding`), a store to its slot where the function is nested.
pub(crate) const fn stores_var_binding(op: Op) -> bool { matches!(op, Op::StoreVarBinding) }
/// Reads `this` (`LoadThis`).
pub(crate) const fn loads_this(op: Op) -> bool { matches!(op, Op::LoadThis) }
/// Reads an `arguments` element (`ArgumentGet`).
pub(crate) const fn reads_argument(op: Op) -> bool { matches!(op, Op::ArgumentGet) }
/// Makes a call through its callee register with its arguments positional (`Call`) or forwarded
/// from the caller's `arguments` (`CallForwardingArguments`).
pub(crate) const fn calls_positional(op: Op) -> bool { matches!(op, Op::Call | Op::CallForwardingArguments) }
/// Makes a call through a method site (`CallMethod`), whose callee is a property of its receiver.
pub(crate) const fn calls_method_site(op: Op) -> bool { matches!(op, Op::CallMethod) }
/// Makes a call in any form that is not a construction (`Call`, `CallArray`, `CallSpread`,
/// `CallForwardingArguments`, `CallMethod`).
pub(crate) const fn makes_call(op: Op) -> bool {
    matches!(op, Op::Call | Op::CallArray | Op::CallSpread | Op::CallForwardingArguments | Op::CallMethod)
}
/// Whether `op` is a leaf record that only moves or computes an immediate value, or a branch a
/// leaf may hold (the catalog's `LEAF_VALUE` and `LEAF_JUMP` traits).
pub(crate) const fn leaf_value_or_jump(op: Op) -> bool { op.is_leaf_value() || op.is_leaf_jump() }
/// Whether `op` is a binary record a leaf admits with register or constant operands (`LEAF_BINARY`).
pub(crate) const fn leaf_binary_operation(op: Op) -> bool { op.is_leaf_binary() }
/// Computes a Wasm f64 binary operator (`WasmF64Binary`).
pub(crate) const fn wasm_f64_binary(op: Op) -> bool { matches!(op, Op::WasmF64Binary) }
/// Whether `op`'s record holds its i64 and f64 operands and result as raw machine words (the
/// `*Raw` Wasm classes, `compile::liveness::raw_webs`, and a raw i64 return). Its row reads and
/// writes those registers as they are (`Frame::raw_read`, `Frame::raw_write`): no tag check and
/// no boxing, so a word a tag claims is an ordinary operand.
pub(crate) const fn raw_representation(op: Op) -> bool {
    matches!(op.class(), crate::bytecode::Class::WasmI64BinaryRaw | crate::bytecode::Class::WasmI64BinaryImmediateRaw
        | crate::bytecode::Class::WasmI64UnaryRaw | crate::bytecode::Class::WasmFloatBinary64Raw
        | crate::bytecode::Class::WasmFloatUnary64Raw | crate::bytecode::Class::WasmReturn64)
}
/// Computes a Wasm f64 unary operator (`WasmF64Unary`).
pub(crate) const fn wasm_f64_unary(op: Op) -> bool { matches!(op, Op::WasmF64Unary) }
/// Loads an f64 from the function's memory (`WasmF64Load`).
pub(crate) const fn wasm_f64_load(op: Op) -> bool { matches!(op, Op::WasmF64Load) }
/// Stores an f64 to the function's memory (`WasmF64Store`).
pub(crate) const fn wasm_f64_store(op: Op) -> bool { matches!(op, Op::WasmF64Store) }
/// Binds the instance a Wasm function runs in (`WasmInstanceBinding`).
pub(crate) const fn wasm_instance_binding(op: Op) -> bool { matches!(op, Op::WasmInstanceBinding) }
/// Reads the size of the function's memory (`WasmMemorySize`).
pub(crate) const fn wasm_memory_size(op: Op) -> bool { matches!(op, Op::WasmMemorySize) }
/// Fills a range of the function's memory (`WasmMemoryFill`).
pub(crate) const fn wasm_memory_fill(op: Op) -> bool { matches!(op, Op::WasmMemoryFill) }
/// Fills a Wasm function's registers (`WasmFillRegisters`).
pub(crate) const fn wasm_fill_registers(op: Op) -> bool { matches!(op, Op::WasmFillRegisters) }
/// Awaits a value (`Await`).
pub(crate) const fn awaits(op: Op) -> bool { matches!(op, Op::Await) }
/// Constructs an object (`Construct`).
pub(crate) const fn constructs(op: Op) -> bool { matches!(op, Op::Construct) }
/// Selects a case by a string (`StringCaseIndex`).
pub(crate) const fn selects_string_case(op: Op) -> bool { matches!(op, Op::StringCaseIndex) }
/// Stores an environment slot (`StoreEnvLocal`).
pub(crate) const fn writes_environment_slot(op: Op) -> bool { matches!(op, Op::StoreEnvLocal) }
/// Checks an iterator result object (`RequireIteratorResult`).
pub(crate) const fn requires_iterator_result(op: Op) -> bool { matches!(op, Op::RequireIteratorResult) }
/// Names a binding (the catalog's name operations).
pub(crate) const fn names_binding(op: Op) -> bool { op.is_name_operation() }
/// Stores a field of an object (the catalog's field stores).
pub(crate) const fn stores_field(op: Op) -> bool { op.is_field_store() }
/// Computes a numeric binary operator (`JsDenotation::NumericBinary`).
pub(crate) fn computes_numeric_binary(op: Op) -> bool { matches!(op.js_denotation(), crate::bytecode::JsDenotation::NumericBinary) }

// Class families: the record families a tier's lowering and lane handlers select by. Each answers
// exactly its catalog class, so a family query and the class it names agree on every opcode.
/// The float binary operator family of an f64 selector (`Class::WasmFloatBinary64`).
pub(crate) const fn float_binary64_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::WasmFloatBinary64) }
/// The float binary operator family of an f32 selector (`Class::WasmFloatBinary32`).
pub(crate) const fn float_binary32_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::WasmFloatBinary32) }
/// The construction family (`Class::Construct`).
pub(crate) const fn construction_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Construct) }
/// The call family (`Class::Call`).
pub(crate) const fn call_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Call) }
/// The argument-array call family (`Class::CallArray`).
pub(crate) const fn call_array_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CallArray) }
/// The spread call family (`Class::CallSpread`).
pub(crate) const fn call_spread_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CallSpread) }
/// The known-callee call family (`Class::CallKnown`).
pub(crate) const fn known_call_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CallKnown) }
/// The forwarding-arguments call family (`Class::CallForwardingArguments`).
pub(crate) const fn forwarded_call_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CallForwardingArguments) }
/// The return family (`Class::Return`).
pub(crate) const fn return_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Return) }
/// The private field definition family (`Class::DefinePrivateField`).
pub(crate) const fn private_field_definition_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::DefinePrivateField) }
/// The private name marking family (`Class::MarkPrivateName`).
pub(crate) const fn private_name_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::MarkPrivateName) }
/// The private brand check family (`Class::CheckPrivate`).
pub(crate) const fn private_check_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CheckPrivate) }
/// The private `in` family (`Class::PrivateIn`).
pub(crate) const fn private_in_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::PrivateIn) }
/// The named field read family (`Class::GetField`).
pub(crate) const fn field_read_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::GetField) }
/// The element read family (`Class::GetIndex`).
pub(crate) const fn element_read_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::GetIndex) }
/// The element write family (`Class::SetIndex`).
pub(crate) const fn element_write_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::SetIndex) }
/// The property record definition family (`Class::DefinePropertyRecord`).
pub(crate) const fn property_record_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::DefinePropertyRecord) }
/// The field definition family (`Class::DefineField`).
pub(crate) const fn field_definition_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::DefineField) }
/// The data property copy family (`Class::CopyDataProperties`).
pub(crate) const fn copy_properties_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CopyDataProperties) }
/// The property delete family (`Class::Delete`).
pub(crate) const fn delete_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Delete) }
/// The constant array family (`Class::MakeConstArray`).
pub(crate) const fn const_array_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::MakeConstArray) }
/// The object pair family (`Class::MakeObject2`).
pub(crate) const fn object_pair_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::MakeObject2) }
/// The super constant array pair family (`Class::SuperConstArrayObject2`).
pub(crate) const fn super_const_array_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::SuperConstArrayObject2) }
/// The object literal family (`Class::MakeObjectLiteral`).
pub(crate) const fn object_literal_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::MakeObjectLiteral) }
/// The unary operator family (`Class::Unary`).
pub(crate) const fn unary_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Unary) }
/// The binary operator family (`Class::Binary`).
pub(crate) const fn binary_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Binary) }
/// The unconditional jump family (`Class::Jump`).
pub(crate) const fn jump_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Jump) }
/// The local read family (`Class::LoadLocal`, `Class::LoadLocalPlain`).
pub(crate) const fn local_read_family(op: Op) -> bool {
    matches!(op.class(), crate::bytecode::Class::LoadLocal | crate::bytecode::Class::LoadLocalPlain)
}
/// The Wasm register-fill and instance-binding family (`Class::WasmFillRegisters`, `Class::WasmInstanceBinding`).
pub(crate) const fn wasm_setup_family(op: Op) -> bool {
    matches!(op.class(), crate::bytecode::Class::WasmFillRegisters | crate::bytecode::Class::WasmInstanceBinding)
}

// Row constants of the opcodes the lane's tables name by index.
/// The opcode whose row runs a Wasm f64 load (`WasmF64Load`).
pub(crate) const WASM_F64_LOAD_ROW: u16 = Op::WasmF64Load as u16;
/// The opcode whose row runs a Wasm f64 store (`WasmF64Store`).
pub(crate) const WASM_F64_STORE_ROW: u16 = Op::WasmF64Store as u16;
/// The opcode whose row increments or decrements a Number (`IncDec`).
pub(crate) const INC_DEC_ROW: u16 = Op::IncDec as u16;
/// The opcode whose row writes a capture (`StoreCapture`).
pub(crate) const STORE_CAPTURE_ROW: u16 = Op::StoreCapture as u16;


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
mod stage_coverage_tests;
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
mod wasm_gc_tests;
#[cfg(test)]
mod wasm_simd_tests;
#[cfg(test)]
mod instanceof_tests;
