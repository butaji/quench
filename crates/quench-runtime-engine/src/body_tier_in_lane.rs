//! A lane view's body as it is published and entered: the view's own publication and the
//! callers that name it. A lane leave is a deoptimization to the interpreter, not feedback.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::tier_machine::TierState;
use super::{Handler, LaneBody, LaneView};

pub(crate) use quench_runtime::vm::engine_types::LaneBodyHistory;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;


#[path = "lane_body_specialize.rs"]
pub(crate) mod specialize;

#[quench_macros::ext(LaneViewOpsX1)]
impl LaneView {
    /// Whether the view's body is retired to `Declined` (`LaneBody::Declined`): a call of the view has no body to enter.
    pub(crate) fn declined_in_lane(&self) -> bool {
        // SAFETY: as `body_view_in_lane`; read only.
        matches!(unsafe { &*self.tier().tier_lane.get() }, LaneBody::Declined)
    }

    /// Warms again a body declined for its length alone, once a caller's compile meets a call to
    /// it: native callers want it whatever its length. Whether it is warming now.
    pub(crate) fn demand_body(&self) -> bool {
        let history = self.tier().history.get();
        // SAFETY: as `lane_body`; no lane body runs while a compile plans.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        if history.unpaid && !history.demanded && matches!(body, LaneBody::Declined) {
            self.tier().history.set(LaneBodyHistory { demanded: true, ..history });
            *body = LaneBody::Unspecialized;
            self.reclaim_for_tier();
        }
        self.warming()
    }

    /// Its tier state and history, for `QUENCH_TRACE=body-leave`.
    pub(crate) fn tier_note(&self) -> String {
        let h = self.tier().history.get();
        // SAFETY: as `lane_body`; read only.
        let state = match unsafe { &*self.tier().tier_lane.get() } {
            LaneBody::Unspecialized => "unspecialized",
            LaneBody::Compiling { .. } => "compiling",
            LaneBody::Specialized { .. } => "specialized",
            LaneBody::Declined => "declined",
        };
        format!("{state} unpaid={} demanded={} declined={:?}", h.unpaid, h.demanded, self.tier().body_stats.declined.get())
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
        if history.unpaid && matches!(body, LaneBody::Declined) {
            *body = LaneBody::Unspecialized;
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

    #[inline]
    pub(crate) fn owns_record(&self, record: super::Ip) -> bool {
        let start = self.records();
        record >= start && record < start.wrapping_add(self.records.len())
    }

    /// The body a just published method makes of this view.
    pub(crate) fn specialized(&self, entry: Handler, method: std::rc::Rc<quench_runtime::vm::engine_types::PublishedLaneMethod>, tier: quench_runtime::vm::tier_policy::Tier) -> LaneBody {
        // SAFETY: as `claim_for_tier`; no handler reads the prologue now.
        let tier_prologue = unsafe { std::mem::replace(&mut (*self.prologue.get()).handler, entry as *const ()) };
        // Native callers enter a closed body through the view's cell, and leave inside it
        // through the view's pin.
        if let Some(body) = method.lease().filter(|body| body.calls_only_natively()) {
            self.tier().pinned.borrow_mut().push(body);
            quench_runtime::vm::tier_machine::publish(&self.entry, TierState::Compiling, tier, &method);
        }
        LaneBody::Specialized { method, tier_prologue }
    }
}

/// Whether a first compile of `view` waits, before any planning, for callees that still compile: it
/// then calls their bodies by their entries (`compiled::DirectArm`). A callee that only lacks a
/// body is asked to warm toward one and is not waited for. The fact `admission::route_for_lane` reads.
pub(crate) fn callees_compiling(vm: &super::Vm, view: &LaneView, function: u32, body: Option<&quench_runtime::bytecode::Function>) -> bool {
    let Some(body) = body else { return false };
    // The callees are the call's proven targets (`FunctionFacts::proven_call_targets`), each by its view in the running program.
    let Some(facts) = vm.programs.aot_facts(vm.active_program) else { return false };
    let (views, _) = vm.programs.lane_tables(vm.active_program);
    let calls = |pending: fn(&LaneView) -> bool| body.code.iter().enumerate().any(|(pc, packed)| {
        crate::compiled::meaning_compiled::is_call(body.widened(*packed))
            && facts.function_facts(function).proven_call_targets(pc as u32).into_iter().any(|callee| {
                views.get(callee as usize).and_then(quench_runtime::vm::program_store::derived_lane_view)
                    .is_some_and(|target| !std::ptr::eq(target, view) && pending(target))
            })
    });
    // A callee declined for its length warms toward the body this one calls by its entry
    // (`LaneView::demand_body`); until it publishes, the call takes the helper path, which runs
    // natively too, so nothing waits for it.
    calls(LaneView::demand_body);
    calls(LaneView::compiling)
}



