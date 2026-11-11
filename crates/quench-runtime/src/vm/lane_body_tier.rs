//! A lane view's body tier (`LaneView::tier_lane`): the `Tier` that counts the function's
//! entries, compiles its frameless body, and judges the body's runs. The view keeps what the
//! lifecycle cannot: the prologue record the body's entry takes over, and the bodies a
//! respecialization dropped, which the direct callers that name them still run.

use super::{Handler, LaneFeedback, LaneLeaves, LaneView, TIERS};
use super::super::compiled::tier_policy::{SPECULATIVE_BODIES, Tiers};
use super::super::compiled::{LaneBodyOutcome, LaneBodyWork, LaneExit, PendingLaneMethod, PublishedLaneMethod};
use super::super::tier_state::{Build, Controller, Event, Past, Polled, Refusal, Tier, Verdict, Why};
use std::rc::Rc;

/// The lane body tier's controller. A speculative body plans where its predecessors' runs left
/// generic; the generic body leaves at every record.
pub(super) struct LaneController;

/// How a lane body's runs went: those that completed, those that left and where, and whether
/// any completed at all.
#[derive(Default)]
pub(super) struct LaneRuns {
    /// Its runs that completed since it specialized: the leaves are judged against them.
    completions: u32,
    /// Its leaves since it specialized, toward `TIERS.lane_body.reoptimize_exits` and
    /// `TIERS.lane_body.reoptimize_share`, and where they went.
    exits: u16,
    leaves: LaneLeaves,
    /// Whether a run of it completed, natively called ones included.
    completed: bool,
}

/// A published body its function's prologue record enters (`lane_body_entry`). Dropping it gives
/// the prologue its earlier handler back, and keeps a body a lifecycle edge did not retire for
/// the direct callers that still run it, until they retire with it at a shape compaction.
pub(super) struct LaneCode {
    pub(super) method: Rc<PublishedLaneMethod>,
    /// The view whose tier owns this code, which outlives it.
    view: *const LaneView,
    /// The handler the prologue record had before the body took it.
    tier_prologue: *const (),
}

impl Drop for LaneCode {
    fn drop(&mut self) {
        // SAFETY: the view owns the tier that owns this code; no handler reads the prologue now.
        let view = unsafe { &*self.view };
        unsafe { (*view.prologue.get()).handler = self.tier_prologue };
        if self.method.is_published() {
            view.retired.borrow_mut().push(self.method.clone());
        }
    }
}

/// The entries and the program's warming clock past which a function specializes by its
/// recurrence alone (`LaneController::recurrent`).
const RECURRENT_ENTRIES: u32 = 16;
const RECURRENT_TICKS: u32 = 256;
/// Percent: the share a body's leaves may reach.
const PERCENT: u64 = 100;
/// Doublings of the warm-up a function's dropped bodies may cause.
const MAX_BACKOFF: u8 = 7;
/// Why a function whose body never completes a run is run by the lane.
const NEVER_COMPLETES: &str = "lane-body-never-completes";

/// The warming entries every function has counted so far: a clock of the work the program does
/// while its functions warm, which, unlike wall time, a rerun repeats.
fn warm_ticks() -> u32 {
    thread_local! { static TICKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }
    TICKS.with(|ticks| { let next = ticks.get().wrapping_add(1); ticks.set(next); next })
}

/// The reading `warm_ticks` would give next, without counting.
fn warm_clock() -> u32 { warm_ticks().wrapping_sub(1) }

impl LaneController {
    /// Whether a function entered rarely has still been entered at least `RECURRENT_ENTRIES` times
    /// over `RECURRENT_TICKS` warming entries of the program (`since` the first): an entry that
    /// runs long (a driver whose callees are hot) costs more than the many cheap entries the
    /// warm-up counts, and its callees' bodies run from the lane until it specializes and
    /// inlines them. Read every `RECURRENT_ENTRIES`th entry.
    fn recurrent(entries: u32, since: u32, past: &Past<LaneFeedback>) -> bool {
        entries >= RECURRENT_ENTRIES && entries % RECURRENT_ENTRIES == 0 && past.refused == 0
            && warm_clock().wrapping_sub(since) >= RECURRENT_TICKS
    }

    /// Whether `entries` is a count at which its function weighs a compile: the warm-up, then
    /// each doubling of it, so a function that has not paid for its compile yet looks again
    /// when it has done twice the work.
    fn attempts_at(policy: &Tiers, entries: u32, past: &Past<LaneFeedback>) -> bool {
        let warmup = Self::warmup(policy, past).max(1);
        entries % warmup == 0 && (entries / warmup).is_power_of_two()
    }

    /// Whether leaves at `leaves` would plan the same body again: only where the predecessors'
    /// bodies left, and those places already generic.
    fn nothing_new(leaves: LaneLeaves, stuck: LaneLeaves) -> bool {
        leaves != LaneLeaves::Scattered && stuck != LaneLeaves::Scattered
            && leaves.exits().all(|exit| stuck.exits().any(|known| known == exit))
    }
}

impl Controller for LaneController {
    type Pending = PendingLaneMethod;
    type Artifact = LaneCode;
    type Feedback = LaneFeedback;
    type Stats = LaneRuns;
    type Leave = LaneExit;

    /// A warm-up keeps the program's warming clock at its first entry.
    fn begin(_: bool) -> u32 {
        warm_ticks()
    }

    fn planned(pending: &PendingLaneMethod, feedback: &mut LaneFeedback) {
        feedback.shrinks = pending.shrinks();
    }

    /// The entries before the next body specializes: the warm-up, doubled per dropped body.
    fn warmup(policy: &Tiers, past: &Past<LaneFeedback>) -> u32 {
        u32::from(policy.lane_body.warmup_entries) << past.refused.min(MAX_BACKOFF)
    }

    /// Each entry that does not end the warm-up ticks the program's clock.
    fn ready(policy: &Tiers, entries: u32, since: u32, past: &Past<LaneFeedback>) -> bool {
        let ready = Self::attempts_at(policy, entries, past) || Self::recurrent(entries, since, past);
        if !ready { warm_ticks(); }
        ready
    }

    /// A body whose leaves reach the reoptimization threshold (and exceed `reoptimize_share`
    /// percent of its runs) is replaced by one that plans those places generic, until the
    /// speculative bodies are spent; one whose leaves would plan the same body stays, unless no
    /// run of it ever completed, which gains nothing from its entry: its function runs in the
    /// lane again.
    fn judge(policy: &Tiers, stats: &mut LaneRuns, past: &Past<LaneFeedback>, exit: LaneExit) -> Verdict<LaneFeedback> {
        stats.exits = stats.exits.saturating_add(1);
        stats.leaves = stats.leaves.with(exit);
        let (leaves, runs) = (u64::from(stats.exits), u64::from(stats.exits) + u64::from(stats.completions));
        if stats.exits < policy.lane_body.reoptimize_exits || leaves * PERCENT <= runs * u64::from(policy.lane_body.reoptimize_share) {
            return Verdict::Keep;
        }
        let stuck = past.feedback.stuck;
        if past.bodies > SPECULATIVE_BODIES || (past.bodies > 1 && Self::nothing_new(stats.leaves, stuck)) {
            return if stats.completed { Verdict::Keep } else { Verdict::Interpret(Refusal::Shape(NEVER_COMPLETES)) };
        }
        match stuck.union(stats.leaves) {
            LaneLeaves::Scattered => Verdict::Generalize,
            stuck => Verdict::Respecialize(LaneFeedback { stuck, ..past.feedback }),
        }
    }

    fn completed(stats: &mut LaneRuns) {
        stats.completed = true;
        stats.completions = stats.completions.saturating_add(1);
    }

    /// Where a dropped body's runs left joins where its predecessors' did.
    fn learned(stats: &LaneRuns, feedback: &LaneFeedback) -> LaneFeedback {
        LaneFeedback { stuck: feedback.stuck.union(stats.leaves), ..*feedback }
    }
}

/// A lane view's body tier.
pub(super) type LaneTier = Tier<LaneController>;

impl LaneView {
    /// The body tier, which only the view's own methods read: the lane runs on its VM's thread, a
    /// body is taken out of reach while the VM works on it, and none of them names the tier again.
    #[inline(always)]
    pub(super) fn lane_tier(&self) -> &mut LaneTier {
        // SAFETY: as above.
        unsafe { &mut *self.tier_lane.get() }
    }

    /// Counts how a run of the specialized body ended, at `exit`: a completed run is counted
    /// as one; any other leave is judged, and a body it condemns goes (`LaneController::judge`).
    pub(super) fn note_lane_body_end(&self, exit: LaneExit, completed_region: bool) {
        let tier = self.lane_tier();
        if completed_region || matches!(exit, LaneExit::Returned { .. }) {
            tier.complete();
        } else if tier.leave(exit, &TIERS) == Why::Unprofitable {
            self.release_from_tier();
        }
    }

    /// A compile a lifecycle edge outran, or a body it retired, is dropped by the tier.
    pub(super) fn drop_body(&self) {
        self.lane_tier().apply(Event::INVALIDATED, &TIERS);
    }

    /// The body a just published method makes of this view: its prologue record enters it with
    /// `entry`, which tests nothing the specialization proved (`lane_body_entry`). A body whose
    /// entry would not pay for itself serves native callers only (`lane_body_call`, direct
    /// calls); the lane's own entries keep the records, and it has no trial to run.
    fn code(&self, entry: Handler, method: Rc<PublishedLaneMethod>) -> LaneCode {
        let tier_prologue = if method.lease().expect("a body just published is live").amortizes_entry() {
            // SAFETY: as `claim_for_tier`; no handler reads the prologue now.
            unsafe { std::mem::replace(&mut (*self.prologue.get()).handler, entry as *const ()) }
        } else {
            self.release_from_tier();
            unsafe { (*self.prologue.get()).handler }
        };
        LaneCode { method, view: self, tier_prologue }
    }

    /// What the work for the body (`outcome`) did to it, as an event.
    fn event_of(&self, entry: Handler, outcome: LaneBodyOutcome) -> Event<LaneController> {
        match outcome {
            LaneBodyOutcome::Compiling(pending) => Event::Started(Build::new(pending, TIERS.lane_body.compile_poll_interval)),
            LaneBodyOutcome::Published(method) => Event::Compiled(self.code(entry, method)),
            // A compile a collection or a changed fact outran is a body a lifecycle edge dropped.
            LaneBodyOutcome::Stale => Event::STALE,
            // A compile the cost model puts off waits for the next look at its function.
            LaneBodyOutcome::Deferred => Event::DEFERRED,
            LaneBodyOutcome::Declined => Event::Failed(Refusal::Codegen),
        }
    }

    /// Counts an entry toward the frameless body and specializes it at the warm-up, for a
    /// function of `length` instructions; whether the body is still warming or specialized, so
    /// its prologue stays. A specialized body is a fact of the view from then on
    /// (`lane_body_entry`).
    pub(super) fn specialize_lane_body(&self, entry: Handler, length: usize, work: impl FnOnce(LaneBodyWork) -> LaneBodyOutcome) -> bool {
        self.settle_retired_body();
        let tier = self.lane_tier();
        match tier {
            Tier::Compiling { .. } => {
                tier.poll_build(TIERS.lane_body.compile_poll_interval, |pending| match work(LaneBodyWork::Poll(pending)) {
                    LaneBodyOutcome::Compiling(pending) => Polled::Pending(pending),
                    LaneBodyOutcome::Published(method) => Polled::Ready(self.code(entry, method)),
                    LaneBodyOutcome::Stale | LaneBodyOutcome::Deferred => Polled::Stale,
                    LaneBodyOutcome::Declined => Polled::Failed,
                });
            }
            Tier::Cold | Tier::Warming { .. } | Tier::Queued { .. } => {
                if tier.count(false, &TIERS) { return true; }
                // A predecessor's few leaves are the specialization's feedback.
                let shrinks = tier.past().map_or(0, |past| past.feedback.shrinks);
                let generic = super::generic_exits(tier.past(), length);
                // The entries a look at a doubled warm-up weighs, for the cost model.
                let observed = match tier { Tier::Warming { entries, past, .. } if LaneController::attempts_at(&TIERS, *entries + 1, past) => Some(*entries + 1), _ => None };
                tier.apply(self.event_of(entry, work(LaneBodyWork::Start { generic: &generic, shrinks, observed })), &TIERS);
            }
            Tier::Speculative { .. } | Tier::Generic { .. } | Tier::GenericInterpreted { .. } => {}
        }
        !tier.is_interpreted()
    }

    /// `specialize_lane_body` at once, past the warm-up: a hot loop's backward jump has run
    /// its calls, so their sites have linked (`lane_loop_body_or_retire`). A function whose
    /// bodies lifecycle edges discarded warms as any entry does.
    pub(super) fn specialize_lane_body_now(&self, entry: Handler, length: usize, specialize: impl FnOnce(LaneBodyWork) -> LaneBodyOutcome) {
        let tier = self.lane_tier();
        if tier.past().is_none_or(|past| past.refused == 0) && tier.is_warming() {
            tier.apply(Event::DUE, &TIERS);
        }
        self.specialize_lane_body(entry, length, specialize);
    }
}
