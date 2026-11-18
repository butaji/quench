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

/// The handler of a `ForInNext` record whose exit jump runs `BACKWARD` or not.
pub(super) const fn for_in_next_handler<H: Host, const BACKWARD: bool>() -> Handler {
    lane_for_in_next::<H, BACKWARD>
}
