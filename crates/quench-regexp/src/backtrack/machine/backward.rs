//! The instructions a lookbehind body runs (the spec's `direction` of -1), each the forward
//! instruction's meaning read backward over the same `Machine`, and backreferences, read
//! either way. The interpreter and a native compiler run these functions, as the parent's.
use super::{set_capture, start_register, At, Cursor, Machine, Next, Step};

/// `unit` read backward: one unit before the position that passes `test`, which the position
/// moves onto.
#[inline(always)]
pub fn unit_back<M: Machine>(machine: &mut M, test: M::Test, at: At<M>) -> Next<M> {
    let start = machine.at_start(at.position);
    let inside = machine.not(start);
    if !machine.require(inside) {
        return Step::Fail;
    }
    let before = machine.offset(at.position, -1);
    let unit = machine.unit_at(before);
    let passes = machine.passes(unit, test);
    if !machine.require(passes) {
        return Step::Fail;
    }
    Step::Continue(at.then_at(before))
}

/// `capture_close` read backward: the open recorded the capture's end, the close is at its
/// start.
#[inline(always)]
pub fn capture_close_back<M: Machine>(machine: &mut M, (capture, temporary): (u32, u32), at: At<M>) -> Next<M> {
    let end = machine.load(temporary);
    set_capture(machine, capture, (at.position, end));
    Step::Continue(at.next())
}

/// `unit_repeat` read backward: the run of units ends at the position; greedy takes the most
/// and keeps a frame to give units back toward the position, lazy takes the fewest and keeps
/// a frame to take more away from it.
#[inline(always)]
pub fn unit_repeat_back<M: Machine>(machine: &mut M, (test, min, max, greedy): (M::Test, u32, u32, bool), at: At<M>) -> Next<M> {
    let least = machine.constant(min as usize);
    let short = machine.before(at.position, least);
    let fits = machine.not(short);
    if !machine.require(fits) {
        return Step::Fail;
    }
    let floor = machine.offset(at.position, -(min as isize));
    let deepest = machine.reach_back(at.position, max);
    let end = machine.run_start(test, at.position, if greedy { deepest } else { floor });
    let over = machine.before(floor, end);
    let enough = machine.not(over);
    if !machine.require(enough) {
        return Step::Fail;
    }
    let pc = at.pc + 1;
    if greedy {
        let more = machine.before(end, floor);
        machine.when(more, #[inline(always)] |machine| machine.push_greedy_back(pc, floor, end));
    } else {
        let more = machine.before(deepest, end);
        machine.when(more, #[inline(always)] |machine| machine.push_lazy_back(pc, test, deepest, end));
    }
    Step::Continue(at.then_at(end))
}

/// `give_back` read backward: the repeat before `pc` that started at `position` gives back a
/// unit, toward `ceiling`, and stays while it can give back more.
#[inline(always)]
pub fn give_back_back<M: Machine>(machine: &mut M, pc: u32, (ceiling, position): (M::Pos, M::Pos)) -> Next<M> {
    let at = machine.offset(position, 1);
    let more = machine.before(at, ceiling);
    machine.when(more, #[inline(always)] |machine| machine.keep_greedy_back(pc, ceiling, at));
    Step::Continue(Cursor::at(pc, at))
}

/// `take_more` read backward: the repeat before `pc` that started at `position` takes the unit
/// before it where `test` passes, and stays above `floor`.
#[inline(always)]
pub fn take_more_back<M: Machine>(machine: &mut M, pc: u32, test: M::Test, (floor, position): (M::Pos, M::Pos)) -> Next<M> {
    let at = machine.offset(position, -1);
    let unit = machine.unit_at(at);
    let passes = machine.passes(unit, test);
    if !machine.require(passes) {
        return Step::Fail;
    }
    let more = machine.before(floor, at);
    machine.when(more, #[inline(always)] |machine| machine.keep_lazy_back(pc, test, floor, at));
    Step::Continue(Cursor::at(pc, at))
}

/// A backreference to the first set capture of the list `Program::lists[first..first + count]`:
/// it matches the captured units at the position (read `backward`, the units ending there),
/// and trivially where no listed capture is set. An unset capture's bounds are both `unset`,
/// so it spans no units: the match needs no branch for it.
#[inline(always)]
pub fn backreference<M: Machine>(machine: &mut M, (first, count): (u32, u32), flags: (bool, bool, bool), at: At<M>) -> Next<M> {
    if count == 0 {
        return Step::Continue(at.next());
    }
    let last = first + count - 1;
    let bound = start_register(machine.listed(last));
    let (mut from, mut to) = (machine.load(bound), machine.load(bound + 1));
    // The first set capture of the list wins: read from the last, each set one taking over.
    for index in (first..last).rev() {
        let bound = start_register(machine.listed(index));
        let (start, end, unset) = (machine.load(bound), machine.load(bound + 1), machine.unset());
        let unset_here = machine.equal(start, unset);
        from = machine.pick(unset_here, from, start);
        to = machine.pick(unset_here, to, end);
    }
    captured_units(machine, (from, to), flags, at)
}

/// `Inst::ClassWidths`: a class of strings, read either way. Its longest match runs, the
/// shorter ones kept as choices in order.
#[inline(always)]
pub fn class_widths<M: Machine>(machine: &mut M, (class, backward): (u32, bool), at: At<M>) -> Next<M> {
    let (found, end) = machine.class_ends(class, at.position, backward, at.pc + 1);
    if !machine.require(found) {
        return Step::Fail;
    }
    Step::Continue(at.then_at(end))
}

/// The captured units `from..to` at the position (read `backward`, ending there).
#[inline(always)]
fn captured_units<M: Machine>(machine: &mut M, (from, to): (M::Pos, M::Pos), (ignore_case, unicode, backward): (bool, bool, bool), at: At<M>) -> Next<M> {
    let width = machine.minus(to, from);
    let (begin, after) = if backward {
        let short = machine.before(at.position, width);
        let fits = machine.not(short);
        if !machine.require(fits) {
            return Step::Fail;
        }
        let begin = machine.minus(at.position, width);
        (begin, begin)
    } else {
        let end = machine.plus(at.position, width);
        let length = machine.length();
        let beyond = machine.before(length, end);
        let fits = machine.not(beyond);
        if !machine.require(fits) {
            return Step::Fail;
        }
        (at.position, end)
    };
    let same = machine.same_units((from, to), begin, (ignore_case, unicode));
    if !machine.require(same) {
        return Step::Fail;
    }
    Step::Continue(at.then_at(after))
}
