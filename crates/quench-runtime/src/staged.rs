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
mod word_ops;
pub mod numeric;
/// The stencil layout numbers, one source for the stencils (`#[path]`) and the runtime's const checks.
pub mod layout_offsets;
/// The head of the dispatch payload, which the runtime embeds and the stencils take offsets from.
pub mod js_step_head;
pub mod record_heads;
pub(crate) mod encodings;
/// The value encoding (tags, handle bits, narrow layout), shared with the stencils.
pub mod value_encoding;
/// The field cache entry's stored words and complement encoding, shared with the stencils.
pub mod field_cache_layout;
/// The object, array and record words (storage forms, tags, prototype ranges, record blocks), shared with the stencils.
pub mod object_encoding;
/// The head of an object's extras record (inline values, arena start, element backing).
pub(crate) mod object_extras;
/// The head of a property buffer (base pointer and length), embedded first by `PropertyBuffer`.
pub(crate) mod property_buffer;
/// The flat string forms' heads and offsets, shared with the stencils (`vm::wtf16::native_string`).
pub mod string_form;
/// The typed-array kinds, one file for the runtime and the stencils (`heap/cell/typed_array_catalog.rs`).
#[path = "heap/cell/typed_array_catalog.rs"]
pub(crate) mod typed_array_catalog;
/// The b-jit's shared words, as the stencils take them.
pub(crate) use quench_bjit::shared_words;
pub mod flow;
pub mod machine;
#[cfg_attr(not(feature = "jit"), allow(dead_code))] // a specializer's composition
pub mod barrier;
#[cfg_attr(not(feature = "jit"), allow(dead_code))] // a specializer's operations; the interpreter has native Numbers
pub mod number_machine;
mod heap;
mod superinstructions;
mod global;
#[cfg_attr(not(test), allow(dead_code))] // the division is data the specializers will ask; today only its tests do
pub mod division;
pub mod math;
pub(crate) mod int_math;
mod names;
pub mod protocol_literals;
pub mod protocol_classes;
pub(crate) mod protocol_values;
pub mod protocol_owned;
pub(crate) mod protocol_wasm;
mod callout;
mod spread;
mod admission;
mod delete;
mod operation;
pub use operation::Operation;
pub use operation::kind as op_kind;
mod binary_class;
#[allow(unused_imports)]
pub use binary_class::BinaryClass;
mod edge;
pub use edge::Edge;

mod decode;
pub mod meaning_fast;
pub mod meaning;
mod captures;
pub mod compare;
pub(crate) mod meaning_dispatch;
pub(crate) mod frame_arm;
#[allow(unused_imports)]
pub use global::{lexical_read, GlobalBinding, GlobalFrame};
pub use names::Protocol;
pub use callout::{CallOut, OperandValues};
pub use spread::Passing;
pub use decode::{DecodeContext, Fields};
#[allow(unused_imports)]
pub use compare::{nullish_of, Comparison};
pub use concrete::Concrete;
#[allow(unused_imports)]
pub use numeric::{Domain, NegativeZero, NumberOperation, NumericFrame, NumericStage, Overflow};
pub mod builtins;
pub mod add;
pub mod add_plan;
pub mod elements;
pub(crate) mod double_view;
pub mod element_rows;
pub mod typed_elements;
pub mod typed_view;
pub mod data_view;
mod element_frame;
pub use element_frame::ElementFrame;
pub mod entry;
pub mod environment;
pub mod ic;
pub(crate) mod ic_flat;
pub(crate) mod ic_read;
pub mod object;
pub mod narrow;
pub mod store;
pub(crate) mod slot_store;
pub mod strings;
pub mod string_length;
pub(crate) mod code_units;
pub(crate) mod parse_int;
/// ASCII text builtins (trim, case, pad, repeat): one body per builtin.
pub(crate) mod string_text;
/// String search builtins (indexOf, lastIndexOf, includes, startsWith, endsWith) and cuts (slice, substring, substr).
pub(crate) mod string_search;
/// String.prototype.split with a string separator: the cut offsets, once.
pub(crate) mod string_split;
/// String.prototype.replace/replaceAll with a string pattern: the plan and the write, once.
pub(crate) mod string_replace;
/// Map/Set/WeakMap/WeakSet hash and probe: one lookup body.
pub(crate) mod collections;
/// Array indexOf/includes/lastIndexOf over dense elements: one search body.
pub mod array_search;
/// BigInt small-magnitude arithmetic and comparison: one body.
pub(crate) mod bigint_small;
/// The calendar fields of a Date's time value, derived once for the getters, setters and `toISOString` (`vm::date`).
pub(crate) mod date_fields;
pub(crate) mod holder;
pub(crate) mod iteration;
pub mod array_iteration;
pub mod naming;
pub mod call; pub mod frame;
pub mod forms;
pub mod protocol;
pub mod call_keeping;
pub mod call_environment;
pub mod call_leaf;
pub mod call_construct;
pub mod call_callback;
pub mod call_body;
pub mod call_native;
pub mod call_complete;
pub mod call_iteration;
#[allow(unused_imports)]
pub use forms::{Arguments, CallFacts, CallForm, Callee, FunctionId, Invocation, Receiver, RegisterList};
pub use protocol::CallOperation;
pub(crate) mod invoke;
pub(crate) mod prologue;
#[allow(unused_imports)]
pub use prologue::{step_entry, PrologueFrame};
pub mod plan;
mod native_call;
#[allow(unused_imports)]
pub use plan::{plan_entry, CalleeKind, CallEntry, DirectPlan, Identity, Resolved};
#[cfg(test)]
pub(crate) use plan::Bound;
#[allow(unused_imports)]
pub use invoke::{CallFrame, Entered, Room};
mod suspend;
#[allow(unused_imports)]
pub use suspend::{await_source, suspend as suspension_step, SuspendEffect, Suspending, Suspension, YieldConversion};
mod delegation;
pub use delegation::DelegationStep;
mod allocation;
#[allow(unused_imports)]
pub use allocation::{Allocation, InlineEmpty};
pub mod plan_fields;
pub mod property;
mod property_write;
pub use property::{NoPlans, PropertyFrame};
pub mod value;
pub mod poll;
mod guard;
mod steps;
mod control;
mod generic;
#[allow(unused_imports)]
pub use steps::{step, step_of, step_out_of_line, row_step, RowStep, step_flow, step_wasm_int, step_local, step_number, step_value, step_names, step_property, step_keyed, step_generic, step_wasm_float, step_wasm_jump, step_wasm_state, step_wasm_access, step_wasm_vector, step_arith};
pub(crate) mod effects;
pub(crate) mod function_plan;
pub mod region;
pub mod wasm;
pub mod wasm_float;
pub(crate) mod wasm_access;
pub(crate) mod wasm_int;
pub(crate) mod wasm_jump;
pub(crate) mod wasm_state;
pub(crate) use wasm_jump::BranchTable;
pub mod wasm_concrete;
pub mod wasm_gc;
pub mod wasm_exception;
pub mod wasm_call;
pub mod wasm_roots;
pub mod wasm_flow;
pub mod wasm_instance;
#[cfg(not(stencil_build))]
pub mod wasm_activation;
pub mod wasm_machine;
pub mod wasm_memory;
pub mod wasm_memory_vector;
pub mod wasm_memory_bytes;
pub mod wasm_table_ops;
pub mod wasm_operation;
pub mod wasm_simd;
pub mod wasm_simd_rules;
pub(crate) mod wasm_simd_concrete;
pub mod wasm_simd_lanes;
pub mod wasm_step;
pub mod wasm_atomic;
pub(crate) mod wasm_emit;
pub use wasm_concrete::ConcreteWasm;
#[cfg(test)]
pub(crate) mod trace;
#[allow(unused_imports)] // the Stage v2 vocabulary: stencil and UltraJIT stages use the rest
pub use effects::{Effects, Region, RegionSet};
#[allow(unused_imports)]
pub use region::Access;
#[allow(unused_imports)]
pub use ic::{HINTED_FORM, TAGGED_SLOTS, concrete_hinted_own_field_hit, concrete_own_field_hit, concrete_site_own_chain, concrete_site_own_read, concrete_site_store};
#[allow(unused_imports)]
pub use object::{Form, Layout, Slots, concrete_own_read, arena_extras_layout};

mod stage;
pub use stage::{fill_by_rows, Passed, StagedRecordSpace, Stage, Table, word_bytes, word_offset};

mod register_frame;
#[allow(unused_imports)]
pub use register_frame::{Constant, Frame, Step, WasmRep, jump_if, load_this, logical_not, read_arguments_binding, read_initialized_local, read_local, write_initialized_local, write_local};

/// Where a row goes when its frame cannot decide a step: the VM's general step, or, for a
/// capture access whose scopes the frame does not hold, the walk over the scopes from the
/// activation's environment (a read) or the store into the scope it finds (a write).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Miss {
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
    /// A by-name load the frame does not answer: its first run resolves the name, and a later
    /// run whose binding misses takes that binding's residual (`NameLoad`).
    NameLoad(NameLoad),
}

/// The by-name loads a `LoadName` family row answers: a plain read, a call's read with its
/// receiver (`LoadNameCall`), and `typeof`'s read of an absent name as `undefined`
/// (`LoadNameTypeof`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NameLoad {
    Read,
    Call,
    Typeof,
}

/// The by-name load `op` is, or `None` for any other opcode.
pub const fn name_load(op: Op) -> Option<NameLoad> {
    match op {
        Op::LoadName => Some(NameLoad::Read),
        Op::LoadNameCall => Some(NameLoad::Call),
        Op::LoadNameTypeof => Some(NameLoad::Typeof),
        _ => None,
    }
}

/// The row of `op`'s miss route (`Miss`).
pub const fn miss(op: Op) -> Miss {
    match op {
        Op::LoadCapture => Miss::WalkScopes,
        Op::StoreName => Miss::StoreNameFirst,
        Op::JumpBinaryFalse => Miss::CompareChain,
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
/// Each entry is `(A, B, MODE)`. `MODE` is one of the `FUSED_*` constants below: a table mode that
/// `fuse_rows` realizes as the record's settings bits (`MODE_DEAD_FIRST`, `raw_bit(0)`), not as an instance.
pub const FUSED_ROWS: [(u16, u16, u8); 90] = [
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
    // `(a + b) | 0`. Both rows run in full; the first's double result stays raw where the second
    // reads it and nothing after does (`FUSED_RAW`, the hand pair's raw result made general).
    (Op::Binary as u16, Op::Binary as u16, FUSED_RAW),
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
    // A comparison jump is not a member: its own row fixes its operator and operand form at its first
    // run (`compare_jump_row`), which a pair's jump decodes at run time, and a pair's jump builds the
    // capture read's whole frame (`row_frame`).
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
    // f64 arithmetic then f64 arithmetic (any operator, comparisons included: each row decodes its own
    // operator from its immediate). Subsumes the boxed float pairs (`lane_f64_pair`, `FloatPairs::HANDLERS`).
    (Op::WasmF64Binary as u16, Op::WasmF64Binary as u16, FUSED_PLAIN),
    // The raw form of the same pair: both rows read and write raw words (`raw_representation`).
    // Subsumes the raw float pairs (`lane_f64_pair` with `RAW`).
    (Op::WasmF64BinaryRaw as u16, Op::WasmF64BinaryRaw as u16, FUSED_PLAIN),
    // An f64 load then the f64 operator on the loaded value. Subsumes `lane_f64_load_pair`.
    (Op::WasmF64Load as u16, Op::WasmF64Binary as u16, FUSED_PLAIN),
    // An f64 operator then the f64 store of its result. Subsumes `lane_f64_store_pair`.
    (Op::WasmF64Binary as u16, Op::WasmF64Store as u16, FUSED_PLAIN),
];

/// Runs of three or four generated rows the lane runs as one record (`chain_rows`): each
/// entry's links in record order, `NO_LINK` padding a three-link chain. Only the last link may
/// be a jump. A chain at a record wins over a `FUSED_ROWS` pair there. A link whose result only the
/// next link reads, and dies there, keeps its double result raw where `chain_raw_links` allows it
/// (the record's raw mask, `staged::raw_bit`).
pub const FUSED_CHAINS: [[u16; 4]; 42] = [
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
    // `mem[x] op= v` on f64: the load, the f64 operator on the loaded value, then the f64 store of its
    // result. Subsumes `lane_f64_update` (`FloatPairs::UPDATES`).
    [Op::WasmF64Load as u16, Op::WasmF64Binary as u16, Op::WasmF64Store as u16, NO_LINK],
];

/// Whether every `FUSED_ROWS` pair and `FUSED_CHAINS` chain joins rows of one representation
/// (`raw_representation`): a raw word is not a boxed value, so no fused record mixes them.
const fn fused_links_share_representation() -> bool {
    let mut pair = 0;
    while pair < FUSED_ROWS.len() {
        let (a, b, _) = FUSED_ROWS[pair];
        match (Op::from_index(a as usize), Op::from_index(b as usize)) {
            (Some(a), Some(b)) => {
                if raw_representation(a) != raw_representation(b) {
                    return false;
                }
            }
            _ => return false,
        }
        pair += 1;
    }
    let mut chain = 0;
    while chain < FUSED_CHAINS.len() {
        let links = FUSED_CHAINS[chain];
        let mut link = 0;
        let mut first: Option<bool> = None;
        while link < 4 {
            if links[link] != NO_LINK {
                let Some(op) = Op::from_index(links[link] as usize) else { return false };
                let raw = raw_representation(op);
                match first {
                    None => first = Some(raw),
                    Some(earlier) => {
                        if earlier != raw {
                            return false;
                        }
                    }
                }
            }
            link += 1;
        }
        chain += 1;
    }
    true
}
const _: () = assert!(fused_links_share_representation(), "a fused pair or chain mixes raw and boxed rows");

/// The runtime settings word of a fused record, `LaneInstruction::mode`. The fusion passes
/// (`fuse_rows`, `fuse_chains`, `fuse_target_backedges`, `update_fusion`) write it with the record's
/// handler, and the handler reads it at run time. A fused handler's const parameters are its step
/// family only (`FUSED_PLAIN`, `FUSED_WINDOW`, `FUSED_TARGET`) and its rows; the bits below are the
/// settings of one record of that family.
///
/// Bit 0 (`MODE_DEAD_FIRST`): a pair's `A` register write is skipped, which liveness proves.
pub const MODE_DEAD_FIRST: u64 = 1 << 0;
/// Bit 1 (`MODE_BACKWARD`): the record's jump is backward (its distance is not positive). A branch
/// table's direction is a const of its handler instead, so only other jumps read this bit.
pub const MODE_BACKWARD: u64 = 1 << 1;
/// Where the raw mask starts. Bit `RAW_SHIFT + k` set means link `k`'s double result stays raw
/// (`LaneFrame::raw_result`): a pair's `A` is link 0, and a pair's `B` or a chain's last link is
/// `CHAIN_RAW_OUT`. A raw result is read by the next link (or the next record) as a Number operand.
pub(crate) const RAW_SHIFT: u32 = 8;
/// The raw-mask bit `link` of a record's `mode`.
#[inline(always)]
pub const fn raw_bit(link: u8) -> u64 { 1 << (RAW_SHIFT + link as u32) }
/// The raw-mask bit of a chain's last link (a pair's `B`): its double result stays raw into the record
/// after the chain, or past the plain records between that name neither its register (`raw_out_link`),
/// where a Number binary row reads it (`fuse_chains`, `number_binary_row`). No in-chain link names it
/// (`chain_raw_links` names only links with a next link in the chain), so it is free at either length.
pub const CHAIN_RAW_OUT: u8 = 3;
/// The raw-out bit of a record: its last link's double result stays raw for the record after it. Only a
/// Number operator's last link takes it.
pub const MODE_RAW_OUT: u64 = raw_bit(CHAIN_RAW_OUT);
/// The raw mask of a chain record, the bits of its `mode` from `RAW_SHIFT`, for links `raw` (bit `k`).
pub const fn raw_mask_word(raw: u8) -> u64 { (raw as u64) << RAW_SHIFT }
/// The row of a chain's last link (its last entry that is not `NO_LINK`).
pub const fn chain_last_row(chain: [u16; 4]) -> u16 {
    if chain[3] == NO_LINK { chain[2] } else { chain[3] }
}
/// The raw-mask bits `fuse_chains` may set for `chain`: `chain_raw_links`, plus `CHAIN_RAW_OUT` where the
/// chain's last link is a Number operator (its raw result may feed the record after the chain).
pub const fn chain_raw_eligible(chain: [u16; 4]) -> u8 {
    chain_raw_links(chain) | (if number_operator(row_op(chain_last_row(chain))) { 1 << CHAIN_RAW_OUT } else { 0 })
}
/// The row of a Number binary operator (`JsDenotation::NumericBinary`): `Binary`, `NumericAdd`, `NumericMultiply`.
/// Its operands are read as Numbers (`numeric::binary_operation`), so a raw double operand is one. These
/// are the only rows a raw result (`MODE_RAW_OUT`) may feed: a Unary's operand reads as an int32 first.
pub const BINARY_ROW: u16 = Op::Binary as u16;
/// The row of `NumericAdd`, a Number binary operator (see `BINARY_ROW`).
pub const NUMERIC_ADD_ROW: u16 = Op::NumericAdd as u16;
/// The row of `NumericMultiply`, a Number binary operator (see `BINARY_ROW`).
pub const NUMERIC_MULTIPLY_ROW: u16 = Op::NumericMultiply as u16;

/// Whether the result of a `FUSED_CHAINS` link may stay raw where the next link reads it: the next
/// link is a keyed element access (`keyed_element`), which reads its key as a Number, or both links
/// are Number operators (`number_operator`). Bit `k` is for link `k`; the last link and a link whose
/// next link is `NO_LINK` name none. `fuse_chains` also requires, per record, that the register is
/// read only there and dies after (or is overwritten by) the next link.
pub(crate) const fn chain_raw_links(chain: [u16; 4]) -> u8 {
    let mut mask = 0;
    let mut k = 0;
    while k < 3 {
        if chain[k + 1] != NO_LINK && raw_result_link(row_op(chain[k]), row_op(chain[k + 1])) { mask |= 1 << k; }
        k += 1;
    }
    mask
}

/// The bits of `chain_raw_links` whose next link is a keyed element access, which take the key
/// rule: none, as no keyed link keeps a raw key (`raw_result_link`).
pub const fn chain_keyed_links(_chain: [u16; 4]) -> u8 { 0 }

/// Whether `producer`'s result may stay raw in `consumer`, judged by meaning alone.
/// A keyed consumer is not one: its key guard wants an int32, which an integral raw double is not,
/// so a raw key would take the row's miss every time.
const fn raw_result_link(producer: Op, consumer: Op) -> bool {
    number_operator(producer) && number_operator(consumer)
}

/// The absent fourth link of a three-link `FUSED_CHAINS` entry.
pub const NO_LINK: u16 = u16::MAX;
/// The opcode an absent chain link stands for, where its untaken branch still names `NO_LINK`.
pub const NO_LINK_OP: Op = Op::Nop;

/// `FUSED_ROWS` mode: both rows run in full, `A`'s result is written to its register.
pub const FUSED_PLAIN: u8 = 0;
/// `FUSED_ROWS` mode: `A`'s result register is dead after the pair, and `B` does not name it, so
/// the fused record skips `A`'s register write. A table mode, realized as the record's `MODE_DEAD_FIRST`
/// bit where liveness proves that (`fuse_rows`); otherwise the bit is clear. `B` must be a value row: a
/// jump's target liveness is not checked, so no jump pair names this mode.
pub const FUSED_DEAD_FIRST: u8 = 1;
/// `FUSED_ROWS` mode: `A` is a Number operator (`number_operator`) whose double result is written as
/// its raw double bits, not canonicalized to an int32 where integral (`LaneFrame::raw_result`). `B`
/// is a Number operator that reads `A`'s register, so it takes the raw double as its Number; `A`'s
/// register is dead after the pair, or `B` writes it. A table mode, realized as `raw_bit(0)` where
/// those hold (`fuse_rows`); otherwise the bit is clear. A raw register never reaches a store, a key or a jump.
pub const FUSED_RAW: u8 = 2;
/// The mode of a window pair or chain `fuse_rows` installs for a window local's (`FLAG_WINDOW`) first row: both
/// rows run at the window's flag, reading the window's locals. Not a table mode; `fuse_rows` names it.
pub const FUSED_WINDOW: u8 = 3;
/// The mode of a backward jump's record whose target run holds (`FUSED_TARGET` forms): the record runs,
/// in the one dispatch, the back edge's fuel check, then the target run, then continues at the test's
/// taken or fall-through edge. The target run is an optional prefix row (`FUSED_TARGET_PREFIX_ROWS`,
/// or none: `NO_LINK`) at the target, then a conditional test (`FUSED_TARGET_ROWS`) at the record
/// after the prefix (or at the target, with no prefix). The target record keeps its own row for every
/// other entry. Forms, by what precedes the back jump:
/// - a `FUSED_ROWS` pair whose second row is the jump (`fuse_target_backedges`): the pair, the jump and
///   the target run in one record. Subsumes `lane_inc_dec_loop` and `lane_inc_dec_small_loop` (an update
///   pair into a register or constant test), `lane_inc_dec_load_loop` (an update pair into a loaded bound
///   then its test), and an update pair into an update then its test;
/// - a lone back jump with no pair before it (`row_jump`'s target run, `lone_target_form`). Subsumes
///   `lane_jump_back_nullish` and `specialize_nullish_back_edges` (a nullish test), and
///   `lane_jump_back_update_small` for an update then a test directly;
/// - a lone back jump whose target run is a prefix row, a copy (`FUSED_TARGET_COPY_ROWS`) and the test
///   (`lone_copy_target_form`). The copy runs its register write as its own row does, so the test reads
///   the copied value from its register; the form is taken only where the test names the copy's
///   destination. Subsumes `lane_jump_back_update_small`'s copy-then-test variant.
/// The back edge's fuel check runs before the prefix, as the jump's own does (`take_jump`).
pub const FUSED_TARGET: u8 = 4;

/// The rows a back edge's target run may begin with (`FUSED_TARGET`): a plain local or capture read,
/// or an update, each a value row whose result the test after it reads. A target with no prefix is
/// the test itself (`NO_LINK`).
pub const FUSED_TARGET_PREFIX_ROWS: [u16; 3] = [LOAD_LOCAL_PLAIN_ROW, LOAD_CAPTURE_ROW, INC_DEC_ROW];

/// The row a back edge's target run may carry between its prefix row and its test (`FUSED_TARGET`'s second
/// prefix slot): a register copy (`copies_register`), whose destination the test reads.
pub const FUSED_TARGET_COPY_ROWS: [u16; 1] = [MOVE_ROW];

const _: () = {
    let mut k = 0;
    while k < FUSED_TARGET_PREFIX_ROWS.len() {
        assert!(target_prefix(row_op(FUSED_TARGET_PREFIX_ROWS[k])), "a FUSED_TARGET_PREFIX_ROWS entry is not a target prefix");
        k += 1;
    }
    let mut k = 0;
    while k < FUSED_TARGET_COPY_ROWS.len() {
        assert!(copies_register(row_op(FUSED_TARGET_COPY_ROWS[k])), "a FUSED_TARGET_COPY_ROWS entry is not a register copy");
        k += 1;
    }
};

/// The opcode whose row answers an arguments binding's count (`Frame::arguments_length`), which a
/// tier names to give the row a fact it proved at view time.
pub const ARGUMENTS_COUNT_ROW: u16 = Op::ArgumentsLength as u16;
/// The opcode whose row answers an `arguments` element (`Frame::argument`), which the lane's record
/// names its register base for (`LaneFrame::kept`).
pub const ARGUMENT_GET_ROW: u16 = Op::ArgumentGet as u16;
/// The opcode whose row stores an environment local (`Operation::StoreEnvLocal`), which a window whose names
/// resolve through its scope runs with its scope's store first (`Frame::scoped_local_store`).
pub const STORE_ENV_LOCAL_ROW: u16 = Op::StoreEnvLocal as u16;

/// The opcode whose row answers a `Unary` record: `+x`, `-x`, `!x`, `~x` and the index form of
/// `typeof` decode here; a `typeof` test of a name runs the general step.
pub const UNARY_ROW: u16 = Op::Unary as u16;
/// The opcode whose row stores a global name (`GlobalFrame::global_write`), which a tier names to
/// give the row its record's hint.
pub const GLOBAL_WRITE_ROW: u16 = Op::StoreName as u16;
/// The opcode whose row reads the running function (`Frame::callee`), which a tier names to give
/// the row the function it runs.
pub const LOAD_CALLEE_ROW: u16 = Op::LoadCallee as u16;
/// The opcode whose row clones the running activation's environment (`Operation::CloneEnv`), which
/// a tier names to give the row its environment window.
pub const CLONE_ENV_ROW: u16 = Op::CloneEnv as u16;
/// The opcode whose row reads a capture (`Operation::LoadCapture`), which a tier names to find the
/// captures a loop reads.
pub const LOAD_CAPTURE_ROW: u16 = Op::LoadCapture as u16;
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
pub const STRING_CASE_INDEX_ROW: u16 = Op::StringCaseIndex as u16;
/// The opcode whose row is an unconditional jump (`Operation::Jump`).
pub const JUMP_ROW: u16 = Op::Jump as u16;
/// The opcode whose row defines a register from a constant (`Operation::LoadConst`), which the
/// interpreter's constant table answers where the decode does not hold it.
pub const LOAD_CONST_ROW: u16 = Op::LoadConst as u16;
/// The opcode whose row writes a local slot from a register (`Operation::StoreLocalPlain`), which the
/// interpreter names where it records the local write of a fused binary.
pub const STORE_LOCAL_PLAIN_ROW: u16 = Op::StoreLocalPlain as u16;
/// The opcode whose row makes a closure (`Operation::Allocate` over `Allocation::Closure`).
pub const MAKE_CLOSURE_ROW: u16 = Op::MakeClosure as u16;

/// Whether `op` is a Number operator row: a binary or unary operator over Numbers
/// (`JsDenotation::NumericBinary`, `NumericUnary`). Its double result may stay raw (`FUSED_RAW`),
/// and it is the reader such a result feeds: a raw result is only read as a Number.
pub const fn number_operator(op: Op) -> bool {
    matches!(op.js_denotation(), crate::bytecode::JsDenotation::NumericBinary | crate::bytecode::JsDenotation::NumericUnary)
}

/// Whether `op` is a conditional jump whose row answers its control edge from a comparison of
/// two operands (`Operation::CompareJump`): a jump the lane runs as its generated row.
pub const fn compare_jump(op: Op) -> bool {
    matches!(op, Op::JumpBinaryFalse)
}

/// Whether `op` is a Wasm i32 conditional jump or fused add-and-jump (`Operation::WasmJump`): a
/// jump the lane runs as its generated row.
pub const fn wasm_i32_jump(op: Op) -> bool {
    matches!(op.staged(), Some(Decoder::WasmJump))
}

/// Whether `op` is a Wasm branch table (`Operation::BranchTable`): a jump the lane runs as its
/// generated row, whose edge is the entry its index picks (`Edge::Entry`).
pub const fn wasm_branch_table(op: Op) -> bool {
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
pub const fn binary(op: Op) -> bool {
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

/// Whether `op` reads a variable's value into a register (a plain local or a capture read), the prefix
/// a back edge's target run may begin with (`FUSED_TARGET_PREFIX_ROWS`).
pub(crate) const fn reads_variable(op: Op) -> bool {
    matches!(op, Op::LoadLocalPlain | Op::LoadCapture)
}

/// Whether `op` may begin a back edge's target run (`FUSED_TARGET`): a variable read the test after it
/// reads, or an update the test after it reads (`increments`).
pub(crate) const fn target_prefix(op: Op) -> bool {
    reads_variable(op) || increments(op)
}

/// The conditional tests `FUSED_TARGET` instantiates a fused back edge for. `fuse_target_backedges`
/// matches a target record's handler against each one's row; a target outside the list keeps its own row.
/// Each entry passes `jump_target_test` (checked at compile time below).
pub const FUSED_TARGET_ROWS: [u16; 3] = [Op::JumpFalse as u16, Op::JumpTrue as u16, Op::JumpBinaryFalse as u16];

const _: () = {
    let mut k = 0;
    while k < FUSED_TARGET_ROWS.len() {
        assert!(jump_target_test(row_op(FUSED_TARGET_ROWS[k])), "a FUSED_TARGET_ROWS entry is not a conditional test");
        k += 1;
    }
};

/// Whether `op` copies a register (`Move`).
pub const fn copies_register(op: Op) -> bool {
    matches!(op, Op::Move)
}

/// Whether `op` is a Wasm SIMD operator (`WasmSimd`) on the lane's vector registers.
pub const fn wasm_simd(op: Op) -> bool {
    matches!(op, Op::WasmSimd)
}

/// Whether `op` is a Wasm `i8x16.shuffle` (`WasmSimdShuffle`), whose lane indices are its constant.
pub const fn wasm_simd_shuffle(op: Op) -> bool {
    matches!(op, Op::WasmSimdShuffle)
}

/// Whether `op` is a Wasm effective address (`WasmMemoryAddress`) of the function's memory.
pub const fn wasm_memory_address(op: Op) -> bool {
    matches!(op, Op::WasmMemoryAddress)
}

/// Whether `op` is a generic Wasm memory load (`WasmMemoryLoad`) of the function's memory.
pub const fn wasm_memory_load(op: Op) -> bool {
    matches!(op, Op::WasmMemoryLoad)
}

/// Whether `op` is a generic Wasm memory store (`WasmMemoryStore`) of the function's memory.
pub const fn wasm_memory_store(op: Op) -> bool {
    matches!(op, Op::WasmMemoryStore)
}

/// The opcodes of the Wasm i32 zero tests, which the lane names to give a zero test's row the
/// handler of its direction (`jump_row`).
pub const WASM_I32_ADD_IMMEDIATE_ROW: u16 = Op::WasmI32AddImmediate as u16;
/// The opcodes of the by-name load rows, one per `NameLoad`, which a tier names to instantiate
/// the row of a binding it resolved (`lane_frame::name_row`).
pub const LOAD_NAME_ROW: u16 = Op::LoadName as u16;
pub const LOAD_NAME_CALL_ROW: u16 = Op::LoadNameCall as u16;
pub const LOAD_NAME_TYPEOF_ROW: u16 = Op::LoadNameTypeof as u16;
/// The opcodes whose rows are the checked local accesses (`checked_local`), which a window's
/// specialization recognizes by their row.
pub const LOCAL_LOAD_ROW: u16 = Op::LoadLocal as u16;
pub const LOCAL_STORE_ROW: u16 = Op::StoreLocal as u16;
pub const LOCAL_INITIALIZE_ROW: u16 = Op::InitializeTdz as u16;
pub const LOAD_LOCAL_PLAIN_ROW: u16 = Op::LoadLocalPlain as u16;
/// The opcodes whose rows read and write the running captured environment's slots (`LoadEnvLocal`,
/// `StoreEnvLocal`), which a frame's specialization rewrites where the slot is not captured.
pub const LOAD_ENV_LOCAL_ROW: u16 = Op::LoadEnvLocal as u16;

/// The local slot a local-access record names. An environment window's record carries its register
/// count above the slot (`dispatch_fast::specialize_environment_window`), so the slot is the low half
/// of its immediate; every local decoder reads it through here.
pub const fn window_slot(imm: u32) -> u16 { (imm & u16::MAX as u32) as u16 }

/// The register base of the environment window a local-access record names, where its immediate
/// carries one (`window_slot` names the slot above it); `0` where none is named.
pub const fn window_base(imm: u32) -> u16 { (imm >> u16::BITS) as u16 }

/// The row that runs a window's local access `op` (a load, store or initialization of a frame local
/// or an environment slot), or `None` where `op` is no local access.
pub const fn local_access_row(op: Op) -> Option<u16> {
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
pub const fn checked_local(op: Op) -> bool {
    matches!(op, Op::LoadLocal | Op::StoreLocal | Op::InitializeTdz)
}


/// Whether `op`'s row writes its result register itself when it answers a value. A keyed for-in
/// read (`EnumeratedGet`) answers the word it read and leaves the write to the tier; a global
/// store has no result register, and answers the source word it stored; a store's result is its
/// `b` register and an InitializeTdz has none.
pub const fn row_writes_result(op: Op) -> bool {
    // A Wasm row writes the result register its record names, if any. A store, a global store and
    // a fill name none: the lane must not write the answered word into their record's `a`, which
    // a fill leaves at register 0 (`wasm_emit`'s fill emission).
    if matches!(op.staged(), Some(Decoder::WasmState | Decoder::WasmMemory)) {
        return true;
    }
    // An opcode whose A field is no result register defines none, so there is nothing for the lane
    // to write: its row's value (a cleanup pop's 0, a store's source) must not land in register A.
    if !matches!(op.field_layout(crate::bytecode::InstructionField::A), crate::bytecode::FieldLayout::ResultRegister) {
        return true;
    }
    // A keyed read's answer is written by the lane (its record's result register, as every
    // producer writes it beside the carried value).
    !matches!(op, Op::GetEnumerated | Op::GetIndex | Op::StoreName | Op::StoreLocal | Op::InitializeTdz | Op::DefineArrayElement
        | Op::IteratorCleanupPush | Op::IteratorCleanupPop)
}

/// The truthiness on which `op` jumps, for a jump on its register's truthiness
/// (`Operation::JumpIf`'s `taken`).
pub const fn truth_jump(op: Op) -> Option<bool> {
    match op {
        Op::JumpFalse => Some(false),
        Op::JumpTrue => Some(true),
        _ => None,
    }
}

/// The `Op` of the `*_ROW` constant `row`, the bridge from a row constant to the opcode a record is
/// built with (`WideInstruction::new`). Panics on a constant no opcode carries.
pub const fn row_op(row: u16) -> Op {
    match Op::from_index(row as usize) {
        Some(op) => op,
        None => panic!("a staged row constant names no opcode"),
    }
}

/// The `*_ROW` constants of the records a tier builds: a call, a move, a named field read and its
/// stores. `row_op` gives the record's `Op`.
pub(crate) const CALL_ROW: u16 = Op::Call as u16;
pub const MOVE_ROW: u16 = Op::Move as u16;
pub const GET_FIELD_ROW: u16 = Op::GetField as u16;
pub(crate) const SET_FIELD_ROW: u16 = Op::SetField as u16;
pub(crate) const SET_THIS_FIELD_ROW: u16 = Op::SetThisField as u16;

// Meaning queries: what the staged rows say `op` does, one query per meaning a tier lowers. A
// tier asks the query instead of naming opcodes; each answers exactly its row family.

/// Reads a named field (`GetField`).
pub const fn reads_named_field(op: Op) -> bool { matches!(op, Op::GetField) }
/// Reads an element by index (`GetIndex`).
pub const fn reads_element(op: Op) -> bool { matches!(op, Op::GetIndex) }
/// Stores an element by index (`SetIndex`).
pub const fn writes_element(op: Op) -> bool { matches!(op, Op::SetIndex) }
/// Reads or stores an element by a keyed index (`GetIndex`, `SetIndex`): its key register is read as
/// a Number index (`element_rows`' `index_key`).
pub(crate) const fn keyed_element(op: Op) -> bool { reads_element(op) || writes_element(op) }
/// Accesses an element through its object register as an indexed receiver (`GetIndex`,
/// `SetIndex`, `RequireObjectCoercible`).
pub const fn indexes_receiver(op: Op) -> bool { matches!(op, Op::GetIndex | Op::SetIndex | Op::RequireObjectCoercible) }
/// Computes a binary operator of two registers (`Binary`).
pub const fn computes_binary(op: Op) -> bool { matches!(op, Op::Binary) }
/// Computes an arithmetic binary operator (`Binary`, `NumericAdd`, `NumericMultiply`).
pub const fn computes_arithmetic(op: Op) -> bool { matches!(op, Op::Binary | Op::NumericAdd | Op::NumericMultiply) }
/// Computes a unary operator (`Unary`).
pub const fn computes_unary(op: Op) -> bool { matches!(op, Op::Unary) }
/// Defines a register from a constant (`LoadConst`).
pub const fn loads_constant(op: Op) -> bool { matches!(op, Op::LoadConst) }
/// Converts a value to a Number (`ToNumeric`).
pub const fn converts_numeric(op: Op) -> bool { matches!(op, Op::ToNumeric) }
/// Increments or decrements a Number (`IncDec`).
pub const fn increments(op: Op) -> bool { matches!(op, Op::IncDec) }
/// Makes a call (`Call`).
pub const fn calls(op: Op) -> bool { matches!(op, Op::Call) }
/// Makes a call of an argument array (`CallArray`).
pub const fn calls_array(op: Op) -> bool { matches!(op, Op::CallArray) }
/// Makes an ordinary call (`Call`, `CallArray`).
pub const fn calls_ordinary(op: Op) -> bool { matches!(op, Op::Call | Op::CallArray) }
/// Continues at the jump target (`Jump`).
pub const fn jumps(op: Op) -> bool { matches!(op, Op::Jump) }
/// Completes the activation (`Return`).
pub const fn returns(op: Op) -> bool { matches!(op, Op::Return) }
/// Reads a captured slot (`LoadCapture`).
pub const fn reads_capture(op: Op) -> bool { matches!(op, Op::LoadCapture) }
/// Reads a binding by name (`LoadName`).
pub const fn reads_name(op: Op) -> bool { matches!(op, Op::LoadName) }
/// Reads an environment slot (`LoadEnvLocal`).
pub const fn reads_environment_slot(op: Op) -> bool { matches!(op, Op::LoadEnvLocal) }
/// Defines an element of an array literal (`DefineArrayElement`).
pub const fn defines_array_element(op: Op) -> bool { matches!(op, Op::DefineArrayElement) }
/// Refuses `null` and `undefined` (`RequireObjectCoercible`).
pub const fn requires_object_coercible(op: Op) -> bool { matches!(op, Op::RequireObjectCoercible) }
/// Writes `this` as a field (`SetThisField`).
pub const fn writes_this_field(op: Op) -> bool { matches!(op, Op::SetThisField) }
/// Makes a closure (`MakeClosure`).
pub const fn makes_closure(op: Op) -> bool { matches!(op, Op::MakeClosure) }
/// Clones the environment of an iteration (`CloneEnv`).
pub const fn clones_environment(op: Op) -> bool { matches!(op, Op::CloneEnv) }
/// Accesses a local slot of its frame or its environment: a load, a store or an initialization
/// (`LoadLocal`, `LoadLocalPlain`, `LoadEnvLocal`, `StoreLocal`, `StoreLocalPlain`, `StoreEnvLocal`,
/// `InitializeTdz`).
pub const fn accesses_local_slot(op: Op) -> bool {
    matches!(op, Op::LoadLocal | Op::LoadLocalPlain | Op::LoadEnvLocal | Op::StoreLocal | Op::StoreLocalPlain
        | Op::StoreEnvLocal | Op::InitializeTdz)
}
/// Stores a local slot of its frame or its environment (`StoreLocal`, `StoreLocalPlain`, `StoreEnvLocal`).
pub const fn stores_local_slot(op: Op) -> bool { matches!(op, Op::StoreLocal | Op::StoreLocalPlain | Op::StoreEnvLocal) }
/// Stores an environment slot (`StoreEnvLocal`).
pub const fn stores_environment_slot(op: Op) -> bool { matches!(op, Op::StoreEnvLocal) }
/// Loads or stores an environment slot (`LoadEnvLocal`, `StoreEnvLocal`).
pub const fn addresses_environment_slot(op: Op) -> bool { matches!(op, Op::LoadEnvLocal | Op::StoreEnvLocal) }
/// Stores a captured slot (`StoreCapture`).
pub const fn writes_capture(op: Op) -> bool { matches!(op, Op::StoreCapture) }
/// Jumps on a falsy register (`JumpFalse`).
pub const fn jumps_when_false(op: Op) -> bool { matches!(op, Op::JumpFalse) }
/// Does nothing (`Nop`): a marker that a lane skips.
pub const fn does_nothing(op: Op) -> bool { matches!(op, Op::Nop) }
/// Stores a function's `var` binding (`StoreVarBinding`), a store to its slot where the function is nested.
pub const fn stores_var_binding(op: Op) -> bool { matches!(op, Op::StoreVarBinding) }
/// Reads `this` (`LoadThis`).
pub const fn loads_this(op: Op) -> bool { matches!(op, Op::LoadThis) }
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
pub const fn leaf_value_or_jump(op: Op) -> bool { op.is_leaf_value() || op.is_leaf_jump() }
/// Whether `op` is a binary record a leaf admits with register or constant operands (`LEAF_BINARY`).
pub const fn leaf_binary_operation(op: Op) -> bool { op.is_leaf_binary() }
/// Whether `op`'s record holds its i64 and f64 operands and result as raw machine words (the
/// `*Raw` Wasm classes, `compile::liveness::raw_webs`, and a raw i64 return). Its row reads and
/// writes those registers as they are (`Frame::raw_read`, `Frame::raw_write`): no tag check and
/// no boxing, so a word a tag claims is an ordinary operand.
pub const fn raw_representation(op: Op) -> bool {
    matches!(op.class(), crate::bytecode::Class::WasmI64BinaryRaw | crate::bytecode::Class::WasmI64BinaryImmediateRaw
        | crate::bytecode::Class::WasmI64UnaryRaw | crate::bytecode::Class::WasmFloatBinary64Raw
        | crate::bytecode::Class::WasmFloatUnary64Raw | crate::bytecode::Class::WasmReturn64)
}
/// Computes a Wasm f64 unary operator (`WasmF64Unary`).
pub const fn wasm_f64_unary(op: Op) -> bool { matches!(op, Op::WasmF64Unary) }
/// Binds the instance a Wasm function runs in (`WasmInstanceBinding`).
pub const fn wasm_instance_binding(op: Op) -> bool { matches!(op, Op::WasmInstanceBinding) }
/// Reads the size of the function's memory (`WasmMemorySize`).
pub const fn wasm_memory_size(op: Op) -> bool { matches!(op, Op::WasmMemorySize) }
/// Fills a range of the function's memory (`WasmMemoryFill`).
pub const fn wasm_memory_fill(op: Op) -> bool { matches!(op, Op::WasmMemoryFill) }
/// Fills a Wasm function's registers (`WasmFillRegisters`).
pub const fn wasm_fill_registers(op: Op) -> bool { matches!(op, Op::WasmFillRegisters) }
/// Awaits a value (`Await`).
pub const fn awaits(op: Op) -> bool { matches!(op, Op::Await) }
/// Constructs an object (`Construct`).
pub const fn constructs(op: Op) -> bool { matches!(op, Op::Construct) }
/// Selects a case by a string (`StringCaseIndex`).
pub const fn selects_string_case(op: Op) -> bool { matches!(op, Op::StringCaseIndex) }
/// Selects a case of a dense int switch (`IntCaseIndex`), whose slot the `WasmBranchTable` after it
/// reads as an int32.
pub const fn selects_int_case(op: Op) -> bool { matches!(op, Op::IntCaseIndex) }
/// A branch table (`WasmBranchTable`): the jump a dense int switch or a Wasm `br_table` takes.
pub const fn branch_table(op: Op) -> bool { matches!(op, Op::WasmBranchTable) }
/// Stores an environment slot (`StoreEnvLocal`).
pub const fn writes_environment_slot(op: Op) -> bool { matches!(op, Op::StoreEnvLocal) }
/// Checks an iterator result object (`RequireIteratorResult`).
pub const fn requires_iterator_result(op: Op) -> bool { matches!(op, Op::RequireIteratorResult) }
/// Names a binding (the catalog's name operations).
pub(crate) const fn names_binding(op: Op) -> bool { op.is_name_operation() }
/// Stores a field of an object (the catalog's field stores).
pub const fn stores_field(op: Op) -> bool { op.is_field_store() }
/// Computes a numeric binary operator (`JsDenotation::NumericBinary`).
pub fn computes_numeric_binary(op: Op) -> bool { matches!(op.js_denotation(), crate::bytecode::JsDenotation::NumericBinary) }

// Class families: the record families a tier's lowering and lane handlers select by. Each answers
// exactly its catalog class, so a family query and the class it names agree on every opcode.
/// The construction family (`Class::Construct`).
pub const fn construction_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Construct) }
/// The call family (`Class::Call`).
pub const fn call_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Call) }
/// The argument-array call family (`Class::CallArray`).
pub const fn call_array_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CallArray) }
/// The spread call family (`Class::CallSpread`).
pub const fn call_spread_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CallSpread) }
/// The known-callee call family (`Class::CallKnown`).
pub const fn known_call_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CallKnown) }
/// The forwarding-arguments call family (`Class::CallForwardingArguments`).
pub const fn forwarded_call_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CallForwardingArguments) }
/// The return family (`Class::Return`).
pub const fn return_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Return) }
/// The private field definition family (`Class::DefinePrivateField`).
pub const fn private_field_definition_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::DefinePrivateField) }
/// The private name marking family (`Class::MarkPrivateName`).
pub const fn private_name_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::MarkPrivateName) }
/// The private brand check family (`Class::CheckPrivate`).
pub const fn private_check_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CheckPrivate) }
/// The private `in` family (`Class::PrivateIn`).
pub const fn private_in_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::PrivateIn) }
/// The named field read family (`Class::GetField`).
pub const fn field_read_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::GetField) }
/// The element read family (`Class::GetIndex`).
pub const fn element_read_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::GetIndex) }
/// The element write family (`Class::SetIndex`).
pub const fn element_write_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::SetIndex) }
/// The property record definition family (`Class::DefinePropertyRecord`).
pub const fn property_record_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::DefinePropertyRecord) }
/// The field definition family (`Class::DefineField`).
pub const fn field_definition_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::DefineField) }
/// The data property copy family (`Class::CopyDataProperties`).
pub const fn copy_properties_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::CopyDataProperties) }
/// The property delete family (`Class::Delete`).
pub const fn delete_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Delete) }
/// The constant array family (`Class::MakeConstArray`).
pub const fn const_array_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::MakeConstArray) }
/// The object pair family (`Class::MakeObject2`).
pub const fn object_pair_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::MakeObject2) }
/// The super constant array pair family (`Class::SuperConstArrayObject2`).
pub const fn super_const_array_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::SuperConstArrayObject2) }
/// The object literal family (`Class::MakeObjectLiteral`).
pub const fn object_literal_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::MakeObjectLiteral) }
/// The unary operator family (`Class::Unary`).
pub const fn unary_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Unary) }
/// The binary operator family (`Class::Binary`).
pub const fn binary_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Binary) }
/// The unconditional jump family (`Class::Jump`).
pub const fn jump_family(op: Op) -> bool { matches!(op.class(), crate::bytecode::Class::Jump) }
/// The local read family (`Class::LoadLocal`, `Class::LoadLocalPlain`).
pub const fn local_read_family(op: Op) -> bool {
    matches!(op.class(), crate::bytecode::Class::LoadLocal | crate::bytecode::Class::LoadLocalPlain)
}
/// The Wasm register-fill and instance-binding family (`Class::WasmFillRegisters`, `Class::WasmInstanceBinding`).
pub const fn wasm_setup_family(op: Op) -> bool {
    matches!(op.class(), crate::bytecode::Class::WasmFillRegisters | crate::bytecode::Class::WasmInstanceBinding)
}

// Row constants of the opcodes the lane's tables name by index.
/// The opcode whose row increments or decrements a Number (`IncDec`).
pub const INC_DEC_ROW: u16 = Op::IncDec as u16;
/// The opcode whose row reads an element by index (`GetIndex`).
pub const ELEMENT_READ_ROW: u16 = Op::GetIndex as u16;
/// The opcode whose row writes a capture (`StoreCapture`).
pub const STORE_CAPTURE_ROW: u16 = Op::StoreCapture as u16;


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


#[cfg(test)]
mod test_words;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod property_tests;
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

/// Whether the staged layer gives `op` its meaning: a staged decoder of the catalog, a Wasm step
/// (`wasm_step::Step`), or nothing to derive (`NO_STEP`: `Nop`, and the never-dispatched `Wide`).
/// The coverage guard (`tests/staged_guards/coverage.rs`) counts every other opcode as debt.
pub fn op_has_row(op: crate::bytecode::Op) -> bool {
    op.staged().is_some() || op.traits().has(crate::bytecode::Traits::NO_STEP)
        || (0..1024).any(|tag| wasm_step::Step::of(op, tag).is_some())
}
