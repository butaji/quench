//! A native catch's frame state (`Lowering::bind_caught`): what the interpreter's handler entry
//! settles (`Vm::unwind_handler_scopes`, `Vm::bind_thrown_value`) that native stores cannot. A
//! frame's is made by the interpreter's own definition; a lane activation's, a window's, by the
//! same steps over the lane stack's scope chain and the activation's window.
use super::*;
use crate::bytecode::ThrownValueSlot;

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

/// The entry of the handler covering record `call_pc` of the running function, for the thrown
/// `value`: the `with` scopes past the handler's unwound and, for a binding in a local, the local
/// stored where the running function keeps it: a frame's, as the interpreter stores it, a
/// captured frame getting a fresh environment per entry, whose copy replaces the one the lane's
/// caches name, or a window's (`bind_window_local`). A promoted binding is the body's own
/// register. The running function is the innermost activation's, else the frame's.
pub(in crate::vm) unsafe extern "C" fn lane_handler_edge<H: Host>(
    _vm: u64,
    _residual: u64,
    context: u64,
    call_pc: u64,
    value: u64,
) -> u64 {
    let cx = context as usize as *mut LaneContext<H>;
    let (call_pc, value) = (call_pc as u32, Value::from_bits(value));
    match unsafe { vm(cx) }.lane_stack.activations.last().copied() {
        Some(activation) => activation_handler_edge(cx, &activation, call_pc, value),
        None => frame_handler_edge(cx, call_pc, value),
    }
    super::super::compiled::LANE_CALL_COMPLETED
}

/// `lane_handler_edge` in the VM frame the lane runs.
fn frame_handler_edge<H: Host>(cx: *mut LaneContext<H>, call_pc: u32, value: Value) {
    let vm = unsafe { vm(cx) };
    let frame = unsafe { (*cx).frame() };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let handler = code
        .function(vm.frames[frame].function as usize)
        .handler_at(call_pc)
        .expect("a native catch's record has its handler");
    vm.unwind_handler_scopes(frame, handler);
    if let Some(slot @ ThrownValueSlot::Local(_)) = handler.slot {
        vm.bind_thrown_value(code, frame, slot, value)
            .expect("a frame's handler binding is valid");
        note_allocation(vm);
        drop_environment_caches(cx);
    }
}

/// `lane_handler_edge` in the lane activation `activation`, the innermost: the `with` objects it
/// entered past the handler's depth leave its chain, and a local binding is stored in its window.
fn activation_handler_edge<H: Host>(
    cx: *mut LaneContext<H>,
    activation: &Activation,
    call_pc: u32,
    value: Value,
) {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let handler = code
        .function(activation.function as usize)
        .handler_at(call_pc)
        .expect("a native catch's record has its handler");
    let depth = vm.lane_stack.activations.len() - 1;
    vm.lane_stack
        .unwind_with_objects(depth, usize::from(handler.with_depth));
    if let Some(ThrownValueSlot::Local(slot)) = handler.slot
        && activation.script().is_some_and(|script| script.environment)
    {
        bind_window_local(cx, activation, slot, value);
    }
}

/// Stores the thrown `value` to local `slot` of the environment window of `activation`, as
/// `Vm::initialize_handler_binding` does for a frame: a slot the window's environment owns once a
/// closure made it gets a fresh copy of the environment per entry, which closures of earlier
/// entries do not share (`Vm::iteration_environment`); any other slot is among the window's locals.
fn bind_window_local<H: Host>(
    cx: *mut LaneContext<H>,
    activation: &Activation,
    slot: u16,
    value: Value,
) {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let function = unsafe { &*(*cx).code }.function(activation.function as usize);
    // SAFETY: an environment activation's window holds its entries after its registers
    // (`lane_activate_environment`), which nothing else writes while the handler enters.
    let entries = unsafe { activation.window_ptr().add(usize::from(function.registers)) };
    let own = unsafe { *entries.add(usize::from(WINDOW_ENVIRONMENT)) };
    let index = usize::from(slot);
    if own.is_deleted() || !function.local_slot_uses_environment(index) {
        // SAFETY: as above, the locals follow the entries.
        unsafe { *entries.add(usize::from(WINDOW_LOCALS) + index) = value };
        return;
    }
    let program = unsafe { (*cx).program };
    let environment = vm
        .iteration_environment(
            own,
            program,
            activation.function,
            &[slot],
            crate::bytecode::Resets::listed(&[]),
        )
        .expect("a window's handler binding is valid");
    let Some(crate::heap::Cell::Environment { slots, .. }) = vm.heap.get(environment) else {
        unreachable!("an iteration copy is an environment");
    };
    let storage = std::ptr::from_ref(slots);
    // SAFETY: as above; the copy's slot storage never moves while it lives.
    unsafe {
        entries
            .add(usize::from(WINDOW_ENVIRONMENT))
            .write(environment);
        entries
            .add(usize::from(WINDOW_SLOTS))
            .write(std::mem::transmute::<u64, Value>(storage as u64));
    }
    *vm.heap
        .environment_slot_mut(environment, index)
        .expect("the copy owns the handler's binding") = value;
    note_allocation(vm);
}
