//! When a lane body compiles again, keeps its code or retires: decisions from what its runs
//! and its predecessors' did.

use super::{Handler, LaneBody, LaneLeaves, LaneView, TIERS};
use super::compiled::tier_policy::PERCENT;
use super::super::compiled::LaneExit;

/// A view's lane-body record over the run, which a lifecycle edge keeps: the bodies such
/// edges (a shape compaction, a dead native callee) dropped, and those of them no run of
/// which completed (`LaneView::drop_body`). Each dropped body doubles the next one's warm-up,
/// so a function whose bodies keep being discarded stops paying a compile for each;
/// `TIERS.lane_body.max_lifetimes` dropped bodies, decline it.
#[derive(Clone, Copy, Default)]
pub(super) struct LaneBodyHistory {
    pub(super) lifetimes: u8,
    /// The inlining budget halvings its last plan needed to fit (`PendingLaneMethod::shrinks`).
    shrinks: u8,
    /// The warm-ups its compile waited for its callees' bodies (`LaneView::waits_for_callees`).
    callee_waits: u8,
    /// The bodies it specialized past because a callee published one (`respecialize_callers`).
    callee_recompiles: u8,
    late_calls: u32,
    /// Whether a compile of its body waited for its entries to pay for it (`LaneBodyOutcome::Unpaid`).
    unpaid: bool,
    /// Whether its body was declined for its length alone (`LaneBodyOutcome::Short`).
    short: bool,
    /// Whether a caller's compile asked for its body (`LaneView::demand_body`).
    demanded: bool,
}

/// The entries and the milliseconds of warming past which a function specializes by its
/// recurrence alone (`LaneView::recurrent`).
const RECURRENT_ENTRIES: u16 = 16;
const RECURRENT_TICKS: u32 = 256;

/// The calling thread's CPU nanoseconds, excluding time it waited: a loop region's trial uses
/// it, so the idle time between runs is not counted.
pub(in crate::vm) fn thread_cpu_ns() -> u64 {
    #[cfg(unix)]
    {
        let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: `clock_gettime` writes the struct this call owns.
        if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) } == 0 {
            return u64::try_from(now.tv_sec).unwrap_or(0) * 1_000_000_000
                + u64::try_from(now.tv_nsec).unwrap_or(0);
        }
    }
    0
}

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
fn verdict(completed: bool, recompiles: u8, leaves: LaneLeaves, stuck: LaneLeaves) -> Option<LaneLeaves> {
    // A body no run of which completed leaves at the places its recompile would too: only a
    // body that has run natively learns anything from its leaves.
    if !completed { return None; }
    // Leaves only where the predecessors' bodies left, and those places already generic,
    // would plan the same body again: it stays.
    let nothing_new = leaves != LaneLeaves::Scattered && stuck != LaneLeaves::Scattered
        && leaves.exits().all(|exit| stuck.exits().any(|known| known == exit));
    (recompiles < TIERS.lane_body.max_recompiles && !(recompiles > 0 && nothing_new)).then(|| stuck.union(leaves))
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
    /// `TIERS.lane_body.reoptimize_exits` leaves, doubled per recompile, it takes its
    /// `verdict`. A body it compiles past keeps its code for the direct callers that name it.
    pub(super) fn note_lane_body_end(&self, exit: LaneExit, completed_region: bool, left_new: impl FnOnce((Option<u32>, usize))) {
        // SAFETY: as `lane_body`; the run that ended has finished, and nothing reads its body.
        let body = unsafe { &mut *self.tier_lane.get() };
        self.note_native_run(body);
        let LaneBody::Specialized { exits, leaves_at, completed, completions, recompiles, stuck, method, .. } = body else { return };
        if completed_region || matches!(exit, LaneExit::Returned { .. }) {
            completed.set(true);
            completions.set(completions.get().saturating_add(1));
            return;
        }
        exits.set(exits.get().saturating_add(1));
        name_leave(leaves_at, exit, method, left_new);
        // Leaves past the first `reoptimize_exits` that exceed `reoptimize_share` percent of the runs.
        let (leaves, runs) = (u64::from(exits.get()), u64::from(exits.get()) + u64::from(completions.get()));
        if exits.get() < TIERS.lane_body.reoptimize_exits || leaves * PERCENT <= runs * u64::from(TIERS.lane_body.reoptimize_share) {
            return;
        }
        let Some(stuck) = verdict(completed.get(), *recompiles, leaves_at.get(), *stuck) else {
            // A body that keeps leaving where no other body would, and has never completed a
            // run, gains nothing from its entry: its function runs in the lane again.
            let costly = runs >= u64::from(TIERS.lane_body.retire_after_runs) && leaves * PERCENT > runs * u64::from(TIERS.lane_body.retire_share);
            if !completed.get() || costly {
                self.retire_body(body, LaneBody::Declined);
                self.release_from_tier();
            }
            return;
        };
        let next = LaneBody::Unspecialized { entries: 0, recompiles: *recompiles + 1, stuck };
        self.retire_body(body, next);
    }

    /// Records the lane's nanoseconds per run over the `entries` since it began warming, which
    /// the body a compile starts is judged against.
    fn note_lane_rate(&self, entries: u16) {
        let Some(started) = self.lane_started.take() else { return };
        if entries == 0 { return; }
        let nanos = started.elapsed().as_nanos() / u128::from(entries);
        self.lane_ns_per_run.set(u32::try_from(nanos).unwrap_or(u32::MAX));
    }

    /// Counts a run of the specialized body; at `native_trial_runs` it judges the body's
    /// entry against the lane's measured rate: a native run that takes at least the lane's
    /// (`native_slow_permille`) keeps neither its entry nor its compile, and the function runs
    /// in the lane again. A body the lane measured no rate for (one serving native callers
    /// only) is not judged.
    fn note_native_run(&self, body: &mut LaneBody) {
        if self.native_judged.get() || !matches!(body, LaneBody::Specialized { .. }) { return; }
        let runs = self.native_runs.get().saturating_add(1);
        self.native_runs.set(runs);
        if runs < TIERS.lane_body.native_trial_runs { return; }
        self.native_judged.set(true);
        let Some(since) = self.native_since.get() else { return };
        let lane = u128::from(self.lane_ns_per_run.get());
        if lane == 0 { return; }
        let native = since.elapsed().as_nanos() / u128::from(runs);
        if crate::trace::Channel::JsJit.on() {
            eprintln!("QUENCH_LANE_TRIAL {{\"function\":{},\"runs\":{runs},\"lane_ns\":{lane},\"native_ns\":{native}}}", self.body_stats.function());
        }
        if native > u128::from(TIERS.lane_body.native_sample_max_ns) {
            // Calls spread over idle time, not a rate: time the next window.
            self.native_since.set(Some(std::time::Instant::now()));
            self.native_runs.set(0);
            self.native_judged.set(false);
            return;
        }
        if native * 1000 < lane * u128::from(TIERS.lane_body.native_slow_permille) { return; }
        super::dispatch_fast::lane_body_stats::decline("trial:slower-than-lane");
        self.retire_body(body, LaneBody::Declined);
        self.release_from_tier();
    }

    /// Replaces the specialized `body` with `next`: its entry's handler runs no more, its entry
    /// cell takes the stub, and its code waits for the callers that still name it.
    fn retire_body(&self, body: &mut LaneBody, next: LaneBody) {
        let LaneBody::Specialized { tier_prologue, .. } = body else { unreachable!() };
        // SAFETY: as `claim_for_tier`; the prologue's handler runs no more for this entry.
        unsafe { (*self.prologue.get()).handler = *tier_prologue };
        let LaneBody::Specialized { method, .. } = std::mem::replace(body, next) else { unreachable!() };
        self.entry.withdraw();
        self.retired.borrow_mut().push(method);
    }

    /// The entries before its next body specializes: the warm-up, doubled per dropped body.
    pub(super) fn warmup_entries(&self) -> u16 {
        TIERS.lane_body.warmup_entries << self.history.get().lifetimes.min(7)
    }

    /// The bytecodes one entry runs inside its loops: each loop's iteration span times the mean
    /// trip count its region's runs took (loops without a native run count one iteration).
    pub(in crate::vm) fn loop_work(&self) -> u64 {
        self.loops.iter().map(|site| u64::from(site.backedge - site.header + 1) * site.tier.borrow().mean_trip().unwrap_or(1)).sum()
    }

    /// Whether a closed loop of the function runs in a region that still enters native code:
    /// its calls pay that region's entry, which the function's own body would not.
    pub(in crate::vm) fn enters_region(&self) -> bool {
        self.loops.iter().any(|site| site.call_boundary_pcs.is_empty() && site.active_region().is_some() && site.enters())
    }

    /// Whether a function entered rarely, whose loops run no region that enters native code (its
    /// work is already native), has still been entered at least `RECURRENT_ENTRIES` times
    /// over `RECURRENT_TICKS` warming entries of the program: an entry that runs long (a driver whose callees
    /// are hot) costs more than the many cheap entries the warm-up counts, and its callees'
    /// bodies run from the lane until it specializes and inlines them. Read every
    /// `RECURRENT_ENTRIES`th entry.
    fn recurrent(&self, entries: u16) -> bool {
        entries >= RECURRENT_ENTRIES && entries % RECURRENT_ENTRIES == 0 && self.history.get().lifetimes == 0
            && warm_clock().wrapping_sub(self.warm_since.get()) >= RECURRENT_TICKS && !self.enters_region()
    }

    /// Whether `entries` is a count at which its function weighs a compile: the warm-up, then
    /// each doubling of it, so a function that has not paid for its compile yet looks again
    /// when it has done twice the work.
    fn attempts_at(&self, entries: u16) -> bool {
        let warmup = self.warmup_entries();
        entries % warmup == 0 && (entries / warmup).is_power_of_two()
    }

    /// Whether its history affords another compile.
    pub(super) fn affords_compile(&self) -> bool {
        let history = self.history.get();
        history.lifetimes <= TIERS.lane_body.max_lifetimes
    }

    /// Whether its compile waits one more warm-up for callees still warming, whose bodies it
    /// then calls by their entries (`compiled::DirectCallee`).
    pub(super) fn waits_for_callees(&self) -> bool {
        let history = self.history.get();
        let waits = history.callee_waits < TIERS.lane_body.max_callee_waits;
        if waits {
            self.history.set(LaneBodyHistory { callee_waits: history.callee_waits + 1, ..history });
        }
        waits
    }

    /// Whether a body compiled a call of this function (`note_caller`): native callers want its
    /// body whatever its length, which only the lane's own entries ask to amortize.
    fn called_by_bodies(&self) -> bool { !self.callers.borrow().is_empty() || self.history.get().demanded }

    /// Warms again a body declined for its length alone, once a caller's compile meets a call to
    /// it: native callers want it whatever its length. Whether it is warming now.
    pub(in crate::vm) fn demand_body(&self) -> bool {
        let history = self.history.get();
        // SAFETY: as `lane_body`; no lane body runs while a compile plans.
        let body = unsafe { &mut *self.tier_lane.get() };
        if history.short && !history.demanded && matches!(body, LaneBody::Declined) {
            self.history.set(LaneBodyHistory { demanded: true, ..history });
            *body = LaneBody::Unspecialized { entries: 0, recompiles: 0, stuck: LaneLeaves::NONE };
            self.reclaim_for_tier();
        }
        self.warming()
    }

    /// Records that `caller`'s body compiled a call of this function, by its entry or through
    /// the lane's helper: a fact of this view's body, whose publication invalidates it.
    pub(in crate::vm) fn note_caller(&self, caller: &LaneView) {
        let caller = std::ptr::from_ref(caller);
        let mut callers = self.callers.borrow_mut();
        if !std::ptr::eq(caller, self) && !callers.contains(&caller) { callers.push(caller); }
    }

    /// Its body published: each caller that compiled against the one before, or against
    /// none, specializes again at its next entry and calls this one by its entry
    /// (`compiled::DirectCallee`). A caller's running body keeps its code (`retired`).
    fn respecialize_callers(&self) {
        for caller in self.callers.take() {
            unsafe { &*caller }.respecialize();
        }
    }

    pub(super) fn note_late_call(&self) -> bool {
        let history = self.history.get();
        let late_calls = history.late_calls.saturating_add(1);
        self.history.set(LaneBodyHistory { late_calls, ..history });
        late_calls == TIERS.lane_body.late_call_calls
    }

    pub(super) fn owns_record(&self, record: super::Ip) -> bool {
        let start = self.records();
        record >= start && record < start.wrapping_add(self.records.len())
    }

    pub(super) fn respecialize(&self) {
        let history = self.history.get();
        let body = unsafe { &mut *self.tier_lane.get() };
        let LaneBody::Specialized { tier_prologue, recompiles, stuck, .. } = body else { return };
        if history.callee_recompiles >= TIERS.lane_body.max_callee_recompiles { return; }
        self.history.set(LaneBodyHistory { callee_recompiles: history.callee_recompiles + 1, late_calls: 0, ..history });
        unsafe { (*self.prologue.get()).handler = *tier_prologue };
        let next = LaneBody::Unspecialized { entries: TIERS.lane_body.warmup_entries - 1, recompiles: *recompiles, stuck: *stuck };
        let LaneBody::Specialized { method, .. } = std::mem::replace(body, next) else { unreachable!() };
        self.entry.withdraw();
        self.retired.borrow_mut().push(method);
    }

    /// A compile a lifecycle edge outran counts as a dropped body: no run
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
        let LaneBody::Specialized { leaves_at, stuck, recompiles, .. } = body else { return };
        let (stuck, recompiles) = (stuck.union(leaves_at.get()), *recompiles);
        let history = self.history.get();
        self.history.set(LaneBodyHistory {
            lifetimes: history.lifetimes.saturating_add(1),
            ..history
        });
        self.entry.withdraw();
        *body = LaneBody::Unspecialized { entries: 0, recompiles, stuck };
    }

    /// The body a just published method makes of this view.
    pub(super) fn specialized(&self, entry: Handler, method: std::rc::Rc<super::super::compiled::PublishedLaneMethod>, recompiles: u8, stuck: LaneLeaves) -> LaneBody {
        self.native_since.set(Some(std::time::Instant::now()));
        self.native_runs.set(0);
        self.native_judged.set(false);
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
        // Native callers enter a closed body through the view's cell, and leave inside it
        // through the view's pin.
        if let Some(body) = method.lease().filter(|body| body.calls_only_natively()) {
            self.pinned.borrow_mut().push(body);
            self.entry.publish(&method);
        }
        LaneBody::Specialized { method, tier_prologue, completions: Cell::new(0), exits: Cell::new(0),
            leaves_at: Cell::new(LaneLeaves::NONE), completed: Cell::new(false), recompiles, stuck }
    }

    /// `--b-jit` compiles no function body: the view's body is declined, once.
    fn declined_by_mode(&self) -> bool {
        let declined = !crate::jit_mode::function_tiers();
        if declined {
            // SAFETY: as `lane_body`.
            unsafe { *self.tier_lane.get() = LaneBody::Declined };
        }
        declined
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
        if self.declined_by_mode() {
            return false;
        }
        self.settle_retired_body();
        // SAFETY: as `lane_body`; specializing runs no lane body, and the body is taken out
        // of the view while the VM works on it.
        let body = unsafe { &mut *self.tier_lane.get() };
        if awaits_look(body) {
            return true;
        }
        let (outcome, entries, recompiles, stuck, looks) = match std::mem::replace(body, LaneBody::Declined) {
            LaneBody::Unspecialized { entries, recompiles, stuck } if self.attempts_at(entries.saturating_add(1)) || self.recurrent(entries.saturating_add(1)) => {
                if !self.affords_compile() {
                    *body = LaneBody::Unspecialized { entries, recompiles, stuck };
                    return false;
                }
                super::super::compiled::base_region::count(super::super::compiled::base_region::Event::BodyCompile);
                self.note_lane_rate(entries);
                // A predecessor's few leaves are the specialization's feedback (`stuck`).
                let generic: Vec<_> = stuck.exits().collect();
                (work(super::super::compiled::LaneBodyWork::Start { generic: &generic, shrinks: self.history.get().shrinks, observed: self.attempts_at(entries.saturating_add(1)).then(|| u32::from(entries) + 1), called: self.called_by_bodies(), caller: self, arrays: &[] }), entries, recompiles, stuck, 0)
            }
            LaneBody::Unspecialized { entries, recompiles, stuck } => {
                let ticks = warm_ticks();
                if entries == 0 { self.warm_since.set(ticks); self.lane_started.set(Some(std::time::Instant::now())); }
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
    fn body_after(&self, outcome: super::super::compiled::LaneBodyOutcome, entry: Handler, entries: u16, recompiles: u8, stuck: LaneLeaves, looks: u8) -> LaneBody {
        match outcome {
            super::super::compiled::LaneBodyOutcome::Compiling(pending) => {
                self.history.set(LaneBodyHistory { shrinks: pending.shrinks(), ..self.history.get() });
                let wait = 1u16.checked_shl(u32::from(looks)).map_or(u16::MAX, |entries| entries - 1).min(TIERS.lane_body.compile_poll_interval);
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
                self.history.set(LaneBodyHistory { unpaid: true, ..self.history.get() });
                LaneBody::Unspecialized { entries: entries.saturating_add(1), recompiles, stuck }
            }
            super::super::compiled::LaneBodyOutcome::Short => {
                self.history.set(LaneBodyHistory { short: true, ..self.history.get() });
                LaneBody::Declined
            }
            super::super::compiled::LaneBodyOutcome::Declined => {
                if self.history.get().lifetimes > 0 { super::dispatch_fast::lane_body_stats::decline("dropped bodies"); }
                self.body_stats.declined.set(super::dispatch_fast::lane_body_stats::take_decline());
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
        if let LaneBody::Unspecialized { entries, .. } = unsafe { &mut *self.tier_lane.get() }
            && self.history.get().lifetimes == 0 && !self.history.get().unpaid
        {
            *entries = TIERS.lane_body.warmup_entries - 1;
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
/// (`compiled::DirectCallee`).
pub(super) fn start_gate<H: super::Host>(vm: &super::Vm<H>, view: &LaneView, body: Option<&crate::bytecode::Function>) -> Option<super::super::compiled::LaneBodyOutcome> {
    use super::super::compiled::LaneBodyOutcome;
    let body = body?;
    let calls = |pending: fn(&LaneView) -> bool| body.code.iter().enumerate().any(|(pc, packed)| {
        super::decoded_instruction(body, *packed).op().is_call()
            && site_target(vm, view, pc).is_some_and(|target| {
                // SAFETY: a resolved target's view outlives its program.
                target.program == vm.active_program && !target.view.is_null() && !std::ptr::eq(target.view, view) && pending(unsafe { &*target.view })
            })
    });
    (calls(LaneView::compiling) || (calls(LaneView::demand_body) && view.waits_for_callees())).then_some(LaneBodyOutcome::Deferred)
}

/// Whether a compiling `body` lets this entry pass without looking for its code, counting it
/// down: a compile takes milliseconds and a hot loop enters at each backward jump.
fn awaits_look(body: &LaneBody) -> bool {
    let LaneBody::Compiling { wait, .. } = body else { return false };
    let left = wait.get();
    wait.set(left.saturating_sub(1));
    left > 0
}
