//! The lane's store to a capture slot.

use super::*;

/// The lane's store of `value` to the capture at `depth` and `slot` past the running
/// function's dense slots (`lane_store_capture`): whether it stored, which a binding the
/// direct store does not take (a `const`, an uninitialized `let`) does not.
pub(super) fn lane_capture_store<H: Host>(cx: *mut LaneContext<H>, depth: u16, slot: u16, value: Value) -> bool {
    let vm = unsafe { vm(cx) };
    let env = unsafe { (*cx).env };
    // A frame run's first depth-0 store finds its slots, tagged when they admit every
    // store (reads leave them untagged: most frame runs store none), then stores through
    // them.
    if depth == 0
        && env.is_deleted()
        && let Some(capture) = lane_frame_capture(cx)
    {
        unsafe { (*cx).capture = capture };
        if let Some(slots) = capture_stores(capture)
            // SAFETY: the running function's dense capture slots, which admit this write
            // (`CAPTURE_STORES`); no collection runs in the lane.
            && unsafe { crate::heap::EnvironmentSlots::write_storage(slots, usize::from(slot), value) }.is_some()
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
        return vm.capture_env(unsafe { (*cx).frame }, depth)
            .is_some_and(|target| vm.direct_store_environment(code, program, target, slot, value));
    }
    lane_environment_store(vm, code, program, env, depth, slot, value)
}

/// `lane_capture_store` of a function running in environment `env` without a frame: its
/// depth-0 capture is `env` itself, stored with one lookup.
pub(super) fn lane_environment_store<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, program: ProgramId, env: Value, depth: u16, slot: u16, value: Value) -> bool {
    if depth == 0 && vm.direct_store_own_environment(code, program, env, slot, value) {
        return true;
    }
    vm.environment_capture_env(env, false, depth)
        .is_some_and(|target| vm.direct_store_environment(code, program, target, slot, value))
}
