//! `Binary`, `Unary` and comparison-jump steps of an activation with no frame of its own (a native callee, a lane
//! callee): made over the values the records name in the registers and the program's constants, as `Vm::step` makes
//! them in a frame, so the activations stay the lane's unless guest code the step ran made them frames.
use super::*;
use super::super::compiled::{LANE_CALL_BRANCHED, LANE_CALL_COMPLETED, LANE_CALL_LEFT, LANE_CALL_MISSED};
use crate::bytecode::Op;

/// What the value step made: the value it writes to its result register, and whether it takes its jump.
struct Made {
    value: Option<Value>,
    jumps: bool,
}

/// The operand values of the value step `instruction`, read from the registers `r` and the program's constants; `None`
/// for an operand of another kind (a local, which only a frame holds).
fn operand_values<H: Host>(cx: *mut LaneContext<H>, r: Registers, instruction: WideInstruction) -> Option<[Value; 2]> {
    if instruction.op() == Op::Unary {
        return Some([r.get(instruction.register_b()); 2]);
    }
    Some([lane_operand_value(cx, r, instruction.operand_b().0)?, lane_operand_value(cx, r, instruction.operand_c().0)?])
}

/// The step `instruction` of its operand values `[left, right]`.
fn make<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, instruction: WideInstruction, [left, right]: [Value; 2]) -> Result<Made, JsError> {
    Ok(match instruction.op() {
        Op::Binary => {
            let operator = instruction.binary_operator();
            let value = match vm.number_binary(operator, left, right) {
                Some(value) => value,
                None => vm.binary(code, operator, left, right)?,
            };
            Made { value: Some(value), jumps: false }
        }
        Op::Unary => Made { value: Some(vm.unary(code, instruction.unary_operator(), left)?), jumps: false },
        _ => Made { value: None, jumps: !vm.binary_truthy(code, instruction.binary_operator_field(), left, right)? },
    })
}

/// The record at `call_pc` of the innermost activation, where it is a value step (a `Binary`, a `Unary` or a comparison
/// jump that defines no local): `LANE_CALL_COMPLETED` with the result written, `LANE_CALL_BRANCHED` where the jump is
/// taken, `LANE_CALL_LEFT` where guest code the step ran made the activations frames, which resume after it,
/// `LANE_CALL_MISSED` where an operand is no register or constant or the step raised (the error stashed for the general
/// path); `None` for any other record. A returning record is made as the step that writes its result register: the
/// body's own `Return` row completes the activation with it.
///
/// # Safety
/// As `lane_body_step`; `record` is the record at `call_pc`.
pub(super) unsafe fn lane_activation_value_step<H: Host>(cx: *mut LaneContext<H>, call_pc: u64, record: WideInstruction) -> Option<u64> {
    let instruction = record.without_return();
    if instruction.result_flags() != 0 || !matches!(instruction.op(), Op::Binary | Op::Unary | Op::JumpBinaryFalse) { return None; }
    let r = unsafe { (*cx).registers };
    let Some(values) = operand_values(cx, r, instruction) else { return Some(LANE_CALL_MISSED) };
    let ip = unsafe { (*cx).base.add(call_pc as usize) };
    let Some(made) = lane_in_place(cx, ip, |vm, code| make(vm, code, instruction, values)) else { return Some(LANE_CALL_MISSED) };
    // Guest code the step ran may have made the activations frames, whose registers these are.
    let after = unsafe { (*cx).registers };
    if let Some(value) = made.value { after.set(instruction.result_register(), value); }
    if after.0 == r.0 { return Some(if made.jumps { LANE_CALL_BRANCHED } else { LANE_CALL_COMPLETED }); }
    let resume = if made.jumps { unsafe { (*cx).base.add(instruction.jump_target() as usize) } } else { following(ip) };
    unsafe { (*cx).callee_resume = resume; (*cx).callee_acc = Value::UNDEFINED; }
    Some(LANE_CALL_LEFT)
}
