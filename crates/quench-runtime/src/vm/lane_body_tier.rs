//! When a lane body compiles again, keeps its code or retires: decisions from what its runs
//! and its predecessors' did.

use super::super::tier_machine::{TierEvent, TierState};
use super::{Handler, LaneBody, LaneLeaves, LaneView, Tiers};
use super::compiled::tier_policy::PERCENT;
use super::super::compiled::LaneExit;

/// A view's lane-body record over the run, which a lifecycle edge keeps: the bodies such
/// edges (a shape compaction, a dead native callee) dropped, and those of them no run of
/// which completed (`LaneView::drop_body`). Each dropped body doubles the next one's warm-up,
/// so a function whose bodies keep being discarded stops paying a compile for each;
/// `self.tiers.lane_body.max_lifetimes` dropped bodies, decline it.
#[derive(Clone, Copy, Default)]
pub(super) struct LaneBodyHistory {
    pub(super) lifetimes: u8,
    /// The inlining budget halvings its last plan needed to fit (`PendingLaneMethod::shrinks`).
    shrinks: u8,
    /// The bodies it specialized past because a callee published one (`respecialize_callers`).
    callee_recompiles: u8,
    late_calls: u32,
    /// Whether a compile of its body waited for its entries to pay for it (`LaneBodyOutcome::Unpaid`).
    unpaid: bool,
    /// Whether its body was declined for its length alone (`LaneBodyOutcome::Short`).
    short: bool,
    /// Whether a caller's compile asked for its body (`LaneView::demand_body`).
    demanded: bool,
    /// The timed runs of its current body and of the interpreter (`audit`).
    audit: audit::Audit,
}

#[path = "lane_body_audit.rs"]
mod audit;
pub(super) use audit::{Epochs, Verdict};

/// The entries and the milliseconds of warming past which a function specializes by its
/// recurrence alone (`LaneView::recurrent`).
const RECURRENT_ENTRIES: u32 = 16;
/// The completed handler runs of a body per credit to the callees it calls natively: a callee's
/// completions inside those runs are bounded by the caller's, so the credit lags by fewer than this.
const NATIVE_CREDIT_RUNS: u64 = 64;
const RECURRENT_TICKS: u32 = 256;
/// The most times a dropped body doubles the warm-up of the next.
const MAX_DOUBLINGS: u8 = 7;

/// The warming entries every function has counted so far: a clock of the work the program does
/// while its functions warm, which, unlike wall time, a rerun repeats.
fn warm_ticks() -> u32 {
    thread_local! { static TICKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }
    TICKS.with(|ticks| { let next = ticks.get().wrapping_add(1); ticks.set(next); next })
}

/// The reading `warm_ticks` would give next, without counting.
fn warm_clock() -> u32 { warm_ticks().wrapping_sub(1) }

/// What a body whose leaves reached the reoptimization threshold does next: it specializes
/// again, these places generically, until `max_recompiles` bodies have; the last of them has
/// every place any body left at generic, and is permanent. A body never retires to the lane.
fn verdict(tiers: &Tiers, completed: bool, recompiles: u8, leaves: LaneLeaves, stuck: LaneLeaves) -> Option<LaneLeaves> {
    // A body no run of which completed leaves at the places its recompile would too: only a
    // body that has run natively learns anything from its leaves.
    if !completed { return None; }
    // Leaves only where the predecessors' bodies left, and those places already generic,
    // would plan the same body again: it stays.
    let nothing_new = leaves != LaneLeaves::Scattered && stuck != LaneLeaves::Scattered
        && leaves.exits().all(|exit| stuck.exits().any(|known| known == exit));
    (recompiles < tiers.lane_body.max_recompiles && !(recompiles > 0 && nothing_new)).then(|| stuck.union(leaves))
}

/// Credits the callees `method`'s body calls natively with `NATIVE_CREDIT_RUNS` completions
/// (a bound, as the caller's completed runs need not reach each call).
fn credit_native_callees(method: &super::compiled::PublishedLaneMethod) {
    let Some(body) = method.lease() else { return };
    for callee in body.direct.iter() {
        // SAFETY: the caller's body owns the callee it named; the view outlives the run.
        unsafe { &*callee.target.view }.entry.note_completions(NATIVE_CREDIT_RUNS);
    }
}

/// The function a leave at `exit` is in (`None`: the body's own) and its PC, for a leave that
/// has an instruction.
fn leave_place(exit: LaneExit, method: &super::compiled::PublishedLaneMethod) -> Option<(Option<u32>, usize)> {
    match exit {
        LaneExit::At(pc) => Some((None, pc)),
        LaneExit::Inlined { frame, pc } => method.lease().and_then(|method| method.inlined.get(frame).map(|call| (Some(call.target.function), pc as usize))),
        _ => None,
    }
}

/// Names the place of a leave at `exit` among the body's, and hands a place it had not named to
/// `left_new` once. A scattered body names none.
fn name_leave(leaves_at: &std::cell::Cell<LaneLeaves>, exit: LaneExit, method: &super::compiled::PublishedLaneMethod, left_new: impl FnOnce((Option<u32>, usize))) {
    // SAFETY: the cell is read in place and nothing writes it meanwhile.
    let seen = match unsafe { &*leaves_at.as_ptr() } {
        LaneLeaves::Few(named) => named.contains(&Some(exit)),
        LaneLeaves::Scattered => true,
    };
    if seen { return; }
    leaves_at.set(leaves_at.get().with(exit));
    if leaves_at.get() != LaneLeaves::Scattered && let Some(place) = leave_place(exit, method) { left_new(place); }
}

impl LaneView {
    /// Counts how a run of the specialized body ended, at `exit`; at
    /// `self.tiers.lane_body.reoptimize_exits` leaves, doubled per recompile, it takes its
    /// `verdict`. A body it compiles past keeps its code for the direct callers that name it.
    /// Whether the run retired the body.
    pub(super) fn note_lane_body_end(&self, exit: LaneExit, completed_region: bool, left_new: impl FnOnce((Option<u32>, usize))) -> bool {
        // SAFETY: as `lane_body`; the run that ended has finished, and nothing reads its body.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        let LaneBody::Specialized { leaves_at, completed, recompiles, stuck, method, .. } = body else { return false };
        if completed_region || exit.completes() {
            completed.set(true);
            if self.entry.note_completions(1) % NATIVE_CREDIT_RUNS == 0 { credit_native_callees(method); }
            return false;
        }
        let exits = self.entry.note_leave();
        name_leave(leaves_at, exit, method, left_new);
        // Leaves past the first `reoptimize_exits` that exceed `reoptimize_share` percent of the runs.
        let (completions, _) = self.entry.evidence();
        let (leaves, runs) = (exits, exits + completions);
        if exits < u64::from(self.tiers.lane_body.reoptimize_exits) || leaves * PERCENT <= runs * u64::from(self.tiers.lane_body.reoptimize_share) {
            return false;
        }
        let Some(stuck) = verdict(self.tiers, completed.get(), *recompiles, leaves_at.get(), *stuck) else {
            // A body that keeps leaving where no other body would, and has never completed a
            // run, gains nothing from its entry: its function runs in the lane again.
            let costly = runs >= u64::from(self.tiers.lane_body.retire_after_runs) && leaves * PERCENT > runs * u64::from(self.tiers.lane_body.retire_share);
            if !completed.get() || costly {
                self.retire_body(body, LaneBody::Declined);
                self.release_from_tier();
                return true;
            }
            return false;
        };
        let next = LaneBody::Unspecialized { entries: 0, recompiles: *recompiles + 1, stuck };
        self.retire_body(body, next);
        true
    }

    /// Counts a leave inside this function's body that a native caller's run made, which no
    /// handler entry notes (`native_frames::advance_into_frames`): one add to the cell's word, on the leave.
    pub(super) fn note_native_leave(&self) { self.entry.note_leave(); }

    /// Replaces the specialized `body` with `next`: its entry's handler runs no more, its entry
    /// cell takes the stub, and its code waits for the callers that still name it.
    fn retire_body(&self, body: &mut LaneBody, next: LaneBody) {
        let body_state = body.tier_state();
        let LaneBody::Specialized { tier_prologue, .. } = body else { unreachable!() };
        let event = if matches!(next, LaneBody::Declined) { TierEvent::Decline } else { TierEvent::Demote };
        // SAFETY: as `claim_for_tier`; the prologue's handler runs no more for this entry.
        unsafe { (*self.prologue.get()).handler = *tier_prologue };
        let LaneBody::Specialized { method, .. } = std::mem::replace(body, next) else { unreachable!() };
        super::super::tier_machine::withdraw(&self.entry, body_state, event);
        self.tier().retired.borrow_mut().push(method);
    }

    /// The entries before its next body specializes: the warm-up, doubled per dropped body.
    pub(super) fn warmup_entries(&self) -> u32 {
        u32::from(self.tiers.lane_body.warmup()) << self.tier().history.get().lifetimes.min(MAX_DOUBLINGS)
    }

    /// The bytecodes one entry runs in the interpreter inside its loops: each loop that runs in no
    /// region, its iteration span times its mean trip count (one iteration without a native run).
    pub(in crate::vm) fn loop_work(&self) -> u64 {
        self.loops.iter().filter(|site| !site.runs_region()).map(|site| site.span() * site.tier.borrow().mean_trip().unwrap_or(1)).sum()
    }

    /// The native runs of the function's loops that run in a region entered from the lane: each
    /// crossed into native code, which the function's own body would run without a crossing.
    pub(in crate::vm) fn region_runs(&self) -> u64 {
        self.loops.iter().filter(|site| site.runs_region()).map(|site| site.tier.borrow().runs()).sum()
    }

    /// Whether a closed loop of the function runs in a region that has run natively and still
    /// enters it: its calls pay that region's entry, which the function's own body would not.
    /// A region that never ran (its compile was refused) makes none of the work native.
    pub(in crate::vm) fn enters_region(&self) -> bool {
        self.loops.iter().any(super::LoopSite::runs_region)
    }

    /// Whether a function entered rarely, whose loops run no region that enters native code (its
    /// work is already native), has still been entered at least `RECURRENT_ENTRIES` times
    /// over `RECURRENT_TICKS` warming entries of the program: an entry that runs long (a driver whose callees
    /// are hot) costs more than the many cheap entries the warm-up counts, and its callees'
    /// bodies run from the lane until it specializes and inlines them. Read every
    /// `RECURRENT_ENTRIES`th entry.
    fn recurrent(&self, entries: u32) -> bool {
        entries >= RECURRENT_ENTRIES && entries % RECURRENT_ENTRIES == 0 && self.tier().history.get().lifetimes == 0
            && warm_clock().wrapping_sub(self.tier().warm_since.get()) >= RECURRENT_TICKS && !self.enters_region()
    }

    /// Whether `entries` is a count at which its function weighs a compile: the warm-up, then
    /// each doubling of it, so a function that has not paid for its compile yet looks again
    /// when it has done twice the work.
    fn attempts_at(&self, entries: u32) -> bool {
        let warmup = self.warmup_entries();
        entries % warmup == 0 && (entries / warmup).is_power_of_two()
    }

    /// Whether `entries`, an attempt's count, is the last the warm-up schedule can name: the
    /// next attempt doubles it past the counter's range, so no later entry looks again.
    fn last_attempt(&self, entries: u32) -> bool {
        let warmup = u64::from(self.warmup_entries().max(1));
        let attempts = u64::from(entries) / warmup;
        attempts >= 1 && u64::from(entries) + warmup * attempts > u64::from(u32::MAX)
    }

    /// Whether its history affords another compile.
    pub(super) fn affords_compile(&self) -> bool {
        let history = self.tier().history.get();
        history.lifetimes <= self.tiers.lane_body.max_lifetimes
    }

    /// Whether a body compiled a call of this function (`note_caller`): native callers want its
    /// body whatever its length, which only the lane's own entries ask to amortize.
    fn called_by_bodies(&self) -> bool { !self.tier().callers.borrow().is_empty() || self.tier().history.get().demanded }

    /// Warms again a body declined for its length alone, once a caller's compile meets a call to
    /// it: native callers want it whatever its length. Whether it is warming now.
    pub(in crate::vm) fn demand_body(&self) -> bool {
        let history = self.tier().history.get();
        // SAFETY: as `lane_body`; no lane body runs while a compile plans.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        if history.short && !history.demanded && matches!(body, LaneBody::Declined) {
            self.tier().history.set(LaneBodyHistory { demanded: true, ..history });
            *body = LaneBody::Unspecialized { entries: 0, recompiles: 0, stuck: LaneLeaves::NONE };
            self.reclaim_for_tier();
        }
        self.warming()
    }

    /// Records that `caller`'s body compiled a call of this function, by its entry or through
    /// the lane's helper: a fact of this view's body, whose publication invalidates it.
    pub(in crate::vm) fn note_caller(&self, caller: &LaneView) {
        let caller = std::ptr::from_ref(caller);
        let mut callers = self.tier().callers.borrow_mut();
        if !std::ptr::eq(caller, self) && !callers.contains(&caller) { callers.push(caller); }
    }

    /// Its body published: each caller that compiled against the one before, or against
    /// none, specializes again at its next entry and calls this one by its entry
    /// (`compiled::DirectCallee`). A caller's running body keeps its code (`retired`).
    fn respecialize_callers(&self) {
        for caller in self.tier().callers.take() {
            unsafe { &*caller }.respecialize();
        }
    }

    pub(super) fn note_late_call(&self) -> bool {
        let history = self.tier().history.get();
        let late_calls = history.late_calls.saturating_add(1);
        self.tier().history.set(LaneBodyHistory { late_calls, ..history });
        late_calls == self.tiers.lane_body.late_call_calls
    }

    pub(super) fn owns_record(&self, record: super::Ip) -> bool {
        let start = self.records();
        record >= start && record < start.wrapping_add(self.records.len())
    }

    pub(super) fn respecialize(&self) {
        let history = self.tier().history.get();
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        let LaneBody::Specialized { tier_prologue, recompiles, stuck, .. } = body else { return };
        if history.callee_recompiles >= self.tiers.lane_body.max_callee_recompiles { return; }
        self.tier().history.set(LaneBodyHistory { callee_recompiles: history.callee_recompiles + 1, late_calls: 0, ..history });
        unsafe { (*self.prologue.get()).handler = *tier_prologue };
        let next = LaneBody::Unspecialized { entries: u32::from(self.tiers.lane_body.warmup_entries) - 1, recompiles: *recompiles, stuck: *stuck };
        let body_state = body.tier_state();
        let LaneBody::Specialized { method, .. } = std::mem::replace(body, next) else { unreachable!() };
        super::super::tier_machine::withdraw(&self.entry, body_state, TierEvent::Demote);
        self.tier().retired.borrow_mut().push(method);
    }

    /// A compile a lifecycle edge outran counts as a dropped body: no run
    /// of it took place.
    pub(super) fn note_dropped_compile(&self) {
        let history = self.tier().history.get();
        self.tier().history.set(LaneBodyHistory { lifetimes: history.lifetimes.saturating_add(1), ..history });
    }

    /// A specialized body that went at a lifecycle edge warms again from the start, keeping
    /// where it and its predecessors left, and its recompiles, as its successor's feedback: a call site's behavior
    /// outlives the shape ids the edge renumbered. One no run of which completed counts as
    /// wasted (`LaneBodyHistory`).
    pub(super) fn drop_body(&self, body: &mut LaneBody) {
        let LaneBody::Specialized { leaves_at, stuck, recompiles, .. } = body else { return };
        let (stuck, recompiles) = (stuck.union(leaves_at.get()), *recompiles);
        let history = self.tier().history.get();
        self.tier().history.set(LaneBodyHistory {
            lifetimes: history.lifetimes.saturating_add(1),
            ..history
        });
        super::super::tier_machine::withdraw(&self.entry, body.tier_state(), TierEvent::Demote);
        *body = LaneBody::Unspecialized { entries: 0, recompiles, stuck };
    }

    /// The body a just published method makes of this view.
    pub(super) fn specialized(&self, entry: Handler, method: std::rc::Rc<super::super::compiled::PublishedLaneMethod>, recompiles: u8, stuck: LaneLeaves) -> LaneBody {
        // A body whose entry would not pay for itself serves native callers only
        // (`lane_body_call`, direct calls); the lane's own entries keep the records, and it
        // has no trial to run.
        let tier_prologue = if method.lease().expect("a body just published is live").amortizes_entry() {
            // SAFETY: as `claim_for_tier`; no handler reads the prologue now.
            unsafe { std::mem::replace(&mut (*self.prologue.get()).handler, entry as *const ()) }
        } else {
            self.release_from_tier();
            unsafe { (*self.prologue.get()).handler }
        };
        use std::cell::Cell;
        self.audit_restart();
        // Native callers enter a closed body through the view's cell, and leave inside it
        // through the view's pin.
        if let Some(body) = method.lease().filter(|body| body.calls_only_natively()) {
            self.tier().pinned.borrow_mut().push(body);
            super::super::tier_machine::publish(&self.entry, TierState::Compiling, &method);
        }
        self.entry.reset_evidence();
        LaneBody::Specialized { method, tier_prologue,
            leaves_at: Cell::new(LaneLeaves::NONE), completed: Cell::new(false), recompiles, stuck }
    }

    /// A mode with neither Cranelift function tiers nor `--b-jit`'s stencil bodies compiles no
    /// function body: the view's body is declined, once.
    fn declined_by_mode(&self) -> bool {
        let declined = !crate::jit_mode::tiers_allowed(crate::jit_mode::Tier::CRANELIFT) && !crate::jit_mode::base_only();
        if declined {
            // SAFETY: as `lane_body`.
            unsafe { *self.tier().tier_lane.get() = LaneBody::Declined };
        }
        declined
    }

    /// Whether its body's code is compiling (`LaneBody::Compiling`).
    pub(super) fn compiling(&self) -> bool {
        // SAFETY: as `lane_body`.
        matches!(unsafe { &*self.tier().tier_lane.get() }, LaneBody::Compiling { .. })
    }

    /// Counts an entry toward the frameless body and specializes it at the warm-up; whether
    /// the body is still warming or specialized, so its prologue stays. A specialized body is
    /// a fact of the view from then on: its prologue record enters it with `entry`, which
    /// tests nothing the specialization proved (`lane_body_entry`).
    pub(super) fn specialize_lane_body(&self, entry: Handler, work: impl FnOnce(super::super::compiled::LaneBodyWork) -> super::super::compiled::LaneBodyOutcome) -> bool {
        if self.declined_by_mode() { return false; }
        self.settle_retired_body();
        // SAFETY: as `lane_body`; specializing runs no lane body, and the body is out of the view while the VM works on it.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        if awaits_look(body) { return true; }
        let (outcome, entries, recompiles, stuck, looks) = match std::mem::replace(body, LaneBody::Declined) {
            LaneBody::Unspecialized { entries, recompiles, stuck } if self.attempts_at(entries.saturating_add(1)) || self.recurrent(entries.saturating_add(1)) => {
                if !self.affords_compile() {
                    *body = LaneBody::Unspecialized { entries, recompiles, stuck };
                    return false;
                }
                super::super::compiled::base_region::count(super::super::compiled::base_region::Event::BodyCompile);
                if !self.attempts_at(entries.saturating_add(1)) { super::super::compiled::base_region::count(super::super::compiled::base_region::Event::BodyRecurrent); }
                // A predecessor's few leaves are the specialization's feedback (`stuck`).
                let generic: Vec<_> = stuck.exits().collect();
                (work(super::super::compiled::LaneBodyWork::Start { generic: &generic, shrinks: self.tier().history.get().shrinks, observed: self.attempts_at(entries.saturating_add(1)).then(|| entries.saturating_add(1)), looped: None, called: self.called_by_bodies(), caller: self, arrays: &[] }), entries, recompiles, stuck, 0)
            }
            LaneBody::Unspecialized { entries, recompiles, stuck } => {
                let ticks = warm_ticks();
                if entries == 0 { self.tier().warm_since.set(ticks); }
                *body = LaneBody::Unspecialized { entries: entries.saturating_add(1), recompiles, stuck };
                return true;
            }
            LaneBody::Compiling { pending, recompiles, stuck, looks, .. } =>
                (work(super::super::compiled::LaneBodyWork::Poll(pending)), 0, recompiles, stuck, looks.saturating_add(1)),
            settled => { *body = settled; return !matches!(body, LaneBody::Declined); }
        };
        *body = self.body_after(outcome, entry, entries, recompiles, stuck, looks);
        !matches!(body, LaneBody::Declined)
    }

    /// The body a view's compile work came to (`LaneBodyOutcome`) makes of it; a compile in
    /// flight records the inlining its plan fits (`LaneBodyHistory::shrinks`).
    fn body_after(&self, outcome: super::super::compiled::LaneBodyOutcome, entry: Handler, entries: u32, recompiles: u8, stuck: LaneLeaves, looks: u8) -> LaneBody {
        {
            use super::super::compiled::{LaneBodyOutcome as O, base_region::{Event, count}};
            count(match &outcome {
                O::Published(_) => Event::BodyPublished, O::Compiling(_) => Event::BodyCompiling, O::Unpaid => Event::BodyUnpaid,
                O::Short => Event::BodyShort, O::Declined => Event::BodyDeclined, O::Stale => Event::BodyStale, O::Deferred => Event::BodyDeferred,
            });
        }
        match outcome {
            super::super::compiled::LaneBodyOutcome::Compiling(pending) => {
                self.tier().history.set(LaneBodyHistory { shrinks: pending.shrinks(), ..self.tier().history.get() });
                let wait = 1u16.checked_shl(u32::from(looks)).map_or(u16::MAX, |entries| entries - 1).min(self.tiers.lane_body.compile_poll_interval);
                LaneBody::Compiling { pending, wait: std::cell::Cell::new(wait), looks, recompiles, stuck }
            }
            super::super::compiled::LaneBodyOutcome::Published(published) => {
                self.respecialize_callers();
                self.specialized(entry, published, recompiles, stuck)
            }
            // A compile a collection or a changed fact outran is a body a lifecycle edge dropped.
            super::super::compiled::LaneBodyOutcome::Stale => {
                self.note_dropped_compile();
                LaneBody::Unspecialized { entries: 0, recompiles, stuck }
            }
            super::super::compiled::LaneBodyOutcome::Deferred => LaneBody::Unspecialized { entries: entries.saturating_add(1), recompiles, stuck },
            // Its entries decide when it looks again (`attempts_at`), no hot loop's skip.
            super::super::compiled::LaneBodyOutcome::Unpaid => {
                self.tier().history.set(LaneBodyHistory { unpaid: true, ..self.tier().history.get() });
                let next = entries.saturating_add(1);
                // The attempts double with each warm-up; past the last one the counter can
                // name, the model has priced every entry this function can give and refused.
                // The lane would keep running it through its records for nothing, slower than
                // the general interpreter, so decline it there (`release_from_tier`).
                if self.last_attempt(next) { return LaneBody::Declined }
                LaneBody::Unspecialized { entries: next, recompiles, stuck }
            }
            super::super::compiled::LaneBodyOutcome::Short => {
                self.tier().history.set(LaneBodyHistory { short: true, ..self.tier().history.get() });
                LaneBody::Declined
            }
            super::super::compiled::LaneBodyOutcome::Declined => {
                if self.tier().history.get().lifetimes > 0 { super::dispatch_fast::lane_body_stats::decline("dropped bodies"); }
                self.tier().body_stats.declined.set(super::dispatch_fast::lane_body_stats::take_decline());
                LaneBody::Declined
            }
        }
    }

    /// `specialize_lane_body` at once, past the warm-up: a hot loop's backward jump has run
    /// its calls, so their sites have linked (`lane_loop_retire`).
    pub(super) fn specialize_lane_body_now(&self, entry: Handler, specialize: impl FnOnce(super::super::compiled::LaneBodyWork) -> super::super::compiled::LaneBodyOutcome) {
        self.skip_warm_up();
        self.specialize_lane_body(entry, specialize);
        // Its entries are few (the loop is what is hot): the next one compiles where this one
        // waited for its callees' bodies (`LaneBodyOutcome::Deferred`).
        self.skip_warm_up();
    }

    /// A function whose bodies lifecycle edges discarded warms as any entry does.
    fn skip_warm_up(&self) {
        // SAFETY: as `lane_body`; no lane body runs here.
        if let LaneBody::Unspecialized { entries, .. } = unsafe { &mut *self.tier().tier_lane.get() }
            && self.tier().history.get().lifetimes == 0 && !self.tier().history.get().unpaid
        {
            *entries = u32::from(self.tiers.lane_body.warmup()) - 1;
        }
    }
}

/// The target of the callee the call at `pc` of `view` last reached itself, by the site's own
/// record (`lane_site_targets`): the entry its record links is another callee's once the
/// target cache replaced it.
pub(super) fn site_target<H: super::Host>(vm: &super::Vm<H>, view: &LaneView, pc: usize) -> Option<super::ScriptCallTarget> {
    site_targets(vm, view, pc).next()
}

/// Every callee the call at `pc` of `view` reached (`lane_site_targets`), by its target, the
/// latest first.
pub(super) fn site_targets<'a, H: super::Host>(vm: &'a super::Vm<H>, view: &LaneView, pc: usize) -> impl Iterator<Item = super::ScriptCallTarget> + 'a {
    super::lane_site_targets(vm, view, pc as u32).into_iter().filter(|site| site.through().is_none()).filter_map(|site| {
        // SAFETY: as `Vm::script_call_entry`; the entry is copied before anything resolves a target.
        vm.script_call_entry(site.identity()?).map(|target| unsafe { *target })
    })
}


/// What a first compile of `view` comes to before any planning. A function whose callees still compile waits for their bodies, which it then calls by their entries
/// (`compiled::DirectCallee`); one whose callees only lack a body does not.
pub(super) fn start_gate<H: super::Host>(vm: &super::Vm<H>, view: &LaneView, body: Option<&crate::bytecode::Function>) -> Option<super::super::compiled::LaneBodyOutcome> {
    use super::super::compiled::LaneBodyOutcome;
    let body = body?;
    let calls = |pending: fn(&LaneView) -> bool| body.code.iter().enumerate().any(|(pc, packed)| {
        body.widened(*packed).op().is_call()
            && site_target(vm, view, pc).is_some_and(|target| {
                // SAFETY: a resolved target's view outlives its program.
                target.program() == vm.active_program && !target.view.is_null() && !std::ptr::eq(target.view, view) && pending(unsafe { &*target.view })
            })
    });
    // A callee declined for its length warms toward the body this one calls by its entry
    // (`LaneView::demand_body`); until it publishes, the call takes the helper path, which runs
    // natively too, so nothing waits for it.
    calls(LaneView::demand_body);
    calls(LaneView::compiling).then_some(LaneBodyOutcome::Deferred)
}

/// Whether a compiling `body` lets this entry pass without looking for its code, counting it
/// down: a compile takes milliseconds and a hot loop enters at each backward jump.
fn awaits_look(body: &LaneBody) -> bool {
    let LaneBody::Compiling { wait, .. } = body else { return false };
    let left = wait.get();
    wait.set(left.saturating_sub(1));
    left > 0
}

impl LaneBody {
    /// Its place in the per-function tier machine (`tier_machine`).
    pub(super) fn tier_state(&self) -> TierState {
        match self {
            Self::Unspecialized { .. } => TierState::Warming,
            Self::Compiling { .. } => TierState::Compiling,
            Self::Specialized { .. } if crate::jit_mode::quick_only() => TierState::QJit,
            Self::Specialized { .. } => TierState::UJit,
            Self::Declined => TierState::Declined,
        }
    }
}
