//! When a lane body compiles again, keeps its code or retires: decisions from what its runs
//! and its predecessors' did.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::tier_machine::{TierEvent, TierState};
use super::{Handler, LaneBody, LaneLeaves, LaneView, Tiers};
use quench_runtime::vm::tier_policy::PERCENT;
use crate::compiled::LaneExit;

pub(crate) use quench_runtime::vm::engine_types::LaneBodyHistory;


#[path = "lane_body_specialize.rs"]
pub(crate) mod specialize;

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
fn credit_native_callees(method: &quench_runtime::vm::engine_types::PublishedLaneMethod) {
    let Some(body) = method.lease() else { return };
    for callee in body.direct.iter() {
        // SAFETY: the caller's body owns the callee it named; the view outlives the run.
        unsafe { &*callee.target.view }.entry.note_completions(NATIVE_CREDIT_RUNS);
    }
}

/// The function a leave at `exit` is in (`None`: the body's own) and its PC, for a leave that
/// has an instruction.
fn leave_place(exit: LaneExit, method: &quench_runtime::vm::engine_types::PublishedLaneMethod) -> Option<(Option<u32>, usize)> {
    match exit {
        LaneExit::At(pc) => Some((None, pc)),
        LaneExit::Inlined { frame, pc } => method.lease().and_then(|method| method.inlined.get(frame).map(|call| (Some(call.target.function), pc as usize))),
        _ => None,
    }
}

/// Names the place of a leave at `exit` among the body's, and hands a place it had not named to
/// `left_new` once. A scattered body names none.
fn name_leave(leaves_at: &std::cell::Cell<LaneLeaves>, exit: LaneExit, method: &quench_runtime::vm::engine_types::PublishedLaneMethod, left_new: impl FnOnce((Option<u32>, usize))) {
    // SAFETY: the cell is read in place and nothing writes it meanwhile.
    let seen = match unsafe { &*leaves_at.as_ptr() } {
        LaneLeaves::Few(named) => named.contains(&Some(exit)),
        LaneLeaves::Scattered => true,
    };
    if seen { return; }
    leaves_at.set(leaves_at.get().with(exit));
    if leaves_at.get() != LaneLeaves::Scattered && let Some(place) = leave_place(exit, method) { left_new(place); }
}

#[quench_macros::ext(LaneViewOpsX1)]
impl LaneView {
    /// Whether the view's body is retired to `Declined` (`LaneBody::Declined`): a call of the view has no body to enter.
    pub(crate) fn declined_in_lane(&self) -> bool {
        // SAFETY: as `body_view_in_lane`; read only.
        matches!(unsafe { &*self.tier().tier_lane.get() }, LaneBody::Declined)
    }

    /// Whether a leave at `exit` resumed in a direct callee's own body, `ran`, which the frame ran rather than the
    /// root body (`root`) and which is still its current one: that leave takes the verdict its own runs' leaves
    /// take (`note_lane_body_end`), so a body that leaves at every native call recompiles, or falls back to the
    /// tier below, as one the interpreter enters would.
    fn left_own_body(&self, root: Option<&crate::compiled::LaneMethod>, ran: Option<&crate::compiled::LaneMethod>, exit: LaneExit) -> bool {
        ran.is_some_and(|ran| !root.is_some_and(|root| std::ptr::eq(root, ran))
            && exit.at_place()
            && self.body_view_in_lane().is_some_and(|current| std::ptr::eq(&*current, ran)))
    }

    /// `left`, the place a leave at `exit` resumed, once the leave's activation is made: a leave in a direct callee's own
    /// body (`left_own_body`) is noted as that body's (`note_body_end_in_lane`), which its retirement may not outlive.
    pub(crate) fn resumed_leave<T>(&self, cx: *mut crate::dispatch_fast::LaneContext, (root, ran): (Option<&crate::compiled::LaneMethod>, Option<&crate::compiled::LaneMethod>), exit: LaneExit, left: T) -> T {
        if self.left_own_body(root, ran, exit) { crate::dispatch_fast::note_body_end_in_lane(cx, self, exit, |_| {}); }
        left
    }

    /// Counts how a run of the specialized body ended, at `exit`; at
    /// `self.tiers.lane_body.reoptimize_exits` leaves, doubled per recompile, it takes its
    /// `verdict`. A body it compiles past keeps its code for the direct callers that name it.
    /// Whether the run retired the body.
    pub(crate) fn note_lane_body_end(&self, exit: LaneExit, completed_region: bool, left_new: impl FnOnce((Option<u32>, usize))) -> bool {
        // SAFETY: as `lane_body`; the run that ended has finished, and nothing reads its body.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        let LaneBody::Specialized { leaves_at, completed, recompiles, stuck, method, .. } = body else { return false };
        if completed_region || exit.completes() {
            completed.set(true);
            let completions = self.entry.note_completions(1);
            if completions % NATIVE_CREDIT_RUNS == 0 { credit_native_callees(method); }
            // A body compiled quick weighs the optimizing compile at each doubling of its runs, and the b-jit
            // body the default mode started on weighs the u-jit compile the same way (`promote_base_body`).
            if completions.is_power_of_two() && self.promote_base_body(u32::try_from(completions).unwrap_or(u32::MAX)) { return true; }
            let quick_ir = self.tier().history.get().quick_ir;
            if quick_ir != 0 && completions.is_power_of_two()
                && crate::compiled::lane_facts::optimizes_quick_body(self.tiers, self, self.records.len() as u64, u32::try_from(completions).unwrap_or(u32::MAX), quick_ir as usize)
            {
                self.optimize_body();
                return true;
            }
            return false;
        }
        let exits = self.entry.note_leave();
        name_leave(leaves_at, exit, method, left_new);
        // An ahead image's body is the ladder's first body: it reoptimizes on its leaves as a JIT body does.
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
                // Where the stencil tier runs too, the tier below takes it over (`LaneBodyHistory::base_fallback`):
                // its next entry stitches the b-jit body, whose rows take any shape the u-jit body guarded.
                let history = self.tier().history.get();
                // The stencil body reads the places this one left at generically (`base_body::left_read_sites`).
                let left = stuck.union(leaves_at.get());
                if !history.base_fallback && crate::compiled::admission::stencil_below() {
                    self.tier().history.set(LaneBodyHistory { base_fallback: true, ..history });
                    let next = LaneBody::Unspecialized { entries: u32::from(self.tiers.lane_body.warmup()).saturating_sub(1), recompiles: *recompiles, stuck: left };
                    self.retire_body(body, next);
                    return true;
                }
                // The stencil body that took over keeps running where it completes: retiring it would leave its
                // function no body at all (`LaneBody::Declined`), whose every call from a compiled caller leaves it.
                if history.base_fallback && completed.get() { return false; }
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

    /// Whether the default mode starts this function's body on b-jit (`LaneBodyHistory::base_first`): one not promoted
    /// past it, whose loops never ran. A function whose loops ran keeps them on their own regions, which tier up by their
    /// measured runs; a stencil body around a loop never would (c_i32_indirect: 263 Ir/iter in it, 13 in the loop's u-jit region).
    pub(crate) fn starts_on_base(&self, history: LaneBodyHistory) -> bool {
        !history.promoted && !self.loops.iter().any(|site| site.taken.get() > 0 || { let tier = site.tier.borrow(); tier.code.is_some() || tier.iterations() > 0 })
    }

    /// Retires the b-jit body the default mode started its function on once its `completions` repay the u-jit
    /// compile over it (`lane_facts::promotes_base_body`): its next entry compiles the u-jit body.
    fn promote_base_body(&self, completions: u32) -> bool {
        let history = self.tier().history.get();
        if !history.base_first || history.base_fallback { return false; }
        let length = self.records.len() as u64;
        let ir = crate::compiled::lane_facts::unplanned_ir(&self.tiers.loops, length);
        if !crate::compiled::lane_facts::promotes_base_body(self.tiers, self, length, completions, ir) { return false; }
        self.tier().history.set(LaneBodyHistory { base_first: false, promoted: true, ..history });
        // SAFETY: as `lane_body`; the run that ended has finished, and nothing reads its body.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        let LaneBody::Specialized { recompiles, stuck, .. } = body else { return false };
        let next = LaneBody::Unspecialized { entries: u32::from(self.tiers.lane_body.warmup()) - 1, recompiles: *recompiles, stuck: *stuck };
        self.retire_body(body, next);
        true
    }

    /// Retires its quick body for the optimizing compile its runs repaid, which starts at its next
    /// entry (`LaneBodyHistory::optimize`); the quick body's direct callers keep running it.
    fn optimize_body(&self) {
        let history = self.tier().history.get();
        self.tier().history.set(LaneBodyHistory { quick_ir: 0, optimize: true, ..history });
        // SAFETY: as `lane_body`; the run that ended has finished, and nothing reads its body.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        let LaneBody::Specialized { recompiles, stuck, .. } = body else { return };
        let next = LaneBody::Unspecialized { entries: u32::from(self.tiers.lane_body.warmup()) - 1, recompiles: *recompiles, stuck: *stuck };
        self.retire_body(body, next);
    }

    /// Notes the level of the compile it plans now: quick, of about `ir` IR instructions, or optimizing.
    pub(crate) fn note_compile_level(&self, quick: bool, ir: usize) {
        let history = self.tier().history.get();
        self.tier().history.set(LaneBodyHistory { quick_ir: if quick { u32::try_from(ir.max(1)).unwrap_or(u32::MAX) } else { 0 }, ..history });
    }

    /// Whether its next compile is the optimizing one its quick body's runs repaid.
    pub(crate) fn optimize_due(&self) -> bool { self.tier().history.get().optimize }

    /// Counts a leave inside this function's body that a native caller's run made, which no
    /// handler entry notes (`native_frames::advance_into_frames`): one add to the cell's word, on the leave.
    pub(crate) fn note_native_leave(&self) { self.entry.note_leave(); }

    /// The entries a compile at its warm-up count `entries` is priced at: those and its retired bodies' runs.
    fn priced_entries(&self, entries: u32) -> u32 {
        entries.saturating_add(self.tier().history.get().runs)
    }

    /// The entries before its next body specializes: the warm-up, doubled per dropped body.
    pub(crate) fn warmup_entries(&self) -> u32 {
        let history = self.tier().history.get();
        // The default mode starts a body on b-jit (`specialize_in_lane`), whose stitch warms as `--b-jit`'s does, where
        // the function has no loop: a loop tiers on its own, b-jit region first, then u-jit once its iterations repay it,
        // and a b-jit body stitched early would run that loop at b-jit's speed in place of its u-jit region.
        let warmup = if !history.promoted && self.loops.is_empty() && !self.tiers.admits_all && crate::compiled::admission::stencil_below() { self.tiers.lane_body.base_warmup() } else { self.tiers.lane_body.warmup() };
        u32::from(warmup) << history.lifetimes.min(MAX_DOUBLINGS)
    }

    /// The bytecodes one entry runs in the interpreter inside its loops: each loop that runs in no
    /// region, its iteration span times its mean trip count (one iteration without a native run).
    pub(crate) fn loop_work(&self) -> u64 {
        self.loops.iter().filter(|site| !site.runs_region()).map(|site| site.span() * site.tier.borrow().mean_trip().unwrap_or(1)).sum()
    }

    /// The native runs of the function's loops that run in a region entered from the lane: each
    /// crossed into native code, which the function's own body would run without a crossing.
    pub(crate) fn region_runs(&self) -> u64 {
        self.loops.iter().filter(|site| site.runs_region()).map(|site| site.tier.borrow().runs()).sum()
    }

    /// Whether a closed loop of the function runs in a region that has run natively and still
    /// enters it: its calls pay that region's entry, which the function's own body would not.
    /// A region that never ran (its compile was refused) makes none of the work native.
    pub(crate) fn enters_region(&self) -> bool {
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
    pub(crate) fn affords_compile(&self) -> bool {
        let history = self.tier().history.get();
        history.lifetimes <= self.tiers.lane_body.max_lifetimes
    }

    /// Warms again a body declined for its length alone, once a caller's compile meets a call to
    /// it: native callers want it whatever its length. Whether it is warming now.
    pub(crate) fn demand_body(&self) -> bool {
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

    /// Its tier state and history, for `QUENCH_TRACE=body-leave`.
    pub(crate) fn tier_note(&self) -> String {
        let h = self.tier().history.get();
        // SAFETY: as `lane_body`; read only.
        let state = match unsafe { &*self.tier().tier_lane.get() } {
            LaneBody::Unspecialized { entries, .. } => format!("unspecialized:{entries}"),
            LaneBody::Compiling { .. } => "compiling".into(),
            LaneBody::Specialized { .. } => "specialized".into(),
            LaneBody::Declined => "declined".into(),
            #[allow(unreachable_patterns)]
            _ => "other".into(),
        };
        format!("{state} short={} unpaid={} demanded={} lifetimes={} declined={:?} audit={}", h.short, h.unpaid, h.demanded, h.lifetimes, self.tier().body_stats.declined.get(), self.tier().body_stats.audit.get().is_some_and(|verdict| verdict.retire))
    }

    /// Native code (a loop region or a body) called this function, which has no body to enter: the
    /// call left that code to run it in the lane, a crossing per call that only its body removes.
    /// Such a caller wants its body whatever its length or entries, as a compiled caller does
    /// (`called_by_bodies`).
    /// A body declined before such a call (unpaid or short) warms again, once: the flag bounds it.
    pub(crate) fn note_native_caller(&self) {
        let history = self.tier().history.get();
        if history.region_called { return; }
        self.tier().history.set(LaneBodyHistory { demanded: true, region_called: true, ..history });
        // SAFETY: as `demand_body`; the lane runs no body of this view here (it has none).
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        if (history.short || history.unpaid) && matches!(body, LaneBody::Declined) {
            *body = LaneBody::Unspecialized { entries: 0, recompiles: 0, stuck: LaneLeaves::NONE };
            self.reclaim_for_tier();
        }
    }

    /// Records that `caller`'s body compiled a call of this function, by its entry or through
    /// the lane's helper: a fact of this view's body, whose publication invalidates it.
    pub(crate) fn note_caller(&self, caller: &LaneView) {
        let caller = std::ptr::from_ref(caller);
        let mut callers = self.tier().callers.borrow_mut();
        if !std::ptr::eq(caller, self) && !callers.contains(&caller) { callers.push(caller); }
    }

    /// Its body published: each caller that compiled against the one before, or against
    /// none, specializes again at its next entry and calls this one by its entry
    /// (`compiled::DirectCallee`). A caller's running body keeps its code (`retired`).
    fn respecialize_callers(&self) {
        let callers = self.tier().callers.take();
        // A caller's code keeps running while it waits to specialize again (a loop it entered
        // never returns to its entry), and calls through the lane's helper until then: its
        // demand outlives this body, so a later recompile of it is still wanted whatever its length.
        if !callers.is_empty() { self.tier().history.set(LaneBodyHistory { demanded: true, ..self.tier().history.get() }); }
        for caller in callers {
            unsafe { &*caller }.respecialize();
        }
    }

    #[inline]
    pub(crate) fn note_late_call(&self) -> bool {
        let history = self.tier().history.get();
        let late_calls = history.late_calls.saturating_add(1);
        self.tier().history.set(LaneBodyHistory { late_calls, ..history });
        late_calls == self.tiers.lane_body.late_call_calls
    }

    /// Counts the late calls from none again: the callee had no entry yet (still compiling).
    pub(crate) fn recount_late_calls(&self) {
        self.tier().history.set(LaneBodyHistory { late_calls: 0, ..self.tier().history.get() });
    }

    /// Whether the late calls counted past the threshold (`note_late_call`), which only a
    /// respecialization resets.
    #[inline]
    pub(crate) fn late_calls_settled(&self) -> bool {
        self.tier().history.get().late_calls > self.tiers.lane_body.late_call_calls
    }

    #[inline]
    pub(crate) fn owns_record(&self, record: super::Ip) -> bool {
        let start = self.records();
        record >= start && record < start.wrapping_add(self.records.len())
    }

    pub(crate) fn respecialize(&self) {
        let history = self.tier().history.get();
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        let LaneBody::Specialized { tier_prologue, recompiles, stuck, .. } = body else { return };
        if history.callee_recompiles >= self.tiers.lane_body.max_callee_recompiles { return; }
        self.credit_runs();
        let history = self.tier().history.get();
        self.tier().history.set(LaneBodyHistory { callee_recompiles: history.callee_recompiles + 1, late_calls: 0, ..history });
        unsafe { (*self.prologue.get()).handler = *tier_prologue };
        let next = LaneBody::Unspecialized { entries: u32::from(self.tiers.lane_body.warmup_entries) - 1, recompiles: *recompiles, stuck: *stuck };
        let body_state = body.tier_state();
        let LaneBody::Specialized { method, .. } = std::mem::replace(body, next) else { unreachable!() };
        quench_runtime::vm::tier_machine::withdraw(&self.entry, body_state, TierEvent::Demote);
        self.tier().retired.borrow_mut().push(method);
    }

    /// A compile a lifecycle edge outran counts as a dropped body: no run
    /// of it took place.
    pub(crate) fn note_dropped_compile(&self) {
        let history = self.tier().history.get();
        self.tier().history.set(LaneBodyHistory { lifetimes: history.lifetimes.saturating_add(1), ..history });
    }

    /// The body a just published method makes of this view.
    pub(crate) fn specialized(&self, entry: Handler, method: std::rc::Rc<quench_runtime::vm::engine_types::PublishedLaneMethod>, recompiles: u8, stuck: LaneLeaves) -> LaneBody {
        // A body whose entry would not pay for itself serves native callers only
        // (`body_call_helper`, direct calls); the lane's own entries keep the records, and it
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
        self.tier().history.set(LaneBodyHistory { optimize: false, ..self.tier().history.get() });
        // Native callers enter a closed body through the view's cell, and leave inside it
        // through the view's pin.
        if let Some(body) = method.lease().filter(|body| body.calls_only_natively()) {
            self.tier().pinned.borrow_mut().push(body);
            quench_runtime::vm::tier_machine::publish(&self.entry, TierState::Compiling, &method);
        }
        self.entry.reset_evidence();
        LaneBody::Specialized { method, tier_prologue,
            leaves_at: Cell::new(LaneLeaves::NONE), completed: Cell::new(false), recompiles, stuck }
    }
}

/// The target of the callee the call at `pc` of `view` last reached itself, by the site's own
/// record (`site_targets_in_lane`): the entry its record links is another callee's once the
/// target cache replaced it.
pub(crate) fn site_target(vm: &super::Vm, view: &LaneView, pc: usize) -> Option<super::ScriptCallTarget> {
    site_targets(vm, view, pc).next()
}

/// Every callee the call at `pc` of `view` reached (`site_targets_in_lane`), by its target, the
/// latest first.
pub(crate) fn site_targets<'a>(vm: &'a super::Vm, view: &LaneView, pc: usize) -> impl Iterator<Item = super::ScriptCallTarget> + 'a {
    super::site_targets_in_lane(vm, view, pc as u32).into_iter().filter(|site| site.through().is_none()).filter_map(|site| {
        // SAFETY: as `Vm::script_call_entry`; the entry is copied before anything resolves a target.
        vm.script_call_entry(site.identity()?).map(|target| unsafe { *target })
    })
}


/// Whether a first compile of `view` waits, before any planning, for callees that still compile: it
/// then calls their bodies by their entries (`compiled::DirectCallee`). A callee that only lacks a
/// body is asked to warm toward one and is not waited for. The fact `admission::route_for_lane` reads.
pub(crate) fn callees_compiling(vm: &super::Vm, view: &LaneView, body: Option<&quench_runtime::bytecode::Function>) -> bool {
    let Some(body) = body else { return false };
    let calls = |pending: fn(&LaneView) -> bool| body.code.iter().enumerate().any(|(pc, packed)| {
        crate::compiled::meaning_compiled::is_call(body.widened(*packed))
            && site_target(vm, view, pc).is_some_and(|target| {
                // SAFETY: a resolved target's view outlives its program.
                target.program() == vm.active_program && !target.view.is_null() && !std::ptr::eq(target.view, view) && pending(unsafe { &*target.view })
            })
    });
    // A callee declined for its length warms toward the body this one calls by its entry
    // (`LaneView::demand_body`); until it publishes, the call takes the helper path, which runs
    // natively too, so nothing waits for it.
    calls(LaneView::demand_body);
    calls(LaneView::compiling)
}

/// Whether a compiling `body` lets this entry pass without looking for its code, counting it
/// down: a compile takes milliseconds and a hot loop enters at each backward jump.
fn awaits_look(body: &LaneBody) -> bool {
    let LaneBody::Compiling { wait, .. } = body else { return false };
    let left = wait.get();
    wait.set(left.saturating_sub(1));
    left > 0
}


