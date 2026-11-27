//! The call protocol's shared facts: the status words a call returns, the per-site record the
//! direct-call rows read, and the largest argument count a direct row exists for. One home:
//! `stencils/call_rows.rs` (compiled by rustc, no_std) includes this file by path, and the
//! runtime's planner (`quench-runtime/src/vm/compiled/base_region/call_plan.rs`) reads it as
//! `quench_bjit::call_record`; the runtime's own constants for these words are checked equal
//! there (`call_protocol_matches_the_runtime`). The status numbers are defined in
//! `quench-runtime/src/staged/lane_protocol.rs`, mounted here by path, and re-exported.

#[path = "../../quench-runtime/src/staged/lane_protocol.rs"]
mod lane_protocol;
// The runtime mounts this file by path inside private modules, where only some names are read.
#[allow(unused_imports)]
pub use lane_protocol::exit;
#[allow(unused_imports)]
pub use lane_protocol::{
    helper_outcome, return_status, return_status_pc, LaneStatus, Outcome, CALLEE_EXIT, GENERAL_DISPATCH_BLOCKS,
    GENERAL_RETURN_STATUS_BASE, LANE_BODY_NOT_ENTERED, LANE_BODY_RETURNED, LANE_BODY_THREW, LANE_CALL_BRANCHED,
    LANE_CALL_COMPLETED, LANE_CALL_HOLDS, LANE_CALL_LEFT, LANE_CALL_MISSED, LANE_CALL_RAISED, LANE_CALL_TERMINATED,
    NATIVE_JS_STOPPED, RETURN_PC_MASK, TERMINATED_STATUS,
};

/// The most arguments a direct-call row passes (`call_direct_a0` .. `call_direct_a6`); a call
/// with more takes the helper row.
pub const MAX_DIRECT_ARGC: usize = 6;

/// The leading arguments a direct-call row may take from a pinned register rather than the caller's
/// window slot (`CallRecord::argument_places`); the rest are always read from the window.
pub const PLACED_ARGUMENTS: usize = 2;

/// The value holes of a helper call row (`CallRow::Helper`, `CallRow::HelperPacked`), by position. The planner
/// (`helper_part` in `call_plan.rs`) fills each at its index; `stencils/call_rows.rs` reads it by the same index.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelperHole {
    /// The call pc the helper is told.
    CallPc = 0,
    /// The helper's address.
    Helper = 1,
    /// The packed row's instruction first word (its opcode and operands).
    PackedFirst = 2,
    /// The packed row's instruction immediate.
    PackedImm = 3,
}

/// The value holes of a numeric leaf call row (`CallRow::NumericLeaf`), by position: the destination's and the two
/// operands' frame offsets (the operands' pinned registers are spilled before the row, `call_plan::plan_leaf`), and the
/// leaf's address (a branch's symbol on Apple AArch64). The first three are the places a `Places` row reads (hole 0 the
/// destination, holes 1 and 2 the operands), so the row reads its operands as any Number row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NumericLeafHole {
    Destination = 0,
    Left = 1,
    Right = 2,
    Leaf = 3,
}

/// The value holes of a direct call row (`CallRow::Direct`), by position. The planner (`direct_part` in
/// `call_plan.rs`) fills each at its index; `stencils/call_rows.rs` reads it by the same index.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectHole {
    /// The address of the site's `CallRecord`.
    Record = 0,
    /// The call's pc, which a raise names to the lane.
    CallPc = 1,
}

/// Bytes of one register slot of the frame window (`size_of::<Value>()`, a word).
pub const REGISTER_BYTES: usize = core::mem::size_of::<u64>();

/// The callee's record of a direct call: the words that are the callee's (its closure identity, its cell and the
/// validity the call was planned under, its captures, its sloppy global, its function, its descriptor, its closure
/// environment and captures pointer, its window size and sloppiness). One per bound callee; a site's
/// `CallRecord` names it by address (`callee_record`), and a closed-set indirect call reads it from its class's
/// method vector (`layouts::class_methods`), so one callee record serves every site that reaches that callee.
/// Its field order is the offsets the stencils and the lane read by (`offset_of!(CalleeRecord, _)`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct CalleeRecord {
    pub callee: u64,
    pub cell: u64,
    pub expected: u64,
    /// The callee's capture word (`CONTEXT_BODY`).
    pub captures: u64,
    pub global_this: u64,
    /// The callee's function: the function bits (0-31) of its plain header's packed word (`CONTEXT_PACKED`). The flags and
    /// the argc are the site's (`CallRecord::packed`), so the header is the two words' union.
    pub function: u64,
    /// The callee's descriptor word: the address of its `DirectCallee`, which the native header's descriptor
    /// (`native_table`) names. Written by the binding of an image's proven call (`BoundCalls::bind`) and read by the
    /// AOT lane body's native call (`call_frames::push_native`); a JIT plan's record leaves it zero.
    pub descriptor: u64,
    /// The callee's closure environment (`CONTEXT_ENV`) and captures pointer (`CONTEXT_CAPTURE`): the
    /// interpreter reads both from a linked body frame's window (`ScriptCallTarget::env`, `capture`).
    pub env: u64,
    pub capture: u64,
    /// The callee's window size (`DirectPlan::registers`), which an indirect closed-set call reads at run time.
    pub registers: u64,
    /// 1 where the callee is sloppy (`DirectPlan::sloppy` is `Some`), else 0.
    pub sloppy: u64,
}

/// The per-site record of a direct-call row, one cache line, patched into the row as the one 64-bit value hole 0
/// (its address). Everything a call site knows that does not belong to the callee: the caller's call record and base,
/// the flags and argc of the plain header, the caller registers the call names, where each leading
/// argument's word is (`argument_places`: a place code, `MEM` for the caller's window slot, else the pinned register
/// holding the value), the caller's proven Number arguments, and the pointer to the callee's `CalleeRecord`. The plan
/// owns the record and must outlive the code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct CallRecord {
    /// The caller's call record the callee returns to (`CONTEXT_CALL`).
    pub call: u64,
    /// The caller's base instruction pointer (`CONTEXT_CALLER_BASE`).
    pub caller_base: u64,
    /// The arguments the caller proves Numbers (bit `k`: argument `k`). The row enters the callee's proven-arguments
    /// entry where every guard its cell names (`EntryCell::numbers`) is covered by these bits.
    pub proven_args: u64,
    /// The flags and the argc of the plain header (`CONTEXT_PACKED`), which the site fixes; the function bits are the
    /// callee's (`CalleeRecord::function`).
    pub packed: u64,
    /// The address of the callee's `CalleeRecord` (the one the row reads the callee's words from).
    pub callee_record: u64,
    pub callee_register: u16,
    pub this_register: u16,
    pub args_register: u16,
    pub result_register: u16,
    pub argument_places: [u8; PLACED_ARGUMENTS],
}

/// One cache line: the site record is aligned to one.
const _: () = assert!(core::mem::size_of::<CallRecord>() == core::mem::align_of::<CallRecord>());
