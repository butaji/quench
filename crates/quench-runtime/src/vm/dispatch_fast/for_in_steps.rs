//! `ForInNext` in the lane: the step over the key snapshot a loop took at its start.
use super::*;

script_handler! {
    /// `ForInNext`: the next key of the loop's snapshot into its register, or the jump out of
    /// the loop once the snapshot is exhausted. A snapshot of a shape-keyed object answers
    /// from the cursor alone (`Vm::for_in_step_listed`); any other checks each key's
    /// enumerability as the general step does (`Vm::for_in_next`), which may run a proxy's
    /// traps, so it is made in place beside the activations (`lane_in_place`).
    fn lane_for_in_next<BACKWARD: bool>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let state = r.get(i.b);
        let Some(step) = vm
            .for_in_step_listed(state)
            .or_else(|| lane_in_place(cx, ip, |vm, code| vm.for_in_next(code, state)))
        else {
            return exit(ip);
        };
        // Guest code the step ran may have made the activations frames, whose registers these are.
        let r = unsafe { (*cx).registers };
        match step {
            Some(key) => {
                r.set(i.a, key);
                next!(cx, r, following(ip), Carried::script(cx))
            }
            None => resume!(cx, lane_jump::<H>(r, ip, BACKWARD, Carried::script(cx), None)),
        }
    }
}

script_handler! {
    /// `ForInStart` over an object whose keys the for-in memo's snapshot lists
    /// (`Vm::listed_for_in_keys`): the loop's state over that snapshot, made without the
    /// general step. Any other source takes the general step in place.
    fn lane_for_in_start<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let source = r.get(i.b);
        let Some(keys) = vm.listed_for_in_keys(source) else {
            sibling!(lane_general_step::<H>, r, ip, Carried::script_in(cx, view))
        };
        let state = vm.lane_for_in_state(source, keys);
        // A state may be allocated; a due collection runs at the lane's next safepoint.
        note_allocation(vm);
        r.set(i.a, state);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

/// The handler of a for-in record: the loop's start, or its step (`LANE_LOOP_EXIT`), whose exit
/// jump runs `BACKWARD` or not. It is chosen before the general in-place step, which would make
/// a frameless activation frames for the step's jump.
pub(super) const fn for_in_handler<H: Host, const BACKWARD: bool>(opcode: Op) -> Option<Handler> {
    match opcode.class() {
        Class::ForInStart => Some(lane_for_in_start::<H>),
        _ if opcode.traits().has(crate::bytecode::Traits::LANE_LOOP_EXIT) => Some(lane_for_in_next::<H, BACKWARD>),
        _ => None,
    }
}
