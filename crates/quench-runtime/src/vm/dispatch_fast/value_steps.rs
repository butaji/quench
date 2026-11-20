//! `Binary`, `Unary` and comparison-jump steps of an activation with no frame of its own (a native callee, a lane
//! callee): made over the values the records name in the registers and the program's constants, as `Vm::step` makes
//! them in a frame, so the activations stay the lane's unless guest code the step ran made them frames.
use super::*;
use super::super::compiled::{LANE_CALL_BRANCHED, LANE_CALL_COMPLETED, LANE_CALL_LEFT, LANE_CALL_MISSED};
use crate::bytecode::ValueStep;

/// A value step's operands: left and right (a unary step's is its operand twice).
const OPERANDS: usize = 2;

/// What the value step made: the value it writes to its result register, and whether it takes its jump.
struct Made {
    value: Option<Value>,
    jumps: bool,
}

/// The operand values of the value step `instruction`, read from the registers `r` and the program's constants; `None`
/// for an operand of another kind (a local, which only a frame holds).
fn operand_values<H: Host>(cx: *mut LaneContext<H>, r: Registers, step: ValueStep, instruction: WideInstruction) -> Option<[Value; OPERANDS]> {
    match step {
        ValueStep::Unary => Some([r.get(instruction.register_b()); OPERANDS]),
        ValueStep::Binary | ValueStep::CompareJump => {
            Some([operand_value_in_lane(cx, r, instruction.operand_b().0)?, operand_value_in_lane(cx, r, instruction.operand_c().0)?])
        }
    }
}

/// The step `instruction` of its operand values `[left, right]`.
fn make<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, (step, instruction): (ValueStep, WideInstruction), [left, right]: [Value; 2]) -> Result<Made, JsError> {
    Ok(match step {
        ValueStep::Binary => Made { value: Some(vm.binary(code, instruction.binary_operator(), left, right)?), jumps: false },
        ValueStep::Unary => Made { value: Some(vm.unary(code, instruction.unary_operator(), left)?), jumps: false },
        ValueStep::CompareJump => Made { value: None, jumps: !vm.binary_truthy(code, instruction.binary_operator_field(), left, right)? },
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
/// As `body_general_step`; `record` is the record at `call_pc`.
pub(super) unsafe fn activation_value_step<H: Host>(cx: *mut LaneContext<H>, call_pc: u64, record: WideInstruction) -> Option<u64> {
    let instruction = record.without_return();
    if let Some(status) = unsafe { activation_field_read(cx, call_pc, instruction) } { return Some(status); }
    let step = instruction.op().value_step().filter(|_| instruction.result_flags() == 0)?;
    let r = unsafe { (*cx).registers };
    let Some(values) = operand_values(cx, r, step, instruction) else { return Some(LANE_CALL_MISSED) };
    let ip = unsafe { (*cx).base.add(call_pc as usize) };
    let Some(made) = in_place_in_lane(cx, ip, |vm, code| make(vm, code, (step, instruction), values)) else { return Some(LANE_CALL_MISSED) };
    // Guest code the step ran may have made the activations frames, whose registers these are.
    let after = unsafe { (*cx).registers };
    if let Some(value) = made.value { after.set(instruction.result_register(), value); }
    if after.0 == r.0 { return Some(if made.jumps { LANE_CALL_BRANCHED } else { LANE_CALL_COMPLETED }); }
    let resume = if made.jumps { unsafe { (*cx).base.add(instruction.jump_target() as usize) } } else { following(ip) };
    unsafe { (*cx).callee_resume = resume; (*cx).callee_acc = Value::UNDEFINED; }
    Some(LANE_CALL_LEFT)
}

/// The named field read `instruction` at `call_pc` of the innermost activation (`GetField` of one atom off a register
/// or the receiver, writing its result register alone), made by the interpreter's own cached read
/// (`Vm::get_field_cached`) over the activation's registers: `LANE_CALL_COMPLETED` with the result written,
/// `LANE_CALL_LEFT` where a getter it ran made the activations frames, `LANE_CALL_MISSED` where it raised; `None` for
/// any other record, or a receiver that is no object (a primitive's read is the frame step's).
///
/// # Safety
/// As `activation_value_step`.
unsafe fn activation_field_read<H: Host>(cx: *mut LaneContext<H>, call_pc: u64, instruction: WideInstruction) -> Option<u64> {
    if !matches!(instruction.op().class(), crate::bytecode::Class::GetField) || instruction.result_flags() != 0 || instruction.writes_current_this() { return None; }
    let Some(crate::bytecode::FieldLookup::Atom { atom, base, cache_site }) = instruction.field_lookup() else { return None };
    let r = unsafe { (*cx).registers };
    let object = if base == crate::bytecode::FieldBase::THIS { r.this() } else { r.get(base.register_index()?) };
    if !unsafe { vm(cx) }.is_object_like(object) { return None; }
    let ip = unsafe { (*cx).base.add(call_pc as usize) };
    let Some(value) = in_place_in_lane(cx, ip, |vm, code| vm.get_field_cached(code, object, atom, cache_site)) else { return Some(LANE_CALL_MISSED) };
    let after = unsafe { (*cx).registers };
    after.set(instruction.result_register(), value);
    if after.0 == r.0 { return Some(LANE_CALL_COMPLETED); }
    unsafe { (*cx).callee_resume = following(ip); (*cx).callee_acc = Value::UNDEFINED; }
    Some(LANE_CALL_LEFT)
}
