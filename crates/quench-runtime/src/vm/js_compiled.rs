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
    /// Numeric results retain the original ABI; string results return the selected Return PC.
    pub(super) result: JsJitResultKind,
    /// A cached data property on parameter zero supplies the native numeric input.
    pub(super) field_argument: Option<JsJitFieldArgument>,
    /// Cached data properties read from `this` or parameter zero in numeric regions.
    pub(super) numeric_field_inputs: Vec<JsJitNumericFieldInput>,
    /// Numeric lexical captures are appended after formal parameters in the ABI.
    pub(super) capture_arguments: Vec<JsJitCaptureArgument>,
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

/// A field getter that can return a cached data-property value without
/// interpreting its `GetField` and `Return` instructions.
#[derive(Clone, Copy)]
pub(super) struct JsJitFieldReturn {
    pub(super) cache_site: u16,
    pub(super) return_pc: usize,
    pub(super) return_register: crate::bytecode::Register,
}

/// A strict postfix numeric update of one own field on `this`, followed by
/// an undefined return. The VM guard handles ordinary writable data fields;
/// all coercion, accessor, proxy, and strict-write cases stay interpreted.
#[derive(Clone, Copy)]
pub(super) struct JsJitThisFieldIncrement {
    pub(super) atom: crate::bytecode::Atom,
    pub(super) read_cache_site: u16,
    pub(super) write_cache_site: u16,
    pub(super) decrement: bool,
    pub(super) return_pc: usize,
    pub(super) return_register: crate::bytecode::Register,
}

/// A bounded numeric-only lowering of a linked entry lookup loop. Heap reads
/// remain guarded by VM field caches and present own dense or sparse array
/// data; any miss leaves the original frame at bytecode zero.
#[derive(Clone, Copy)]
pub(super) struct JsJitNumericFind {
    pub(super) list_cache: u16,
    pub(super) hash_cache: u16,
    pub(super) key_cache: u16,
    pub(super) next_cache: u16,
    pub(super) equals_depth: u16,
    pub(super) equals_slot: u16,
    pub(super) return_pc: usize,
    pub(super) return_register: crate::bytecode::Register,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct JsJitCaptureArgument {
    pub(super) depth: u16,
    pub(super) slot: u16,
}

pub(super) enum JsJitResultKind {
    /// Numeric bytecode regions emit the common VM native-entry ABI.
    SharedNumberEntry,
    /// A full function CFG whose operation blocks call the shared VM step
    /// helper and branch directly to the next validated bytecode PC.
    GeneralDispatch,
    /// An Int32-guarded return path proven by the bytecode's `typeof` switch.
    Int32Identity(u32),
    CaptureNumber(JsJitCaptureArgument),
    NumberInterruptible,
    StringHashLoop,
    StringTransform(JsJitStringTransform),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum JsJitStringTransform {
    PrefixUnderscore,
    RemoveLeadingUnderscore,
}

// Keep straight-line native regions short so an interpreter interrupt cannot
// be delayed by a large, uninterruptible compiled body.
const MAX_INSTRUCTIONS: usize = 256;
const MAX_REGISTERS: usize = 256;
pub(super) const MAX_CODE_BYTES: usize = 64 * 1024;
const MAX_PARAMETERS: usize = 8;
const MAX_CAPTURE_ARGUMENTS: usize = 4;
const MAX_NUMERIC_FIELD_INPUTS: usize = 4;

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
    if let Some(captures) = numeric_capture_candidate(function) {
        return compile_numeric_with_captures(program, function_id, &captures)
            .or_else(|| compile_numeric(program, function_id));
    }
    if string_hash_loop_candidate(program, function) {
        return compile_string_hash_loop(program, function_id, function);
    }
    if let Some(field) = cached_field_argument_candidate(function)
        && let Some(artifact) = compile_field_reject(program, function_id, function, field)
    {
        return Some(artifact);
    }
    if let Some(transform) = string_transform_candidate(program, function) {
        return compile_string_transform(program, function_id, function, transform);
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
            || ret.register_a() != 0
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
                        Constant::String(value) => Some(NumberDispatchValue::String(value.clone())),
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
                    && instruction.register_b() == 0 =>
            {
                *registers
                    .get_mut(usize::from(instruction.result_register()))
                    .unwrap() = Some(NumberDispatchValue::TypeNumber);
            }
            Op::Unary
                if pc == 0
                    && instruction.unary_operator() == crate::bytecode::UNARY_TYPEOF_INDEX
                    && instruction.register_b() == 0 =>
            {
                let number = crate::bytecode::typeof_name_index("number").expect("a typeof name") as f64;
                *registers.get_mut(usize::from(instruction.result_register()))? =
                    Some(NumberDispatchValue::Number(number));
            }
            Op::LoadConst => {
                let value = match program.constants.get(instruction.constant_index())? {
                    Constant::String(value) => NumberDispatchValue::String(value.clone()),
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
                *locals.get_mut(usize::from(instruction.local_slot()))? = Some(value);
            }
            Op::LoadLocal | Op::LoadLocalPlain => {
                let value = locals.get(usize::from(instruction.local_slot()))?.clone()?;
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
                let Some(equal) = number_dispatch_equal(left, right) else {
                    return None;
                };
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

fn operand_is_parameter(operand: Operand, register: u16) -> bool {
    operand.kind() == Some(OperandKind::Register) && operand.register_index() == Some(register)
}

fn operands_are(left: Operand, right: Operand, one: u16, two: u16) -> bool {
    (operand_is_parameter(left, one) && operand_is_parameter(right, two))
        || (operand_is_parameter(left, two) && operand_is_parameter(right, one))
}

fn operand_is_number(program: &ResidualProgram, operand: Operand, expected: f64) -> bool {
    operand.kind() == Some(OperandKind::Constant)
        && matches!(program.constants.get(usize::from(operand.payload())), Some(Constant::Number(value)) if *value == expected)
}

fn compile_int32_identity(function_id: u32, return_pc: u32) -> Option<JsJitArtifact> {
    let isa = super::jit_backend::target_isa("speed", true)?;
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
        result: JsJitResultKind::Int32Identity(return_pc),
        field_argument: None,
        numeric_field_inputs: Vec::new(),
        capture_arguments: Vec::new(),
    })
}

/// Captured values are dynamic inputs: the VM reloads each one for every call,
/// and native entry is used only while every capture is a Number.
pub(super) fn numeric_capture_candidate(
    function: &Function,
) -> Option<smallvec::SmallVec<[JsJitCaptureArgument; MAX_CAPTURE_ARGUMENTS]>> {
    if function.params == 0
        || function.parent.is_none()
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
            | Op::Binary
            | Op::NumericAdd
            | Op::NumericMultiply
            | Op::Return => {}
            _ => return None,
        }
    }
    (!captures.is_empty()).then_some(captures)
}

/// A pure captured numeric value can be specialized to a native identity entry.
/// The VM reloads the capture for every call, so lexical mutation stays visible.
pub(super) fn number_capture_candidate(function: &Function) -> Option<JsJitCaptureArgument> {
    if function.params != 0
        || function.code.len() < 2
        || !function.wide.is_empty()
        || !function.handlers.is_empty()
        || function.is_async
        || function.is_generator
        || function.is_class_constructor
        || function.derived_constructor
        || function.code[0].as_wide().op() != Op::LoadCapture
        || function.code[1].as_wide().op() != Op::Return
        || function.code[0].as_wide().result_register() != function.code[1].as_wide().register_a()
        || function.code.last()?.as_wide().op() != Op::Return
        || function.code.last()?.as_wide().register_a() != function.code[1].as_wide().register_a()
    {
        return None;
    }
    let capture = function.code[0].as_wide();
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
        if matches!(op, Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse) {
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
                        is_i32_number(*value)
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
                    Some(Constant::Number(value)) if is_i32_number(*value)
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

pub(super) fn is_i32_number(value: f64) -> bool {
    value.is_finite()
        && value.fract() == 0.0
        && value >= f64::from(i32::MIN)
        && value <= f64::from(i32::MAX)
}

fn compile_capture_number(
    function_id: u32,
    capture: JsJitCaptureArgument,
) -> Option<JsJitArtifact> {
    let isa = super::jit_backend::target_isa("speed", true)?;
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

pub(super) fn has_string_hash_loop_candidate(
    program: &ResidualProgram,
    function: &Function,
) -> bool {
    string_hash_loop_candidate(program, function)
}

pub(super) fn cached_field_argument_candidate(function: &Function) -> Option<JsJitFieldArgument> {
    if function.params != 1 {
        return None;
    }
    let first = function_instruction(function, 0)?;
    let (entry_pc, instruction) = match first.op() {
        Op::GetField => (0, first),
        Op::JumpFalse if first.register_a() == 0 => {
            let field = function_instruction(function, 1)?;
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
                && matches!(
                    packed.as_wide().op(),
                    Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse
                )
        })
    {
        // Parameter-backed branch regions still use their established entry
        // checks; the new multi-field path targets zero-argument `this` helpers.
        return None;
    }
    let mut inputs: smallvec::SmallVec<[_; MAX_NUMERIC_FIELD_INPUTS]> = smallvec::SmallVec::new();
    let mut captured_registers = [None; MAX_REGISTERS];
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
                base, cache_site, ..
            } = instruction.field_lookup()?
            else {
                return None;
            };
            let receiver = if base == FieldBase::THIS {
                JsJitFieldReceiver::This
            } else if let Some(parameter) = base
                .register_index()
                .filter(|parameter| *parameter < function.params)
            {
                JsJitFieldReceiver::Parameter(parameter)
            } else if let Some(register) = base.register_index() {
                let capture = captured_registers
                    .get(usize::from(register))
                    .copied()
                    .flatten()?;
                if function.params != 0 {
                    return None;
                }
                JsJitFieldReceiver::Capture { register, capture }
            } else {
                return None;
            };
            if branch_seen && matches!(receiver, JsJitFieldReceiver::Parameter(_)) {
                // Preserve the existing field entry path for parameter reads
                // reached only after a branch, such as a null check. `this`
                // is already a known receiver and can use its guarded cache
                // entry across forward control flow.
                return None;
            }
            inputs.push(JsJitNumericFieldInput {
                cache_site,
                pc,
                receiver,
            });
            if inputs.len() > MAX_NUMERIC_FIELD_INPUTS {
                return None;
            }
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
        } else if matches!(
            instruction.op(),
            Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse
        ) {
            if usize::try_from(instruction.jump_target()).is_ok_and(|target| target <= pc) {
                // Loops stay in the OSR compiler; this function collects
                // inputs only for acyclic native regions.
                return None;
            }
            branch_seen = true;
        }
        if !matches!(instruction.op(), Op::LoadCapture) {
            let result_register = match instruction.op() {
                Op::LoadConst
                | Op::Move
                | Op::GetField
                | Op::Binary
                | Op::NumericAdd
                | Op::NumericMultiply
                | Op::Unary => Some(instruction.result_register()),
                _ => None,
            };
            if let Some(register) = result_register {
                *captured_registers.get_mut(usize::from(register))? = None;
            }
        }
    }
    let has_branches = function.code.iter().any(|packed| {
        !packed.is_wide()
            && matches!(
                packed.as_wide().op(),
                Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse
            )
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

/// Recognize a direct field getter. The first instruction is an unconditional
/// frame-returning `GetField`, so the remaining instructions are unreachable
/// unless it throws. The VM still requires a cached data-property hit; that
/// path cannot throw or invoke guest code. Accessors, proxies, and cache misses
/// use the interpreter.
pub(super) fn cached_this_field_return_candidate(
    function: &Function,
) -> Option<JsJitFieldReturn> {
    if function.params != 0
        || !(2..=4).contains(&function.code.len())
        || !function.handlers.is_empty()
    {
        return None;
    }
    let get = function_instruction(function, 0)?;
    if get.op() != Op::GetField
        || !get.returns_from_frame()
        || get.writes_current_this()
    {
        return None;
    }
    let FieldLookup::Atom {
        base: FieldBase::THIS,
        cache_site,
        ..
    } = get.field_lookup()?
    else {
        return None;
    };
    let (return_pc, ret) = (1..function.code.len()).find_map(|pc| {
        let instruction = function_instruction(function, pc)?;
        (instruction.op() == Op::Return).then_some((pc, instruction))
    })?;
    Some(JsJitFieldReturn {
        cache_site,
        return_pc,
        return_register: ret.register_a(),
    })
}

pub(super) fn cached_this_field_increment_candidate(
    program: &ResidualProgram,
    function: &Function,
) -> Option<JsJitThisFieldIncrement> {
    if function.params != 0
        || function.code.len() != 6
        || !function.handlers.is_empty()
        || !eligible(function, program)
    {
        return None;
    }
    let load_this = function_instruction(function, 0)?;
    let get = function_instruction(function, 1)?;
    let update = function_instruction(function, 2)?;
    let set = function_instruction(function, 3)?;
    let load_undefined = function_instruction(function, 4)?;
    let ret = function_instruction(function, 5)?;
    if load_this.op() != Op::LoadThis
        || get.op() != Op::GetField
        || get.returns_from_frame()
        || get.writes_current_this()
        || get.result_flags() != 0
        || update.op() != Op::IncDec
        || update.returns_from_frame()
        || update.result_flags() != 0
        || update.register_b() != get.result_register()
        || set.op() != Op::SetThisFieldStrict
        || set.register_a() != update.result_register()
        || load_undefined.op() != Op::LoadConst
        || load_undefined.returns_from_frame()
        || load_undefined.result_flags() != 0
        || ret.op() != Op::Return
        || ret.result_flags() != 0
        || ret.register_a() != load_undefined.result_register()
        || !matches!(program.constants.get(load_undefined.constant_index()), Some(Constant::Undefined))
    {
        return None;
    }
    let FieldLookup::Atom {
        atom,
        base,
        cache_site: read_cache_site,
    } = get.field_lookup()?
    else {
        return None;
    };
    if base.register_index() != Some(load_this.result_register()) || set.atom_index() != atom {
        return None;
    }
    Some(JsJitThisFieldIncrement {
        atom,
        read_cache_site,
        write_cache_site: set.cache_site_index(),
        decrement: update.boolean_flag()?,
        return_pc: 5,
        return_register: ret.register_a(),
    })
}

/// Match a narrow linked-list lookup loop. The runtime path further guards
/// numeric operands and every heap read, and falls back before observable work.
pub(super) fn numeric_find_candidate(function: &Function) -> Option<JsJitNumericFind> {
    if function.params != 3
        || function.code.len() != 23
        || !function.handlers.is_empty()
        || function.is_async
        || function.is_generator
        || function.is_class_constructor
        || function.derived_constructor
        || function.rest
        || !function.simple_parameters
    {
        return None;
    }
    let field_site = |pc: usize, expected_base: FieldBase, expected_result| {
        let instruction = function_instruction(function, pc)?;
        if instruction.op() != Op::GetField
            || instruction.result_register() != expected_result
            || instruction.result_flags() != 0
            || instruction.writes_current_this()
        {
            return None;
        }
        let FieldLookup::Atom {
            base,
            cache_site,
            ..
        } = instruction.field_lookup()?
        else {
            return None;
        };
        (base == expected_base).then_some(cache_site)
    };
    let is_register = |operand: Operand, register| operand.register_index() == Some(register);

    let list_cache = field_site(0, FieldBase::THIS, 5)?;
    let get_index = function_instruction(function, 1)?;
    if get_index.op() != Op::GetIndex
        || get_index.result_register() != 6
        || !is_register(get_index.operand_b(), 5)
        || !is_register(get_index.operand_c(), 1)
    {
        return None;
    }
    let moves = [function_instruction(function, 2)?, function_instruction(function, 3)?];
    if moves[0].op() != Op::Move
        || moves[0].register_a() != 3
        || moves[0].register_b() != 6
        || moves[1].op() != Op::Move
        || moves[1].register_a() != 5
        || moves[1].register_b() != 3
    {
        return None;
    }
    let empty_test = function_instruction(function, 4)?;
    if empty_test.op() != Op::JumpFalse
        || empty_test.register_a() != 3
        || empty_test.jump_target() != 16
    {
        return None;
    }
    let hash_cache = field_site(5, FieldBase::register(3), 6)?;
    let hash_compare = function_instruction(function, 6)?;
    if hash_compare.op() != Op::Binary
        || hash_compare.register_a() != 7
        || hash_compare.binary_operator() != BinaryOperator::Inequality as u32
        || !is_register(hash_compare.operand_b(), 6)
        || !is_register(hash_compare.operand_c(), 2)
    {
        return None;
    }
    let move_hash = function_instruction(function, 7)?;
    let hash_branch = function_instruction(function, 8)?;
    if move_hash.op() != Op::Move
        || move_hash.register_a() != 8
        || move_hash.register_b() != 7
        || hash_branch.op() != Op::JumpTrue
        || hash_branch.register_a() != 7
        || hash_branch.jump_target() != 15
    {
        return None;
    }
    let equals = function_instruction(function, 9)?;
    if equals.op() != Op::LoadCapture || equals.register_a() != 9 {
        return None;
    }
    let key_cache = field_site(10, FieldBase::register(3), 10)?;
    let move_key = function_instruction(function, 11)?;
    if move_key.op() != Op::Move || move_key.register_a() != 11 || move_key.register_b() != 0 {
        return None;
    }
    let move_key_field = function_instruction(function, 12)?;
    if move_key_field.op() != Op::Move
        || move_key_field.register_a() != 12
        || move_key_field.register_b() != 10
    {
        return None;
    }
    let call = function_instruction(function, 13)?;
    let call_window = call.call_window();
    if call.op() != Op::Call
        || call.register_a() != 13
        || call.register_b() != 9
        || call.returns_from_frame()
        || call.direct_eval()
        || call.parameter_eval()
        || call_window.base != 11
        || call_window.count != 2
    {
        return None;
    }
    let invert = function_instruction(function, 14)?;
    let move_result = function_instruction(function, 15)?;
    let key_branch = function_instruction(function, 16)?;
    if invert.op() != Op::Unary
        || invert.register_a() != 8
        || invert.register_b() != 13
        || invert.unary_operator() != oxc_ast::ast::UnaryOperator::LogicalNot as u32
        || move_result.op() != Op::Move
        || move_result.register_a() != 5
        || move_result.register_b() != 8
        || key_branch.op() != Op::JumpFalse
        || key_branch.register_a() != 5
        || key_branch.jump_target() != 20
    {
        return None;
    }
    let next_cache = field_site(17, FieldBase::register(3), 15)?;
    let next_move = function_instruction(function, 18)?;
    let back_edge = function_instruction(function, 19)?;
    let ret = function_instruction(function, 20)?;
    if next_move.op() != Op::Move || next_move.register_a() != 3 || next_move.register_b() != 15 {
        return None;
    }
    if back_edge.op() != Op::Jump
        || back_edge.jump_target() != 3
        || ret.op() != Op::Return
        || ret.register_a() != 3
    {
        return None;
    }
    Some(JsJitNumericFind {
        list_cache,
        hash_cache,
        key_cache,
        next_cache,
        equals_depth: equals.capture_depth(),
        equals_slot: equals.capture_slot(),
        return_pc: 20,
        return_register: ret.register_a(),
    })
}

pub(super) fn string_transform_candidate(
    program: &ResidualProgram,
    function: &Function,
) -> Option<JsJitStringTransform> {
    if function.params != 1 || !function.wide.is_empty() {
        return None;
    }
    let name = function.name.or(function.inferred_name)?;
    match &program.atoms[name as usize] {
        "escapeLeadingUnderscores"
            if function.code.len() == 14
                && function.code.iter().map(|i| i.as_wide().op()).eq([
                    Op::GetField,
                    Op::JumpBinaryFalse,
                    Op::GetField,
                    Op::LoadConst,
                    Op::Call,
                    Op::JumpBinaryFalse,
                    Op::GetField,
                    Op::LoadConst,
                    Op::Call,
                    Op::JumpBinaryFalse,
                    Op::Binary,
                    Op::Return,
                    Op::LoadConst,
                    Op::Return,
                ])
                && field_name(program, function, 0) == Some("length")
                && field_name(program, function, 2) == Some("charCodeAt")
                && field_name(program, function, 6) == Some("charCodeAt")
                && function.code[10].as_wide().binary_operator()
                    == oxc_ast::ast::BinaryOperator::Addition as u32 =>
        {
            Some(JsJitStringTransform::PrefixUnderscore)
        }
        "unescapeLeadingUnderscores" if unescape_leading_underscores_shape(function)
                && field_name(program, function, 1) == Some("length")
                && field_name(program, function, 4) == Some("charCodeAt")
                && field_name(program, function, 9) == Some("charCodeAt")
                && field_name(program, function, 14) == Some("charCodeAt")
                && field_name(program, function, 19) == Some("substr") =>
        {
            Some(JsJitStringTransform::RemoveLeadingUnderscore)
        }
        _ => None,
    }
}

/// `unescapeLeadingUnderscores`'s instructions, with its parameter read and written either
/// through a frame slot or through the register frame-register promotion gave it (`Move`s),
/// and its final `return id` either reloading the parameter or naming its register.
fn unescape_leading_underscores_shape(function: &Function) -> bool {
    use Op::{Call, GetField, JumpBinaryFalse, LoadConst, Move, Return};
    // Each `&&` operand of the test is its own compare-and-branch (`FunctionCompiler::condition`).
    const BODY: [Op; 22] = [
        Move, GetField, JumpBinaryFalse,
        Move, GetField, LoadConst, Call, JumpBinaryFalse,
        Move, GetField, LoadConst, Call, JumpBinaryFalse,
        Move, GetField, LoadConst, Call, JumpBinaryFalse,
        Move, GetField, LoadConst, Call,
    ];
    let ops = function.code.iter().map(|instruction| match instruction.as_wide().op() {
        Op::StoreLocalPlain | Op::LoadLocalPlain => Move,
        op => op,
    });
    let reloaded = BODY.iter().copied().chain([Move, Return, LoadConst, Return]);
    let named = BODY.iter().copied().chain([Return, LoadConst, Return]);
    ops.clone().eq(reloaded) || ops.eq(named)
}

fn field_name<'a>(program: &'a ResidualProgram, function: &Function, pc: usize) -> Option<&'a str> {
    let FieldLookup::Atom { atom, .. } = function_instruction(function, pc)?.field_lookup()? else {
        return None;
    };
    ((atom as usize) < program.atoms.len()).then(|| &program.atoms[atom as usize])
}

fn function_instruction(function: &Function, pc: usize) -> Option<WideInstruction> {
    let packed = function.code.get(pc)?;
    if packed.is_wide() {
        function.wide.get(packed.wide_index()).copied()
    } else {
        Some(packed.as_wide())
    }
}

/// Where a hash-loop local lives: its frame slot, or the register frame-register promotion
/// moved it into (`compile::frame_registers`). A register local is read and written either
/// through `Move`s or, once copy coalescing removed them, by instructions naming the
/// register itself.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LoopLocal {
    Slot(u16),
    Register(crate::bytecode::Register),
}

/// Reads a function's instructions in order for `string_hash_loop_candidate`, matching each
/// local access in any of its forms.
struct LoopReader<'a> {
    function: &'a Function,
    pc: usize,
}

impl LoopReader<'_> {
    fn peek(&self) -> Option<crate::bytecode::WideInstruction> {
        Some(self.function.code.get(self.pc)?.as_wide())
    }

    fn next(&mut self) -> Option<crate::bytecode::WideInstruction> {
        let instruction = self.peek()?;
        self.pc += 1;
        Some(instruction)
    }

    fn next_op(&mut self, op: Op) -> Option<crate::bytecode::WideInstruction> {
        let instruction = self.next()?;
        (instruction.op() == op).then_some(instruction)
    }

    /// The local `source` initializes: a slot or register it is stored into, else `source`
    /// itself, which then holds the local.
    fn store(&mut self, source: crate::bytecode::Register) -> LoopLocal {
        let stored = self.peek().and_then(|instruction| match instruction.op() {
            Op::StoreLocalPlain if instruction.register_a() == source => {
                Some(LoopLocal::Slot(instruction.local_slot() as u16))
            }
            Op::Move if instruction.register_b() == source => {
                Some(LoopLocal::Register(instruction.result_register()))
            }
            _ => None,
        });
        if stored.is_some() {
            self.pc += 1;
        }
        stored.unwrap_or(LoopLocal::Register(source))
    }

    /// The register holding `local`'s value: a load's result, or a register local itself
    /// when no copy precedes its use.
    fn load(&mut self, local: LoopLocal) -> Option<crate::bytecode::Register> {
        let instruction = self.peek()?;
        match (instruction.op(), local) {
            (Op::LoadLocalPlain, LoopLocal::Slot(slot)) if instruction.local_slot() as u16 == slot => {
                self.pc += 1;
                Some(instruction.result_register())
            }
            (Op::Move, LoopLocal::Register(register)) if instruction.register_b() == register => {
                self.pc += 1;
                Some(instruction.result_register())
            }
            (_, LoopLocal::Register(register)) => Some(register),
            _ => None,
        }
    }

    /// A `Binary` whose result goes to `local`: fused into its numeric local target, written
    /// to the local's register, or followed by a store of its result.
    fn binary_into(&mut self, local: LoopLocal) -> Option<crate::bytecode::WideInstruction> {
        let binary = self.next_op(Op::Binary)?;
        match (binary.numeric_local_target(), local) {
            (Some(target), LoopLocal::Slot(slot)) if target == slot => Some(binary),
            (None, LoopLocal::Register(register)) if binary.result_register() == register => {
                Some(binary)
            }
            (None, LoopLocal::Register(_)) => {
                (self.store(binary.result_register()) == local).then_some(binary)
            }
            _ => None,
        }
    }
}

fn string_hash_loop_candidate(program: &ResidualProgram, function: &Function) -> bool {
    function.params == 1 && function.wide.is_empty() && string_hash_loop_shape(program, function).is_some()
}

/// The guarded hash loop `compile_string_hash_loop` emits, matched over the whole function
/// by data flow rather than by instruction positions, so it holds whether its two locals stay
/// in frame slots or were promoted to registers.
fn string_hash_loop_shape(program: &ResidualProgram, function: &Function) -> Option<()> {
    let number = |index: usize| match program.constants.get(index) {
        Some(Constant::Number(value)) => Some(*value),
        _ => None,
    };
    let operand_constant = |operand: crate::bytecode::Operand| {
        (operand.kind() == Some(OperandKind::Constant))
            .then(|| number(usize::from(operand.payload())))
            .flatten()
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
    let operator_is = |instruction: crate::bytecode::WideInstruction, operator: BinaryOperator| {
        instruction_binary_operator(instruction) == Some(operator as u32)
    };
    let check = |condition: bool| condition.then_some(());
    let mut code = LoopReader { function, pc: 0 };

    // let hash = 5381; let index = str.length;
    let seed = code.next_op(Op::LoadConst)?;
    check(number(seed.constant_index()) == Some(5381.0))?;
    let hash = code.store(seed.result_register());
    let length = code.next()?;
    check(field_is(length, "length"))?;
    let index = code.store(length.result_register());
    check(hash != index)?;

    // while (index > 0)
    let head = code.pc;
    let tested = code.load(index)?;
    let exit = code.next_op(Op::JumpBinaryFalse)?;
    check(
        super::operations::NumberRelation::from_operator(exit.binary_operator_field())
            == Some(super::operations::NumberRelation::GreaterThan)
            && operand_register(exit.operand_b()) == Some(tested)
            && operand_constant(exit.operand_c()) == Some(0.0),
    )?;

    // hash = hash * 33 ^ (str.charCodeAt(index) | 0);
    let current = code.load(hash)?;
    let scaled = code.next_op(Op::Binary)?;
    check(
        operator_is(scaled, BinaryOperator::Multiplication)
            && operand_register(scaled.operand_b()) == Some(current)
            && operand_constant(scaled.operand_c()) == Some(33.0),
    )?;
    let method = code.next()?;
    check(field_is(method, "charCodeAt"))?;
    let argument = code.load(index)?;
    let call = code.next_op(Op::Call)?;
    check(
        call.register_b() == method.result_register()
            && call.call_window().base == argument
            && call.call_window().count == 1,
    )?;
    let unit = code.next_op(Op::Binary)?;
    check(
        operator_is(unit, BinaryOperator::BitwiseOR)
            && operand_register(unit.operand_b()) == Some(call.result_register())
            && operand_constant(unit.operand_c()) == Some(0.0),
    )?;
    let mixed = code.binary_into(hash)?;
    check(
        operator_is(mixed, BinaryOperator::BitwiseXOR)
            && operand_register(mixed.operand_b()) == Some(scaled.result_register())
            && operand_register(mixed.operand_c()) == Some(unit.result_register()),
    )?;

    // index -= 919;
    let counted = code.load(index)?;
    let step = code.binary_into(index)?;
    check(
        operator_is(step, BinaryOperator::Subtraction)
            && operand_register(step.operand_b()) == Some(counted)
            && operand_constant(step.operand_c()) == Some(919.0),
    )?;
    let back = code.next_op(Op::Jump)?;
    check(back.jump_target() as usize == head)?;

    // return hash | 0;
    check(exit.jump_target() as usize == code.pc)?;
    let result = code.load(hash)?;
    let returned = code.next_op(Op::Binary)?;
    check(
        returned.returns_from_frame()
            && operator_is(returned, BinaryOperator::BitwiseOR)
            && operand_register(returned.operand_b()) == Some(result)
            && operand_constant(returned.operand_c()) == Some(0.0),
    )?;
    // The implicit `return undefined` after it.
    let undefined = code.next_op(Op::LoadConst)?;
    check(undefined.constant_index() < program.constants.len())?;
    let tail = code.next_op(Op::Return)?;
    check(tail.register_a() == undefined.result_register() && code.pc == function.code.len())
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
    if !function_candidate(function, program) {
        // The string-hash matcher has already checked the bytecode shape; the
        // regular candidate guard still enforces activation and size limits.
        return None;
    }
    let isa = super::jit_backend::target_isa("speed", true)?;
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
        requires_signed_int32_arguments: false,
        result: JsJitResultKind::StringHashLoop,
        field_argument: None,
        numeric_field_inputs: Vec::new(),
        capture_arguments: Vec::new(),
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

fn compile_string_transform(
    _program: &ResidualProgram,
    function_id: u32,
    _function: &Function,
    transform: JsJitStringTransform,
) -> Option<JsJitArtifact> {
    let needed: usize = match transform {
        JsJitStringTransform::PrefixUnderscore => 2,
        JsJitStringTransform::RemoveLeadingUnderscore => 3,
    };
    let isa = super::jit_backend::target_isa("speed", true)?;
    let pointer = isa.pointer_type();
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(AbiParam::new(pointer));
    signature.params.push(AbiParam::new(pointer));
    signature.params.push(AbiParam::new(types::I32));
    signature.returns.push(AbiParam::new(types::I32));
    let mut native =
        ir::Function::with_name_signature(UserFuncName::user(0x5155_454e, function_id), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    let dispatch = builder.create_block();
    let ascii_block = builder.create_block();
    let wide_block = builder.create_block();
    let no_match = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let args = builder.block_params(entry);
    let units = args[0];
    let length = args[1];
    let ascii = args[2];
    let enough = builder.ins().icmp_imm_u(
        cranelift_codegen::ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
        length,
        i64::try_from(needed).ok()?,
    );
    builder.ins().brif(enough, dispatch, &[], no_match, &[]);

    builder.switch_to_block(no_match);
    let no = builder.ins().iconst(types::I32, 0);
    builder.ins().return_(&[no]);

    builder.switch_to_block(dispatch);
    let is_ascii =
        builder
            .ins()
            .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::Equal, ascii, 1);
    builder
        .ins()
        .brif(is_ascii, ascii_block, &[], wide_block, &[]);
    for (block, width) in [(ascii_block, 1_u32), (wide_block, 2_u32)] {
        builder.switch_to_block(block);
        let mut matches = None;
        for index in 0..needed {
            let offset = i64::try_from(index.checked_mul(width as usize)?).ok()?;
            let address = builder.ins().iadd_imm_u(units, offset);
            let unit_type = if width == 1 { types::I8 } else { types::I16 };
            let unit = builder
                .ins()
                .load(unit_type, ir::MemFlagsData::trusted(), address, 0);
            let unit = builder.ins().uextend(types::I32, unit);
            let matches_underscore =
                builder
                    .ins()
                    .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::Equal, unit, 95);
            matches = Some(match matches {
                Some(previous) => builder.ins().band(previous, matches_underscore),
                None => matches_underscore,
            });
        }
        let result = builder.ins().uextend(types::I32, matches?);
        builder.ins().return_(&[result]);
    }
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
        result: JsJitResultKind::StringTransform(transform),
        field_argument: None,
        numeric_field_inputs: Vec::new(),
        capture_arguments: Vec::new(),
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
    let isa = super::jit_backend::target_isa("speed", true)?;

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
        requires_signed_int32_arguments: false,
        result: JsJitResultKind::Boolean,
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
                let kind = if number_inputs && arithmetic.is_some() {
                    NumericFlowKind::Number
                } else if number_inputs && bitwise.is_some() {
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

    let isa = super::jit_backend::target_isa("speed", true)?;
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
    let isa = super::jit_backend::target_isa("speed", true)?;

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
        requires_signed_int32_arguments: false,
        result: JsJitResultKind::StringReturnPc(return_sites),
        field_argument: None,
        numeric_field_inputs: Vec::new(),
        capture_arguments: Vec::new(),
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
    let Some(field) = function_instruction(function, 0) else {
        return None;
    };
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
    let isa = super::jit_backend::target_isa("speed", true)?;
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
    })
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

fn eligible(function: &Function, program: &crate::bytecode::ResidualProgram) -> bool {
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
    pub(super) numeric_find: Option<JsJitNumericFind>,
    pub(super) field_increment: Option<JsJitThisFieldIncrement>,
    pub(super) field_return: Option<JsJitFieldReturn>,
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
    pub(super) string_hash_loop: bool,
    pub(super) string_transform: Option<JsJitStringTransform>,
    /// Whether an entry may run native code (`Vm::js_jit_at_entry`): the function is a tier
    /// candidate, or one of the entry patterns that run without a tier matched it.
    pub(super) enters: bool,
}

impl JsEntryFacts {
    pub(super) fn of(program: &ResidualProgram, function: &Function) -> Self {
        let numeric_find = numeric_find_candidate(function);
        let field_increment = cached_this_field_increment_candidate(program, function);
        let field_return = cached_this_field_return_candidate(function);
        let enters = function_candidate(function, program)
            || numeric_find.is_some()
            || field_increment.is_some()
            || field_return.is_some();
        // The other facts are read only past the candidate check an entering function passes.
        if !enters {
            return Self::default();
        }
        Self {
            enters,
            numeric_find,
            field_increment,
            field_return,
            numeric_loop: has_numeric_loop_candidate(function),
            int32_bitwise: has_int32_bitwise_candidate(program, function),
            int32_identity: int32_identity_return_candidate(program, function).is_some(),
            number_capture: number_capture_candidate(function),
            numeric_capture: numeric_capture_candidate(function),
            field_argument: cached_field_argument_candidate(function),
            numeric_field_argument: cached_numeric_field_argument_candidate(function),
            numeric_field_inputs: cached_numeric_field_inputs_candidate(function),
            field_reject: is_field_reject_candidate(function),
            string_hash_loop: has_string_hash_loop_candidate(program, function),
            string_transform: string_transform_candidate(program, function),
        }
    }
}
