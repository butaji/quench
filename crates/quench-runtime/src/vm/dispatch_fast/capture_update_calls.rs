//! Plain calls of a capture leaf whose body is a fused int32 update returning the value it
//! stores, answered by the call record without entering the leaf.
use super::*;

impl LaneView {
    /// Whether the body is an `UPDATE_LOADS` update followed by a `Return` of its read's
    /// register, as `function () { c = (c + 1) | 0; return c; }` compiles, whose plain call the
    /// call record answers (`lane_script_call_update_capture`). The record after the run says
    /// so, not the update's handler: the forms' handlers share one body, which the linker may
    /// fold into one address.
    pub(super) fn update_returns<H: Host>(&self) -> bool {
        // SAFETY: as `requicken`: no reference to a record's handler field is live.
        let record = |at: usize| self.records.get(at).map(|record| unsafe { &*record.get() });
        let (Some(read), Some(done)) = (record(UPDATE_LOADS_RUN - 1), record(UPDATE_LOADS_RUN)) else {
            return false;
        };
        let returns = [lane_return::<H, NOT_LEAF> as Handler as *const (), lane_return::<H, CAPTURE_LEAF> as Handler as *const ()];
        returns.contains(&done.handler) && done.a == read.a
    }
}

/// The call handler of a plain call record `i` whose target is a capture leaf whose body is a
/// fused capture update returning the value it stores (`LaneView::update_returns`):
/// `lane_script_call_update_capture` of its operator; `None` for any other target or a call
/// returning its completion.
pub(super) fn capture_update_call_handler<H: Host>(target: &ScriptCallTarget, i: &LaneInstruction) -> Option<Handler> {
    use super::operations::{ADDITION, SUBTRACTION};
    if target.entry != CallEntry::CaptureLeaf || i.a & crate::bytecode::RETURN_REGISTER != 0 {
        return None;
    }
    // SAFETY: the program store keeps a derived view alive with its program.
    let view = unsafe { &*target.view };
    let first = unsafe { &*view.records() }.handler;
    if !view.update_returns::<H>() {
        None
    } else if first == lane_update_capture_int32::<H, ADDITION, UPDATE_LOADS> as Handler as *const () {
        Some(lane_script_call_update_capture::<H, ADDITION>)
    } else if first == lane_update_capture_int32::<H, SUBTRACTION, UPDATE_LOADS> as Handler as *const () {
        Some(lane_script_call_update_capture::<H, SUBTRACTION>)
    } else {
        None
    }
}

script_handler! {
    /// A plain call of a capture leaf whose body is a fused update of `OP` returning the value
    /// it stores (`capture_update_call_handler`): the update of the target's captures
    /// (`capture_int32_update`), the call's value, as the leaf's records would make it, without
    /// entering them. The body reads no parameter, receiver or frame state and runs no guest
    /// code, so nothing observes the call it does not make. Another target takes
    /// `lane_script_call_requicken`, an update its record declines `lane_script_call_full`.
    fn lane_script_call_update_capture<OP: u32>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        let target = if unsafe { (*cx).script_calls } && !lane_constructing(vm, callee) {
            vm.linked_script_call_entry(callee, &i.call_link)
        } else {
            None
        };
        // SAFETY: as `Vm::script_call_entry`; the update resolves no target and collects nothing.
        let Some(target) = target.map(|target| unsafe { &*target }) else {
            sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
        };
        if target.entry != CallEntry::CaptureLeaf {
            sibling!(lane_script_call_requicken::<H>, r, ip, Carried::script(cx))
        }
        // SAFETY: the program store keeps a derived view alive with its program.
        let view = unsafe { &*target.view };
        let update = unsafe { &*view.records() };
        if update.handler != lane_update_capture_int32::<H, OP, UPDATE_LOADS> as Handler as *const ()
            || !view.update_returns::<H>()
        {
            sibling!(lane_script_call_requicken::<H>, r, ip, Carried::script(cx))
        }
        let Some((_, value)) = capture_int32_update::<OP>(target.capture, update) else {
            sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}
