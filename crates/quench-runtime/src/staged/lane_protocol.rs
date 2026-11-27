//! The lane call protocol's status numbers: one definition, shared by the b-jit stencils
//! (`stencils/call_rows.rs`, through `crate::layout::call_record`) and the runtime's status
//! handling (`quench_bjit::call_record`, which mounts this file by path).
//!
//! This file depends on nothing but `core`, so the host crate (`quench-bjit`) and the stencil build
//! compile the same statuses. The Stage-generic steps the stencils run over them live in
//! `protocol.rs`.
//!
//! The meaning of each status is `exit::Exit` (`exit.rs`, mounted here by path so the host and the
//! stencils compile it too); this file holds the numbers it is encoded with and the views the call
//! rows read. Two domains share numbers: a helper's return and a body's or native run's status both
//! use `1` (`LANE_CALL_MISSED`, `NATIVE_JS_STOPPED`), which is why `Exit` has two encodings.

#[path = "exit.rs"]
pub mod exit;
use exit::Exit;

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

/// A body's or a native run's status word (the `u32` a lane body or entry returns with), the body
/// encoding of `Exit`: each named status is `exit::BODY_*`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneStatus {
    /// The body of a stop with a result word naming where (`NATIVE_JS_STOPPED`); also the status
    /// of a callee's leave, whose word is `CALLEE_EXIT`-tagged.
    NativeJsStopped = exit::BODY_LEFT,
    /// A terminated run (`TERMINATED_STATUS`), one past the runtime's raised status, which follows
    /// its Wasm trap codes (`vm::compiled::TRAPS`, asserted there).
    Terminated = exit::BODY_TERMINATED,
    /// A lane body whose activation completes (`LANE_BODY_RETURNED`).
    Returned = exit::BODY_RETURNED,
    /// A callee entry cell that held the stub, so nothing ran (`LANE_BODY_NOT_ENTERED`).
    NotEntered = exit::BODY_NOT_ENTERED,
    /// A lane body whose activation ends by an exception no handler of its function covers
    /// (`LANE_BODY_THREW`).
    Threw = exit::BODY_THREW,
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

pub const LANE_CALL_COMPLETED: u64 = exit::HELPER_COMPLETED;
pub const LANE_CALL_MISSED: u64 = exit::HELPER_MISSED;
pub const LANE_CALL_LEFT: u64 = exit::HELPER_LEFT;
pub const LANE_CALL_TERMINATED: u64 = exit::HELPER_TERMINATED;
/// `LANE_CALL_HOLDS` is a call-out's decision that a comparison holds (a completion too).
pub const LANE_CALL_HOLDS: u64 = exit::HELPER_HOLDS;
pub const LANE_CALL_RAISED: u64 = exit::HELPER_RAISED;
pub const LANE_CALL_BRANCHED: u64 = exit::HELPER_BRANCHED;

pub const LANE_BODY_RETURNED: u32 = exit::BODY_RETURNED;
pub const LANE_BODY_NOT_ENTERED: u32 = exit::BODY_NOT_ENTERED;
pub const LANE_BODY_THREW: u32 = exit::BODY_THREW;
pub const NATIVE_JS_STOPPED: u32 = exit::BODY_LEFT;
pub const TERMINATED_STATUS: u32 = exit::BODY_TERMINATED;

/// Where a helper call's status sends the call row (`call_helper`'s successors): a view of `Exit`.
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

impl Outcome {
    /// The successor of an exit. Exits the rows neither continue nor leave by (`Holds`, a body's
    /// `Returned`) terminate the run, as the helper row always did.
    pub const fn of(exit: Exit) -> Self {
        match exit {
            Exit::Completed => Self::Next,
            Exit::Branched => Self::Taken,
            Exit::Missed => Self::Missed,
            Exit::Left { .. } => Self::Left,
            Exit::Raised => Self::Raised,
            Exit::Terminated | Exit::Holds | Exit::Returned { .. } => Self::Terminated,
        }
    }
}

/// The successor a helper's `status` selects. Any status no helper returns terminates the run.
pub fn helper_outcome(status: u64) -> Outcome {
    match Exit::from_helper(status) {
        Some(exit) => Outcome::of(exit),
        None => Outcome::Terminated,
    }
}
