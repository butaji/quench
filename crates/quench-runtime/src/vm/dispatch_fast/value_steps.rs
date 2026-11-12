//! A lane body's value steps (`Vm::value_step`: `ToPropertyKey`, `GetIterator`, `ForInStart`,
//! `IteratorClose`, `AppendArray`), the native form of every opcode that has one: the body's
//! helper makes the step by the interpreter's own definition, as `lane_step_beside_activations`
//! does for a frameless activation, and continues, so no such record hands its body back.
use super::*;

/// The value step of the record at `call_pc`, its operands read from the running registers:
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
pub(in crate::vm) unsafe extern "C" fn lane_body_value_step<H: Host>(vm: u64, residual: u64, context: u64, call_pc: u64) -> u64 {
    // SAFETY: the caller's contract.
    let status = unsafe { lane_body_value_step_record::<H>(vm, residual, context, call_pc) };
    unsafe { super::vm(context as usize as *mut LaneContext<H>) }.refresh_body_bases();
    status
}

unsafe fn lane_body_value_step_record<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    let cx = context as usize as *mut LaneContext<H>;
    native_frames::in_innermost_frame(cx, || unsafe { lane_body_value_step_in(cx, call_pc) })
}

/// `lane_body_value_step` with the context of the running body, which a native callee's frame
/// made the innermost (`native_frames::in_innermost_frame`).
///
/// # Safety
/// As `lane_body_value_step`.
unsafe fn lane_body_value_step_in<H: Host>(cx: *mut LaneContext<H>, call_pc: u64) -> u64 {
    use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_LEFT, LANE_CALL_MISSED};
    let vm = unsafe { vm(cx) };
    let (r, ip) = unsafe { ((*cx).registers, (*cx).base.add(call_pc as usize)) };
    let function = match vm.lane_stack.activations.last() {
        Some(activation) => activation.function,
        None => vm.frames[unsafe { (*cx).frame }].function,
    };
    // SAFETY: the context's program outlives the lane run.
    let body = unsafe { &*(*cx).code }.function(function as usize);
    // A returning record (`return a + b`) is made as the step that writes its result register: the body's own
    // `Return` row completes the activation with it.
    let instruction = decoded_instruction(body, body.code[call_pc as usize]).without_return();
    if !super::super::dispatch::is_value_step(instruction) { return LANE_CALL_MISSED; }
    let Some([a, b]) = value_step_values(cx, r, instruction) else { return LANE_CALL_MISSED };
    let Some(value) = lane_in_place(cx, ip, |vm, code| vm.value_step(code, instruction, a, b)) else { return LANE_CALL_MISSED };
    // Guest code the step ran may have made the activations frames, whose registers these are.
    let after = unsafe { (*cx).registers };
    if let (Some(value), Some(register)) = (value, super::super::dispatch::value_step_result(instruction)) {
        after.set(register, value);
    }
    if after.0 == r.0 { return LANE_CALL_COMPLETED; }
    unsafe { (*cx).callee_resume = following(ip); (*cx).callee_acc = Value::UNDEFINED; }
    LANE_CALL_LEFT
}

/// The operand values of the value step `instruction`, read from the registers `r` and the program's constants:
/// a `Binary`'s two operands, else the registers `value_step_operands` names; `None` for an operand of another
/// kind (a local, which only a frame holds).
fn value_step_values<H: Host>(cx: *mut LaneContext<H>, r: Registers, instruction: WideInstruction) -> Option<[Option<Value>; 2]> {
    if instruction.op() != crate::bytecode::Op::Binary {
        return Some(super::super::dispatch::value_step_operands(instruction).map(|register| register.map(|register| r.get(register))));
    }
    let [left, right] = [instruction.operand_b(), instruction.operand_c()];
    Some([Some(lane_operand_value(cx, r, left.0)?), Some(lane_operand_value(cx, r, right.0)?)])
}

/// The record at `call_pc`, an instruction whose general step only continues, jumps or throws
/// (`Op::steps_in_body`: the lane's own in-place steps and those that may replace the frame's
/// environment), or a call form that is not a plain call or a construction over registers
/// (`Op::makes_script_call`: the callee runs as it does for any call a helper makes, the body's
/// activation not pushed, so its result is the step's), made by that step on the running frame as
/// `lane_general_step` makes it: `LANE_CALL_COMPLETED` with its effects in the frame,
/// `LANE_CALL_BRANCHED` where it also took its conditional jump (`ForInNext` past its last key),
/// or `LANE_CALL_MISSED` where it raised (the error stashed for the general path to raise at the
/// record, which is how a body's `throw` ends it) or where the record is no such step or runs
/// where it has no frame of its own (frameless activations, a native callee's shadow frames,
/// whose registers and function are not the running frame's); nothing ran then. The step may have
/// replaced the frame's environment, so the lane's capture caches, which name the old one, are
/// dropped after it (`lane_clone_env`).
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
    let cx = context as usize as *mut LaneContext<H>;
    native_frames::in_innermost_frame(cx, || unsafe { lane_body_step_in(cx, call_pc) })
}

/// `lane_body_step` with the context of the running body, which a native callee's frame made the
/// innermost (`native_frames::in_innermost_frame`). Where the body runs as lane activations (a native
/// callee, a lane callee) the steps that have no frame to run in take the value step's form, which
/// reads the operands from the activation's registers (`lane_body_value_step_in`).
///
/// # Safety
/// As `lane_body_step`.
unsafe fn lane_body_step_in<H: Host>(cx: *mut LaneContext<H>, call_pc: u64) -> u64 {
    use super::super::compiled::{LANE_CALL_BRANCHED, LANE_CALL_COMPLETED, LANE_CALL_MISSED};
    let vm = unsafe { vm(cx) };
    if !vm.lane_stack.activations.is_empty() { return unsafe { lane_body_value_step_in(cx, call_pc) }; }
    // SAFETY: the context's program outlives the lane run, and `call_pc` is a record of the running frame's function.
    let (code, frame, ip) = unsafe { (&*(*cx).code, (*cx).frame, (*cx).base.add(call_pc as usize)) };
    let function = code.function(vm.frames[frame].function as usize);
    // A returning record (`return a + b`) is made as the step that writes its result register: the body's
    // own `Return` row completes the activation with it.
    let instruction = decoded_instruction(function, function.code[call_pc as usize]).without_return();
    if !(instruction.op().steps_in_body() || instruction.op().makes_script_call()) { return LANE_CALL_MISSED; }
    let following = call_pc as usize + 1;
    let branched = lane_in_place(cx, ip, |vm, code| {
        let mut next = following;
        vm.step(code, frame, instruction, &mut next, false).map(|result| {
            debug_assert!(matches!(result, StepResult::Continue), "an in-place step only continues");
            next != following
        })
    });
    if branched.is_some() && (instruction.op() == crate::bytecode::Op::InitializeThis || instruction.writes_current_this()) {
        // The step bound the frame's `this` (`super()`): the rows that read the receiver take the payload's.
        let this = vm.current_this(frame).unwrap_or(Value::DELETED);
        vm.publish_body_receiver(this);
    }
    // SAFETY: as above; the dropped caches are derived again where the lane next needs them.
    unsafe {
        (*cx).capture = std::ptr::null();
        (*cx).outer = NO_OUTER_WALK;
        (*cx).frame_storage = NO_FRAME_STORAGE;
    }
    match branched {
        Some(true) => LANE_CALL_BRANCHED,
        Some(false) => LANE_CALL_COMPLETED,
        None => LANE_CALL_MISSED,
    }
}
