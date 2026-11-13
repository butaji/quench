//! ToBoolean of a register in specialized code: its proved representation's test, else its
//! word's, else its cell's by the VM.
use super::*;
use crate::staged::Stage;

/// ToBoolean of `register` as an i8, from its proved representation; a tagged value whose
/// truthiness needs its cell (`emit_js_tagged_truthiness`) leaves at `miss`.
#[inline(always)]
pub(super) fn register_truthiness(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register, (payload, helpers): (ir::Value, LaneHelpers), miss: ir::Block) -> ir::Value {
    let fact = state.values[usize::from(register)].value;
    match fact.representation {
        Representation::Int32 => {
            let int = { let variable = register_int(builder, registers, usize::from(register)); builder.use_var(variable) };
            let mut stage = super::super::super::object_slots::emit(builder);
            let word = stage.uextend32(int);
            stage.ne_imm(word, 0u32)
        }
        Representation::Float64 => {
            let number = { let variable = register_float(builder, registers, usize::from(register)); builder.use_var(variable) };
            emit_number_truthiness(builder, number)
        }
        Representation::Tagged => {
            let bits = register_tagged(builder, registers, state, register);
            if fact.ty.is_subtype(Type::language(crate::ValueType::Boolean)) {
                super::super::super::object_slots::emit(builder).eq_imm(bits, Value::TRUE.to_bits())
            } else if helpers.is_numeric() {
                emit_js_tagged_truthiness(builder, bits, miss)
            } else {
                cell_truthiness(builder, payload, helpers, bits)
            }
        }
    }
}

/// ToBoolean of tagged `bits` whatever they hold: its word's answer, else its cell's by the
/// VM (`lane_truthy`), so a string or a function a condition tests keeps the body running.
fn cell_truthiness(builder: &mut FunctionBuilder<'_>, payload: ir::Value, helpers: LaneHelpers, bits: ir::Value) -> ir::Value {
    let mut stage = super::super::super::object_slots::emit(builder);
    let (undecided, truthy) = crate::staged::value::tagged_truthiness(&mut stage, bits);
    let word = stage.choose(undecided, |stage| {
        let address = helpers.address(stage.builder, LaneHelper::Truthy);
        super::super::super::emit_lane_helper_call(stage.builder, payload, address, &[bits])
    }, |stage| {
        let (yes, no) = (stage.constant(1), stage.constant(0));
        stage.select(truthy, yes, no)
    });
    stage.ne_imm(word, 0u32)
}
