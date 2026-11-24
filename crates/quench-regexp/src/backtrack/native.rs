//! A hot program's search over an ASCII subject as native code: the program as plain data for
//! a compiler the embedder registers (`set_native_compiler`), and the entry it returns, which
//! runs the same steps as `Program::run` (so the interpreter stays the one source of meaning)
//! on a stack of its own. A search the native code cannot finish (its stack is full) is run
//! again by the interpreter from the same start.
use super::matcher::{Admission, AssertionTest};
use super::{Inst, MatchSet, Program, Search, Text, UnitTest, ASCII_UNITS, MAXIMUM_STACK_FRAMES};
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

/// `Inst`, for the instructions a native search runs; `Dispatch`, `Backreference` and
/// lookarounds keep a program interpreted.
#[derive(Clone)]
pub enum NativeInst {
    Unit(NativeTest),
    /// `atomic`: a greedy repeat that keeps no frame (`possessive`).
    UnitRepeat { test: NativeTest, min: u32, max: u32, greedy: bool, atomic: bool },
    Assert(AssertionTest),
    Split { alternative: u32 },
    GuardedSplit { alternative: u32, guard: Admission<NativeTest> },
    Jump(u32),
    /// `count` consecutive temporaries from `temporary`.
    CaptureOpen { temporary: u32, count: u32, restores: bool },
    CaptureClose { capture: u32, temporary: u32 },
    /// The captures listed in `NativePlan::lists[first..first + count]`.
    ResetCaptures { first: u32, count: u32 },
    LoopInit { counter: u32 },
    LoopHead { counter: u32, min: u32, max: u32, greedy: bool, exit: u32 },
    LoopIterate { start: u32 },
    EmptyCheck { start: u32 },
    LoopTail { counter: u32, start: u32, min: u32, head: u32 },
    /// A lookahead whose body follows, ending at its `LookSucceed`; matching goes on at `end`.
    Look { negative: bool, end: u32 },
    LookSucceed,
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
    /// The capture lists the instructions name (`Program::lists`).
    pub lists: Box<[u32]>,
    /// How the code moves on from a start that failed (`Program::native_restarts`).
    pub restarts: NativeRestarts,
    /// The test of the leading atomic repeat whose units a failed start skips, and the unit
    /// past them (`Program::next_candidate`).
    pub restart_skip: Option<NativeTest>,
}

/// Where a native search goes once no match starts at its start: back to its caller, which
/// finds the next start (a prefix or lookahead scan), or on itself, to the next position
/// past the start that `Admission` admits, as `Program::next_start` finds it, up to `last`:
/// one call per search, not per start.
#[derive(Clone, Copy)]
pub enum NativeRestarts {
    Caller,
    Every,
    Admitted(Admission<NativeTest>),
}

/// `fn(bytes, length, position, registers, stack, stack_end, last, found)`: the end of the
/// match the program finds from `position`, its start in `*found`; `NATIVE_FAIL` where none
/// starts there or (`NativeRestarts` other than `Caller`) at any start up to `last` (the
/// registers are then as they were); or `NATIVE_OVERFLOW` where the stack of
/// `stack..stack_end` words held too few frames for the start in `*found` (the registers
/// are then unspecified).
pub type NativeFn = unsafe extern "C" fn(*const u8, usize, usize, *mut usize, *mut usize, *mut usize, usize, *mut usize) -> usize;
pub const NATIVE_FAIL: usize = usize::MAX;
pub const NATIVE_OVERFLOW: usize = usize::MAX - 1;
/// The units of an ASCII subject: a byte below this.
pub const NATIVE_ASCII_UNITS: u32 = ASCII_UNITS;
/// The words of one backtracking frame: its resume id and two operands.
pub const NATIVE_FRAME_WORDS: usize = 3;

/// A compiled search and what keeps its code alive.
pub struct NativeCode {
    pub entry: NativeFn,
    pub keep: Box<dyn std::any::Any>,
}

/// The embedder's compiler, and the searches a program runs before asking it for code.
#[derive(Clone, Copy)]
pub struct NativeCompiler {
    pub runs: u32,
    /// Starts compiling a plan, which the search polls for its code while the interpreter runs.
    pub start: fn(NativePlan) -> NativeBuild,
}

/// A compile in progress: polled for the code, or `Ready(None)` where it has none.
pub type NativeBuild = Box<dyn FnMut() -> Poll<Option<NativeCode>>>;

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
    code: OnceCell<Option<NativeCode>>,
    building: RefCell<Option<NativeBuild>>,
}

/// The frames a native search starts with room for; it grows to `MAXIMUM_STACK_FRAMES`.
const INITIAL_STACK_FRAMES: usize = 1024;
/// A full stack grows by this factor.
const STACK_GROWTH: usize = 4;

/// The loop instructions: counters, iteration starts and empty checks.
fn native_loop_inst(inst: Inst) -> NativeInst {
    match inst {
        Inst::LoopInit { counter } => NativeInst::LoopInit { counter },
        Inst::LoopHead { counter, min, max, greedy, exit } => NativeInst::LoopHead { counter, min, max, greedy, exit },
        Inst::LoopIterate { start } => NativeInst::LoopIterate { start },
        Inst::EmptyCheck { start } => NativeInst::EmptyCheck { start },
        Inst::LoopTail { counter, start, min, head } => NativeInst::LoopTail { counter, start, min, head },
        _ => unreachable!("`native_inst` takes the others"),
    }
}

fn native_test(test: UnitTest, sets: &[MatchSet]) -> NativeTest {
    match test {
        UnitTest::Exact(unit) if unit < ASCII_UNITS => NativeTest::Exact(unit as u8),
        UnitTest::Exact(_) => NativeTest::Never,
        UnitTest::Set(set) => NativeTest::Mask(sets[set as usize].table.0),
    }
}

impl Program {
    /// The program as a native plan, or `None` where an instruction is not one native code runs.
    fn native_plan(&self) -> Option<NativePlan> {
        let insts = self.instructions.iter().map(|inst| self.native_inst(*inst)).collect::<Option<Vec<_>>>()?;
        let restarts = self.native_restarts();
        let restart_skip = self.restart_skip.map(|test| native_test(test, &self.sets));
        Some(NativePlan { insts, entry: self.entry.unwrap_or(0), registers: self.register_count, unset: super::UNSET, lists: self.lists.clone(), restarts, restart_skip })
    }

    /// How native code moves past a failed start: on itself where `next_start` admits a
    /// position by the start set alone, which a test of one unit decides in place; through
    /// the caller where a prefix or lookahead scan finds it, or the pattern is anchored.
    fn native_restarts(&self) -> NativeRestarts {
        if self.anchored || self.prefix.is_some() || self.lookahead.is_some() || self.inner_literal.is_some() {
            return NativeRestarts::Caller;
        }
        match &self.starts {
            None => NativeRestarts::Every,
            Some(starts) => NativeRestarts::Admitted(Admission {
                at_start: starts.at_start,
                at_end: starts.at_end,
                units: NativeTest::Mask(starts.units.table.0),
            }),
        }
    }

    fn native_inst(&self, inst: Inst) -> Option<NativeInst> {
        let sets = &self.sets;
        Some(match inst {
            Inst::Unit(test) => NativeInst::Unit(native_test(test, sets)),
            Inst::UnitRepeat { test, min, max, greedy, atomic } => NativeInst::UnitRepeat { test: native_test(test, sets), min, max, greedy, atomic },
            Inst::Assert(test) => NativeInst::Assert(test),
            Inst::Split { alternative } => NativeInst::Split { alternative },
            Inst::GuardedSplit { alternative, guard } => {
                let Admission { at_start, at_end, units } = self.guards[guard as usize];
                NativeInst::GuardedSplit { alternative, guard: Admission { at_start, at_end, units: native_test(units, sets) } }
            }
            Inst::Jump(target) => NativeInst::Jump(target),
            Inst::Match => NativeInst::Match,
            Inst::Dispatch { .. }
            | Inst::Backreference { .. }
            | Inst::BackreferenceBack { .. }
            | Inst::UnitBack(_)
            | Inst::UnitRepeatBack { .. }
            | Inst::CaptureCloseBack { .. }
            | Inst::ClassWidths { .. } => return None,
            registers => self.native_registers_inst(registers),
        })
    }

    /// `native_inst` of the instructions that read and write registers.
    fn native_registers_inst(&self, inst: Inst) -> NativeInst {
        match inst {
            Inst::CaptureOpen { temporary, count, restores } => NativeInst::CaptureOpen { temporary, count, restores },
            Inst::CaptureClose { capture, temporary } => NativeInst::CaptureClose { capture, temporary },
            Inst::ResetCaptures { first, count } => NativeInst::ResetCaptures { first, count },
            // A lookbehind's body is backward instructions, which keep the program interpreted.
            Inst::Look { negative, end } => NativeInst::Look { negative, end },
            Inst::LookSucceed => NativeInst::LookSucceed,
            loops => native_loop_inst(loops),
        }
    }

    /// The native code for this program, asked of the embedder's compiler once the program has
    /// run `NativeCompiler::runs` searches; `None` while it has none.
    #[inline(always)]
    fn native_code(&self) -> Option<&NativeCode> {
        if let Some(code) = self.native.code.get() {
            return code.as_ref();
        }
        // An interpreted run compiles no native search: no call to count its searches.
        if !ENABLED.get() {
            return None;
        }
        self.native_counted()
    }

    #[inline(never)]
    fn native_counted(&self) -> Option<&NativeCode> {
        let compiler = COMPILER.get().filter(|_| ENABLED.get())?;
        let runs = self.native.runs.get().saturating_add(1);
        self.native.runs.set(runs);
        if runs < compiler.runs {
            return None;
        }
        let mut building = self.native.building.borrow_mut();
        if building.is_none() {
            let Some(plan) = self.native_plan() else {
                return self.native.code.get_or_init(|| None).as_ref();
            };
            *building = Some((compiler.start)(plan));
        } else if runs % POLL_EVERY != 0 {
            return None;
        }
        let Some(Poll::Ready(code)) = building.as_mut().map(|build| build()) else {
            return None;
        };
        *building = None;
        self.native.code.get_or_init(|| code).as_ref()
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
        // Code that moves on by itself searches every start up to `last` in one call.
        let last = if matches!(self.native_restarts(), NativeRestarts::Caller) { None } else { Some(search.last) };
        loop {
            let (found, end) = self.native_attempt(code, bytes, (start, last.unwrap_or(start)), registers, stack)?;
            if end != NATIVE_FAIL {
                return Some(Some((found, end)));
            }
            if last.is_some() {
                return Some(None);
            }
            start = match self.next_start(text, self.next_candidate(text, start), search.last) {
                Some(next) => next,
                None => return Some(None),
            };
        }
    }

    /// One native run from the search start `start` through `last`, on a stack grown as far
    /// as `MAXIMUM_STACK_FRAMES` allows: the start of its match and the match's end (or
    /// `NATIVE_FAIL`); `None` where the stack is not enough.
    fn native_attempt(&self, code: &NativeCode, bytes: &[u8], (mut start, last): (usize, usize), registers: &mut [usize], stack: &mut Vec<usize>) -> Option<(usize, usize)> {
        let offset = self.entry.unwrap_or(0) as usize;
        if stack.len() < INITIAL_STACK_FRAMES * NATIVE_FRAME_WORDS {
            stack.resize(INITIAL_STACK_FRAMES * NATIVE_FRAME_WORDS, 0);
        }
        loop {
            let limit = stack.as_mut_ptr().wrapping_add(stack.len());
            let mut found = start;
            // SAFETY: the code reads `bytes` below `length`, writes `registers` below the
            // program's register count, its frames between `stack` and `limit`, and `found`.
            let end = unsafe { (code.entry)(bytes.as_ptr(), bytes.len(), start + offset, registers.as_mut_ptr(), stack.as_mut_ptr(), limit, last, &raw mut found) };
            if end != NATIVE_OVERFLOW {
                return Some((found, end));
            }
            // The start whose run overflowed runs again on a larger stack.
            start = found;
            registers.fill(super::UNSET);
            if stack.len() >= MAXIMUM_STACK_FRAMES * NATIVE_FRAME_WORDS {
                return None;
            }
            stack.resize(stack.len() * STACK_GROWTH, 0);
        }
    }
}
