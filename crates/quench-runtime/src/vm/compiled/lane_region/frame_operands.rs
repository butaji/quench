//! A staged operand's value: a register's proved one, or a constant's, statically.
use super::*;
use super::super::super::object_slots::emit;
use crate::staged::flow::FlowStage;
use crate::staged::machine::{Lane, MachineScalar, Unary};
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
            Some(super::super::super::object_slots::emit(builder).f64_constant(value))
        }
        _ => None,
    }
}

/// A bitwise operand's ToInt32: an int32 as it is, a number by truncation modulo 2^32. A
/// double of magnitude 2^63 or more, an infinity or NaN leaves (`EXACT_I64_TRUNCATION_LIMIT`).
pub(in crate::vm) fn operand_to_int32(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, constants: &[crate::bytecode::Constant], operand: crate::bytecode::Operand,
    exit: ir::Block) -> Option<ir::Value> {
    use crate::bytecode::OperandKind;
    let integral = match operand.kind()? {
        OperandKind::Register => state.values[usize::from(operand.payload())].value.ty.is_subtype(Type::INT32),
        OperandKind::Constant => constants.get(usize::from(operand.payload()))
            .and_then(super::super::facts::immediate).is_some_and(super::super::facts::is_int32),
        OperandKind::Field | OperandKind::Local => return None,
    };
    if integral { return operand_int(builder, registers, state, constants, operand, exit); }
    // A register only its tag types is its int where the tag says so, else its Number's
    // ToInt32; neither path proves a fact the other does not.
    let tagged = operand.register_index()
        .filter(|register| state.values[usize::from(*register)].value.representation == Representation::Tagged);
    match tagged {
        Some(register) => Some(tagged_to_int32(builder, registers, state, register, exit)),
        None => {
            let number = operand_number(builder, registers, state, constants, operand, exit)?;
            Some(crate::vm::compiled::emit_number_to_int32(builder, number, exit))
        }
    }
}

/// A tagged register's ToInt32, kept for the later uses of the same bits.
fn tagged_to_int32(builder: &mut FunctionBuilder<'_>, registers: &mut Registers, state: &mut State,
    register: Register, exit: ir::Block) -> ir::Value {
    let bits = register_tagged(builder, registers, state, register);
    if let Some((_, int32)) = state.proofs.int32_projections.iter().find(|(source, _)| *source == bits) {
        #[cfg(test)] INT32_PROJECTION_REUSES.with(|count| count.set(count.get() + 1));
        return *int32;
    }
    let integer = is_int_tagged(builder, bits);
    let (int_block, number_block, join) = (builder.create_block(), builder.create_block(), builder.create_block());
    let int32 = builder.append_block_param(join, types::I32);
    emit(builder).branch(integer, (int_block, &[]), (number_block, &[]));
    builder.switch_to_block(int_block);
    let int = emit(builder).raw_unary(Unary::Narrow(Lane::I32), bits);
    emit(builder).jump(join, &[int]);
    builder.switch_to_block(number_block);
    let number = emit_js_tagged_number(builder, bits, exit);
    let truncated = crate::vm::compiled::emit_number_to_int32(builder, number, exit);
    emit(builder).jump(join, &[truncated]);
    builder.switch_to_block(join);
    state.proofs.int32_projections.push((bits, int32));
    int32
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
            super::super::super::object_slots::emit(builder).narrow(bits)
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
            let bits = builder.func.dfg.resolve_aliases(bits);
            let known = state.proofs.numbers.iter().find(|(source, _)| *source == bits).map(|(_, number)| *number);
            let number = if let Some(number) = known { number } else if fact.ty.is_subtype(Type::NUMBER) {
                emit_known_number(builder, bits)
            } else {
                emit_js_tagged_number(builder, bits, otherwise)
            };
            if known.is_none() && state.proofs.numbers.len() < 4 * INLINE_NUMBER_PROJECTIONS { state.proofs.numbers.push((bits, number)); }
            let variable = register_float(builder, registers, index);
            builder.def_var(variable, number);
            state.values[index] = RegisterFact { tagged: state.values[index].tagged,
                ..RegisterFact::definition(Type::NUMBER, Representation::Float64) };
            number
        }
    }
}
