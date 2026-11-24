//! The VM's instructions, each meaning written once over a `Machine`: the interpreter runs
//! them on its own state (`specialize::Concrete`), and a native compiler implements the same
//! operations as code generation, so its code for an instruction is the function below
//! specialized to the instruction's static operands (the first Futamura projection), not a
//! second copy of the meaning.
//!
//! Control flow is part of the machine: `require` fails the run where a flag does not hold,
//! `when` makes an effect conditional, `choose` continues one of two ways. The interpreter
//! reads the flag; an emitting machine branches on it and goes on along both paths.
use super::matcher::{admits, assertion_holds, takes_unit, Admission, AssertionTest, MatchStage};

// The instructions a lookbehind body runs, read backward, and backreferences either way.
#[path = "machine/backward.rs"]
mod backward;
// Each instruction's row.
#[path = "machine/step.rs"]
mod step;
pub use step::step;
pub use backward::{backreference, class_widths, capture_close_back, give_back_back, take_more_back, unit_back, unit_repeat_back};

/// A static count that exceeds any subject (`Inst::UnitRepeat`, `Inst::LoopHead`).
pub const UNBOUNDED: u32 = u32::MAX;
/// A loop whose body cannot match empty keeps no iteration start (`Inst::LoopTail`).
pub const NO_START: u32 = u32::MAX;
/// A capture's registers: its start, then its end.
pub const CAPTURE_REGISTERS: u32 = 2;

/// A machine position: the next instruction, static, and the subject position it reads at.
#[derive(Clone, Copy)]
pub struct Cursor<P = usize> {
    pub pc: u32,
    pub position: P,
}

impl<P: Copy> Cursor<P> {
    /// The cursor a run starts or resumes at.
    #[inline(always)]
    pub fn at(pc: u32, position: P) -> Self {
        Self { pc, position }
    }

    #[inline(always)]
    pub fn next(self) -> Self {
        Self { pc: self.pc + 1, ..self }
    }

    #[inline(always)]
    pub fn jump(self, pc: u32) -> Self {
        Self { pc, ..self }
    }

    /// The next instruction, `position` taken as the new subject position.
    #[inline(always)]
    pub fn then_at(self, position: P) -> Self {
        Self { pc: self.pc + 1, position }
    }
}

/// What one instruction does to the machine.
#[derive(Clone, Copy)]
pub enum Step<P = usize> {
    /// Continue at the cursor.
    Continue(Cursor<P>),
    /// The instruction fails: backtrack.
    Fail,
    /// The run ends in a match at the current position.
    Accept,
    /// An emitting machine branched (`Machine::choose`) and finished both paths itself.
    Branched,
}

/// The dynamic accesses and the control flow of the instructions over a machine's own
/// positions and flags.
pub trait Machine: MatchStage {
    /// A static count as a position.
    fn constant(&mut self, value: usize) -> Self::Pos;
    /// The value of a register no capture has set.
    fn unset(&mut self) -> Self::Pos;
    fn load(&mut self, register: u32) -> Self::Pos;
    /// Writes a register with no restore entry.
    fn store(&mut self, register: u32, value: Self::Pos);
    /// The end of the run of units from `from` to at most `to` that pass `test`.
    fn taken_end(&mut self, test: Self::Test, from: Self::Pos, to: Self::Pos) -> Self::Pos;
    /// `position` moved on by `count` units, at most to the end; `UNBOUNDED` reaches the end.
    fn ceiling(&mut self, position: Self::Pos, count: u32) -> Self::Pos;
    /// Backtracking writes `old` back to `register`.
    fn push_restore(&mut self, register: u32, old: Self::Pos);
    /// Backtracking writes both old bounds back to `capture`.
    fn push_capture_restore(&mut self, capture: u32, start: Self::Pos, end: Self::Pos);
    /// Backtracking continues at `pc` from `position`.
    fn push_choice(&mut self, pc: u32, position: Self::Pos);
    /// A greedy repeat ending at `position`, before `pc`, may give back units down to `floor`.
    fn push_greedy(&mut self, pc: u32, floor: Self::Pos, position: Self::Pos);
    /// A lazy repeat ending at `position`, before `pc`, may take units of `test` up to `ceiling`.
    fn push_lazy(&mut self, pc: u32, test: Self::Test, ceiling: Self::Pos, position: Self::Pos);
    /// The last unit in `floor..end` that passes `test`; `None` where none does, the step then
    /// failing (an emitting machine branches to its backtracking).
    fn last_passing(&mut self, test: Self::Test, floor: Self::Pos, end: Self::Pos) -> Option<Self::Pos>;
    /// Backtracking a greedy repeat that can give back more keeps its entry, ending at `position`.
    fn keep_greedy(&mut self, pc: u32, floor: Self::Pos, position: Self::Pos);
    /// Backtracking a lazy repeat that can take more keeps its entry, ending at `position`.
    fn keep_lazy(&mut self, pc: u32, test: Self::Test, ceiling: Self::Pos, position: Self::Pos);
    /// The `index`th entry of the program's capture lists (`Program::lists`), static.
    fn listed(&mut self, index: u32) -> u32;
    /// The test of the instruction at `pc` where it takes one unit (`Inst::Unit`), static:
    /// the follower a greedy repeat gives units back to.
    fn follower(&mut self, pc: u32) -> Option<Self::Test>;
    /// The positions the body behind `GuardedSplit` guard `guard` can start at (static).
    fn guard(&mut self, guard: u32) -> Admission<Self::Test>;
    /// `Inst::Look`: the lookaround whose body follows, matching going on at `end` where it
    /// holds. The interpreter runs the body as a nested run; native code runs it inline,
    /// above a mark its `LookSucceed` or a backtrack to the mark decides at.
    fn lookaround(&mut self, look: (bool, u32), at: Cursor<Self::Pos>) -> Step<Self::Pos>;
    /// `Inst::LookSucceed`: the lookaround's body matched.
    fn look_succeed(&mut self, at: Cursor<Self::Pos>) -> Step<Self::Pos>;
    /// The step of `arm` for the interval of dispatch table `table` that holds `unit`, with
    /// the interval's branches `Program::lists[first..first + count]`; `Step::Fail` where no
    /// interval holds it. The table is static: an emitting machine branches on `unit` to an
    /// arm per interval.
    fn dispatch_on(&mut self, unit: Self::Unit, table: u32, arm: impl FnMut(&mut Self, (u32, u32)) -> Step<Self::Pos>) -> Step<Self::Pos>;
    /// Where the matches of string class `class` at `position` end (read `backward`, where
    /// those ending there start): whether there is one, and the longest, the shorter ones
    /// kept as choices at `pc` in order.
    fn class_ends(&mut self, class: u32, position: Self::Pos, backward: bool, pc: u32) -> (Self::Flag, Self::Pos);
    /// `then` where `flag` holds, else `otherwise`.
    fn pick(&mut self, flag: Self::Flag, then: Self::Pos, otherwise: Self::Pos) -> Self::Pos;
    /// `left + right`, which stays inside the subject.
    fn plus(&mut self, left: Self::Pos, right: Self::Pos) -> Self::Pos;
    /// `left - right`, where `right` is at most `left`.
    fn minus(&mut self, left: Self::Pos, right: Self::Pos) -> Self::Pos;
    /// `position` moved back by `count` units, at least to the start; `UNBOUNDED` reaches it.
    fn reach_back(&mut self, position: Self::Pos, count: u32) -> Self::Pos;
    /// The start of the run of units from `from` back to at least `to` that pass `test`.
    fn run_start(&mut self, test: Self::Test, from: Self::Pos, to: Self::Pos) -> Self::Pos;
    /// Whether the units of `span` (`from..to`) equal those from `at` on, compared as `flags`
    /// (ignore case, unicode); the subject holds them all.
    fn same_units(&mut self, span: (Self::Pos, Self::Pos), at: Self::Pos, flags: (bool, bool)) -> Self::Flag;
    /// A backward greedy repeat that started at `position`, before `pc`, may give back units
    /// up to `ceiling`.
    fn push_greedy_back(&mut self, pc: u32, ceiling: Self::Pos, position: Self::Pos);
    /// A backward lazy repeat that started at `position`, before `pc`, may take units of
    /// `test` down to `floor`.
    fn push_lazy_back(&mut self, pc: u32, test: Self::Test, floor: Self::Pos, position: Self::Pos);
    /// `keep_greedy` of a backward repeat.
    fn keep_greedy_back(&mut self, pc: u32, ceiling: Self::Pos, position: Self::Pos);
    /// `keep_lazy` of a backward repeat.
    fn keep_lazy_back(&mut self, pc: u32, test: Self::Test, floor: Self::Pos, position: Self::Pos);
    /// Whether the run goes on: `false` where `flag` does not hold, the step then failing. An
    /// emitting machine branches to its backtracking and goes on where it holds.
    fn require(&mut self, flag: Self::Flag) -> bool;
    /// Runs `effect` where `flag` holds.
    fn when(&mut self, flag: Self::Flag, effect: impl FnOnce(&mut Self));
    /// The step of `then` where `flag` holds, else of `otherwise`.
    fn choose(&mut self, flag: Self::Flag, then: impl FnOnce(&mut Self) -> Step<Self::Pos>, otherwise: impl FnOnce(&mut Self) -> Step<Self::Pos>) -> Step<Self::Pos>;
}

type At<M> = Cursor<<M as MatchStage>::Pos>;
type Next<M> = Step<<M as MatchStage>::Pos>;

#[inline(always)]
pub fn start_register(capture: u32) -> u32 {
    capture * CAPTURE_REGISTERS
}

/// Writes `register`, keeping its old value on the stack for backtracking.
#[inline(always)]
pub fn set<M: Machine>(machine: &mut M, register: u32, value: M::Pos) {
    let old = machine.load(register);
    machine.push_restore(register, old);
    machine.store(register, value);
}

/// Writes both bounds of `capture`, keeping the old pair on the stack.
#[inline(always)]
pub fn set_capture<M: Machine>(machine: &mut M, capture: u32, (start, end): (M::Pos, M::Pos)) {
    let bound = start_register(capture);
    let (old_start, old_end) = (machine.load(bound), machine.load(bound + 1));
    machine.push_capture_restore(capture, old_start, old_end);
    machine.store(bound, start);
    machine.store(bound + 1, end);
}

/// `Inst::Unit`: one unit that passes `test`.
#[inline(always)]
pub fn unit<M: Machine>(machine: &mut M, test: M::Test, at: At<M>) -> Next<M> {
    let takes = takes_unit(machine, test, at.position);
    if !machine.require(takes) {
        return Step::Fail;
    }
    let after = machine.offset(at.position, 1);
    Step::Continue(at.then_at(after))
}

#[inline(always)]
pub fn assertion<M: Machine>(machine: &mut M, test: AssertionTest, at: At<M>) -> Next<M> {
    let holds = assertion_holds(machine, test, at.position);
    if !machine.require(holds) {
        return Step::Fail;
    }
    Step::Continue(at.next())
}

/// `Inst::Dispatch`: an alternation whose branches begin with known units, one lookup of the
/// unit at the position. The first branch that can begin with it runs, the others kept as
/// choices in order.
#[inline(always)]
pub fn dispatch<M: Machine>(machine: &mut M, table: u32, at: At<M>) -> Next<M> {
    let takes = {
        let length = machine.length();
        machine.before(at.position, length)
    };
    if !machine.require(takes) {
        return Step::Fail;
    }
    let unit = machine.unit_at(at.position);
    machine.dispatch_on(unit, table, #[inline(always)] |machine, (first, count)| {
        for index in (first + 1..first + count).rev() {
            let pc = machine.listed(index);
            machine.push_choice(pc, at.position);
        }
        let target = machine.listed(first);
        Step::Continue(at.jump(target))
    })
}

#[inline(always)]
pub fn split<M: Machine>(machine: &mut M, alternative: u32, at: At<M>) -> Next<M> {
    machine.push_choice(alternative, at.position);
    Step::Continue(at.next())
}

/// `Inst::GuardedSplit`: a split whose next body can start only where `guard` admits; the
/// run goes straight to `alternative`, with no choice, elsewhere.
#[inline(always)]
pub fn guarded_split<M: Machine>(machine: &mut M, alternative: u32, guard: Admission<M::Test>, at: At<M>) -> Next<M> {
    let admitted = admits(machine, guard, at.position);
    machine.choose(admitted, |machine| split(machine, alternative, at), |_| Step::Continue(at.jump(alternative)))
}

/// `min..=max` units: greedy takes the most and keeps a frame to give units back (none when
/// `atomic`), lazy takes the fewest and keeps a frame to take more.
#[inline(always)]
pub fn unit_repeat<M: Machine>(machine: &mut M, (test, min, max): (M::Test, u32, u32), (greedy, atomic): (bool, bool), at: At<M>) -> Next<M> {
    let ceiling = machine.ceiling(at.position, max);
    let floor = machine.offset(at.position, min as isize);
    let length = machine.length();
    let beyond = machine.before(length, floor);
    let fits = machine.not(beyond);
    if !machine.require(fits) {
        return Step::Fail;
    }
    let end = machine.taken_end(test, at.position, if greedy { ceiling } else { floor });
    let short = machine.before(end, floor);
    let enough = machine.not(short);
    if !machine.require(enough) {
        return Step::Fail;
    }
    let pc = at.pc + 1;
    if greedy && !atomic {
        let more = machine.before(floor, end);
        machine.when(more, |machine| machine.push_greedy(pc, floor, end));
    } else if !greedy {
        let more = machine.before(end, ceiling);
        machine.when(more, |machine| machine.push_lazy(pc, test, ceiling, end));
    }
    Step::Continue(at.then_at(end))
}

/// Records where `count` groups open, keeping the old values on the stack when backtracking
/// must undo them.
#[inline(always)]
pub fn capture_open<M: Machine>(machine: &mut M, (temporary, count, restores): (u32, u32, bool), at: At<M>) -> Next<M> {
    for register in temporary..temporary + count {
        if restores { set(machine, register, at.position) } else { machine.store(register, at.position) }
    }
    Step::Continue(at.next())
}

#[inline(always)]
pub fn capture_close<M: Machine>(machine: &mut M, (capture, temporary): (u32, u32), at: At<M>) -> Next<M> {
    let start = machine.load(temporary);
    set_capture(machine, capture, (start, at.position));
    Step::Continue(at.next())
}

/// Unsets the captures listed in `Program::lists[first..first + count]`.
#[inline(always)]
pub fn reset_captures<M: Machine>(machine: &mut M, (first, count): (u32, u32), at: At<M>) -> Next<M> {
    for index in first..first + count {
        let capture = machine.listed(index);
        reset_capture(machine, capture);
    }
    Step::Continue(at.next())
}

/// Unsets `capture` where it is set (`Inst::ResetCaptures`, one listed capture).
#[inline(always)]
fn reset_capture<M: Machine>(machine: &mut M, capture: u32) {
    let (current, unset) = (machine.load(start_register(capture)), machine.unset());
    let same = machine.equal(current, unset);
    let is_set = machine.not(same);
    machine.when(is_set, |machine| set_capture(machine, capture, (unset, unset)));
}

#[inline(always)]
pub fn loop_init<M: Machine>(machine: &mut M, counter: u32, at: At<M>) -> Next<M> {
    let zero = machine.constant(0);
    set(machine, counter, zero);
    Step::Continue(at.next())
}

/// A quantifier's decision before an iteration of its body: enter below the minimum, leave at
/// the maximum, else choose by greediness with the other way kept as a choice.
#[inline(always)]
pub fn loop_head<M: Machine>(machine: &mut M, (counter, min, max): (u32, u32, u32), (greedy, exit): (bool, u32), at: At<M>) -> Next<M> {
    let count = machine.load(counter);
    let (enter, leave) = (at.next(), at.jump(exit));
    let minimum = machine.constant(min as usize);
    let below = machine.before(count, minimum);
    let decide = move |machine: &mut M| {
        let (taken, kept) = if greedy { (enter, leave) } else { (leave, enter) };
        machine.push_choice(kept.pc, at.position);
        Step::Continue(taken)
    };
    machine.choose(below, |_| Step::Continue(enter), |machine| {
        if max == UNBOUNDED {
            return decide(machine);
        }
        let maximum = machine.constant(max as usize);
        let room = machine.before(count, maximum);
        machine.choose(room, decide, |_| Step::Continue(leave))
    })
}

#[inline(always)]
pub fn loop_iterate<M: Machine>(machine: &mut M, start: u32, at: At<M>) -> Next<M> {
    set(machine, start, at.position);
    Step::Continue(at.next())
}

/// The end of an optional atom's one iteration: it fails having consumed nothing.
#[inline(always)]
pub fn empty_check<M: Machine>(machine: &mut M, start: u32, at: At<M>) -> Next<M> {
    let began = machine.load(start);
    let same = machine.equal(at.position, began);
    let moved = machine.not(same);
    if !machine.require(moved) {
        return Step::Fail;
    }
    Step::Continue(at.next())
}

/// RepeatMatcher's continuation: once the minimum is met, an iteration that consumed nothing
/// fails.
#[inline(always)]
pub fn loop_tail<M: Machine>(machine: &mut M, (counter, start, min, head): (u32, u32, u32, u32), at: At<M>) -> Next<M> {
    let count = machine.load(counter);
    // A body that cannot match empty keeps no start and needs no empty check.
    if start != NO_START {
        let minimum = machine.constant(min as usize);
        let short = machine.before(count, minimum);
        let enough = machine.not(short);
        let empty = machine.guarded(enough, |machine| {
            let began = machine.load(start);
            machine.equal(at.position, began)
        });
        let accepted = machine.not(empty);
        if !machine.require(accepted) {
            return Step::Fail;
        }
    }
    let next = machine.offset(count, 1);
    set(machine, counter, next);
    Step::Continue(at.jump(head))
}

/// A backtrack entry that offers a way on, as backtracking pops it: each machine keeps its
/// entries its own way (the interpreter as `Frame`s, native code as words), and both resume
/// them here.
#[derive(Clone, Copy)]
pub enum Resumed<P, T> {
    /// A choice: the run continues at `pc` from `position`.
    Choice { pc: u32, position: P },
    /// A greedy repeat before `pc` that ended at `position` and may give units back to `floor`.
    Greedy { pc: u32, floor: P, position: P },
    /// A lazy repeat before `pc` that ended at `position` and may take units of `test` up to
    /// `ceiling`.
    Lazy { pc: u32, test: T, ceiling: P, position: P },
    /// `Greedy` of a backward repeat, which gives units back up to `ceiling`.
    GreedyBack { pc: u32, ceiling: P, position: P },
    /// `Lazy` of a backward repeat, which takes units down to `floor`.
    LazyBack { pc: u32, test: T, floor: P, position: P },
}

/// Backtracking to `entry`: where the run goes on, or `Step::Fail` where the repeat it names
/// cannot give or take another unit. A repeat that still can stays on the stack.
#[inline(always)]
pub fn resume<M: Machine>(machine: &mut M, entry: Resumed<M::Pos, M::Test>) -> Next<M> {
    match entry {
        Resumed::Choice { pc, position } => Step::Continue(Cursor::at(pc, position)),
        Resumed::Greedy { pc, floor, position } => {
            let follower = machine.follower(pc);
            give_back(machine, pc, follower, (floor, position))
        }
        Resumed::Lazy { pc, test, ceiling, position } => take_more(machine, pc, test, (ceiling, position)),
        Resumed::GreedyBack { pc, ceiling, position } => give_back_back(machine, pc, (ceiling, position)),
        Resumed::LazyBack { pc, test, floor, position } => take_more_back(machine, pc, test, (floor, position)),
    }
}

/// Backtracking a greedy repeat before `pc` that ended at `position`: it gives back a unit and
/// stays while it can give back more. Where the next instruction takes one unit by `follower`,
/// the units it refuses fail at once, so the repeat gives back down to the last unit it takes,
/// which it takes, failing where none down to `floor` does.
#[inline(always)]
pub fn give_back<M: Machine>(machine: &mut M, pc: u32, follower: Option<M::Test>, (floor, position): (M::Pos, M::Pos)) -> Next<M> {
    let Some(test) = follower else {
        let at = machine.offset(position, -1);
        let more = machine.before(floor, at);
        machine.when(more, |machine| machine.keep_greedy(pc, floor, at));
        return Step::Continue(Cursor::at(pc, at));
    };
    let Some(at) = machine.last_passing(test, floor, position) else { return Step::Fail };
    let more = machine.before(floor, at);
    machine.when(more, |machine| machine.keep_greedy(pc, floor, at));
    // The follower takes the unit at `at`: on past it.
    let after = machine.offset(at, 1);
    Step::Continue(Cursor::at(pc + 1, after))
}

/// Backtracking a lazy repeat before `pc` that ended at `position`: it takes one more unit
/// where `test` passes, and stays below `ceiling`.
#[inline(always)]
pub fn take_more<M: Machine>(machine: &mut M, pc: u32, test: M::Test, (ceiling, position): (M::Pos, M::Pos)) -> Next<M> {
    let unit = machine.unit_at(position);
    let passes = machine.passes(unit, test);
    if !machine.require(passes) {
        return Step::Fail;
    }
    let at = machine.offset(position, 1);
    let more = machine.before(at, ceiling);
    machine.when(more, |machine| machine.keep_lazy(pc, test, ceiling, at));
    Step::Continue(Cursor::at(pc, at))
}
