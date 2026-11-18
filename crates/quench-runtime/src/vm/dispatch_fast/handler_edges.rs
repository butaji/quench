//! A native catch's frame state (`Lowering::bind_caught`): what the interpreter's handler entry
//! settles (`Vm::unwind_handler_scopes`, `Vm::bind_thrown_value`) that native stores cannot, made
//! by those operations over the home of the running function's state (`HandlerHome`): its VM
//! frame, or its activation's window.
use super::*;
use super::super::handler_entry::{HandlerHome, WindowHome};
use crate::bytecode::ThrownValueSlot;

/// Drops what the lane caches of the running frame's environment, which a copy of it replaced
/// (`Vm::clone_frame_environment`): its captures, its outer walk and its storage.
pub(super) fn drop_environment_caches<H: Host>(cx: *mut LaneContext<H>) {
    // SAFETY: the running lane's context.
    unsafe {
        (*cx).registers.set_capture(std::ptr::null());
        (*cx).outer = NO_OUTER_WALK;
        (*cx).root_walk = NO_OUTER_WALK;
        (*cx).frame_storage = NO_FRAME_STORAGE;
    }
}

/// The running function, the home a handler's scopes and binding live in, and whether the running
/// function keeps its locals in its record: the innermost activation's, else the frame's.
fn handler_home<H: Host>(cx: *mut LaneContext<H>, vm: &Vm<H>) -> (u32, HandlerHome, bool) {
    match vm.lane_stack.activations.last().copied() {
        Some(activation) => {
            let home = HandlerHome::Window(WindowHome {
                depth: vm.lane_stack.activations.len() - 1,
                program: unsafe { (*cx).program },
                function: activation.function,
                window: activation.window_ptr(),
            });
            (activation.function, home, activation.script().is_some_and(|script| script.environment))
        }
        None => {
            let frame = unsafe { (*cx).frame() };
            (vm.frames[frame].function, HandlerHome::Frame(frame), true)
        }
    }
}

/// The entry of the handler covering record `call_pc` of the running function, for the thrown
/// `value`: the `with` scopes past the handler's unwound and, for a binding in a local, the local
/// stored where the running function keeps it (`Vm::bind_thrown_value`), a captured one getting a
/// fresh environment per entry, whose copy replaces the one the lane's caches name in a frame.
/// A promoted binding is the body's own register. The running function is the innermost
/// activation's, else the frame's.
pub(in crate::vm) unsafe extern "C" fn lane_handler_edge<H: Host>(
    _vm: u64,
    _residual: u64,
    context: u64,
    call_pc: u64,
    value: u64,
) -> u64 {
    let cx = context as usize as *mut LaneContext<H>;
    let (call_pc, value) = (call_pc as u32, Value::from_bits(value));
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let (function, home, keeps_locals) = handler_home(cx, vm);
    let handler = code
        .function(function as usize)
        .handler_at(call_pc)
        .expect("a native catch's record has its handler");
    vm.unwind_handler_scopes(home, handler);
    if let Some(slot @ ThrownValueSlot::Local(_)) = handler.slot
        && keeps_locals
    {
        vm.bind_thrown_value(code, home, slot, value)
            .expect("a handler's binding is valid");
        note_allocation(vm);
        if matches!(home, HandlerHome::Frame(_)) {
            drop_environment_caches(cx);
        }
    }
    super::super::compiled::LANE_CALL_COMPLETED
}
