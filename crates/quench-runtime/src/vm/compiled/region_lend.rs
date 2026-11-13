//! A loop region that calls callees by their entries (the direct row) runs on the lane frame's
//! payload, the one every lane body on the thread runs with (`Vm::run_lane_method`): the callee's body
//! pushes its native frame on the payload's shadow stack and places its window at the lane stack's
//! top, and its own helpers find them in the thread's lane frame (`Vm::innermost_shadow_frame`). The
//! payload the region was set up with takes the lane frame's place for the run, and the dispatch of the
//! run it interrupts (an enclosing body's or region's, which reads it again when this one returns) is
//! put back after it.

use super::*;

/// Binds `payload`'s stacks to the room the lane has now (`Vm::lane_stack_spare`): the lane stack's
/// free values, where the callees' windows go, and, where the lane may make their activations
/// (`direct_calls`), the shadow frames; no frame is pushed. A call the region made that ran lane code
/// (`lane_region_call`) moved both.
pub(super) fn bind_stacks<H: Host>(vm: &mut Vm<H>, payload: &mut JsStepDispatch, direct_calls: bool) {
    let (top, end, activations, shadow) = vm.lane_stack_spare();
    let frames = if direct_calls { SHADOW_FRAMES.min(activations) } else { 0 };
    (payload.native_top, payload.native_end) = (top.addr(), end.addr());
    // SAFETY: `lane_stack_spare` bounds the shadow stack by the activations' room.
    (payload.shadow_base, payload.shadow_top, payload.shadow_end) = (shadow.addr(), shadow.addr(), unsafe { shadow.add(frames) }.addr());
}

/// Gives the lane frame's place to `payload`, a region's, with the room a lane body's direct calls
/// have (`bind_stacks`); returns the lane frame's payload, which is now the region's, and which the
/// region's rows and its callees' read.
///
/// # Safety
/// As `Vm::run_lane_method`: the region runs on the lane's activation at the lane stack's top, and
/// `reclaim` follows the run.
pub(super) unsafe fn lend<H: Host>(vm: &mut Vm<H>, payload: &mut JsStepDispatch, direct_calls: bool) -> *mut JsStepDispatch {
    bind_stacks(vm, payload, direct_calls);
    let lane = unsafe { &raw mut (*lane_frame()).dispatch };
    // SAFETY: the frame lives for the thread; no reference into it is held.
    unsafe { std::ptr::swap(payload, lane) };
    lane
}

/// Takes the native frames the region's callees left the run inside (`Vm::take_shadow_frames`), and
/// puts the dispatch `lend` displaced back in the lane frame's place, `payload` receiving the region's.
///
/// # Safety
/// `payload` is the one `lend` was given, after the run it lent for.
pub(super) unsafe fn reclaim<H: Host>(payload: &mut JsStepDispatch) -> ShadowFrames {
    let frames = Vm::<H>::take_shadow_frames();
    // SAFETY: as `lend`.
    unsafe { std::ptr::swap(payload, &raw mut (*lane_frame()).dispatch) };
    frames
}
