//! Cranelift code generation for a deliberately small, guarded JavaScript subset.
//!
//! The entry ABI is `extern "C" fn(*const f64) -> f64`; the pointer addresses
//! exactly `JsJitArtifact::params` Number arguments. The caller must guard each
//! argument as a JavaScript Number before entering this code and convert the
//! returned `f64` with `Value::number`. A miss stays in the interpreter before
//! this function has performed any guest-visible operation.

use crate::bytecode::{
    Constant, Function, Op, OperandKind, ProgramKind, RETURN_REGISTER, ResidualProgram,
};
use cranelift_codegen::ir::{self, AbiParam, InstBuilder, UserFuncName, immediates::Ieee64, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use oxc_ast::ast::BinaryOperator;

/// Native body for a straight-line Number-only JavaScript function.
pub(super) struct JsJitArtifact {
    /// Machine code for `extern "C" fn(*const f64) -> f64`.
    pub(super) code: Vec<u8>,
    /// The number of formal parameters read from the argument array.
    pub(super) params: usize,
}

// Keep straight-line native regions short so an interpreter interrupt cannot
// be delayed by a large, uninterruptible compiled body.
const MAX_INSTRUCTIONS: usize = 256;
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
    Some(JsJitArtifact { code, params })
}

/// Cheap activation-level filter used before retaining hotness state. The
/// bytecode scan remains deferred until the function actually crosses the
/// statistical promotion gate.
pub(super) fn function_candidate(function: &Function, atoms: &crate::bytecode::AtomTable) -> bool {
    !function.code.is_empty()
        && function.code.len() <= MAX_INSTRUCTIONS
        && function.wide.is_empty()
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
        || function.is_arrow
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
