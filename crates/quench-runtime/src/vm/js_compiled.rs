//! Cranelift code generation for a deliberately small, guarded JavaScript subset.
//!
//! Native entry arguments are guarded Number values. Numeric functions return
//! `f64`; static-string decision trees return a bytecode Return PC so the VM
//! can load its rooted constant and preserve ordinary frame unwinding.

use crate::bytecode::{
    Constant, FieldBase, FieldLookup, Function, Op, OperandKind, ProgramKind, RETURN_REGISTER,
    ResidualProgram,
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
    NumberInterruptible,
    StringHashLoop,
    StringReturnPc(Vec<(u32, u32)>),
    Boolean,
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
    if string_hash_loop_candidate(program, function) {
        return compile_string_hash_loop(program, function_id, function);
    }
    if function.code.iter().any(|packed| {
        !packed.is_wide()
            && matches!(
                packed.as_wide().op(),
                Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse
            )
            || (packed.as_wide().op() == Op::Unary
                && packed.as_wide().unary_operator()
                    == oxc_ast::ast::UnaryOperator::LogicalNot as u32)
    }) {
        compile_numeric_loop(program, function_id, function)
            .or_else(|| compile_number_domain_boolean_tree(program, function_id, function))
            .or_else(|| compile_string_branches(program, function_id, function))
    } else {
        compile_numeric(program, function_id)
    }
}

pub(super) fn has_string_hash_loop_candidate(
    program: &ResidualProgram,
    function: &Function,
) -> bool {
    string_hash_loop_candidate(program, function)
}

fn string_hash_loop_candidate(program: &ResidualProgram, function: &Function) -> bool {
    if function.params != 1 || function.code.len() != 20 || !function.wide.is_empty() {
        return false;
    }
    let Some(get_length) = function
        .code
        .get(2)
        .map(|instruction| instruction.as_wide())
    else {
        return false;
    };
    let Some(get_char_code) = function
        .code
        .get(8)
        .map(|instruction| instruction.as_wide())
    else {
        return false;
    };
    let field_is = |instruction: crate::bytecode::WideInstruction, name: &str| {
        instruction.op() == Op::GetField
            && matches!(
                instruction.field_lookup(),
                Some(FieldLookup::Atom {
                    atom,
                    base: FieldBase(0),
                    ..
                }) if (atom as usize) < program.atoms.len() && &program.atoms[atom as usize] == name
            )
    };
    if !field_is(get_length, "length") || !field_is(get_char_code, "charCodeAt") {
        return false;
    }
    let ops = [
        Op::LoadConst,
        Op::StoreLocalPlain,
        Op::GetField,
        Op::StoreLocalPlain,
        Op::LoadLocalPlain,
        Op::JumpBinaryFalse,
        Op::LoadLocalPlain,
        Op::Binary,
        Op::GetField,
        Op::LoadLocalPlain,
        Op::Call,
        Op::Binary,
        Op::Binary,
        Op::LoadLocalPlain,
        Op::Binary,
        Op::Jump,
        Op::LoadLocalPlain,
        Op::Binary,
        Op::LoadConst,
        Op::Return,
    ];
    if function
        .code
        .iter()
        .zip(ops)
        .any(|(packed, op)| packed.as_wide().op() != op)
    {
        return false;
    }
    let instruction = |pc: usize| function.code[pc].as_wide();
    let constant = |pc: usize| match program.constants.get(instruction(pc).constant_index()) {
        Some(Constant::Number(value)) => Some(*value),
        _ => None,
    };
    let operand_constant = |operand: crate::bytecode::Operand| {
        (operand.kind() == Some(OperandKind::Constant))
            .then(|| program.constants.get(usize::from(operand.payload())))
            .flatten()
            .and_then(|constant| match constant {
                Constant::Number(value) => Some(*value),
                _ => None,
            })
    };
    let jump = instruction(5);
    let call = instruction(10);
    constant(0) == Some(5381.0)
        && instruction(1).local_slot() == 1
        && instruction(2).field_lookup().is_some()
        && instruction(3).local_slot() == 2
        && instruction(4).local_slot() == 2
        && instruction(0).result_register() == 2
        && instruction(1).register_a() == 2
        && instruction(2).result_register() == 2
        && instruction(3).register_a() == 2
        && instruction(4).result_register() == 2
        && jump.jump_target() == 16
        && super::operations::NumberRelation::from_operator(jump.binary_operator_field())
            == Some(super::operations::NumberRelation::GreaterThan)
        && jump.operand_b().kind() == Some(OperandKind::Register)
        && jump.operand_b().payload() == 2
        && operand_constant(jump.operand_c()) == Some(0.0)
        && instruction(6).local_slot() == 1
        && instruction(6).result_register() == 3
        && instruction_binary_operator(instruction(7))
            == Some(BinaryOperator::Multiplication as u32)
        && instruction(7).result_register() == 4
        && operand_register(instruction(7).operand_b()) == Some(3)
        && operand_constant(instruction(7).operand_c()) == Some(33.0)
        && instruction(8).result_register() == 5
        && instruction(9).local_slot() == 2
        && instruction(9).result_register() == 7
        && call.register_b() == instruction(8).result_register()
        && call.call_window().base == instruction(9).result_register()
        && call.call_window().count == 1
        && instruction_binary_operator(instruction(11)) == Some(BinaryOperator::BitwiseOR as u32)
        && operand_register(instruction(11).operand_b()) == Some(8)
        && operand_constant(instruction(11).operand_c()) == Some(0.0)
        && instruction_binary_operator(instruction(12)) == Some(BinaryOperator::BitwiseXOR as u32)
        && instruction(12).numeric_local_target() == Some(1)
        && operand_register(instruction(12).operand_b()) == Some(4)
        && operand_register(instruction(12).operand_c()) == Some(9)
        && instruction(13).local_slot() == 2
        && instruction(13).result_register() == 2
        && instruction_binary_operator(instruction(14)) == Some(BinaryOperator::Subtraction as u32)
        && instruction(14).numeric_local_target() == Some(2)
        && operand_register(instruction(14).operand_b()) == Some(2)
        && operand_constant(instruction(14).operand_c()) == Some(919.0)
        && instruction(15).jump_target() == 4
        && instruction(16).local_slot() == 1
        && instruction(16).result_register() == 2
        && instruction(17).returns_from_frame()
        && instruction_binary_operator(instruction(17)) == Some(BinaryOperator::BitwiseOR as u32)
        && operand_register(instruction(17).operand_b()) == Some(2)
        && operand_constant(instruction(17).operand_c()) == Some(0.0)
        && instruction(18).constant_index() < program.constants.len()
        && instruction(18).result_register() == 2
        && instruction(19).register_a() == instruction(18).result_register()
}

fn operand_register(operand: crate::bytecode::Operand) -> Option<u16> {
    (operand.kind() == Some(OperandKind::Register)).then_some(operand.payload())
}

fn instruction_binary_operator(instruction: crate::bytecode::WideInstruction) -> Option<u32> {
    match instruction.op().immediate_role() {
        crate::bytecode::ImmediateRole::BinaryOperator
        | crate::bytecode::ImmediateRole::AdditionOperator
        | crate::bytecode::ImmediateRole::MultiplicationOperator => {
            Some(instruction.binary_operator())
        }
        _ if instruction.op() == Op::JumpBinaryFalse => Some(instruction.binary_operator_field()),
        _ => None,
    }
}

fn compile_string_hash_loop(
    program: &ResidualProgram,
    function_id: u32,
    function: &Function,
) -> Option<JsJitArtifact> {
    if !function_candidate(function, &program.atoms) {
        // The string-hash matcher has already checked the bytecode shape; the
        // regular candidate guard still enforces activation and size limits.
        return None;
    }
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").ok()?;
    let isa_builder = cranelift_native::builder().ok()?;
    let isa = isa_builder.finish(settings::Flags::new(flags)).ok()?;
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(AbiParam::new(isa.pointer_type()));
    signature.params.push(AbiParam::new(isa.pointer_type()));
    signature.params.push(AbiParam::new(types::I32));
    signature.params.push(AbiParam::new(isa.pointer_type()));
    signature.params.push(AbiParam::new(isa.pointer_type()));
    signature.returns.push(AbiParam::new(types::F64));
    let mut native =
        ir::Function::with_name_signature(UserFuncName::user(0x5155_454e, function_id), signature);
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let units = builder.block_params(entry)[0];
    let length = builder.block_params(entry)[1];
    let ascii = builder.block_params(entry)[2];
    let interrupt = builder.block_params(entry)[3];
    let status = builder.block_params(entry)[4];
    let length = builder
        .ins()
        .load(isa.pointer_type(), ir::MemFlagsData::trusted(), length, 0);
    let length = builder.ins().fcvt_from_uint(types::F64, length);
    let index_variable = builder.declare_var(types::F64);
    builder.def_var(index_variable, length);
    let hash_variable = builder.declare_var(types::I32);
    let hash = builder.ins().iconst(types::I32, 5381);
    builder.def_var(hash_variable, hash);
    let counter_variable = builder.declare_var(types::I32);
    let zero = builder.ins().iconst(types::I32, 0);
    builder.def_var(counter_variable, zero);
    let header = builder.create_block();
    let body = builder.create_block();
    let load_character = builder.create_block();
    let load_ascii_character = builder.create_block();
    let load_wide_character = builder.create_block();
    let zero_character = builder.create_block();
    let join_character = builder.create_block();
    let character = builder.append_block_param(join_character, types::I32);
    let normal_exit = builder.create_block();
    let interrupt_exit = builder.create_block();
    let interrupt_status = builder.append_block_param(interrupt_exit, isa.pointer_type());
    builder.ins().jump(header, &[]);

    builder.switch_to_block(header);
    let index = builder.use_var(index_variable);
    let zero_number = builder.ins().f64const(Ieee64::with_bits(0.0f64.to_bits()));
    let more = builder.ins().fcmp(FloatCC::GreaterThan, index, zero_number);
    builder.ins().brif(more, body, &[], normal_exit, &[]);

    builder.switch_to_block(body);
    let index = builder.use_var(index_variable);
    let in_range = builder.ins().fcmp(FloatCC::LessThan, index, length);
    builder
        .ins()
        .brif(in_range, load_character, &[], zero_character, &[]);

    builder.switch_to_block(load_character);
    let index = builder.use_var(index_variable);
    let index = builder.ins().fcvt_to_sint_sat(isa.pointer_type(), index);
    let is_ascii =
        builder
            .ins()
            .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::Equal, ascii, 1);
    builder.ins().brif(
        is_ascii,
        load_ascii_character,
        &[],
        load_wide_character,
        &[],
    );

    builder.switch_to_block(load_ascii_character);
    let address = builder.ins().iadd(units, index);
    let code_unit = builder
        .ins()
        .load(types::I8, ir::MemFlagsData::trusted(), address, 0);
    let code_unit = builder.ins().uextend(types::I32, code_unit);
    builder
        .ins()
        .jump(join_character, &[ir::BlockArg::Value(code_unit)]);

    builder.switch_to_block(load_wide_character);
    let index = builder.use_var(index_variable);
    let index = builder.ins().fcvt_to_sint_sat(isa.pointer_type(), index);
    let byte_offset = builder.ins().ishl_imm_u(index, 1);
    let address = builder.ins().iadd(units, byte_offset);
    let code_unit = builder
        .ins()
        .load(types::I16, ir::MemFlagsData::trusted(), address, 0);
    let code_unit = builder.ins().uextend(types::I32, code_unit);
    builder
        .ins()
        .jump(join_character, &[ir::BlockArg::Value(code_unit)]);

    builder.switch_to_block(zero_character);
    let zero_character = builder.ins().iconst(types::I32, 0);
    builder
        .ins()
        .jump(join_character, &[ir::BlockArg::Value(zero_character)]);

    builder.switch_to_block(join_character);
    let hash = builder.use_var(hash_variable);
    let multiplier = builder.ins().iconst(types::I32, 33);
    let hash = builder.ins().imul(hash, multiplier);
    let hash = builder.ins().bxor(hash, character);
    builder.def_var(hash_variable, hash);
    let index = builder.use_var(index_variable);
    let stride = builder
        .ins()
        .f64const(Ieee64::with_bits(919.0f64.to_bits()));
    let index = builder.ins().fsub(index, stride);
    builder.def_var(index_variable, index);
    let counter = builder.use_var(counter_variable);
    let counter = builder.ins().iadd_imm_s(counter, 1);
    builder.def_var(counter_variable, counter);
    let interval = builder.ins().band_imm_u(counter, 1023);
    let should_poll =
        builder
            .ins()
            .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::Equal, interval, 0);
    let poll = builder.create_block();
    let continue_loop = builder.create_block();
    builder
        .ins()
        .brif(should_poll, poll, &[], continue_loop, &[]);
    builder.switch_to_block(poll);
    let requested = builder
        .ins()
        .atomic_load(types::I8, ir::MemFlagsData::trusted(), interrupt);
    let cancelled = builder.ins().icmp_imm_u(
        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
        requested,
        0,
    );
    let return_loop = builder.create_block();
    builder.ins().brif(
        cancelled,
        interrupt_exit,
        &[ir::BlockArg::Value(status)],
        return_loop,
        &[],
    );
    builder.switch_to_block(return_loop);
    builder.ins().jump(continue_loop, &[]);
    builder.switch_to_block(continue_loop);
    builder.ins().jump(header, &[]);

    builder.switch_to_block(normal_exit);
    let hash = builder.use_var(hash_variable);
    let result = builder.ins().fcvt_from_sint(types::F64, hash);
    let complete = builder.ins().iconst(types::I32, 1);
    builder
        .ins()
        .store(ir::MemFlagsData::trusted(), complete, status, 0);
    builder.ins().return_(&[result]);
    builder.switch_to_block(interrupt_exit);
    let interrupted = builder.ins().iconst(types::I32, 0);
    builder.ins().store(
        ir::MemFlagsData::trusted(),
        interrupted,
        interrupt_status,
        0,
    );
    let no_result = builder.ins().f64const(Ieee64::with_bits(0.0f64.to_bits()));
    builder.ins().return_(&[no_result]);
    builder.seal_all_blocks();
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
        params: 1,
        result: JsJitResultKind::StringHashLoop,
    })
}

pub(super) fn has_numeric_loop_candidate(function: &Function) -> bool {
    function.code.iter().enumerate().any(|(pc, instruction)| {
        if instruction.is_wide() {
            return false;
        }
        let instruction = instruction.as_wide();
        matches!(
            instruction.op(),
            Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse
        ) && usize::try_from(instruction.jump_target()).is_ok_and(|target| target <= pc)
    })
}

/// Compile a forward-only boolean tree after specializing formal parameters to
/// Number. Static `typeof` and string-equality tests are reduced while the
/// control-flow graph is discovered, so bytecodes on impossible type branches
/// do not constrain the generated function. Dynamic Number comparisons remain
/// native comparisons. Every non-Number entry is rejected by the caller's
/// guard and resumes from the untouched interpreter frame.
fn compile_number_domain_boolean_tree(
    program: &ResidualProgram,
    function_id: u32,
    function: &Function,
) -> Option<JsJitArtifact> {
    if !function_candidate(function, &program.atoms)
        || usize::from(function.registers) > MAX_REGISTERS
    {
        return None;
    }

    let code_len = function.code.len();
    let mut states = vec![None::<BranchState>; code_len];
    let mut entry = BranchState {
        registers: vec![BranchValue::Unknown; usize::from(function.registers)],
        locals: vec![BranchValue::Unknown; usize::from(function.locals)],
    };
    for value in entry
        .registers
        .iter_mut()
        .take(usize::from(function.params))
    {
        *value = BranchValue::Number;
    }
    states[0] = Some(entry);
    let mut reachable = vec![false; code_len];

    for pc in 0..code_len {
        let Some(mut state) = states[pc].clone() else {
            continue;
        };
        reachable[pc] = true;
        let instruction = function.code[pc].as_wide();
        let mut successors = smallvec::SmallVec::<[usize; 2]>::new();
        match instruction.op() {
            Op::LoadConst => {
                if instruction.result_flags() != 0 {
                    return None;
                }
                let value = match program.constants.get(instruction.constant_index())? {
                    Constant::Number(_) => {
                        BranchValue::NumberConstant(instruction.constant_index() as u32)
                    }
                    Constant::String(_) | Constant::StringUnits(_) => {
                        BranchValue::String(instruction.constant_index() as u32)
                    }
                    Constant::Boolean(value) => BranchValue::BooleanLiteral {
                        constant: instruction.constant_index() as u32,
                        value: *value,
                    },
                    _ => return None,
                };
                *state
                    .registers
                    .get_mut(usize::from(instruction.result_register()))? = value;
                successors.push(pc + 1);
            }
            Op::Move => {
                if instruction.result_flags() != 0 {
                    return None;
                }
                let source = *state.registers.get(usize::from(instruction.register_b()))?;
                *state
                    .registers
                    .get_mut(usize::from(instruction.result_register()))? = source;
                successors.push(pc + 1);
            }
            Op::Unary => {
                if instruction.result_flags() != 0 && instruction.result_flags() != RETURN_REGISTER
                {
                    return None;
                }
                let source = *state.registers.get(usize::from(instruction.register_b()))?;
                if instruction.unary_operator() == oxc_ast::ast::UnaryOperator::LogicalNot as u32 {
                    let result = match source {
                        BranchValue::NumberConstant(constant) => {
                            let Constant::Number(value) =
                                program.constants.get(constant as usize)?
                            else {
                                return None;
                            };
                            BranchValue::BooleanFolded(*value == 0.0 || value.is_nan())
                        }
                        BranchValue::BooleanLiteral { value, .. }
                        | BranchValue::BooleanFolded(value) => BranchValue::BooleanFolded(!value),
                        _ => return None,
                    };
                    *state
                        .registers
                        .get_mut(usize::from(instruction.result_register()))? = result;
                    if instruction.result_flags() == 0 {
                        successors.push(pc + 1);
                    }
                    continue;
                }
                if instruction.unary_operator() != oxc_ast::ast::UnaryOperator::Typeof as u32 {
                    return None;
                }
                let type_name = match source {
                    BranchValue::Number | BranchValue::NumberConstant(_) => BranchType::Number,
                    BranchValue::String(_) => BranchType::String,
                    BranchValue::BooleanLiteral { .. } | BranchValue::BooleanFolded(_) => {
                        BranchType::Boolean
                    }
                    _ => return None,
                };
                *state
                    .registers
                    .get_mut(usize::from(instruction.result_register()))? =
                    BranchValue::TypeName(type_name);
                if instruction.result_flags() == 0 {
                    successors.push(pc + 1);
                }
            }
            Op::StoreLocal | Op::StoreLocalPlain => {
                let slot = usize::from(instruction.local_slot());
                let value = *state.registers.get(usize::from(instruction.register_a()))?;
                if !branch_value_supported_local(value) {
                    return None;
                }
                *state.locals.get_mut(slot)? = value;
                if let Some(register) = instruction.optional_register_b() {
                    *state.registers.get_mut(usize::from(register))? = value;
                }
                successors.push(pc + 1);
            }
            Op::LoadLocal | Op::LoadLocalPlain => {
                let slot = usize::from(instruction.local_slot());
                let value = *state.locals.get(slot)?;
                if !branch_value_supported_local(value) {
                    return None;
                }
                *state
                    .registers
                    .get_mut(usize::from(instruction.result_register()))? = value;
                successors.push(pc + 1);
            }
            Op::Binary => {
                if instruction.result_flags() != 0 && instruction.result_flags() != RETURN_REGISTER
                {
                    return None;
                }
                let left = branch_operand(program, &state.registers, instruction.operand_b())?;
                let right = branch_operand(program, &state.registers, instruction.operand_c())?;
                let result = match (left, right) {
                    (left, right)
                        if branch_value_is_number(left)
                            && branch_value_is_number(right)
                            && super::operations::NumberRelation::from_operator(
                                instruction.binary_operator(),
                            )
                            .is_some() =>
                    {
                        BranchValue::BooleanDynamic
                    }
                    (left, right) => BranchValue::BooleanFolded(branch_equality(
                        program,
                        instruction.binary_operator(),
                        left,
                        right,
                    )?),
                };
                *state
                    .registers
                    .get_mut(usize::from(instruction.result_register()))? = result;
                if instruction.result_flags() == 0 {
                    successors.push(pc + 1);
                }
            }
            Op::JumpFalse => {
                let condition = *state.registers.get(usize::from(instruction.register_a()))?;
                match condition {
                    BranchValue::BooleanLiteral { value, .. }
                    | BranchValue::BooleanFolded(value) => successors.push(if value {
                        pc + 1
                    } else {
                        instruction.jump_target() as usize
                    }),
                    BranchValue::BooleanDynamic => {
                        successors.push(pc + 1);
                        successors.push(instruction.jump_target() as usize);
                    }
                    _ => return None,
                }
            }
            Op::JumpTrue => {
                let condition = *state.registers.get(usize::from(instruction.register_a()))?;
                match condition {
                    BranchValue::BooleanLiteral { value, .. }
                    | BranchValue::BooleanFolded(value) => successors.push(if value {
                        instruction.jump_target() as usize
                    } else {
                        pc + 1
                    }),
                    BranchValue::BooleanDynamic => {
                        successors.push(pc + 1);
                        successors.push(instruction.jump_target() as usize);
                    }
                    _ => return None,
                }
            }
            Op::Jump => successors.push(instruction.jump_target() as usize),
            Op::JumpBinaryFalse => {
                let left = branch_operand(program, &state.registers, instruction.operand_b())?;
                let right = branch_operand(program, &state.registers, instruction.operand_c())?;
                if let Some(value) =
                    branch_equality(program, instruction.binary_operator_field(), left, right)
                {
                    successors.push(if value {
                        pc + 1
                    } else {
                        instruction.jump_target() as usize
                    });
                } else if branch_value_is_number(left)
                    && branch_value_is_number(right)
                    && super::operations::NumberRelation::from_operator(
                        instruction.binary_operator_field(),
                    )
                    .is_some()
                {
                    successors.push(pc + 1);
                    successors.push(instruction.jump_target() as usize);
                } else {
                    return None;
                }
            }
            Op::Return => {
                if instruction.result_flags() != 0
                    || !matches!(
                        state.registers.get(usize::from(instruction.register_a()))?,
                        BranchValue::BooleanLiteral { .. }
                            | BranchValue::BooleanFolded(_)
                            | BranchValue::BooleanDynamic
                    )
                {
                    return None;
                }
            }
            _ => {
                return None;
            }
        }

        for target in successors {
            if target <= pc || target >= code_len {
                return None;
            }
            match states.get_mut(target)? {
                Some(existing) => merge_branch_state(existing, &state),
                slot @ None => *slot = Some(state.clone()),
            }
        }
    }

    if !reachable.iter().enumerate().any(|(pc, is_reachable)| {
        *is_reachable
            && (function.code[pc].as_wide().op() == Op::Return
                || (function.code[pc].as_wide().op() == Op::Binary
                    && function.code[pc].as_wide().result_flags() == RETURN_REGISTER)
                || (function.code[pc].as_wide().op() == Op::Unary
                    && function.code[pc].as_wide().result_flags() == RETURN_REGISTER))
    }) {
        return None;
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
    let variables: Vec<_> = (0..usize::from(function.registers))
        .map(|_| builder.declare_var(types::F64))
        .collect();
    let local_variables: Vec<_> = (0..usize::from(function.locals))
        .map(|_| builder.declare_var(types::F64))
        .collect();
    let blocks: Vec<_> = reachable
        .iter()
        .map(|is_reachable| is_reachable.then(|| builder.create_block()))
        .collect();
    let entry_block = blocks[0]?;
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    builder.seal_block(entry_block);
    let arguments = builder.block_params(entry_block)[0];
    for (index, variable) in variables.iter().take(params).enumerate() {
        let offset = i32::try_from(index.checked_mul(std::mem::size_of::<f64>())?).ok()?;
        let value = builder
            .ins()
            .load(types::F64, ir::MemFlagsData::trusted(), arguments, offset);
        builder.def_var(*variable, value);
    }

    let mut emitted_return = false;
    for pc in 0..code_len {
        let Some(state) = states[pc].clone() else {
            continue;
        };
        let block = blocks[pc]?;
        if pc != 0 {
            builder.seal_block(block);
            builder.switch_to_block(block);
        }
        let instruction = function.code[pc].as_wide();
        match instruction.op() {
            Op::LoadConst => {
                if matches!(
                    program.constants.get(instruction.constant_index())?,
                    Constant::Number(_)
                ) {
                    let Constant::Number(value) =
                        program.constants.get(instruction.constant_index())?
                    else {
                        return None;
                    };
                    let native_value = builder.ins().f64const(Ieee64::with_bits(value.to_bits()));
                    builder.def_var(
                        *variables.get(usize::from(instruction.result_register()))?,
                        native_value,
                    );
                }
            }
            Op::StoreLocal | Op::StoreLocalPlain => {
                let source = usize::from(instruction.register_a());
                let value_kind = *state.registers.get(source)?;
                if matches!(
                    value_kind,
                    BranchValue::Number
                        | BranchValue::NumberConstant(_)
                        | BranchValue::BooleanDynamic
                ) {
                    let value = builder.use_var(*variables.get(source)?);
                    builder.def_var(
                        *local_variables.get(usize::from(instruction.local_slot()))?,
                        value,
                    );
                    if let Some(register) = instruction.optional_register_b() {
                        builder.def_var(*variables.get(usize::from(register))?, value);
                    }
                }
            }
            Op::LoadLocal | Op::LoadLocalPlain => {
                let local = usize::from(instruction.local_slot());
                if matches!(
                    state.locals.get(local),
                    Some(
                        BranchValue::Number
                            | BranchValue::NumberConstant(_)
                            | BranchValue::BooleanDynamic
                    )
                ) {
                    let value = builder.use_var(*local_variables.get(local)?);
                    builder.def_var(
                        *variables.get(usize::from(instruction.result_register()))?,
                        value,
                    );
                }
            }
            Op::Move => {
                let source = usize::from(instruction.register_b());
                if matches!(
                    state.registers.get(source),
                    Some(
                        BranchValue::Number
                            | BranchValue::NumberConstant(_)
                            | BranchValue::BooleanDynamic
                    )
                ) {
                    let value = builder.use_var(*variables.get(source)?);
                    builder.def_var(
                        *variables.get(usize::from(instruction.result_register()))?,
                        value,
                    );
                }
            }
            Op::Unary => {
                if instruction.result_flags() == RETURN_REGISTER {
                    let source = match state
                        .registers
                        .get(usize::from(instruction.register_b()))
                        .copied()?
                    {
                        BranchValue::NumberConstant(constant) => {
                            let Constant::Number(value) =
                                program.constants.get(constant as usize)?
                            else {
                                return None;
                            };
                            *value == 0.0 || value.is_nan()
                        }
                        BranchValue::BooleanLiteral { value, .. }
                        | BranchValue::BooleanFolded(value) => !value,
                        _ => return None,
                    };
                    let result = builder.ins().iconst(types::I32, i64::from(source));
                    builder.ins().return_(&[result]);
                    emitted_return = true;
                }
            }
            Op::Binary => {
                let left = branch_operand(program, &state.registers, instruction.operand_b())?;
                let right = branch_operand(program, &state.registers, instruction.operand_c())?;
                if branch_value_is_number(left) && branch_value_is_number(right) {
                    let condition = branch_number_comparison(
                        &mut builder,
                        instruction.binary_operator(),
                        &variables,
                        program,
                        &state.registers,
                        instruction.operand_b(),
                        instruction.operand_c(),
                    )?;
                    if instruction.result_flags() == RETURN_REGISTER {
                        let yes = builder.ins().iconst(types::I32, 1);
                        let no = builder.ins().iconst(types::I32, 0);
                        let result = builder.ins().select(condition, yes, no);
                        builder.ins().return_(&[result]);
                        emitted_return = true;
                    } else {
                        let yes = builder.ins().f64const(Ieee64::with_bits(1.0f64.to_bits()));
                        let no = builder.ins().f64const(Ieee64::with_bits(0.0f64.to_bits()));
                        let value = builder.ins().select(condition, yes, no);
                        builder.def_var(
                            *variables.get(usize::from(instruction.result_register()))?,
                            value,
                        );
                    }
                }
            }
            Op::Jump => {
                builder
                    .ins()
                    .jump(blocks[instruction.jump_target() as usize]?, &[]);
            }
            Op::JumpFalse => {
                match state
                    .registers
                    .get(usize::from(instruction.register_a()))
                    .copied()?
                {
                    BranchValue::BooleanLiteral { value, .. }
                    | BranchValue::BooleanFolded(value) => {
                        let target = if value {
                            pc + 1
                        } else {
                            instruction.jump_target() as usize
                        };
                        builder.ins().jump(blocks[target]?, &[]);
                    }
                    BranchValue::BooleanDynamic => {
                        let register = usize::from(instruction.register_a());
                        let value = builder.use_var(*variables.get(register)?);
                        let zero = builder.ins().f64const(Ieee64::with_bits(0.0f64.to_bits()));
                        let truthy = builder.ins().fcmp(FloatCC::NotEqual, value, zero);
                        builder.ins().brif(
                            truthy,
                            blocks[pc + 1]?,
                            &[],
                            blocks[instruction.jump_target() as usize]?,
                            &[],
                        );
                    }
                    _ => return None,
                }
            }
            Op::JumpTrue => {
                match state
                    .registers
                    .get(usize::from(instruction.register_a()))
                    .copied()?
                {
                    BranchValue::BooleanLiteral { value, .. }
                    | BranchValue::BooleanFolded(value) => {
                        let target = if value {
                            instruction.jump_target() as usize
                        } else {
                            pc + 1
                        };
                        builder.ins().jump(blocks[target]?, &[]);
                    }
                    BranchValue::BooleanDynamic => {
                        let register = usize::from(instruction.register_a());
                        let value = builder.use_var(*variables.get(register)?);
                        let zero = builder.ins().f64const(Ieee64::with_bits(0.0f64.to_bits()));
                        let truthy = builder.ins().fcmp(FloatCC::NotEqual, value, zero);
                        builder.ins().brif(
                            truthy,
                            blocks[instruction.jump_target() as usize]?,
                            &[],
                            blocks[pc + 1]?,
                            &[],
                        );
                    }
                    _ => return None,
                }
            }
            Op::JumpBinaryFalse => {
                let left = branch_operand(program, &state.registers, instruction.operand_b())?;
                let right = branch_operand(program, &state.registers, instruction.operand_c())?;
                if let Some(value) =
                    branch_equality(program, instruction.binary_operator_field(), left, right)
                {
                    let target = if value {
                        pc + 1
                    } else {
                        instruction.jump_target() as usize
                    };
                    builder.ins().jump(blocks[target]?, &[]);
                } else {
                    let condition = branch_number_comparison(
                        &mut builder,
                        instruction.binary_operator_field(),
                        &variables,
                        program,
                        &state.registers,
                        instruction.operand_b(),
                        instruction.operand_c(),
                    )?;
                    builder.ins().brif(
                        condition,
                        blocks[pc + 1]?,
                        &[],
                        blocks[instruction.jump_target() as usize]?,
                        &[],
                    );
                }
            }
            Op::Return => {
                let register = usize::from(instruction.register_a());
                let result = match state.registers.get(register).copied()? {
                    BranchValue::BooleanLiteral { value, .. }
                    | BranchValue::BooleanFolded(value) => {
                        builder.ins().iconst(types::I32, i64::from(value))
                    }
                    BranchValue::BooleanDynamic => {
                        let value = builder.use_var(*variables.get(register)?);
                        let zero = builder.ins().f64const(Ieee64::with_bits(0.0f64.to_bits()));
                        let truthy = builder.ins().fcmp(FloatCC::NotEqual, value, zero);
                        let yes = builder.ins().iconst(types::I32, 1);
                        let no = builder.ins().iconst(types::I32, 0);
                        builder.ins().select(truthy, yes, no)
                    }
                    _ => return None,
                };
                builder.ins().return_(&[result]);
                emitted_return = true;
            }
            _ => return None,
        }
        let terminal = matches!(
            instruction.op(),
            Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse | Op::Return
        ) || (instruction.op() == Op::Binary
            && instruction.result_flags() == RETURN_REGISTER)
            || (instruction.op() == Op::Unary && instruction.result_flags() == RETURN_REGISTER);
        if !terminal {
            builder
                .ins()
                .jump(blocks.get(pc + 1).and_then(|block| *block)?, &[]);
        }
    }
    if !emitted_return {
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
        result: JsJitResultKind::Boolean,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BranchType {
    Number,
    String,
    Boolean,
}

#[derive(Clone)]
struct BranchState {
    registers: Vec<BranchValue>,
    locals: Vec<BranchValue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BranchValue {
    Unknown,
    Number,
    NumberConstant(u32),
    String(u32),
    TypeName(BranchType),
    BooleanLiteral { constant: u32, value: bool },
    BooleanFolded(bool),
    BooleanDynamic,
}

fn branch_value_supported_local(value: BranchValue) -> bool {
    matches!(
        value,
        BranchValue::Number
            | BranchValue::NumberConstant(_)
            | BranchValue::String(_)
            | BranchValue::TypeName(_)
            | BranchValue::BooleanLiteral { .. }
            | BranchValue::BooleanFolded(_)
            | BranchValue::BooleanDynamic
    )
}

fn branch_value_is_number(value: BranchValue) -> bool {
    matches!(value, BranchValue::Number | BranchValue::NumberConstant(_))
}

fn branch_operand(
    program: &ResidualProgram,
    registers: &[BranchValue],
    operand: crate::bytecode::Operand,
) -> Option<BranchValue> {
    match operand.kind()? {
        OperandKind::Register => registers
            .get(usize::from(operand.register_index()?))
            .copied()
            .filter(|value| *value != BranchValue::Unknown),
        OperandKind::Constant => {
            let index = usize::from(operand.payload());
            match program.constants.get(index)? {
                Constant::Number(_) => Some(BranchValue::NumberConstant(index as u32)),
                Constant::String(_) | Constant::StringUnits(_) => {
                    Some(BranchValue::String(u32::try_from(index).ok()?))
                }
                Constant::Boolean(value) => Some(BranchValue::BooleanLiteral {
                    constant: u32::try_from(index).ok()?,
                    value: *value,
                }),
                _ => None,
            }
        }
        OperandKind::Field | OperandKind::Local => None,
    }
}

fn branch_equality(
    program: &ResidualProgram,
    operator: u32,
    left: BranchValue,
    right: BranchValue,
) -> Option<bool> {
    use oxc_ast::ast::BinaryOperator as B;
    let static_string = |value| -> Option<Vec<u16>> {
        match value {
            BranchValue::TypeName(name) => Some(
                match name {
                    BranchType::Number => "number",
                    BranchType::String => "string",
                    BranchType::Boolean => "boolean",
                }
                .encode_utf16()
                .collect(),
            ),
            BranchValue::String(index) => string_constant_units(program, index),
            _ => None,
        }
    };
    if let (Some(left), Some(right)) = (static_string(left), static_string(right)) {
        let ordering = left.cmp(&right);
        return Some(match operator {
            value if value == B::Equality as u32 || value == B::StrictEquality as u32 => {
                ordering.is_eq()
            }
            value if value == B::Inequality as u32 || value == B::StrictInequality as u32 => {
                !ordering.is_eq()
            }
            value if value == B::LessThan as u32 => ordering.is_lt(),
            value if value == B::LessEqualThan as u32 => !ordering.is_gt(),
            value if value == B::GreaterThan as u32 => ordering.is_gt(),
            value if value == B::GreaterEqualThan as u32 => !ordering.is_lt(),
            _ => return None,
        });
    }
    let equal = match (left, right) {
        (
            BranchValue::BooleanLiteral { value: a, .. } | BranchValue::BooleanFolded(a),
            BranchValue::BooleanLiteral { value: b, .. } | BranchValue::BooleanFolded(b),
        ) => a == b,
        _ => return None,
    };
    match operator {
        value if value == B::Equality as u32 || value == B::StrictEquality as u32 => Some(equal),
        value if value == B::Inequality as u32 || value == B::StrictInequality as u32 => {
            Some(!equal)
        }
        _ => None,
    }
}

fn string_constant_units(program: &ResidualProgram, index: u32) -> Option<Vec<u16>> {
    match program.constants.get(index as usize)? {
        Constant::String(value) => Some(value.encode_utf16().collect()),
        Constant::StringUnits(value) => Some(value.clone()),
        _ => None,
    }
}

fn branch_number_comparison(
    builder: &mut FunctionBuilder<'_>,
    operator: u32,
    variables: &[cranelift_frontend::Variable],
    program: &ResidualProgram,
    state: &[BranchValue],
    left: crate::bytecode::Operand,
    right: crate::bytecode::Operand,
) -> Option<ir::Value> {
    let relation = super::operations::NumberRelation::from_operator(operator)?;
    let code = match relation {
        super::operations::NumberRelation::Equal => FloatCC::Equal,
        super::operations::NumberRelation::NotEqual => FloatCC::NotEqual,
        super::operations::NumberRelation::LessThan => FloatCC::LessThan,
        super::operations::NumberRelation::LessEqual => FloatCC::LessThanOrEqual,
        super::operations::NumberRelation::GreaterThan => FloatCC::GreaterThan,
        super::operations::NumberRelation::GreaterEqual => FloatCC::GreaterThanOrEqual,
    };
    let lhs = branch_numeric_operand(builder, variables, program, state, left)?;
    let rhs = branch_numeric_operand(builder, variables, program, state, right)?;
    Some(builder.ins().fcmp(code, lhs, rhs))
}

fn branch_numeric_operand(
    builder: &mut FunctionBuilder<'_>,
    variables: &[cranelift_frontend::Variable],
    program: &ResidualProgram,
    state: &[BranchValue],
    operand: crate::bytecode::Operand,
) -> Option<ir::Value> {
    match operand.kind()? {
        OperandKind::Register => {
            let index = usize::from(operand.register_index()?);
            match state.get(index).copied()? {
                BranchValue::Number => Some(builder.use_var(*variables.get(index)?)),
                BranchValue::NumberConstant(constant) => {
                    let Constant::Number(value) = program.constants.get(constant as usize)? else {
                        return None;
                    };
                    Some(builder.ins().f64const(Ieee64::with_bits(value.to_bits())))
                }
                _ => return None,
            }
        }
        OperandKind::Constant => {
            let Constant::Number(value) = program.constants.get(usize::from(operand.payload()))?
            else {
                return None;
            };
            Some(builder.ins().f64const(Ieee64::with_bits(value.to_bits())))
        }
        OperandKind::Field | OperandKind::Local => None,
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
                let left =
                    operand_value(&mut builder, program, instruction.operand_b(), &registers)?;
                let right =
                    operand_value(&mut builder, program, instruction.operand_c(), &registers)?;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NumericFlowKind {
    Uninitialized,
    Number,
    Boolean,
    Conflict,
}

/// Compile pure numeric loops as Cranelift CFGs. Every backedge polls the
/// VM's cancellation flag; an interrupted function has no guest-visible
/// effects, so it safely resumes through the ordinary interpreter path.
fn compile_numeric_loop(
    program: &ResidualProgram,
    function_id: u32,
    function: &Function,
) -> Option<JsJitArtifact> {
    if !function_candidate(function, &program.atoms)
        || function
            .code
            .iter()
            .any(|instruction| instruction.is_wide())
    {
        return None;
    }
    let code_len = function.code.len();
    let registers = usize::from(function.registers);
    let locals = usize::from(function.locals);
    let state_len = registers.checked_add(locals)?;
    let params = usize::from(function.params);
    let mut states = vec![None::<Vec<NumericFlowKind>>; code_len];
    let mut pending = std::collections::VecDeque::new();
    let mut initial = vec![NumericFlowKind::Uninitialized; state_len];
    for kind in initial.iter_mut().take(params) {
        *kind = NumericFlowKind::Number;
    }
    states[0] = Some(initial);
    pending.push_back(0usize);

    while let Some(pc) = pending.pop_front() {
        let mut state = states.get(pc)?.as_ref()?.clone();
        let instruction = function.code.get(pc)?.as_wide();
        let mut successors = smallvec::SmallVec::<[usize; 2]>::new();
        match instruction.op() {
            Op::LoadConst => {
                if instruction.result_flags() != 0 || instruction.returns_from_frame() {
                    return None;
                }
                if !matches!(
                    program.constants.get(instruction.constant_index())?,
                    Constant::Number(_)
                ) {
                    return None;
                }
                *state.get_mut(usize::from(instruction.result_register()))? =
                    NumericFlowKind::Number;
                successors.push(pc.checked_add(1)?);
            }
            Op::Move => {
                if instruction.result_flags() != 0 || instruction.returns_from_frame() {
                    return None;
                }
                let source = *state.get(usize::from(instruction.register_b()))?;
                if !matches!(source, NumericFlowKind::Number | NumericFlowKind::Boolean) {
                    return None;
                }
                *state.get_mut(usize::from(instruction.result_register()))? = source;
                successors.push(pc.checked_add(1)?);
            }
            Op::LoadLocalPlain => {
                if instruction.result_flags() != 0 || instruction.returns_from_frame() {
                    return None;
                }
                let local = registers.checked_add(instruction.local_slot())?;
                let kind = *state.get(local)?;
                if !matches!(kind, NumericFlowKind::Number | NumericFlowKind::Boolean) {
                    return None;
                }
                *state.get_mut(usize::from(instruction.result_register()))? = kind;
                successors.push(pc.checked_add(1)?);
            }
            Op::StoreLocalPlain => {
                if instruction.result_flags() != 0 || instruction.returns_from_frame() {
                    return None;
                }
                let source = *state.get(usize::from(instruction.register_a()))?;
                if !matches!(source, NumericFlowKind::Number | NumericFlowKind::Boolean) {
                    return None;
                }
                let local = registers.checked_add(instruction.local_slot())?;
                *state.get_mut(local)? = source;
                if let Some(register) = instruction.optional_register_b() {
                    *state.get_mut(usize::from(register))? = source;
                }
                successors.push(pc.checked_add(1)?);
            }
            Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                if (instruction.returns_from_frame()
                    && (instruction.result_flags() != RETURN_REGISTER
                        || instruction.numeric_local_target().is_some()))
                    || (!instruction.returns_from_frame()
                        && instruction.result_flags() != 0
                        && instruction.numeric_local_target().is_none())
                {
                    return None;
                }
                let left =
                    numeric_flow_operand(program, &state, registers, instruction.operand_b())?;
                let right =
                    numeric_flow_operand(program, &state, registers, instruction.operand_c())?;
                let kind = match (instruction.op(), instruction.binary_operator()) {
                    (Op::Binary, operator)
                        if matches!(
                            operator,
                            value if value == BinaryOperator::Addition as u32
                                || value == BinaryOperator::Subtraction as u32
                                || value == BinaryOperator::Multiplication as u32
                                || value == BinaryOperator::Division as u32
                        ) && left == NumericFlowKind::Number
                            && right == NumericFlowKind::Number =>
                    {
                        NumericFlowKind::Number
                    }
                    (Op::Binary, operator)
                        if left == NumericFlowKind::Number
                            && right == NumericFlowKind::Number
                            && matches!(
                                operator,
                                value if value == BinaryOperator::BitwiseOR as u32
                                    || value == BinaryOperator::BitwiseXOR as u32
                                    || value == BinaryOperator::BitwiseAnd as u32
                                    || value == BinaryOperator::ShiftLeft as u32
                                    || value == BinaryOperator::ShiftRight as u32
                                    || value == BinaryOperator::ShiftRightZeroFill as u32
                            ) =>
                    {
                        NumericFlowKind::Number
                    }
                    (Op::NumericAdd, operator)
                        if operator == BinaryOperator::Addition as u32
                            && left == NumericFlowKind::Number
                            && right == NumericFlowKind::Number =>
                    {
                        NumericFlowKind::Number
                    }
                    (Op::NumericMultiply, operator)
                        if operator == BinaryOperator::Multiplication as u32
                            && left == NumericFlowKind::Number
                            && right == NumericFlowKind::Number =>
                    {
                        NumericFlowKind::Number
                    }
                    (Op::Binary, operator)
                        if left == NumericFlowKind::Number
                            && right == NumericFlowKind::Number
                            && super::operations::NumberRelation::from_operator(operator)
                                .is_some() =>
                    {
                        NumericFlowKind::Boolean
                    }
                    _ => return None,
                };
                let destination = match instruction.numeric_local_target() {
                    Some(local) => registers.checked_add(usize::from(local))?,
                    None => usize::from(instruction.result_register()),
                };
                if instruction.returns_from_frame() {
                    if kind != NumericFlowKind::Number
                        || instruction.numeric_local_target().is_some()
                    {
                        return None;
                    }
                } else {
                    *state.get_mut(destination)? = kind;
                    successors.push(pc.checked_add(1)?);
                }
            }
            Op::Jump => {
                let target = usize::try_from(instruction.jump_target()).ok()?;
                successors.push(target);
            }
            Op::JumpFalse | Op::JumpTrue => {
                let condition = *state.get(usize::from(instruction.register_a()))?;
                if !matches!(
                    condition,
                    NumericFlowKind::Number | NumericFlowKind::Boolean
                ) {
                    return None;
                }
                let target = usize::try_from(instruction.jump_target()).ok()?;
                successors.push(pc.checked_add(1)?);
                successors.push(target);
            }
            Op::JumpBinaryFalse => {
                let left =
                    numeric_flow_operand(program, &state, registers, instruction.operand_b())?;
                let right =
                    numeric_flow_operand(program, &state, registers, instruction.operand_c())?;
                if left != NumericFlowKind::Number
                    || right != NumericFlowKind::Number
                    || super::operations::NumberRelation::from_operator(
                        instruction.binary_operator_field(),
                    )
                    .is_none()
                {
                    return None;
                }
                let target = usize::try_from(instruction.jump_target()).ok()?;
                successors.push(pc.checked_add(1)?);
                successors.push(target);
            }
            Op::Return => {
                if instruction.result_flags() != 0
                    || *state.get(usize::from(instruction.register_a()))? != NumericFlowKind::Number
                {
                    return None;
                }
            }
            _ => return None,
        }

        for successor in successors {
            let target = states.get_mut(successor)?;
            match target {
                None => {
                    *target = Some(state.clone());
                    pending.push_back(successor);
                }
                Some(existing) => {
                    let mut changed = false;
                    for (current, incoming) in existing.iter_mut().zip(&state) {
                        // A value written only on a prior loop iteration is not
                        // definitely initialized at the loop header. Keep the
                        // conflict marker and reject only if bytecode reads it;
                        // dead temporaries can still flow through the join.
                        let joined = if *current == *incoming {
                            *current
                        } else {
                            NumericFlowKind::Conflict
                        };
                        if joined != *current {
                            *current = joined;
                            changed = true;
                        }
                    }
                    if changed {
                        pending.push_back(successor);
                    }
                }
            }
        }
    }
    if !function
        .code
        .iter()
        .any(|instruction| !instruction.is_wide() && instruction.as_wide().op() == Op::Return)
    {
        return None;
    }

    let mut flags = settings::builder();
    flags.set("opt_level", "speed").ok()?;
    let isa_builder = cranelift_native::builder().ok()?;
    let isa = isa_builder.finish(settings::Flags::new(flags)).ok()?;
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(AbiParam::new(isa.pointer_type()));
    signature.params.push(AbiParam::new(isa.pointer_type()));
    signature.params.push(AbiParam::new(isa.pointer_type()));
    signature.returns.push(AbiParam::new(types::F64));
    let name = UserFuncName::user(0x5155_454e, function_id);
    let mut native = ir::Function::with_name_signature(name, signature);
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let arguments = builder.block_params(entry)[0];
    let interrupt = builder.block_params(entry)[1];
    let status = builder.block_params(entry)[2];
    let numeric_variables = (0..state_len)
        .map(|_| {
            let variable = builder.declare_var(types::F64);
            Some(variable)
        })
        .collect::<Option<Vec<_>>>()?;
    let boolean_variables = (0..state_len)
        .map(|_| {
            let variable = builder.declare_var(types::I8);
            Some(variable)
        })
        .collect::<Option<Vec<_>>>()?;
    let backedge_counter = builder.declare_var(types::I32);
    let counter_zero = builder.ins().iconst(types::I32, 0);
    builder.def_var(backedge_counter, counter_zero);
    let blocks = (0..code_len)
        .map(|_| builder.create_block())
        .collect::<Vec<_>>();
    let interrupt_exit = builder.create_block();
    let interrupt_status = builder.append_block_param(interrupt_exit, isa.pointer_type());
    for index in 0..params {
        let offset = i32::try_from(index.checked_mul(std::mem::size_of::<f64>())?).ok()?;
        let value = builder
            .ins()
            .load(types::F64, ir::MemFlagsData::trusted(), arguments, offset);
        builder.def_var(numeric_variables[index], value);
    }
    builder.ins().jump(*blocks.first()?, &[]);

    for pc in 0..code_len {
        let Some(state) = states[pc].as_ref() else {
            continue;
        };
        builder.switch_to_block(blocks[pc]);
        let instruction = function.code[pc].as_wide();
        match instruction.op() {
            Op::LoadConst => {
                let Constant::Number(value) =
                    program.constants.get(instruction.constant_index())?
                else {
                    return None;
                };
                let value = builder.ins().f64const(Ieee64::with_bits(value.to_bits()));
                builder.def_var(
                    numeric_variables[usize::from(instruction.result_register())],
                    value,
                );
                builder.ins().jump(*blocks.get(pc.checked_add(1)?)?, &[]);
            }
            Op::Move => {
                let source = usize::from(instruction.register_b());
                let destination = usize::from(instruction.result_register());
                match state[source] {
                    NumericFlowKind::Number => {
                        let value = builder.use_var(numeric_variables[source]);
                        builder.def_var(numeric_variables[destination], value);
                    }
                    NumericFlowKind::Boolean => {
                        let value = builder.use_var(boolean_variables[source]);
                        builder.def_var(boolean_variables[destination], value);
                    }
                    _ => return None,
                }
                builder.ins().jump(*blocks.get(pc.checked_add(1)?)?, &[]);
            }
            Op::LoadLocalPlain => {
                let local = registers.checked_add(instruction.local_slot())?;
                let destination = usize::from(instruction.result_register());
                match state[local] {
                    NumericFlowKind::Number => {
                        let value = builder.use_var(numeric_variables[local]);
                        builder.def_var(numeric_variables[destination], value);
                    }
                    NumericFlowKind::Boolean => {
                        let value = builder.use_var(boolean_variables[local]);
                        builder.def_var(boolean_variables[destination], value);
                    }
                    _ => return None,
                }
                builder.ins().jump(*blocks.get(pc.checked_add(1)?)?, &[]);
            }
            Op::StoreLocalPlain => {
                let source = usize::from(instruction.register_a());
                let local = registers.checked_add(instruction.local_slot())?;
                match state[source] {
                    NumericFlowKind::Number => {
                        let value = builder.use_var(numeric_variables[source]);
                        builder.def_var(numeric_variables[local], value);
                        if let Some(register) = instruction.optional_register_b() {
                            builder.def_var(numeric_variables[usize::from(register)], value);
                        }
                    }
                    NumericFlowKind::Boolean => {
                        let value = builder.use_var(boolean_variables[source]);
                        builder.def_var(boolean_variables[local], value);
                        if let Some(register) = instruction.optional_register_b() {
                            builder.def_var(boolean_variables[usize::from(register)], value);
                        }
                    }
                    _ => return None,
                }
                builder.ins().jump(*blocks.get(pc.checked_add(1)?)?, &[]);
            }
            Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                let left = numeric_loop_operand(
                    &mut builder,
                    program,
                    &state,
                    &numeric_variables,
                    registers,
                    instruction.operand_b(),
                )?;
                let right = numeric_loop_operand(
                    &mut builder,
                    program,
                    &state,
                    &numeric_variables,
                    registers,
                    instruction.operand_c(),
                )?;
                let operator = instruction.binary_operator();
                let value = match (instruction.op(), operator) {
                    (Op::Binary, value) if value == BinaryOperator::Addition as u32 => {
                        let result = builder.ins().fadd(left, right);
                        Some((NumericFlowKind::Number, result))
                    }
                    (Op::Binary, value) if value == BinaryOperator::Subtraction as u32 => {
                        let result = builder.ins().fsub(left, right);
                        Some((NumericFlowKind::Number, result))
                    }
                    (Op::Binary, value) if value == BinaryOperator::Multiplication as u32 => {
                        let result = builder.ins().fmul(left, right);
                        Some((NumericFlowKind::Number, result))
                    }
                    (Op::Binary, value) if value == BinaryOperator::Division as u32 => {
                        let result = builder.ins().fdiv(left, right);
                        Some((NumericFlowKind::Number, result))
                    }
                    (Op::Binary, value)
                        if value == BinaryOperator::BitwiseOR as u32
                            || value == BinaryOperator::BitwiseXOR as u32
                            || value == BinaryOperator::BitwiseAnd as u32
                            || value == BinaryOperator::ShiftLeft as u32
                            || value == BinaryOperator::ShiftRight as u32
                            || value == BinaryOperator::ShiftRightZeroFill as u32 =>
                    {
                        let left = numeric_to_i32(&mut builder, left);
                        let right = numeric_to_i32(&mut builder, right);
                        let shift = builder.ins().band_imm_u(right, 31);
                        let integer = match value {
                            value if value == BinaryOperator::BitwiseOR as u32 => {
                                builder.ins().bor(left, right)
                            }
                            value if value == BinaryOperator::BitwiseXOR as u32 => {
                                builder.ins().bxor(left, right)
                            }
                            value if value == BinaryOperator::BitwiseAnd as u32 => {
                                builder.ins().band(left, right)
                            }
                            value if value == BinaryOperator::ShiftLeft as u32 => {
                                builder.ins().ishl(left, shift)
                            }
                            value if value == BinaryOperator::ShiftRight as u32 => {
                                builder.ins().sshr(left, shift)
                            }
                            _ => builder.ins().ushr(left, shift),
                        };
                        let result = if value == BinaryOperator::ShiftRightZeroFill as u32 {
                            let unsigned = builder.ins().uextend(types::I64, integer);
                            builder.ins().fcvt_from_uint(types::F64, unsigned)
                        } else {
                            builder.ins().fcvt_from_sint(types::F64, integer)
                        };
                        Some((NumericFlowKind::Number, result))
                    }
                    (Op::NumericAdd, value) if value == BinaryOperator::Addition as u32 => {
                        let result = builder.ins().fadd(left, right);
                        Some((NumericFlowKind::Number, result))
                    }
                    (Op::NumericMultiply, value)
                        if value == BinaryOperator::Multiplication as u32 =>
                    {
                        let result = builder.ins().fmul(left, right);
                        Some((NumericFlowKind::Number, result))
                    }
                    (Op::Binary, value)
                        if super::operations::NumberRelation::from_operator(value).is_some() =>
                    {
                        let condition =
                            builder.ins().fcmp(numeric_relation_cc(value)?, left, right);
                        let one = builder.ins().iconst(types::I8, 1);
                        let zero = builder.ins().iconst(types::I8, 0);
                        let result = builder.ins().select(condition, one, zero);
                        Some((NumericFlowKind::Boolean, result))
                    }
                    _ => return None,
                };
                let (kind, value) = value?;
                if instruction.returns_from_frame() {
                    if kind != NumericFlowKind::Number {
                        return None;
                    }
                    let complete = builder.ins().iconst(types::I32, 1);
                    builder
                        .ins()
                        .store(ir::MemFlagsData::trusted(), complete, status, 0);
                    builder.ins().return_(&[value]);
                    continue;
                }
                let destination = match instruction.numeric_local_target() {
                    Some(local) => registers.checked_add(usize::from(local))?,
                    None => usize::from(instruction.result_register()),
                };
                match kind {
                    NumericFlowKind::Number => {
                        builder.def_var(numeric_variables[destination], value)
                    }
                    NumericFlowKind::Boolean => {
                        builder.def_var(boolean_variables[destination], value)
                    }
                    _ => return None,
                }
                builder.ins().jump(*blocks.get(pc.checked_add(1)?)?, &[]);
            }
            Op::Jump => {
                let target = usize::try_from(instruction.jump_target()).ok()?;
                emit_numeric_loop_backedge_poll(
                    &mut builder,
                    pc,
                    target,
                    interrupt,
                    status,
                    interrupt_exit,
                    backedge_counter,
                    &blocks,
                )?;
            }
            Op::JumpFalse | Op::JumpTrue => {
                let register = usize::from(instruction.register_a());
                let truthy = match state[register] {
                    NumericFlowKind::Boolean => {
                        let condition = builder.use_var(boolean_variables[register]);
                        builder.ins().icmp_imm_u(
                            cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                            condition,
                            0,
                        )
                    }
                    NumericFlowKind::Number => {
                        let value = builder.use_var(numeric_variables[register]);
                        let zero = builder.ins().f64const(Ieee64::with_bits(0.0f64.to_bits()));
                        let less = builder.ins().fcmp(FloatCC::LessThan, value, zero);
                        let greater = builder.ins().fcmp(FloatCC::GreaterThan, value, zero);
                        builder.ins().bor(less, greater)
                    }
                    _ => return None,
                };
                let target = usize::try_from(instruction.jump_target()).ok()?;
                let (then_pc, else_pc) = if instruction.op() == Op::JumpFalse {
                    (pc.checked_add(1)?, target)
                } else {
                    (target, pc.checked_add(1)?)
                };
                emit_numeric_loop_conditional(
                    &mut builder,
                    pc,
                    then_pc,
                    else_pc,
                    truthy,
                    interrupt,
                    status,
                    interrupt_exit,
                    backedge_counter,
                    &blocks,
                )?;
            }
            Op::JumpBinaryFalse => {
                let left = numeric_loop_operand(
                    &mut builder,
                    program,
                    state,
                    &numeric_variables,
                    registers,
                    instruction.operand_b(),
                )?;
                let right = numeric_loop_operand(
                    &mut builder,
                    program,
                    state,
                    &numeric_variables,
                    registers,
                    instruction.operand_c(),
                )?;
                let condition = builder.ins().fcmp(
                    numeric_relation_cc(instruction.binary_operator_field())?,
                    left,
                    right,
                );
                let target = usize::try_from(instruction.jump_target()).ok()?;
                emit_numeric_loop_conditional(
                    &mut builder,
                    pc,
                    pc.checked_add(1)?,
                    target,
                    condition,
                    interrupt,
                    status,
                    interrupt_exit,
                    backedge_counter,
                    &blocks,
                )?;
            }
            Op::Return => {
                let result =
                    builder.use_var(numeric_variables[usize::from(instruction.register_a())]);
                let complete = builder.ins().iconst(types::I32, 1);
                builder
                    .ins()
                    .store(ir::MemFlagsData::trusted(), complete, status, 0);
                builder.ins().return_(&[result]);
            }
            _ => return None,
        }
    }

    builder.switch_to_block(interrupt_exit);
    let interrupted = builder.ins().iconst(types::I32, 0);
    builder.ins().store(
        ir::MemFlagsData::trusted(),
        interrupted,
        interrupt_status,
        0,
    );
    let no_result = builder.ins().f64const(Ieee64::with_bits(0.0f64.to_bits()));
    builder.ins().return_(&[no_result]);
    builder.seal_all_blocks();
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
        result: JsJitResultKind::NumberInterruptible,
    })
}

fn numeric_flow_operand(
    program: &ResidualProgram,
    state: &[NumericFlowKind],
    registers: usize,
    operand: crate::bytecode::Operand,
) -> Option<NumericFlowKind> {
    match operand.kind()? {
        OperandKind::Register => {
            let kind = *state.get(usize::from(operand.register_index()?))?;
            matches!(kind, NumericFlowKind::Number | NumericFlowKind::Boolean).then_some(kind)
        }
        OperandKind::Constant => matches!(
            program.constants.get(usize::from(operand.payload()))?,
            Constant::Number(_)
        )
        .then_some(NumericFlowKind::Number),
        OperandKind::Local => {
            let local = usize::try_from(operand.payload()).ok()?;
            let kind = *state.get(registers.checked_add(local)?)?;
            matches!(kind, NumericFlowKind::Number | NumericFlowKind::Boolean).then_some(kind)
        }
        OperandKind::Field => None,
    }
}

fn numeric_loop_operand(
    builder: &mut FunctionBuilder<'_>,
    program: &ResidualProgram,
    state: &[NumericFlowKind],
    variables: &[cranelift_frontend::Variable],
    registers: usize,
    operand: crate::bytecode::Operand,
) -> Option<ir::Value> {
    match operand.kind()? {
        OperandKind::Register => {
            let register = usize::from(operand.register_index()?);
            if state.get(register).copied()? != NumericFlowKind::Number {
                return None;
            }
            Some(builder.use_var(*variables.get(register)?))
        }
        OperandKind::Constant => {
            let Constant::Number(value) = program.constants.get(usize::from(operand.payload()))?
            else {
                return None;
            };
            Some(builder.ins().f64const(Ieee64::with_bits(value.to_bits())))
        }
        OperandKind::Local => {
            let local = registers.checked_add(usize::try_from(operand.payload()).ok()?)?;
            if state.get(local).copied()? != NumericFlowKind::Number {
                return None;
            }
            Some(builder.use_var(*variables.get(local)?))
        }
        OperandKind::Field => None,
    }
}

/// Apply ECMAScript ToUint32 modulo conversion to an already numeric value.
/// Reducing before the saturating conversion preserves wraparound for large
/// finite values and maps NaN and infinities to zero.
fn numeric_to_i32(builder: &mut FunctionBuilder<'_>, value: ir::Value) -> ir::Value {
    let modulus = builder
        .ins()
        .f64const(Ieee64::with_bits(4_294_967_296.0f64.to_bits()));
    let quotient = builder.ins().fdiv(value, modulus);
    let quotient = builder.ins().trunc(quotient);
    let product = builder.ins().fmul(quotient, modulus);
    let reduced = builder.ins().fsub(value, product);
    let truncated = builder.ins().fcvt_to_sint_sat(types::I64, reduced);
    builder.ins().ireduce(types::I32, truncated)
}

fn numeric_relation_cc(operator: u32) -> Option<FloatCC> {
    Some(
        match super::operations::NumberRelation::from_operator(operator)? {
            super::operations::NumberRelation::Equal => FloatCC::Equal,
            super::operations::NumberRelation::NotEqual => FloatCC::NotEqual,
            super::operations::NumberRelation::LessThan => FloatCC::LessThan,
            super::operations::NumberRelation::LessEqual => FloatCC::LessThanOrEqual,
            super::operations::NumberRelation::GreaterThan => FloatCC::GreaterThan,
            super::operations::NumberRelation::GreaterEqual => FloatCC::GreaterThanOrEqual,
        },
    )
}

fn emit_numeric_loop_backedge_poll(
    builder: &mut FunctionBuilder<'_>,
    pc: usize,
    target: usize,
    interrupt: ir::Value,
    status: ir::Value,
    interrupt_exit: ir::Block,
    backedge_counter: cranelift_frontend::Variable,
    blocks: &[ir::Block],
) -> Option<()> {
    if target <= pc {
        emit_numeric_loop_interrupt_gate(
            builder,
            interrupt,
            status,
            interrupt_exit,
            backedge_counter,
        );
    }
    builder.ins().jump(*blocks.get(target)?, &[]);
    Some(())
}

fn emit_numeric_loop_conditional(
    builder: &mut FunctionBuilder<'_>,
    pc: usize,
    then_pc: usize,
    else_pc: usize,
    condition: ir::Value,
    interrupt: ir::Value,
    status: ir::Value,
    interrupt_exit: ir::Block,
    backedge_counter: cranelift_frontend::Variable,
    blocks: &[ir::Block],
) -> Option<()> {
    if then_pc <= pc || else_pc <= pc {
        emit_numeric_loop_interrupt_gate(
            builder,
            interrupt,
            status,
            interrupt_exit,
            backedge_counter,
        );
    }
    builder.ins().brif(
        condition,
        *blocks.get(then_pc)?,
        &[],
        *blocks.get(else_pc)?,
        &[],
    );
    Some(())
}

/// Count backedges cheaply and load the shared cancellation flag once every
/// 1024 iterations, keeping interrupt latency bounded without an atomic load
/// on every numeric loop iteration.
fn emit_numeric_loop_interrupt_gate(
    builder: &mut FunctionBuilder<'_>,
    interrupt: ir::Value,
    status: ir::Value,
    interrupt_exit: ir::Block,
    backedge_counter: cranelift_frontend::Variable,
) {
    let count = builder.use_var(backedge_counter);
    let count = builder.ins().iadd_imm_s(count, 1);
    builder.def_var(backedge_counter, count);
    let interval = builder.ins().band_imm_u(count, 1023);
    let check =
        builder
            .ins()
            .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::Equal, interval, 0);
    let poll = builder.create_block();
    let skip = builder.create_block();
    builder.ins().brif(check, poll, &[], skip, &[]);
    builder.switch_to_block(poll);
    let requested = builder
        .ins()
        .atomic_load(types::I8, ir::MemFlagsData::trusted(), interrupt);
    let cancelled = builder.ins().icmp_imm_u(
        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
        requested,
        0,
    );
    let continue_loop = builder.create_block();
    builder.ins().brif(
        cancelled,
        interrupt_exit,
        &[ir::BlockArg::Value(status)],
        continue_loop,
        &[],
    );
    builder.switch_to_block(continue_loop);
    builder.ins().jump(skip, &[]);
    builder.switch_to_block(skip);
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

fn merge_branch_state(target: &mut BranchState, incoming: &BranchState) {
    for (target, incoming) in target.registers.iter_mut().zip(&incoming.registers) {
        *target = merge_branch_value(*target, *incoming);
    }
    for (target, incoming) in target.locals.iter_mut().zip(&incoming.locals) {
        *target = merge_branch_value(*target, *incoming);
    }
}

fn merge_branch_value(left: BranchValue, right: BranchValue) -> BranchValue {
    if left == right {
        return left;
    }
    if branch_value_is_number(left) && branch_value_is_number(right) {
        return BranchValue::Number;
    }
    if matches!(
        left,
        BranchValue::BooleanLiteral { .. }
            | BranchValue::BooleanFolded(_)
            | BranchValue::BooleanDynamic
    ) && matches!(
        right,
        BranchValue::BooleanLiteral { .. }
            | BranchValue::BooleanFolded(_)
            | BranchValue::BooleanDynamic
    ) {
        return BranchValue::BooleanDynamic;
    }
    BranchValue::Unknown
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
    let relation = super::operations::NumberRelation::from_operator(instruction_binary_operator(
        instruction,
    )?)?;
    let code = match relation {
        super::operations::NumberRelation::LessThan => FloatCC::LessThan,
        super::operations::NumberRelation::LessEqual => FloatCC::LessThanOrEqual,
        super::operations::NumberRelation::GreaterThan => FloatCC::GreaterThan,
        super::operations::NumberRelation::GreaterEqual => FloatCC::GreaterThanOrEqual,
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
        && (function.wide.is_empty() || wide_boolean_tree_candidate(function))
        && usize::from(function.registers) <= MAX_REGISTERS
        && usize::from(function.params) <= MAX_PARAMETERS
        && eligible(function, atoms)
}

fn wide_boolean_tree_candidate(function: &Function) -> bool {
    let mut has_forward_branch = false;
    function.code.iter().enumerate().all(|(pc, instruction)| {
        let op = instruction.as_wide().op();
        has_forward_branch |= matches!(op, Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse);
        let supported = matches!(
            op,
            Op::LoadConst
                | Op::Move
                | Op::Unary
                | Op::StoreLocal
                | Op::StoreLocalPlain
                | Op::LoadLocal
                | Op::LoadLocalPlain
                | Op::Binary
                | Op::Jump
                | Op::JumpFalse
                | Op::JumpTrue
                | Op::JumpBinaryFalse
                | Op::Return
        );
        supported
            && (!matches!(
                op,
                Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse
            ) || usize::try_from(instruction.as_wide().jump_target())
                .is_ok_and(|target| target > pc))
    }) && has_forward_branch
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
    builder: &mut FunctionBuilder<'_>,
    program: &ResidualProgram,
    operand: crate::bytecode::Operand,
    registers: &[Option<ir::Value>],
) -> Option<ir::Value> {
    match operand.kind()? {
        OperandKind::Register => registers
            .get(usize::from(operand.register_index()?))?
            .as_ref()
            .copied(),
        OperandKind::Constant => {
            let Constant::Number(value) = program.constants.get(usize::from(operand.payload()))?
            else {
                return None;
            };
            Some(builder.ins().f64const(Ieee64::with_bits(value.to_bits())))
        }
        OperandKind::Field | OperandKind::Local => None,
    }
}
