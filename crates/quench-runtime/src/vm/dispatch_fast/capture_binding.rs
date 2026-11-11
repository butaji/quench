//! `LoadCapture` of a binding in the activation's own environment, resolved once per record.
use super::*;

script_handler! {
    /// A `LoadCapture` of a binding in the activation's own environment, which
    /// `lane_load_capture_walk` found: the binding at the record's own depth and slot, read
    /// without the root's slot rule the walk resolves each read. A miss (a frame's
    /// environment, a deleted binding) walks, which reads the same immediate.
    fn lane_load_root_binding<>(cx, r, ip, i, view) {
        let env = unsafe { (*cx).env };
        let imm = i.imm();
        let (slot, depth) = (ImmediateLayout::capture_slot(imm), ImmediateLayout::capture_depth(imm));
        let vm = unsafe { vm(cx) };
        let slots = (!env.is_deleted())
            .then(|| vm.walked_capture_slots(env, false, depth))
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

/// Make the `LoadCapture` record at `ip` read its binding directly
/// (`lane_load_root_binding`): the walk found it, so the record needs no slot rule. Its
/// immediate keeps the read's depth and slot, which the handler and the walk a miss takes
/// both read.
pub(super) fn requicken_root_binding<H: Host>(ip: Ip) {
    requicken::<H>(ip, lane_load_root_binding::<H>);
}
