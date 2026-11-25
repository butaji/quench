//! A loop body's tier-up leaves and resumes: the poll's unconditional backward jump and the
//! header that dispatches the body a tier-up published.

#[allow(unused_imports)]
use crate::ext_traits::*;
use crate::dispatch_fast::{vm, LaneView, LoopSite};
use quench_runtime::vm::engine_types::{LaneContext, LoopBody};
use quench_runtime::vm::Vm;

/// The running regions leave at their next backward jump (`left_for_tier_up`), and the header
/// dispatch (`enter_loop_body`) takes the body that published.
pub(crate) fn raise_tier_up(vm: &Vm, site: &LoopSite) {
    vm.execution_interrupt.request_tier_up();
    if site.diagnostics { site.tier_ups.set(site.tier_ups.get().saturating_add(1)); }
}

/// Whether a region's exit at `pc` is its poll's tier-up leave: at the loop's unconditional
/// backward jump (no guard of it can fail) while a tier-up is requested and no termination,
/// which the jump's own safepoint ends. The lane resumes at the header, which dispatches.
pub(crate) fn left_for_tier_up(cx: *mut LaneContext, site: &LoopSite, pc: u32) -> bool {
    let vm = unsafe { vm(cx) };
    if pc != site.backedge || !vm.execution_interrupt.tier_up_requested() || vm.execution_interrupt.is_termination_requested() {
        return false;
    }
    vm.programs.get(vm.active_program).as_deref().is_some_and(|program| {
        let body = program.function(site.function() as usize);
        body.code.get(pc as usize).is_some_and(|packed| {
            matches!(quench_runtime::staged::Operation::<u64, ()>::decode(body.widened(*packed), quench_runtime::staged::DecodeContext::EMPTY),
                Some(quench_runtime::staged::Operation::Jump))
        })
    })
}

/// Where a body that left at `pc` resumes when a tier-up is pending: the header of the loop
/// whose backward jump `pc` is, which dispatches the ready body, once that header runs the
/// dispatch (`held`); otherwise the leave's own PC, whose jump record dispatches it.
const fn tier_up_resume(header: u32, backedge: u32, pc: u32, held: bool, ready: bool) -> Option<u32> {
    if pc == backedge && held && ready { Some(header) } else { None }
}

/// The leave of any body at a loop's backward jump (`LaneExit::At(backedge)`, whichever tier
/// compiled it): while a tier-up is requested for a loop whose body has published, the lane
/// resumes at that loop's header, as a region's leave does (`left_for_tier_up`). The leaving
/// body stays published for its other entries.
pub(crate) fn header_for_tier_up(cx: *mut LaneContext, own: &LaneView, pc: usize) -> Option<usize> {
    let pc = u32::try_from(pc).ok()?;
    let at = own.loop_at_backedge.binary_search_by_key(&pc, |&(backedge, _)| backedge).ok()?;
    let site = &own.loops[own.loop_at_backedge[at].1 as usize];
    if !left_for_tier_up(cx, site, pc) { return None; }
    let ready = matches!(*site.body.borrow(), LoopBody::Ready { .. });
    tier_up_resume(site.header, site.backedge, pc, !site.header_handler.get().is_null() && site.header_handler.get() != crate::dispatch_fast::RETIRED_HEADER, ready).map(|header| header as usize)
}
