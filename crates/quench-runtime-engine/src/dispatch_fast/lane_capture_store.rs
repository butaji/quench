//! The lane's store to a capture slot.

use super::*;

/// The lane's store of `value` to the capture at `depth` and `slot` past the running
/// function's dense slots (`lane_store_capture`): whether it stored, which a binding the
/// direct store does not take (a `const`, an uninitialized `let`) does not.
pub(super) fn store_to_capture(cx: *mut LaneContext, depth: u16, slot: u16, value: Value) -> bool {
    let vm = unsafe { vm(cx) };
    let env = unsafe { (*cx).registers }.env();
    // A frame run's first depth-0 store finds its slots, tagged when they admit every
    // store (reads leave them untagged: most frame runs store none), then stores through
    // them.
    if depth == 0
        && env.is_deleted()
        && let Some(capture) = frame_capture_in_lane(cx)
    {
        unsafe { (*cx).registers.set_capture(capture) };
        if let Some(slots) = capture_stores(capture)
            // SAFETY: the running function's dense capture slots, which admit this write
            // (`CAPTURE_STORES`); no collection runs in the lane.
            && unsafe { quench_runtime::heap::EnvironmentSlots::write_storage(slots, usize::from(slot), value) }.is_some()
        {
            return true;
        }
    }
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let program = unsafe { (*cx).program };
    // A frameless activation's function has no environment of its own: its depth-0
    // capture is the environment it runs in, stored with one lookup.
    if env.is_deleted() {
        return vm.owner_capture_env(VarOwner::Frame(unsafe { (*cx).frame() }), depth)
            .is_some_and(|target| vm.direct_store_environment(code, program, target, slot, value));
    }
    store_to_environment(vm, code, program, env, depth, slot, value)
}

/// `store_to_capture` of a function running in environment `env` without a frame: its
/// depth-0 capture is `env` itself, stored with one lookup.
pub(super) fn store_to_environment(vm: &mut Vm, code: &ResidualProgram, program: ProgramId, env: Value, depth: u16, slot: u16, value: Value) -> bool {
    if depth == 0 && vm.direct_store_own_environment(code, program, env, slot, value) {
        return true;
    }
    vm.environment_capture_env(env, false, depth)
        .is_some_and(|target| vm.direct_store_environment(code, program, target, slot, value))
}
