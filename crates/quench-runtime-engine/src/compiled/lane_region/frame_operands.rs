//! A staged operand's value: a register's proved one, or a constant's, statically.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use crate::compiled::object_slots::emit;
use quench_runtime::staged::flow::FlowStage;
use quench_runtime::staged::machine::{Lane, MachineScalar, Relation, Unary};
use quench_runtime::staged::{Float64, Int32, IntFact, NumberFact, NumericStage, Stage};

/// An operand's int32 value, statically where it is a constant int32.
pub(crate) fn operand_int(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, constants: &[quench_runtime::bytecode::Constant], operand: quench_runtime::bytecode::Operand,
    otherwise: ir::Block) -> Option<Int32<ir::Value>> {
    match operand.kind()? {
        quench_runtime::bytecode::OperandKind::Register =>
            Some(register_int_value(builder, registers, state, operand.register_index()?, otherwise)),
        quench_runtime::bytecode::OperandKind::Constant => {
            let constant = constants.get(usize::from(operand.payload()))?;
            let value = super::super::facts::immediate(constant)?;
            let int32 = super::super::facts::is_int32(value);
            let mut stage = crate::compiled::object_slots::emit(builder);
            if !int32 {
                let zero = stage.constant(0);
                let never = stage.ne_imm(zero, 0u32);
                stage.guard(never, otherwise)?;
            }
            Some(Int32::constant(&mut stage, if int32 { value.wasm_bits32() as i32 } else { 0 }))
        }
        _ => None,
    }
}

/// `register`'s 64-bit integer word where it is held as one (`Representation::Int64`).
pub(crate) fn held_long(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &State, register: Register) -> Option<ir::Value> {
    let index = usize::from(register);
    (state.values[index].value.representation == Representation::Int64).then(|| {
        let variable = register_long(builder, registers, index);
        builder.use_var(variable)
    })
}

/// An operand's integer value as a signed 64-bit word: a register's held word, or its int32
/// sign-extended, or a constant int32's.
pub(crate) fn operand_long(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, constants: &[quench_runtime::bytecode::Constant], operand: quench_runtime::bytecode::Operand,
    otherwise: ir::Block) -> Option<ir::Value> {
    if let Some(word) = operand.register_index().and_then(|register| held_long(builder, registers, state, register)) {
        return Some(word);
    }
    let int = operand_int(builder, registers, state, constants, operand, otherwise)?.word();
    Some(emit(builder).raw_unary(Unary::SignExtend(Lane::I64), int))
}

/// An operand's binary64 value, statically where it is a constant Number.
pub(crate) fn operand_number(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, constants: &[quench_runtime::bytecode::Constant], operand: quench_runtime::bytecode::Operand,
    otherwise: ir::Block) -> Option<ir::Value> {
    match operand.kind()? {
        quench_runtime::bytecode::OperandKind::Register =>
            Some(register_number_value(builder, registers, state, operand.register_index()?, otherwise)),
        quench_runtime::bytecode::OperandKind::Constant => {
            let value = constants.get(usize::from(operand.payload()))
                .and_then(super::super::facts::immediate)?.as_number()?;
            Some(crate::compiled::object_slots::emit(builder).f64_constant(value))
        }
        _ => None,
    }
}

/// A bitwise operand's ToInt32: an int32 as it is, a number by truncation modulo 2^32. A
/// double of magnitude 2^63 or more, an infinity or NaN leaves (`EXACT_I64_TRUNCATION_LIMIT`).
pub(crate) fn operand_to_int32(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, constants: &[quench_runtime::bytecode::Constant], operand: quench_runtime::bytecode::Operand,
    exit: ir::Block) -> Option<ir::Value> {
    use quench_runtime::bytecode::OperandKind;
    let integral = match operand.kind()? {
        OperandKind::Register => state.values[usize::from(operand.payload())].value.ty.is_subtype(Type::INT32),
        OperandKind::Constant => constants.get(usize::from(operand.payload()))
            .and_then(super::super::facts::immediate).is_some_and(super::super::facts::is_int32),
        OperandKind::Field | OperandKind::Local => return None,
    };
    if integral { return operand_int(builder, registers, state, constants, operand, exit).map(Int32::word); }
    // A register only its tag types is its int where the tag says so, else its Number's
    // ToInt32; neither path proves a fact the other does not.
    let tagged = operand.register_index()
        .filter(|register| state.values[usize::from(*register)].value.representation == Representation::Tagged);
    match tagged {
        Some(register) => Some(tagged_to_int32(builder, registers, state, register, exit)),
        None => {
            let number = operand_number(builder, registers, state, constants, operand, exit)?;
            Some(crate::compiled::emit_number_to_int32(builder, number, exit))
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
    let truncated = crate::compiled::emit_number_to_int32(builder, number, exit);
    emit(builder).jump(join, &[truncated]);
    builder.switch_to_block(join);
    state.proofs.int32_projections.push((bits, int32));
    int32
}

/// Whether the 32-bit `int` is the whole of the 64-bit `word`.
fn int_is_word(builder: &mut FunctionBuilder<'_>, int: ir::Value, word: ir::Value) -> ir::Value {
    let mut stage = emit(builder);
    let widened = stage.raw_unary(Unary::SignExtend(Lane::I64), int);
    stage.raw_compare(Relation::Equal, widened, word)
}

/// Leaves to `otherwise` unless `condition` holds, which a register's proved type makes certain (`proved`).
fn guard_unless(builder: &mut FunctionBuilder<'_>, condition: ir::Value, proved: bool, otherwise: ir::Block) {
    if !proved {
        emit_js_guard(builder, condition, otherwise);
    }
}

/// `register`'s int32 value: by its fact, or by the staged guard of the representation it holds.
pub(crate) fn register_int_value(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register, otherwise: ir::Block) -> Int32<ir::Value> {
    let index = usize::from(register);
    let fact = state.values[index];
    let typed = IntFact::of_type(fact.value.ty);
    let int = match fact.value.representation {
        Representation::Int32 => {
            let variable = register_int(builder, registers, index);
            let held = IntFact::of_representation(Representation::Int32).expect("an int32 home");
            return Int32::by_fact(builder.use_var(variable), held);
        }
        Representation::Float64 if !fact.tagged.current() => float_home_int(builder, registers, index, typed, otherwise),
        Representation::Int64 => {
            let variable = register_long(builder, registers, index);
            let (word, mut stage) = (builder.use_var(variable), emit(builder));
            match typed {
                Some(proof) => Int32::by_fact(stage.raw_unary(Unary::Narrow(Lane::I32), word), proof),
                None => Int32::of_long(&mut stage, word, otherwise).expect("an emitted guard continues"),
            }
        }
        Representation::Tagged | Representation::Float64 => {
            let (bits, mut stage) = (register_tagged(builder, registers, state, register), emit(builder)); // Staged guard, or the type's proof.
            match typed {
                Some(proof) => Int32::by_fact(stage.narrow(bits), proof),
                None => Int32::guard(&mut stage, bits, otherwise).expect("an emitted guard continues"),
            }
        }
    };
    let variable = register_int(builder, registers, index);
    builder.def_var(variable, int.word());
    state.values[index] = RegisterFact { tagged: state.values[index].tagged,
        ..RegisterFact::definition(Type::INT32, Representation::Int32) };
    int
}

/// The int32 of a register held as a binary64: the staged exactness guard, or the truncation
/// alone where its type proves it an int32.
fn float_home_int(builder: &mut FunctionBuilder<'_>, registers: &mut Registers, index: usize,
    typed: Option<IntFact>, otherwise: ir::Block) -> Int32<ir::Value> {
    let variable = register_float(builder, registers, index);
    let home = NumberFact::of_representation(Representation::Float64).expect("a float home");
    let (number, mut stage) = (Float64::by_fact(builder.use_var(variable), home), emit(builder));
    match typed {
        Some(proof) => Int32::by_fact(quench_runtime::staged::value::exact_int32(&mut stage, number.word()).0, proof),
        None => Int32::exact(&mut stage, number, otherwise).expect("an emitted guard continues"),
    }
}

/// The binary64 view a test already proved of tagged `bits` in this block (`LocalProofs::numbers`), if any.
pub(crate) fn known_number(builder: &FunctionBuilder<'_>, state: &State, bits: ir::Value) -> Option<ir::Value> {
    let bits = builder.func.dfg.resolve_aliases(bits);
    state.proofs.numbers.iter().find(|(source, _)| *source == bits).map(|(_, number)| *number)
}

/// `register`'s binary64 value, proving Number where the fact does not.
pub(crate) fn register_number_value(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
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
            crate::compiled::object_slots::emit(builder).i32_to_f64(int)
        }
        Representation::Int64 => {
            let variable = register_long(builder, registers, index);
            let word = builder.use_var(variable);
            let number = emit(builder).raw_unary(Unary::FromSigned(Lane::F64), word);
            let variable = register_float(builder, registers, index);
            builder.def_var(variable, number);
            state.values[index] = RegisterFact { tagged: state.values[index].tagged,
                ..RegisterFact::definition(fact.ty, Representation::Float64) };
            number
        }
        Representation::Tagged => {
            let bits = register_tagged(builder, registers, state, register);
            let bits = builder.func.dfg.resolve_aliases(bits);
            let known = known_number(builder, state, bits);
            let number = if let Some(number) = known { number } else if fact.ty.is_subtype(Type::NUMBER) {
                emit_known_number(builder, bits)
            } else {
                emit_js_tagged_number(builder, bits, otherwise)
            };
            if known.is_none() { state.proofs.note_number(bits, number); }
            let variable = register_float(builder, registers, index);
            builder.def_var(variable, number);
            state.values[index] = RegisterFact { tagged: state.values[index].tagged,
                ..RegisterFact::definition(Type::NUMBER, Representation::Float64) };
            number
        }
    }
}
