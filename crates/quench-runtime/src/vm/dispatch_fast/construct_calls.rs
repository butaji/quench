//! Construct records a plan answers without activating the callee.
use super::*;

script_handler! {
    /// `lane_script_construct` at a site into a register whose construction was planned
    /// (`lane_planned_construction`), which `lane_script_construct` moves it to: the plan of
    /// the callee alone, without resolving its call target. A plan exists only for a callee
    /// whose target constructs by its initializer stores, which its function fixes, and while
    /// it holds the finished object is what the activation would make. Any other construction
    /// moves the record back to `lane_script_construct`.
    fn lane_planned_construct<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !unsafe { (*cx).script_calls && (*cx).script_constructs } || vm.construct_target.is_some() {
            return exit(ip);
        }
        let Some(this) = lane_planned_construction(vm, r.get(i.b), unsafe { &*(*cx).code }, r, i) else {
            requicken::<H>(ip, lane_script_construct::<H>);
            sibling!(lane_script_construct::<H>, r, ip, Carried::script(cx))
        };
        // Allocating the receiver can make a collection due.
        note_allocation(vm);
        r.set(i.a, this);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_planned_construct` at a site whose construction is returned (`RETURN_REGISTER`),
    /// which `lane_script_construct` moves it to: the planned receiver completes the
    /// activation that returns it, or the frame's own returning construct where
    /// `lane_finishes_construction` admits it. A leaf's construction completes the leaf's call
    /// instead (`lane_complete_leaf`). Any other construction moves the record back to
    /// `lane_script_construct`.
    fn lane_planned_construct_returned<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !unsafe { (*cx).script_calls && (*cx).script_constructs } || vm.construct_target.is_some() {
            return exit(ip);
        }
        let planned = lane_finishes_construction(cx, i)
            .then(|| lane_planned_construction(vm, r.get(i.b), unsafe { &*(*cx).code }, r, i))
            .flatten();
        let Some(this) = planned else {
            requicken::<H>(ip, lane_script_construct::<H>);
            sibling!(lane_script_construct::<H>, r, ip, Carried::script(cx))
        };
        // Allocating the receiver can make a collection due.
        note_allocation(vm);
        // A leaf's construction is the call's completion: the receiver it returns.
        if unsafe { !(*cx).leaf.call.is_null() } {
            let target = unsafe { (*cx).leaf.target.as_ref() };
            let form = target.map_or(NOT_LEAF, |target| target.entry.leaf_form());
            if let Some((registers, next)) = lane_complete_leaf::<H>(cx, this, form) {
                let view = Carried {
                    memory: MemoryView::script(cx),
                    integers: crate::value::IntegerEncoding::TAG,
                    acc: Slot::of(Value::UNDEFINED),
                };
                resume!(cx, Resumed::At(registers, next, view))
            }
        }
        if let Some(resumed) = lane_complete_activation(cx, ip, this) {
            resume!(cx, resumed)
        }
        lane_frame_return(vm, ip, this)
    }
}

/// `lane_planned_construct`, as the handler `lane_script_construct` requickens a site to.
pub(super) fn planned_construct_handler<H: Host>() -> Handler {
    lane_planned_construct::<H>
}

/// `lane_planned_construct_returned`, as the handler `lane_script_construct` requickens a
/// returning site to.
pub(super) fn planned_construct_returned_handler<H: Host>() -> Handler {
    lane_planned_construct_returned::<H>
}
