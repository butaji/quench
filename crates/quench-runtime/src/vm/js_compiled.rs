//! Cranelift code generation for a deliberately small, guarded JavaScript subset.
//!
//! Native entry arguments are guarded Number values. Numeric bytecode regions
//! use the VM's common native-entry ABI; other artifacts retain their current
//! bounded result ABIs, including static-string bytecode Return PCs.

use crate::bytecode::{
    Constant, FieldBase, FieldLookup, Function, Op, Operand, OperandKind, ProgramKind,
    RETURN_REGISTER, ResidualProgram, WideInstruction,
};
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, UserFuncName, condcodes::FloatCC, immediates::Ieee64, types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use oxc_ast::ast::BinaryOperator;

/// Native code and its result interpretation for a guarded JavaScript function.
pub(super) struct JsJitArtifact {
    /// Machine code using the ABI selected by `result`.
    pub(super) code: Vec<u8>,
    /// The number of formal parameters read from the argument array.
    pub(super) params: usize,
    /// Entry guards require every formal argument to be a signed Int32.
    /// Direct compiled calls must apply this same predicate before using the
    /// artifact's checked conversion instructions.
    pub(super) requires_signed_int32_arguments: bool,
    /// Selects the native result interpretation; strings name their Return PC.
    pub(super) result: JsJitResultKind,
    /// A cached data property on parameter zero supplies the native numeric input.
    pub(super) field_argument: Option<JsJitFieldArgument>,
    /// Cached data properties read from `this` or parameter zero in numeric regions.
    pub(super) numeric_field_inputs: Vec<JsJitNumericFieldInput>,
    /// Numeric lexical captures are appended after formal parameters in the ABI.
    pub(super) capture_arguments: Vec<JsJitCaptureArgument>,
    /// Directly emitted bytecode PCs for the shared general JS dispatcher.
    pub(super) general_dispatch_native_pcs: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(super) struct JsJitFieldArgument {
    pub(super) cache_site: u16,
    pub(super) entry_pc: usize,
}

#[derive(Clone, Copy)]
pub(super) struct JsJitNumericFieldInput {
    pub(super) cache_site: u16,
    pub(super) pc: usize,
    pub(super) receiver: JsJitFieldReceiver,
    /// The earlier identical data-property read to reuse, or this input's index.
    pub(super) source: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum JsJitFieldReceiver {
    This,
    Parameter(u16),
    Capture {
        register: u16,
        capture: JsJitCaptureArgument,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct JsJitCaptureArgument {
    pub(super) depth: u16,
    pub(super) slot: u16,
}

pub(super) enum JsJitResultKind {
    /// Numeric bytecode regions emit the common VM native-entry ABI.
    SharedNumberEntry(super::compiled::NativeEntryMode, super::compiled::NativeNumberResult),
    /// A full function CFG whose operation blocks call the shared VM step
    /// helper and branch directly to the next validated bytecode PC.
    GeneralDispatch,
    /// An Int32-guarded return path proven by the bytecode's `typeof` switch.
    Int32Identity(u32),
    CaptureNumber(JsJitCaptureArgument),
    NumberInterruptible,
    StringReturnPc(Vec<(u32, u32)>),
    Boolean,
    /// Return to the interpreter at a proven early-return continuation unless
    /// the guarded field comparison selects the common native path.
    FieldReject {
        resume_pc: u32,
        resume_register: u16,
        resume_value: JsJitResumeValue,
    },
}

#[derive(Clone, Copy)]
pub(super) enum JsJitResumeValue {
    Number(f64),
    Boolean(bool),
}

// Keep straight-line native regions short so an interpreter interrupt cannot
// be delayed by a large, uninterruptible compiled body.
const MAX_INSTRUCTIONS: usize = 256;
/// Where every function's execution starts.
const ENTRY_PC: usize = 0;
const MAX_REGISTERS: usize = 256;
pub(crate) const MAX_CODE_BYTES: usize = 64 * 1024;
const MAX_PARAMETERS: usize = 8;
pub(super) const MAX_CAPTURE_ARGUMENTS: usize = 4;
pub(super) const MAX_NUMERIC_FIELD_INPUTS: usize = 4;

/// Compile `function_id` when all reachable behavior is represented by the
/// supported straight-line bytecode subset. Every unsupported shape is left
/// to the interpreter.
pub(super) fn compile(program: &ResidualProgram, function_id: u32) -> Option<JsJitArtifact> {
    if program.kind == ProgramKind::Wasm || function_id == 0 {
        return None;
    }
    let function = program.function_get(function_id as usize)?;
    if let Some(return_pc) = int32_identity_return_candidate(program, function) {
        return compile_int32_identity(function_id, return_pc);
    }
    if let Some(capture) = number_capture_candidate(function) {
        return compile_capture_number(function_id, capture);
    }
    if let Some(captures) = numeric_capture_candidate(function)
        && let Some(artifact) = compile_numeric_with_captures(program, function_id, &captures)
    {
        return Some(artifact);
    }
    if let Some(field) = cached_field_argument_candidate(function)
        && let Some(artifact) = compile_field_reject(program, function_id, function, field)
    {
        return Some(artifact);
    }
    if function.code.iter().any(|packed| {
        !packed.is_wide()
            && packed.as_wide().op().is_js_jump()
            || (packed.as_wide().op() == Op::Unary
                && packed.as_wide().unary_operator()
                    == oxc_ast::ast::UnaryOperator::LogicalNot as u32)
            // A boolean literal, as compile-time folding leaves `!0`, needs the boolean tree.
            || (packed.as_wide().op() == Op::LoadConst
                && matches!(
                    program.constants.get(packed.as_wide().constant_index()),
                    Some(Constant::Boolean(_))
                ))
    }) {
        compile_numeric(program, function_id)
            .or_else(|| compile_numeric_loop(program, function_id, function))
            .or_else(|| compile_number_domain_boolean_tree(program, function_id, function))
            .or_else(|| compile_string_branches(program, function_id, function))
    } else {
        compile_numeric(program, function_id)
    }
}

/// Recognize a common integer fast arm without depending on source names or
/// benchmark layout: `typeof arg === "number"` dispatches to
/// `(arg | 0) == arg ? arg : ...`. The VM additionally guards the input as an
/// Int32 before taking the native return. Instructions before the number arm
/// must be side-effect free, and the number arm must return the original arg.
pub(super) fn int32_identity_return_candidate(
    program: &ResidualProgram,
    function: &Function,
) -> Option<u32> {
    if function.params != 1
        || function.code.len() > MAX_INSTRUCTIONS
        || function.registers as usize > MAX_REGISTERS
        || !eligible(function, program)
    {
        return None;
    }
    // The integer arm: `(param | 0) == param` returning the parameter, found anywhere and
    // proven to be what a Number argument reaches (`number_type_dispatch_reaches`), however
    // the `typeof` dispatch before it was compiled.
    let code = &function.code;
    for target in 0..code.len() {
        let fast = code[target].as_wide();
        if fast.op() != Op::Binary
            || fast.binary_operator() != BinaryOperator::BitwiseOR as u32
            || !operand_is_parameter(fast.operand_b(), 0)
            || !operand_is_number(program, fast.operand_c(), 0.0)
        {
            continue;
        }
        let (Some(check), Some(ret)) = (code.get(target + 1), code.get(target + 2)) else {
            continue;
        };
        let (check, ret) = (check.as_wide(), ret.as_wide());
        if check.op() != Op::JumpBinaryFalse
            || check.binary_operator_field() != BinaryOperator::Equality as u32
            || !operands_are(check.operand_b(), check.operand_c(), fast.result_register(), 0)
            || ret.op() != Op::Return
            || !is_parameter_register(ret.register_a(), 0)
            || usize::try_from(check.jump_target()).ok() != Some(target + 3)
        {
            continue;
        }
        if number_type_dispatch_reaches(program, function, target) == Some(true) {
            return u32::try_from(target + 2).ok();
        }
    }
    None
}

#[derive(Clone)]
enum NumberDispatchValue {
    TypeNumber,
    String(String),
    /// A Number: a constant, or the index of `"number"` a `UNARY_TYPEOF_INDEX` of the
    /// argument yields (`switch (typeof arg)` over string cases).
    Number(f64),
    Boolean(bool),
    Other,
}

/// Whether two values of the Number arm's abstract interpretation are equal, as `==` and
/// `===` agree on them; `None` where the interpretation does not know.
fn number_dispatch_equal(left: NumberDispatchValue, right: NumberDispatchValue) -> Option<bool> {
    Some(match (left, right) {
        (NumberDispatchValue::TypeNumber, NumberDispatchValue::String(value))
        | (NumberDispatchValue::String(value), NumberDispatchValue::TypeNumber) => value == "number",
        (NumberDispatchValue::String(left), NumberDispatchValue::String(right)) => left == right,
        (NumberDispatchValue::Boolean(left), NumberDispatchValue::Boolean(right)) => left == right,
        (NumberDispatchValue::Number(left), NumberDispatchValue::Number(right)) => left == right,
        // `==` converts the other side to a Number, which the interpretation does not model.
        (NumberDispatchValue::Number(_), _) | (_, NumberDispatchValue::Number(_)) => return None,
        (NumberDispatchValue::Other, NumberDispatchValue::Other) => return None,
        _ => false,
    })
}

/// Interpret only the pure prefix executed when `typeof arg` is `"number"`.
/// Branches into other type arms may contain arbitrary effects; they are safe
/// because this proof follows the Number path and rejects effects on that path.
fn number_type_dispatch_reaches(
    program: &ResidualProgram,
    function: &Function,
    target: usize,
) -> Option<bool> {
    let mut registers = vec![None; usize::from(function.registers)];
    let mut locals = vec![None; usize::from(function.locals)];
    let mut pc = 0_usize;
    let mut fuel = function.code.len().saturating_mul(2);
    while fuel > 0 {
        if pc == target {
            return Some(true);
        }
        fuel -= 1;
        let Some(packed) = function.code.get(pc) else {
            return Some(false);
        };
        let instruction = packed.as_wide();
        let next = pc.saturating_add(1);
        let read = |operand: Operand,
                    registers: &[Option<NumberDispatchValue>]|
         -> Option<NumberDispatchValue> {
            match operand.kind()? {
                OperandKind::Register => registers
                    .get(usize::from(operand.register_index()?))?
                    .clone(),
                OperandKind::Constant => {
                    match program.constants.get(usize::from(operand.payload()))? {
                        Constant::String(value) => Some(NumberDispatchValue::String(value.to_string())),
                        Constant::Number(value) => Some(NumberDispatchValue::Number(*value)),
                        _ => Some(NumberDispatchValue::Other),
                    }
                }
                _ => None,
            }
        };
        match instruction.op() {
            Op::Unary
                if pc == 0
                    && instruction.unary_operator()
                        == oxc_ast::ast::UnaryOperator::Typeof as u32
                    && is_parameter_register(instruction.register_b(), 0) =>
            {
                *registers
                    .get_mut(usize::from(instruction.result_register()))
                    .unwrap() = Some(NumberDispatchValue::TypeNumber);
            }
            Op::Unary
                if pc == 0
                    && instruction.unary_operator() == crate::bytecode::UNARY_TYPEOF_INDEX
                    && is_parameter_register(instruction.register_b(), 0) =>
            {
                let number = crate::bytecode::typeof_name_index("number").expect("a typeof name") as f64;
                *registers.get_mut(usize::from(instruction.result_register()))? =
                    Some(NumberDispatchValue::Number(number));
            }
            Op::LoadConst => {
                let value = match program.constants.get(instruction.constant_index())? {
                    Constant::String(value) => NumberDispatchValue::String(value.to_string()),
                    Constant::Number(value) => NumberDispatchValue::Number(*value),
                    _ => NumberDispatchValue::Other,
                };
                *registers.get_mut(usize::from(instruction.result_register()))? = Some(value);
            }
            Op::Move => {
                let value = registers
                    .get(usize::from(instruction.register_b()))?
                    .clone()?;
                *registers.get_mut(usize::from(instruction.result_register()))? = Some(value);
            }
            Op::StoreLocal | Op::StoreLocalPlain => {
                let value = registers
                    .get(usize::from(instruction.register_a()))?
                    .clone()?;
                *locals.get_mut(instruction.local_slot())? = Some(value);
            }
            Op::LoadLocal | Op::LoadLocalPlain => {
                let value = locals.get(instruction.local_slot())?.clone()?;
                *registers.get_mut(usize::from(instruction.result_register()))? = Some(value);
            }
            Op::Binary
                if instruction.binary_operator() == BinaryOperator::Equality as u32
                    || instruction.binary_operator() == BinaryOperator::StrictEquality as u32
                    || instruction.binary_operator() == BinaryOperator::Inequality as u32
                    || instruction.binary_operator() == BinaryOperator::StrictInequality as u32 =>
            {
                let left = read(instruction.operand_b(), &registers)?;
                let right = read(instruction.operand_c(), &registers)?;
                let equal = number_dispatch_equal(left, right)?;
                let value = match instruction.binary_operator() {
                    x if x == BinaryOperator::Inequality as u32
                        || x == BinaryOperator::StrictInequality as u32 =>
                    {
                        !equal
                    }
                    _ => equal,
                };
                *registers.get_mut(usize::from(instruction.result_register()))? =
                    Some(NumberDispatchValue::Boolean(value));
            }
            Op::JumpBinaryFalse
                if [
                    BinaryOperator::Equality,
                    BinaryOperator::StrictEquality,
                    BinaryOperator::Inequality,
                    BinaryOperator::StrictInequality,
                ]
                .iter()
                .any(|operator| instruction.binary_operator_field() == *operator as u32) =>
            {
                let left = read(instruction.operand_b(), &registers)?;
                let right = read(instruction.operand_c(), &registers)?;
                let equal = number_dispatch_equal(left, right)?;
                let operator = instruction.binary_operator_field();
                let holds = if operator == BinaryOperator::Inequality as u32
                    || operator == BinaryOperator::StrictInequality as u32
                {
                    !equal
                } else {
                    equal
                };
                pc = if holds { next } else { usize::try_from(instruction.jump_target()).ok()? };
                continue;
            }
            Op::JumpTrue | Op::JumpFalse => {
                let condition = match registers
                    .get(usize::from(instruction.register_a()))?
                    .as_ref()?
                {
                    NumberDispatchValue::Boolean(value) => *value,
                    _ => return Some(false),
                };
                let take = if instruction.op() == Op::JumpTrue {
                    condition
                } else {
                    !condition
                };
                pc = if take {
                    usize::try_from(instruction.jump_target()).ok()?
                } else {
                    next
                };
                continue;
            }
            Op::Jump => {
                pc = usize::try_from(instruction.jump_target()).ok()?;
                continue;
            }
            _ => return Some(false),
        }
        pc = next;
    }
    Some(false)
}

/// Whether `register` holds parameter `index` at entry: parameters arrive in the first
/// registers, in order.
fn is_parameter_register(register: crate::bytecode::Register, index: u16) -> bool {
    register == index
}

fn operand_is_parameter(operand: Operand, index: u16) -> bool {
    operand.kind() == Some(OperandKind::Register) && operand.register_index().is_some_and(|register| is_parameter_register(register, index))
}

fn operands_are(left: Operand, right: Operand, one: u16, two: u16) -> bool {
    (operand_is_parameter(left, one) && operand_is_parameter(right, two))
        || (operand_is_parameter(left, two) && operand_is_parameter(right, one))
}

fn operand_is_number(program: &ResidualProgram, operand: Operand, expected: f64) -> bool {
    super::compiled::facts::constant_number(&program.constants, operand) == Some(expected)
}

fn compile_int32_identity(function_id: u32, return_pc: u32) -> Option<JsJitArtifact> {
    let isa = super::jit_backend::target_isa()?;
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(AbiParam::new(isa.pointer_type()));
    signature.returns.push(AbiParam::new(types::F64));
    let mut native =
        ir::Function::with_name_signature(UserFuncName::user(0x5155_454e, function_id), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let arguments = builder.block_params(entry)[0];
    let value = builder
        .ins()
        .load(types::F64, ir::MemFlagsData::trusted(), arguments, 0);
    builder.ins().return_(&[value]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    let mut context = cranelift_codegen::Context::for_function(native);
    let compiled = context
        .compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default())
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
        requires_signed_int32_arguments: false,
        result: JsJitResultKind::Int32Identity(return_pc),
        field_argument: None,
        numeric_field_inputs: Vec::new(),
        capture_arguments: Vec::new(),
        general_dispatch_native_pcs: Vec::new(),
    })
}

/// Captured values are dynamic inputs: the VM reloads each one for every call,
/// and native entry is used only while every capture is a Number.
pub(super) fn numeric_capture_candidate(
    function: &Function,
) -> Option<smallvec::SmallVec<[JsJitCaptureArgument; MAX_CAPTURE_ARGUMENTS]>> {
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
        || function.arguments_slot.is_some()
        || function.inherited_with_scope
        || !function.wide.is_empty()
    {
        return None;
    }

    let field_inputs = cached_numeric_field_inputs_candidate(function);
    let mut captures = smallvec::SmallVec::new();
    for packed in &function.code {
        let instruction = packed.as_wide();
        match instruction.op() {
            Op::LoadCapture => {
                if instruction.returns_from_frame() || instruction.result_flags() != 0 {
                    return None;
                }
                let capture = JsJitCaptureArgument {
                    depth: instruction.capture_depth(),
                    slot: instruction.capture_slot(),
                };
                if field_inputs.as_ref().is_some_and(|inputs| inputs.iter().any(|input| {
                    matches!(input.receiver, JsJitFieldReceiver::Capture { register, capture: receiver }
                        if register == instruction.result_register()
                            && receiver.depth == capture.depth && receiver.slot == capture.slot)
                })) {
                    continue;
                }
                if !captures.iter().any(|seen: &JsJitCaptureArgument| {
                    seen.depth == capture.depth && seen.slot == capture.slot
                }) {
                    if captures.len() == MAX_CAPTURE_ARGUMENTS {
                        return None;
                    }
                    captures.push(capture);
                }
            }
            Op::LoadConst
            | Op::Move
            | Op::GetField
            | Op::Binary
            | Op::NumericAdd
            | Op::NumericMultiply
            | Op::Jump
            | Op::JumpFalse
            | Op::JumpTrue
            | Op::JumpBinaryFalse
            | Op::Return => {}
            _ => return None,
        }
    }
    (!captures.is_empty()).then_some(captures)
}

/// A pure captured numeric value can be specialized to a native identity entry.
/// The VM reloads the capture for every call, so lexical mutation stays visible.
pub(super) fn number_capture_candidate(function: &Function) -> Option<JsJitCaptureArgument> {
    // The entry loads the capture and its fallthrough returns it; a Number result resumes at
    // the function's final `Return`, which must return the same register.
    let (entry, next) = (function_instruction(function, ENTRY_PC)?, function_instruction(function, ENTRY_PC + 1)?);
    if function.params != 0
        || !function.wide.is_empty()
        || !function.handlers.is_empty()
        || function.is_async
        || function.is_generator
        || function.is_class_constructor
        || function.derived_constructor
        || entry.op() != Op::LoadCapture
        || next.op() != Op::Return
        || entry.result_register() != next.register_a()
        || function.code.last()?.as_wide().op() != Op::Return
        || function.code.last()?.as_wide().register_a() != next.register_a()
    {
        return None;
    }
    let capture = entry;
    Some(JsJitCaptureArgument {
        depth: capture.capture_depth(),
        slot: capture.capture_slot(),
    })
}

/// Restrict bitwise-number lowering to expressions whose operands are either
/// direct parameters (guarded as signed Int32 at entry) or static Int32 number
/// constants. This keeps JavaScript ToInt32 semantics exact without a coercion
/// slow path inside generated code.
pub(super) fn has_int32_bitwise_candidate(program: &ResidualProgram, function: &Function) -> bool {
    let mut found = false;
    let mut branch_seen = false;
    let mut registers = [false; MAX_REGISTERS];
    for register in registers.iter_mut().take(usize::from(function.params)) {
        *register = true;
    }
    for packed in function.code.iter().copied() {
        let instruction = packed.as_wide();
        let op = instruction.op();
        if op.is_js_jump() {
            branch_seen = true;
            continue;
        }
        // Read only by the arms below, whose ops each write a result register.
        let destination = || usize::from(instruction.result_register());
        if op == Op::Binary
            && matches!(
                instruction.binary_operator(),
                x if x == BinaryOperator::BitwiseAnd as u32
                    || x == BinaryOperator::BitwiseOR as u32
                    || x == BinaryOperator::BitwiseXOR as u32
            )
        {
            // Guarded field and capture inputs are exact ToInt32 values at native entry.
            // Keep this proof straight-line so no unanalysed branch join can weaken it.
            if branch_seen {
                return false;
            }
            found = true;
            let operands_are_i32 = [instruction.operand_b(), instruction.operand_c()]
                .into_iter()
                .all(|operand| match operand.kind() {
                    Some(crate::bytecode::OperandKind::Register) => operand
                        .register_index()
                        .and_then(|register| registers.get(usize::from(register)))
                        .copied()
                        .unwrap_or(false),
                    Some(crate::bytecode::OperandKind::Constant) => {
                        let Some(Constant::Number(value)) =
                            program.constants.get(usize::from(operand.payload()))
                        else {
                            return false;
                        };
                        super::compiled::facts::is_int32_number(*value)
                    }
                    _ => false,
                });
            if !operands_are_i32 {
                return false;
            }
            if let Some(slot) = registers.get_mut(destination()) {
                *slot = true;
            }
            continue;
        }

        match op {
            Op::LoadConst => {
                let is_i32 = matches!(
                    program.constants.get(instruction.constant_index()),
                    Some(Constant::Number(value)) if super::compiled::facts::is_int32_number(*value)
                );
                if let Some(slot) = registers.get_mut(destination()) {
                    *slot = is_i32;
                }
            }
            Op::LoadCapture | Op::GetField => {
                if let Some(slot) = registers.get_mut(destination()) {
                    *slot = true;
                }
            }
            Op::Move => {
                let value = registers
                    .get(usize::from(instruction.register_b()))
                    .copied()
                    .unwrap_or(false);
                if let Some(slot) = registers.get_mut(destination()) {
                    *slot = value;
                }
            }
            Op::Unary | Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                if let Some(slot) = registers.get_mut(destination()) {
                    *slot = false;
                }
            }
            _ => {}
        }
    }
    found
}

fn compile_capture_number(
    function_id: u32,
    capture: JsJitCaptureArgument,
) -> Option<JsJitArtifact> {
    let isa = super::jit_backend::target_isa()?;
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(AbiParam::new(types::F64));
    signature.returns.push(AbiParam::new(types::F64));
    let mut native =
        ir::Function::with_name_signature(UserFuncName::user(0x5155_454e, function_id), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let value = builder.block_params(entry)[0];
    builder.ins().return_(&[value]);
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
        params: 0,
        requires_signed_int32_arguments: false,
        result: JsJitResultKind::CaptureNumber(capture),
        field_argument: None,
        numeric_field_inputs: Vec::new(),
        capture_arguments: Vec::new(),
        general_dispatch_native_pcs: Vec::new(),
    })
}

/// Stable top-level rejection category for a hot candidate that did not
/// produce a JS artifact. Detailed lowering instrumentation can refine this
/// later without changing the diagnostic contract.
pub(super) fn candidate_rejection_reason(
    program: &ResidualProgram,
    function_id: u32,
) -> &'static str {
    let Some(function) = program.function_get(function_id as usize) else {
        return "invalid-function-id";
    };
    if function.code.is_empty() {
        return "empty-function";
    }
    if function.code.len() > MAX_INSTRUCTIONS {
        return "bytecode-limit";
    }
    if int32_identity_return_candidate(program, function).is_some() {
        return "int32-identity-return";
    }
    if usize::from(function.registers) > MAX_REGISTERS {
        return "register-limit";
    }
    if usize::from(function.params) > MAX_PARAMETERS {
        return "parameter-limit";
    }
    let field_reject = cached_field_argument_candidate(function)
        .is_some_and(|field| field_reject_layout(function).is_some() && field.entry_pc == 0);
    if !eligible(function, program) && !field_reject {
        return "unsupported-activation-or-function-kind";
    }
    if !function_candidate(function, program) {
        return "unsupported-opcode-or-control-flow";
    }
    "unsupported-lowering-shape-or-codegen-failure"
}

pub(super) fn cached_field_argument_candidate(function: &Function) -> Option<JsJitFieldArgument> {
    if function.params != 1 {
        return None;
    }
    let first = function_instruction(function, ENTRY_PC)?;
    let (entry_pc, instruction) = match first.op() {
        Op::GetField => (ENTRY_PC, first),
        // A falsy parameter leaves; the read is the test's fallthrough.
        Op::JumpFalse if is_parameter_register(first.register_a(), 0) => {
            let field = function_instruction(function, ENTRY_PC + 1)?;
            if field.op() != Op::GetField {
                return None;
            }
            (1, field)
        }
        _ => return None,
    };
    let FieldLookup::Atom {
        base: FieldBase(0),
        cache_site,
        ..
    } = instruction.field_lookup()?
    else {
        return None;
    };
    Some(JsJitFieldArgument {
        cache_site,
        entry_pc,
    })
}

/// Find one cached field read in a side-effect-free numeric region. The object
/// is formal parameter zero; the field value is appended to the numeric ABI
/// after parameters and captures. Control flow and effectful opcodes are left
/// to their existing denotations.
pub(super) fn cached_numeric_field_argument_candidate(
    function: &Function,
) -> Option<JsJitFieldArgument> {
    if function.params == 0 {
        return None;
    }
    let mut candidate = None;
    for (pc, packed) in function.code.iter().copied().enumerate() {
        if packed.is_wide() {
            return None;
        }
        let instruction = packed.as_wide();
        if instruction.op() == Op::GetField {
            if instruction.returns_from_frame()
                || instruction.result_flags() != 0
                || instruction.writes_current_this()
                || candidate.is_some()
            {
                return None;
            }
            let FieldLookup::Atom {
                base: FieldBase(0),
                cache_site,
                ..
            } = instruction.field_lookup()?
            else {
                return None;
            };
            candidate = Some(JsJitFieldArgument {
                cache_site,
                entry_pc: pc,
            });
        } else if !matches!(
            instruction.op(),
            Op::LoadConst
                | Op::Move
                | Op::LoadCapture
                | Op::Binary
                | Op::NumericAdd
                | Op::NumericMultiply
                | Op::Return
        ) {
            return None;
        }
    }
    candidate
}

/// Collect cached reads from `this`, parameters, and captured objects in pure
/// regions. Field values are guarded and supplied as separate ABI inputs, so
/// native code never holds a guest heap reference.
pub(super) fn cached_numeric_field_inputs_candidate(
    function: &Function,
) -> Option<smallvec::SmallVec<[JsJitNumericFieldInput; MAX_NUMERIC_FIELD_INPUTS]>> {
    if function.params != 0
        && function.code.iter().any(|packed| {
            !packed.is_wide()
                && packed.as_wide().op().is_js_jump()
        })
    {
        // Parameter-backed branch regions still use their established entry
        // checks; the new multi-field path targets zero-argument `this` helpers.
        return None;
    }
    let mut inputs: smallvec::SmallVec<[_; MAX_NUMERIC_FIELD_INPUTS]> = smallvec::SmallVec::new();
    let mut properties: smallvec::SmallVec<[(JsJitFieldReceiver, crate::bytecode::Atom); MAX_NUMERIC_FIELD_INPUTS]> = smallvec::SmallVec::new();
    let mut captured_registers = [None; MAX_REGISTERS];
    let mut parameter_unchanged = [true; MAX_REGISTERS];
    let mut branch_seen = false;
    for (pc, packed) in function.code.iter().copied().enumerate() {
        if packed.is_wide() {
            return None;
        }
        let instruction = packed.as_wide();
        if instruction.op() == Op::GetField {
            if instruction.returns_from_frame()
                || instruction.result_flags() != 0
                || instruction.writes_current_this()
            {
                return None;
            }
            let FieldLookup::Atom {
                atom, base, cache_site,
            } = instruction.field_lookup()?
            else {
                return None;
            };
            let receiver = if base == FieldBase::THIS {
                JsJitFieldReceiver::This
            } else if let Some(parameter) = base
                .register_index()
                .filter(|parameter| *parameter < function.params
                    && parameter_unchanged.get(usize::from(*parameter)).copied().unwrap_or(false))
            {
                JsJitFieldReceiver::Parameter(parameter)
            } else {
                let register = base.register_index()?;
                let capture = captured_registers
                    .get(usize::from(register))
                    .copied()
                    .flatten()?;
                if function.params != 0 {
                    return None;
                }
                JsJitFieldReceiver::Capture { register, capture }
            };
            if branch_seen && matches!(receiver, JsJitFieldReceiver::Parameter(_)) {
                // Preserve the existing field entry path for parameter reads
                // reached only after a branch, such as a null check. `this`
                // is already a known receiver and can use its guarded cache
                // entry across forward control flow.
                return None;
            }
            if inputs.len() == MAX_NUMERIC_FIELD_INPUTS {
                return None;
            }
            let source = properties.iter().position(|property| *property == (receiver, atom))
                .unwrap_or(properties.len());
            properties.push((receiver, atom));
            inputs.push(JsJitNumericFieldInput {
                cache_site,
                pc,
                receiver,
                source: u16::try_from(source).ok()?,
            });
        } else if instruction.op() == Op::LoadCapture {
            if instruction.returns_from_frame() || instruction.result_flags() != 0 {
                return None;
            }
            *captured_registers.get_mut(usize::from(instruction.result_register()))? =
                Some(JsJitCaptureArgument {
                    depth: instruction.capture_depth(),
                    slot: instruction.capture_slot(),
                });
        } else if !matches!(
            instruction.op(),
            Op::LoadConst
                | Op::Move
                | Op::Binary
                | Op::NumericAdd
                | Op::NumericMultiply
                | Op::Unary
                | Op::Jump
                | Op::JumpFalse
                | Op::JumpTrue
                | Op::JumpBinaryFalse
                | Op::Return
        ) {
            return None;
        } else if instruction.op().is_js_jump() {
            if usize::try_from(instruction.jump_target()).is_ok_and(|target| target <= pc) {
                // Loops stay in the OSR compiler; this function collects
                // inputs only for acyclic native regions.
                return None;
            }
            branch_seen = true;
        }
        let result_register = match instruction.op() {
            Op::LoadConst | Op::LoadCapture | Op::Move | Op::GetField | Op::Binary
            | Op::NumericAdd | Op::NumericMultiply | Op::Unary => Some(instruction.result_register()),
            _ => None,
        };
        if let Some(register) = result_register {
            if usize::from(register) < usize::from(function.params) {
                *parameter_unchanged.get_mut(usize::from(register))? = false;
            }
            if instruction.op() != Op::LoadCapture {
                *captured_registers.get_mut(usize::from(register))? = None;
            }
        }
    }
    let has_branches = function.code.iter().any(|packed| {
        !packed.is_wide()
            && packed.as_wide().op().is_js_jump()
    });
    if has_branches
        && inputs
            .iter()
            .any(|input| matches!(input.receiver, JsJitFieldReceiver::Capture { .. }))
    {
        return None;
    }
    (!inputs.is_empty()).then_some(inputs)
}

/// Static inputs and completion of a captured-value field assignment.
#[derive(Clone, Copy)]
pub(super) struct CapturedFieldStore {
    pub(super) atom: crate::bytecode::Atom,
    pub(super) capture_slot: u16,
    pub(super) cache_site: u16,
    pub(super) return_register: u16,
    pub(super) update: Option<super::operations::NumberBitwise>,
}

#[cfg(feature = "native-execution")]
pub(super) const CAPTURED_STORE_ASSIGNMENT: u16 = u16::MAX;

/// What a register of a captured-store body holds, as its straight-line code computes it.
#[derive(Clone, Copy, PartialEq)]
enum StoreValue {
    Unknown,
    Field(crate::bytecode::Atom),
    Capture(u16),
    Update(crate::bytecode::Atom, u16, super::operations::NumberBitwise),
    Undefined,
}

/// The value an instruction of a captured-store body computes, other than its store and
/// return; `None` for any other instruction.
fn store_value(program: &ResidualProgram, instruction: WideInstruction, values: &[StoreValue]) -> Option<StoreValue> {
    let value = |register: crate::bytecode::Register| values.get(usize::from(register)).copied();
    Some(match instruction.op() {
        Op::LoadCapture if instruction.capture_depth() == 0 => StoreValue::Capture(instruction.capture_slot()),
        Op::LoadConst => match program.constants.get(instruction.constant_index()) {
            Some(crate::bytecode::Constant::Undefined) => StoreValue::Undefined,
            _ => return None,
        },
        Op::GetField => match instruction.field_lookup()? {
            FieldLookup::Atom { atom, base: FieldBase::THIS, .. } => StoreValue::Field(atom),
            _ => return None,
        },
        Op::Binary => {
            let operation = super::operations::NumberBitwise::from_js_instruction(instruction.op(), instruction.binary_operator())?;
            match (value(instruction.operand_b().register_index()?)?, value(instruction.operand_c().register_index()?)?) {
                (StoreValue::Field(atom), StoreValue::Capture(slot)) => StoreValue::Update(atom, slot, operation),
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// The store a body that returns `result` from `register` makes, once it has made `stored`.
fn finished_store(
    (source, store): (StoreValue, WideInstruction),
    result: StoreValue,
    register: crate::bytecode::Register,
) -> Option<CapturedFieldStore> {
    let (capture_slot, update) = match (source, result) {
        (StoreValue::Capture(slot), StoreValue::Undefined) => (slot, None),
        (StoreValue::Update(_, slot, operation), StoreValue::Undefined) => (slot, Some(operation)),
        _ => return None,
    };
    Some(CapturedFieldStore {
        atom: store.atom_index(), capture_slot,
        cache_site: store.cache_site_index(), return_register: register,
        update,
    })
}

/// A body that assigns a captured value, or a bitwise update of a field of `this` by one, to
/// that field of `this`, and returns undefined: found by evaluating its straight-line code
/// symbolically, whatever registers and order the compiler chose. The activation and all
/// implicit bindings must be plain before it can be elided.
pub(super) fn captured_field_store_candidate(
    program: &ResidualProgram,
    function: &Function,
) -> Option<CapturedFieldStore> {
    if function.params != 0 || !function.plain_activation(program) || !eligible(function, program) {
        return None;
    }
    let mut values = [StoreValue::Unknown; MAX_REGISTERS];
    let mut stored: Option<(StoreValue, WideInstruction)> = None;
    let mut reads = 0_usize;
    for instruction in function.instructions() {
        // The elided body performs the one store, preceded by at most the one field read its
        // update needs; past the store only the undefined result remains to be made.
        let settled = stored.is_some() && !matches!(instruction.op(), Op::LoadConst | Op::Return);
        reads += usize::from(instruction.op() == Op::GetField);
        if instruction.result_flags() != 0 || settled || reads > 1 {
            return None;
        }
        let value = |register: crate::bytecode::Register| values.get(usize::from(register)).copied();
        let result = match instruction.op() {
            _ if instruction.stores_to_this() && stored.is_none() => {
                let source = value(instruction.register_a())?;
                let fits = match source {
                    StoreValue::Capture(_) => reads == 0,
                    StoreValue::Update(atom, ..) => atom == instruction.atom_index(),
                    _ => false,
                };
                if !fits {
                    return None;
                }
                stored = Some((source, instruction));
                continue;
            }
            Op::Return => return finished_store(stored?, value(instruction.register_a())?, instruction.register_a()),
            _ => store_value(program, instruction, &values)?,
        };
        *values.get_mut(usize::from(instruction.result_register()))? = result;
    }
    None
}

fn function_instruction(function: &Function, pc: usize) -> Option<WideInstruction> {
    let packed = function.code.get(pc)?;
    if packed.is_wide() {
        function.wide.get(packed.wide_index()).copied()
    } else {
        Some(packed.as_wide())
    }
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

pub(super) fn has_numeric_loop_candidate(function: &Function) -> bool {
    function.code.iter().enumerate().any(|(pc, instruction)| {
        if instruction.is_wide() {
            return false;
        }
        let instruction = instruction.as_wide();
        instruction.op().is_js_jump() && usize::try_from(instruction.jump_target()).is_ok_and(|target| target <= pc)
    })
}

/// Compile a forward-only boolean tree after specializing formal parameters to
/// Number. Static `typeof` and string-equality tests are reduced while the
/// control-flow graph is discovered, so bytecodes on impossible type branches
/// do not constrain the generated function. Dynamic Number comparisons remain
/// native comparisons. Every non-Number entry is rejected by the caller's
/// guard and resumes from the untouched interpreter frame.
pub(super) struct JsBooleanRegion {
    pub(super) function: ir::Function,
    pub(super) params: usize,
    pub(super) requires_signed_int32_arguments: bool,
    pub(super) field_argument: Option<JsJitFieldArgument>,
    pub(super) numeric_field_inputs: Vec<JsJitNumericFieldInput>,
    pub(super) capture_arguments: Vec<JsJitCaptureArgument>,
}

/// A boolean region returns its exact bytecode PC above the low result bit.
pub(super) const BOOLEAN_REGION_RESULT_BITS: u32 = 1;
pub(super) const BOOLEAN_REGION_RESULT_MASK: u32 = (1 << BOOLEAN_REGION_RESULT_BITS) - 1;

fn emit_boolean_region_return(
    builder: &mut FunctionBuilder<'_>, value: ir::Value, pc: usize,
) -> Option<()> {
    let encoded_pc = u32::try_from(pc.checked_mul(1usize << BOOLEAN_REGION_RESULT_BITS)?).ok()?;
    let encoded = builder.ins().bor_imm_u(value, i64::from(encoded_pc));
    builder.ins().return_(&[encoded]);
    Some(())
}

fn compile_number_domain_boolean_tree(
    program: &ResidualProgram,
    function_id: u32,
    function: &Function,
) -> Option<JsJitArtifact> {
    let isa = super::jit_backend::target_isa()?;
    let region = lower_number_domain_boolean_tree(program, function_id, function, &*isa)?;
    let mut context = cranelift_codegen::Context::for_function(region.function);
    let compiled = context
        .compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default())
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
        params: region.params,
        requires_signed_int32_arguments: region.requires_signed_int32_arguments,
        result: JsJitResultKind::Boolean,
        field_argument: region.field_argument,
        numeric_field_inputs: region.numeric_field_inputs,
        capture_arguments: region.capture_arguments,
        general_dispatch_native_pcs: Vec::new(),
    })
}

/// Residualize the boolean Number domain independently of code publication.
pub(super) fn lower_number_domain_boolean_tree(
    program: &ResidualProgram,
    function_id: u32,
    function: &Function,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
) -> Option<JsBooleanRegion> {
    if !function_candidate(function, program)
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
    // The branch-aware field path is currently limited to `this` reads in
    // zero-parameter helpers. Parameter reads retain the established guarded
    // entry path, which handles null checks before the field access.
    let numeric_field_inputs = (function.params == 0)
        .then(|| cached_numeric_field_inputs_candidate(function))
        .flatten();
    let field_argument = if numeric_field_inputs.is_none() {
        cached_field_argument_candidate(function)
    } else {
        None
    };
    let entry_pc = field_argument.map_or(0, |field| field.entry_pc);
    if field_argument.is_some() {
        entry.registers[0] = BranchValue::Unknown;
    }
    if let Some(inputs) = &numeric_field_inputs {
        for input in inputs {
            if let JsJitFieldReceiver::Parameter(parameter) = input.receiver {
                *entry.registers.get_mut(usize::from(parameter))? = BranchValue::Unknown;
            }
        }
    }
    let capture_arguments = numeric_capture_candidate(function).unwrap_or_default();
    states[entry_pc] = Some(entry);
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
            Op::LoadCapture => {
                let captured_object_receiver =
                    numeric_field_inputs.as_ref().is_some_and(|inputs| {
                        inputs.iter().any(|input| {
                            matches!(input.receiver,
                                JsJitFieldReceiver::Capture {
                                    register,
                                    capture,
                                } if register == instruction.result_register()
                                    && capture.depth == instruction.capture_depth()
                                    && capture.slot == instruction.capture_slot())
                        })
                    });
                if captured_object_receiver {
                    *state
                        .registers
                        .get_mut(usize::from(instruction.result_register()))? =
                        BranchValue::Unknown;
                } else {
                    if !capture_arguments.iter().any(|capture| {
                        capture.depth == instruction.capture_depth()
                            && capture.slot == instruction.capture_slot()
                    }) {
                        return None;
                    }
                    *state
                        .registers
                        .get_mut(usize::from(instruction.result_register()))? = BranchValue::Number;
                }
                successors.push(pc + 1);
            }
            Op::GetField => {
                let input = numeric_field_inputs
                    .as_ref()
                    .and_then(|inputs| inputs.iter().find(|input| input.pc == pc));
                if input.is_none() && (pc != entry_pc || field_argument.is_none()) {
                    return None;
                }
                if let Some(input) = input {
                    let expected_base = match input.receiver {
                        JsJitFieldReceiver::This => FieldBase::THIS,
                        JsJitFieldReceiver::Parameter(parameter) => FieldBase(parameter),
                        JsJitFieldReceiver::Capture { register, .. } => FieldBase(register),
                    };
                    if !matches!(
                        instruction.field_lookup(),
                        Some(FieldLookup::Atom { base, cache_site, .. })
                            if base == expected_base && cache_site == input.cache_site
                    ) {
                        return None;
                    }
                }
                let result = usize::from(instruction.result_register());
                *state.registers.get_mut(result)? = BranchValue::Number;
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
                let slot = instruction.local_slot();
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
                let slot = instruction.local_slot();
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
                let left = branch_operand(
                    program,
                    &state.registers,
                    &state.locals,
                    instruction.operand_b(),
                )?;
                let right = branch_operand(
                    program,
                    &state.registers,
                    &state.locals,
                    instruction.operand_c(),
                )?;
                let bitwise = matches!(
                    instruction.binary_operator(),
                    x if x == BinaryOperator::BitwiseAnd as u32
                        || x == BinaryOperator::BitwiseOR as u32
                        || x == BinaryOperator::BitwiseXOR as u32
                );
                let result = match (left, right) {
                    (left, right)
                        if bitwise
                            && branch_value_is_number(left)
                            && branch_value_is_number(right)
                            && has_int32_bitwise_candidate(program, function) =>
                    {
                        BranchValue::Number
                    }
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
                let left = branch_operand(
                    program,
                    &state.registers,
                    &state.locals,
                    instruction.operand_b(),
                )?;
                let right = branch_operand(
                    program,
                    &state.registers,
                    &state.locals,
                    instruction.operand_c(),
                )?;
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
                    || !branch_value_is_boolean(*state.registers.get(usize::from(instruction.register_a()))?)
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
    let entry_block = blocks[entry_pc]?;
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
    for (index, capture) in capture_arguments.iter().enumerate() {
        let register = function
            .code
            .iter()
            .map(|packed| packed.as_wide())
            .find(|instruction| {
                instruction.op() == Op::LoadCapture
                    && instruction.capture_depth() == capture.depth
                    && instruction.capture_slot() == capture.slot
            })?
            .result_register();
        let argument_index = params.checked_add(index)?;
        let offset = i32::try_from(argument_index.checked_mul(std::mem::size_of::<f64>())?).ok()?;
        let value = builder
            .ins()
            .load(types::F64, ir::MemFlagsData::trusted(), arguments, offset);
        builder.def_var(*variables.get(usize::from(register))?, value);
    }

    let mut emitted_return = false;
    for pc in 0..code_len {
        let Some(state) = states[pc].clone() else {
            continue;
        };
        let block = blocks[pc]?;
        if pc != entry_pc {
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
            Op::LoadCapture => {}
            Op::GetField => {
                let value = if let Some((field_index, _)) = numeric_field_inputs
                    .as_ref()
                    .and_then(|inputs| {
                        inputs
                            .iter()
                            .enumerate()
                            .find(|(_, input)| input.pc == pc)
                    })
                {
                    let argument_index = params
                        .checked_add(capture_arguments.len())?
                        .checked_add(field_index)?;
                    let offset =
                        i32::try_from(argument_index.checked_mul(std::mem::size_of::<f64>())?)
                            .ok()?;
                    builder
                        .ins()
                        .load(types::F64, ir::MemFlagsData::trusted(), arguments, offset)
                } else if field_argument.is_some_and(|field| field.entry_pc == pc) {
                    // Legacy field-argument entries pass only the guarded
                    // property value at ABI slot zero. The appended slot
                    // layout belongs to numeric_field_inputs above.
                    builder
                        .ins()
                        .load(types::F64, ir::MemFlagsData::trusted(), arguments, 0)
                } else {
                    return None;
                };
                builder.def_var(
                    *variables.get(usize::from(instruction.result_register()))?,
                    value,
                );
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
                        *local_variables.get(instruction.local_slot())?,
                        value,
                    );
                    if let Some(register) = instruction.optional_register_b() {
                        builder.def_var(*variables.get(usize::from(register))?, value);
                    }
                }
            }
            Op::LoadLocal | Op::LoadLocalPlain => {
                let local = instruction.local_slot();
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
                    emit_boolean_region_return(&mut builder, result, pc)?;
                    emitted_return = true;
                }
            }
            Op::Binary => {
                let left = branch_operand(
                    program,
                    &state.registers,
                    &state.locals,
                    instruction.operand_b(),
                )?;
                let right = branch_operand(
                    program,
                    &state.registers,
                    &state.locals,
                    instruction.operand_c(),
                )?;
                if branch_value_is_number(left) && branch_value_is_number(right) {
                    let operator = instruction.binary_operator();
                    if matches!(
                        operator,
                        x if x == BinaryOperator::BitwiseAnd as u32
                            || x == BinaryOperator::BitwiseOR as u32
                            || x == BinaryOperator::BitwiseXOR as u32
                    ) {
                        if instruction.result_flags() == RETURN_REGISTER {
                            return None;
                        }
                        let left = branch_numeric_operand(
                            &mut builder,
                            &variables,
                            &local_variables,
                            program,
                            &state.registers,
                            &state.locals,
                            instruction.operand_b(),
                        )?;
                        let right = branch_numeric_operand(
                            &mut builder,
                            &variables,
                            &local_variables,
                            program,
                            &state.registers,
                            &state.locals,
                            instruction.operand_c(),
                        )?;
                        let left = builder.ins().fcvt_to_sint(types::I32, left);
                        let right = builder.ins().fcvt_to_sint(types::I32, right);
                        let value = match operator {
                            x if x == BinaryOperator::BitwiseAnd as u32 => {
                                builder.ins().band(left, right)
                            }
                            x if x == BinaryOperator::BitwiseOR as u32 => {
                                builder.ins().bor(left, right)
                            }
                            _ => builder.ins().bxor(left, right),
                        };
                        let value = builder.ins().fcvt_from_sint(types::F64, value);
                        builder.def_var(
                            *variables.get(usize::from(instruction.result_register()))?,
                            value,
                        );
                    } else {
                        let condition = branch_number_comparison(
                            &mut builder,
                            operator,
                            &variables,
                            &local_variables,
                            program,
                            &state.registers,
                            &state.locals,
                            instruction.operand_b(),
                            instruction.operand_c(),
                        )?;
                        if instruction.result_flags() == RETURN_REGISTER {
                            let yes = builder.ins().iconst(types::I32, 1);
                            let no = builder.ins().iconst(types::I32, 0);
                            let result = builder.ins().select(condition, yes, no);
                            emit_boolean_region_return(&mut builder, result, pc)?;
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
                let left = branch_operand(
                    program,
                    &state.registers,
                    &state.locals,
                    instruction.operand_b(),
                )?;
                let right = branch_operand(
                    program,
                    &state.registers,
                    &state.locals,
                    instruction.operand_c(),
                )?;
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
                        &local_variables,
                        program,
                        &state.registers,
                        &state.locals,
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
                emit_boolean_region_return(&mut builder, result, pc)?;
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
    Some(JsBooleanRegion {
        function: native,
        params,
        requires_signed_int32_arguments: has_int32_bitwise_candidate(program, function),
        field_argument,
        numeric_field_inputs: numeric_field_inputs.map_or_else(Vec::new, |inputs| inputs.into_vec()),
        capture_arguments: capture_arguments.into_vec(),
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

fn branch_value_is_boolean(value: BranchValue) -> bool {
    matches!(value, BranchValue::BooleanLiteral { .. } | BranchValue::BooleanFolded(_) | BranchValue::BooleanDynamic)
}

fn branch_operand(
    program: &ResidualProgram,
    registers: &[BranchValue],
    locals: &[BranchValue],
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
        OperandKind::Local => locals
            .get(usize::from(operand.payload()))
            .copied()
            .filter(|value| *value != BranchValue::Unknown),
        OperandKind::Field => None,
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
    local_variables: &[cranelift_frontend::Variable],
    program: &ResidualProgram,
    state: &[BranchValue],
    local_state: &[BranchValue],
    left: crate::bytecode::Operand,
    right: crate::bytecode::Operand,
) -> Option<ir::Value> {
    let lhs = branch_numeric_operand(
        builder,
        variables,
        local_variables,
        program,
        state,
        local_state,
        left,
    )?;
    let rhs = branch_numeric_operand(
        builder,
        variables,
        local_variables,
        program,
        state,
        local_state,
        right,
    )?;
    super::compiled::emit_number_relation(
        builder,
        operator,
        lhs,
        rhs,
        super::compiled::NumberRelationDomain::Float64,
    )
}

fn branch_numeric_operand(
    builder: &mut FunctionBuilder<'_>,
    variables: &[cranelift_frontend::Variable],
    local_variables: &[cranelift_frontend::Variable],
    program: &ResidualProgram,
    state: &[BranchValue],
    local_state: &[BranchValue],
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
                _ => None,
            }
        }
        OperandKind::Constant => {
            let value = super::compiled::facts::constant_number(&program.constants, operand)?;
            Some(builder.ins().f64const(Ieee64::with_bits(value.to_bits())))
        }
        OperandKind::Local => {
            let index = usize::from(operand.payload());
            match local_state.get(index).copied()? {
                BranchValue::Number => Some(builder.use_var(*local_variables.get(index)?)),
                BranchValue::NumberConstant(constant) => {
                    let Constant::Number(value) = program.constants.get(constant as usize)? else {
                        return None;
                    };
                    Some(builder.ins().f64const(Ieee64::with_bits(value.to_bits())))
                }
                _ => None,
            }
        }
        OperandKind::Field => None,
    }
}

fn compile_numeric(program: &ResidualProgram, function_id: u32) -> Option<JsJitArtifact> {
    compile_numeric_with_captures(program, function_id, &[])
}

fn compile_numeric_with_captures(
    program: &ResidualProgram,
    function_id: u32,
    captures: &[JsJitCaptureArgument],
) -> Option<JsJitArtifact> {
    let function = program.function_get(function_id as usize)?;
    if !function_candidate(function, program)
        || function.code.iter().any(|instruction| instruction.is_wide())
    {
        return None;
    }

    // Pure scalar control flow has no safepoint or callout and uses one guarded entry.
    let numeric_field_inputs = cached_numeric_field_inputs_candidate(function);
    let field_argument = if numeric_field_inputs.is_some() {
        None
    } else {
        cached_numeric_field_argument_candidate(function)
    };
    super::compiled::compile_js_numeric_region(
        program,
        function_id,
        captures,
        numeric_field_inputs.as_deref(),
        field_argument,
        has_int32_bitwise_candidate(program, function),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NumericFlowKind {
    Uninitialized,
    Number,
    Boolean,
    Conflict,
}

impl NumericFlowKind {
    fn is_numeric(self) -> bool {
        matches!(self, NumericFlowKind::Number | NumericFlowKind::Boolean)
    }
}

/// Compile pure numeric loops as Cranelift CFGs. Every backedge polls the
/// VM's cancellation flag; an interrupted function has no guest-visible
/// effects, so it safely resumes through the ordinary interpreter path.
fn compile_numeric_loop(
    program: &ResidualProgram,
    function_id: u32,
    function: &Function,
) -> Option<JsJitArtifact> {
    if !function_candidate(function, program)
        || function
            .code
            .iter()
            .any(|instruction| instruction.is_wide())
    {
        return None;
    }
    if let Some(region) = super::compiled::compile_js_typed_numeric_region(program, function_id) {
        return Some(region);
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
                if !source.is_numeric() {
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
                if !kind.is_numeric() {
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
                if !source.is_numeric() {
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
                let number_inputs = left == NumericFlowKind::Number
                    && right == NumericFlowKind::Number;
                let operator = instruction.binary_operator();
                let arithmetic = super::operations::NumberArithmetic::from_js_instruction(
                    instruction.op(),
                    operator,
                );
                let bitwise = super::operations::NumberBitwise::from_js_instruction(
                    instruction.op(),
                    operator,
                );
                let relation = (instruction.op() == Op::Binary)
                    .then(|| super::operations::NumberRelation::from_operator(operator))
                    .flatten();
                let kind = if number_inputs && (arithmetic.is_some() || bitwise.is_some()) {
                    NumericFlowKind::Number
                } else if number_inputs && relation.is_some() {
                    NumericFlowKind::Boolean
                } else {
                    return None;
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
                if !condition.is_numeric() {
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

    let isa = super::jit_backend::target_isa()?;
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
    for (index, &variable) in numeric_variables[..params].iter().enumerate() {
        let offset = i32::try_from(index.checked_mul(std::mem::size_of::<f64>())?).ok()?;
        let value = builder
            .ins()
            .load(types::F64, ir::MemFlagsData::trusted(), arguments, offset);
        builder.def_var(variable, value);
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
                let operator = instruction.binary_operator();
                let value = match (instruction.op(), operator) {
                    (instruction, operator)
                        if let Some(operation) =
                            super::operations::NumberArithmetic::from_js_instruction(
                                instruction,
                                operator,
                            ) =>
                    {
                        let result = super::compiled::emit_number_arithmetic_operation(
                            &mut builder,
                            operation,
                            left,
                            right,
                        );
                        Some((NumericFlowKind::Number, result))
                    }
                    (instruction, operator)
                        if let Some(operation) =
                            super::operations::NumberBitwise::from_js_instruction(
                                instruction,
                                operator,
                            ) =>
                    {
                        let left = numeric_to_i32(&mut builder, left);
                        let right = numeric_to_i32(&mut builder, right);
                        let result = super::compiled::emit_number_bitwise_operation(
                            &mut builder,
                            operation,
                            left,
                            right,
                        );
                        Some((NumericFlowKind::Number, result))
                    }
                    (Op::Binary, value)
                        if super::operations::NumberRelation::from_operator(value).is_some() =>
                    {
                        let condition = super::compiled::emit_number_relation(
                            &mut builder,
                            value,
                            left,
                            right,
                            super::compiled::NumberRelationDomain::Float64,
                        )?;
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
                let condition = super::compiled::emit_number_relation(
                    &mut builder,
                    instruction.binary_operator_field(),
                    left,
                    right,
                    super::compiled::NumberRelationDomain::Float64,
                )?;
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
        requires_signed_int32_arguments: false,
        result: JsJitResultKind::NumberInterruptible,
        field_argument: None,
        numeric_field_inputs: Vec::new(),
        capture_arguments: Vec::new(),
        general_dispatch_native_pcs: Vec::new(),
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
            kind.is_numeric().then_some(kind)
        }
        OperandKind::Constant => matches!(
            program.constants.get(usize::from(operand.payload()))?,
            Constant::Number(_)
        )
        .then_some(NumericFlowKind::Number),
        OperandKind::Local => {
            let local = usize::from(operand.payload());
            let kind = *state.get(registers.checked_add(local)?)?;
            kind.is_numeric().then_some(kind)
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
            let value = super::compiled::facts::constant_number(&program.constants, operand)?;
            Some(builder.ins().f64const(Ieee64::with_bits(value.to_bits())))
        }
        OperandKind::Local => {
            let local = registers.checked_add(usize::from(operand.payload()))?;
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
    let isa = super::jit_backend::target_isa()?;

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
                let condition = comparison(&mut builder, program, instruction, &registers, &state)?;
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
        requires_signed_int32_arguments: false,
        result: JsJitResultKind::StringReturnPc(return_sites),
        field_argument: None,
        numeric_field_inputs: Vec::new(),
        capture_arguments: Vec::new(),
        general_dispatch_native_pcs: Vec::new(),
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
    if branch_value_is_boolean(left) && branch_value_is_boolean(right) {
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
    let operator = instruction_binary_operator(instruction)?;
    if !matches!(
        super::operations::NumberRelation::from_operator(operator)?,
        super::operations::NumberRelation::LessThan
            | super::operations::NumberRelation::LessEqual
            | super::operations::NumberRelation::GreaterThan
            | super::operations::NumberRelation::GreaterEqual
    ) {
        return None;
    }
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
    super::compiled::emit_number_relation(
        builder,
        operator,
        left,
        right,
        super::compiled::NumberRelationDomain::Float64,
    )
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
/// Whether the tier may compile `function` of `program`, its own: decided once per function,
/// since its code and its program never change. Every frame entry and lane call of a function
/// the tier has not tracked asks, and a fresh scan of its instructions each time had cost the
/// semver npm scenario 4.6% more instructions in JIT mode than without it.
pub(super) fn function_candidate(function: &Function, program: &crate::bytecode::ResidualProgram) -> bool {
    *function.js_tier_candidate.get_or_init(|| scanned_function_candidate(function, program))
}

fn scanned_function_candidate(function: &Function, program: &crate::bytecode::ResidualProgram) -> bool {
    !function.code.is_empty()
        && function.code.len() <= MAX_INSTRUCTIONS
        && (function.wide.is_empty()
            || wide_boolean_tree_candidate(function)
            || field_reject_layout(function).is_some())
        && usize::from(function.registers) <= MAX_REGISTERS
        && usize::from(function.params) <= MAX_PARAMETERS
        && (eligible(function, program) || field_reject_layout(function).is_some())
}

#[derive(Clone, Copy)]
struct FieldRejectLayout {
    branch_pc: usize,
    constant_pc: usize,
    continuation_pc: usize,
    resume_register: usize,
}

fn field_reject_layout(function: &Function) -> Option<FieldRejectLayout> {
    if function.params != 1
        || function.code.len() < 4
        || function.parent.is_none()
        || function.is_async
        || function.is_generator
        || function.is_class_constructor
        || function.derived_constructor
        || !function.handlers.is_empty()
        || !function.simple_parameters
        || function.rest
    {
        return None;
    }
    let field = function_instruction(function, 0)?;
    if field.op() != Op::GetField
        || field.returns_from_frame()
        || !matches!(
            field.field_lookup(),
            Some(FieldLookup::Atom {
                base: FieldBase(0),
                ..
            })
        )
    {
        return None;
    }

    // Follow the field value through register copies to a strict-inequality
    // branch. Recognition is based on data flow rather than a property name
    // or fixed branch PC, while the fast region stays effect-free.
    let mut field_values = [false; MAX_REGISTERS];
    *field_values.get_mut(usize::from(field.result_register()))? = true;
    let mut branch_pc = None;
    for pc in 1..function.code.len() {
        let instruction = function_instruction(function, pc)?;
        match instruction.op() {
            Op::Move => {
                if instruction.returns_from_frame() {
                    return None;
                }
                let source = usize::from(instruction.register_b());
                let destination = usize::from(instruction.register_a());
                if !field_values.get(source).copied().unwrap_or(false) {
                    return None;
                }
                let destination = field_values.get_mut(destination)?;
                *destination = true;
            }
            Op::JumpBinaryFalse => {
                branch_pc = Some(pc);
                break;
            }
            _ => return None,
        }
    }
    let branch_pc = branch_pc?;
    let branch = function_instruction(function, branch_pc)?;
    let operands = [branch.operand_b(), branch.operand_c()];
    let field_is_compared = operands.iter().any(|operand| {
        operand.kind() == Some(OperandKind::Register)
            && field_values
                .get(usize::from(operand.payload()))
                .copied()
                .unwrap_or(false)
    });
    let has_constant = operands
        .iter()
        .any(|operand| operand.kind() == Some(OperandKind::Constant));
    let constant_pc = branch_pc.checked_add(1)?;
    let continuation_pc = constant_pc.checked_add(1)?;
    let constant = function_instruction(function, constant_pc)?;
    let continuation = function_instruction(function, continuation_pc)?;
    let resume_register = if continuation.op() == Op::Return
        && continuation.register_a() == constant.result_register()
    {
        constant.result_register()
    } else if continuation.op() == Op::Unary
        && continuation.unary_operator() == oxc_ast::ast::UnaryOperator::LogicalNot as u32
        && continuation.returns_from_frame()
        && constant.result_register() == continuation.register_b()
    {
        continuation.register_b()
    } else {
        return None;
    };
    let target = usize::try_from(branch.jump_target()).ok()?;
    if branch.binary_operator_field() != BinaryOperator::StrictInequality as u32
        || !field_is_compared
        || !has_constant
        || constant.op() != Op::LoadConst
        || constant.returns_from_frame()
        || target <= continuation_pc
        || target >= function.code.len()
    {
        return None;
    }
    Some(FieldRejectLayout {
        branch_pc,
        constant_pc,
        continuation_pc,
        resume_register: usize::from(resume_register),
    })
}

pub(super) fn is_field_reject_candidate(function: &Function) -> bool {
    let Some(field_argument) = cached_field_argument_candidate(function) else {
        return false;
    };
    field_argument.entry_pc == 0 && field_reject_layout(function).is_some()
}

fn compile_field_reject(
    program: &ResidualProgram,
    function_id: u32,
    function: &Function,
    field_argument: JsJitFieldArgument,
) -> Option<JsJitArtifact> {
    let layout = field_reject_layout(function)?;
    if field_argument.entry_pc != 0 {
        return None;
    }
    let branch = function_instruction(function, layout.branch_pc)?;
    let constant = function_instruction(function, layout.constant_pc)?;
    let comparison_constant = [branch.operand_b(), branch.operand_c()]
        .into_iter()
        .find(|operand| operand.kind() == Some(OperandKind::Constant))?;
    let Some(Constant::Number(comparison_value)) = program
        .constants
        .get(usize::from(comparison_constant.payload()))
    else {
        return None;
    };
    let resume_value = match program.constants.get(constant.constant_index())? {
        Constant::Number(value) => JsJitResumeValue::Number(*value),
        Constant::Boolean(value) => JsJitResumeValue::Boolean(*value),
        _ => return None,
    };
    let resume_pc = u32::try_from(layout.continuation_pc).ok()?;
    let resume_register = u16::try_from(layout.resume_register).ok()?;
    let isa = super::jit_backend::target_isa()?;
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(AbiParam::new(isa.pointer_type()));
    signature.returns.push(AbiParam::new(types::I32));
    let mut native =
        ir::Function::with_name_signature(UserFuncName::user(0x5155_454e, function_id), signature);
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let arguments = builder.block_params(entry)[0];
    let value = builder
        .ins()
        .load(types::F64, ir::MemFlagsData::trusted(), arguments, 0);
    let expected = builder
        .ins()
        .f64const(Ieee64::with_bits(comparison_value.to_bits()));
    let mismatch = builder.ins().fcmp(FloatCC::NotEqual, value, expected);
    let fast = builder.ins().iconst(types::I32, 1);
    let fallback = builder.ins().iconst(types::I32, 0);
    let outcome = builder.ins().select(mismatch, fast, fallback);
    builder.ins().return_(&[outcome]);
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
        requires_signed_int32_arguments: false,
        result: JsJitResultKind::FieldReject {
            resume_pc,
            resume_register,
            resume_value,
        },
        field_argument: Some(field_argument),
        numeric_field_inputs: Vec::new(),
        capture_arguments: Vec::new(),
        general_dispatch_native_pcs: Vec::new(),
    })
}

fn wide_boolean_tree_candidate(function: &Function) -> bool {
    let mut has_forward_branch = false;
    function.code.iter().enumerate().all(|(pc, instruction)| {
        let op = instruction.as_wide().op();
        has_forward_branch |= op.is_js_conditional_jump();
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
            && (!op.is_js_jump() || usize::try_from(instruction.as_wide().jump_target())
                .is_ok_and(|target| target > pc))
    }) && has_forward_branch
}

pub(super) fn eligible(function: &Function, program: &crate::bytecode::ResidualProgram) -> bool {
    // An arrow's `this` is its environment's, which this tier does not bind.
    if function.parent.is_none()
        || function.is_arrow
        || function.is_async
        || function.is_generator
        || function.is_class_constructor
        || function.derived_constructor
        || function.class_initializer
        || function.instance_initializer.is_some()
        || function.rest
        || !function.simple_parameters
        || !function.handlers.is_empty()
        || (!function.plain_activation(program)
            && !activation_is_irrelevant(function)
            && numeric_capture_candidate(function).is_none()
            && !cached_numeric_field_inputs_candidate(function).is_some_and(|inputs| {
                inputs.iter().any(|input| {
                    matches!(input.receiver, JsJitFieldReceiver::Capture { .. })
                })
            }))
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

/// Some closures need a non-plain environment only because their source
/// nesting supplies one; a pure numeric body that never reads it has no
/// observable dependency on that activation. Keep this proof deliberately
/// narrow so calls, captures, dynamic names, `this`, and closure creation stay
/// on the interpreter path.
fn activation_is_irrelevant(function: &Function) -> bool {
    function.arguments_slot.is_none()
        && !function.inherited_with_scope
        && function.wide.is_empty()
        && function.code.iter().all(|packed| {
            matches!(
                packed.as_wide().op(),
                Op::LoadConst
                    | Op::Move
                    | Op::Unary
                    | Op::Binary
                    | Op::NumericAdd
                    | Op::NumericMultiply
                    | Op::LoadLocal
                    | Op::LoadLocalPlain
                    | Op::StoreLocal
                    | Op::StoreLocalPlain
                    | Op::Jump
                    | Op::JumpFalse
                    | Op::JumpTrue
                    | Op::JumpBinaryFalse
                    | Op::Return
            )
        })
}

/// What the tier finds in a function at its entries, before it has a compiled form: the shapes
/// its candidate filters recognize. Each depends only on the function and its own program,
/// which never change, so it is derived once per function (`ProgramStore::js_entry_facts`);
/// re-deriving them at every entry of a function the tier watches had made JIT runs execute
/// more instructions than interpreted ones (semver npm scenario +4.6%).
#[derive(Default)]
pub(super) struct JsEntryFacts {
    pub(super) captured_store: Option<CapturedFieldStore>,
    pub(super) numeric_loop: bool,
    pub(super) int32_bitwise: bool,
    pub(super) int32_identity: bool,
    pub(super) number_capture: Option<JsJitCaptureArgument>,
    pub(super) numeric_capture: Option<smallvec::SmallVec<[JsJitCaptureArgument; MAX_CAPTURE_ARGUMENTS]>>,
    pub(super) field_argument: Option<JsJitFieldArgument>,
    pub(super) numeric_field_argument: Option<JsJitFieldArgument>,
    pub(super) numeric_field_inputs:
        Option<smallvec::SmallVec<[JsJitNumericFieldInput; MAX_NUMERIC_FIELD_INPUTS]>>,
    pub(super) field_reject: bool,
    /// Whether an entry may run native code (`Vm::js_jit_at_entry`): the function is a tier
    /// candidate, or one of the entry patterns that run without a tier matched it.
    pub(super) enters: bool,
    /// Whether the function is a specialized tier candidate (`function_candidate`).
    pub(super) candidate: bool,
    /// Calls whose cost a frameless unit can remove by specializing their targets.
    pub(super) has_calls: bool,
    /// Whether the general baseline admits it, and without backward jumps
    /// (`js_general_dispatch_shape`).
    pub(super) general: bool,
    pub(super) general_acyclic: bool,
}

impl JsEntryFacts {
    /// Whether every fact is unset (`Self::default`): a function that does not enter
    /// (`enters` false, so neither a candidate) keeps only its general-baseline shape
    /// (`of`), and `general_acyclic` implies `general`.
    pub(super) const fn is_unset(&self) -> bool {
        !self.enters && !self.general
    }

    pub(super) fn of(program: &ResidualProgram, function: &Function, priced: bool) -> Self {
        let captured_store = captured_field_store_candidate(program, function);
        let has_calls = function.instructions().any(|instruction|
            instruction.op().control_flow_layout() == crate::bytecode::ControlFlowLayout::Call);
        let candidate = function_candidate(function, program) && (!priced || has_calls || super::compiled::pays_native_entry(function));
        let shape = super::compiled::js_general_dispatch_shape(function);
        let (general, general_acyclic) = (shape.is_some(), shape == Some(true));
        let enters = candidate || captured_store.is_some();
        // The other facts are read only past the candidate check an entering function passes.
        if !enters {
            return Self { candidate, has_calls, general, general_acyclic, ..Self::default() };
        }
        Self {
            captured_store,
            enters,
            candidate,
            has_calls,
            general,
            general_acyclic,
            numeric_loop: has_numeric_loop_candidate(function),
            int32_bitwise: has_int32_bitwise_candidate(program, function),
            int32_identity: int32_identity_return_candidate(program, function).is_some(),
            number_capture: number_capture_candidate(function),
            numeric_capture: numeric_capture_candidate(function),
            field_argument: cached_field_argument_candidate(function),
            numeric_field_argument: cached_numeric_field_argument_candidate(function),
            numeric_field_inputs: cached_numeric_field_inputs_candidate(function),
            field_reject: is_field_reject_candidate(function),
        }
    }
}
