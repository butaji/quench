//! The exit: how control leaves an activation, one meaning for every tier.
//!
//! An exit is an effect handler of a function's signature (L4): the way the activation's control
//! leaves the code that runs it. One meaning, two encodings. A body (a lane body, or a native run
//! of one) returns a `u32` status word (`to_body_status`); a helper returns a `u64` status word
//! (`to_helper`). The two domains share numbers, so each encoding is partial: an exit with no word
//! in a domain is one that domain never returns. Closed-world units have no exits: a unit whose
//! every callee and receiver class is known leaves nothing to a status, so it has no status word.
//!
//! This file depends on nothing but `core`, like `lane_protocol.rs` which mounts it (by path, so the
//! host crate `quench-bjit` and the stencil build compile the same numbers).
//!
//! The table. Every old status and outcome value, in each domain, names one `Exit`:
//!
//! | old value                                             | domain | `Exit`                       |
//! |-------------------------------------------------------|--------|------------------------------|
//! | `LANE_CALL_COMPLETED` = 0, `LaneCall::Completed`, `Outcome::Next`, `HelperOutcome::Completed` | helper | `Completed` |
//! | `LANE_CALL_MISSED` = 1, `LaneCall::Missed`, `Outcome::Missed`, `HelperOutcome::Missed` | helper | `Missed` |
//! | `LANE_BODY_NOT_ENTERED`, `LaneStatus::NotEntered` (`GENERAL_RETURN_STATUS_BASE - 2`), `LaneExit::Stub` | body | `Missed` |
//! | `LANE_CALL_LEFT` = 2, `LaneCall::Left`, `Outcome::Left` (its word is `CALLEE_EXIT`) | helper | `Left { word: CALLEE_EXIT }` |
//! | `NATIVE_JS_STOPPED` = 1, `LaneStatus::NativeJsStopped`, `LaneExit::At` / `Inlined` / `Callee` / `Entry` (their words are `ExitKind` words) | body | `Left { word }` |
//! | `LANE_CALL_TERMINATED` = 3, `LaneCall::Terminated`, `Outcome::Terminated` (also every unnamed helper status) | helper | `Terminated` |
//! | `TERMINATED_STATUS` = 26, `LaneStatus::Terminated`, `LaneExit::Terminated` | body | `Terminated` |
//! | `LANE_CALL_HOLDS` = 4, `LaneCall::Holds` (`Outcome::Terminated` in `helper_outcome`) | helper | `Holds` |
//! | `LANE_CALL_RAISED` = 5, `LaneCall::Raised`, `Outcome::Raised`, `HelperOutcome::Raised` | helper | `Raised` |
//! | `LANE_BODY_THREW`, `LaneStatus::Threw` (`GENERAL_RETURN_STATUS_BASE - 3`), `LaneExit::Threw` | body | `Raised` |
//! | `LANE_CALL_BRANCHED` = 6, `LaneCall::Branched`, `Outcome::Taken` | helper | `Branched` |
//! | `LANE_BODY_RETURNED`, `LaneStatus::Returned` (`GENERAL_RETURN_STATUS_BASE - 1`), `LaneExit::Returned` | body | `Returned { pc: None }`: the record is read from the body |
//! | `return_status(pc)` = `GENERAL_RETURN_STATUS_BASE + pc`, `LaneExit::At(pc)` completing at a record | body | `Returned { pc: Some(pc) }` |
//!
//! A helper's `Left` always leaves with `CALLEE_EXIT`, the word of a leave into a callee's activation.
//! The body's `Left` takes its word from the result slot the body returns with.

use super::{return_status, return_status_pc, CALLEE_EXIT, GENERAL_RETURN_STATUS_BASE};

/// The helper status words (`LANE_CALL_*`, `u64`).
pub const HELPER_COMPLETED: u64 = 0;
pub const HELPER_MISSED: u64 = 1;
pub const HELPER_LEFT: u64 = 2;
pub const HELPER_TERMINATED: u64 = 3;
pub const HELPER_HOLDS: u64 = 4;
pub const HELPER_RAISED: u64 = 5;
pub const HELPER_BRANCHED: u64 = 6;

/// The body status words (`LANE_BODY_*`, `NATIVE_JS_STOPPED`, `TERMINATED_STATUS`, `u32`).
pub const BODY_LEFT: u32 = 1;
pub const BODY_TERMINATED: u32 = 26;
pub const BODY_RETURNED: u32 = GENERAL_RETURN_STATUS_BASE - 1;
pub const BODY_NOT_ENTERED: u32 = GENERAL_RETURN_STATUS_BASE - 2;
pub const BODY_THREW: u32 = GENERAL_RETURN_STATUS_BASE - 3;

/// How control left an activation. See the module doc for the table from every old value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// A helper step completed, its result written to the step's register; the row goes on.
    Completed,
    /// The activation completed with its value. `pc` is the record that completes it where the
    /// body's status names it (`return_status`); `None` where the caller reads it from the record.
    Returned { pc: Option<u32> },
    /// The activation ended by an exception; the exception waits in the VM's lane-raised slot.
    Raised,
    /// Control left into an activation the lane makes. `word` is the leave word: its `ExitKind` in
    /// the top bits and its place below (`LaneExit::word`); a helper's is always `CALLEE_EXIT`.
    Left { word: u64 },
    /// Nothing ran: the call or step has no effect yet, and the caller takes its general path.
    Missed,
    /// The run is terminated.
    Terminated,
    /// A call-out decided a comparison holds.
    Holds,
    /// A completed step that took its conditional jump.
    Branched,
}

impl Exit {
    /// The exit a helper's status word names, or `None` for a word no helper returns.
    pub const fn from_helper(status: u64) -> Option<Self> {
        match status {
            HELPER_COMPLETED => Some(Self::Completed),
            HELPER_MISSED => Some(Self::Missed),
            HELPER_LEFT => Some(Self::Left { word: CALLEE_EXIT }),
            HELPER_TERMINATED => Some(Self::Terminated),
            HELPER_HOLDS => Some(Self::Holds),
            HELPER_RAISED => Some(Self::Raised),
            HELPER_BRANCHED => Some(Self::Branched),
            _ => None,
        }
    }

    /// The helper status word of this exit, or `None` for an exit no helper returns (a body's
    /// `Returned`: a helper that completes a call completes with `Completed`).
    pub const fn to_helper(self) -> Option<u64> {
        match self {
            Self::Completed => Some(HELPER_COMPLETED),
            Self::Missed => Some(HELPER_MISSED),
            Self::Left { .. } => Some(HELPER_LEFT),
            Self::Terminated => Some(HELPER_TERMINATED),
            Self::Holds => Some(HELPER_HOLDS),
            Self::Raised => Some(HELPER_RAISED),
            Self::Branched => Some(HELPER_BRANCHED),
            Self::Returned { .. } => None,
        }
    }

    /// The exit a body's status word names, with `word` the result slot the body returned with (it
    /// is the leave word where the status is `BODY_LEFT`, and unread otherwise). `None` where the
    /// status is no body status: a value below the named statuses that is not a return status.
    pub const fn from_body_status(status: u32, word: u64) -> Option<Self> {
        match status {
            BODY_LEFT => Some(Self::Left { word }),
            BODY_TERMINATED => Some(Self::Terminated),
            BODY_RETURNED => Some(Self::Returned { pc: None }),
            BODY_NOT_ENTERED => Some(Self::Missed),
            BODY_THREW => Some(Self::Raised),
            _ => match return_status_pc(status) {
                Some(pc) => Some(Self::Returned { pc: Some(pc) }),
                None => None,
            },
        }
    }

    /// The body status word of this exit, or `None` for an exit no body returns (`Completed`,
    /// `Holds`, `Branched`: the helper's step outcomes). A body's leave carries its word in the
    /// result slot, which the caller writes.
    pub const fn to_body_status(self) -> Option<u32> {
        match self {
            Self::Left { .. } => Some(BODY_LEFT),
            Self::Terminated => Some(BODY_TERMINATED),
            Self::Returned { pc: None } => Some(BODY_RETURNED),
            Self::Returned { pc: Some(pc) } => Some(return_status(pc)),
            Self::Missed => Some(BODY_NOT_ENTERED),
            Self::Raised => Some(BODY_THREW),
            Self::Completed | Self::Holds | Self::Branched => None,
        }
    }
}
