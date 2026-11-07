//! When a lane body compiles again, keeps its code or retires: decisions from what its runs
//! and its predecessors' did.

use super::{Handler, LaneBody, LaneLeaves, LaneView, TIERS};
use super::super::compiled::LaneExit;

/// A view's lane-body record over the run, which a lifecycle edge keeps: the bodies such
/// edges (a shape compaction, a dead native callee) dropped, and those of them no run of
/// which completed (`LaneView::drop_body`). Each dropped body doubles the next one's warm-up,
/// so a function whose bodies keep being discarded stops paying a compile for each;
/// `TIERS.lane_body.max_lifetimes` dropped bodies, or `max_wasted` wasted ones, decline it.
#[derive(Clone, Copy, Default)]
pub(super) struct LaneBodyHistory {
    pub(super) lifetimes: u8,
    wasted: u8,
}

/// What a body whose leaves reached the reoptimization threshold does next.
enum Verdict {
    /// It keeps its code, and counts on.
    Keep,
    /// It keeps its code and reoptimizes no more.
    Settle,
    /// It specializes again, these places generically.
    Recompile(LaneLeaves),
    Retire,
}

/// Past its last recompile, a body that completed keeps its code and one no run of which
/// completed retires. Before it, a body that left only where its predecessors did gained
/// nothing from their feedback: it settles where its runs complete, and retires where none
/// did, as one that left from many places and never completed does. Any other takes the
/// places it left at, and its predecessors', generically next (`LaneView::specialize_lane_body`).
fn verdict(recompiles: u8, completed: bool, leaves: LaneLeaves, stuck: LaneLeaves) -> Verdict {
    let repeated = leaves != LaneLeaves::NONE && leaves.within(&stuck);
    match (recompiles >= TIERS.lane_body.max_recompiles, completed) {
        (true, true) => Verdict::Keep,
        (true, false) => Verdict::Retire,
        (false, true) if repeated => Verdict::Settle,
        _ if repeated || (leaves == LaneLeaves::Scattered && !completed) => Verdict::Retire,
        _ => Verdict::Recompile(stuck.union(leaves)),
    }
}

impl LaneView {
    /// Counts how a run of the specialized body ended, at `exit`; at
    /// `TIERS.lane_body.reoptimize_exits` leaves, doubled per recompile, it takes its
    /// `verdict`. A body it compiles past keeps its code for the direct callers that name it.
    pub(super) fn note_lane_body_end(&self, exit: LaneExit, completed_region: bool) {
        // SAFETY: as `lane_body`; the run that ended has finished, and nothing reads its body.
        let body = unsafe { &mut *self.tier_lane.get() };
        let LaneBody::Specialized { exits, leaves_at, completed, recompiles, tier_prologue, stuck, .. } = body else { return };
        if completed_region || matches!(exit, LaneExit::Returned { .. }) {
            completed.set(true);
            return;
        }
        exits.set(exits.get().saturating_add(1));
        leaves_at.set(leaves_at.get().with(exit));
        if exits.get() < TIERS.lane_body.reoptimize_exits << *recompiles {
            return;
        }
        let next = match verdict(*recompiles, completed.get(), leaves_at.get(), *stuck) {
            Verdict::Keep => return,
            Verdict::Settle => return *recompiles = TIERS.lane_body.max_recompiles,
            Verdict::Retire => LaneBody::Declined,
            Verdict::Recompile(stuck) => LaneBody::Unspecialized { entries: 0, recompiles: *recompiles + 1, stuck },
        };
        // SAFETY: as `claim_for_tier`; the prologue's handler runs no more for this entry.
        unsafe { (*self.prologue.get()).handler = *tier_prologue };
        let retires = matches!(next, LaneBody::Declined);
        let LaneBody::Specialized { method, .. } = std::mem::replace(body, next) else { unreachable!() };
        if retires {
            *body = LaneBody::Retired { _method: method };
            self.release_from_tier();
        } else {
            self.retired.borrow_mut().push(method);
        }
    }

    /// The entries before its next body specializes: the warm-up, doubled per dropped body.
    pub(super) fn warmup_entries(&self) -> u16 {
        TIERS.lane_body.warmup_entries << self.history.get().lifetimes.min(7)
    }

    /// Whether its history affords another compile.
    pub(super) fn affords_compile(&self) -> bool {
        let history = self.history.get();
        history.lifetimes <= TIERS.lane_body.max_lifetimes && history.wasted < TIERS.lane_body.max_wasted
    }

    /// A compile a lifecycle edge outran counts as a dropped body, never a wasted one: no run
    /// of it took place.
    pub(super) fn note_dropped_compile(&self) {
        let history = self.history.get();
        self.history.set(LaneBodyHistory { lifetimes: history.lifetimes.saturating_add(1), ..history });
    }

    /// A body that went at a lifecycle edge warms again from the start; one no run of which
    /// `completed` counts as wasted (`LaneBodyHistory`).
    pub(super) fn drop_body(&self, body: &mut LaneBody, completed: bool) {
        let history = self.history.get();
        self.history.set(LaneBodyHistory {
            lifetimes: history.lifetimes.saturating_add(1),
            wasted: history.wasted + u8::from(!completed),
        });
        *body = LaneBody::Unspecialized { entries: 0, recompiles: 0, stuck: LaneLeaves::NONE };
    }

    /// The body a just published method makes of this view.
    pub(super) fn specialized(&self, entry: Handler, method: std::rc::Rc<super::super::compiled::PublishedLaneMethod>, recompiles: u8, stuck: LaneLeaves) -> LaneBody {
        // A body whose entry would not pay for itself serves native callers only
        // (`lane_body_call`, direct calls); the lane's own entries keep the records, and it
        // has no trial to run.
        let (tier_prologue, runs) = if method.lease().expect("a body just published is live").amortizes_entry() {
            // SAFETY: as `claim_for_tier`; no handler reads the prologue now.
            (unsafe { std::mem::replace(&mut (*self.prologue.get()).handler, entry as *const ()) }, 0)
        } else {
            self.release_from_tier();
            (unsafe { (*self.prologue.get()).handler }, TIERS.lane_body.trial_runs)
        };
        use std::cell::Cell;
        LaneBody::Specialized { method, tier_prologue, runs: Cell::new(runs), leaves: Cell::new(0), exits: Cell::new(0),
            leaves_at: Cell::new(LaneLeaves::NONE), completed: Cell::new(false), recompiles, stuck }
    }

    /// Whether its body's code is compiling (`LaneBody::Compiling`).
    pub(super) fn compiling(&self) -> bool {
        // SAFETY: as `lane_body`.
        matches!(unsafe { &*self.tier_lane.get() }, LaneBody::Compiling { .. })
    }
}
