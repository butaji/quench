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
//! (backtracking as `replay`) and the nested lookahead run. Their native forms are next
//! (TOD-2078).

use crate::in_units;
// The interpreter's stage: the operations on the machine's own state.
#[path = "backtrack/concrete.rs"]
mod concrete;
pub(super) use concrete::Concrete;
use super::machine::{self as m, Machine};
pub(super) use super::machine::{start_register, Cursor, Step};
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
    /// The positions the body behind guard `guard` can start at.
    fn guard(&self, guard: u32) -> Admission<UnitTest>;
    /// The test of the instruction at `pc` where it takes one unit (`Inst::Unit`).
    fn unit_follower(&self, pc: u32) -> Option<UnitTest>;
    /// Runs the lookahead body at `body` as a nested run; whether the lookahead holds
    /// (`found != negative`), its stack effects applied.
    fn look(&mut self, body: Cursor, negative: bool) -> bool;
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

/// The meaning of `inst` at `at`: each instruction's function over the machine (`machine.rs`),
/// shared with the native compiler, or one this stage alone runs.
#[inline(always)]
pub(super) fn step<S: Stage>(stage: &mut S, inst: Inst, at: Cursor) -> Step {
    match inst {
        Inst::Unit(test) => m::unit(stage, test, at),
        Inst::UnitRepeat { test, min, max, greedy, atomic } => m::unit_repeat(stage, (test, min, max), (greedy, atomic), at),
        Inst::Assert(test) => m::assertion(stage, test, at),
        Inst::Split { alternative } => m::split(stage, alternative, at),
        Inst::GuardedSplit { alternative, guard } => {
            let guard = stage.guard(guard);
            m::guarded_split(stage, alternative, guard, at)
        }
        Inst::Dispatch { table } => m::dispatch(stage, table, at),
        Inst::Jump(target) => Step::Continue(at.jump(target)),
        Inst::Look { negative, end } => look(stage, (negative, end), at),
        Inst::LookSucceed | Inst::Match => Step::Accept,
        inst => registers(stage, inst, at),
    }
}

/// The instructions that read and write the register file: captures, loop counters, backreferences.
#[inline(always)]
fn registers<S: Stage>(stage: &mut S, inst: Inst, at: Cursor) -> Step {
    match inst {
        Inst::CaptureOpen { temporary, count, restores } => m::capture_open(stage, (temporary, count, restores), at),
        Inst::CaptureClose { capture, temporary } => m::capture_close(stage, (capture, temporary), at),
        Inst::ResetCaptures { first, count } => m::reset_captures(stage, (first, count), at),
        Inst::LoopInit { counter } => m::loop_init(stage, counter, at),
        Inst::LoopHead { counter, min, max, greedy, exit } => m::loop_head(stage, (counter, min, max), (greedy, exit), at),
        Inst::LoopIterate { start } => m::loop_iterate(stage, start, at),
        Inst::EmptyCheck { start } => m::empty_check(stage, start, at),
        Inst::LoopTail { counter, start, min, head } => m::loop_tail(stage, (counter, start, min, head), at),
        Inst::Backreference { first, count, ignore_case, unicode } => m::backreference(stage, (first, count), (ignore_case, unicode, false), at),
        inst => rare(stage, inst, at),
    }
}

/// A lookahead: its body runs as a nested run, and matching goes on at `end` where it holds.
#[inline(always)]
fn look<S: Stage>(stage: &mut S, (negative, end): (bool, u32), at: Cursor) -> Step {
    let holds = stage.look(at.next(), negative);
    if holds { Step::Continue(at.jump(end)) } else { Step::Fail }
}

/// The instructions a lookbehind body runs, read backward, and the classes of strings: each
/// continues at the next pc or fails.
#[inline(always)]
fn rare<S: Stage>(stage: &mut S, inst: Inst, at: Cursor) -> Step {
    match rare_position(stage, inst, at) {
        Some(position) => Step::Continue(at.then_at(position)),
        None => Step::Fail,
    }
}

/// `rare`'s position at the next pc, `None` where the instruction fails. Out of line, and
/// returning in registers, so the patterns with none of these instructions, the common ones,
/// keep the interpreter's loop tight.
#[inline(never)]
fn rare_position<S: Stage>(stage: &mut S, inst: Inst, at: Cursor) -> Option<usize> {
    let step = match inst {
        Inst::UnitBack(test) => m::unit_back(stage, test, at),
        Inst::UnitRepeatBack { test, min, max, greedy } => m::unit_repeat_back(stage, (test, min, max, greedy), at),
        Inst::CaptureCloseBack { capture, temporary } => m::capture_close_back(stage, (capture, temporary), at),
        Inst::BackreferenceBack { first, count, ignore_case, unicode } => m::backreference(stage, (first, count), (ignore_case, unicode, true), at),
        Inst::ClassWidths { class, backward } => m::class_widths(stage, (class, backward), at),
        _ => unreachable!("`registers` hands over only these"),
    };
    match step {
        Step::Continue(next) => {
            debug_assert_eq!(next.pc, at.pc + 1);
            Some(next.position)
        }
        _ => None,
    }
}
