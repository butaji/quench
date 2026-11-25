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
//! * The native compiler (`quench-runtime-engine` `regexp_native`, through the shared stitcher) implements the same operations as
//!   Cranelift IR: a position becomes a value, a required flag a branch to the backtrack block,
//!   a push a store through the stack pointer. Each instruction's code is the function in
//!   `machine.rs` run with its static operands, so an interpreter change reaches native code.
//!
//! Binding times. Static: the instruction, its operands, `Program` tables (sets, lists, guards,
//! dispatch tables). Dynamic: positions, register contents, stack contents, subject units. The
//! operations are exactly the dynamic accesses; a static operand is an argument, so an emitting
//! machine sees it as a constant and folds it.
//!
//! Each instruction's row is `machine::step`, and each backtrack entry that offers a way on is
//! resumed by `machine::resume`. Here stays only the interpreter's own frame stack: `replay`
//! pops a `Frame`, undoes a restore, and hands the rest to `resume`.

use crate::in_units;
// The interpreter's stage: the operations on the machine's own state.
#[path = "backtrack/concrete.rs"]
mod concrete;
pub(super) use concrete::Concrete;
use super::machine::{self as m, Machine, Resumed};
pub(super) use super::machine::{start_register, step, Cursor, Step};
use super::matcher::{Admission, MatchStage};
use super::{equal, Frame, Inst, Program, Text, UnitTest, UNSET};

/// Puts back the registers a restore entry saved; any other entry restores nothing.
#[inline(always)]
fn undo<T: Text + ?Sized>(stage: &mut Concrete<'_, T>, frame: Frame) {
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

/// Backtracks one entry of the interpreter's stack: a restore undone here, an entry that
/// offers a way on resumed by the row both machines share (`machine::resume`).
#[inline(always)]
pub(super) fn replay<T: Text + ?Sized>(stage: &mut Concrete<'_, T>) -> Replay {
    match stage.pop() {
        None | Some(Frame::Barrier) => Replay::Exhausted,
        Some(frame @ (Frame::Restore { .. } | Frame::RestoreCapture { .. })) => {
            undo(stage, frame);
            Replay::Undone
        }
        Some(Frame::Choice { pc, position }) => resumed(m::resume(stage, Resumed::Choice { pc, position })),
        Some(Frame::Greedy { pc, floor, position }) => resumed(m::resume(stage, Resumed::Greedy { pc, floor, position })),
        Some(Frame::Lazy { pc, test, ceiling, position }) => resumed(m::resume(stage, Resumed::Lazy { pc, test, ceiling, position })),
        Some(frame @ (Frame::GreedyBack { .. } | Frame::LazyBack { .. })) => replay_back(stage, frame),
    }
}

/// Backtracks a backward repeat's entry, as `replay` does a forward one's. Out of line: a
/// lookbehind's repeats are rare, and the interpreter's loop stays tight without them.
#[inline(never)]
fn replay_back<T: Text + ?Sized>(stage: &mut Concrete<'_, T>, frame: Frame) -> Replay {
    let entry = match frame {
        Frame::GreedyBack { pc, ceiling, position } => Resumed::GreedyBack { pc, ceiling, position },
        Frame::LazyBack { pc, test, floor, position } => Resumed::LazyBack { pc, test, floor, position },
        _ => unreachable!("`replay` hands over only backward repeats"),
    };
    resumed(m::resume(stage, entry))
}
