//! The registers and stack a search runs in, shared by every program on a thread.
use super::{Frame, Found, Inst, Program, Search, Text, UnitTest, UNBOUNDED, UNSET};
use std::cell::Cell;
use std::ops::Range;

/// Registers and stack reused across searches.
#[derive(Default)]
struct Workspace {
    registers: Vec<usize>,
    stack: Vec<Frame>,
}

thread_local! {
    /// The registers and stack of the running search, parked here between searches. A search
    /// never starts another, so every program on a thread shares one workspace, sized by the
    /// largest search so far. A plain pointer, as a `RefCell<Vec>` pays a lazy-destructor
    /// state check and a borrow flag on each of a regexp benchmark's millions of searches.
    static WORKSPACE: Cell<Option<std::ptr::NonNull<Workspace>>> = const { Cell::new(None) };
}

/// The thread's parked workspace, or a new one where none is parked (the first search, or a
/// search started inside a search).
#[inline(always)]
fn take_workspace() -> Box<Workspace> {
    match WORKSPACE.get() {
        // SAFETY: a parked pointer came from `Box::into_raw` in `return_workspace`, and
        // taking it leaves `None`, so this box is its only owner.
        Some(parked) => {
            WORKSPACE.set(None);
            unsafe { Box::from_raw(parked.as_ptr()) }
        }
        None => Box::default(),
    }
}

#[inline(always)]
fn return_workspace(workspace: Box<Workspace>) {
    // A workspace already parked (a nested search finished first) is dropped.
    if WORKSPACE.get().is_none() {
        WORKSPACE.set(std::ptr::NonNull::new(Box::into_raw(workspace)));
    }
}

impl Program {
    /// The run of `search` from `start` in the thread's workspace, with the captures of a
    /// match built from its registers by `captures`.
    #[inline(always)]
    pub(super) fn run_in_workspace<T: Text + ?Sized>(
        &self,
        text: &T,
        start: usize,
        search: Search,
        captures: impl FnOnce(&[usize]) -> Vec<Option<Range<usize>>>,
    ) -> Option<Found> {
        let mut workspace = take_workspace();
        let found = self.run_in(&mut workspace, text, start, search, captures);
        return_workspace(workspace);
        found
    }

    /// `run_in_workspace` in a workspace the caller holds. The registers hold `UNSET`
    /// between runs, so only a larger program grows them, and each run resets them after.
    #[inline(always)]
    fn run_in<T: Text + ?Sized>(
        &self,
        workspace: &mut Workspace,
        text: &T,
        start: usize,
        search: Search,
        captures: impl FnOnce(&[usize]) -> Vec<Option<Range<usize>>>,
    ) -> Option<Found> {
        let Workspace { registers, stack } = workspace;
        if registers.len() < self.register_count {
            registers.resize(self.register_count, UNSET);
        }
        let registers = &mut registers[..self.register_count];
        stack.clear();
        stack.push(Frame::Barrier);
        let (pc, position) = self.entry(text, start);
        let ran = self.run(text, (pc, start, position), Some(search), registers, stack);
        let found = ran.map(|(start, end)| Found { start, end, captures: captures(registers) });
        registers.fill(UNSET);
        found
    }

    fn is_class_run(&self) -> bool {
        self.class_run.is_some()
    }

    /// The bounds of each match of a scan from `first`, in order, passed to `found`: each
    /// search starts where the one before ended, one unit on from an empty match. Every
    /// search runs in one workspace, taken once; a literal or class-run pattern needs none.

    pub(crate) fn find_all_bounds<T: Text + ?Sized>(&self, text: &T, first: usize, mut found: impl FnMut(Range<usize>)) {
        let mut workspace = ((self.literal_length.is_none() && !self.is_class_run()) || text.bytes().is_none()).then(take_workspace);
        let mut at = first;
        loop {
            let range = match &mut workspace {
                Some(workspace) => self.run_start(text, at, false).and_then(|(start, search)| {
                    self.run_in(workspace, text, start, search, |_| Vec::new()).map(|found| found.start..found.end)
                }),
                None => self.find_bounds(text, at, false),
            };
            let Some(range) = range else { break };
            at = if range.is_empty() { range.end + 1 } else { range.end };
            found(range);
        }
        if let Some(workspace) = workspace {
            return_workspace(workspace);
        }
    }
}

/// The unit test of a program that is one greedy repeat of a unit, at least once and with no
/// maximum, and nothing else (`\\s+`): an ASCII subject's match is the run of such units from
/// where the search finds the first (`Program::class_run_found`), with no run of the program.
pub(super) fn class_run_of(instructions: &[Inst], capture_count: usize) -> Option<UnitTest> {
    match instructions {
        [Inst::UnitRepeat { test, min: 1, max: UNBOUNDED, greedy: true }, Inst::Match] if capture_count == 0 => Some(*test),
        _ => None,
    }
}

impl Program {
    /// The match of a `class_run` pattern over an ASCII subject: `Some(None)` where no
    /// unit can start one, `Some(Some(found))` for the run of units from the first start, and
    /// `None` where the pattern or subject is another kind, or the start scan was wrong.
    #[inline(always)]
    pub(super) fn class_run_found<T: Text + ?Sized>(&self, text: &T, first: usize, sticky: bool) -> Option<Option<Found>> {
        let (test, Some(_)) = (self.class_run?, text.bytes()) else {
            return None;
        };
        let last = if sticky { first } else { text.len() };
        let Some(start) = self.next_start(text, first, last) else {
            return Some(None);
        };
        let end = self.run_end(test, text, start, text.len());
        (end > start).then(|| Some(Found { start, end, captures: Vec::new() }))
    }
}
