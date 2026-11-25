//! The regexp rows: each instruction shape's stencil is `quench_regexp`'s own row
//! (`backtrack::machine`) run on `Stencil` (`regexp_stage.rs`), the b-jit's projection of the
//! regexp `Machine`: positions and flags are machine words, the static operands are holes, and a
//! step's way on is the row's successor. The interpreter (`Concrete`) and the u-jit
//! (`staged::regexp`'s `Emit`) run the same rows, so the three tiers cannot disagree on what an
//! instruction means. The search driver's routines (`backtrack`, `restart`, `enter`) are the
//! stage's.
#![allow(dead_code)]

use crate::regexp_layout::*;
use crate::words::{S, hole64};

use super::regexp_stage::{assertion, backtrack, enter, restart, test, Stencil, Way};
use super::regexp_stage::machine::{self as m, Cursor, Machine, Resumed, Step};
use super::regexp_stage::matcher::AssertionTest;

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
