//! Whether a script root's lane calls functions in the lane, as every function's does.
//!
//! The general tier compiles only where a VM frame enters a function
//! (`Vm::js_tier_frame_entry`), and it is a test-and-stress tier. Its tests drive it from a
//! script's top level, so they keep the root's calls in VM frames, as the root made them
//! before it called in the lane (`call_from_script_root_in_frames`).

#[cfg(test)]
thread_local! {
    static ROOT_CALLS_IN_FRAMES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether a lane running the frame of `function` calls in the lane: the newest frame's
/// does (`top`), others' never.
#[inline(always)]
pub(super) fn in_lane(top: bool, function: u32) -> bool {
    #[cfg(test)]
    if function == super::super::ROOT_FUNCTION_ID && ROOT_CALLS_IN_FRAMES.get() {
        return false;
    }
    let _ = function;
    top
}

/// Makes this thread's script roots leave their calls to VM frames (`in_lane`).
#[cfg(test)]
pub(in crate::vm) fn call_from_script_root_in_frames() {
    ROOT_CALLS_IN_FRAMES.set(true);
}
