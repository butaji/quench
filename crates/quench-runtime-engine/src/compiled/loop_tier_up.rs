//! A loop body's tier-up request: the running body's leave at its poll and the header dispatch
//! that takes the body a tier-up published.

#[allow(unused_imports)]
use crate::ext_traits::*;
use crate::dispatch_fast::LoopSite;

use quench_runtime::vm::Vm;

/// The running bodies leave at their next backward jump, and the backward jump's dispatch
/// (`enter_loop_body`) takes the body that published.
pub(crate) fn raise_tier_up(vm: &Vm, site: &LoopSite) {
    vm.execution_interrupt.request_tier_up();
    if site.diagnostics { site.tier_ups.set(site.tier_ups.get().saturating_add(1)); }
}
