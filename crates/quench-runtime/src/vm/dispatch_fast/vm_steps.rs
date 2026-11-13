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
/// body at the next record. A decision (`CallOut::Decide`) completes with `LANE_CALL_HOLDS` where
/// the comparison held.
pub(in crate::vm) unsafe extern "C" fn lane_vm_step<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    let cx = context as usize as *mut LaneContext<H>;
    let status = native_frames::in_innermost_frame(cx, || unsafe { lane_vm_step_in(cx, call_pc) });
    // SAFETY: `context` is the running lane's; the step may have allocated or run guest code.
    unsafe { refresh_body_bases::<H>(context) };
    status
}

/// `lane_vm_step` with the context of the running body, which a native callee's frame made the
/// innermost (`native_frames::in_innermost_frame`).
///
/// # Safety
/// As `lane_vm_step`.
unsafe fn lane_vm_step_in<H: Host>(cx: *mut LaneContext<H>, call_pc: u64) -> u64 {
    use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_HOLDS, LANE_CALL_LEFT, LANE_CALL_MISSED};
    // The function's activation is the one the step reads (`activations.last`).
    lane_materialize_running_leaf(cx);
    let (r, ip) = unsafe { ((*cx).registers, (*cx).base.add(call_pc as usize)) };
    let function = lane_running_function(cx);
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let body = code.function(function as usize);
    let instruction = decoded_instruction(body, body.code[call_pc as usize]);
    let (operation, result) = crate::staged::Operation::<(), ()>::decode(instruction, crate::staged::DecodeContext::of(code))
        .and_then(|row| row.call_out()).expect("a call-out record decodes");
    let operands = operation.operands().read(&code.method_arguments, |register| r.get(register));
    let Some(value) = lane_in_place(cx, ip, |vm, code| {
        let site = super::super::call_out::CallSite { frame: vm.frames.len() - 1, function, resume: call_pc as usize + 1 };
        vm.call_out(code, site, operation, operands)
    }) else { return LANE_CALL_MISSED };
    // Guest code the step ran may have made the activations frames, whose registers these are.
    let after = unsafe { (*cx).registers };
    if let (Some(value), Some(register)) = (value, result) {
        after.set(register, value);
    }
    if after.0 == r.0 {
        let holds = matches!(operation, crate::staged::CallOut::Decide { .. }) && value == Some(Value::TRUE);
        return if holds { LANE_CALL_HOLDS } else { LANE_CALL_COMPLETED };
    }
    // A decision that failed took its jump (`JumpBinaryFalse`), which the activations resume at.
    let resume = match operation {
        crate::staged::CallOut::Decide { .. } if value != Some(Value::TRUE) =>
            unsafe { (*cx).base.add(instruction.jump_target() as usize) },
        _ => following(ip),
    };
    unsafe { (*cx).callee_resume = resume; (*cx).callee_acc = Value::UNDEFINED; }
    LANE_CALL_LEFT
}

/// `+` of the record at `call_pc` whose operands are registers holding strings (or Numbers, one a
/// string) as `Vm::primitive_concatenation` defines it: `LANE_CALL_COMPLETED` with the result in
/// its register, or `LANE_CALL_MISSED` before any effect where the operands are no such pair
/// (guest code never runs, so the body is never left).
pub(in crate::vm) unsafe extern "C" fn lane_body_string_concat<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_MISSED};
    let cx = context as usize as *mut LaneContext<H>;
    let vm = unsafe { vm(cx) };
    let r = unsafe { (*cx).registers };
    let function = match vm.lane_stack.activations.last() {
        Some(activation) => activation.function,
        None => vm.frames[unsafe { (*cx).frame() }].function,
    };
    // SAFETY: the context's program outlives the lane run.
    let body = unsafe { &*(*cx).code }.function(function as usize);
    let instruction = decoded_instruction(body, body.code[call_pc as usize]);
    let [Some(left), Some(right)] = [instruction.operand_b(), instruction.operand_c()].map(|operand| operand.register_index().map(|register| r.get(register))) else { return LANE_CALL_MISSED };
    match lane_concatenation(cx, left, right) {
        Some(value) => {
            r.set(instruction.result_register(), value);
            LANE_CALL_COMPLETED
        }
        None => LANE_CALL_MISSED,
    }
}

/// The decision of `left operator right` that runs no guest code and allocates nothing: a strict
/// equality (`Vm::strict_equal`), `instanceof` by the remembered constructor layout
/// (`Vm::steady_instanceof`) and `in` of a string key in an ordinary object (`Vm::plain_in`);
/// `None` for any other pair, which the full algorithm decides. The one rule of the interpreter's
/// staged frame and of the lane's decide helper.
pub(super) fn lane_decision<H: Host>(vm: &mut Vm<H>, operator: u32, left: Value, right: Value) -> Option<bool> {
    match crate::vm::operations::equality_operator(operator) {
        Some((strict, negated)) => strict.then(|| vm.strict_equal(left, right) != negated),
        None if operator == INSTANCEOF_OPERATOR => vm.steady_instanceof(left, right),
        None => vm.plain_in(operator, left, right),
    }
}

/// A lane body's decision of `left operator right` by the words a `CallOut::Decide` carries, which
/// needs no record of the VM's: `Value::TRUE` or `Value::FALSE`, or `Value::DELETED` where only
/// the full algorithm decides, which leaves the body at the test (`lane_decision`).
pub(in crate::vm) unsafe extern "C" fn lane_decide<H: Host>(vm: u64, _residual: u64, left: u64, right: u64, operator: u64) -> u64 {
    // SAFETY: as `lane_field_set`; the decision reads the heap, runs no guest code and allocates nothing.
    let vm = unsafe { &mut *(vm as *mut Vm<H>) };
    lane_decision(vm, operator as u32, Value::from_bits(left), Value::from_bits(right))
        .map_or_else(|| Value::DELETED.to_bits(), |holds| Value::boolean(holds).to_bits())
}
