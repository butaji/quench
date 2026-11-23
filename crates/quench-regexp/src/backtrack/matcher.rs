//! The matcher's operations on units and positions, written once over a `MatchStage`: the
//! unit test, the guard, the assertions. The interpreter runs them on its `Concrete` stage
//! (plain Rust: positions are `usize`, flags `bool`, every method `#[inline(always)]`), and the
//! native compiler runs the same functions on a stage that emits code (a position is a value, a
//! flag a branch condition), so each operation has one definition for both. What is static (a
//! test, a flag of the pattern) is an argument; what is dynamic (a position, a unit, a flag
//! computed from them) is a stage value.
use crate::{Assertion, FOLDED_WORD_UNITS, LINE_TERMINATORS, WORD_UNITS};

/// The dynamic accesses the operations make.
pub trait MatchStage: quench_stack::projection::Sealed {
    /// A position in the subject; `0..=length`.
    type Pos: Copy;
    /// A unit of the subject.
    type Unit: Copy;
    /// A truth value.
    type Flag: Copy;
    /// What one unit must be, a static row of the program.
    type Test: Copy;

    fn length(&mut self) -> Self::Pos;
    fn at_start(&mut self, position: Self::Pos) -> Self::Flag;
    fn equal(&mut self, left: Self::Pos, right: Self::Pos) -> Self::Flag;
    fn before(&mut self, left: Self::Pos, right: Self::Pos) -> Self::Flag;
    /// `position` moved by `delta` units; the result stays inside the subject.
    fn offset(&mut self, position: Self::Pos, delta: isize) -> Self::Pos;
    /// The unit at `index`, which lies before the end.
    fn unit_at(&mut self, index: Self::Pos) -> Self::Unit;
    fn passes(&mut self, unit: Self::Unit, test: Self::Test) -> Self::Flag;
    /// Whether `unit` is in one of the inclusive ranges.
    fn in_ranges(&mut self, unit: Self::Unit, ranges: &[(u32, u32)]) -> Self::Flag;
    fn not(&mut self, flag: Self::Flag) -> Self::Flag;
    fn differ(&mut self, left: Self::Flag, right: Self::Flag) -> Self::Flag;
    /// `condition && then`, where `then` is computed only if `condition` holds.
    fn guarded(&mut self, condition: Self::Flag, then: impl FnOnce(&mut Self) -> Self::Flag) -> Self::Flag;
    /// `first || otherwise`, where `otherwise` is computed only if `first` does not hold.
    fn or_else(&mut self, first: Self::Flag, otherwise: impl FnOnce(&mut Self) -> Self::Flag) -> Self::Flag;
}

/// An assertion with the flags that decide it.
#[derive(Clone, Copy)]
pub struct AssertionTest {
    pub assertion: Assertion,
    pub multiline: bool,
    /// `is_word_mode`'s case-folding argument.
    pub word_ignore_case: bool,
}

/// The positions a body can start at: the subject's start, its end, and where a unit passes
/// `units`.
#[derive(Clone, Copy)]
pub struct Admission<Test> {
    pub at_start: bool,
    pub at_end: bool,
    pub units: Test,
}

/// Whether a unit lies at `at` and passes `test`.
#[inline(always)]
pub fn takes_unit<S: MatchStage>(stage: &mut S, test: S::Test, at: S::Pos) -> S::Flag {
    let length = stage.length();
    let inside = stage.before(at, length);
    stage.guarded(inside, |stage| {
        let unit = stage.unit_at(at);
        stage.passes(unit, test)
    })
}

/// Whether a body guarded by `guard` can start at `position`.
#[inline(always)]
pub fn admits<S: MatchStage>(stage: &mut S, guard: Admission<S::Test>, position: S::Pos) -> S::Flag {
    let Admission { at_start, at_end, units } = guard;
    let body = move |stage: &mut S| {
        let length = stage.length();
        let inside = stage.before(position, length);
        let hit = stage.guarded(inside, |stage| {
            let unit = stage.unit_at(position);
            stage.passes(unit, units)
        });
        if !at_end {
            return hit;
        }
        let outside = stage.not(inside);
        stage.or_else(hit, move |_| outside)
    };
    if !at_start {
        return body(stage);
    }
    let start = stage.at_start(position);
    stage.or_else(start, body)
}

/// Whether `assertion` holds at `position`.
#[inline(always)]
pub fn assertion_holds<S: MatchStage>(stage: &mut S, test: AssertionTest, position: S::Pos) -> S::Flag {
    match test.assertion {
        Assertion::Start => start_of_line(stage, position, test.multiline),
        Assertion::End => end_of_line(stage, position, test.multiline),
        Assertion::StartBuffer => stage.at_start(position),
        Assertion::EndBuffer => at_end(stage, position),
        Assertion::EndBufferOptionalNewline => end_or_final_newline(stage, position),
        Assertion::Boundary => boundary(stage, position, test.word_ignore_case),
        Assertion::NonBoundary => {
            let at_boundary = boundary(stage, position, test.word_ignore_case);
            stage.not(at_boundary)
        }
    }
}

#[inline(always)]
fn at_end<S: MatchStage>(stage: &mut S, position: S::Pos) -> S::Flag {
    let length = stage.length();
    stage.equal(position, length)
}

/// Whether the unit at `index`, inside the subject, is a line terminator.
#[inline(always)]
fn terminator_at<S: MatchStage>(stage: &mut S, index: S::Pos) -> S::Flag {
    let unit = stage.unit_at(index);
    stage.in_ranges(unit, &LINE_TERMINATORS)
}

#[inline(always)]
fn start_of_line<S: MatchStage>(stage: &mut S, position: S::Pos, multiline: bool) -> S::Flag {
    let start = stage.at_start(position);
    if !multiline {
        return start;
    }
    stage.or_else(start, |stage| {
        let before = stage.offset(position, -1);
        terminator_at(stage, before)
    })
}

#[inline(always)]
fn end_of_line<S: MatchStage>(stage: &mut S, position: S::Pos, multiline: bool) -> S::Flag {
    let end = at_end(stage, position);
    if !multiline {
        return end;
    }
    stage.or_else(end, |stage| terminator_at(stage, position))
}

/// `$` without the multiline flag in Python's sense: the end, or before one final line
/// terminator, which is a `\r\n` pair at two units from the end.
#[inline(always)]
fn end_or_final_newline<S: MatchStage>(stage: &mut S, position: S::Pos) -> S::Flag {
    let end = at_end(stage, position);
    stage.or_else(end, |stage| {
        let length = stage.length();
        let one = stage.offset(position, 1);
        let last = stage.equal(one, length);
        let single = stage.guarded(last, |stage| terminator_at(stage, position));
        stage.or_else(single, |stage| {
            let two = stage.offset(position, CRLF_LENGTH);
            let before_last = stage.equal(two, length);
            stage.guarded(before_last, |stage| {
                let (first, second) = (stage.unit_at(position), stage.unit_at(one));
                let carriage_return = stage.in_ranges(first, &[CARRIAGE_RETURN]);
                let line_feed = stage.in_ranges(second, &[LINE_FEED]);
                stage.guarded(carriage_return, move |_| line_feed)
            })
        })
    })
}

/// The length of a CR LF line terminator.
const CRLF_LENGTH: isize = 2;
const CARRIAGE_RETURN: (u32, u32) = (0x000D, 0x000D);
const LINE_FEED: (u32, u32) = (0x000A, 0x000A);

/// Whether the unit at `index` is a word unit.
#[inline(always)]
fn word_at<S: MatchStage>(stage: &mut S, index: S::Pos, ignore_case: bool) -> S::Flag {
    let unit = stage.unit_at(index);
    let word = stage.in_ranges(unit, &WORD_UNITS);
    if !ignore_case {
        return word;
    }
    stage.or_else(word, |stage| stage.in_ranges(unit, &FOLDED_WORD_UNITS))
}

/// `assertion_holds` of `\b` or `\B` at `position` where the unit there is known to be a
/// word unit (`word`) inside the subject, or known not to be one (or the subject's end): only
/// the unit before it is read. `None` for any other assertion.
pub fn word_assertion_given<S: MatchStage>(stage: &mut S, test: AssertionTest, position: S::Pos, word: bool) -> Option<S::Flag> {
    let boundary = match test.assertion {
        Assertion::Boundary => true,
        Assertion::NonBoundary => false,
        _ => return None,
    };
    let start = stage.at_start(position);
    let after_start = stage.not(start);
    let left = stage.guarded(after_start, |stage| {
        let before = stage.offset(position, -1);
        word_at(stage, before, test.word_ignore_case)
    });
    // `\b` holds where the left unit differs from the known right one, `\B` where it agrees.
    Some(if boundary == word { stage.not(left) } else { left })
}

/// Whether the ASCII units of `mask` (bit `n` of the low word first) are all word units
/// (`Some(true)`) or none is (`Some(false)`); `None` where some are.
pub fn ascii_units_are_words(mask: [u64; 2]) -> Option<bool> {
    let units = (0..super::ASCII_UNITS).filter(|unit| mask[(unit / u64::BITS) as usize] >> (unit % u64::BITS) & 1 != 0);
    let (mut words, mut others) = (false, false);
    for unit in units {
        let word = WORD_UNITS.iter().any(|&(first, last)| (first..=last).contains(&unit));
        words |= word;
        others |= !word;
    }
    match (words, others) {
        (true, false) => Some(true),
        (false, _) => Some(false),
        (true, true) => None,
    }
}

/// `\b`: exactly one of the units either side of `position` is a word unit.
#[inline(always)]
fn boundary<S: MatchStage>(stage: &mut S, position: S::Pos, ignore_case: bool) -> S::Flag {
    let length = stage.length();
    let start = stage.at_start(position);
    let after_start = stage.not(start);
    let left = stage.guarded(after_start, |stage| {
        let before = stage.offset(position, -1);
        word_at(stage, before, ignore_case)
    });
    let before_end = stage.before(position, length);
    let right = stage.guarded(before_end, |stage| word_at(stage, position, ignore_case));
    stage.differ(left, right)
}
