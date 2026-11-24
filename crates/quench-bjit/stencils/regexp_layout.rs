//! The codes and the program record the regexp rows read (`regexp_rows.rs`), which the stencil
//! crate, the build script and the lib share: a row is a kind and two const operands, its
//! instruction's operands are its value holes, and what an instruction names in the program's
//! tables (sets, lists, guards, dispatch intervals, the code of each pc and resume) is read
//! through the `RegexpProgram` record whose address is every row's hole 0.

/// Hole 0 of every row: the program record's address. Hole 1: the row's pc.
pub const HOLE_PROGRAM: u8 = 0;
pub const HOLE_PC: u8 = 1;
/// The instruction's operands, in the order each kind names them.
pub const HOLE_FIRST: u8 = 2;
pub const HOLE_SECOND: u8 = 3;
pub const HOLE_THIRD: u8 = 4;

/// A unit test as one word: an exact unit, or a set's index above `TEST_SET`.
pub const TEST_SET: u64 = 1 << 32;
/// The word of no test (a pc whose instruction takes no single unit, `RegexpProgram::followers`).
pub const NO_TEST: u64 = u64::MAX;
/// A test's const operand: whether the row's test is an exact unit or a set.
pub const EXACT: u8 = 0;
pub const SET: u8 = 1;

/// The frame ids below `FIRST_RESUME`, which the backtrack row handles itself; the rest index
/// `RegexpProgram::resumes` as `FIRST_RESUME + kind * insts + pc`.
pub const RESTORE: usize = 0;
pub const BARRIER: usize = 1;
pub const FIRST_RESUME: usize = 2;
/// The resume kinds: how a popped frame continues.
pub const RESUME_CHOICE: u8 = 0;
pub const RESUME_GREEDY: u8 = 1;
pub const RESUME_LAZY: u8 = 2;
pub const RESUME_GREEDY_BACK: u8 = 3;
pub const RESUME_LAZY_BACK: u8 = 4;
pub const RESUME_LOOK_FAIL: u8 = 5;
pub const RESUME_KINDS: usize = 6;

/// The words of one frame (`quench_regexp::backtrack::native::NATIVE_FRAME_WORDS`): its id, then
/// two operands. The engine asserts the two equal.
pub const FRAME_WORDS: usize = 3;

/// How a failed start moves on (`NativeRestarts`): the caller finds the next one, every next
/// position is one, or the next position the program's restart admission admits.
pub const RESTART_CALLER: u8 = 0;
pub const RESTART_EVERY: u8 = 1;
pub const RESTART_ADMITTED: u8 = 2;

/// The words the entry keeps below the rows' context pointer: the stack's end, the last start,
/// the address the start is written to, the current start; the lookarounds' marks follow.
pub const CONTEXT_STACK_END: usize = 0;
pub const CONTEXT_LAST: usize = 8;
pub const CONTEXT_FOUND: usize = 16;
pub const CONTEXT_START: usize = 24;
pub const CONTEXT_MARKS: usize = 32;

/// The row kinds: one per instruction shape, then the routines' rows.
pub const UNIT: u8 = 0;
pub const UNIT_BACK: u8 = 1;
pub const UNIT_REPEAT: u8 = 2;
pub const UNIT_REPEAT_BACK: u8 = 3;
pub const ASSERT: u8 = 4;
pub const SPLIT: u8 = 5;
pub const GUARDED_SPLIT: u8 = 6;
pub const DISPATCH: u8 = 7;
pub const JUMP: u8 = 8;
pub const CAPTURE_OPEN: u8 = 9;
pub const CAPTURE_CLOSE: u8 = 10;
pub const CAPTURE_CLOSE_BACK: u8 = 11;
pub const RESET_CAPTURES: u8 = 12;
pub const LOOP_INIT: u8 = 13;
pub const LOOP_HEAD: u8 = 14;
pub const LOOP_ITERATE: u8 = 15;
pub const EMPTY_CHECK: u8 = 16;
pub const LOOP_TAIL: u8 = 17;
pub const BACKREFERENCE: u8 = 18;
pub const CLASS_WIDTHS: u8 = 19;
pub const LOOK: u8 = 20;
pub const LOOK_SUCCEED: u8 = 21;
/// A resume of the kind in the first const operand (`RESUME_*`), its test's kind in the second.
pub const RESUME: u8 = 22;
/// Pops frames: restores run, a barrier goes on to the restart row (taken), a resume jumps to its code.
pub const BACKTRACK: u8 = 23;
/// A failed start: the next start, by the restart mode in the first const operand, the second
/// whether the program skips its leading repeat's units.
pub const RESTART: u8 = 24;
/// The first row: the run's barrier, then the entry instruction (taken).
pub const ENTER: u8 = 25;

/// The routines around the rows: the entry (the `NativeFn` ABI) and the leaves with the match's
/// end, a failure and an overflow, which return to it.
pub const ROUTINES: [&str; 4] = ["entry_regexp", "leave_regexp_match", "leave_regexp_fail", "leave_regexp_overflow"];

/// The assertions as codes (`Assertion`'s order).
pub const ASSERTIONS: u8 = 7;

/// A row as codes: kind and two const operands.
pub type RegexpCode = (u8, u8, u8);

/// Every row the library holds.
#[cfg(not(stencil_build))]
pub fn regexp_library() -> Vec<RegexpCode> {
    let mut rows = Vec::new();
    for test in [EXACT, SET] {
        rows.push((UNIT, test, 0));
        rows.push((UNIT_BACK, test, 0));
        // Greedy in bit 0, atomic in bit 1 (an atomic repeat is greedy).
        for shape in [0, 1, 3] {
            rows.push((UNIT_REPEAT, test, shape));
        }
        for greedy in [0, 1] {
            rows.push((UNIT_REPEAT_BACK, test, greedy));
        }
        rows.push((RESUME, RESUME_LAZY, test));
        rows.push((RESUME, RESUME_LAZY_BACK, test));
    }
    for assertion in 0..ASSERTIONS {
        // Multiline in bit 0, the word test's case folding in bit 1.
        for flags in 0..4 {
            rows.push((ASSERT, assertion, flags));
        }
    }
    for kind in [SPLIT, GUARDED_SPLIT, DISPATCH, JUMP, CAPTURE_CLOSE, CAPTURE_CLOSE_BACK, RESET_CAPTURES, LOOP_INIT, LOOP_ITERATE, EMPTY_CHECK, LOOP_TAIL, BACKTRACK, ENTER] {
        rows.push((kind, 0, 0));
    }
    for flag in [0, 1] {
        rows.push((CAPTURE_OPEN, flag, 0));
        rows.push((LOOP_HEAD, flag, 0));
        rows.push((CLASS_WIDTHS, flag, 0));
        rows.push((LOOK, flag, 0));
        rows.push((LOOK_SUCCEED, flag, 0));
        // The look-fail resume of a positive (0) or negative (1) lookahead.
        rows.push((RESUME, RESUME_LOOK_FAIL, flag));
    }
    // Ignore case in bit 0, unicode in bit 1; the second operand reads backward.
    for flags in 0..4 {
        for backward in [0, 1] {
            rows.push((BACKREFERENCE, flags, backward));
        }
    }
    for kind in [RESUME_CHOICE, RESUME_GREEDY, RESUME_GREEDY_BACK] {
        rows.push((RESUME, kind, 0));
    }
    for mode in [RESTART_CALLER, RESTART_EVERY, RESTART_ADMITTED] {
        for skips in [0, 1] {
            rows.push((RESTART, mode, skips));
        }
    }
    rows
}

/// The name of a regexp row's stencil symbol; a row that calls is named so (`build/checks.rs`).
#[cfg(not(stencil_build))]
pub fn regexp_name((kind, first, second): RegexpCode) -> String {
    let call = if kind == CLASS_WIDTHS { "call_" } else { "" };
    format!("{call}re_{kind}_{first}_{second}")
}

/// The program record every row reads through hole 0. Pointers are to the stitched program's
/// own tables, which live as long as its code.
#[repr(C)]
pub struct RegexpProgram {
    /// Each pc's first row.
    pub pcs: *const usize,
    /// Each resume's row, by `id - FIRST_RESUME`.
    pub resumes: *const usize,
    /// The ASCII words of each set (`UnitTest::Set` indexes it).
    pub masks: *const [u64; 2],
    /// The capture and branch lists (`Program::lists`).
    pub lists: *const u32,
    /// Each guard: at the start, at the end, its units' test word.
    pub guards: *const [u64; 3],
    /// Each dispatch table's intervals (`lo, hi, first, count`), its ASCII part, from `dispatch_starts[table]`
    /// to `dispatch_starts[table + 1]`.
    pub intervals: *const [u32; 4],
    pub dispatch_starts: *const u32,
    /// The test of the instruction at each pc where it takes one unit, else `NO_TEST`.
    pub followers: *const u64,
    /// The case partners of the ASCII units, without and with `unicode`.
    pub folds: [[u64; 2]; 2],
    /// `NativePlan::classes`, and `native_class_ends`'s address.
    pub classes: usize,
    pub class_ends: usize,
    /// The value of an unset register.
    pub unset: usize,
    /// The program's instruction count, which spaces the resume kinds' ids.
    pub insts: usize,
    /// The entry instruction.
    pub entry: usize,
    /// The leave routines.
    pub accept: usize,
    pub fail: usize,
    pub overflow: usize,
    /// The restart's admission (at start, at end, test word) and the test of the units a failed
    /// start skips.
    pub restart: [u64; 3],
    pub restart_skip: u64,
}
