//! A root environment's binding slots, which `LoadCapture` and `StoreCapture` records of a
//! module's top-level declarations read and write in place, from a frame or an activation.
use super::*;

/// A frame run's depth-0 capture slots, read when its capture walk ran: dense ones of a
/// function's own (non-root) environment become `LaneContext::capture`.
pub(super) fn frame_capture_at_root<H: Host>(cx: *mut LaneContext<H>, frame: usize) {
    let vm = unsafe { vm(cx) };
    if let Some((slots, None)) = vm.current_capture_slots(frame) {
        if let Some(slots) = unsafe { &*slots }.dense_storage() {
            unsafe { (*cx).registers.set_capture(slots) };
        }
    }
}

