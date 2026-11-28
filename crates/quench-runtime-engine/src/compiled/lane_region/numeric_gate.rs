use super::*;

pub(crate) fn restartable_numeric_function(program: &ResidualProgram, function: &quench_runtime::bytecode::Function,
    instructions: impl IntoIterator<Item = WideInstruction>) -> bool {
    crate::js_compiled::function_candidate(function, program) && numeric_instructions(program, instructions)
}

pub(super) fn numeric_instructions(program: &ResidualProgram, instructions: impl IntoIterator<Item = WideInstruction>) -> bool {
    instructions.into_iter().all(|instruction| numeric_instruction(program, instruction))
}

pub(super) fn numeric_instruction(program: &ResidualProgram, instruction: WideInstruction) -> bool {
    if instruction.writes_current_this()
        || (instruction.result_flags() != 0 && !instruction.returns_from_frame()) { return false; }
    match () {
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) => program.constants.get(instruction.constant_index())
            .and_then(super::super::facts::immediate).is_some(),
        _ if instruction.op().traits().has(quench_runtime::bytecode::Traits::SCALAR_REGION) => true,
        _ => quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary)
            && instruction.numeric_local_target().is_none(),
    }
}

pub(super) fn register_offset(register: Register) -> i32 {
    i32::from(register) * std::mem::size_of::<u64>() as i32
}

pub(crate) fn int_tagged(builder: &mut FunctionBuilder<'_>, int: ir::Value) -> ir::Value {
    let mut stage = emit(builder);
    let payload = stage.raw_unary(Unary::ZeroExtend(Lane::I64), int);
    quench_runtime::staged::value::int_word(&mut stage, payload)
}

pub(crate) fn is_int_tagged(builder: &mut FunctionBuilder<'_>, bits: ir::Value) -> ir::Value {
    quench_runtime::staged::value::is_int(&mut emit(builder), bits)
}

/// The tagged Boolean a predicate `holds` selects: the boxed `true` where it holds, else the boxed `false`.
pub(crate) fn boolean_word(builder: &mut FunctionBuilder<'_>, holds: ir::Value) -> ir::Value {
    let yes = builder.ins().iconst(types::I64, Value::TRUE.to_bits() as i64);
    let no = builder.ins().iconst(types::I64, Value::FALSE.to_bits() as i64);
    builder.ins().select(holds, yes, no)
}

/// A block marked cold before any code is lowered into it, for the exits and slow paths the hot path leaves to.
pub(crate) fn cold_block(builder: &mut FunctionBuilder<'_>) -> ir::Block {
    let block = builder.create_block();
    builder.set_cold_block(block);
    block
}

/// The registers an instruction writes, by its operand layouts.
pub(crate) fn writes(instruction: WideInstruction) -> super::super::facts::Writes {
    instruction.written_registers().collect()
}

impl<'f> Builds<'f> for Lowering<'_, 'f> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'f> {
        &mut self.builder
    }
}

