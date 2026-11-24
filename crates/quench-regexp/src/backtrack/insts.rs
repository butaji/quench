//! A program's instructions and unit tests: plain data the rows (`machine`) read, shared by
//! the interpreter, the u-jit's emitter and the b-jit's stencil rows, which mount this file.
use super::matcher::AssertionTest;

#[derive(Clone, Copy)]
pub enum Assertion {
    Start,
    End,
    StartBuffer,
    EndBuffer,
    EndBufferOptionalNewline,
    Boundary,
    NonBoundary,
}

pub(crate) const WORD_UNITS: [(u32, u32); 4] = [(b'0' as u32, b'9' as u32), (b'A' as u32, b'Z' as u32), (b'_' as u32, b'_' as u32), (b'a' as u32, b'z' as u32)];
pub(crate) const FOLDED_WORD_UNITS: [(u32, u32); 2] = [(0x017F, 0x017F), (0x212A, 0x212A)];
pub(crate) const LINE_TERMINATORS: [(u32, u32); 3] = [(0x000A, 0x000A), (0x000D, 0x000D), (0x2028, 0x2029)];

/// What one subject unit must be.
#[derive(Clone, Copy)]
pub enum UnitTest {
    Exact(u32),
    /// An index into `Program::sets`.
    Set(u32),
}

/// One instruction of a program: the interpreter steps through them, and native code is
/// their rows (`machine`) specialized to them.
#[derive(Clone, Copy)]
pub enum Inst {
    Unit(UnitTest),
    /// `Unit` read backward, in a lookbehind body: the unit before the position.
    UnitBack(UnitTest),
    /// `UnitRepeat` read backward: the units before the position, the run ending there.
    UnitRepeatBack {
        test: UnitTest,
        min: u32,
        max: u32,
        greedy: bool,
    },
    /// `min..=max` units of a test, without captures and never empty, so its choices are
    /// only how many units it takes.
    UnitRepeat {
        test: UnitTest,
        min: u32,
        max: u32,
        greedy: bool,
        /// A greedy repeat that never gives a unit back (`possessive::mark`).
        atomic: bool,
    },
    Assert(AssertionTest),
    /// Continue at the next instruction; on backtracking, at `alternative`.
    Split {
        alternative: u32,
    },
    /// `Split` whose next instruction begins a body that can start only at the positions
    /// `Program::guards[guard]` admits: elsewhere the body fails at once, so the run goes
    /// straight to `alternative` and keeps no choice.
    GuardedSplit {
        alternative: u32,
        guard: u32,
    },
    /// An alternation's branches, each beginning with known units: continue at the first
    /// branch `Program::dispatches[table]` finds for the unit, the others being choices.
    Dispatch {
        table: u32,
    },
    Jump(u32),
    /// A group's start into its temporary register: with a restore entry where the group
    /// may run again on one path (`Compiler::repeated`), plainly elsewhere, as the group's
    /// own open then always writes the temporary again before its close reads it.
    CaptureOpen {
        temporary: u32,
        /// Groups opening together, as `((a)b)` does, write `count` consecutive temporaries.
        count: u32,
        restores: bool,
    },
    CaptureClose {
        capture: u32,
        temporary: u32,
    },
    /// `CaptureClose` of a backward body: the open recorded the capture's end, the close
    /// is at its start.
    CaptureCloseBack {
        capture: u32,
        temporary: u32,
    },
    /// Unset the captures listed in `Program::lists[first..first + count]`.
    ResetCaptures {
        first: u32,
        count: u32,
    },
    LoopInit {
        counter: u32,
    },
    /// A quantifier's decision before an iteration of the body that follows it.
    LoopHead {
        counter: u32,
        min: u32,
        max: u32,
        greedy: bool,
        exit: u32,
    },
    /// The start of one iteration, kept for the empty check.
    LoopIterate {
        start: u32,
    },
    /// The end of the one iteration an optional atom (`max` of one, `min` of zero) makes:
    /// it fails having consumed nothing.
    EmptyCheck {
        start: u32,
    },
    LoopTail {
        counter: u32,
        start: u32,
        min: u32,
        head: u32,
    },
    /// A backreference to the first set capture of `Program::lists[first..first + count]`.
    Backreference {
        first: u32,
        count: u32,
        ignore_case: bool,
        unicode: bool,
    },
    /// `Backreference` in a backward body: the captured units end at the position.
    BackreferenceBack {
        first: u32,
        count: u32,
        ignore_case: bool,
        unicode: bool,
    },
    /// A class that can match strings of several units (`Program::classes[class]`): the
    /// longest match first, each shorter one kept as a choice. Read backward in a lookbehind
    /// body, where its matches end at the position.
    ClassWidths {
        class: u32,
        backward: bool,
    },
    /// A lookaround whose body follows, ending at `LookSucceed`; matching continues at `end`.
    /// A lookbehind's body is compiled backward, so both run the same way.
    Look {
        negative: bool,
        end: u32,
    },
    LookSucceed,
    Match,
}
