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
    /// The inlining budget halvings its last plan needed to fit (`PendingLaneMethod::shrinks`).
    shrinks: u8,
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

/// Past its last recompile, a body any run of which completed keeps its code and any other
/// retires: a run that leaves after a long native stretch still paid for its entry. Before it, a body that
/// left only where its predecessors did gained nothing from their feedback: it settles where
/// its runs mostly complete, and retires otherwise, as one that left from many places and
/// never completed does. Any other takes the
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
            Verdict::Recompile(stuck) => {
                // A body no run of which completed was a wasted compile, whatever comes next,
                // but one whose runs all left at its entry contract proved only that: its
                // replan drops the contract (`LaneExit::Entry`).
                let entry_only = leaves_at.get().exits().all(|exit| exit == LaneExit::Entry);
                if !completed.get() && !entry_only { self.note_wasted_compile(); }
                LaneBody::Unspecialized { entries: 0, recompiles: *recompiles + 1, stuck }
            }
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

    /// A compile none of whose runs completed counts toward `TIERS.lane_body.max_wasted`.
    fn note_wasted_compile(&self) {
        let history = self.history.get();
        self.history.set(LaneBodyHistory { wasted: history.wasted.saturating_add(1), ..history });
    }

    /// A compile a lifecycle edge outran counts as a dropped body, never a wasted one: no run
    /// of it took place.
    pub(super) fn note_dropped_compile(&self) {
        let history = self.history.get();
        self.history.set(LaneBodyHistory { lifetimes: history.lifetimes.saturating_add(1), ..history });
    }

    /// A specialized body that went at a lifecycle edge warms again from the start, keeping
    /// where it and its predecessors left, and its recompiles, as its successor's feedback: a call site's behavior
    /// outlives the shape ids the edge renumbered. One no run of which completed counts as
    /// wasted (`LaneBodyHistory`).
    pub(super) fn drop_body(&self, body: &mut LaneBody) {
        let LaneBody::Specialized { completed, leaves_at, stuck, recompiles, .. } = body else { return };
        let (completed, stuck, recompiles) = (completed.get(), stuck.union(leaves_at.get()), *recompiles);
        let history = self.history.get();
        self.history.set(LaneBodyHistory {
            lifetimes: history.lifetimes.saturating_add(1),
            wasted: history.wasted + u8::from(!completed),
            ..history
        });
        *body = LaneBody::Unspecialized { entries: 0, recompiles, stuck };
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

    /// Counts an entry toward the frameless body and specializes it at the warm-up; whether
    /// the body is still warming or specialized, so its prologue stays. A specialized body is
    /// a fact of the view from then on: its prologue record enters it with `entry`, which
    /// tests nothing the specialization proved (`lane_body_entry`).
    pub(super) fn specialize_lane_body(&self, entry: Handler, work: impl FnOnce(super::super::compiled::LaneBodyWork) -> super::super::compiled::LaneBodyOutcome) -> bool {
        self.settle_retired_body();
        // SAFETY: as `lane_body`; specializing runs no lane body, and the body is taken out
        // of the view while the VM works on it.
        let body = unsafe { &mut *self.tier_lane.get() };
        if awaits_look(body) {
            return true;
        }
        let (outcome, recompiles, stuck) = match std::mem::replace(body, LaneBody::Declined) {
            LaneBody::Unspecialized { entries, recompiles, stuck } if entries.saturating_add(1) >= self.warmup_entries() => {
                if !self.affords_compile() { return false; }
                // A predecessor's few leaves are the specialization's feedback (`stuck`).
                let generic: Vec<_> = stuck.exits().collect();
                (work(super::super::compiled::LaneBodyWork::Start { generic: &generic, shrinks: self.history.get().shrinks }), recompiles, stuck)
            }
            LaneBody::Unspecialized { entries, recompiles, stuck } => {
                *body = LaneBody::Unspecialized { entries: entries + 1, recompiles, stuck };
                return true;
            }
            LaneBody::Compiling { pending, recompiles, stuck, .. } =>
                (work(super::super::compiled::LaneBodyWork::Poll(pending)), recompiles, stuck),
            settled => { *body = settled; return !matches!(body, LaneBody::Declined); }
        };
        *body = self.body_after(outcome, entry, recompiles, stuck);
        !matches!(body, LaneBody::Declined)
    }

    /// The body a view's compile work came to (`LaneBodyOutcome`) makes of it; a compile in
    /// flight records the inlining its plan fits (`LaneBodyHistory::shrinks`).
    fn body_after(&self, outcome: super::super::compiled::LaneBodyOutcome, entry: Handler, recompiles: u8, stuck: LaneLeaves) -> LaneBody {
        match outcome {
            super::super::compiled::LaneBodyOutcome::Compiling(pending) => {
                self.history.set(LaneBodyHistory { shrinks: pending.shrinks(), ..self.history.get() });
                LaneBody::Compiling { pending, wait: std::cell::Cell::new(TIERS.lane_body.compile_poll_interval), recompiles, stuck }
            }
            super::super::compiled::LaneBodyOutcome::Published(published) => self.specialized(entry, published, recompiles, stuck),
            // A compile a collection or a changed fact outran is a body a lifecycle edge dropped.
            super::super::compiled::LaneBodyOutcome::Stale => {
                self.note_dropped_compile();
                LaneBody::Unspecialized { entries: 0, recompiles, stuck }
            }
            super::super::compiled::LaneBodyOutcome::Declined => LaneBody::Declined,
        }
    }

    /// `specialize_lane_body` at once, past the warm-up: a hot loop's backward jump has run
    /// its calls, so their sites have linked (`lane_loop_body_or_retire`).
    pub(super) fn specialize_lane_body_now(&self, entry: Handler, specialize: impl FnOnce(super::super::compiled::LaneBodyWork) -> super::super::compiled::LaneBodyOutcome) {
        // SAFETY: as `lane_body`; specializing runs no lane body.
        // A function whose bodies lifecycle edges discarded warms as any entry does.
        if let LaneBody::Unspecialized { entries, .. } = unsafe { &mut *self.tier_lane.get() }
            && self.history.get().lifetimes == 0
        {
            *entries = TIERS.lane_body.warmup_entries - 1;
        }
        self.specialize_lane_body(entry, specialize);
    }
}

/// Whether a compiling `body` lets this entry pass without looking for its code, counting it
/// down: a compile takes milliseconds and a hot loop enters at each backward jump.
fn awaits_look(body: &LaneBody) -> bool {
    let LaneBody::Compiling { wait, .. } = body else { return false };
    let left = wait.get();
    wait.set(left.saturating_sub(1));
    left > 0
}
