//! `LoadCapture` of a binding in the activation's own environment, resolved once per record.
use super::*;

script_handler! {
    /// A `LoadCapture` of a binding in the activation's own environment, which
    /// `lane_load_capture_walk` found and kept in the record's hint slot: the value at that
    /// slot, read without the root's slot rule the walk resolves each read.
    fn lane_load_root_binding<>(cx, r, ip, i, view) {
        let env = unsafe { (*cx).env };
        let slot = i.field_hint().slot;
        let vm = unsafe { vm(cx) };
        let slots = (!env.is_deleted())
            .then(|| vm.walked_capture_slots(env, false, 0))
            .flatten();
        // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
        let value = slots.and_then(|(slots, _)| vm.slot_binding(unsafe { &*slots }, slot));
        let Some(value) = value else {
            sibling!(lane_load_capture_walk::<H>, r, ip, view)
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

/// Make the `LoadCapture` record at `ip` read slot `slot` of its activation's environment
/// directly (`lane_load_root_binding`): the walk found the binding and needs no slot rule.
pub(super) fn requicken_root_binding<H: Host>(ip: Ip, slot: u16) {
    set_field_hint(ip, super::FieldCache::own(0, slot));
    requicken::<H>(ip, lane_load_root_binding::<H>);
}
