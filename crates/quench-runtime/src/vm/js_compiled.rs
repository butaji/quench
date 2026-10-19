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
    if function.code.iter().any(|packed| {
        !packed.is_wide()
            && matches!(
                packed.as_wide().op(),
                Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse
            )
    }) {
        compile_number_domain_boolean_tree(program, function_id, function)
            .or_else(|| compile_string_branches(program, function_id, function))
    } else {
        compile_numeric(program, function_id)
    }
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
    if !function_candidate(function, program)
        || usize::from(function.registers) > MAX_REGISTERS
        || function
            .code
            .iter()
            .any(|instruction| instruction.is_wide())
    {
        return None;
    }

    let code_len = function.code.len();
    let mut states = vec![None::<BranchState>; code_len];
    let mut predecessors = vec![0_u8; code_len];
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
                    Constant::Number(_) => BranchValue::Number,
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
                if instruction.result_flags() != 0
                    || instruction.unary_operator() != oxc_ast::ast::UnaryOperator::Typeof as u32
                {
                    return None;
                }
                let source = *state.registers.get(usize::from(instruction.register_b()))?;
                let type_name = match source {
                    BranchValue::Number => BranchType::Number,
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
                successors.push(pc + 1);
            }
            Op::StoreLocal => {
                let slot = usize::from(instruction.local_slot());
                if !hidden_local(program, function, slot) {
                    return None;
                }
                let value = *state.registers.get(usize::from(instruction.register_a()))?;
                if !branch_value_is_static(value) {
                    return None;
                }
                *state.locals.get_mut(slot)? = value;
                if let Some(register) = instruction.optional_register_b() {
                    *state.registers.get_mut(usize::from(register))? = value;
                }
                successors.push(pc + 1);
            }
            Op::LoadLocal => {
                let slot = usize::from(instruction.local_slot());
                if !hidden_local(program, function, slot) {
                    return None;
                }
                let value = *state.locals.get(slot)?;
                if !branch_value_is_static(value) {
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
                    (BranchValue::Number, BranchValue::Number)
                        if super::operations::NumberRelation::from_operator(
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
                } else if matches!((left, right), (BranchValue::Number, BranchValue::Number))
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
            let predecessor_count = predecessors.get_mut(target)?;
            *predecessor_count = predecessor_count.checked_add(1)?;
            if *predecessor_count > 1 || states[target].is_some() {
                return None;
            }
            states[target] = Some(state.clone());
        }
    }

    if !reachable.iter().enumerate().any(|(pc, is_reachable)| {
        *is_reachable
            && (function.code[pc].as_wide().op() == Op::Return
                || (function.code[pc].as_wide().op() == Op::Binary
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
            Op::StoreLocal | Op::LoadLocal => {}
            Op::Move => {
                let source = usize::from(instruction.register_b());
                if matches!(
                    state.registers.get(source),
                    Some(BranchValue::Number | BranchValue::BooleanDynamic)
                ) {
                    let value = builder.use_var(*variables.get(source)?);
                    builder.def_var(
                        *variables.get(usize::from(instruction.result_register()))?,
                        value,
                    );
                }
            }
            Op::Unary => {}
            Op::Binary => {
                let left = branch_operand(program, &state.registers, instruction.operand_b())?;
                let right = branch_operand(program, &state.registers, instruction.operand_c())?;
                if matches!((left, right), (BranchValue::Number, BranchValue::Number)) {
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
            && instruction.result_flags() == RETURN_REGISTER);
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
    String(u32),
    TypeName(BranchType),
    BooleanLiteral { constant: u32, value: bool },
    BooleanFolded(bool),
    BooleanDynamic,
}

fn hidden_local(program: &ResidualProgram, function: &Function, slot: usize) -> bool {
    function
        .local_atoms
        .get(slot)
        .is_some_and(|atom| program.atoms[*atom as usize].starts_with("\0quench:"))
}

fn branch_value_is_static(value: BranchValue) -> bool {
    matches!(
        value,
        BranchValue::String(_)
            | BranchValue::TypeName(_)
            | BranchValue::BooleanLiteral { .. }
            | BranchValue::BooleanFolded(_)
    )
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
                Constant::Number(_) => Some(BranchValue::Number),
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
            if state.get(index).copied()? != BranchValue::Number {
                return None;
            }
            Some(builder.use_var(*variables.get(index)?))
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
    if !function_candidate(function, program)
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

/// Compile a forward-only decision tree whose conditions compare Numbers and
/// whose leaves return static string constants. The native result is a Return
/// PC, never a heap pointer; the VM materializes the already-rooted constant.
fn compile_string_branches(
    program: &ResidualProgram,
    function_id: u32,
    function: &Function,
) -> Option<JsJitArtifact> {
    if !function_candidate(function, program)
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
    let relation = super::operations::NumberRelation::from_operator(instruction.binary_operator())?;
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
pub(super) fn function_candidate(function: &Function, program: &crate::bytecode::ResidualProgram) -> bool {
    !function.code.is_empty()
        && function.code.len() <= MAX_INSTRUCTIONS
        && function.wide.is_empty()
        && usize::from(function.registers) <= MAX_REGISTERS
        && usize::from(function.params) <= MAX_PARAMETERS
        && eligible(function, program)
}

fn eligible(function: &Function, program: &crate::bytecode::ResidualProgram) -> bool {
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
        || !function.plain_activation(program)
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
