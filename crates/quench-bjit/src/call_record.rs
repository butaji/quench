//! The call protocol's shared facts: the status words a call returns, the per-site record the
//! direct-call rows read, and the largest argument count a direct row exists for. One home:
//! `stencils/call_rows.rs` (compiled by rustc, no_std) includes this file by path, and the
//! runtime's planner (`quench-runtime/src/vm/compiled/base_region/call_plan.rs`) reads it as
//! `quench_bjit::call_record`; the runtime's own constants for these words are checked equal
//! there (`call_protocol_matches_the_runtime`).

/// The most arguments a direct-call row passes (`call_direct_a0` .. `call_direct_a3`); a call
/// with more takes the helper row.
pub const MAX_DIRECT_ARGC: usize = 3;

/// Bytes of one register slot of the frame window (`size_of::<Value>()`).
pub const REGISTER_BYTES: usize = 8;

/// What a helper call returned (`vm::compiled::LANE_CALL_*`): completed with its result in the
/// call's register, missed before any effect, left the lane inside the callee's activation,
/// terminated the run, or completed a step that branched (`LANE_CALL_BRANCHED`: a conditional
/// jump of the interpreter's own step, taken).
pub const LANE_CALL_COMPLETED: u64 = 0;
pub const LANE_CALL_MISSED: u64 = 1;
pub const LANE_CALL_LEFT: u64 = 2;
pub const LANE_CALL_TERMINATED: u64 = 3;
/// A call-out that decides a comparison completed, and the comparison held; `LANE_CALL_COMPLETED`
/// is that it did not.
pub const LANE_CALL_HOLDS: u64 = 4;
/// The callee's body ended by an exception (`LANE_BODY_THREW`): its activation is popped, the
/// call's register is untouched and the exception waits in `Vm::lane_raised`.
pub const LANE_CALL_RAISED: u64 = 5;
pub const LANE_CALL_BRANCHED: u64 = 6;

/// The general dispatch's return statuses count down from the top of `u32` by this many blocks
/// (`vm::compiled::MAX_JS_GENERAL_DISPATCH_BLOCKS`).
pub const GENERAL_DISPATCH_BLOCKS: u32 = 2048;
pub const GENERAL_RETURN_STATUS_BASE: u32 = u32::MAX - GENERAL_DISPATCH_BLOCKS;
/// A lane body whose activation completes (`LANE_BODY_RETURNED`); a callee entry cell that held
/// the stub, so nothing ran (`LANE_BODY_NOT_ENTERED`).
pub const LANE_BODY_RETURNED: u32 = GENERAL_RETURN_STATUS_BASE - 1;
pub const LANE_BODY_NOT_ENTERED: u32 = GENERAL_RETURN_STATUS_BASE - 2;
/// A lane body whose activation ends by an exception no handler of its function covers (`LANE_BODY_THREW`).
pub const LANE_BODY_THREW: u32 = GENERAL_RETURN_STATUS_BASE - 3;
/// The status of a body that stops with a result word naming where (`NATIVE_JS_STOPPED`), the
/// result word of a stop inside a callee's activation (`LaneExit::Callee`), and the status of a
/// terminated run (`TERMINATED_STATUS`).
pub const NATIVE_JS_STOPPED: u32 = 1;
pub const CALLEE_EXIT: u64 = 1 << 63;
/// One past the runtime's raised status, which follows its Wasm trap codes (`vm::compiled::TRAPS`, asserted there).
pub const TERMINATED_STATUS: u32 = 11;

/// The per-site record of a direct-call row, one cache line, patched into the row as the one
/// 64-bit value hole 0 (its address). Everything a site knows that does not fit the three value
/// holes: the callee's identity and cell, the cell's validity word the call was planned under,
/// the descriptor the shadow frame names, the callee's capture word, the global object a sloppy
/// callee binds an `undefined` or `null` receiver to, the stride of shadow frames, and the
/// caller registers the call names. The plan owns the record and must outlive the code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct CallRecord {
    pub callee: u64,
    pub cell: u64,
    pub expected: u64,
    pub descriptor: u64,
    pub captures: u64,
    pub global_this: u64,
    pub frame_bytes: u64,
    pub callee_register: u16,
    pub this_register: u16,
    pub args_register: u16,
    pub result_register: u16,
    /// Stage C plain header (`register_file::FRAME_CONTEXT` words 4 and 7): the caller's call
    /// record the callee returns to, and the packed `function | PLAIN << 32 | argc << 48` word,
    /// both fixed per site, so a plain direct call stores them without computing either.
    pub call: u64,
    pub header: u64,
    /// The caller's lane view records (`LaneContext::base` while it runs), header word 6, and the
    /// caller's program (`ProgramId::raw`), which the first plain frame names (`PlainFrames::program`).
    pub caller_base: u64,
    pub program: u64,
}

/// The packed header word's flag bit for a plain activation (bits 32..48 hold the flags) and the
/// shift of its argument count (`argc = word >> HEADER_ARGC_SHIFT`, no mask).
pub const HEADER_PLAIN: u64 = 1 << 32;
pub const HEADER_ARGC_SHIFT: u32 = 48;
/// The flag of a compiled body's plain header, whose word 8 is its `DirectCallee` descriptor
/// (`staged::frame::BODY_FLAG`): the lane spills it as the native frame a direct row would push.
pub const HEADER_BODY: u64 = 1 << 33;

/// The packed header word of a plain activation of `function` taking `argc` arguments.
pub const fn plain_header(function: u32, argc: u16) -> u64 {
    function as u64 | HEADER_PLAIN | (argc as u64) << HEADER_ARGC_SHIFT
}

const CALL_RECORD_BYTES: usize = 128;
const _: () = assert!(core::mem::size_of::<CallRecord>() == CALL_RECORD_BYTES);
