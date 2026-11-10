//! The registers and stack a search runs in, shared by every program on a thread.
use super::{Frame, Found, Program, Search, Text, UNSET};
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
    /// match built from its registers by `captures`. The registers hold `UNSET` between
    /// searches, so only a larger program grows them, and a run leaves them for the next
    /// to reset.
    #[inline(always)]
    pub(super) fn run_in_workspace<T: Text + ?Sized>(
        &self,
        text: &T,
        start: usize,
        search: Search,
        captures: impl FnOnce(&[usize]) -> Vec<Option<Range<usize>>>,
    ) -> Option<Found> {
        let mut workspace = take_workspace();
        let Workspace { registers, stack } = &mut *workspace;
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
        return_workspace(workspace);
        found
    }
}
