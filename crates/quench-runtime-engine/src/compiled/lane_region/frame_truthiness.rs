//! ToBoolean of a register in specialized code: its proved representation's test, else its
//! word's, else its cell's by the VM.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::Stage;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// ToBoolean of `register` as an i8, from its proved representation; a tagged value whose
/// truthiness needs its cell (`emit_js_tagged_truthiness`) leaves at `miss`.
#[inline(always)]
pub(crate) fn register_truthiness(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register, (payload, helpers): (ir::Value, LaneHelpers), miss: ir::Block) -> ir::Value {
    let fact = state.values[usize::from(register)].value;
    match fact.representation {
        Representation::Int32 => {
            let int = { let variable = register_int(builder, registers, usize::from(register)); builder.use_var(variable) };
            quench_runtime::staged::value::int_truthy(&mut crate::compiled::object_slots::emit(builder), int)
        }
        Representation::Int64 => {
            let word = { let variable = register_long(builder, registers, usize::from(register)); builder.use_var(variable) };
            quench_runtime::staged::value::int_truthy(&mut crate::compiled::object_slots::emit(builder), word)
        }
        Representation::Float64 => {
            let number = { let variable = register_float(builder, registers, usize::from(register)); builder.use_var(variable) };
            quench_runtime::staged::number_machine::number_truthy(&mut crate::compiled::object_slots::emit(builder), number)
        }
        Representation::Tagged => {
            let bits = register_tagged(builder, registers, state, register);
            if fact.ty.is_subtype(Type::language(quench_runtime::ValueType::Boolean)) {
                quench_runtime::staged::value::boolean_truthy(&mut crate::compiled::object_slots::emit(builder), bits)
            } else if helpers.is_numeric() {
                emit_js_tagged_truthiness(builder, bits, miss)
            } else {
                cell_truthiness(builder, payload, helpers, bits)
            }
        }
    }
}

/// ToBoolean of tagged `bits` whatever they hold: its word's answer, else a flat string's record
/// length (`string_length::flat_truth`), else its cell's by the VM (`truthy_in_lane`), so a rope
/// or a function a condition tests keeps the body running.
fn cell_truthiness(builder: &mut FunctionBuilder<'_>, payload: ir::Value, helpers: LaneHelpers, bits: ir::Value) -> ir::Value {
    let records = crate::compiled::emit_js_payload_word(builder, payload, std::mem::offset_of!(quench_runtime::vm::engine_types::JsStepDispatch, head.string_record_base));
    let mut stage = crate::compiled::object_slots::emit(builder);
    let (undecided, truthy) = quench_runtime::staged::value::tagged_truthiness(&mut stage, bits);
    let word = stage.choose_cold(undecided, |stage| {
        let flat = quench_runtime::staged::string_length::flat_truth(stage, records, bits);
        let unknown = stage.eq_imm(flat, 2u64);
        stage.choose_cold(unknown, |stage| {
            let address = helpers.target(stage.builder, LaneHelper::Truthy);
            crate::compiled::emit_lane_helper_call(stage.builder, payload, address, &[bits])
        }, |_| flat)
    }, |stage| {
        let (yes, no) = (stage.constant(1), stage.constant(0));
        stage.select(truthy, yes, no)
    });
    stage.ne_imm(word, 0u32)
}
