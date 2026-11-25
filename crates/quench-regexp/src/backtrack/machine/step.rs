//! Which row each instruction is: one dispatch, which the interpreter runs per step and the
//! native compiler runs once per instruction.
use super::{self as m, At, Machine, Next, Step};
use super::super::{Inst, UnitTest};

/// The meaning of `inst` at `at`: the instruction's row. The interpreter runs it on its
/// state, the native compiler specializes it to the instruction.
#[inline(always)]
pub fn step<M: Machine<Test = UnitTest>>(machine: &mut M, inst: Inst, at: At<M>) -> Next<M> {
    match inst {
        Inst::Unit(test) => m::unit(machine, test, at),
        Inst::UnitRepeat { test, min, max, greedy, atomic } => m::unit_repeat(machine, (test, min, max), (greedy, atomic), at),
        Inst::Assert(test) => m::assertion(machine, test, at),
        Inst::Split { alternative } => m::split(machine, alternative, at),
        Inst::GuardedSplit { alternative, guard } => {
            let guard = machine.guard(guard);
            m::guarded_split(machine, alternative, guard, at)
        }
        Inst::Dispatch { table } => m::dispatch(machine, table, at),
        Inst::Jump(target) => Step::Continue(at.jump(target)),
        Inst::Look { negative, end } => machine.lookaround((negative, end), at),
        Inst::LookSucceed => machine.look_succeed(at),
        Inst::Match => Step::Accept,
        inst => registers(machine, inst, at),
    }
}

/// The instructions that read and write the register file: captures, loop counters, backreferences.
#[inline(always)]
fn registers<M: Machine<Test = UnitTest>>(machine: &mut M, inst: Inst, at: At<M>) -> Next<M> {
    match inst {
        Inst::CaptureOpen { temporary, count, restores } => m::capture_open(machine, (temporary, count, restores), at),
        Inst::CaptureClose { capture, temporary } => m::capture_close(machine, (capture, temporary), at),
        Inst::ResetCaptures { first, count } => m::reset_captures(machine, (first, count), at),
        Inst::LoopInit { counter } => m::loop_init(machine, counter, at),
        Inst::LoopHead { counter, min, max, greedy, exit } => m::loop_head(machine, (counter, min, max), (greedy, exit), at),
        Inst::LoopIterate { start } => m::loop_iterate(machine, start, at),
        Inst::EmptyCheck { start } => m::empty_check(machine, start, at),
        Inst::LoopTail { counter, start, min, head } => m::loop_tail(machine, (counter, start, min, head), at),
        Inst::Backreference { first, count, ignore_case, unicode } => m::backreference(machine, (first, count), (ignore_case, unicode, false), at),
        inst => rare(machine, inst, at),
    }
}

/// The instructions a lookbehind body runs, read backward, and the classes of strings: each
/// continues at the next pc or fails.
#[inline(always)]
fn rare<M: Machine<Test = UnitTest>>(machine: &mut M, inst: Inst, at: At<M>) -> Next<M> {
    match rare_position(machine, inst, at) {
        Some(position) => Step::Continue(at.then_at(position)),
        None => Step::Fail,
    }
}

/// `rare`'s position at the next pc, `None` where the instruction fails. Out of line, and
/// returning in registers, so the patterns with none of these instructions, the common ones,
/// keep the interpreter's loop tight. An emitting machine's rows continue on every path.
#[inline(never)]
fn rare_position<M: Machine<Test = UnitTest>>(machine: &mut M, inst: Inst, at: At<M>) -> Option<M::Pos> {
    let step = match inst {
        Inst::UnitBack(test) => m::unit_back(machine, test, at),
        Inst::UnitRepeatBack { test, min, max, greedy } => m::unit_repeat_back(machine, (test, min, max, greedy), at),
        Inst::CaptureCloseBack { capture, temporary } => m::capture_close_back(machine, (capture, temporary), at),
        Inst::BackreferenceBack { first, count, ignore_case, unicode } => m::backreference(machine, (first, count), (ignore_case, unicode, true), at),
        Inst::ClassWidths { class, backward } => m::class_widths(machine, (class, backward), at),
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
