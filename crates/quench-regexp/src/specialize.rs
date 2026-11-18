//! The regexp matcher as a specializer product (Futamura): one semantics, written once over a
//! `Stage`, run by the interpreter today and specialized to a program tomorrow.
//!
//! Design note. `Program::run` interprets one `Inst` at a time over a machine state (a position
//! in the subject, the register file, the backtrack stack). A native search (`native.rs`) is that
//! same interpreter with the program's instructions known: its program counter, every
//! instruction's operands, the register count and the unit tests are binding-time static, and
//! only the subject, the position and the register values are dynamic. The first Futamura
//! projection is the specializer of `run` to a `Program`; this file is the step that makes it
//! derivable instead of rewritten by hand. The meaning of each instruction lives here once, as a
//! function over the machine's operations (`Stage`), not over the machine:
//!
//! * `Concrete` runs them on the interpreter's own state. Every method is `#[inline(always)]`
//!   and the step functions are monomorphized into `Program::run`, so the interpreter pays
//!   nothing for the indirection: the same loads and stores as the code it replaced.
//! * A later `Emit` stage will implement the same operations as code generation (Cranelift IR or
//!   a stencil row, as `quench-runtime/src/staged.rs` does for object layouts): a position
//!   becomes a value, a `bool` outcome a branch, `push` a store through the stack pointer. Each
//!   instruction's code is then derived from the function below, so an interpreter change reaches
//!   the native search, and a regexp that is hot is compiled from what runs it cold.
//!
//! Binding times. Static: the instruction, its operands, `Program` tables (sets, lists, guards,
//! dispatch tables). Dynamic: positions, register contents, stack contents, subject units. The
//! operations below are exactly the dynamic accesses; a static operand is an argument, so an
//! emitting stage sees it as a constant and folds it.
//!
//! Backtracking is here too (`replay`, the same machine read backwards), so the whole matcher
//! runs on the stage. Not yet here, and the next steps in order: the search driver (`next_start`, prefix scan) as the
//! outer loop of the specialized code, and the lookahead body, which `Stage::look` runs as a
//! nested interpreter until an emitting stage inlines it. Matching behaviour is unchanged.

use crate::in_units;
// Lookbehind's backward reads and the classes of strings, over the same stage.
#[path = "backtrack/backward.rs"]
mod backward;
// The interpreter's stage: the operations on the machine's own state.
#[path = "backtrack/concrete.rs"]
mod concrete;
pub(super) use concrete::Concrete;
use super::matcher::{admits, assertion_holds, takes_unit, Admission, MatchStage};
use super::{equal, Frame, Inst, Program, Text, UnitTest, NO_START, UNBOUNDED, UNSET};

/// A machine position: the next instruction and the subject position it reads at.
#[derive(Clone, Copy)]
pub(super) struct Cursor {
    pc: u32,
    position: usize,
}

impl Cursor {
    #[inline(always)]
    fn next(self) -> Self {
        Self { pc: self.pc + 1, ..self }
    }

    #[inline(always)]
    fn jump(self, pc: u32) -> Self {
        Self { pc, ..self }
    }

    /// The next instruction, `position` taken as the new subject position.
    #[inline(always)]
    fn then_at(self, position: usize) -> Self {
        Self { pc: self.pc + 1, position }
    }
}

/// What one instruction does to the machine.
#[derive(Clone, Copy)]
pub(super) enum Step {
    /// Continue at the cursor.
    Continue(Cursor),
    /// The instruction fails: backtrack.
    Fail,
    /// The run ends in a match at the cursor's position.
    Accept,
}

impl Step {
    #[inline(always)]
    fn when(holds: bool, then: Cursor) -> Self {
        if holds { Self::Continue(then) } else { Self::Fail }
    }
}

/// The dynamic accesses the instructions make. Positions are `usize` here; a code-generating
/// stage keeps the same operations over its own values.
pub(super) trait Stage: MatchStage<Pos = usize, Unit = u32, Flag = bool, Test = UnitTest> {
    /// The end of the run of units from `from` to at most `to` that pass `test`.
    fn run_end(&self, test: UnitTest, from: usize, to: usize) -> usize;
    /// The start of the run of units from at least `to` up to `from` that pass `test`.
    fn run_start(&self, test: UnitTest, from: usize, to: usize) -> usize;
    fn register(&self, register: u32) -> usize;
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
    /// The target of dispatch table `table` for the unit at `position`, its other branches
    /// pushed as choices.
    fn dispatch(&mut self, table: u32, position: usize) -> Option<u32>;
    /// The `index`th entry of `Program::lists`.
    fn listed(&self, index: u32) -> u32;
    /// The positions a match of string class `class` at `position` ends at, longest first
    /// (read backward: where the matches ending at `position` start).
    fn class_ends(&mut self, class: u32, position: usize, backward: bool) -> Vec<usize>;
    /// Runs the lookahead body at `body` as a nested run; whether the lookahead holds
    /// (`found != negative`), its stack effects applied.
    fn look(&mut self, body: Cursor, negative: bool) -> bool;
}

/// Writes `register`, keeping its old value on the stack for backtracking.
#[inline(always)]
fn set<S: Stage>(stage: &mut S, register: u32, value: usize) {
    let old = stage.register(register);
    stage.push(Frame::Restore { register, value: old });
    stage.assign(register, value);
}

/// A capture's registers: its start, then its end.
pub const CAPTURE_REGISTERS: u32 = 2;

#[inline(always)]
fn start_register(capture: u32) -> u32 {
    capture * CAPTURE_REGISTERS
}

/// Writes both bounds of `capture`, keeping the old pair on the stack.
#[inline(always)]
fn set_capture<S: Stage>(stage: &mut S, capture: u32, (start, end): (usize, usize)) {
    let bound = start_register(capture);
    let (old_start, old_end) = (stage.register(bound), stage.register(bound + 1));
    stage.push(Frame::RestoreCapture { capture, start: old_start, end: old_end });
    stage.assign(bound, start);
    stage.assign(bound + 1, end);
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
            let at = position - 1;
            if at > floor {
                stage.keep(Frame::Greedy { pc, floor, position: at });
            }
            Replay::Resume(Cursor::at(pc, at))
        }
        Some(frame @ (Frame::GreedyBack { .. } | Frame::LazyBack { .. })) => backward::replay(stage, frame),
        Some(Frame::Lazy { pc, test, ceiling, position }) => {
            let unit = stage.unit_at(position);
            if !stage.passes(unit, test) {
                return Replay::Undone;
            }
            let at = position + 1;
            if at < ceiling {
                stage.keep(Frame::Lazy { pc, test, ceiling, position: at });
            }
            Replay::Resume(Cursor::at(pc, at))
        }
    }
}

/// The meaning of `inst` at `at`: control flow here, the registers' bookkeeping in `registers`.
#[inline(always)]
pub(super) fn step<S: Stage>(stage: &mut S, inst: Inst, at: Cursor) -> Step {
    match inst {
        Inst::Unit(test) => unit(stage, test, at),
        Inst::UnitRepeat { test, min, max, greedy, atomic } => unit_repeat(stage, (test, min, max, greedy, atomic), at),
        Inst::Assert(test) => Step::when(assertion_holds(stage, test, at.position), at.next()),
        Inst::Split { alternative } => {
            stage.push(Frame::Choice { pc: alternative, position: at.position });
            Step::Continue(at.next())
        }
        Inst::GuardedSplit { alternative, guard } => guarded_split(stage, (alternative, guard), at),
        Inst::Dispatch { table } => dispatch(stage, table, at),
        Inst::Jump(target) => Step::Continue(at.jump(target)),
        Inst::Look { negative, end } => Step::when(stage.look(at.next(), negative), at.jump(end)),
        Inst::LookSucceed | Inst::Match => Step::Accept,
        inst => registers(stage, inst, at),
    }
}

/// The instructions that read and write the register file: captures, loop counters, backreferences.
#[inline(always)]
fn registers<S: Stage>(stage: &mut S, inst: Inst, at: Cursor) -> Step {
    match inst {
        Inst::CaptureOpen { temporary, count, restores } => capture_open(stage, (temporary, count, restores), at),
        Inst::CaptureClose { capture, temporary } => {
            let start = stage.register(temporary);
            set_capture(stage, capture, (start, at.position));
            Step::Continue(at.next())
        }
        Inst::ResetCaptures { first, count } => reset_captures(stage, (first, count), at),
        Inst::LoopInit { counter } => {
            set(stage, counter, 0);
            Step::Continue(at.next())
        }
        Inst::LoopHead { counter, min, max, greedy, exit } => loop_head(stage, (counter, min, max, greedy, exit), at),
        Inst::LoopIterate { start } => {
            set(stage, start, at.position);
            Step::Continue(at.next())
        }
        Inst::EmptyCheck { start } => Step::when(at.position != stage.register(start), at.next()),
        Inst::LoopTail { counter, start, min, head } => loop_tail(stage, (counter, start, min, head), at),
        Inst::Backreference { first, count, ignore_case, unicode } => backreference(stage, (first, count, ignore_case, unicode), at),
        inst => backward::step(stage, inst, at),
    }
}

#[inline(always)]
fn dispatch<S: Stage>(stage: &mut S, table: u32, at: Cursor) -> Step {
    match stage.dispatch(table, at.position) {
        Some(target) => Step::Continue(at.jump(target)),
        None => Step::Fail,
    }
}

/// Records where a capture opens, keeping the old value on the stack when backtracking must undo it.
#[inline(always)]
fn capture_open<S: Stage>(stage: &mut S, (temporary, count, restores): (u32, u32, bool), at: Cursor) -> Step {
    for register in temporary..temporary + count {
        if restores { set(stage, register, at.position) } else { stage.assign(register, at.position) }
    }
    Step::Continue(at.next())
}

#[inline(always)]
fn unit<S: Stage>(stage: &mut S, test: UnitTest, at: Cursor) -> Step {
    let takes = takes_unit(stage, test, at.position);
    Step::when(takes, at.then_at(at.position + 1))
}

/// `min..=max` units: greedy takes the most and keeps a frame to give units back (none when
/// `atomic`), lazy takes the fewest and keeps a frame to take more. `UNBOUNDED` exceeds any subject, so the ceiling is the
/// subject's length then.
#[inline(always)]
fn unit_repeat<S: Stage>(stage: &mut S, (test, min, max, greedy, atomic): (UnitTest, u32, u32, bool, bool), at: Cursor) -> Step {
    let ceiling = at.position.saturating_add(max as usize).min(stage.length());
    let floor = at.position + min as usize;
    if floor > stage.length() {
        return Step::Fail;
    }
    let end = stage.run_end(test, at.position, if greedy { ceiling } else { floor });
    if end < floor {
        return Step::Fail;
    }
    let pc = at.pc + 1;
    if greedy && !atomic && end > floor {
        stage.push(Frame::Greedy { pc, floor, position: end });
    } else if !greedy && end < ceiling {
        stage.push(Frame::Lazy { pc, test, ceiling, position: end });
    }
    Step::Continue(at.then_at(end))
}

#[inline(always)]
fn guarded_split<S: Stage>(stage: &mut S, (alternative, guard): (u32, u32), at: Cursor) -> Step {
    let guard = stage.guard(guard);
    let admitted = admits(stage, guard, at.position);
    if admitted {
        stage.push(Frame::Choice { pc: alternative, position: at.position });
    }
    Step::Continue(if admitted { at.next() } else { at.jump(alternative) })
}

#[inline(always)]
fn reset_captures<S: Stage>(stage: &mut S, (first, count): (u32, u32), at: Cursor) -> Step {
    for index in first..first + count {
        let capture = stage.listed(index);
        if stage.register(start_register(capture)) != UNSET {
            set_capture(stage, capture, (UNSET, UNSET));
        }
    }
    Step::Continue(at.next())
}

/// A quantifier's decision before an iteration of its body: enter below the minimum, leave at the
/// maximum, else choose by greediness with the other way kept as a choice.
#[inline(always)]
fn loop_head<S: Stage>(stage: &mut S, (counter, min, max, greedy, exit): (u32, u32, u32, bool, u32), at: Cursor) -> Step {
    let count = stage.register(counter);
    let (enter, leave) = (at.next(), at.jump(exit));
    if count < min as usize {
        return Step::Continue(enter);
    }
    if max != UNBOUNDED && count >= max as usize {
        return Step::Continue(leave);
    }
    let (taken, kept) = if greedy { (enter, leave) } else { (leave, enter) };
    stage.push(Frame::Choice { pc: kept.pc, position: at.position });
    Step::Continue(taken)
}

/// RepeatMatcher's continuation: once the minimum is met, an iteration that consumed nothing
/// fails.
#[inline(always)]
fn loop_tail<S: Stage>(stage: &mut S, (counter, start, min, head): (u32, u32, u32, u32), at: Cursor) -> Step {
    let count = stage.register(counter);
    if start != NO_START && count >= min as usize && at.position == stage.register(start) {
        return Step::Fail;
    }
    set(stage, counter, count + 1);
    Step::Continue(at.jump(head))
}

/// A backreference to the first set capture of the list: it matches the captured units at the
/// position, and trivially where no listed capture is set.
#[inline(always)]
fn backreference<S: Stage>(stage: &mut S, (first, count, ignore_case, unicode): (u32, u32, bool, bool), at: Cursor) -> Step {
    let set_bound = (first..first + count).map(|index| start_register(stage.listed(index))).find(|bound| stage.register(*bound) != UNSET);
    let Some(bound) = set_bound else { return Step::Continue(at.next()) };
    let (from, to) = (stage.register(bound), stage.register(bound + 1));
    let width = to - from;
    let matches = at.position + width <= stage.length()
        && (from..to).zip(at.position..).all(|(expected, actual)| equal(stage.unit_at(expected), stage.unit_at(actual), ignore_case, unicode));
    Step::when(matches, at.then_at(at.position + width))
}

impl Cursor {
    /// The cursor a run starts or resumes at.
    #[inline(always)]
    pub(super) fn at(pc: u32, position: usize) -> Self {
        Self { pc, position }
    }

    #[inline(always)]
    pub(super) fn pc(self) -> u32 {
        self.pc
    }

    #[inline(always)]
    pub(super) fn position(self) -> usize {
        self.position
    }
}
