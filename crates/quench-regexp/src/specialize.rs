//! The regexp matcher as a specializer product (Futamura): one semantics, written once, run by
//! the interpreter and specialized to a program by the native compiler.
//!
//! Design note. `Program::run` interprets one `Inst` at a time over a machine state (a position
//! in the subject, the register file, the backtrack stack). A native search (`native.rs`) is that
//! same interpreter with the program's instructions known: its program counter, every
//! instruction's operands, the register count and the unit tests are binding-time static, and
//! only the subject, the position and the register values are dynamic. The first Futamura
//! projection is the specializer of `run` to a `Program`. The meaning of each instruction lives
//! once in `machine.rs`, as a function over a `Machine`'s operations, not over the machine:
//!
//! * `Concrete` runs them on the interpreter's own state. Every method is `#[inline(always)]`
//!   and the step functions are monomorphized into `Program::run`, so the interpreter pays
//!   nothing for the indirection.
//! * The native compiler (`quench-runtime` `vm/regexp_native`) implements the same operations as
//!   Cranelift IR: a position becomes a value, a required flag a branch to the backtrack block,
//!   a push a store through the stack pointer. Each instruction's code is the function in
//!   `machine.rs` run with its static operands, so an interpreter change reaches native code.
//!
//! Binding times. Static: the instruction, its operands, `Program` tables (sets, lists, guards,
//! dispatch tables). Dynamic: positions, register contents, stack contents, subject units. The
//! operations are exactly the dynamic accesses; a static operand is an argument, so an emitting
//! machine sees it as a constant and folds it.
//!
//! Here stays what only the interpreter runs: `Stage`, the operations on frames themselves
//! (backtracking as `replay`). Each instruction's row is `machine::step`.

use crate::in_units;
// The interpreter's stage: the operations on the machine's own state.
#[path = "backtrack/concrete.rs"]
mod concrete;
pub(super) use concrete::Concrete;
use super::machine::{self as m, Machine};
pub(super) use super::machine::{start_register, step, Cursor, Step};
use super::matcher::{Admission, MatchStage};
use super::{equal, Frame, Inst, Program, Text, UnitTest, UNSET};

/// The dynamic accesses the instructions make. Positions are `usize` here; a code-generating
/// stage keeps the same operations over its own values.
///
/// Not folded into the engine's `staged::Stage`: `quench-runtime` depends on this crate, so the
/// engine's trait cannot be named here. The native driver (`vm/regexp_native`) already emits
/// through the engine's `staged::machine` traits, and implements this trait over them. A fold
/// needs the matcher moved into `quench-runtime` or `staged::machine` moved below both crates.
pub(super) trait Stage: Machine<Pos = usize, Unit = u32, Flag = bool, Test = UnitTest> + quench_stack::projection::Sealed {
    /// Writes a register with no restore entry.
    fn assign(&mut self, register: u32, value: usize);
    fn push(&mut self, frame: Frame);
    /// Puts back the entry `pop` just took, changed: a repeat that can still give or take
    /// a unit stays on the stack one position on, in the slot it was popped from.
    fn keep(&mut self, frame: Frame);
    /// The latest backtrack entry, if the stack holds one.
    fn pop(&mut self) -> Option<Frame>;
    /// The test of the instruction at `pc` where it takes one unit (`Inst::Unit`).
    fn unit_follower(&self, pc: u32) -> Option<UnitTest>;
}

/// Puts back the registers a restore entry saved; any other entry restores nothing.
#[inline(always)]
fn undo<S: Stage>(stage: &mut S, frame: Frame) {
    match frame {
        Frame::Restore { register, value } => stage.assign(register, value),
        Frame::RestoreCapture { capture, start, end } => {
            stage.assign(start_register(capture), start);
            stage.assign(start_register(capture) + 1, end);
        }
        _ => {}
    }
}

/// What backtracking one entry of the stack comes to.
#[derive(Clone, Copy)]
pub(super) enum Replay {
    /// The entry offers a choice: the run continues at the cursor.
    Resume(Cursor),
    /// The entry was a restore, or a lazy repeat that cannot take another unit: pop on.
    Undone,
    /// The run's bottom: no choice is left (a barrier, or an empty stack).
    Exhausted,
}

/// A frame's step as backtracking's outcome: a failed resume pops on.
#[inline(always)]
fn resumed(step: Step) -> Replay {
    match step {
        Step::Continue(at) => Replay::Resume(at),
        _ => Replay::Undone,
    }
}

/// Backtracks one entry: the same machine read backwards, with each entry's meaning written
/// once over the stage. A greedy or lazy repeat that may still give or take a unit stays on the
/// stack, one position on.
#[inline(always)]
pub(super) fn replay<S: Stage>(stage: &mut S) -> Replay {
    match stage.pop() {
        None | Some(Frame::Barrier) => Replay::Exhausted,
        Some(frame @ (Frame::Restore { .. } | Frame::RestoreCapture { .. })) => {
            undo(stage, frame);
            Replay::Undone
        }
        Some(Frame::Choice { pc, position }) => Replay::Resume(Cursor::at(pc, position)),
        Some(Frame::Greedy { pc, floor, position }) => {
            let follower = stage.unit_follower(pc);
            resumed(m::give_back(stage, pc, follower, (floor, position)))
        }
        Some(frame @ (Frame::GreedyBack { .. } | Frame::LazyBack { .. })) => replay_back(stage, frame),
        Some(Frame::Lazy { pc, test, ceiling, position }) => resumed(m::take_more(stage, pc, test, (ceiling, position))),
    }
}

/// Backtracks a backward repeat's entry, as `replay` does a forward one's. Out of line: a
/// lookbehind's repeats are rare, and the interpreter's loop stays tight without them.
#[inline(never)]
fn replay_back<S: Stage>(stage: &mut S, frame: Frame) -> Replay {
    match frame {
        Frame::GreedyBack { pc, ceiling, position } => resumed(m::give_back_back(stage, pc, (ceiling, position))),
        Frame::LazyBack { pc, test, floor, position } => resumed(m::take_more_back(stage, pc, test, (floor, position))),
        _ => unreachable!("`replay` hands over only backward repeats"),
    }
}
