//! The helpers admitting an inlined callee bound by its function (`lane_region::bindings`).
use super::*;

fn lane_function_matches<H: Host>(vm: &Vm<H>, program: u64, callee: u64, function: u64) -> bool {
    #[cfg(test)] FUNCTION_BINDING_CALLS.with(|count| count.set(count.get() + 1));
    !vm.direct_eval && !vm.parameter_eval && vm.same_realm_user_target(Value::from_bits(callee))
        .is_some_and(|(owner, id, _)| owner == ProgramId::from_raw(program as u32) && u64::from(id) == function)
}

/// Admit the actual closure and complete call depth without entering it.
pub(super) unsafe extern "C" fn lane_function_binding<H: Host>(vm: u64, _residual: u64, program: u64, callee: u64, function: u64, depth: u64) -> u64 {
    // SAFETY: the linked unit owns this VM/program throughout the no-GC operation.
    let vm = unsafe { &*(vm as *const Vm<H>) };
    u64::from(lane_function_matches(vm, program, callee, function)
        && crate::stack::GuestCallGuard::remaining() as u64 >= depth
        && vm.enter_guarded_call().is_some())
}

/// The same binding after closed-loop entry has admitted complete call depth. A match warms
/// the in-loop call target the body's binding probes first (`JsStepDispatch::call_targets`).
pub(super) unsafe extern "C" fn lane_admitted_function_binding<H: Host>(vm: u64, residual: u64, program: u64, callee: u64, function: u64) -> u64 {
    // SAFETY: the region owns the VM; only termination can change asynchronously.
    let (vm, residual) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram)) };
    let matches = lane_function_matches(vm, program, callee, function);
    if matches { vm.in_loop_call_target(residual, Value::from_bits(callee)); }
    u64::from(matches && !vm.execution_interrupt.atomic_flag().load(std::sync::atomic::Ordering::Relaxed))
}
