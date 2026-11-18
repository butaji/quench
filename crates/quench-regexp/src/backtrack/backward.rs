//! The VM's backward reads, which a lookbehind body runs (the spec's `direction` of -1), and
//! the classes of strings, read either way: each instruction's meaning once over the stage,
//! as in the parent module.
use super::{set_capture, start_register, Cursor, Replay, Stage, Step};
use crate::backtrack::{equal, Frame, Inst, UnitTest, UNSET};

/// Backtracks a backward repeat's entry, as `replay` does a forward one's.
#[inline(never)]
pub(super) fn replay<S: Stage>(stage: &mut S, frame: Frame) -> Replay {
    match frame {
        Frame::GreedyBack { pc, ceiling, position } => {
            let at = position + 1;
            if at < ceiling {
                stage.keep(Frame::GreedyBack { pc, ceiling, position: at });
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
                stage.keep(Frame::LazyBack { pc, test, floor, position: at });
            }
            Replay::Resume(Cursor::at(pc, at))
        }
        _ => unreachable!("`replay` hands over only backward repeats"),
    }
}

/// The meaning of the instructions `step` hands over: `advance`'s position at the next pc.
#[inline(always)]
pub(super) fn step<S: Stage>(stage: &mut S, inst: Inst, at: Cursor) -> Step {
    match advance(stage, inst, at) {
        Some(position) => Step::Continue(at.then_at(position)),
        None => Step::Fail,
    }
}

/// Where the instruction `step` hands over leaves the position, its run going on at the next
/// pc; `None` where it fails. Out of line, and every instruction here continues at the next
/// pc, so forward patterns, the common ones, keep the interpreter's loop as tight as before
/// these instructions existed.
#[inline(never)]
pub(super) fn advance<S: Stage>(stage: &mut S, inst: Inst, Cursor { pc, position }: Cursor) -> Option<usize> {
    let next = pc + 1;
    match inst {
        Inst::UnitBack(test) => unit_back(stage, test, position),
        Inst::UnitRepeatBack { test, min, max, greedy } => unit_repeat_back(stage, (test, min, max, greedy), position, next),
        Inst::ClassWidths { class, backward } => class_widths(stage, (class, backward), position, next),
        Inst::CaptureCloseBack { capture, temporary } => {
            // The open recorded the capture's end; the close is at its start.
            let end = stage.register(temporary);
            set_capture(stage, capture, (position, end));
            Some(position)
        }
        Inst::BackreferenceBack { first, count, ignore_case, unicode } => backreference_back(stage, (first, count, ignore_case, unicode), position),
        _ => unreachable!("`step` hands over only these"),
    }
}

/// `unit` read backward: the unit before the position, which moves onto it.
#[inline(always)]
fn unit_back<S: Stage>(stage: &mut S, test: UnitTest, position: usize) -> Option<usize> {
    let before = position.checked_sub(1)?;
    let unit = stage.unit_at(before);
    stage.passes(unit, test).then_some(before)
}

/// `unit_repeat` read backward: the run of units ends at the position; greedy gives units
/// back toward the position, lazy takes more away from it.
#[inline(always)]
fn unit_repeat_back<S: Stage>(stage: &mut S, (test, min, max, greedy): (UnitTest, u32, u32, bool), position: usize, pc: u32) -> Option<usize> {
    let floor = position.checked_sub(min as usize)?;
    let deepest = position.saturating_sub(max as usize);
    let end = stage.run_start(test, position, if greedy { deepest } else { floor });
    if end > floor {
        return None;
    }
    if greedy && end < floor {
        stage.push(Frame::GreedyBack { pc, ceiling: floor, position: end });
    } else if !greedy && end > deepest {
        stage.push(Frame::LazyBack { pc, test, floor: deepest, position: end });
    }
    Some(end)
}

/// A class of strings: its longest match, the shorter ones kept as choices in order.
#[inline(always)]
fn class_widths<S: Stage>(stage: &mut S, (class, backward): (u32, bool), position: usize, pc: u32) -> Option<usize> {
    let ends = stage.class_ends(class, position, backward);
    let (&first, rest) = ends.split_first()?;
    for &end in rest.iter().rev() {
        stage.push(Frame::Choice { pc, position: end });
    }
    Some(first)
}

/// `backreference` read backward: the captured units end at the position, which moves to
/// their start.
#[inline(always)]
fn backreference_back<S: Stage>(stage: &mut S, (first, count, ignore_case, unicode): (u32, u32, bool, bool), position: usize) -> Option<usize> {
    let set_bound = (first..first + count).map(|index| start_register(stage.listed(index))).find(|bound| stage.register(*bound) != UNSET);
    let Some(bound) = set_bound else { return Some(position) };
    let (from, to) = (stage.register(bound), stage.register(bound + 1));
    let begin = position.checked_sub(to - from)?;
    let matches = (from..to).zip(begin..).all(|(expected, actual)| equal(stage.unit_at(expected), stage.unit_at(actual), ignore_case, unicode));
    matches.then_some(begin)
}
