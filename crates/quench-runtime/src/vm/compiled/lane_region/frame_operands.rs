//! A staged operand's value: a register's proved one, or a constant's, statically.
use super::*;
use super::frame::{register_int_value, register_number_value};

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
            if !int32 {
                builder.ins().jump(otherwise, &[]);
                let dead = builder.create_block();
                builder.switch_to_block(dead);
            }
            Some(builder.ins().iconst(types::I32, if int32 { i64::from(value.wasm_bits32() as i32) } else { 0 }))
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

