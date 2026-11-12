//! A lane body's native catch in a frame (`Lowering::bind_caught`): the frame state the
//! interpreter's handler entry settles (`Vm::unwind_handler_scopes`, `Vm::bind_thrown_value`)
//! that native stores cannot, made by the interpreter's own definition.
use super::*;

/// Drops what the lane caches of the running frame's environment, which a copy of it replaced
/// (`Vm::clone_frame_environment`): its captures, its outer walk and its storage.
pub(super) fn drop_environment_caches<H: Host>(cx: *mut LaneContext<H>) {
    // SAFETY: the running lane's context.
    unsafe {
        (*cx).capture = std::ptr::null();
        (*cx).outer = NO_OUTER_WALK;
        (*cx).frame_storage = NO_FRAME_STORAGE;
    }
}

/// The entry of the handler covering record `call_pc` of the running frame's function, for the
/// thrown `value`: the `with` scopes past the handler's unwound and, for a binding in a frame
/// local, the local stored as the interpreter stores it, a captured frame getting a fresh
/// environment per entry, whose copy replaces the one the lane's caches name. A promoted
/// binding is the body's own register. Only a function that runs in a frame reaches this
/// (`catches::frame_bound`), so the lane context's frame is the function's.
pub(in crate::vm) unsafe extern "C" fn lane_handler_edge<H: Host>(
    _vm: u64,
    _residual: u64,
    context: u64,
    call_pc: u64,
    value: u64,
) -> u64 {
    let cx = context as usize as *mut LaneContext<H>;
    let vm = unsafe { vm(cx) };
    let frame = unsafe { (*cx).frame };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let handler = code
        .function(vm.frames[frame].function as usize)
        .handler_at(call_pc as u32)
        .expect("a native catch's record has its handler");
    vm.unwind_handler_scopes(frame, handler);
    if let Some(slot @ crate::bytecode::ThrownValueSlot::Local(_)) = handler.slot {
        vm.bind_thrown_value(code, frame, slot, Value::from_bits(value))
            .expect("a frame's handler binding is valid");
        note_allocation(vm);
        drop_environment_caches(cx);
    }
    super::super::compiled::LANE_CALL_COMPLETED
}
