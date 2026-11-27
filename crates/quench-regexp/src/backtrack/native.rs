//! A hot program's search over an ASCII subject as native code: the program as plain data for
//! a compiler the embedder registers (`set_native_compiler`), and the entry it returns, which
//! runs the same steps as `Program::run` (so the interpreter stays the one source of meaning)
//! on a stack of its own. A search the native code cannot finish (its stack is full) is run
//! again by the interpreter from the same start.
use super::dispatch::Interval;
use super::matcher::Admission;
use super::{canonical, Inst, Program, Search, StringClass, Text, UnitTest, ASCII_UNITS, MAXIMUM_STACK_FRAMES};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::task::Poll;
use std::sync::OnceLock;

/// A program for a native compiler: its own instructions and the static tables they name,
/// owned, so the compile runs on another thread. The search of an ASCII subject begins at
/// `entry` with the position `start + entry` (`Program::entry`).
pub struct NativePlan {
    pub insts: Box<[Inst]>,
    /// The ASCII units of each set an instruction tests (`UnitTest::Set` indexes it), bit `n`
    /// of the 128-bit mask (low word first); the start set's last (`NativeRestarts`).
    pub masks: Box<[[u64; 2]]>,
    /// The capture and branch lists the instructions name (`Program::lists`).
    pub lists: Box<[u32]>,
    pub guards: Box<[Admission<UnitTest>]>,
    /// Each dispatch table's intervals, sorted (`Inst::Dispatch`).
    pub dispatches: Box<[Box<[Interval]>]>,
    /// The program's classes of strings, which `native_class_ends` reads: an address the code
    /// embeds, valid while the program, which owns the code, lives.
    pub classes: usize,
    /// The ASCII units whose case partner (`unit ^ CASE_BIT`) is the same character under
    /// case folding, without and with `unicode` (`canonical`).
    pub folds: [[u64; 2]; 2],
    pub entry: u32,
    /// The registers a run uses; each holds `unset` between runs.
    pub registers: usize,
    pub unset: usize,
    /// How the code moves on from a start that failed (`Program::native_restarts`).
    pub restarts: NativeRestarts,
    /// The test of the leading atomic repeat whose units a failed start skips, and the unit
    /// past them (`Program::next_candidate`).
    pub restart_skip: Option<UnitTest>,
}

/// The bit an ASCII letter and its other case differ by.
pub const CASE_BIT: u32 = 0x20;

/// The ASCII units whose case partner (`unit ^ CASE_BIT`) has the same canonical form.
fn case_partners(unicode: bool) -> [u64; 2] {
    let mut mask = [0u64; 2];
    for unit in (0..ASCII_UNITS).filter(|unit| canonical(*unit, unicode) == canonical(unit ^ CASE_BIT, unicode)) {
        mask[(unit / u64::BITS) as usize] |= 1 << (unit % u64::BITS);
    }
    mask
}

/// `native_class_ends`' result where no match of the class is at the position.
pub const CLASS_NONE: usize = usize::MAX;
/// `native_class_ends`' result where the stack has no room for the shorter matches.
pub const CLASS_OVERFLOW: usize = usize::MAX - 1;
/// A packed result's count of shorter matches is above this many bits.
pub const CLASS_COUNT_SHIFT: u32 = u32::BITS;

/// `Inst::ClassWidths` for native code (`Machine::class_ends`): the matches of class
/// `class` (`backward` in bit 0 of `class_and_backward`, the class above it) of the ASCII
/// subject `bytes[..length]` at `position`. The shorter ones are pushed as choice frames
/// `(choice, end, end)` from `top`, below `stack_end`, in the order the interpreter pushes
/// them. Returns `CLASS_NONE`, `CLASS_OVERFLOW`, or the frames pushed above
/// `CLASS_COUNT_SHIFT` and the longest match's end below.
///
/// # Safety
/// `classes` is `NativePlan::classes` of a live program, `class` one of its classes,
/// `bytes[..length]` readable, and `top..stack_end` writable words.
pub unsafe extern "C" fn native_class_ends(classes: usize, class_and_backward: usize, bytes: *const u8, length: usize, position: usize, top: *mut usize, stack_end: *mut usize, choice: usize) -> usize {
    // SAFETY: the caller's contract.
    let (class, subject) = unsafe { (&*(classes as *const StringClass).add(class_and_backward >> 1), std::slice::from_raw_parts(bytes, length)) };
    let ends = class.ends(subject, position, class_and_backward & 1 != 0);
    let Some((&first, rest)) = ends.split_first() else { return CLASS_NONE };
    let room = (stack_end as usize - top as usize) / (NATIVE_FRAME_WORDS * std::mem::size_of::<usize>());
    if rest.len() > room {
        return CLASS_OVERFLOW;
    }
    for (index, &end) in rest.iter().rev().enumerate() {
        // SAFETY: `room` frames fit from `top`.
        unsafe { top.add(index * NATIVE_FRAME_WORDS).cast::<[usize; NATIVE_FRAME_WORDS]>().write([choice, end, end]) };
    }
    rest.len() << CLASS_COUNT_SHIFT | first
}

/// Where a native search goes once no match starts at its start: back to its caller, which
/// finds the next start (a prefix or lookahead scan), or on itself, to the next position
/// past the start that `Admission` admits, as `Program::next_start` finds it, up to `last`:
/// one call per search, not per start.
#[derive(Clone, Copy)]
pub enum NativeRestarts {
    Caller,
    Every,
    /// `Admission::units` is the plan's last mask.
    Admitted(Admission<UnitTest>),
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
    /// The optimizing tier, where the mode has one above the first: the searches (counted from
    /// the program's first) after which its code replaces the first tier's, and its compiler.
    pub optimize: Option<(u32, fn(NativePlan) -> NativeBuild)>,
}

/// A compile in progress: polled for the code, or `Ready(None)` where it has none.
pub type NativeBuild = Box<dyn FnMut() -> Poll<Option<NativeCode>>>;

/// A building program asks its compile again on every this-many searches.
const POLL_EVERY: u32 = 64;

static COMPILER: OnceLock<NativeCompiler> = OnceLock::new();

thread_local! {
    /// Whether this thread's programs may compile (`enable_native_search`).
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    /// The native searches an ahead image holds (`register_ahead`), by pattern (`AheadKey`).
    static AHEAD: RefCell<HashMap<AheadKey, (NativeFn, Rc<dyn std::any::Any>)>> = RefCell::new(HashMap::new());
}

/// An ahead search's pattern: its normalized source (`Regex::source`), its flags (`Flags::bits`), and
/// whether it is the capture-free bounds program (`CompleteMatcher::bounds`) or the search program.
pub type AheadKey = (Box<str>, u8, bool);

/// Registers an ahead image's native search of a pattern (`AheadKey`) on this thread: a program
/// of the pattern lowered from now on runs it from its first search, whatever the mode
/// (`Program::adopt_ahead`), as the code an image's other bodies are.
pub fn register_ahead(key: AheadKey, entry: NativeFn, keep: Rc<dyn std::any::Any>) {
    AHEAD.with_borrow_mut(|ahead| ahead.insert(key, (entry, keep)));
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
    /// The optimizing tier's code, which replaces `code` once it arrives.
    optimized: OnceCell<NativeCode>,
    /// The first tier's code stays: no optimizing tier, or its compile gave no code.
    settled: Cell<bool>,
}

/// The frames a native search starts with room for; it grows to `MAXIMUM_STACK_FRAMES`.
const INITIAL_STACK_FRAMES: usize = 1024;
/// A full stack grows by this factor.
const STACK_GROWTH: usize = 4;

impl Program {
    /// The program's native plan for an ahead image, compiled before any program lives: `None`
    /// where its code would name a live program's address (a class of strings, `NativePlan::classes`).
    pub(crate) fn ahead_plan(&self) -> Option<NativePlan> {
        self.classes.is_empty().then(|| self.native_plan())
    }

    /// Adopts the ahead search registered for this program's pattern (`register_ahead`), where
    /// there is one: the program's code from its first search, with no tier above it.
    pub(crate) fn adopt_ahead(&self, key: AheadKey) {
        let Some((entry, keep)) = AHEAD.with_borrow(|ahead| ahead.get(&key).map(|(entry, keep)| (*entry, Rc::clone(keep)))) else { return };
        if self.native.code.set(Some(NativeCode { entry, keep: Box::new(keep) })).is_ok() {
            self.native.settled.set(true);
        }
    }

    /// The program as a native plan: its instructions and the tables they name.
    fn native_plan(&self) -> NativePlan {
        let starts = self.facts.starts.as_ref().map(|starts| (starts.at_start, starts.at_end, starts.units.table.0));
        let masks = self.sets.iter().map(|set| set.table.0).chain(starts.map(|(_, _, units)| units)).collect();
        let restarts = self.native_restarts(starts.map(|(at_start, at_end, _)| Admission { at_start, at_end, units: UnitTest::Set(self.sets.len() as u32) }));
        NativePlan {
            insts: self.instructions.clone(),
            masks,
            lists: self.lists.clone(),
            guards: self.guards.clone(),
            dispatches: self.dispatches.iter().map(|dispatch| dispatch.intervals().into()).collect(),
            classes: self.classes.as_ptr() as usize,
            folds: [false, true].map(case_partners),
            entry: self.facts.entry.unwrap_or(0),
            registers: self.register_count,
            unset: super::UNSET,
            restarts,
            restart_skip: self.facts.restart_skip,
        }
    }

    /// How native code moves past a failed start: on itself where `next_start` admits a
    /// position by the start set alone (`starts`), which a test of one unit decides in place;
    /// through the caller where a prefix or lookahead scan finds it, or the pattern is anchored.
    fn native_restarts(&self, starts: Option<Admission<UnitTest>>) -> NativeRestarts {
        if self.restarts_through_caller() {
            return NativeRestarts::Caller;
        }
        match starts {
            None => NativeRestarts::Every,
            Some(admission) => NativeRestarts::Admitted(admission),
        }
    }

    /// Whether a scan finds the next start (a prefix, lookahead or inner literal), or the
    /// pattern is anchored: native code then gives a failed start back to its caller.
    fn restarts_through_caller(&self) -> bool {
        self.facts.anchored || self.facts.prefix.is_some() || self.facts.lookahead.is_some() || self.facts.inner_literal.is_some()
    }

    /// The native code for this program, asked of the embedder's compiler once the program has
    /// run `NativeCompiler::runs` searches; `None` while it has none.
    #[inline(always)]
    fn native_code(&self) -> Option<&NativeCode> {

        if let Some(code) = self.native.code.get() {
            if let Some(optimized) = self.native.optimized.get() {
                return Some(optimized);
            }
            if self.native.settled.get() {
                return code.as_ref();
            }
            return self.native_optimizing(code.as_ref());
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
            *building = Some((compiler.start)(self.native_plan()));
        } else if runs % POLL_EVERY != 0 {
            return None;
        }
        let Some(Poll::Ready(code)) = building.as_mut().map(|build| build()) else {
            return None;
        };
        *building = None;
        // A program with no first-tier code has no tier above it either.
        self.native.settled.set(code.is_none() || compiler.optimize.is_none());
        self.native.code.get_or_init(|| code).as_ref()
    }

    /// The first tier's `code` while the optimizing tier counts the program's searches toward
    /// its compile, then compiles it, polled as the first tier's compile is.
    #[inline(never)]
    fn native_optimizing<'a>(&'a self, code: Option<&'a NativeCode>) -> Option<&'a NativeCode> {
        let Some((threshold, start)) = COMPILER.get().and_then(|compiler| compiler.optimize) else {
            self.native.settled.set(true);
            return code;
        };
        let runs = self.native.runs.get().saturating_add(1);
        self.native.runs.set(runs);
        if runs < threshold {
            return code;
        }
        let mut building = self.native.building.borrow_mut();
        if building.is_none() {
            *building = Some(start(self.native_plan()));
        } else if runs % POLL_EVERY != 0 {
            return code;
        }
        let Some(Poll::Ready(optimized)) = building.as_mut().map(|build| build()) else {
            return code;
        };
        *building = None;
        match optimized {
            Some(optimized) => Some(self.native.optimized.get_or_init(|| optimized)),
            None => {
                self.native.settled.set(true);
                code
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
        // Code that moves on by itself searches every start up to `last` in one call.
        let last = if self.restarts_through_caller() { None } else { Some(search.last) };
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
        let offset = self.facts.entry.unwrap_or(0) as usize;
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
