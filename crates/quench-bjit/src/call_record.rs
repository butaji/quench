//! The call protocol's shared facts: the status words a call returns, the per-site record the
//! direct-call rows read, and the largest argument count a direct row exists for. One home:
//! `stencils/call_rows.rs` (compiled by rustc, no_std) includes this file by path, and the
//! runtime's planner (`quench-runtime/src/vm/compiled/base_region/call_plan.rs`) reads it as
//! `quench_bjit::call_record`; the runtime's own constants for these words are checked equal
//! there (`call_protocol_matches_the_runtime`). The status numbers are defined in
//! `quench-runtime/src/staged/lane_protocol.rs`, mounted here by path, and re-exported.

#[path = "../../quench-runtime/src/staged/lane_protocol.rs"]
mod lane_protocol;
pub use lane_protocol::{
    helper_outcome, LaneCall, LaneStatus, Outcome, CALLEE_EXIT, GENERAL_DISPATCH_BLOCKS, GENERAL_RETURN_STATUS_BASE,
    LANE_BODY_NOT_ENTERED, LANE_BODY_RETURNED, LANE_BODY_THREW, LANE_CALL_BRANCHED, LANE_CALL_COMPLETED, LANE_CALL_HOLDS,
    LANE_CALL_LEFT, LANE_CALL_MISSED, LANE_CALL_RAISED, LANE_CALL_TERMINATED, NATIVE_JS_STOPPED, TERMINATED_STATUS,
};

/// The most arguments a direct-call row passes (`call_direct_a0` .. `call_direct_a3`); a call
/// with more takes the helper row.
pub const MAX_DIRECT_ARGC: usize = 3;

/// The leading arguments a direct-call row may take from a pinned register rather than the caller's
/// window slot (`CallRecord::argument_places`); the rest are always read from the window.
pub const PLACED_ARGUMENTS: usize = 2;

/// Bytes of one register slot of the frame window (`size_of::<Value>()`, a word).
pub const REGISTER_BYTES: usize = core::mem::size_of::<u64>();

/// The per-site record of a direct-call row, two cache lines, patched into the row as the one
/// 64-bit value hole 0 (its address). Everything a site knows that does not fit the three value
/// holes: the callee's identity and cell, the cell's validity word the call was planned under,
/// the descriptor the shadow frame names, the callee's capture word, the global object a sloppy
/// callee binds an `undefined` or `null` receiver to, the stride of shadow frames, the caller
/// registers the call names, and where each leading argument's word is (`argument_places`: a place
/// code, `MEM` for the caller's window slot, else the pinned register holding the value). The plan
/// owns the record and must outlive the code.
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
    pub argument_places: [u8; PLACED_ARGUMENTS],
}

/// Two cache lines: the record is aligned to one.
const _: () = assert!(core::mem::size_of::<CallRecord>() == 2 * core::mem::align_of::<CallRecord>());
