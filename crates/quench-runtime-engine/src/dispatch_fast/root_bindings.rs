//! A root environment's binding slots, which `LoadCapture` and `StoreCapture` records of a
//! module's top-level declarations read and write in place, from a frame or an activation.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// A frame run's depth-0 capture slots, read when its capture walk ran: dense ones become
/// `LaneContext::capture`; whether they are a root environment's, whose binding slots the
/// record may read in place (`root_binding_slot_in_lane`).
pub(crate) fn frame_capture_at_root(cx: *mut LaneContext, frame: usize) -> bool {
    let vm = unsafe { vm(cx) };
    match vm.current_capture_slots(frame) {
        Some((slots, None)) => {
            if let Some(slots) = unsafe { &*slots }.dense_storage() {
                unsafe { (*cx).registers.set_capture(slots) };
            }
            false
        }
        current => current.is_some(),
    }
}
