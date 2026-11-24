//! The regexp rows: each instruction shape's stencil is `quench_regexp`'s own row
//! (`backtrack::machine`, mounted here from its files) run on `Stencil`, the b-jit's
//! projection of the regexp `Machine`: positions and flags are machine words, the static
//! operands are holes, and a step's way on is the row's successor. The interpreter
//! (`Concrete`) and the u-jit (`staged::regexp`'s `Emit`) run the same rows, so the three
//! tiers cannot disagree on what an instruction means.
//!
//! The rows keep the u-jit's search state: the subject's bytes and length in the third and
//! fourth pins, the position in the first and the backtracking stack's top in the second, the
//! registers as the frame, and the entry's words (`CONTEXT_*`) behind the context. A frame is
//! the u-jit's (`FRAME_WORDS`: an id, two operands); a resume id indexes the program's table of
//! rows (`RegexpProgram::resumes`), where the u-jit's code dispatches on it.
#![allow(dead_code)]

use crate::regexp_layout::*;
use crate::words::{S, hole64};

#[path = "../../quench-regexp/src/backtrack/insts.rs"]
pub(crate) mod insts;
#[path = "../../quench-regexp/src/backtrack/matcher.rs"]
pub(crate) mod matcher;
#[path = "../../quench-regexp/src/backtrack/machine.rs"]
pub(crate) mod machine;
pub(crate) use insts::{Assertion, Inst, UnitTest};
/// The units of an ASCII subject (the matcher's `super::ASCII_UNITS`).
pub(crate) const ASCII_UNITS: u32 = 128;
use machine::{self as m, Cursor, Machine, Resumed, Step};
use matcher::{admits, Admission, AssertionTest, MatchStage};

/// A row's way on: the next pc's row, the taken pc's row, the backtrack row, or a row by its
/// address (a pc or resume the stitcher does not patch in, or a leave routine).
pub(crate) enum Way {
    Next(S),
    Taken(S),
    Fail(S),
    Jump(S, usize),
}

const WORD: usize = core::mem::size_of::<usize>();
const FRAME_BYTES: usize = FRAME_WORDS * WORD;
/// `native_class_ends`' results (`quench_regexp::backtrack::native`; the engine asserts them equal).
pub const CLASS_NONE: usize = usize::MAX;
pub const CLASS_OVERFLOW: usize = usize::MAX - 1;
pub const CLASS_COUNT_SHIFT: u32 = u32::BITS;
/// The bit an ASCII letter and its other case differ by (`native::CASE_BIT`).
pub const CASE_BIT: usize = 0x20;
/// `Machine`'s unbounded count (`machine::UNBOUNDED`).
const UNBOUNDED: u32 = machine::UNBOUNDED;

/// The class-ends leaf's signature (`native_class_ends`).
type ClassEnds = unsafe extern "C" fn(usize, usize, *const u8, usize, usize, *mut usize, *mut usize, usize) -> usize;

/// The regexp `Machine` of a row: the search state, the program record, and what a row sets
/// before running its instruction (a lookahead's mark slot, negativity and end).
pub(crate) struct Stencil {
    pub(crate) s: S,
    program: &'static RegexpProgram,
    /// A push found the stack full: the row leaves through the overflow routine.
    overflowed: bool,
    look: (usize, bool, u32),
}

crate::projection!(Stencil => StencilStage);

/// The test of a test word.
#[inline(always)]
fn test_of(word: u64) -> UnitTest {
    if word & TEST_SET != 0 { UnitTest::Set(word as u32) } else { UnitTest::Exact(word as u32) }
}

/// The test of the hole `H`, of the kind `T` (`EXACT`, `SET`).
#[inline(always)]
fn test<const T: u8, const H: u8>() -> UnitTest {
    let value = hole64::<H>() as u32;
    if T == SET { UnitTest::Set(value) } else { UnitTest::Exact(value) }
}

#[inline(always)]
const fn assertion(code: u8) -> Assertion {
    match code {
        0 => Assertion::Start,
        1 => Assertion::End,
        2 => Assertion::StartBuffer,
        3 => Assertion::EndBuffer,
        4 => Assertion::EndBufferOptionalNewline,
        5 => Assertion::Boundary,
        _ => Assertion::NonBoundary,
    }
}

impl Stencil {
    #[inline(always)]
    unsafe fn of(s: S) -> Self {
        // SAFETY: hole 0 is the stitched program's record, which lives as long as its code.
        let program = unsafe { &*(hole64::<HOLE_PROGRAM>() as *const RegexpProgram) };
        Self { s, program, overflowed: false, look: (0, false, 0) }
    }

    #[inline(always)]
    fn context(&self, offset: usize) -> usize {
        // SAFETY: the entry keeps its words behind the context.
        unsafe { *((self.s.ctx + offset) as *const usize) }
    }

    #[inline(always)]
    fn set_context(&mut self, offset: usize, value: usize) {
        // SAFETY: as `context`.
        unsafe { *((self.s.ctx + offset) as *mut usize) = value }
    }

    /// The id of the resume of `kind` at `pc`.
    #[inline(always)]
    fn resume_id(&self, kind: u8, pc: u32) -> usize {
        FIRST_RESUME + kind as usize * self.program.insts + pc as usize
    }

    /// Pushes a frame `(id, first, second)`; a full stack marks the row overflowed.
    #[inline(always)]
    fn push_frame(&mut self, id: usize, first: usize, second: usize) {
        let top = self.s.p[1];
        let after = top + FRAME_BYTES;
        if after > self.context(CONTEXT_STACK_END) {
            self.overflowed = true;
            return;
        }
        // SAFETY: `top..after` lies below the stack's end.
        unsafe {
            let frame = top as *mut usize;
            *frame = id;
            *frame.add(1) = first;
            *frame.add(2) = second;
        }
        self.s.p[1] = after;
    }

    /// The frame at `address`: its id and operands.
    #[inline(always)]
    fn frame(address: usize) -> [usize; FRAME_WORDS] {
        // SAFETY: `address` is a pushed frame.
        unsafe { *(address as *const [usize; FRAME_WORDS]) }
    }

    /// The register a restore frame names gets its old value back.
    #[inline(always)]
    fn write_back(&mut self, [_, register, value]: [usize; FRAME_WORDS]) {
        self.store(register as u32, value);
    }

    /// Pops and undoes every frame above `mark`, the mark's own frame then dropped.
    #[inline(always)]
    fn undo_to(&mut self, mark: usize) {
        while self.s.p[1] != mark + FRAME_BYTES {
            self.s.p[1] -= FRAME_BYTES;
            let frame = Self::frame(self.s.p[1]);
            if frame[0] == RESTORE {
                self.write_back(frame);
            }
        }
        self.s.p[1] = mark;
    }

    /// Moves the restore frames above `mark` down over it, in order, and drops the rest.
    #[inline(always)]
    fn keep_restores(&mut self, mark: usize) {
        let (mut from, mut to) = (mark + FRAME_BYTES, mark);
        while from != self.s.p[1] {
            let frame = Self::frame(from);
            if frame[0] == RESTORE {
                // SAFETY: `to` is below `from`, a frame of the stack.
                unsafe { *(to as *mut [usize; FRAME_WORDS]) = frame };
                to += FRAME_BYTES;
            }
            from += FRAME_BYTES;
        }
        self.s.p[1] = to;
    }

    /// Where `step` goes: the next or taken pc's row (patched), any other pc's row by its
    /// address, the backtrack row, or a leave routine.
    #[inline(always)]
    fn finish(mut self, step: Step<usize>, next: u32, taken: u32) -> Way {
        if self.overflowed {
            return Way::Jump(self.s, self.program.overflow);
        }
        match step {
            Step::Continue(cursor) => {
                self.s.p[0] = cursor.position;
                if cursor.pc == next {
                    Way::Next(self.s)
                } else if cursor.pc == taken {
                    Way::Taken(self.s)
                } else {
                    // SAFETY: every pc has a row.
                    Way::Jump(self.s, unsafe { *self.program.pcs.add(cursor.pc as usize) })
                }
            }
            Step::Fail | Step::Branched => Way::Fail(self.s),
            Step::Accept => Way::Jump(self.s, self.program.accept),
        }
    }

    #[inline(always)]
    fn mask_has(mask: [u64; 2], unit: usize) -> bool {
        mask[(unit >> 6) & 1] >> (unit & 63) & 1 != 0
    }
}

impl MatchStage for Stencil {
    type Pos = usize;
    type Unit = usize;
    type Flag = bool;
    type Test = UnitTest;

    #[inline(always)]
    fn length(&mut self) -> usize {
        self.s.p[3]
    }

    #[inline(always)]
    fn at_start(&mut self, position: usize) -> bool {
        position == 0
    }

    #[inline(always)]
    fn equal(&mut self, left: usize, right: usize) -> bool {
        left == right
    }

    #[inline(always)]
    fn before(&mut self, left: usize, right: usize) -> bool {
        left < right
    }

    #[inline(always)]
    fn offset(&mut self, position: usize, delta: isize) -> usize {
        position.wrapping_add_signed(delta)
    }

    #[inline(always)]
    fn unit_at(&mut self, index: usize) -> usize {
        // SAFETY: the subject holds `index`, which lies before its length.
        unsafe { usize::from(*(self.s.p[2] as *const u8).add(index)) }
    }

    /// An ASCII subject's unit: a literal at or past 128 matches none, a set by its ASCII mask.
    #[inline(always)]
    fn passes(&mut self, unit: usize, test: UnitTest) -> bool {
        match test {
            UnitTest::Exact(expected) => unit == expected as usize,
            // SAFETY: a set test names one of the program's sets.
            UnitTest::Set(set) => Self::mask_has(unsafe { *self.program.masks.add(set as usize) }, unit),
        }
    }

    #[inline(always)]
    fn in_ranges(&mut self, unit: usize, ranges: &[(u32, u32)]) -> bool {
        let unit = unit as u32;
        let mut index = 0;
        while index < ranges.len() {
            let (first, last) = ranges[index];
            if first <= unit && unit <= last {
                return true;
            }
            index += 1;
        }
        false
    }

    #[inline(always)]
    fn not(&mut self, flag: bool) -> bool {
        !flag
    }

    #[inline(always)]
    fn differ(&mut self, left: bool, right: bool) -> bool {
        left != right
    }

    #[inline(always)]
    fn guarded(&mut self, condition: bool, then: impl FnOnce(&mut Self) -> bool) -> bool {
        condition && then(self)
    }

    #[inline(always)]
    fn or_else(&mut self, first: bool, otherwise: impl FnOnce(&mut Self) -> bool) -> bool {
        first || otherwise(self)
    }
}

impl Machine for Stencil {
    #[inline(always)]
    fn constant(&mut self, value: usize) -> usize {
        value
    }

    #[inline(always)]
    fn unset(&mut self) -> usize {
        self.program.unset
    }

    #[inline(always)]
    fn load(&mut self, register: u32) -> usize {
        // SAFETY: the frame is the run's registers.
        unsafe { *(self.s.frame as *const usize).add(register as usize) }
    }

    #[inline(always)]
    fn store(&mut self, register: u32, value: usize) {
        // SAFETY: as `load`.
        unsafe { *(self.s.frame as *mut usize).add(register as usize) = value }
    }

    #[inline(always)]
    fn taken_end(&mut self, test: UnitTest, from: usize, to: usize) -> usize {
        let mut index = from;
        while index < to {
            let unit = self.unit_at(index);
            if !self.passes(unit, test) {
                break;
            }
            index += 1;
        }
        index
    }

    #[inline(always)]
    fn ceiling(&mut self, position: usize, count: u32) -> usize {
        let length = self.s.p[3];
        if count == UNBOUNDED {
            return length;
        }
        let reach = position + count as usize;
        if reach < length { reach } else { length }
    }

    #[inline(always)]
    fn push_restore(&mut self, register: u32, old: usize) {
        self.push_frame(RESTORE, register as usize, old);
    }

    #[inline(always)]
    fn push_capture_restore(&mut self, capture: u32, start: usize, end: usize) {
        self.push_restore(m::CAPTURE_REGISTERS * capture, start);
        self.push_restore(m::CAPTURE_REGISTERS * capture + 1, end);
    }

    #[inline(always)]
    fn push_choice(&mut self, pc: u32, position: usize) {
        let id = self.resume_id(RESUME_CHOICE, pc);
        self.push_frame(id, position, position);
    }

    #[inline(always)]
    fn push_greedy(&mut self, pc: u32, floor: usize, position: usize) {
        let id = self.resume_id(RESUME_GREEDY, pc);
        self.push_frame(id, floor, position);
    }

    #[inline(always)]
    fn push_lazy(&mut self, pc: u32, _: UnitTest, ceiling: usize, position: usize) {
        let id = self.resume_id(RESUME_LAZY, pc);
        self.push_frame(id, ceiling, position);
    }

    #[inline(always)]
    fn last_passing(&mut self, test: UnitTest, floor: usize, end: usize) -> Option<usize> {
        let mut at = end - 1;
        loop {
            let unit = self.unit_at(at);
            if self.passes(unit, test) {
                return Some(at);
            }
            if at <= floor {
                return None;
            }
            at -= 1;
        }
    }

    #[inline(always)]
    fn keep_greedy(&mut self, pc: u32, floor: usize, position: usize) {
        self.push_greedy(pc, floor, position);
    }

    #[inline(always)]
    fn keep_lazy(&mut self, pc: u32, test: UnitTest, ceiling: usize, position: usize) {
        self.push_lazy(pc, test, ceiling, position);
    }

    #[inline(always)]
    fn listed(&mut self, index: u32) -> u32 {
        // SAFETY: the instruction names one of the program's list entries.
        unsafe { *self.program.lists.add(index as usize) }
    }

    #[inline(always)]
    fn follower(&mut self, pc: u32) -> Option<UnitTest> {
        // SAFETY: the table has a word per pc, and one past the last.
        let word = unsafe { *self.program.followers.add(pc as usize) };
        (word != NO_TEST).then(|| test_of(word))
    }

    #[inline(always)]
    fn guard(&mut self, guard: u32) -> Admission<UnitTest> {
        // SAFETY: the instruction names one of the program's guards.
        let [at_start, at_end, units] = unsafe { *self.program.guards.add(guard as usize) };
        Admission { at_start: at_start != 0, at_end: at_end != 0, units: test_of(units) }
    }

    /// The lookahead's mark (the stack's top, in its slot) under a frame that a backtrack to it
    /// resumes as the body's failure; the body runs next.
    #[inline(always)]
    fn lookaround(&mut self, _: (bool, u32), at: Cursor<usize>) -> Step<usize> {
        let (slot, ..) = self.look;
        let top = self.s.p[1];
        self.set_context(CONTEXT_MARKS + slot, top);
        let id = self.resume_id(RESUME_LOOK_FAIL, at.pc);
        self.push_frame(id, at.position, 0);
        Step::Continue(at.next())
    }

    /// The body matched: a positive lookahead keeps its restores and goes on from where it
    /// began; a negative one undoes the body and fails.
    #[inline(always)]
    fn look_succeed(&mut self, _: Cursor<usize>) -> Step<usize> {
        let (slot, negative, end) = self.look;
        let mark = self.context(CONTEXT_MARKS + slot);
        let [_, position, _] = Self::frame(mark);
        if negative {
            self.undo_to(mark);
            return Step::Fail;
        }
        self.keep_restores(mark);
        Step::Continue(Cursor::at(end, position))
    }

    /// The interval of the table that holds `unit`, its ASCII part, runs its arm.
    #[inline(always)]
    fn dispatch_on(&mut self, unit: usize, table: u32, mut arm: impl FnMut(&mut Self, (u32, u32)) -> Step<usize>) -> Step<usize> {
        // SAFETY: the table's intervals lie between its start and the next table's.
        let (mut at, end) = unsafe { (*self.program.dispatch_starts.add(table as usize), *self.program.dispatch_starts.add(table as usize + 1)) };
        let unit = unit as u32;
        while at < end {
            // SAFETY: as above.
            let [lo, hi, first, count] = unsafe { *self.program.intervals.add(at as usize) };
            if lo <= unit && unit <= hi {
                return arm(self, (first, count));
            }
            at += 1;
        }
        Step::Fail
    }

    /// The class's matches come from `native_class_ends`, which pushes the shorter ones as
    /// choice frames itself.
    #[inline(always)]
    fn class_ends(&mut self, class: u32, position: usize, backward: bool, pc: u32) -> (bool, usize) {
        let choice = self.resume_id(RESUME_CHOICE, pc);
        let top = self.s.p[1];
        // SAFETY: the record's leaf is `native_class_ends`, the classes its program's.
        let ends = unsafe {
            let leaf: ClassEnds = core::mem::transmute(self.program.class_ends);
            leaf(self.program.classes, (class as usize) << 1 | backward as usize, self.s.p[2] as *const u8, self.s.p[3], position, top as *mut usize, self.context(CONTEXT_STACK_END) as *mut usize, choice)
        };
        if ends == CLASS_OVERFLOW {
            self.overflowed = true;
            return (false, position);
        }
        if ends == CLASS_NONE {
            return (false, position);
        }
        self.s.p[1] = top + (ends >> CLASS_COUNT_SHIFT) * FRAME_BYTES;
        (true, ends & u32::MAX as usize)
    }

    #[inline(always)]
    fn pick(&mut self, flag: bool, then: usize, otherwise: usize) -> usize {
        if flag { then } else { otherwise }
    }

    #[inline(always)]
    fn plus(&mut self, left: usize, right: usize) -> usize {
        left + right
    }

    #[inline(always)]
    fn minus(&mut self, left: usize, right: usize) -> usize {
        left - right
    }

    #[inline(always)]
    fn reach_back(&mut self, position: usize, count: u32) -> usize {
        if count == UNBOUNDED || position < count as usize { 0 } else { position - count as usize }
    }

    #[inline(always)]
    fn run_start(&mut self, test: UnitTest, from: usize, to: usize) -> usize {
        let mut index = from;
        while index > to {
            let unit = self.unit_at(index - 1);
            if !self.passes(unit, test) {
                break;
            }
            index -= 1;
        }
        index
    }

    #[inline(always)]
    fn same_units(&mut self, (from, to): (usize, usize), at: usize, (ignore_case, unicode): (bool, bool)) -> bool {
        let (mut expected, mut actual) = (from, at);
        while expected < to {
            let (left, right) = (self.unit_at(expected), self.unit_at(actual));
            let folded = ignore_case && left ^ right == CASE_BIT && Self::mask_has(self.program.folds[usize::from(unicode)], left);
            if left != right && !folded {
                return false;
            }
            expected += 1;
            actual += 1;
        }
        true
    }

    #[inline(always)]
    fn push_greedy_back(&mut self, pc: u32, ceiling: usize, position: usize) {
        let id = self.resume_id(RESUME_GREEDY_BACK, pc);
        self.push_frame(id, ceiling, position);
    }

    #[inline(always)]
    fn push_lazy_back(&mut self, pc: u32, _: UnitTest, floor: usize, position: usize) {
        let id = self.resume_id(RESUME_LAZY_BACK, pc);
        self.push_frame(id, floor, position);
    }

    #[inline(always)]
    fn keep_greedy_back(&mut self, pc: u32, ceiling: usize, position: usize) {
        self.push_greedy_back(pc, ceiling, position);
    }

    #[inline(always)]
    fn keep_lazy_back(&mut self, pc: u32, test: UnitTest, floor: usize, position: usize) {
        self.push_lazy_back(pc, test, floor, position);
    }

    #[inline(always)]
    fn require(&mut self, flag: bool) -> bool {
        flag
    }

    #[inline(always)]
    fn when(&mut self, flag: bool, effect: impl FnOnce(&mut Self)) {
        if flag {
            effect(self);
        }
    }

    #[inline(always)]
    fn choose(&mut self, flag: bool, then: impl FnOnce(&mut Self) -> Step<usize>, otherwise: impl FnOnce(&mut Self) -> Step<usize>) -> Step<usize> {
        if flag { then(self) } else { otherwise(self) }
    }
}

/// The row of codes `K`, `A`, `B` (`regexp_layout.rs`): hole 1 its pc, holes 2 to 4 its
/// instruction's operands; its way on (`Stencil::finish`) the next pc, or the pc the kind takes.
#[inline(always)]
pub(crate) unsafe fn run<const K: u8, const A: u8, const B: u8>(s: S) -> Way {
    let mut stage = unsafe { Stencil::of(s) };
    let pc = hole64::<HOLE_PC>() as u32;
    let (first, second, third) = (hole64::<HOLE_FIRST>(), hole64::<HOLE_SECOND>(), hole64::<HOLE_THIRD>());
    let at = Cursor::at(pc, stage.s.p[0]);
    let none = u32::MAX;
    let (step, taken) = match K {
        UNIT => (m::unit(&mut stage, test::<A, HOLE_FIRST>(), at), none),
        UNIT_BACK => (m::unit_back(&mut stage, test::<A, HOLE_FIRST>(), at), none),
        UNIT_REPEAT => (m::unit_repeat(&mut stage, (test::<A, HOLE_FIRST>(), second as u32, third as u32), (B & 1 != 0, B & 2 != 0), at), none),
        UNIT_REPEAT_BACK => (m::unit_repeat_back(&mut stage, (test::<A, HOLE_FIRST>(), second as u32, third as u32, B != 0), at), none),
        ASSERT => {
            let test = AssertionTest { assertion: const { assertion(A) }, multiline: B & 1 != 0, word_ignore_case: B & 2 != 0 };
            (m::assertion(&mut stage, test, at), none)
        }
        SPLIT => (m::split(&mut stage, first as u32, at), none),
        GUARDED_SPLIT => {
            let guard = stage.guard(second as u32);
            (m::guarded_split(&mut stage, first as u32, guard, at), first as u32)
        }
        DISPATCH => (m::dispatch(&mut stage, first as u32, at), none),
        JUMP => return Way::Taken(stage.s),
        CAPTURE_OPEN => (m::capture_open(&mut stage, (first as u32, second as u32, A != 0), at), none),
        CAPTURE_CLOSE => (m::capture_close(&mut stage, (first as u32, second as u32), at), none),
        CAPTURE_CLOSE_BACK => (m::capture_close_back(&mut stage, (first as u32, second as u32), at), none),
        RESET_CAPTURES => (m::reset_captures(&mut stage, (first as u32, second as u32), at), none),
        LOOP_INIT => (m::loop_init(&mut stage, first as u32, at), none),
        // Hole 2 the counter, hole 3 the minimum (high half) and maximum, hole 4 the exit.
        LOOP_HEAD => (m::loop_head(&mut stage, (first as u32, (second >> 32) as u32, second as u32), (A != 0, third as u32), at), third as u32),
        LOOP_ITERATE => (m::loop_iterate(&mut stage, first as u32, at), none),
        EMPTY_CHECK => (m::empty_check(&mut stage, first as u32, at), none),
        // Hole 2 the counter (high half) and start, hole 3 the minimum, hole 4 the head.
        LOOP_TAIL => (m::loop_tail(&mut stage, ((first >> 32) as u32, first as u32, second as u32, third as u32), at), third as u32),
        BACKREFERENCE => (m::backreference(&mut stage, (first as u32, second as u32), (A & 1 != 0, A & 2 != 0, B != 0), at), none),
        CLASS_WIDTHS => (m::class_widths(&mut stage, (first as u32, A != 0), at), none),
        // Hole 2 the end, hole 3 the mark's slot.
        LOOK => {
            stage.look = (second as usize, A != 0, first as u32);
            (stage.lookaround((A != 0, first as u32), at), none)
        }
        LOOK_SUCCEED => {
            stage.look = (second as usize, A != 0, first as u32);
            (stage.look_succeed(at), first as u32)
        }
        _ => (Step::Fail, none),
    };
    stage.finish(step, pc + 1, taken)
}

/// The resume of kind `A` (its test's kind `B`) at hole 1's pc, from the frame the backtrack
/// row popped (at the stack's top): the shared row (`machine::resume`) goes on taken at the pc
/// (a negative lookahead's failure at its end, hole 2), or at the one after it by its row's
/// address (a greedy repeat whose follower took its unit).
#[inline(always)]
pub(crate) unsafe fn resume<const A: u8, const B: u8>(s: S) -> Way {
    let mut stage = unsafe { Stencil::of(s) };
    let pc = hole64::<HOLE_PC>() as u32;
    let [_, first, second] = Stencil::frame(stage.s.p[1]);
    let entry = match A {
        RESUME_CHOICE => Resumed::Choice { pc, position: second },
        RESUME_GREEDY => Resumed::Greedy { pc, floor: first, position: second },
        RESUME_LAZY => Resumed::Lazy { pc, test: test::<B, HOLE_FIRST>(), ceiling: first, position: second },
        RESUME_GREEDY_BACK => Resumed::GreedyBack { pc, ceiling: first, position: second },
        RESUME_LAZY_BACK => Resumed::LazyBack { pc, test: test::<B, HOLE_FIRST>(), floor: first, position: second },
        // A lookahead's mark: the body found no match. Hole 2 the lookahead's end.
        _ => {
            if B == 0 {
                return Way::Fail(stage.s);
            }
            stage.s.p[0] = first;
            return Way::Taken(stage.s);
        }
    };
    let step = m::resume(&mut stage, entry);
    stage.finish(step, u32::MAX, pc)
}

/// Pops frames: a restore writes its register back and the pops go on, a barrier goes on to
/// the restart row (taken), a resume to its row.
#[inline(always)]
pub(crate) unsafe fn backtrack(s: S) -> Way {
    let mut stage = unsafe { Stencil::of(s) };
    loop {
        stage.s.p[1] -= FRAME_BYTES;
        let frame = Stencil::frame(stage.s.p[1]);
        match frame[0] {
            RESTORE => stage.write_back(frame),
            BARRIER => return Way::Taken(stage.s),
            // SAFETY: a resume id indexes the program's resumes.
            id => return Way::Jump(stage.s, unsafe { *stage.program.resumes.add(id - FIRST_RESUME) }),
        }
    }
}

/// The run from this start failed: the caller finds the next start (`RESTART_CALLER`), or the
/// row moves to the next position past it (past the units its leading repeat took, where
/// `SKIPS`) that the mode admits and runs from there above a new barrier (taken: the entry's
/// row), failing past the last start.
#[inline(always)]
pub(crate) unsafe fn restart<const MODE: u8, const SKIPS: u8>(s: S) -> Way {
    let mut stage = unsafe { Stencil::of(s) };
    if MODE == RESTART_CALLER {
        return Way::Jump(stage.s, stage.program.fail);
    }
    let mut start = stage.context(CONTEXT_START);
    if SKIPS != 0 {
        let skipped = test_of(stage.program.restart_skip);
        while matcher::takes_unit(&mut stage, skipped, start) {
            start += 1;
        }
    }
    let last = stage.context(CONTEXT_LAST);
    let [at_start, at_end, units] = stage.program.restart;
    let admission = Admission { at_start: at_start != 0, at_end: at_end != 0, units: test_of(units) };
    loop {
        start += 1;
        if start > last {
            return Way::Jump(stage.s, stage.program.fail);
        }
        if MODE == RESTART_EVERY || admits(&mut stage, admission, start) {
            break;
        }
    }
    stage.set_context(CONTEXT_START, start);
    // SAFETY: the entry's found word is the caller's start out-parameter.
    unsafe { *(stage.context(CONTEXT_FOUND) as *mut usize) = start };
    stage.s.p[0] = start;
    stage.push_frame(BARRIER, 0, 0);
    if stage.overflowed {
        return Way::Jump(stage.s, stage.program.overflow);
    }
    Way::Taken(stage.s)
}

/// The first row: the run's barrier, then the entry instruction's row (taken).
#[inline(always)]
pub(crate) unsafe fn enter(s: S) -> Way {
    let mut stage = unsafe { Stencil::of(s) };
    stage.push_frame(BARRIER, 0, 0);
    if stage.overflowed {
        return Way::Jump(stage.s, stage.program.overflow);
    }
    Way::Taken(stage.s)
}

/// The row of codes `K`, `A`, `B`, whatever its kind.
#[inline(always)]
pub(crate) unsafe fn row<const K: u8, const A: u8, const B: u8>(s: S) -> Way {
    unsafe {
        match K {
            RESUME => resume::<A, B>(s),
            BACKTRACK => backtrack(s),
            RESTART => restart::<A, B>(s),
            ENTER => enter(s),
            _ => run::<K, A, B>(s),
        }
    }
}
