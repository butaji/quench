//! What the tiers did in one VM: a flat table of counters, one per `Event`, kept always and
//! incremented on the paths that already do the work. Observation reads this record; nothing
//! here (or behind a diagnostics switch) decides what the tiers run.

use super::compiled::LaneExit;
use std::cell::Cell;

/// One kind of tier event. The ends of a lane body's run are contiguous, `Completed` first
/// (`Event::END_COUNT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A lane body published.
    Published,
    /// The calls a published body inlines, summed.
    Inlined,
    /// A lane body's run started (`Vm::run_lane_body`).
    Entered,
    /// A run completed its activation: a return or an exception.
    Completed,
    /// A run left at a caller PC, its entry included.
    LeftAt,
    /// A run left inside an inlined callee.
    LeftInlined,
    /// A run left in a native call's callee.
    LeftInCallee,
    /// A run was irreversibly terminated.
    Terminated,
    /// A body's entry the lane could not make the activations of a leave for (`lane_body_admits`).
    Refused,
    /// A lane body entered through its entry cell before any activation.
    CellCrossed,
    /// An entry that bound a different run into the lane frame (`LaneFrame::bind_run`).
    RunBound,
    /// A run that completed with its body's `Return`, as opposed to an exception.
    Returned,
    /// A loop site's run ran published code and continued past it or completed.
    OsrEntered,
    /// A loop site's run ran published code that left at an exit.
    OsrLeft,
    /// A body a leave retired: dropped for recompiling or declined.
    Retired,
}

impl Event {
    /// The kinds, in discriminant order.
    pub const ALL: [Event; Self::COUNT] = [
        Self::Published, Self::Inlined, Self::Entered, Self::Completed, Self::LeftAt, Self::LeftInlined,
        Self::LeftInCallee, Self::Terminated, Self::Refused, Self::CellCrossed, Self::RunBound,
        Self::Returned, Self::OsrEntered, Self::OsrLeft, Self::Retired,
    ];
    /// How many kinds there are.
    pub const COUNT: usize = Self::Retired as usize + 1;
    /// The kinds that end a run, `Completed` first.
    pub const END_COUNT: usize = Self::Terminated as usize - Self::Completed as usize + 1;

    /// How a run that ended at `exit` ended: completed, left at its own PC (its entry, or the
    /// stub of an entry cell that ran no body, included), inside an inlined call, into a
    /// callee, or terminated.
    pub(in crate::vm) fn ended(exit: LaneExit) -> Self {
        match exit {
            LaneExit::Returned { .. } | LaneExit::Threw { .. } => Self::Completed,
            LaneExit::At(_) | LaneExit::Entry | LaneExit::Stub => Self::LeftAt,
            LaneExit::Inlined { .. } => Self::LeftInlined,
            LaneExit::Callee => Self::LeftInCallee,
            LaneExit::Terminated => Self::Terminated,
        }
    }

    /// An end's place among the ends.
    pub(in crate::vm) fn end_index(self) -> usize { self as usize - Self::Completed as usize }
}

/// The counters of one VM, indexed by `Event`.
#[derive(Clone, Default)]
pub struct TierEvents([Cell<u64>; Event::COUNT]);

impl TierEvents {
    pub(in crate::vm) fn record(&self, event: Event) { self.add(event, 1); }

    pub(in crate::vm) fn add(&self, event: Event, count: usize) {
        let counter = &self.0[event as usize];
        counter.set(counter.get() + count as u64);
    }

    /// How many times `event` happened.
    pub fn count(&self, event: Event) -> u64 { self.0[event as usize].get() }

    /// Runs that left a body, whatever the reason.
    pub fn leaves(&self) -> u64 {
        [Event::LeftAt, Event::LeftInlined, Event::LeftInCallee, Event::Terminated].iter().map(|event| self.count(*event)).sum()
    }
}

impl std::fmt::Debug for TierEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(Event::ALL.iter().map(|event| (event, self.count(*event)))).finish()
    }
}

impl<H: super::Host> super::Vm<H> {
    /// What the tiers did in this VM, counted whatever diagnostics are on.
    pub fn tier_events(&self) -> &TierEvents { self.programs.tier_events() }
}
