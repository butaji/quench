//! When a lane body compiles again, keeps its code or retires: decisions from what its runs
//! and its predecessors' did.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::tier_machine::{TierEvent, TierState};
use super::{Handler, LaneBody, LaneView, Tiers};
use crate::compiled::LaneExit;

pub(crate) use quench_runtime::vm::engine_types::LaneBodyHistory;


#[path = "lane_body_specialize.rs"]
pub(crate) mod specialize;

/// The completed handler runs of a body per credit to the callees it calls natively: a callee's
/// completions inside those runs are bounded by the caller's, so the credit lags by fewer than this.
const NATIVE_CREDIT_RUNS: u64 = 64;

/// Credits the callees `method`'s body calls natively with `NATIVE_CREDIT_RUNS` completions
/// (a bound, as the caller's completed runs need not reach each call).
fn credit_native_callees(method: &quench_runtime::vm::engine_types::PublishedLaneMethod) {
    let Some(body) = method.lease() else { return };
    for callee in body.direct.iter() {
        // SAFETY: the caller's body owns the callee it named; the view outlives the run.
        unsafe { &*callee.target.view }.entry.note_completions(NATIVE_CREDIT_RUNS);
    }
}

#[quench_macros::ext(LaneViewOpsX1)]
impl LaneView {
    /// Whether the view's body is retired to `Declined` (`LaneBody::Declined`): a call of the view has no body to enter.
    pub(crate) fn declined_in_lane(&self) -> bool {
        // SAFETY: as `body_view_in_lane`; read only.
        matches!(unsafe { &*self.tier().tier_lane.get() }, LaneBody::Declined)
    }

    /// Counts how a run of the specialized body ended, at `exit`: a completed run is the body's
    /// evidence of completion. A leave is a deoptimization to the interpreter at its place; it
    /// feeds no decision about the body.
    pub(crate) fn note_lane_body_end(&self, exit: LaneExit, completed_region: bool) {
        // SAFETY: as `lane_body`; the run that ended has finished, and nothing reads its body.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        let LaneBody::Specialized { completed, method, .. } = body else { return };
        if completed_region || exit.completes() {
            completed.set(true);
            let completions = self.entry.note_completions(1);
            if completions % NATIVE_CREDIT_RUNS == 0 { credit_native_callees(method); }
        }
    }

    /// Counts a leave inside this function's body that a native caller's run made, which no
    /// handler entry notes (`native_frames::advance_into_frames`): one add to the cell's word, on the leave.
    pub(crate) fn note_native_leave(&self) { self.entry.note_leave(); }

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
            *body = LaneBody::Unspecialized { recompiles: 0 };
            self.reclaim_for_tier();
        }
        self.warming()
    }

    /// Its tier state and history, for `QUENCH_TRACE=body-leave`.
    pub(crate) fn tier_note(&self) -> String {
        let h = self.tier().history.get();
        // SAFETY: as `lane_body`; read only.
        let state = match unsafe { &*self.tier().tier_lane.get() } {
            LaneBody::Unspecialized { .. } => "unspecialized".into(),
            LaneBody::Compiling { .. } => "compiling".into(),
            LaneBody::Specialized { .. } => "specialized".into(),
            LaneBody::Declined => "declined".into(),
            #[allow(unreachable_patterns)]
            _ => "other".into(),
        };
        format!("{state} short={} unpaid={} demanded={} lifetimes={} declined={:?}", h.short, h.unpaid, h.demanded, h.lifetimes, self.tier().body_stats.declined.get())
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
            *body = LaneBody::Unspecialized { recompiles: 0 };
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
        let LaneBody::Specialized { tier_prologue, recompiles, .. } = body else { return };
        if history.callee_recompiles >= self.tiers.lane_body.max_callee_recompiles { return; }
        let history = self.tier().history.get();
        self.tier().history.set(LaneBodyHistory { callee_recompiles: history.callee_recompiles + 1, late_calls: 0, ..history });
        unsafe { (*self.prologue.get()).handler = *tier_prologue };
        let next = LaneBody::Unspecialized { recompiles: *recompiles };
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
    pub(crate) fn specialized(&self, entry: Handler, method: std::rc::Rc<quench_runtime::vm::engine_types::PublishedLaneMethod>, tier: quench_runtime::vm::tier_policy::Tier, recompiles: u8) -> LaneBody {
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
        // Native callers enter a closed body through the view's cell, and leave inside it
        // through the view's pin.
        if let Some(body) = method.lease().filter(|body| body.calls_only_natively()) {
            self.tier().pinned.borrow_mut().push(body);
            quench_runtime::vm::tier_machine::publish(&self.entry, TierState::Compiling, tier, &method);
        }
        self.entry.reset_evidence();
        LaneBody::Specialized { method, tier_prologue, completed: Cell::new(false), recompiles }
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


