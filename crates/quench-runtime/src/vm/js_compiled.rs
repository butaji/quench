//! Cranelift code generation for a deliberately small, guarded JavaScript subset.
//!
//! Native entry arguments are guarded Number values. Numeric functions return
//! `f64`; static-string decision trees return a bytecode Return PC so the VM
//! can load its rooted constant and preserve ordinary frame unwinding.

use crate::bytecode::{
    Constant, Function, Op, OperandKind, ProgramKind, RETURN_REGISTER, ResidualProgram,
};
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, UserFuncName, condcodes::FloatCC, immediates::Ieee64, types,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use oxc_ast::ast::BinaryOperator;

/// Native code and its result interpretation for a guarded JavaScript function.
pub(super) struct JsJitArtifact {
    /// Machine code for `extern "C" fn(*const f64) -> f64`.
    pub(super) code: Vec<u8>,
    /// The number of formal parameters read from the argument array.
    pub(super) params: usize,
    /// Numeric results retain the original ABI; string results return the selected Return PC.
    pub(super) result: JsJitResultKind,
}

pub(super) enum JsJitResultKind {
    Number,
    StringReturnPc(Vec<(u32, u32)>),
}

// Keep straight-line native regions short so an interpreter interrupt cannot
// be delayed by a large, uninterruptible compiled body.
const MAX_INSTRUCTIONS: usize = 256;
const MAX_REGISTERS: usize = 256;
const MAX_CODE_BYTES: usize = 64 * 1024;
const MAX_PARAMETERS: usize = 8;

/// Compile `function_id` when all reachable behavior is represented by the
/// supported straight-line bytecode subset. Every unsupported shape is left
/// to the interpreter.
pub(super) fn compile(program: &ResidualProgram, function_id: u32) -> Option<JsJitArtifact> {
    if program.kind == ProgramKind::Wasm || function_id == 0 {
        return None;
    }
    let function = program.functions.get(function_id as usize)?;
    if function.code.iter().any(|packed| {
        !packed.is_wide() && matches!(packed.as_wide().op(), Op::Jump | Op::JumpBinaryFalse)
    }) {
        compile_string_branches(program, function_id, function)
    } else {
        compile_numeric(program, function_id)
    }
}

fn compile_numeric(program: &ResidualProgram, function_id: u32) -> Option<JsJitArtifact> {
    let function = program.functions.get(function_id as usize)?;
    if !function_candidate(function, &program.atoms)
        || function
            .code
            .iter()
            .any(|instruction| instruction.is_wide())
    {
        return None;
    }

    let params = usize::from(function.params);
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").ok()?;
    let isa_builder = cranelift_native::builder().ok()?;
    let isa = isa_builder.finish(settings::Flags::new(flags)).ok()?;

    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(AbiParam::new(isa.pointer_type()));
    signature.returns.push(AbiParam::new(types::F64));
    let name = UserFuncName::user(0x5155_454e, function_id);
    let mut native = ir::Function::with_name_signature(name, signature);
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);

    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let arguments = builder.block_params(entry)[0];

    // Registers are SSA values here. The validated plain activation places its
    // promoted parameter locals in registers 0..params.
    let mut registers = vec![None; usize::from(function.registers)];
    for (index, register) in registers.iter_mut().take(params).enumerate() {
        let offset = i32::try_from(index.checked_mul(std::mem::size_of::<f64>())?).ok()?;
        *register = Some(builder.ins().load(
            types::F64,
            ir::MemFlagsData::trusted(),
            arguments,
            offset,
        ));
    }

    let mut returned = false;
    for (pc, packed) in function.code.iter().copied().enumerate() {
        if returned || packed.is_wide() {
            return None;
        }
        let instruction = packed.as_wide();
        let result = match instruction.op() {
            Op::LoadConst => {
                if instruction.returns_from_frame() || instruction.result_flags() != 0 {
                    return None;
                }
                let Constant::Number(value) =
                    program.constants.get(instruction.constant_index())?
                else {
                    return None;
                };
                Some(builder.ins().f64const(Ieee64::with_bits(value.to_bits())))
            }
            Op::Move => {
                if instruction.returns_from_frame() || instruction.result_flags() != 0 {
                    return None;
                }
                let source = usize::from(instruction.register_b());
                Some(*registers.get(source)?.as_ref()?)
            }
            Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                if instruction.numeric_local_target().is_some()
                    || (instruction.result_flags() != 0
                        && instruction.result_flags() != RETURN_REGISTER)
                {
                    return None;
                }
                let (left, right) = (
                    operand_value(instruction.operand_b(), &registers)?,
                    operand_value(instruction.operand_c(), &registers)?,
                );
                let value = match (instruction.op(), instruction.binary_operator()) {
                    (Op::Binary, op) if op == BinaryOperator::Addition as u32 => {
                        builder.ins().fadd(left, right)
                    }
                    (Op::Binary, op) if op == BinaryOperator::Subtraction as u32 => {
                        builder.ins().fsub(left, right)
                    }
                    (Op::Binary, op) if op == BinaryOperator::Multiplication as u32 => {
                        builder.ins().fmul(left, right)
                    }
                    (Op::Binary, op) if op == BinaryOperator::Division as u32 => {
                        builder.ins().fdiv(left, right)
                    }
                    (Op::NumericAdd, op) if op == BinaryOperator::Addition as u32 => {
                        builder.ins().fadd(left, right)
                    }
                    (Op::NumericMultiply, op) if op == BinaryOperator::Multiplication as u32 => {
                        builder.ins().fmul(left, right)
                    }
                    _ => return None,
                };
                if instruction.returns_from_frame() {
                    // The residual marks this operation as the semantic
                    // return edge. Later bytecodes are unreachable; return
                    // through the native ABI, then resume at the residual's
                    // terminal Return so normal frame unwinding still runs.
                    builder.ins().return_(&[value]);
                    returned = true;
                    break;
                }
                Some(value)
            }
            Op::Return => {
                if instruction.result_flags() != 0 || pc + 1 != function.code.len() {
                    return None;
                }
                let value = *registers
                    .get(usize::from(instruction.register_a()))?
                    .as_ref()?;
                builder.ins().return_(&[value]);
                returned = true;
                None
            }
            _ => return None,
        };
        if let Some(value) = result {
            let destination = usize::from(instruction.result_register());
            *registers.get_mut(destination)? = Some(value);
        }
    }
    if !returned {
        return None;
    }

    builder.finalize(isa.frontend_config());
    let mut context = cranelift_codegen::Context::for_function(native);
    let compiled = context
        .compile(
            &*isa,
            &mut cranelift_codegen::control::ControlPlane::default(),
        )
        .ok()?;
    if !compiled.buffer.relocs().is_empty() {
        return None;
    }
    let code = compiled.code_buffer().to_vec();
    if code.is_empty() || code.len() > MAX_CODE_BYTES {
        return None;
    }
    Some(JsJitArtifact {
        code,
        params,
        result: JsJitResultKind::Number,
    })
}

/// Compile a forward-only decision tree whose conditions compare Numbers and
/// whose leaves return static string constants. The native result is a Return
/// PC, never a heap pointer; the VM materializes the already-rooted constant.
fn compile_string_branches(
    program: &ResidualProgram,
    function_id: u32,
    function: &Function,
) -> Option<JsJitArtifact> {
    if !function_candidate(function, &program.atoms)
        || usize::from(function.registers) > MAX_REGISTERS
        || function
            .code
            .iter()
            .any(|instruction| instruction.is_wide())
    {
        return None;
    }

    let code_len = function.code.len();
    let mut reachable = vec![false; code_len];
    let mut predecessors = vec![0_u8; code_len];
    reachable[0] = true;
    for pc in 0..code_len {
        if !reachable[pc] {
            continue;
        }
        let instruction = function.code[pc].as_wide();
        let targets: smallvec::SmallVec<[usize; 2]> = match instruction.op() {
            Op::Jump => smallvec::smallvec![instruction.jump_target() as usize],
            Op::JumpBinaryFalse => smallvec::smallvec![pc + 1, instruction.jump_target() as usize,],
            Op::Return => smallvec::SmallVec::new(),
            Op::LoadConst | Op::Move => smallvec::smallvec![pc + 1],
            _ => return None,
        };
        for target in targets {
            if target <= pc || target >= code_len {
                return None;
            }
            predecessors[target] = predecessors[target].checked_add(1)?;
            if predecessors[target] > 1 {
                // Joins need a register phi; keep this tier to trees with one
                // incoming state per bytecode PC.
                return None;
            }
            reachable[target] = true;
        }
    }

    let params = usize::from(function.params);
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").ok()?;
    let isa_builder = cranelift_native::builder().ok()?;
    let isa = isa_builder.finish(settings::Flags::new(flags)).ok()?;

    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(AbiParam::new(isa.pointer_type()));
    signature.returns.push(AbiParam::new(types::I32));
    let name = UserFuncName::user(0x5155_454e, function_id);
    let mut native = ir::Function::with_name_signature(name, signature);
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let registers: Vec<_> = (0..usize::from(function.registers))
        .map(|_| builder.declare_var(types::F64))
        .collect();
    let blocks: Vec<_> = reachable
        .iter()
        .map(|is_reachable| is_reachable.then(|| builder.create_block()))
        .collect();

    let entry = blocks[0]?;
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let arguments = builder.block_params(entry)[0];
    for (index, register) in registers.iter().take(params).enumerate() {
        let offset = i32::try_from(index.checked_mul(std::mem::size_of::<f64>())?).ok()?;
        let value = builder
            .ins()
            .load(types::F64, ir::MemFlagsData::trusted(), arguments, offset);
        builder.def_var(*register, value);
    }

    // The supported tree has no merges, so each PC has one incoming abstract
    // register state. String constants are indices into the VM's rooted pool.
    let mut states = vec![None::<Vec<Option<StringJitRegister>>>; code_len];
    let mut entry_state = vec![None; registers.len()];
    for state in entry_state.iter_mut().take(params) {
        *state = Some(StringJitRegister::Number);
    }
    states[0] = Some(entry_state);
    let mut return_sites = Vec::new();
    for pc in 0..code_len {
        if !reachable[pc] {
            continue;
        }
        let block = blocks[pc]?;
        if pc != 0 {
            // All predecessors are smaller PCs because backward edges were
            // rejected, so the block can be sealed before it is emitted.
            builder.seal_block(block);
            builder.switch_to_block(block);
        }
        let mut state = states[pc].take()?;
        let instruction = function.code[pc].as_wide();
        match instruction.op() {
            Op::LoadConst => {
                if instruction.result_flags() != 0 {
                    return None;
                }
                let constant_index = instruction.constant_index();
                if !matches!(
                    program.constants.get(constant_index)?,
                    Constant::String(_) | Constant::StringUnits(_)
                ) {
                    return None;
                }
                let destination = usize::from(instruction.result_register());
                *state.get_mut(destination)? = Some(StringJitRegister::String(
                    u32::try_from(constant_index).ok()?,
                ));
                push_string_state(&mut states, pc + 1, state)?;
                builder.ins().jump(blocks[pc + 1]?, &[]);
            }
            Op::Move => {
                if instruction.result_flags() != 0 {
                    return None;
                }
                let source = usize::from(instruction.register_b());
                let destination = usize::from(instruction.result_register());
                let source_state = *state.get(source)?;
                match source_state? {
                    StringJitRegister::Number => {
                        let value = builder.use_var(*registers.get(source)?);
                        builder.def_var(*registers.get(destination)?, value);
                    }
                    StringJitRegister::String(_) => {}
                }
                *state.get_mut(destination)? = source_state;
                push_string_state(&mut states, pc + 1, state)?;
                builder.ins().jump(blocks[pc + 1]?, &[]);
            }
            Op::Jump => {
                let target = instruction.jump_target() as usize;
                push_string_state(&mut states, target, state)?;
                builder.ins().jump(blocks[target]?, &[]);
            }
            Op::JumpBinaryFalse => {
                let Some(condition) =
                    comparison(&mut builder, program, instruction, &registers, &state)
                else {
                    return None;
                };
                let fallthrough = pc + 1;
                let target = instruction.jump_target() as usize;
                push_string_state(&mut states, fallthrough, state.clone())?;
                push_string_state(&mut states, target, state)?;
                // JumpBinaryFalse jumps when the comparison is false.
                builder
                    .ins()
                    .brif(condition, blocks[fallthrough]?, &[], blocks[target]?, &[]);
            }
            Op::Return => {
                let register = usize::from(instruction.register_a());
                if instruction.result_flags() != 0 {
                    return None;
                }
                let Some(StringJitRegister::String(constant)) =
                    state.get(register).copied().flatten()
                else {
                    return None;
                };
                let return_pc = i32::try_from(pc).ok()?;
                let return_pc = builder.ins().iconst(types::I32, i64::from(return_pc));
                builder.ins().return_(&[return_pc]);
                return_sites.push((u32::try_from(pc).ok()?, constant));
            }
            _ => return None,
        }
    }
    if return_sites.is_empty() {
        return None;
    }

    builder.finalize(isa.frontend_config());
    let mut context = cranelift_codegen::Context::for_function(native);
    let compiled = context
        .compile(
            &*isa,
            &mut cranelift_codegen::control::ControlPlane::default(),
        )
        .ok()?;
    if !compiled.buffer.relocs().is_empty() {
        return None;
    }
    let code = compiled.code_buffer().to_vec();
    if code.is_empty() || code.len() > MAX_CODE_BYTES {
        return None;
    }
    Some(JsJitArtifact {
        code,
        params,
        result: JsJitResultKind::StringReturnPc(return_sites),
    })
}

fn push_string_state(
    states: &mut [Option<Vec<Option<StringJitRegister>>>],
    target: usize,
    state: Vec<Option<StringJitRegister>>,
) -> Option<()> {
    let slot = states.get_mut(target)?;
    if slot.is_some() {
        return None;
    }
    *slot = Some(state);
    Some(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StringJitRegister {
    Number,
    String(u32),
}

fn comparison(
    builder: &mut cranelift_frontend::FunctionBuilder<'_>,
    program: &ResidualProgram,
    instruction: crate::bytecode::WideInstruction,
    registers: &[cranelift_frontend::Variable],
    strings: &[Option<StringJitRegister>],
) -> Option<ir::Value> {
    let operator = instruction.binary_operator_field();
    let code = match operator {
        value if value == BinaryOperator::LessThan as u32 => FloatCC::LessThan,
        value if value == BinaryOperator::LessEqualThan as u32 => FloatCC::LessThanOrEqual,
        value if value == BinaryOperator::GreaterThan as u32 => FloatCC::GreaterThan,
        value if value == BinaryOperator::GreaterEqualThan as u32 => FloatCC::GreaterThanOrEqual,
        _ => return None,
    };
    let left = numeric_operand(
        builder,
        program,
        instruction.operand_b(),
        registers,
        strings,
    )?;
    let right = numeric_operand(
        builder,
        program,
        instruction.operand_c(),
        registers,
        strings,
    )?;
    Some(builder.ins().fcmp(code, left, right))
}

fn numeric_operand(
    builder: &mut cranelift_frontend::FunctionBuilder<'_>,
    program: &ResidualProgram,
    operand: crate::bytecode::Operand,
    registers: &[cranelift_frontend::Variable],
    strings: &[Option<StringJitRegister>],
) -> Option<ir::Value> {
    match operand.kind()? {
        OperandKind::Register => {
            let register = usize::from(operand.register_index()?);
            if strings.get(register).copied().flatten() != Some(StringJitRegister::Number) {
                return None;
            }
            Some(builder.use_var(*registers.get(register)?))
        }
        OperandKind::Constant => {
            let index = usize::from(operand.payload());
            let Constant::Number(value) = program.constants.get(index)? else {
                return None;
            };
            Some(builder.ins().f64const(Ieee64::with_bits(value.to_bits())))
        }
        OperandKind::Field | OperandKind::Local => None,
    }
}

/// Cheap activation-level filter used before retaining hotness state. The
/// bytecode scan remains deferred until the function actually crosses the
/// statistical promotion gate.
pub(super) fn function_candidate(function: &Function, atoms: &crate::bytecode::AtomTable) -> bool {
    !function.code.is_empty()
        && function.code.len() <= MAX_INSTRUCTIONS
        && function.wide.is_empty()
        && usize::from(function.registers) <= MAX_REGISTERS
        && usize::from(function.params) <= MAX_PARAMETERS
        && eligible(function, atoms)
}

fn eligible(function: &Function, atoms: &crate::bytecode::AtomTable) -> bool {
    if function.parent.is_none()
        || function.is_async
        || function.is_generator
        || function.is_class_constructor
        || function.derived_constructor
        || function.class_initializer
        || function.instance_initializer.is_some()
        || function.rest
        || !function.simple_parameters
        || !function.handlers.is_empty()
        || !function.plain_activation(atoms)
    {
        return false;
    }

    let params = usize::from(function.params);
    if params > usize::from(function.registers) || params > function.local_registers.len() {
        return false;
    }
    function
        .local_registers
        .iter()
        .take(params)
        .enumerate()
        .all(|(index, binding)| {
            usize::from(binding.local) == index && usize::from(binding.register) == index
        })
}

fn operand_value(
    operand: crate::bytecode::Operand,
    registers: &[Option<ir::Value>],
) -> Option<ir::Value> {
    if operand.kind() != Some(OperandKind::Register) {
        return None;
    }
    registers
        .get(usize::from(operand.register_index()?))?
        .as_ref()
        .copied()
}
