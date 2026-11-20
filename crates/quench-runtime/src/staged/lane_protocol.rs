//! The lane call protocol's status numbers: one definition, shared by the b-jit stencils
//! (`stencils/call_rows.rs`, through `crate::layout::call_record`) and the runtime's status
//! handling (`quench_bjit::call_record`, which mounts this file by path).
//!
//! This file depends on nothing but `core`, so the host crate (`quench-bjit`) and the stencil build
//! compile the same statuses. The Stage-generic steps the stencils run over them live in
//! `protocol.rs`.
//!
//! Two enums, because the two status domains share numbers: a helper's return (`LaneCall`) and a
//! body's or native run's status (`LaneStatus`) both use `1` (`LANE_CALL_MISSED`,
//! `NATIVE_JS_STOPPED`).

/// The general dispatch's return statuses count down from the top of `u32` by this many blocks
/// (`vm::compiled::MAX_JS_GENERAL_DISPATCH_BLOCKS`).
pub const GENERAL_DISPATCH_BLOCKS: u32 = 2048;
pub const GENERAL_RETURN_STATUS_BASE: u32 = u32::MAX - GENERAL_DISPATCH_BLOCKS;

/// The status of a native activation's completion at record `pc`: `GENERAL_RETURN_STATUS_BASE + pc`,
/// with the result word in the entry's result slot. The AOT trampoline (`vm::compiled::ahead_entry`)
/// emits this encoding as IR from `GENERAL_RETURN_STATUS_BASE` and `RETURN_PC_MASK`; this is its named
/// form, which the trampoline's test checks its status against.
pub const fn return_status(pc: u32) -> u32 {
    GENERAL_RETURN_STATUS_BASE + pc
}

/// The record a return status names, or `None` where `status` is not one (a named status, or no
/// status at all). The one decoder of `return_status`; `LaneExit::read` and the linked entry use it.
pub const fn return_status_pc(status: u32) -> Option<u32> {
    if status >= GENERAL_RETURN_STATUS_BASE { Some(status - GENERAL_RETURN_STATUS_BASE) } else { None }
}

/// The bits of a record's word its completion's PC is read from: the trampoline keeps their low 32
/// bits, which its IR narrows into the status word after the add of `GENERAL_RETURN_STATUS_BASE`.
pub const RETURN_PC_MASK: u32 = u32::MAX;

/// What a helper call returned (`vm::compiled::LANE_CALL_*`): completed with its result in the
/// call's register, missed before any effect, left the lane inside the callee's activation,
/// terminated the run, a call-out that decides a comparison held, the callee's body ended by an
/// exception (its activation popped, the exception waiting in `Vm::lane_raised`), or a completed
/// step that branched (a conditional jump, taken).
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneCall {
    Completed = 0,
    Missed = 1,
    Left = 2,
    Terminated = 3,
    /// `LANE_CALL_COMPLETED` is the comparison's not-holding completion.
    Holds = 4,
    Raised = 5,
    Branched = 6,
}

/// A body's or a native run's status word (the `u32` a lane body or entry returns with).
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneStatus {
    /// The body of a stop with a result word naming where (`NATIVE_JS_STOPPED`); also the status
    /// of a callee's leave, whose word is `CALLEE_EXIT`-tagged.
    NativeJsStopped = 1,
    /// A terminated run (`TERMINATED_STATUS`), one past the runtime's raised status, which follows
    /// its Wasm trap codes (`vm::compiled::TRAPS`, asserted there).
    Terminated = 11,
    /// A lane body whose activation completes (`LANE_BODY_RETURNED`).
    Returned = GENERAL_RETURN_STATUS_BASE - 1,
    /// A callee entry cell that held the stub, so nothing ran (`LANE_BODY_NOT_ENTERED`).
    NotEntered = GENERAL_RETURN_STATUS_BASE - 2,
    /// A lane body whose activation ends by an exception no handler of its function covers
    /// (`LANE_BODY_THREW`).
    Threw = GENERAL_RETURN_STATUS_BASE - 3,
}

impl LaneStatus {
    /// Every named status.
    pub const ALL: [LaneStatus; 5] = [Self::NativeJsStopped, Self::Terminated, Self::Returned, Self::NotEntered, Self::Threw];

    /// The named status `status` is, or `None` where it is a return status naming a PC
    /// (`>= GENERAL_RETURN_STATUS_BASE`) or no status at all.
    pub fn of(status: u32) -> Option<Self> {
        Self::ALL.iter().copied().find(|named| *named as u32 == status)
    }
}

/// The result word of a stop inside a callee's activation (`LaneExit::Callee`), with `NATIVE_JS_STOPPED`.
pub const CALLEE_EXIT: u64 = 1 << 63;

pub const LANE_CALL_COMPLETED: u64 = LaneCall::Completed as u64;
pub const LANE_CALL_MISSED: u64 = LaneCall::Missed as u64;
pub const LANE_CALL_LEFT: u64 = LaneCall::Left as u64;
pub const LANE_CALL_TERMINATED: u64 = LaneCall::Terminated as u64;
pub const LANE_CALL_HOLDS: u64 = LaneCall::Holds as u64;
pub const LANE_CALL_RAISED: u64 = LaneCall::Raised as u64;
pub const LANE_CALL_BRANCHED: u64 = LaneCall::Branched as u64;

pub const LANE_BODY_RETURNED: u32 = LaneStatus::Returned as u32;
pub const LANE_BODY_NOT_ENTERED: u32 = LaneStatus::NotEntered as u32;
pub const LANE_BODY_THREW: u32 = LaneStatus::Threw as u32;
pub const NATIVE_JS_STOPPED: u32 = LaneStatus::NativeJsStopped as u32;
pub const TERMINATED_STATUS: u32 = LaneStatus::Terminated as u32;

/// Where a helper call's status sends the call row (`call_helper`'s successors).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Continues at the next row.
    Next,
    /// Continues at the reloads of the registers live at the jump's target.
    Taken,
    /// Nothing ran: leaves at the call's pc.
    Missed,
    /// Leaves with `NATIVE_JS_STOPPED` and `CALLEE_EXIT`.
    Left,
    /// Records the call's pc and leaves as a raise.
    Raised,
    /// Leaves with `TERMINATED_STATUS`.
    Terminated,
}

/// The successor a helper's `status` selects. Any status but the four the rows continue or leave by
/// terminates the run, as the helper row always did.
pub fn helper_outcome(status: u64) -> Outcome {
    match status {
        LANE_CALL_COMPLETED => Outcome::Next,
        LANE_CALL_BRANCHED => Outcome::Taken,
        LANE_CALL_MISSED => Outcome::Missed,
        LANE_CALL_LEFT => Outcome::Left,
        LANE_CALL_RAISED => Outcome::Raised,
        _ => Outcome::Terminated,
    }
}
