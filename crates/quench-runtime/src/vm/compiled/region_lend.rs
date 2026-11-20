//! A loop region that calls callees by their entries (the direct row) runs on the lane frame's
//! payload, the one every lane body on the thread runs with (`Vm::run_lane_method`): the callee's
//! body pushes its native frame on the lane stack's activations and places its window at the lane
//! stack's values' top, and its own helpers find the frames in the lane stack (`native_frames`) and
//! the payload in the thread's lane frame. The payload the region was set up with takes the lane
//! frame's place for the run, and the dispatch of the run it interrupts (an enclosing body's or
//! region's, which reads it again when this one returns) is put back after it.

use super::*;

/// Binds `payload`'s lane stack words to the room the lane has now (`Vm::stack_words_in_lane`): the tops
/// the callees' windows and frames are pushed through, and, where the lane may make their
/// activations (`direct_calls`), the end of the frames they may nest; none is pushed. A call the
/// region made that ran lane code (`region_call_in_lane`) may have moved the stacks' storage.
pub(super) fn bind_stacks<H: Host>(vm: &mut Vm<H>, payload: &mut JsStepDispatch, direct_calls: bool) {
    vm.stack_words_in_lane(if direct_calls { SHADOW_FRAMES } else { 0 }).bind(payload);
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
    let lane = unsafe { &raw mut (*frame_in_lane()).dispatch };
    // A callee's leave inside a frame it inlined writes that frame's registers to the lane frame's
    // scratch window (`reserve_lane_scratch`), which the lane reads: a lending region inlines
    // nothing, so its payload names the lane's window.
    // SAFETY: as below.
    unsafe { payload.inlined = (*lane).inlined };
    // SAFETY: the frame lives for the thread; no reference into it is held.
    unsafe { std::ptr::swap(payload, lane) };
    lane
}

/// Puts the dispatch `lend` displaced back in the lane frame's place, `payload` receiving the
/// region's. The native frames the region's callees left the run inside stay on the lane stack,
/// which the lane continues in (`native_frames::advance_into_frames`).
///
/// # Safety
/// `payload` is the one `lend` was given, after the run it lent for.
pub(super) unsafe fn reclaim(payload: &mut JsStepDispatch) {
    // SAFETY: as `lend`.
    unsafe { std::ptr::swap(payload, &raw mut (*frame_in_lane()).dispatch) };
}
