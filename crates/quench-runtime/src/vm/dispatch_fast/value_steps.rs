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
pub(in crate::vm) unsafe extern "C" fn lane_body_value_step<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
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
    // A RegExp literal reads no register: the lane handler's own step (`lane_regexp_literal`).
    if instruction.op() == Op::CreateRegExpLiteral {
        let Some(value) = lane_regexp_literal_value(cx, instruction.regexp_literal_site_index()) else { return LANE_CALL_MISSED };
        r.set(instruction.result_register(), value);
        return LANE_CALL_COMPLETED;
    }
    let [a, b] = super::super::dispatch::value_step_operands(instruction).map(|register| register.map(|register| r.get(register)));
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
