//! The VM's backward reads, which a lookbehind body runs (the spec's `direction` of -1), and
//! the classes of strings, read either way: each instruction's meaning once over the stage,
//! as in the parent module.
use super::{set_capture, start_register, Cursor, Replay, Stage, Step};
use crate::backtrack::{equal, Frame, Inst, UnitTest, UNSET};

/// The meaning of the instructions `step` leaves to this module.
#[inline(always)]
pub(super) fn step<S: Stage>(stage: &mut S, inst: Inst, at: Cursor) -> Step {
    match inst {
        Inst::UnitBack(test) => unit_back(stage, test, at),
        Inst::UnitRepeatBack { test, min, max, greedy } => unit_repeat_back(stage, (test, min, max, greedy), at),
        Inst::CaptureCloseBack { capture, temporary } => {
            let end = stage.register(temporary);
            set_capture(stage, capture, (at.position, end));
            Step::Continue(at.next())
        }
        Inst::BackreferenceBack { first, count, ignore_case, unicode } => backreference_back(stage, (first, count, ignore_case, unicode), at),
        Inst::ClassWidths { class, backward } => class_widths(stage, (class, backward), at),
        _ => unreachable!("`step` and `registers` handle the others"),
    }
}

/// Backtracks a backward repeat's entry, as `replay` does a forward one's.
#[inline(always)]
pub(super) fn replay<S: Stage>(stage: &mut S, frame: Frame) -> Replay {
    match frame {
        Frame::GreedyBack { pc, ceiling, position } => {
            let at = position + 1;
            if at < ceiling {
                stage.push(Frame::GreedyBack { pc, ceiling, position: at });
            }
            Replay::Resume(Cursor::at(pc, at))
        }
        Frame::LazyBack { pc, test, floor, position } => {
            let unit = stage.unit_at(position - 1);
            if !stage.passes(unit, test) {
                return Replay::Undone;
            }
            let at = position - 1;
            if at > floor {
                stage.push(Frame::LazyBack { pc, test, floor, position: at });
            }
            Replay::Resume(Cursor::at(pc, at))
        }
        _ => unreachable!("`replay` hands over only backward repeats"),
    }
}

/// `unit` read backward: the unit before the position, which moves onto it.
#[inline(always)]
fn unit_back<S: Stage>(stage: &mut S, test: UnitTest, at: Cursor) -> Step {
    let takes = at.position > 0 && {
        let unit = stage.unit_at(at.position - 1);
        stage.passes(unit, test)
    };
    Step::when(takes, at.then_at(at.position.wrapping_sub(1)))
}

/// `unit_repeat` read backward: the run of units ends at the position; greedy gives units
/// back toward the position, lazy takes more away from it.
#[inline(always)]
fn unit_repeat_back<S: Stage>(stage: &mut S, (test, min, max, greedy): (UnitTest, u32, u32, bool), at: Cursor) -> Step {
    let Some(floor) = at.position.checked_sub(min as usize) else {
        return Step::Fail;
    };
    let deepest = at.position.saturating_sub(max as usize);
    let end = stage.run_start(test, at.position, if greedy { deepest } else { floor });
    if end > floor {
        return Step::Fail;
    }
    let pc = at.pc + 1;
    if greedy && end < floor {
        stage.push(Frame::GreedyBack { pc, ceiling: floor, position: end });
    } else if !greedy && end > deepest {
        stage.push(Frame::LazyBack { pc, test, floor: deepest, position: end });
    }
    Step::Continue(at.then_at(end))
}

/// A class of strings: its longest match, the shorter ones kept as choices in order.
#[inline(never)]
fn class_widths<S: Stage>(stage: &mut S, (class, backward): (u32, bool), at: Cursor) -> Step {
    let ends = stage.class_ends(class, at.position, backward);
    let Some((&first, rest)) = ends.split_first() else { return Step::Fail };
    for &end in rest.iter().rev() {
        stage.push(Frame::Choice { pc: at.pc + 1, position: end });
    }
    Step::Continue(at.then_at(first))
}

/// `backreference` read backward: the captured units end at the position, which moves to
/// their start.
#[inline(always)]
fn backreference_back<S: Stage>(stage: &mut S, (first, count, ignore_case, unicode): (u32, u32, bool, bool), at: Cursor) -> Step {
    let set_bound = (first..first + count).map(|index| start_register(stage.listed(index))).find(|bound| stage.register(*bound) != UNSET);
    let Some(bound) = set_bound else { return Step::Continue(at.next()) };
    let (from, to) = (stage.register(bound), stage.register(bound + 1));
    let width = to - from;
    let Some(begin) = at.position.checked_sub(width) else { return Step::Fail };
    let matches = (from..to).zip(begin..).all(|(expected, actual)| equal(stage.unit_at(expected), stage.unit_at(actual), ignore_case, unicode));
    Step::when(matches, at.then_at(begin))
}
