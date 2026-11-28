//! The helpers admitting an inlined callee bound by its function (`lane_region::bindings`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

fn function_matches_in_lane(vm: &Vm, program: u64, callee: Value, function: u64) -> bool {
    #[cfg(test)] FUNCTION_BINDING_CALLS.with(|count| count.set(count.get() + 1));
    !vm.direct_eval && !vm.parameter_eval && vm.same_realm_user_target(callee)
        .is_some_and(|(owner, id, _)| owner == ProgramId::from_raw(program as u32) && u64::from(id) == function)
}

/// Admit the actual closure and complete call depth without entering it.
pub(crate) unsafe extern "C" fn function_binding_in_lane(vm: u64, _residual: u64, program: u64, callee: Value, function: u64, depth: u64) -> u64 {
    // SAFETY: the linked unit owns this VM/program throughout the no-GC operation.
    let vm = unsafe { &*(vm as *const Vm) };
    u64::from(function_matches_in_lane(vm, program, callee, function)
        && quench_runtime::stack::GuestCallGuard::remaining() as u64 >= depth
        && vm.enter_guarded_call().is_some())
}

/// The same binding after closed-loop entry has admitted complete call depth. A match warms
/// the in-loop call target the body's binding probes first (`JsStepDispatch::call_targets`).
pub(crate) unsafe extern "C" fn admitted_function_binding_in_lane(vm: u64, residual: u64, program: u64, callee: Value, function: u64) -> u64 {
    // SAFETY: the region owns the VM; only termination can change asynchronously.
    let (vm, residual) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram)) };
    let matches = function_matches_in_lane(vm, program, callee, function);
    if matches { vm.in_loop_call_target(residual, callee); }
    u64::from(matches && !vm.execution_interrupt.atomic_flag().load(std::sync::atomic::Ordering::Relaxed))
}
