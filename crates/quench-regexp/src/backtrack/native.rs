//! A hot program's search over an ASCII subject as native code: the program as plain data for
//! a compiler the embedder registers (`set_native_compiler`), and the entry it returns, which
//! runs the same steps as `Program::run` (so the interpreter stays the one source of meaning)
//! on a stack of its own. A search the native code cannot finish (its stack is full) is run
//! again by the interpreter from the same start.
use super::{Assertion, AssertionTest, Inst, MatchSet, Program, Search, Text, UnitTest, ASCII_UNITS};
use std::cell::{Cell, OnceCell, RefCell};
use std::task::Poll;
use std::sync::OnceLock;

/// What one ASCII unit must be: a literal at or past 128 matches no ASCII unit.
#[derive(Clone, Copy)]
pub enum NativeTest {
    Never,
    Exact(u8),
    /// The set's ASCII units, bit `n` of the 128-bit mask (low word first).
    Mask([u64; 2]),
}

#[derive(Clone, Copy)]
pub enum NativeAssertion {
    Start { multiline: bool },
    End { multiline: bool },
    StartBuffer,
    EndBuffer,
    EndBufferOptionalNewline,
    Boundary,
    NonBoundary,
}

/// The positions a guarded body can start at.
#[derive(Clone, Copy)]
pub struct NativeGuard {
    pub at_start: bool,
    pub at_end: bool,
    pub mask: [u64; 2],
}

/// `Inst`, for the instructions a native search runs; `Dispatch`, `Backreference` and
/// lookarounds keep a program interpreted.
#[derive(Clone)]
pub enum NativeInst {
    Unit(NativeTest),
    UnitRepeat { test: NativeTest, min: u32, max: u32, greedy: bool },
    Assert(NativeAssertion),
    Split { alternative: u32 },
    GuardedSplit { alternative: u32, guard: NativeGuard },
    Jump(u32),
    CaptureOpen { temporary: u32, restores: bool },
    CaptureClose { capture: u32, temporary: u32 },
    ResetCaptures { captures: Vec<u32> },
    LoopInit { counter: u32 },
    LoopHead { counter: u32, min: u32, max: u32, greedy: bool, exit: u32 },
    LoopIterate { start: u32 },
    EmptyCheck { start: u32 },
    LoopTail { counter: u32, start: u32, min: u32, head: u32 },
    Match,
}

/// A program for a native compiler. The search of an ASCII subject begins at `entry` with the
/// position `start + entry` (`Program::entry`).
pub struct NativePlan {
    pub insts: Vec<NativeInst>,
    pub entry: u32,
    /// The registers a run uses; each holds `unset` between runs.
    pub registers: usize,
    pub unset: usize,
}

/// `fn(bytes, length, position, registers, stack, stack_end)`: the end of the match the
/// program finds at `position`, `NATIVE_FAIL` where none starts there (the registers are then
/// as they were), or `NATIVE_OVERFLOW` where the stack of `stack..stack_end` words held too
/// few frames (the registers are then unspecified).
pub type NativeFn = unsafe extern "C" fn(*const u8, usize, usize, *mut usize, *mut usize, *mut usize) -> usize;
pub const NATIVE_FAIL: usize = usize::MAX;
pub const NATIVE_OVERFLOW: usize = usize::MAX - 1;
/// A quantifier bound without a maximum.
pub const NATIVE_UNBOUNDED: u32 = super::UNBOUNDED;
/// The words of one backtracking frame: its resume id and two operands.
pub const NATIVE_FRAME_WORDS: usize = 3;

/// A compiled search and what keeps its code alive.
pub struct NativeCode {
    pub entry: NativeFn,
    pub keep: Box<dyn std::any::Any>,
}

/// What the embedder's compiler answers a program's request for code.
pub enum NativeOutcome {
    Code(NativeCode),
    /// The searches so far do not repay the compile; ask again after as many more.
    Later,
    /// The program has no native code.
    Never,
}

/// The embedder's compiler, which decides from the searches a program has run (the first
/// request comes after `first_request`, each next after twice the searches before it).
#[derive(Clone, Copy)]
pub struct NativeCompiler {
    pub first_request: u32,
    /// Starts compiling a plan for a program that has run `searches` searches, which the
    /// program polls for the outcome while the interpreter runs.
    pub start: fn(NativePlan, u32) -> NativeBuild,
}

/// A compile in progress, polled for what the embedder's compiler answers.
pub type NativeBuild = Box<dyn FnMut() -> Poll<NativeOutcome>>;

/// A building program asks its compile again on every this-many searches.
const POLL_EVERY: u32 = 64;

static COMPILER: OnceLock<NativeCompiler> = OnceLock::new();

thread_local! {
    /// Whether this thread's programs may compile (`enable_native_search`).
    static ENABLED: Cell<bool> = const { Cell::new(false) };
}

/// Lets the programs searched on this thread compile once hot, or not (`--no-jit`).
pub fn enable_native_search(enabled: bool) {
    ENABLED.set(enabled);
}

/// Registers the compiler the programs of this process use (the first call decides).
pub fn set_native_compiler(compiler: NativeCompiler) {
    let _ = COMPILER.set(compiler);
}

/// A program's native state: the searches it has run, then its code or the knowledge that it
/// has none.
#[derive(Default)]
pub(super) struct NativeSlot {
    runs: Cell<u32>,
    /// The search count of the next request for code.
    next_request: Cell<u32>,
    code: OnceCell<Option<NativeCode>>,
    building: RefCell<Option<NativeBuild>>,
}

/// The frames a native search starts with room for, and the most a search may grow it to.
const INITIAL_STACK_FRAMES: usize = 1024;
const MAXIMUM_STACK_FRAMES: usize = 1 << 20;
/// Each request for code waits for this many times the searches of the one before.
const REQUEST_GROWTH: u32 = 2;
/// A full stack grows by this factor.
const STACK_GROWTH: usize = 4;
/// The next start of a search lies a unit past the one that failed.
const NEXT_START: usize = 1;

fn native_test(test: UnitTest, sets: &[MatchSet]) -> NativeTest {
    match test {
        UnitTest::Exact(unit) if unit < ASCII_UNITS => NativeTest::Exact(unit as u8),
        UnitTest::Exact(_) => NativeTest::Never,
        UnitTest::Set(set) => NativeTest::Mask(sets[set as usize].table.0),
    }
}

fn native_assertion(test: AssertionTest) -> NativeAssertion {
    match test.assertion {
        Assertion::Start => NativeAssertion::Start { multiline: test.multiline },
        Assertion::End => NativeAssertion::End { multiline: test.multiline },
        Assertion::StartBuffer => NativeAssertion::StartBuffer,
        Assertion::EndBuffer => NativeAssertion::EndBuffer,
        Assertion::EndBufferOptionalNewline => NativeAssertion::EndBufferOptionalNewline,
        Assertion::Boundary => NativeAssertion::Boundary,
        Assertion::NonBoundary => NativeAssertion::NonBoundary,
    }
}

impl Program {
    /// The program as a native plan, or `None` where an instruction is not one native code runs.
    fn native_plan(&self) -> Option<NativePlan> {
        let insts = self.instructions.iter().map(|inst| self.native_inst(*inst)).collect::<Option<Vec<_>>>()?;
        Some(NativePlan { insts, entry: self.entry.unwrap_or(0), registers: self.register_count, unset: super::UNSET })
    }

    fn native_inst(&self, inst: Inst) -> Option<NativeInst> {
        let sets = &self.sets;
        Some(match inst {
            Inst::Unit(test) => NativeInst::Unit(native_test(test, sets)),
            Inst::UnitRepeat { test, min, max, greedy } => NativeInst::UnitRepeat { test: native_test(test, sets), min, max, greedy },
            Inst::Assert(test) => NativeInst::Assert(native_assertion(test)),
            Inst::Split { alternative } => NativeInst::Split { alternative },
            Inst::GuardedSplit { alternative, guard } => {
                let guard = &self.guards[guard as usize];
                NativeInst::GuardedSplit { alternative, guard: NativeGuard { at_start: guard.at_start, at_end: guard.at_end, mask: guard.units.table.0 } }
            }
            Inst::Jump(target) => NativeInst::Jump(target),
            Inst::Match => NativeInst::Match,
            Inst::Dispatch { .. } | Inst::Backreference { .. } | Inst::Look { .. } | Inst::LookSucceed => return None,
            registers => self.native_registers_inst(registers),
        })
    }

    /// `native_inst` of the instructions that read and write registers.
    fn native_registers_inst(&self, inst: Inst) -> NativeInst {
        match inst {
            Inst::CaptureOpen { temporary, restores } => NativeInst::CaptureOpen { temporary, restores },
            Inst::CaptureClose { capture, temporary } => NativeInst::CaptureClose { capture, temporary },
            Inst::ResetCaptures { first, count } => NativeInst::ResetCaptures { captures: self.lists[first as usize..(first + count) as usize].to_vec() },
            Inst::LoopInit { counter } => NativeInst::LoopInit { counter },
            Inst::LoopHead { counter, min, max, greedy, exit } => NativeInst::LoopHead { counter, min, max, greedy, exit },
            Inst::LoopIterate { start } => NativeInst::LoopIterate { start },
            Inst::EmptyCheck { start } => NativeInst::EmptyCheck { start },
            Inst::LoopTail { counter, start, min, head } => NativeInst::LoopTail { counter, start, min, head },
            _ => unreachable!("`native_inst` takes the others"),
        }
    }

    /// The native code for this program, asked of the embedder's compiler once the program has
    /// run `NativeCompiler::runs` searches; `None` while it has none.
    #[inline(always)]
    fn native_code(&self) -> Option<&NativeCode> {
        if let Some(code) = self.native.code.get() {
            return code.as_ref();
        }
        self.native_counted()
    }

    #[inline(never)]
    fn native_counted(&self) -> Option<&NativeCode> {
        let compiler = COMPILER.get().filter(|_| ENABLED.get())?;
        let runs = self.native.runs.get().saturating_add(1);
        self.native.runs.set(runs);
        if self.native.next_request.get() == 0 {
            self.native.next_request.set(compiler.first_request);
        }
        if runs < self.native.next_request.get() {
            return None;
        }
        let mut building = self.native.building.borrow_mut();
        if building.is_none() {
            let Some(plan) = self.native_plan() else {
                return self.native.code.get_or_init(|| None).as_ref();
            };
            *building = Some((compiler.start)(plan, runs));
        } else if runs % POLL_EVERY != 0 {
            return None;
        }
        let Some(Poll::Ready(outcome)) = building.as_mut().map(|build| build()) else {
            return None;
        };
        *building = None;
        match outcome {
            NativeOutcome::Code(code) => self.native.code.get_or_init(|| Some(code)).as_ref(),
            NativeOutcome::Never => self.native.code.get_or_init(|| None).as_ref(),
            NativeOutcome::Later => {
                self.native.next_request.set(runs.saturating_mul(REQUEST_GROWTH));
                None
            }
        }
    }

    /// `run`'s search of an ASCII subject from the search start `start` in native code: the
    /// match as `(start, end)`, `Some(None)` where there is none. `None` where the program has
    /// no native code or a search outgrew the stack, which the interpreter then runs from
    /// `start`, the registers back at `UNSET`.
    pub(super) fn run_native<T: Text + ?Sized>(
        &self,
        text: &T,
        mut start: usize,
        search: Search,
        registers: &mut [usize],
        stack: &mut Vec<usize>,
    ) -> Option<Option<(usize, usize)>> {
        let bytes = text.bytes()?;
        let code = self.native_code()?;
        loop {
            let end = self.native_attempt(code, bytes, start, registers, stack)?;
            if end != NATIVE_FAIL {
                return Some(Some((start, end)));
            }
            start = match self.next_start(text, start + NEXT_START, search.last) {
                Some(next) => next,
                None => return Some(None),
            };
        }
    }

    /// One native run from the search start `start`, on a stack grown as far as
    /// `MAXIMUM_STACK_FRAMES` allows; `None` where that is not enough.
    fn native_attempt(&self, code: &NativeCode, bytes: &[u8], start: usize, registers: &mut [usize], stack: &mut Vec<usize>) -> Option<usize> {
        let position = start + self.entry.unwrap_or(0) as usize;
        if stack.len() < INITIAL_STACK_FRAMES * NATIVE_FRAME_WORDS {
            stack.resize(INITIAL_STACK_FRAMES * NATIVE_FRAME_WORDS, 0);
        }
        loop {
            let limit = stack.as_mut_ptr().wrapping_add(stack.len());
            // SAFETY: the code reads `bytes` below `length`, writes `registers` below the
            // program's register count and its frames between `stack` and `limit`.
            let end = unsafe { (code.entry)(bytes.as_ptr(), bytes.len(), position, registers.as_mut_ptr(), stack.as_mut_ptr(), limit) };
            if end != NATIVE_OVERFLOW {
                return Some(end);
            }
            registers.fill(super::UNSET);
            if stack.len() >= MAXIMUM_STACK_FRAMES * NATIVE_FRAME_WORDS {
                return None;
            }
            stack.resize(stack.len() * STACK_GROWTH, 0);
        }
    }
}
