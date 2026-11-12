//! ToBoolean of a register in specialized code: its proved representation's test, else its
//! word's, else its cell's by the VM.
use super::*;

/// ToBoolean of `register` as an i8, from its proved representation; a tagged value whose
/// truthiness needs its cell (`emit_js_tagged_truthiness`) leaves at `miss`.
#[inline(always)]
pub(super) fn register_truthiness(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register, (payload, helpers): (ir::Value, LaneHelpers), miss: ir::Block) -> ir::Value {
    let fact = state.values[usize::from(register)].value;
    match fact.representation {
        Representation::Int32 => {
            let int = { let variable = register_int(builder, registers, usize::from(register)); builder.use_var(variable) };
            builder.ins().icmp_imm_u(IntCC::NotEqual, int, 0)
        }
        Representation::Float64 => {
            let number = { let variable = register_float(builder, registers, usize::from(register)); builder.use_var(variable) };
            emit_number_truthiness(builder, number)
        }
        Representation::Tagged => {
            let bits = register_tagged(builder, registers, state, register);
            if fact.ty.is_subtype(Type::language(crate::ValueType::Boolean)) {
                builder.ins().icmp_imm_u(IntCC::Equal, bits, Value::TRUE.to_bits() as i64)
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
    let (undecided, truthy) = crate::staged::value::tagged_truthiness(&mut super::super::super::object_slots::emit(builder), bits);
    let (cell, join) = (builder.create_block(), builder.create_block());
    builder.set_cold_block(cell);
    let result = builder.append_block_param(join, types::I8);
    builder.ins().brif(undecided, cell, &[], join, &[ir::BlockArg::Value(truthy)]);
    builder.switch_to_block(cell);
    let address = helpers.address(builder, LaneHelper::Truthy);
    let word = super::super::super::emit_lane_helper_call(builder, payload, address, &[bits]);
    let truthy = builder.ins().icmp_imm_u(IntCC::NotEqual, word, 0);
    builder.ins().jump(join, &[ir::BlockArg::Value(truthy)]);
    builder.switch_to_block(join);
    result
}
