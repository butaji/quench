//! A staged operand's value: a register's proved one, or a constant's, statically.
use super::*;
use crate::staged::{NumericStage, Stage};

/// An operand's int32 value, statically where it is a constant int32.
pub(in crate::vm) fn operand_int(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, constants: &[crate::bytecode::Constant], operand: crate::bytecode::Operand,
    otherwise: ir::Block) -> Option<ir::Value> {
    match operand.kind()? {
        crate::bytecode::OperandKind::Register =>
            Some(register_int_value(builder, registers, state, operand.register_index()?, otherwise)),
        crate::bytecode::OperandKind::Constant => {
            let constant = constants.get(usize::from(operand.payload()))?;
            let value = super::super::facts::immediate(constant)?;
            let int32 = super::super::facts::is_int32(value);
            let mut stage = super::super::super::object_slots::emit(builder);
            if !int32 {
                let zero = stage.constant(0);
                let never = stage.ne_imm(zero, 0u32);
                stage.guard(never, otherwise)?;
            }
            Some(stage.i32_constant(if int32 { value.wasm_bits32() as i32 } else { 0 }))
        }
        _ => None,
    }
}

/// An operand's binary64 value, statically where it is a constant Number.
pub(in crate::vm) fn operand_number(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, constants: &[crate::bytecode::Constant], operand: crate::bytecode::Operand,
    otherwise: ir::Block) -> Option<ir::Value> {
    match operand.kind()? {
        crate::bytecode::OperandKind::Register =>
            Some(register_number_value(builder, registers, state, operand.register_index()?, otherwise)),
        crate::bytecode::OperandKind::Constant => {
            let value = constants.get(usize::from(operand.payload()))
                .and_then(super::super::facts::immediate)?.as_number()?;
            Some(builder.ins().f64const(value))
        }
        _ => None,
    }
}

/// `register`'s int32 value, proving it where the fact does not.
pub(in crate::vm) fn register_int_value(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
    let index = usize::from(register);
    let fact = state.values[index];
    let int = match fact.value.representation {
        Representation::Int32 => {
            let variable = register_int(builder, registers, index);
            return builder.use_var(variable);
        }
        Representation::Float64 if !fact.tagged.current() => {
            let variable = register_float(builder, registers, index);
            let number = builder.use_var(variable);
            let (int, exact) = emit_js_exact_int32(builder, number);
            if !fact.value.ty.is_subtype(Type::INT32) {
                emit_js_guard(builder, exact, otherwise);
            }
            int
        }
        Representation::Tagged | Representation::Float64 => {
            let bits = register_tagged(builder, registers, state, register);
            if !fact.value.ty.is_subtype(Type::INT32) {
                let integer = is_int_tagged(builder, bits);
                emit_js_guard(builder, integer, otherwise);
            }
            builder.ins().ireduce(types::I32, bits)
        }
    };
    let variable = register_int(builder, registers, index);
    builder.def_var(variable, int);
    state.values[index] = RegisterFact { tagged: state.values[index].tagged,
        ..RegisterFact::definition(Type::INT32, Representation::Int32) };
    int
}

/// `register`'s binary64 value, proving Number where the fact does not.
pub(in crate::vm) fn register_number_value(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
    let index = usize::from(register);
    let fact = state.values[index].value;
    match fact.representation {
        Representation::Float64 => {
            let variable = register_float(builder, registers, index);
            builder.use_var(variable)
        }
        Representation::Int32 => {
            let variable = register_int(builder, registers, index);
            let int = builder.use_var(variable);
            super::super::super::object_slots::emit(builder).i32_to_f64(int)
        }
        Representation::Tagged => {
            let bits = register_tagged(builder, registers, state, register);
            let number = if fact.ty.is_subtype(Type::NUMBER) {
                emit_known_number(builder, bits)
            } else {
                emit_js_tagged_number(builder, bits, otherwise)
            };
            let variable = register_float(builder, registers, index);
            builder.def_var(variable, number);
            state.values[index] = RegisterFact { tagged: state.values[index].tagged,
                ..RegisterFact::definition(Type::NUMBER, Representation::Float64) };
            number
        }
    }
}
