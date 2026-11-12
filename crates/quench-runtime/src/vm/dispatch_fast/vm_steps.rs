//! A lane body's call-outs (`staged::CallOut`: the value steps, `ForInNext`, a `Binary`, the
//! environment, arguments and global-name steps), the one helper a row's slow
//! case calls: it makes the step by the interpreter's own definition (`Vm::call_out`), as
//! `lane_step_beside_activations` does for a frameless activation, and continues, so no such
//! record hands its body back.
use super::*;

/// The call-out of the record at `call_pc`, its operands read from the running registers:
/// `LANE_CALL_COMPLETED` with the result written to its register, `LANE_CALL_MISSED` where it
/// raised (the error stashed for the general path, which re-runs the record and raises it), or
/// `LANE_CALL_LEFT` where guest code it ran made the lane's activations frames, which leave the
/// body at the next record.
///
/// The heap bases the body's rows read through the payload are reloaded after it
/// (`Vm::refresh_body_bases`): the step may have allocated or run guest code.
///
/// # Safety
/// The caller is a body's helper row: `context` its running lane context, `call_pc` a record of its function.
pub(in crate::vm) unsafe extern "C" fn lane_vm_step<H: Host>(vm: u64, residual: u64, context: u64, call_pc: u64) -> u64 {
    // SAFETY: the caller's contract.
    let status = unsafe { lane_vm_step_record::<H>(vm, residual, context, call_pc) };
    unsafe { super::vm(context as usize as *mut LaneContext<H>) }.refresh_body_bases();
    status
}

unsafe fn lane_vm_step_record<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_LEFT, LANE_CALL_MISSED};
    let cx = context as usize as *mut LaneContext<H>;
    let vm = unsafe { vm(cx) };
    let (r, ip) = unsafe { ((*cx).registers, (*cx).base.add(call_pc as usize)) };
    let function = match vm.lane_stack.activations.last() {
        Some(activation) => activation.function,
        None => vm.frames[unsafe { (*cx).frame }].function,
    };
    // SAFETY: the context's program outlives the lane run.
    let body = unsafe { &*(*cx).code }.function(function as usize);
    let instruction = decoded_instruction(body, body.code[call_pc as usize]);
    let (operation, result) = crate::staged::Operation::<(), ()>::decode(instruction, |_| None)
        .and_then(|row| row.call_out()).expect("a call-out record decodes");
    let operands = operation.operands().map(|register| register.map(|register| r.get(register)));
    let Some(value) = lane_in_place(cx, ip, |vm, code| {
        let site = super::super::call_out::CallSite { frame: vm.frames.len() - 1, resume: call_pc as usize + 1 };
        vm.call_out(code, site, operation, operands)
    }) else { return LANE_CALL_MISSED };
    // Guest code the step ran may have made the activations frames, whose registers these are.
    let after = unsafe { (*cx).registers };
    if let (Some(value), Some(register)) = (value, result) {
        after.set(register, value);
    }
    if after.0 == r.0 { return LANE_CALL_COMPLETED; }
    unsafe { (*cx).callee_resume = following(ip); (*cx).callee_acc = Value::UNDEFINED; }
    LANE_CALL_LEFT
}

/// The record at `call_pc`, an instruction whose general step only continues, jumps or throws
/// (`lane_steps_in_place`), made by that step on the running frame as `lane_general_step` makes
/// it: `LANE_CALL_COMPLETED` with its effects in the frame, `LANE_CALL_BRANCHED` where it also took
/// its conditional jump (`ForInNext` past its last key), or `LANE_CALL_MISSED` where it raised
/// (the error stashed for the general path to raise at the record) or where the record is no
/// in-place step or runs where it has no frame of its own (frameless activations, a native callee's
/// shadow frames, whose registers and function are not the running frame's); nothing ran then.
/// `lane_body_step_record`, then the heap bases the body's rows read through the payload reloaded
/// (`Vm::refresh_body_bases`): the step may have allocated or run guest code.
///
/// # Safety
/// As `lane_body_step_record`.
pub(in crate::vm) unsafe extern "C" fn lane_body_step<H: Host>(vm: u64, residual: u64, context: u64, call_pc: u64) -> u64 {
    // SAFETY: the caller's contract.
    let status = unsafe { lane_body_step_record::<H>(vm, residual, context, call_pc) };
    unsafe { super::vm(context as usize as *mut LaneContext<H>) }.refresh_body_bases();
    status
}

unsafe fn lane_body_step_record<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    use super::super::compiled::{LANE_CALL_BRANCHED, LANE_CALL_COMPLETED, LANE_CALL_MISSED};
    let cx = context as usize as *mut LaneContext<H>;
    let vm = unsafe { vm(cx) };
    if !vm.lane_stack.activations.is_empty() || Vm::<H>::lane_shadow_active() { return LANE_CALL_MISSED; }
    // SAFETY: the context's program outlives the lane run, and `call_pc` is a record of the running frame's function.
    let (code, frame, ip) = unsafe { (&*(*cx).code, (*cx).frame, (*cx).base.add(call_pc as usize)) };
    let function = code.function(vm.frames[frame].function as usize);
    let instruction = decoded_instruction(function, function.code[call_pc as usize]);
    if !lane_steps_in_place(instruction.op()) { return LANE_CALL_MISSED; }
    let following = call_pc as usize + 1;
    let branched = lane_in_place(cx, ip, |vm, code| {
        let mut next = following;
        vm.step(code, frame, instruction, &mut next, false).map(|result| {
            debug_assert!(matches!(result, StepResult::Continue), "an in-place step only continues");
            next != following
        })
    });
    match branched {
        Some(true) => LANE_CALL_BRANCHED,
        Some(false) => LANE_CALL_COMPLETED,
        None => LANE_CALL_MISSED,
    }
}
