//! Cranelift-backed compiled tiers for the shared VM.
//!
//! In Ershov's terms the lane's per-instruction semantics have two readings.
//! The lane handlers interpret them; this module is their generating
//! extension: for each instruction it emits the same meaning as Cranelift IR,
//! with every operand the residual already fixes (register numbers,
//! immediates, constants, jump targets, callees) folded in. Running it over a
//! function's residual bytecode is Futamura's first projection, the
//! interpreter specialized to that function; what specialization removes is
//! exactly what the program fixed: dispatch, operand decoding, value tags
//! (i32 and i64 stay unboxed), the register file (registers become SSA
//! variables) and the per-run memory-view resolution.
//!
//! Each operator keeps one semantic authority. Integer operators come from
//! the integer, float and conversion selectors of `crate::wasm` (an i32
//! operator is its i64 namesake at the narrower width), memory accesses from
//! `MemoryLoad`/`MemoryStore`, traps from `WasmTrap`. A function compiles only
//! when every instruction it can reach has a compiled denotation and every
//! function it calls compiles; anything else stays interpreted, so the mode
//! never changes behavior, only speed. Compiled code never allocates or
//! collects itself and holds no heap roots: values cross its boundary in
//! their Wasm encoding, a trap leaves as a status that becomes the
//! interpreter's `WasmTrap`, and an imported call is a callout through which
//! the VM makes the call exactly as the interpreter would (`import_callout`).

use super::*;
use crate::bytecode::{
    ControlFlowLayout, FieldLookup, Function, JsDenotation, Operand, OperandKind, RETURN_REGISTER,
    WideInstruction,
};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{
    F32BinaryOperator, F32UnaryOperator, F64BinaryOperator, F64UnaryOperator,
};
use crate::wasm::integer::{
    I32BinaryOperator, I32UnaryOperator, I64BinaryOperator, I64UnaryOperator,
};
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, MemFlagsData, Signature, UserFuncName,
    condcodes::{FloatCC, IntCC},
    immediates::{Ieee32, Ieee64},
    types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use std::cell::UnsafeCell;
use std::rc::{Rc, Weak};

#[path = "compiled/osr_loop.rs"]
mod osr_loop;
pub(super) use osr_loop::{
    FieldReceiver, LoopEntry, LoopOutcome, LoopRegion, Observation, RegionRejection, Widening,
};

#[derive(Clone, Copy)]
pub(super) enum NumberRelationDomain {
    Int32,
    Float64,
}

/// The shared word written by a compiled region when it hands execution back to the lane.
/// Region-specific metadata stays in the owning tier; this layout carries only the handoff
/// kind and its PC or entry-miss payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RegionExit {
    EntryMiss(u32),
    Resume(u32),
    Fallback(u32),
    Recompile(u32),
}

impl RegionExit {
    const KIND_SHIFT: u32 = u32::BITS;
    const PAYLOAD_MASK: u64 = u32::MAX as u64;

    pub(super) const fn encode(self) -> u64 {
        let (kind, payload) = match self {
            Self::EntryMiss(payload) => (0, payload),
            Self::Resume(pc) => (1, pc),
            Self::Fallback(pc) => (2, pc),
            Self::Recompile(pc) => (3, pc),
        };
        ((kind as u64) << Self::KIND_SHIFT) | payload as u64
    }

    pub(super) const fn decode(word: u64) -> Self {
        let payload = (word & Self::PAYLOAD_MASK) as u32;
        match word >> Self::KIND_SHIFT {
            1 => Self::Resume(payload),
            2 => Self::Fallback(payload),
            3 => Self::Recompile(payload),
            _ => Self::EntryMiss(payload),
        }
    }
}

/// Common cold handoff blocks for a compiled region. The tier still supplies its live-state
/// writeback at each exact PC; this type owns only the shared control-flow continuation.
#[derive(Default)]
pub(super) struct RegionExitBlocks {
    leave_blocks: std::collections::BTreeMap<u32, ir::Block>,
    outcomes: Vec<(ir::Block, RegionExit)>,
}

impl RegionExitBlocks {
    /// Queues a cold outcome block for filling after the hot region has been lowered.
    pub(super) fn outcome_block(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        outcome: RegionExit,
    ) -> ir::Block {
        let block = builder.create_block();
        builder.set_cold_block(block);
        self.outcomes.push((block, outcome));
        block
    }

    /// Sends control to the shared per-PC leave block, carrying the handoff word as an
    /// argument. The tier fills that block with its exact-PC live-state writeback.
    pub(super) fn leave(&mut self, builder: &mut FunctionBuilder<'_>, outcome: RegionExit) {
        let pc = match outcome {
            RegionExit::Resume(pc) | RegionExit::Fallback(pc) | RegionExit::Recompile(pc) => pc,
            RegionExit::EntryMiss(_) => unreachable!("entry miss does not write back"),
        };
        let word = builder.ins().iconst(types::I64, outcome.encode() as i64);
        let block = match self.leave_blocks.get(&pc) {
            Some(block) => *block,
            None => {
                let block = builder.create_block();
                builder.set_cold_block(block);
                builder.append_block_param(block, types::I64);
                self.leave_blocks.insert(pc, block);
                block
            }
        };
        builder.ins().jump(block, &[ir::BlockArg::Value(word)]);
    }

    pub(super) fn take_outcomes(&mut self) -> Vec<(ir::Block, RegionExit)> {
        std::mem::take(&mut self.outcomes)
    }

    pub(super) fn leave_blocks(&self) -> Vec<(u32, ir::Block)> {
        self.leave_blocks
            .iter()
            .map(|(pc, block)| (*pc, *block))
            .collect()
    }
}

/// Emit a Number relation using the same relation dispatch as the interpreter.
/// The domain selects its Cranelift condition codes; the operation mapping is shared.
pub(super) fn emit_number_relation(
    builder: &mut FunctionBuilder<'_>,
    operator: u32,
    left: ir::Value,
    right: ir::Value,
    domain: NumberRelationDomain,
) -> Option<ir::Value> {
    struct Emit<'a, 'f> {
        builder: &'a mut FunctionBuilder<'f>,
        domain: NumberRelationDomain,
    }

    impl crate::vm::operations::NumberRelationStage<ir::Value> for Emit<'_, '_> {
        type Output = ir::Value;

        fn equal(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            match self.domain {
                NumberRelationDomain::Int32 => self.builder.ins().icmp(IntCC::Equal, left, right),
                NumberRelationDomain::Float64 => {
                    self.builder.ins().fcmp(FloatCC::Equal, left, right)
                }
            }
        }
        fn not_equal(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            match self.domain {
                NumberRelationDomain::Int32 => self.builder.ins().icmp(IntCC::NotEqual, left, right),
                NumberRelationDomain::Float64 => {
                    self.builder.ins().fcmp(FloatCC::NotEqual, left, right)
                }
            }
        }
        fn less_than(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            match self.domain {
                NumberRelationDomain::Int32 => {
                    self.builder.ins().icmp(IntCC::SignedLessThan, left, right)
                }
                NumberRelationDomain::Float64 => {
                    self.builder.ins().fcmp(FloatCC::LessThan, left, right)
                }
            }
        }
        fn less_equal(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            match self.domain {
                NumberRelationDomain::Int32 => self
                    .builder
                    .ins()
                    .icmp(IntCC::SignedLessThanOrEqual, left, right),
                NumberRelationDomain::Float64 => self
                    .builder
                    .ins()
                    .fcmp(FloatCC::LessThanOrEqual, left, right),
            }
        }
        fn greater_than(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            match self.domain {
                NumberRelationDomain::Int32 => self
                    .builder
                    .ins()
                    .icmp(IntCC::SignedGreaterThan, left, right),
                NumberRelationDomain::Float64 => {
                    self.builder.ins().fcmp(FloatCC::GreaterThan, left, right)
                }
            }
        }
        fn greater_equal(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            match self.domain {
                NumberRelationDomain::Int32 => self
                    .builder
                    .ins()
                    .icmp(IntCC::SignedGreaterThanOrEqual, left, right),
                NumberRelationDomain::Float64 => self
                    .builder
                    .ins()
                    .fcmp(FloatCC::GreaterThanOrEqual, left, right),
            }
        }
    }

    Some(
        crate::vm::operations::NumberRelation::from_operator(operator)?.apply(
            &mut Emit { builder, domain },
            left,
            right,
        ),
    )
}

pub(super) fn emit_number_arithmetic_operation(
    builder: &mut FunctionBuilder<'_>,
    operation: super::operations::NumberArithmetic,
    left: ir::Value,
    right: ir::Value,
) -> ir::Value {
    struct Emit<'a, 'f> {
        builder: &'a mut FunctionBuilder<'f>,
    }

    impl crate::vm::operations::NumberArithmeticStage<ir::Value> for Emit<'_, '_> {
        type Output = ir::Value;

        fn add(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            self.builder.ins().fadd(left, right)
        }
        fn subtract(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            self.builder.ins().fsub(left, right)
        }
        fn multiply(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            self.builder.ins().fmul(left, right)
        }
        fn divide(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            self.builder.ins().fdiv(left, right)
        }
    }

    operation.apply(&mut Emit { builder }, left, right)
}

pub(super) fn emit_number_unary_operation(
    builder: &mut FunctionBuilder<'_>,
    operation: super::operations::NumberUnary,
    value: ir::Value,
) -> ir::Value {
    struct Emit<'a, 'f> {
        builder: &'a mut FunctionBuilder<'f>,
    }

    impl crate::vm::operations::NumberUnaryStage<ir::Value> for Emit<'_, '_> {
        type Output = ir::Value;

        fn plus(&mut self, value: ir::Value) -> Self::Output {
            value
        }

        fn negate(&mut self, value: ir::Value) -> Self::Output {
            self.builder.ins().fneg(value)
        }
    }

    operation.apply(&mut Emit { builder }, value)
}

fn emit_f64_entry_return(
    builder: &mut FunctionBuilder<'_>,
    result_pointer: ir::Value,
    value: ir::Value,
) {
    builder
        .ins()
        .store(MemFlagsData::trusted(), value, result_pointer, 0);
    let status = builder.ins().iconst(types::I32, 0);
    builder.ins().return_(&[status]);
}

fn read_js_numeric_operand(
    program: &ResidualProgram,
    body: &mut Body<'_>,
    operand: Operand,
    registers: &[bool],
) -> Option<ir::Value> {
    match operand.kind()? {
        OperandKind::Register => {
            let register = operand.register_index()?;
            registers
                .get(usize::from(register))
                .copied()
                .filter(|initialized| *initialized)?;
            Some(body.read(register, crate::WasmType::F64))
        }
        OperandKind::Constant => {
            let crate::bytecode::Constant::Number(value) =
                program.constants.get(usize::from(operand.payload()))?
            else {
                return None;
            };
            Some(
                body.builder
                    .ins()
                    .f64const(Ieee64::with_bits(value.to_bits())),
            )
        }
        OperandKind::Field | OperandKind::Local => None,
    }
}

/// An acyclic bytecode graph and its predecessors. JS numeric regions may
/// merge register state through the shared `Body` SSA variables; local-state
/// branches stay rejected until local slots have an equivalent SSA view.
fn js_numeric_control_flow(function: &Function) -> Option<(Vec<bool>, Vec<Vec<usize>>)> {
    if function.code.is_empty() {
        return None;
    }
    let length = function.code.len();
    let mut reachable = vec![false; length];
    let mut predecessors = vec![Vec::new(); length];
    let mut pending = vec![0_usize];
    while let Some(pc) = pending.pop() {
        if reachable[pc] {
            continue;
        }
        reachable[pc] = true;
        let packed = function.code[pc];
        if packed.is_wide() {
            return None;
        }
        let instruction = packed.as_wide();
        match instruction.op().control_flow_layout() {
            ControlFlowLayout::Jump if instruction.op() == crate::bytecode::Op::Jump => {}
            ControlFlowLayout::ConditionalJump
                if matches!(
                    instruction.op(),
                    crate::bytecode::Op::JumpBinaryFalse
                        | crate::bytecode::Op::JumpFalse
                        | crate::bytecode::Op::JumpTrue
                ) => {}
            ControlFlowLayout::Terminal | ControlFlowLayout::Fallthrough => {}
            ControlFlowLayout::Jump
            | ControlFlowLayout::ConditionalJump
            | ControlFlowLayout::BranchTable
            | ControlFlowLayout::Call => return None,
        }
        let mut successors = Vec::with_capacity(2);
        crate::bytecode::for_each_instruction_successor(pc, instruction, |successor| {
            successors.push(successor);
        });
        for successor in successors {
            if successor <= pc || successor >= length {
                return None;
            }
            predecessors[successor].push(pc);
            pending.push(successor);
        }
    }
    let has_join = (0..length).any(|pc| reachable[pc] && predecessors[pc].len() > 1);
    let has_branch = function.code.iter().enumerate().any(|(pc, packed)| {
        reachable[pc]
            && packed.as_wide().op().control_flow_layout() == ControlFlowLayout::ConditionalJump
    });
    let has_local_state = function.code.iter().enumerate().any(|(pc, packed)| {
        reachable[pc]
            && matches!(
                packed.as_wide().op().js_denotation(),
                JsDenotation::PlainLocalLoad | JsDenotation::PlainLocalStore
            )
    });
    if (has_join || has_branch) && has_local_state {
        return None;
    }
    Some((reachable, predecessors))
}

pub(super) fn emit_number_bitwise_bits(
    builder: &mut FunctionBuilder<'_>,
    operation: crate::vm::operations::NumberBitwise,
    left: ir::Value,
    right: ir::Value,
) -> ir::Value {
    struct Emit<'a, 'f> {
        builder: &'a mut FunctionBuilder<'f>,
    }

    impl crate::vm::operations::NumberBitwiseStage<ir::Value> for Emit<'_, '_> {
        type Output = ir::Value;

        fn and(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            self.builder.ins().band(left, right)
        }
        fn or(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            self.builder.ins().bor(left, right)
        }
        fn xor(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            self.builder.ins().bxor(left, right)
        }
        fn shift_left(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            let shift = self.builder.ins().band_imm_u(right, 31);
            self.builder.ins().ishl(left, shift)
        }
        fn shift_right(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            let shift = self.builder.ins().band_imm_u(right, 31);
            self.builder.ins().sshr(left, shift)
        }
        fn shift_right_unsigned(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
            let shift = self.builder.ins().band_imm_u(right, 31);
            self.builder.ins().ushr(left, shift)
        }
    }

    operation.apply(&mut Emit { builder }, left, right)
}

pub(super) fn emit_number_bitwise_operation(
    builder: &mut FunctionBuilder<'_>,
    operation: crate::vm::operations::NumberBitwise,
    left: ir::Value,
    right: ir::Value,
) -> ir::Value {
    let bits = emit_number_bitwise_bits(builder, operation, left, right);
    if operation == crate::vm::operations::NumberBitwise::ShiftRightUnsigned {
        let unsigned = builder.ins().uextend(types::I64, bits);
        builder.ins().fcvt_from_uint(types::F64, unsigned)
    } else {
        builder.ins().fcvt_from_sint(types::F64, bits)
    }
}

/// Emit JavaScript ToBoolean for a Number after the caller has proved its value is numeric.
fn emit_number_truthiness(builder: &mut FunctionBuilder<'_>, number: ir::Value) -> ir::Value {
    struct Emit<'a, 'f> {
        builder: &'a mut FunctionBuilder<'f>,
    }

    impl crate::vm::operations::NumberTruthinessStage<ir::Value> for Emit<'_, '_> {
        type Predicate = ir::Value;

        fn zero(&mut self) -> ir::Value {
            self.builder
                .ins()
                .f64const(Ieee64::with_bits(0.0_f64.to_bits()))
        }

        fn equal(&mut self, left: ir::Value, right: ir::Value) -> Self::Predicate {
            self.builder.ins().fcmp(FloatCC::Equal, left, right)
        }

        fn not_equal(&mut self, left: ir::Value, right: ir::Value) -> Self::Predicate {
            self.builder.ins().fcmp(FloatCC::NotEqual, left, right)
        }

        fn and(&mut self, left: Self::Predicate, right: Self::Predicate) -> Self::Predicate {
            self.builder.ins().band(left, right)
        }
    }

    crate::vm::operations::number_truthiness(&mut Emit { builder }, number)
}

/// Compile a JS numeric function-entry region with the same register views used by Wasm and
/// OSR regions. JS admission, cache probes and exact fallback remain VM responsibilities; this
/// frontend only receives the already-validated scalar denotations and their entry inputs.
pub(super) fn compile_js_numeric_region(
    program: &ResidualProgram,
    function_id: u32,
    captures: &[super::js_compiled::JsJitCaptureArgument],
    numeric_field_inputs: Option<&[super::js_compiled::JsJitNumericFieldInput]>,
    field_argument: Option<super::js_compiled::JsJitFieldArgument>,
    int32_bitwise: bool,
) -> Option<super::js_compiled::JsJitArtifact> {
    let function = program.function_get(function_id as usize)?;
    let (reachable, predecessors) = js_numeric_control_flow(function)?;
    let params = usize::from(function.params);
    let isa = super::jit_backend::target_isa("speed", true)?;

    let signature = native_entry_signature(isa.pointer_type(), isa.default_call_conv());
    let name = UserFuncName::user(0x5155_454e, function_id);
    let mut native = ir::Function::with_name_signature(name, signature);
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);

    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let blocks: Vec<_> = reachable
        .iter()
        .map(|reachable| reachable.then(|| builder.create_block()))
        .collect();
    let parameters = builder.block_params(entry).to_vec();
    let context = parameters[0];
    let arguments = parameters[3];
    let result_pointer = parameters[4];
    let mut body = Body::new(builder, context, None);
    let mut registers = vec![false; usize::from(function.registers)];
    let mut locals = vec![None; usize::from(function.locals)];

    for (index, initialized) in registers.iter_mut().take(params).enumerate() {
        if numeric_field_inputs.is_some_and(|inputs| {
            inputs.iter().any(|input| {
                input.receiver == super::js_compiled::JsJitFieldReceiver::Parameter(index as u16)
            })
        }) {
            continue;
        }
        let offset = i32::try_from(index.checked_mul(std::mem::size_of::<f64>())?).ok()?;
        let value = body
            .builder
            .ins()
            .load(types::F64, MemFlagsData::trusted(), arguments, offset);
        body.write(index as u16, crate::WasmType::F64, value);
        *initialized = true;
    }
    let entry_registers = registers;
    let mut initialized_after: Vec<Option<Vec<bool>>> = vec![None; function.code.len()];
    body.builder.ins().jump(blocks[0]?, &[]);

    let mut returned = false;
    for (pc, packed) in function.code.iter().copied().enumerate() {
        if !reachable[pc] {
            continue;
        }
        if packed.is_wide() {
            return None;
        }
        let block = blocks[pc]?;
        body.builder.switch_to_block(block);
        body.builder.seal_block(block);
        let mut registers = if pc == 0 {
            entry_registers.clone()
        } else {
            let mut incoming: Option<Vec<bool>> = None;
            for predecessor in &predecessors[pc] {
                let predecessor_state = initialized_after[*predecessor].as_ref()?.clone();
                if let Some(incoming) = &mut incoming {
                    for (initialized, predecessor_initialized) in
                        incoming.iter_mut().zip(predecessor_state)
                    {
                        *initialized &= predecessor_initialized;
                    }
                } else {
                    incoming = Some(predecessor_state);
                }
            }
            incoming?
        };
        let instruction = packed.as_wide();
        match instruction.op().control_flow_layout() {
            ControlFlowLayout::Jump => {
                let target = usize::try_from(instruction.jump_target()).ok()?;
                initialized_after[pc] = Some(registers);
                body.builder.ins().jump(blocks[target]?, &[]);
                continue;
            }
            ControlFlowLayout::ConditionalJump => {
                if instruction.result_flags() != 0 {
                    return None;
                }
                let (condition, jump_when) = match instruction.op() {
                    crate::bytecode::Op::JumpBinaryFalse => {
                        let left = read_js_numeric_operand(
                            program,
                            &mut body,
                            instruction.operand_b(),
                            &registers,
                        )?;
                        let right = read_js_numeric_operand(
                            program,
                            &mut body,
                            instruction.operand_c(),
                            &registers,
                        )?;
                        let relation = emit_number_relation(
                            &mut body.builder,
                            instruction.binary_operator_field(),
                            left,
                            right,
                            NumberRelationDomain::Float64,
                        )?;
                        (relation, false)
                    }
                    crate::bytecode::Op::JumpFalse | crate::bytecode::Op::JumpTrue => {
                        let register = usize::from(instruction.register_a());
                        if !*registers.get(register)? {
                            return None;
                        }
                        let value = body.read(register as u16, crate::WasmType::F64);
                        let truthy = emit_number_truthiness(&mut body.builder, value);
                        let jump_when = instruction.op() == crate::bytecode::Op::JumpTrue;
                        (truthy, jump_when)
                    }
                    _ => return None,
                };
                let target = usize::try_from(instruction.jump_target()).ok()?;
                let fallthrough = pc.checked_add(1)?;
                initialized_after[pc] = Some(registers);
                if jump_when {
                    body.builder
                        .ins()
                        .brif(condition, blocks[target]?, &[], blocks[fallthrough]?, &[]);
                } else {
                    body.builder
                        .ins()
                        .brif(condition, blocks[fallthrough]?, &[], blocks[target]?, &[]);
                }
                continue;
            }
            ControlFlowLayout::Terminal | ControlFlowLayout::Fallthrough => {}
            ControlFlowLayout::Call | ControlFlowLayout::BranchTable => return None,
        }
        let mut terminal = false;
        let result = match instruction.op().js_denotation() {
            JsDenotation::GetField => {
                let inputs = numeric_field_inputs?;
                let (field_index, candidate) = inputs
                    .iter()
                    .enumerate()
                    .find(|(_, candidate)| candidate.pc == pc)?;
                if instruction.returns_from_frame()
                    || instruction.result_flags() != 0
                    || instruction.writes_current_this()
                {
                    return None;
                }
                let expected_base = match candidate.receiver {
                    super::js_compiled::JsJitFieldReceiver::This => FieldBase::THIS,
                    super::js_compiled::JsJitFieldReceiver::Parameter(parameter) => {
                        FieldBase(parameter)
                    }
                    super::js_compiled::JsJitFieldReceiver::Capture { register, .. } => {
                        FieldBase(register)
                    }
                };
                if !matches!(
                    instruction.field_lookup(),
                    Some(FieldLookup::Atom { base, cache_site, .. })
                        if base == expected_base && cache_site == candidate.cache_site
                ) {
                    return None;
                }
                let argument_index = params
                    .checked_add(captures.len())?
                    .checked_add(field_index)?;
                let offset =
                    i32::try_from(argument_index.checked_mul(std::mem::size_of::<f64>())?).ok()?;
                Some(body.builder.ins().load(
                    types::F64,
                    MemFlagsData::trusted(),
                    arguments,
                    offset,
                ))
            }
            JsDenotation::LoadConst => {
                if instruction.returns_from_frame() || instruction.result_flags() != 0 {
                    return None;
                }
                let Constant::Number(value) =
                    program.constants.get(instruction.constant_index())?
                else {
                    return None;
                };
                Some(
                    body.builder
                        .ins()
                        .f64const(Ieee64::with_bits(value.to_bits())),
                )
            }
            JsDenotation::Move => {
                if instruction.returns_from_frame() || instruction.result_flags() != 0 {
                    return None;
                }
                let source = usize::from(instruction.register_b());
                if !*registers.get(source)? {
                    return None;
                }
                Some(body.read(source as u16, crate::WasmType::F64))
            }
            JsDenotation::PlainLocalStore => {
                if instruction.returns_from_frame() || instruction.result_flags() != 0 {
                    return None;
                }
                let source = usize::from(instruction.register_a());
                if !*registers.get(source)? {
                    return None;
                }
                let value = body.read(source as u16, crate::WasmType::F64);
                *locals.get_mut(instruction.local_slot())? = Some(value);
                if let Some(register) = instruction.optional_register_b() {
                    body.write(register, crate::WasmType::F64, value);
                    *registers.get_mut(usize::from(register))? = true;
                }
                None
            }
            JsDenotation::PlainLocalLoad => {
                if instruction.returns_from_frame()
                    || instruction.result_flags() != 0
                    || instruction.numeric_local_target().is_some()
                {
                    return None;
                }
                Some(*locals.get(instruction.local_slot())?.as_ref()?)
            }
            JsDenotation::CapturedValue => {
                if instruction.returns_from_frame() || instruction.result_flags() != 0 {
                    return None;
                }
                let capture = super::js_compiled::JsJitCaptureArgument {
                    depth: instruction.capture_depth(),
                    slot: instruction.capture_slot(),
                };
                if numeric_field_inputs.is_some_and(|inputs| {
                    inputs.iter().any(|input| {
                        matches!(input.receiver,
                            super::js_compiled::JsJitFieldReceiver::Capture {
                                register,
                                capture: receiver_capture,
                                ..
                            } if receiver_capture.depth == capture.depth
                                && receiver_capture.slot == capture.slot
                                && register == instruction.result_register())
                    })
                }) {
                    // The guest object remains in its rooted environment. Its
                    // cached numeric property is appended as a scalar input by
                    // the VM guard; native code never receives the object.
                    None
                } else {
                    let capture = captures.iter().position(|capture| {
                        capture.depth == instruction.capture_depth()
                            && capture.slot == instruction.capture_slot()
                    })?;
                    let argument_index = params.checked_add(capture)?;
                    let offset =
                        i32::try_from(argument_index.checked_mul(std::mem::size_of::<f64>())?)
                            .ok()?;
                    Some(body.builder.ins().load(
                        types::F64,
                        MemFlagsData::trusted(),
                        arguments,
                        offset,
                    ))
                }
            }
            JsDenotation::NumericUnary => {
                if instruction.numeric_local_target().is_some()
                    || (instruction.result_flags() != 0
                        && instruction.result_flags() != RETURN_REGISTER)
                {
                    return None;
                }
                let operation = super::operations::NumberUnary::from_js_instruction(
                    instruction.op(),
                    instruction.unary_operator(),
                )?;
                let source = usize::from(instruction.register_b());
                if !*registers.get(source)? {
                    return None;
                }
                let value = body.read(source as u16, crate::WasmType::F64);
                let value = emit_number_unary_operation(&mut body.builder, operation, value);
                if instruction.returns_from_frame() {
                    emit_f64_entry_return(&mut body.builder, result_pointer, value);
                    returned = true;
                    terminal = true;
                    None
                } else {
                    Some(value)
                }
            }
            JsDenotation::NumericBinary => {
                if instruction.numeric_local_target().is_some()
                    || (instruction.result_flags() != 0
                        && instruction.result_flags() != RETURN_REGISTER)
                {
                    return None;
                }
                let left = read_js_numeric_operand(
                    program,
                    &mut body,
                    instruction.operand_b(),
                    &registers,
                )?;
                let right = read_js_numeric_operand(
                    program,
                    &mut body,
                    instruction.operand_c(),
                    &registers,
                )?;
                let op = instruction.binary_operator();
                let arithmetic = super::operations::NumberArithmetic::from_js_instruction(
                    instruction.op(),
                    op,
                );
                let value = if let Some(operation) = arithmetic {
                    emit_number_arithmetic_operation(&mut body.builder, operation, left, right)
                } else if let Some(operation) =
                    super::operations::NumberBitwise::from_js_instruction(
                        instruction.op(),
                        op,
                    )
                    .filter(|operation| {
                        int32_bitwise
                            && operation.in_signed_int32_domain()
                    })
                {
                    let left = body.builder.ins().fcvt_to_sint(types::I32, left);
                    let right = body.builder.ins().fcvt_to_sint(types::I32, right);
                    emit_number_bitwise_operation(&mut body.builder, operation, left, right)
                } else {
                    return None;
                };
                if instruction.returns_from_frame() {
                    emit_f64_entry_return(&mut body.builder, result_pointer, value);
                    returned = true;
                    terminal = true;
                    None
                } else {
                    Some(value)
                }
            }
            JsDenotation::Return => {
                if instruction.result_flags() != 0 {
                    return None;
                }
                let register = instruction.register_a();
                if !*registers.get(usize::from(register))? {
                    return None;
                }
                let value = body.read(register, crate::WasmType::F64);
                emit_f64_entry_return(&mut body.builder, result_pointer, value);
                returned = true;
                terminal = true;
                None
            }
            JsDenotation::FieldStore
            | JsDenotation::Noop
            | JsDenotation::Jump
            | JsDenotation::Unsupported => return None,
        };
        if let Some(value) = result {
            let destination = usize::from(instruction.result_register());
            body.write(destination as u16, crate::WasmType::F64, value);
            *registers.get_mut(destination)? = true;
        }
        if !terminal {
            if instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough {
                return None;
            }
            let next = pc.checked_add(1)?;
            initialized_after[pc] = Some(registers);
            body.builder.ins().jump(blocks[next]?, &[]);
        }
    }
    if !returned {
        return None;
    }

    body.builder.seal_all_blocks();
    let Body { builder, .. } = body;
    builder.finalize(isa.frontend_config());
    let mut context = cranelift_codegen::Context::for_function(native);
    let compiled = match context.compile(
        &*isa,
        &mut cranelift_codegen::control::ControlPlane::default(),
    ) {
        Ok(compiled) => compiled,
        Err(_error) => {
            #[cfg(test)]
            eprintln!("general JS dispatch Cranelift rejection: {_error:?}");
            return None;
        }
    };
    if !compiled.buffer.relocs().is_empty() {
        #[cfg(test)]
        eprintln!("general JS dispatch has unresolved relocations");
        return None;
    }
    let code = compiled.code_buffer();
    if code.is_empty() || code.len() > super::js_compiled::MAX_CODE_BYTES {
        #[cfg(test)]
        eprintln!("general JS dispatch code exceeds its size budget");
        return None;
    }
    let code = code.to_vec();
    Some(super::js_compiled::JsJitArtifact {
        code,
        params,
        requires_signed_int32_arguments: int32_bitwise,
        result: super::js_compiled::JsJitResultKind::SharedNumberEntry,
        field_argument,
        numeric_field_inputs: numeric_field_inputs.map_or_else(Vec::new, |inputs| inputs.to_vec()),
        capture_arguments: captures.to_vec(),
    })
}

/// Compile a general JavaScript native dispatch loop. It carries the validated
/// bytecode PC in native code and calls the VM's shared `step` implementation
/// for each operation. This complete-semantics bridge lets us move pure
/// denotations into Cranelift without creating a second interpreter.
const MAX_JS_GENERAL_DISPATCH_BLOCKS: usize = 2048;

fn js_general_dispatch_candidate(_program: &ResidualProgram, function: &Function) -> bool {
    !function.code.is_empty()
        && function.code.len() <= MAX_JS_GENERAL_DISPATCH_BLOCKS
        && function.code.iter().all(|packed| {
            let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
            !matches!(
                instruction.op(),
                Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse
            ) || usize::try_from(instruction.jump_target()).is_ok()
        })
}

/// Preserve OSR as the owner of legacy hot-loop promotion. The generic
/// dispatcher can still compile loops admitted through its ordinary tier path;
/// this predicate only selects the earlier fallback for rejected/profiled
/// legacy candidates.
fn js_general_dispatch_acyclic_candidate(
    program: &ResidualProgram,
    function: &Function,
) -> bool {
    js_general_dispatch_candidate(program, function)
        && function.code.iter().enumerate().all(|(pc, packed)| {
            let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
            !matches!(
                instruction.op(),
                Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse
            ) || usize::try_from(instruction.jump_target()).is_ok_and(|target| target > pc)
        })
}

fn read_js_tagged_operand(
    builder: &mut FunctionBuilder<'_>,
    program: &ResidualProgram,
    operand: Operand,
    arguments: ir::Value,
    register_offset: &impl Fn(u16) -> Option<i32>,
) -> Option<ir::Value> {
    match operand.kind()? {
        OperandKind::Register => {
            let offset = register_offset(operand.register_index()?)?;
            Some(builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                arguments,
                offset,
            ))
        }
        OperandKind::Constant => {
            let value = match program.constants.get(usize::from(operand.payload()))? {
                crate::bytecode::Constant::Number(number) => Value::number(*number),
                crate::bytecode::Constant::Boolean(boolean) => Value::boolean(*boolean),
                crate::bytecode::Constant::Null => Value::NULL,
                crate::bytecode::Constant::Undefined => Value::UNDEFINED,
                crate::bytecode::Constant::String(_)
                | crate::bytecode::Constant::StringUnits(_)
                | crate::bytecode::Constant::BigInt(_)
                | crate::bytecode::Constant::WasmBits64(_)
                | crate::bytecode::Constant::WasmV128(_) => return None,
            };
            Some(builder.ins().iconst(types::I64, value.to_bits() as i64))
        }
        OperandKind::Field | OperandKind::Local => None,
    }
}

fn js_tagged_operand_supported(
    program: &ResidualProgram,
    operand: Operand,
    register_offset: &impl Fn(u16) -> Option<i32>,
) -> bool {
    match operand.kind() {
        Some(OperandKind::Register) => operand
            .register_index()
            .is_some_and(|register| register_offset(register).is_some()),
        Some(OperandKind::Constant) => matches!(
            program.constants.get(usize::from(operand.payload())),
            Some(crate::bytecode::Constant::Number(_))
        ),
        Some(OperandKind::Field | OperandKind::Local) | None => false,
    }
}

/// Compute ToBoolean for values whose truthiness is encoded without a heap
/// lookup. Strings, BigInts, and objects take the VM path at the same PC.
fn emit_js_tagged_truthiness(
    builder: &mut FunctionBuilder<'_>,
    bits: ir::Value,
    fallback: ir::Block,
) -> ir::Value {
    let is_true = builder
        .ins()
        .icmp_imm_u(IntCC::Equal, bits, Value::TRUE.to_bits() as i64);
    let is_false = builder
        .ins()
        .icmp_imm_u(IntCC::Equal, bits, Value::FALSE.to_bits() as i64);
    let is_null = builder
        .ins()
        .icmp_imm_u(IntCC::Equal, bits, Value::NULL.to_bits() as i64);
    let is_undefined = builder
        .ins()
        .icmp_imm_u(IntCC::Equal, bits, Value::UNDEFINED.to_bits() as i64);
    let known_primitive = builder.ins().bor(is_true, is_false);
    let known_primitive = builder.ins().bor(known_primitive, is_null);
    let known_primitive = builder.ins().bor(known_primitive, is_undefined);

    let known_block = builder.create_block();
    let number_block = builder.create_block();
    let join = builder.create_block();
    builder.append_block_param(join, types::I8);
    builder.ins().brif(known_primitive, known_block, &[], number_block, &[]);

    builder.switch_to_block(known_block);
    builder.ins().jump(join, &[ir::BlockArg::Value(is_true)]);

    builder.switch_to_block(number_block);
    let number = emit_js_tagged_number(builder, bits, fallback);
    let truthy = emit_number_truthiness(builder, number);
    builder.ins().jump(join, &[ir::BlockArg::Value(truthy)]);

    builder.seal_block(known_block);
    builder.seal_block(number_block);
    builder.seal_block(join);
    builder.switch_to_block(join);
    builder.block_params(join)[0]
}

/// Unbox a tagged JavaScript Number, branching to the current instruction's
/// VM fallback for every other value. The representation keeps all tags in
/// NaN space, with integer Numbers identified by `INT_TAG`.
fn emit_js_tagged_number(
    builder: &mut FunctionBuilder<'_>,
    bits: ir::Value,
    fallback: ir::Block,
) -> ir::Value {
    let number = builder
        .ins()
        .bitcast(types::F64, ir::MemFlagsData::new(), bits);
    let unordered = builder.ins().fcmp(FloatCC::Unordered, number, number);
    let nan = builder.create_block();
    let tagged = builder.create_block();
    let integer_block = builder.create_block();
    let join = builder.create_block();
    builder.append_block_param(join, types::F64);
    builder.ins().brif(
        unordered,
        nan,
        &[],
        join,
        &[ir::BlockArg::Value(number)],
    );

    builder.switch_to_block(nan);
    let field = builder
        .ins()
        .ushr_imm_u(bits, i64::from(crate::value::ValueLayout::TAG_SHIFT));
    let offset = builder.ins().iadd_imm_s(
        field,
        -(crate::value::ValueLayout::FIRST_TAG_FIELD as i64),
    );
    let is_tagged = builder.ins().icmp_imm_u(
        IntCC::UnsignedLessThanOrEqual,
        offset,
        crate::value::ValueLayout::TAG_FIELD_SPAN as i64,
    );
    builder.ins().brif(
        is_tagged,
        tagged,
        &[],
        join,
        &[ir::BlockArg::Value(number)],
    );

    builder.switch_to_block(tagged);
    let int_field = (crate::value::ValueLayout::INT_TAG
        >> crate::value::ValueLayout::TAG_SHIFT) as i64;
    let is_integer_tag = builder
        .ins()
        .icmp_imm_u(IntCC::Equal, field, int_field);
    builder
        .ins()
        .brif(is_integer_tag, integer_block, &[], fallback, &[]);

    builder.switch_to_block(integer_block);
    let integer = builder.ins().ireduce(types::I32, bits);
    let integer = builder.ins().fcvt_from_sint(types::F64, integer);
    builder
        .ins()
        .jump(join, &[ir::BlockArg::Value(integer)]);
    builder.seal_block(nan);
    builder.seal_block(tagged);
    builder.seal_block(integer_block);
    builder.seal_block(join);
    builder.switch_to_block(join);
    builder.block_params(join)[0]
}

/// Encode a Number result with `Value::number`'s integer, double and NaN
/// representations.
fn emit_js_box_number(builder: &mut FunctionBuilder<'_>, number: ir::Value) -> ir::Value {
    let truncated = builder.ins().fcvt_to_sint_sat(types::I32, number);
    let round_trip = builder.ins().fcvt_from_sint(types::F64, truncated);
    let result_bits = builder
        .ins()
        .bitcast(types::I64, ir::MemFlagsData::new(), number);
    let round_trip_bits = builder
        .ins()
        .bitcast(types::I64, ir::MemFlagsData::new(), round_trip);
    let is_integer = builder
        .ins()
        .icmp(IntCC::Equal, result_bits, round_trip_bits);
    let integer_bits = builder.ins().uextend(types::I64, truncated);
    let integer_bits = builder
        .ins()
        .bor_imm_u(integer_bits, crate::value::ValueLayout::INT_TAG as i64);
    let encoded = builder
        .ins()
        .select(is_integer, integer_bits, result_bits);
    let is_nan = builder.ins().fcmp(FloatCC::Unordered, number, number);
    let canonical_nan = builder.ins().iconst(
        types::I64,
        crate::value::ValueLayout::CANONICAL_NAN as i64,
    );
    builder.ins().select(is_nan, canonical_nan, encoded)
}

fn emit_js_step_fallback(
    builder: &mut FunctionBuilder<'_>,
    context: ir::Value,
    callout: ir::Value,
    callout_signature: ir::SigRef,
    result_pointer: ir::Value,
    stopped_block: ir::Block,
    continue_block: ir::Block,
    pc: usize,
) {
    let pc_value = builder.ins().iconst(types::I64, pc as i64);
    let call = builder.ins().call_indirect(
        callout_signature,
        callout,
        &[context, pc_value, result_pointer, result_pointer],
    );
    let status = builder.inst_results(call)[0];
    let failed = builder.ins().icmp_imm_u(IntCC::NotEqual, status, 0);
    let next_pc = builder
        .ins()
        .load(types::I64, MemFlagsData::trusted(), result_pointer, 0);
    builder.ins().brif(
        failed,
        stopped_block,
        &[],
        continue_block,
        &[ir::BlockArg::Value(next_pc)],
    );
}

enum JsGeneralBranch {
    ToBoolean {
        source: u16,
        take_when_truthy: bool,
    },
    NumberFalse {
        operator: u32,
        left: Operand,
        right: Operand,
    },
}

pub(super) fn compile_js_general_dispatch(
    program: &ResidualProgram,
    function_id: u32,
) -> Option<super::js_compiled::JsJitArtifact> {
    if program.kind == crate::bytecode::ProgramKind::Wasm || function_id == 0 {
        return None;
    }
    let function = program.function_get(function_id as usize)?;
    let instruction_count = function.code.len();
    if !js_general_dispatch_candidate(program, function) {
        return None;
    }

    let isa = super::jit_backend::target_isa("speed", true)?;
    let signature = native_entry_signature(isa.pointer_type(), isa.default_call_conv());
    let name = UserFuncName::user(0x5155_454e, function_id);
    let mut native = ir::Function::with_name_signature(name, signature);
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);

    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let parameters = builder.block_params(entry).to_vec();
    let context = parameters[0];
    let locals = parameters[1];
    let local_count = parameters[2];
    let arguments = parameters[3];
    let result_pointer = parameters[4];
    let stopped_block = builder.create_block();
    let continue_block = builder.create_block();
    let continue_pc = builder.append_block_param(continue_block, types::I64);
    let invalid_pc = builder.create_block();
    let invalid_pc_value = builder.append_block_param(invalid_pc, types::I64);
    let callout = builder
        .ins()
        .load(types::I64, invariant(), context, CONTEXT_CALLOUT);
    let blocks: Vec<_> = (0..instruction_count)
        .map(|_| builder.create_block())
        .collect();
    builder.ins().jump(blocks[0], &[]);

    let mut callout_signature = Signature::new(isa.default_call_conv());
    for _ in 0..4 {
        callout_signature.params.push(AbiParam::new(types::I64));
    }
    callout_signature.returns.push(AbiParam::new(types::I64));
    let callout_signature = builder.import_signature(callout_signature);
    builder.switch_to_block(stopped_block);
    // A nonzero callout status has a rooted `JsStepDispatch` result in the
    // adapter payload. The status itself only tells the outer entry to stop.
    let stopped = builder.ins().iconst(types::I32, 1);
    builder.ins().return_(&[stopped]);

    builder.switch_to_block(invalid_pc);
    builder.ins().store(
        MemFlagsData::trusted(),
        invalid_pc_value,
        result_pointer,
        0,
    );
    let out_of_range = builder.ins().iconst(types::I32, 2);
    builder.ins().return_(&[out_of_range]);

    let register_offset = |register: u16| -> Option<i32> {
        if register >= function.registers {
            return None;
        }
        i32::try_from(usize::from(register).checked_mul(std::mem::size_of::<u64>())?).ok()
    };
    for (pc, block) in blocks.iter().copied().enumerate() {
        builder.switch_to_block(block);
        let packed = function.code[pc];
        let instruction = super::dispatch_fast::decoded_instruction(function, packed);
        let next = blocks.get(pc + 1).copied();
        let direct = if let Some(next) = next {
            match instruction.op().js_denotation() {
                JsDenotation::Noop
                    if instruction.result_flags() == 0
                        && !instruction.returns_from_frame()
                        && !instruction.writes_current_this() =>
                {
                    builder.ins().jump(next, &[]);
                    true
                }
                JsDenotation::Jump
                    if instruction.result_flags() == 0
                        && !instruction.returns_from_frame()
                        && !instruction.writes_current_this() =>
                {
                    let target = usize::try_from(instruction.jump_target()).ok();
                    if let Some(target) = target.filter(|target| *target > pc)
                        && let Some(target) = blocks.get(target).copied()
                    {
                        builder.ins().jump(target, &[]);
                        true
                    } else {
                        false
                    }
                }
                JsDenotation::Move
                    if instruction.result_flags() == 0
                        && !instruction.returns_from_frame()
                        && !instruction.writes_current_this() =>
                {
                    if let (Some(source), Some(destination)) = (
                        register_offset(instruction.register_b()),
                        register_offset(instruction.result_register()),
                    ) {
                        let value = builder.ins().load(
                            types::I64,
                            MemFlagsData::trusted(),
                            arguments,
                            source,
                        );
                        builder.ins().store(
                            MemFlagsData::trusted(),
                            value,
                            arguments,
                            destination,
                        );
                        builder.ins().jump(next, &[]);
                        true
                    } else {
                        false
                    }
                }
                JsDenotation::LoadConst
                    if instruction.result_flags() == 0
                        && !instruction.returns_from_frame()
                        && !instruction.writes_current_this() =>
                {
                    let value = program.constants.get(instruction.constant_index()).and_then(
                        |constant| match constant {
                            crate::bytecode::Constant::Number(value) => {
                                Some(Value::number(*value))
                            }
                            crate::bytecode::Constant::Boolean(value) => {
                                Some(Value::boolean(*value))
                            }
                            crate::bytecode::Constant::Null => Some(Value::NULL),
                            crate::bytecode::Constant::Undefined => Some(Value::UNDEFINED),
                            crate::bytecode::Constant::String(_)
                            | crate::bytecode::Constant::StringUnits(_)
                            | crate::bytecode::Constant::BigInt(_)
                            | crate::bytecode::Constant::WasmBits64(_)
                            | crate::bytecode::Constant::WasmV128(_) => None,
                        },
                    );
                    if let (Some(value), Some(destination)) =
                        (value, register_offset(instruction.result_register()))
                    {
                        let value = builder.ins().iconst(types::I64, value.to_bits() as i64);
                        builder.ins().store(
                            MemFlagsData::trusted(),
                            value,
                            arguments,
                            destination,
                        );
                        builder.ins().jump(next, &[]);
                        true
                    } else {
                        false
                    }
                }
                JsDenotation::PlainLocalLoad
                    if instruction.result_flags() == 0
                        && !instruction.returns_from_frame()
                        && !instruction.writes_current_this()
                        && instruction.numeric_local_target().is_none() =>
                {
                    let slot = instruction.local_slot();
                    let slot_offset = slot
                        .checked_mul(std::mem::size_of::<u64>())
                        .and_then(|offset| i32::try_from(offset).ok());
                    if slot < usize::from(function.locals)
                        && let (Some(slot_offset), Some(destination)) =
                            (slot_offset, register_offset(instruction.result_register()))
                    {
                        let fast = builder.create_block();
                        let fallback = builder.create_block();
                        let in_bounds = builder.ins().icmp_imm_u(
                            IntCC::UnsignedGreaterThan,
                            local_count,
                            i64::try_from(slot).expect("validated bytecode local slot"),
                        );
                        builder.ins().brif(in_bounds, fast, &[], fallback, &[]);
                        builder.switch_to_block(fallback);
                        emit_js_step_fallback(
                            &mut builder,
                            context,
                            callout,
                            callout_signature,
                            result_pointer,
                            stopped_block,
                            continue_block,
                            pc,
                        );
                        builder.switch_to_block(fast);
                        let value = builder.ins().load(
                            types::I64,
                            MemFlagsData::trusted(),
                            locals,
                            slot_offset,
                        );
                        builder.ins().store(
                            MemFlagsData::trusted(),
                            value,
                            arguments,
                            destination,
                        );
                        builder.ins().jump(next, &[]);
                        true
                    } else {
                        false
                    }
                }
                JsDenotation::PlainLocalStore
                    if instruction.result_flags() == 0
                        && !instruction.returns_from_frame()
                        && !instruction.writes_current_this() =>
                {
                    let slot = instruction.local_slot();
                    let slot_offset = slot
                        .checked_mul(std::mem::size_of::<u64>())
                        .and_then(|offset| i32::try_from(offset).ok());
                    let source = register_offset(instruction.register_a());
                    let duplicate = match instruction.optional_register_b() {
                        None => Some(None),
                        Some(register) => register_offset(register).map(Some),
                    };
                    if slot < usize::from(function.locals)
                        && let (Some(slot_offset), Some(source), Some(duplicate)) =
                            (slot_offset, source, duplicate)
                    {
                        let fast = builder.create_block();
                        let fallback = builder.create_block();
                        let in_bounds = builder.ins().icmp_imm_u(
                            IntCC::UnsignedGreaterThan,
                            local_count,
                            i64::try_from(slot).expect("validated bytecode local slot"),
                        );
                        builder.ins().brif(in_bounds, fast, &[], fallback, &[]);
                        builder.switch_to_block(fallback);
                        emit_js_step_fallback(
                            &mut builder,
                            context,
                            callout,
                            callout_signature,
                            result_pointer,
                            stopped_block,
                            continue_block,
                            pc,
                        );
                        builder.switch_to_block(fast);
                        let value = builder.ins().load(
                            types::I64,
                            MemFlagsData::trusted(),
                            arguments,
                            source,
                        );
                        builder.ins().store(
                            MemFlagsData::trusted(),
                            value,
                            locals,
                            slot_offset,
                        );
                        if let Some(duplicate) = duplicate {
                            builder.ins().store(
                                MemFlagsData::trusted(),
                                value,
                                arguments,
                                duplicate,
                            );
                        }
                        builder.ins().jump(next, &[]);
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            }
        } else {
            false
        };
        if direct {
            continue;
        }

        let direct_branch = match instruction.op() {
            Op::JumpFalse | Op::JumpTrue => register_offset(instruction.register_a()).map(
                |_| JsGeneralBranch::ToBoolean {
                    source: instruction.register_a(),
                    take_when_truthy: instruction.op() == Op::JumpTrue,
                },
            ),
            Op::JumpBinaryFalse => {
                let operator = instruction.binary_operator_field();
                let left = instruction.operand_b();
                let right = instruction.operand_c();
                (super::operations::NumberRelation::from_operator(operator).is_some()
                    && js_tagged_operand_supported(program, left, &register_offset)
                    && js_tagged_operand_supported(program, right, &register_offset))
                .then_some(JsGeneralBranch::NumberFalse {
                    operator,
                    left,
                    right,
                })
            }
            _ => None,
        };
        // Only a recognized branch has a jump target to read.
        if let (Some(next), Some(direct_branch)) = (next, direct_branch)
            && let Ok(target) = usize::try_from(instruction.jump_target())
            && target > pc
            && let Some(target_block) = blocks.get(target).copied()
            && instruction.result_flags() == 0
            && !instruction.returns_from_frame()
            && !instruction.writes_current_this()
        {
            let fallback = builder.create_block();
            let branch = match direct_branch {
                JsGeneralBranch::ToBoolean {
                    source,
                    take_when_truthy,
                } => {
                    let source = register_offset(source).expect("prevalidated branch register");
                    let bits = builder.ins().load(
                        types::I64,
                        MemFlagsData::trusted(),
                        arguments,
                        source,
                    );
                    let truthy = emit_js_tagged_truthiness(&mut builder, bits, fallback);
                    if take_when_truthy {
                        truthy
                    } else {
                        builder.ins().icmp_imm_u(IntCC::Equal, truthy, 0)
                    }
                }
                JsGeneralBranch::NumberFalse {
                    operator,
                    left,
                    right,
                } => {
                    let left = read_js_tagged_operand(
                        &mut builder,
                        program,
                        left,
                        arguments,
                        &register_offset,
                    )
                    .expect("prevalidated JS relation operand");
                    let left = emit_js_tagged_number(&mut builder, left, fallback);
                    let right = read_js_tagged_operand(
                        &mut builder,
                        program,
                        right,
                        arguments,
                        &register_offset,
                    )
                    .expect("prevalidated JS relation operand");
                    let right = emit_js_tagged_number(&mut builder, right, fallback);
                    let holds = emit_number_relation(
                        &mut builder,
                        operator,
                        left,
                        right,
                        NumberRelationDomain::Float64,
                    )
                    .expect("prevalidated JavaScript numeric relation");
                    builder.ins().icmp_imm_u(IntCC::Equal, holds, 0)
                }
            };
            builder.ins().brif(branch, target_block, &[], next, &[]);
            builder.switch_to_block(fallback);
            emit_js_step_fallback(
                &mut builder,
                context,
                callout,
                callout_signature,
                result_pointer,
                stopped_block,
                continue_block,
                pc,
            );
            continue;
        }

        // The operator and registers are read only once the op is a numeric unary.
        if instruction.op().js_denotation() == JsDenotation::NumericUnary
            && let Some(next) = next
            && let Some(unary) = super::operations::NumberUnary::from_js_instruction(
                instruction.op(),
                instruction.unary_operator(),
            )
            && let Some(source) = register_offset(instruction.register_b())
            && let Some(destination) = register_offset(instruction.result_register())
            && instruction.numeric_local_target().is_none()
            && instruction.result_flags() == 0
            && !instruction.returns_from_frame()
            && !instruction.writes_current_this()
        {
            let fallback = builder.create_block();
            let bits = builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                arguments,
                source,
            );
            let number = emit_js_tagged_number(&mut builder, bits, fallback);
            let number = emit_number_unary_operation(&mut builder, unary, number);
            let encoded = emit_js_box_number(&mut builder, number);
            builder.ins().store(
                MemFlagsData::trusted(),
                encoded,
                arguments,
                destination,
            );
            builder.ins().jump(next, &[]);
            builder.switch_to_block(fallback);
            emit_js_step_fallback(
                &mut builder,
                context,
                callout,
                callout_signature,
                result_pointer,
                stopped_block,
                continue_block,
                pc,
            );
            continue;
        }

        // The operator is read only once the op is a numeric binary.
        let numeric_binary = instruction.op().js_denotation() == JsDenotation::NumericBinary;
        let arithmetic = numeric_binary
            .then(|| {
                super::operations::NumberArithmetic::from_js_instruction(
                    instruction.op(),
                    instruction.binary_operator(),
                )
            })
            .flatten();
        let operands = numeric_binary.then(|| (instruction.operand_b(), instruction.operand_c()));
        let numeric_result = numeric_binary
            && arithmetic.is_some()
            && instruction.numeric_local_target().is_none()
            && instruction.result_flags() == 0
            && !instruction.returns_from_frame()
            && !instruction.writes_current_this()
            && register_offset(instruction.result_register()).is_some()
            && operands.is_some_and(|(left, right)| {
                js_tagged_operand_supported(program, left, &register_offset)
                    && js_tagged_operand_supported(program, right, &register_offset)
            });
        if let (true, Some(next), Some(arithmetic), Some((left_operand, right_operand))) =
            (numeric_result, next, arithmetic, operands)
        {
            let fallback = builder.create_block();
            let left_bits = read_js_tagged_operand(
                &mut builder,
                program,
                left_operand,
                arguments,
                &register_offset,
            )
            .expect("prevalidated JS numeric operand");
            let left = emit_js_tagged_number(&mut builder, left_bits, fallback);
            let right_bits = read_js_tagged_operand(
                &mut builder,
                program,
                right_operand,
                arguments,
                &register_offset,
            )
            .expect("prevalidated JS numeric operand");
            let right = emit_js_tagged_number(&mut builder, right_bits, fallback);
            let number = emit_number_arithmetic_operation(&mut builder, arithmetic, left, right);
            let encoded = emit_js_box_number(&mut builder, number);
            let destination = register_offset(instruction.result_register())
                .expect("prevalidated JS numeric destination");
            builder.ins().store(
                MemFlagsData::trusted(),
                encoded,
                arguments,
                destination,
            );
            builder.ins().jump(next, &[]);
            builder.switch_to_block(fallback);
            emit_js_step_fallback(
                &mut builder,
                context,
                callout,
                callout_signature,
                result_pointer,
                stopped_block,
                continue_block,
                pc,
            );
            continue;
        }

        emit_js_step_fallback(
            &mut builder,
            context,
            callout,
            callout_signature,
            result_pointer,
            stopped_block,
            continue_block,
            pc,
        );
    }
    builder.switch_to_block(continue_block);
    let selector = builder.ins().ireduce(types::I32, continue_pc);
    let cases: Vec<_> = blocks
        .iter()
        .map(|target| builder.func.dfg.block_call(*target, &[]))
        .collect();
    let invalid_argument = ir::BlockArg::Value(continue_pc);
    let default = builder
        .func
        .dfg
        .block_call(invalid_pc, &[invalid_argument]);
    let table = ir::JumpTableData::new(default, &cases);
    let table = builder.create_jump_table(table);
    builder.ins().br_table(selector, table);
    builder.seal_block(continue_block);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    let code = compile_native_function(
        native,
        &*isa,
        super::js_compiled::MAX_CODE_BYTES,
        |code| Some(code.to_vec()),
    )?;
    Some(super::js_compiled::JsJitArtifact {
        code,
        params: usize::from(function.params),
        requires_signed_int32_arguments: false,
        result: super::js_compiled::JsJitResultKind::GeneralDispatch,
        field_argument: None,
        numeric_field_inputs: Vec::new(),
        capture_arguments: Vec::new(),
    })
}

/// How the shared VM executes JavaScript and Wasm functions. Compiled code is
/// lazy and falls back to the interpreter; both modes use the same VM, heap,
/// frames and host APIs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionMode {
    /// The shared interpreter: general loop and fast lane.
    #[default]
    Interpreted,
    /// Hot supported JavaScript functions and Wasm functions run as native code.
    Compiled,
}

/// A value's width inside compiled code. Each register has an integer and a
/// float view per width (`View`); a write defines both, so the analysis
/// tracks only the width and a reinterpretation is free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    I32,
    I64,
}

impl Kind {
    fn of(ty: crate::WasmType) -> Option<Self> {
        match ty {
            crate::WasmType::I32 | crate::WasmType::F32 => Some(Self::I32),
            crate::WasmType::I64 | crate::WasmType::F64 => Some(Self::I64),
            _ => None,
        }
    }

    fn ir(self) -> ir::Type {
        match self {
            Self::I32 => types::I32,
            Self::I64 => types::I64,
        }
    }

    fn integer(self) -> crate::WasmType {
        match self {
            Self::I32 => crate::WasmType::I32,
            Self::I64 => crate::WasmType::I64,
        }
    }

    fn of_ir(ty: ir::Type) -> Self {
        if ty.bits() == types::I32.bits() {
            Self::I32
        } else {
            Self::I64
        }
    }
}

/// The views of a register's value, integer and float per width, in the
/// order of a register's variables.
const VIEW_TYPES: [crate::WasmType; 4] = [
    crate::WasmType::I32,
    crate::WasmType::I64,
    crate::WasmType::F32,
    crate::WasmType::F64,
];

/// The variable index of a register's view as Wasm type `ty`.
fn view(ty: crate::WasmType) -> usize {
    VIEW_TYPES
        .iter()
        .position(|candidate| *candidate == ty)
        .expect("compiled values are numeric scalars")
}

/// The machine type of a register's view as `ty`. An i32 view holds its value zero-extended to
/// 64 bits, so an access can use it as an address offset as it is: x64's 32-bit operations
/// already clear the upper half, and Cranelift elides the extension of their results, but not
/// across the block parameters a loop carries.
fn view_ir(ty: crate::WasmType) -> ir::Type {
    match ty {
        crate::WasmType::I32 => types::I64,
        _ => scalar_ir(ty),
    }
}

/// The machine type of a scalar Wasm type.
fn scalar_ir(ty: crate::WasmType) -> ir::Type {
    match ty {
        crate::WasmType::I32 => types::I32,
        crate::WasmType::I64 => types::I64,
        crate::WasmType::F32 => types::F32,
        crate::WasmType::F64 => types::F64,
        _ => unreachable!("compiled values are numeric scalars"),
    }
}

/// The type of the other view of the same bits.
fn counterpart(ty: crate::WasmType) -> crate::WasmType {
    match ty {
        crate::WasmType::I32 => crate::WasmType::F32,
        crate::WasmType::I64 => crate::WasmType::F64,
        crate::WasmType::F32 => crate::WasmType::I32,
        crate::WasmType::F64 => crate::WasmType::I64,
        _ => unreachable!("compiled values are numeric scalars"),
    }
}

/// An i32 integer operator is its i64 namesake: the i64 selectors extend the
/// shared declaration (`crate::wasm::integer`), so tags agree.
fn widen_binary(operator: I32BinaryOperator) -> I64BinaryOperator {
    I64BinaryOperator::from_tag(operator as u32).expect("i64 selectors extend i32's")
}

fn widen_unary(operator: I32UnaryOperator) -> I64UnaryOperator {
    I64UnaryOperator::from_tag(operator as u32).expect("i64 selectors extend i32's")
}

/// A float operator and its operand type; both widths share one declaration
/// (`crate::wasm::float`), so tags agree.
fn float_binary_operator(op: Op, tag: u32) -> Option<(F64BinaryOperator, crate::WasmType)> {
    match op {
        Op::WasmF32Binary => Some((
            F64BinaryOperator::from_tag(F32BinaryOperator::from_tag(tag)? as u32)?,
            crate::WasmType::F32,
        )),
        Op::WasmF64Binary => Some((F64BinaryOperator::from_tag(tag)?, crate::WasmType::F64)),
        _ => None,
    }
}

fn float_unary_operator(op: Op, tag: u32) -> Option<(F64UnaryOperator, crate::WasmType)> {
    match op {
        Op::WasmF32Unary => Some((
            F64UnaryOperator::from_tag(F32UnaryOperator::from_tag(tag)? as u32)?,
            crate::WasmType::F32,
        )),
        Op::WasmF64Unary => Some((F64UnaryOperator::from_tag(tag)?, crate::WasmType::F64)),
        _ => None,
    }
}

/// What a register holds at an instruction, as the analysis derives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Value(Kind),
    /// An instance binding: the environment slot it was loaded from.
    Binding(u32),
    /// The function's memory binding; only direct accesses read it.
    Memory,
    /// A reference to imported function `index`; only a call reads it.
    Import(u32),
    /// An indirect table call, with its table binding and expected type.
    IndirectReference {
        signature: u32,
        table: u32,
    },
    /// Paths disagree; no compiled instruction may read it.
    Conflict,
}

impl Slot {
    const DEFAULT: Self = Self::Value(Kind::I32);

    fn join(self, other: Self) -> Self {
        if self == other { self } else { Self::Conflict }
    }
}

/// Register facts at one bytecode position. Most Wasm temporaries have the
/// default i32 representation, so retaining a full `registers`-sized vector
/// at every reachable instruction wastes memory. This stores only deviations
/// from that default; joins still apply the same `Slot` lattice to every
/// register because two different deviations meet at `Conflict`.
#[derive(Clone, Debug)]
struct AnalysisState {
    slots: Vec<(u16, Slot)>,
}

impl AnalysisState {
    fn new() -> Self {
        Self { slots: Vec::new() }
    }

    fn get(&self, register: u16, registers: u16) -> Option<Slot> {
        if register >= registers {
            return None;
        }
        Some(
            self.slots
                .binary_search_by_key(&register, |(index, _)| *index)
                .map(|index| self.slots[index].1)
                .unwrap_or(Slot::DEFAULT),
        )
    }

    fn set(&mut self, register: u16, registers: u16, slot: Slot) -> Option<()> {
        if register >= registers {
            return None;
        }
        match self
            .slots
            .binary_search_by_key(&register, |(index, _)| *index)
        {
            Ok(index) if slot == Slot::DEFAULT => {
                self.slots.remove(index);
            }
            Ok(index) => self.slots[index].1 = slot,
            Err(index) if slot != Slot::DEFAULT => {
                self.slots.insert(index, (register, slot));
            }
            Err(_) => {}
        }
        Some(())
    }

    fn join_from(&mut self, incoming: &Self) -> bool {
        let mut joined = Vec::with_capacity(self.slots.len() + incoming.slots.len());
        let (mut left, mut right) = (0, 0);
        while left < self.slots.len() || right < incoming.slots.len() {
            let left_index = self.slots.get(left).map(|(index, _)| *index);
            let right_index = incoming.slots.get(right).map(|(index, _)| *index);
            let register = match (left_index, right_index) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => unreachable!(),
            };
            let current = if left_index == Some(register) {
                let slot = self.slots[left].1;
                left += 1;
                slot
            } else {
                Slot::DEFAULT
            };
            let other = if right_index == Some(register) {
                let slot = incoming.slots[right].1;
                right += 1;
                slot
            } else {
                Slot::DEFAULT
            };
            let slot = current.join(other);
            if slot != Slot::DEFAULT {
                joined.push((register, slot));
            }
        }
        let changed = joined != self.slots;
        self.slots = joined;
        changed
    }
}

/// Traps compiled code can raise, by status (index + 1); status 0 is a
/// normal return.
const TRAPS: [crate::WasmTrap; 6] = [
    crate::WasmTrap::Unreachable,
    crate::WasmTrap::OutOfBoundsMemory,
    crate::WasmTrap::IntegerDivideByZero,
    crate::WasmTrap::IntegerOverflow,
    crate::WasmTrap::CallStackExhausted,
    crate::WasmTrap::InvalidConversionToInteger,
];

const JIT_ROOT_ENV: &str = "QUENCH_WASM_JIT_ROOT";
const JIT_ROOT_OPT_LEVEL_ENV: &str = "QUENCH_WASM_JIT_ROOT_OPT_LEVEL";
const JIT_ROOT_ONLY_ENV: &str = "QUENCH_WASM_JIT_ROOT_ONLY";
const JIT_ROOT_FIRST_ENV: &str = "QUENCH_WASM_JIT_ROOT_FIRST";
const JIT_WASM_SELECTIVE_ENV: &str = "QUENCH_WASM_JIT_SELECTIVE";
const JIT_DIRECT_COMPARE_BRANCHES_ENV: &str = "QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES";
const JIT_AFFINE_PAIR_ENV: &str = "QUENCH_WASM_JIT_AFFINE_PAIR";
const JIT_REFERENCE_BODY_CACHE_ENV: &str = "QUENCH_WASM_JIT_REFERENCE_BODY_CACHE";
const JIT_EMSCRIPTEN_MEMCPY_LEAF_ENV: &str = "QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF";
const JIT_NUMERIC_FIND_ENV: &str = "QUENCH_JS_JIT_NUMERIC_FIND";
const JIT_NATIVE_ENTRY_SAMPLE_INTERVAL: u64 = 256;
const MAX_JIT_ROOT_FUNCTIONS: usize = 128;
const MAX_JIT_ROOT_BYTECODES_PER_FUNCTION: usize = 80_000;
const MAX_JIT_ROOT_CODE_BYTES: usize = 2 * 1024 * 1024;
/// A selective root may include a few hot callees learned from earlier
/// indirect-call executions. Keep the profile-guided closure bounded so one
/// polymorphic module cannot turn a tier promotion into eager compilation.
const MAX_JIT_PROFILED_INDIRECT_TARGETS: usize = 8;
pub(super) const MAX_JIT_TIER_BYTECODES_PER_FUNCTION: usize = 2_048;

fn jit_root_override() -> Result<Option<usize>, ()> {
    #[cfg(test)]
    if let Some(mode) = WASM_JIT_MODE_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return Ok(mode.root);
    }
    let Some(root) = std::env::var_os(JIT_ROOT_ENV) else {
        return Ok(None);
    };
    root.to_str()
        .ok_or(())?
        .parse::<usize>()
        .map(Some)
        .map_err(|_| ())
}

fn jit_wasm_selective_enabled() -> bool {
    std::env::var(JIT_WASM_SELECTIVE_ENV).is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
}

fn jit_direct_compare_branches_enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = WASM_JIT_DIRECT_COMPARE_BRANCHES_TEST_OVERRIDE.with(std::cell::Cell::get)
    {
        return enabled;
    }
    std::env::var(JIT_DIRECT_COMPARE_BRANCHES_ENV)
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
}

/// Keep the screened affine pair loop opt-in; the benchmark harness can still
/// run a same-binary, same-host comparison arm with it enabled.
fn jit_affine_pair_enabled() -> bool {
    std::env::var(JIT_AFFINE_PAIR_ENV).is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
}

fn jit_reference_body_cache_enabled() -> bool {
    std::env::var(JIT_REFERENCE_BODY_CACHE_ENV)
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
}

fn jit_emscripten_memcpy_leaf_enabled() -> bool {
    std::env::var(JIT_EMSCRIPTEN_MEMCPY_LEAF_ENV)
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
}

/// Run the guarded numeric hash-map loop by default. Set the environment
/// variable to `0` to retain the interpreter path for A/B measurement.
fn jit_numeric_find_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| match std::env::var_os(JIT_NUMERIC_FIND_ENV) {
        None => true,
        Some(value) => matches!(value.to_str(), Some("1" | "true")),
    })
}

fn jit_wasm_root_only_enabled() -> bool {
    #[cfg(test)]
    if let Some(mode) = WASM_JIT_MODE_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return mode.root_only;
    }
    std::env::var(JIT_ROOT_ONLY_ENV).is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
}

fn jit_wasm_root_first_enabled() -> bool {
    #[cfg(test)]
    if let Some(mode) = WASM_JIT_MODE_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return mode.root_first;
    }
    std::env::var(JIT_ROOT_FIRST_ENV).is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
}

#[cfg(test)]
#[derive(Clone, Copy, Default)]
struct WasmJitModeTestOverride {
    root_only: bool,
    root_first: bool,
    root: Option<usize>,
}

#[cfg(test)]
thread_local! {
    static WASM_JIT_MODE_TEST_OVERRIDE: std::cell::Cell<Option<WasmJitModeTestOverride>> =
        const { std::cell::Cell::new(None) };
    static WASM_JIT_DIRECT_COMPARE_BRANCHES_TEST_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(super) fn with_wasm_jit_mode_for_test<R>(
    root_only: bool,
    root_first: bool,
    root: Option<usize>,
    run: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<WasmJitModeTestOverride>);

    impl Drop for Restore {
        fn drop(&mut self) {
            WASM_JIT_MODE_TEST_OVERRIDE.with(|override_mode| override_mode.set(self.0));
        }
    }

    let previous = WASM_JIT_MODE_TEST_OVERRIDE.with(|override_mode| {
        override_mode.replace(Some(WasmJitModeTestOverride {
            root_only,
            root_first,
            root,
        }))
    });
    let _restore = Restore(previous);
    run()
}

#[cfg(test)]
pub(super) fn with_jit_direct_compare_branches_for_test<R>(
    enabled: bool,
    run: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<bool>);

    impl Drop for Restore {
        fn drop(&mut self) {
            WASM_JIT_DIRECT_COMPARE_BRANCHES_TEST_OVERRIDE
                .with(|override_mode| override_mode.set(self.0));
        }
    }

    let previous = WASM_JIT_DIRECT_COMPARE_BRANCHES_TEST_OVERRIDE
        .with(|override_mode| override_mode.replace(Some(enabled)));
    let _restore = Restore(previous);
    run()
}

fn jit_root_opt_level(root: Option<usize>, root_only: bool) -> Result<&'static str, ()> {
    if root.is_none() {
        return Ok("speed");
    }
    match std::env::var_os(JIT_ROOT_OPT_LEVEL_ENV) {
        Some(level) => parse_jit_root_opt_level(root_only, Some(level.to_str().ok_or(())?)),
        None => parse_jit_root_opt_level(root_only, None),
    }
}

fn parse_jit_root_opt_level(root_only: bool, level: Option<&str>) -> Result<&'static str, ()> {
    let Some(level) = level else {
        return Ok(if root_only { "none" } else { "speed" });
    };
    match level {
        "none" => Ok("none"),
        "speed" => Ok("speed"),
        "speed_and_size" => Ok("speed_and_size"),
        _ => Err(()),
    }
}

/// Linux process RSS and high-water mark, in KiB, for opt-in JIT diagnostics.
#[cfg(target_os = "linux")]
fn jit_memory_snapshot() -> Option<(u64, u64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let mut rss = None;
    let mut high_water = None;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmRSS:") {
            rss = value.split_whitespace().next()?.parse().ok();
        } else if let Some(value) = line.strip_prefix("VmHWM:") {
            high_water = value.split_whitespace().next()?.parse().ok();
        }
        if rss.is_some() && high_water.is_some() {
            break;
        }
    }
    Some((rss?, high_water?))
}

#[cfg(not(target_os = "linux"))]
fn jit_memory_snapshot() -> Option<(u64, u64)> {
    None
}

/// The status of a callout that raised: the VM's error (or panic) waits in
/// the context.
const RAISED_STATUS: i64 = TRAPS.len() as i64 + 1;
const TERMINATED_STATUS: i64 = RAISED_STATUS + 1;
const MEMCPY_LEAF_MISS_STATUS: i64 = RAISED_STATUS + 2;

fn trap_status(trap: crate::WasmTrap) -> i64 {
    TRAPS
        .iter()
        .position(|candidate| *candidate == trap)
        .map(|index| index as i64 + 1)
        .expect("compiled code raises only listed traps")
}

/// Stack compiled code may use below its entry before a call traps as
/// exhausted; the interpreter's own guards bound the remainder.
const COMPILED_STACK_BUDGET: usize = 512 * 1024;
/// Stack left for a guarded leaf-JS entry after the Rust adapter has run. The
/// JS denotations admitted here have no calls, VM callouts, or internal polls.
const JS_LEAF_STACK_RESERVE: usize = 16 * 1024;

/// The state every compiled function receives. Field offsets are the ABI
/// compiled code loads from.
#[repr(C)]
struct CallContext {
    stack_limit: usize,
    interrupt: *const std::sync::atomic::AtomicBool,
    table: *const usize,
    vm: *mut (),
    env: Value,
    /// The entry trampoline's stack pointer after it saved the caller's
    /// registers; a guard fault resumes there.
    entry_stack: usize,
    /// The guarded memory reservation compiled code may fault in.
    guard_start: usize,
    guard_end: usize,
    /// Per global the program accesses (`CompiledProgram::globals`), the
    /// address of the global's value in this instance. Heap cells never move,
    /// so the addresses outlive callouts.
    globals: *const *mut Value,
    /// `import_callout` for the VM's host type.
    callout: usize,
    /// VM helper for indirect calls, including table checks and invocation.
    call_reference: usize,
    /// The artifact containing the currently executing call site. The active
    /// entry (or its caller) keeps this artifact strongly owned for the callout.
    compiled: *const CompiledProgram,
    // Fields below are the VM's, never read by compiled code.
    program: ProgramId,
    /// Number of Wasm-to-host callouts made by this native invocation.
    import_callouts: *mut u64,
    /// Number of direct known calls that had to use the VM fallback.
    known_callouts: *mut u64,
    /// Number of Wasm indirect-reference callouts made by this native invocation.
    reference_callouts: *mut u64,
    /// The bound linear memory whose base and size this call uses.
    memory: Value,
    /// Its exact current byte length, refreshed when a callout may grow it.
    memory_length: u64,
    /// What a callout that returned `RAISED_STATUS` raised.
    raised: Option<Raised>,
    // Diagnostic-only ABI fields are appended so existing repr(C) offsets
    // used by generated code stay fixed.
    call_reference_diagnostic: usize,
    reference_call_sites: *const ReferenceCallSiteDiagnostics,
    reference_call_site_count: usize,
    /// The VM's poll page (`ExecutionInterrupt::poll_page`), whose fault ends the call as
    /// terminated; empty when compiled code checks the flag instead.
    poll_start: usize,
    poll_end: usize,
    /// VM-local epoch advanced by table writes and collections. Fast indirect
    /// calls are valid only while their artifact-local cached epoch matches.
    reference_epoch: *const u64,
    /// Stable per-site cache storage owned by the current compiled artifact.
    reference_fast_caches: *mut FastReferenceCache,
    /// Diagnostic-only count of direct Emscripten same-memory copies.
    emscripten_memcpy_fast_path_hits: *mut u64,
    /// Diagnostic-only count of early raw-i32 Emscripten import copies.
    emscripten_memcpy_raw_i32_hits: *mut u64,
    /// Diagnostic-only count proving the early callout handled the copy.
    emscripten_memcpy_early_import_hits: *mut u64,
    /// Shared by nested native entries; host callouts invalidate the proof.
    emscripten_memcpy_cache: *mut EmscriptenMemcpyCache,
    /// Entry for the generated direct-copy leaf path.
    emscripten_memcpy_leaf: usize,
    /// Diagnostic-only count of direct leaf calls.
    emscripten_memcpy_leaf_hits: *mut u64,
    /// Per-import attribution, present only during opt-in diagnostic runs.
    import_callout_counts: *const std::cell::Cell<u64>,
    import_callout_count: usize,
    /// A stack-owned adapter payload for JS and OSR entries that use this
    /// module's common native-entry trampoline without Wasm callouts.
    adapter_payload: *mut (),
}

impl CallContext {
    fn native_adapter(
        adapter_payload: *mut (),
        interrupt: *const std::sync::atomic::AtomicBool,
        poll_page: Option<(usize, usize)>,
    ) -> Self {
        Self {
            stack_limit: native_stack_limit(),
            interrupt,
            table: std::ptr::null(),
            vm: std::ptr::null_mut(),
            env: Value::NULL,
            entry_stack: 0,
            guard_start: 0,
            guard_end: 0,
            globals: std::ptr::null(),
            callout: 0,
            call_reference: 0,
            compiled: std::ptr::null(),
            program: ProgramId::MAIN,
            import_callouts: std::ptr::null_mut(),
            known_callouts: std::ptr::null_mut(),
            reference_callouts: std::ptr::null_mut(),
            memory: Value::NULL,
            memory_length: 0,
            raised: None,
            call_reference_diagnostic: 0,
            reference_call_sites: std::ptr::null(),
            reference_call_site_count: 0,
            poll_start: poll_page.map_or(0, |(start, _)| start),
            poll_end: poll_page.map_or(0, |(start, bytes)| start + bytes),
            reference_epoch: std::ptr::null(),
            reference_fast_caches: std::ptr::null_mut(),
            emscripten_memcpy_fast_path_hits: std::ptr::null_mut(),
            emscripten_memcpy_raw_i32_hits: std::ptr::null_mut(),
            emscripten_memcpy_early_import_hits: std::ptr::null_mut(),
            emscripten_memcpy_cache: std::ptr::null_mut(),
            emscripten_memcpy_leaf: 0,
            emscripten_memcpy_leaf_hits: std::ptr::null_mut(),
            import_callout_counts: std::ptr::null(),
            import_callout_count: 0,
            adapter_payload,
        }
    }

    /// The context of code that reads only the interrupt state: the flag, and the VM's
    /// poll page, whose fault ends the call as terminated (`guard_fault`).
    fn interrupt_only(interrupt: &crate::execution_interrupt::ExecutionInterrupt) -> Self {
        let poll = interrupt.poll_page();
        Self {
            stack_limit: 0,
            interrupt: interrupt.atomic_flag(),
            table: std::ptr::null(),
            vm: std::ptr::null_mut(),
            env: Value::NULL,
            entry_stack: 0,
            guard_start: 0,
            guard_end: 0,
            globals: std::ptr::null(),
            callout: 0,
            call_reference: 0,
            compiled: std::ptr::null(),
            program: ProgramId::MAIN,
            import_callouts: std::ptr::null_mut(),
            known_callouts: std::ptr::null_mut(),
            reference_callouts: std::ptr::null_mut(),
            memory: Value::NULL,
            memory_length: 0,
            raised: None,
            call_reference_diagnostic: 0,
            reference_call_sites: std::ptr::null(),
            reference_call_site_count: 0,
            poll_start: poll.map_or(0, |(start, _)| start),
            poll_end: poll.map_or(0, |(start, bytes)| start + bytes),
            reference_epoch: std::ptr::null(),
            reference_fast_caches: std::ptr::null_mut(),
            emscripten_memcpy_fast_path_hits: std::ptr::null_mut(),
            emscripten_memcpy_raw_i32_hits: std::ptr::null_mut(),
            emscripten_memcpy_early_import_hits: std::ptr::null_mut(),
            emscripten_memcpy_cache: std::ptr::null_mut(),
            emscripten_memcpy_leaf: 0,
            emscripten_memcpy_leaf_hits: std::ptr::null_mut(),
            import_callout_counts: std::ptr::null(),
            import_callout_count: 0,
            adapter_payload: std::ptr::null_mut(),
        }
    }

    /// A nested native entry shares the caller's instance and callout state,
    /// but needs its own guard-resume stack pointer and raised completion, as a
    /// nested `run_compiled` invocation would. Its stack limit is never below
    /// the caller's: recursion through the callout (a runaway `call_indirect`)
    /// spends the outermost entry's budget and traps as exhausted, rather than
    /// taking a fresh budget at each level until the native stack overflows.
    fn nested_entry(&self) -> Self {
        let marker = 0u8;
        let stack_pointer = std::hint::black_box(&marker as *const u8 as usize);
        Self {
            stack_limit: self
                .stack_limit
                .max(stack_pointer.saturating_sub(COMPILED_STACK_BUDGET)),
            interrupt: self.interrupt,
            table: self.table,
            vm: self.vm,
            env: self.env,
            entry_stack: 0,
            guard_start: self.guard_start,
            guard_end: self.guard_end,
            globals: self.globals,
            callout: self.callout,
            call_reference: self.call_reference,
            compiled: self.compiled,
            program: self.program,
            import_callouts: self.import_callouts,
            known_callouts: self.known_callouts,
            reference_callouts: self.reference_callouts,
            memory: self.memory,
            memory_length: self.memory_length,
            raised: None,
            call_reference_diagnostic: self.call_reference_diagnostic,
            reference_call_sites: self.reference_call_sites,
            reference_call_site_count: self.reference_call_site_count,
            poll_start: self.poll_start,
            poll_end: self.poll_end,
            reference_epoch: self.reference_epoch,
            reference_fast_caches: self.reference_fast_caches,
            emscripten_memcpy_fast_path_hits: self.emscripten_memcpy_fast_path_hits,
            emscripten_memcpy_raw_i32_hits: self.emscripten_memcpy_raw_i32_hits,
            emscripten_memcpy_early_import_hits: self.emscripten_memcpy_early_import_hits,
            emscripten_memcpy_cache: self.emscripten_memcpy_cache,
            emscripten_memcpy_leaf: self.emscripten_memcpy_leaf,
            emscripten_memcpy_leaf_hits: self.emscripten_memcpy_leaf_hits,
            import_callout_counts: self.import_callout_counts,
            import_callout_count: self.import_callout_count,
            adapter_payload: std::ptr::null_mut(),
        }
    }
}

/// An error, or a panic to resume once compiled frames are gone.
enum Raised {
    Error(JsError),
    Panic(Box<dyn std::any::Any + Send>),
}

const CONTEXT_INTERRUPT: i32 = std::mem::offset_of!(CallContext, interrupt) as i32;
const CONTEXT_STACK_LIMIT: i32 = std::mem::offset_of!(CallContext, stack_limit) as i32;
const CONTEXT_TABLE: i32 = std::mem::offset_of!(CallContext, table) as i32;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const CONTEXT_ENTRY_STACK: u8 = std::mem::offset_of!(CallContext, entry_stack) as u8;
const CONTEXT_GLOBALS: i32 = std::mem::offset_of!(CallContext, globals) as i32;
const CONTEXT_CALLOUT: i32 = std::mem::offset_of!(CallContext, callout) as i32;
const CONTEXT_CALL_REFERENCE: i32 = std::mem::offset_of!(CallContext, call_reference) as i32;
const CONTEXT_MEMORY_LENGTH: i32 = std::mem::offset_of!(CallContext, memory_length) as i32;
const CONTEXT_CALL_REFERENCE_DIAGNOSTIC: i32 =
    std::mem::offset_of!(CallContext, call_reference_diagnostic) as i32;
const CONTEXT_REFERENCE_EPOCH: i32 = std::mem::offset_of!(CallContext, reference_epoch) as i32;
const CONTEXT_ENV: i32 = std::mem::offset_of!(CallContext, env) as i32;
const CONTEXT_REFERENCE_FAST_CACHES: i32 =
    std::mem::offset_of!(CallContext, reference_fast_caches) as i32;
const CONTEXT_EMSCRIPTEN_MEMCPY_CACHE: i32 =
    std::mem::offset_of!(CallContext, emscripten_memcpy_cache) as i32;
const CONTEXT_EMSCRIPTEN_MEMCPY_LEAF: i32 =
    std::mem::offset_of!(CallContext, emscripten_memcpy_leaf) as i32;
const MEMCPY_CACHE_VALID: i32 = std::mem::offset_of!(EmscriptenMemcpyCache, valid) as i32;
const MEMCPY_CACHE_FUNCTION: i32 = std::mem::offset_of!(EmscriptenMemcpyCache, function) as i32;

/// The callout's signature: (context, function index, arguments, result) ->
/// status; arguments and result are Wasm bits, one word each.
type Callout = unsafe extern "C" fn(*mut CallContext, u64, *const u64, *mut u64) -> i64;
type DirectMemcpyLeaf = unsafe extern "C" fn(*mut CallContext, u64, i32, i32, i32) -> i64;
type ReferenceCallout = unsafe extern "C" fn(
    *mut CallContext,
    u64,
    u64,
    u64,
    u64,
    *const u64,
    *mut u64,
    *mut FastReferenceCache,
) -> i64;
type DiagnosticReferenceCallout = ReferenceCallout;

/// A monomorphic target certificate for one same-artifact indirect-call site.
/// The body pointer is owned by this artifact's code mapping. The VM publishes
/// the epoch last after a successful target return and a table-slot recheck;
/// a nonzero epoch is the validity bit.
#[repr(C)]
#[derive(Clone, Copy)]
struct FastReferenceCache {
    epoch: u64,
    env: Value,
    index: u64,
    function: u64,
    body: u64,
}

impl Default for FastReferenceCache {
    fn default() -> Self {
        Self {
            epoch: 0,
            env: Value::NULL,
            index: 0,
            function: 0,
            body: 0,
        }
    }
}

/// Per-top-level-call proof for the guarded Emscripten memcpy leaf. The VM
/// publishes it after a full dynamic check and clears it before host code runs.
#[repr(C)]
#[derive(Default)]
struct EmscriptenMemcpyCache {
    valid: u64,
    function: u64,
    base: usize,
    length: usize,
}

/// Fill an already bounds-checked range in the instance's unshared memory.
/// The compiler checks against the exact current length before calling this
/// leaf, and no VM call can move the guarded memory during the operation.
unsafe extern "C" fn compiled_memory_fill(address: u64, byte: u64, length: u64) -> i64 {
    // Wasm32 operands are zero-extended before this call; they fit every
    // supported host's address size.
    unsafe {
        std::ptr::write_bytes(address as usize as *mut u8, byte as u8, length as usize);
    }
    0
}

/// The dispatcher-free second-call path for the exact Emscripten helper.
/// Its memory proof is scoped to one native invocation and ordinary callouts
/// revoke it before guest JavaScript can mutate a view or grow memory.
unsafe extern "C" fn compiled_emscripten_memcpy_leaf(
    context: *mut CallContext,
    function: u64,
    destination: i32,
    source: i32,
    length: i32,
) -> i64 {
    if context.is_null() {
        return MEMCPY_LEAF_MISS_STATUS;
    }
    // SAFETY: generated code passes its live context, and its cache points to
    // storage owned by the suspended `run_compiled` activation.
    let context = unsafe { &mut *context };
    if context.emscripten_memcpy_cache.is_null() {
        return MEMCPY_LEAF_MISS_STATUS;
    }
    let cache = unsafe { &mut *context.emscripten_memcpy_cache };
    if cache.valid != 1 || cache.function != function || cache.base == 0 {
        cache.valid = 0;
        return MEMCPY_LEAF_MISS_STATUS;
    }
    if !context.interrupt.is_null()
        && unsafe { &*context.interrupt }.load(std::sync::atomic::Ordering::Acquire)
    {
        return TERMINATED_STATUS;
    }

    let target = super::wasm_js::copy_within_relative_index(i64::from(destination), cache.length);
    let start = super::wasm_js::copy_within_relative_index(i64::from(source), cache.length);
    let raw_end = i64::from(source) + i64::from(length);
    let end = super::wasm_js::copy_within_relative_index(raw_end, cache.length);
    let count = end.saturating_sub(start).min(cache.length - target);
    if count > 0 {
        // SAFETY: the published certificate names the current full-length
        // unshared Wasm view. All indices are clamped to that allocation.
        unsafe {
            std::ptr::copy(
                (cache.base as *const u8).add(start),
                (cache.base as *mut u8).add(target),
                count,
            );
        }
    }
    if !context.emscripten_memcpy_leaf_hits.is_null() {
        unsafe {
            let hits = &mut *context.emscripten_memcpy_leaf_hits;
            *hits = hits.saturating_add(1);
        }
    }
    0
}

fn note_compiled_callout(counter: *mut u64) {
    if !counter.is_null() {
        // SAFETY: only diagnostic-enabled calls provide this pointer, and it
        // points to a counter owned by the suspended native invocation.
        unsafe {
            *counter = (*counter).saturating_add(1);
        }
    }
}

fn native_entry_sample_due(entry_count: u64) -> bool {
    entry_count > 0 && entry_count % JIT_NATIVE_ENTRY_SAMPLE_INTERVAL == 0
}

unsafe fn note_import_callout_target(
    counters: *const std::cell::Cell<u64>,
    counter_count: usize,
    function: u64,
) {
    let Ok(index) = usize::try_from(function) else {
        return;
    };
    if counters.is_null() || index >= counter_count {
        return;
    }
    // SAFETY: diagnostic contexts point at `counter_count` initialized Cells
    // owned by their CompiledProgram, which remains live across the callout.
    let counter = unsafe { &*counters.add(index) };
    counter.set(counter.get().saturating_add(1));
}

fn json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0C}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character <= '\u{1F}' => {
                use std::fmt::Write;
                write!(escaped, "\\u{:04x}", character as u32)
                    .expect("writing to a String cannot fail");
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn format_import_callout_targets(
    names: &[(u32, u32, String, String)],
    counters: &[std::cell::Cell<u64>],
) -> String {
    debug_assert_eq!(names.len(), counters.len());
    names
        .iter()
        .zip(counters)
        .filter_map(|((function, import_index, module, name), count)| {
            let count = count.get();
            (count > 0).then(|| {
                format!(
                    "{{\"function\":{function},\"import_index\":{import_index},\"module\":{},\"name\":{},\"count\":{count}}}",
                    json_string(module),
                    json_string(name)
                )
            })
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[inline]
fn invalidate_emscripten_memcpy_cache(cache: *mut EmscriptenMemcpyCache) {
    if !cache.is_null() {
        // SAFETY: the pointer is the live cache owned by the suspended native
        // invocation, and callout entry runs synchronously on that invocation.
        unsafe { (*cache).valid = 0 };
    }
}

#[derive(Clone, Copy, Default)]
struct ReferenceCallSiteCounts {
    total: u64,
    native: u64,
    fallback: u64,
    same_artifact_direct: u64,
}

#[derive(Clone, Copy, Default)]
struct ReferenceCallTargetCounts {
    function: usize,
    total: u64,
    native: u64,
    same_artifact_native: u64,
}

struct ReferenceCallSiteDiagnostics {
    function: usize,
    pc: usize,
    counts: std::cell::Cell<ReferenceCallSiteCounts>,
    targets: std::cell::RefCell<Vec<ReferenceCallTargetCounts>>,
}

fn note_reference_call_site(
    context: &CallContext,
    site: u64,
    native: bool,
    same_artifact_direct: bool,
    target_function: Option<usize>,
    target_same_artifact_native: bool,
) {
    let Ok(site) = usize::try_from(site) else {
        return;
    };
    if context.reference_call_sites.is_null() || site >= context.reference_call_site_count {
        return;
    }
    // SAFETY: diagnostic call sites are a stable vector owned by the compiled
    // program for the full native invocation, and generated ids are assigned
    // from that vector at compile time.
    let counts = unsafe { &*context.reference_call_sites.add(site) };
    let mut updated = counts.counts.get();
    updated.total = updated.total.saturating_add(1);
    if native {
        updated.native = updated.native.saturating_add(1);
    } else {
        updated.fallback = updated.fallback.saturating_add(1);
    }
    if same_artifact_direct {
        updated.same_artifact_direct = updated.same_artifact_direct.saturating_add(1);
    }
    counts.counts.set(updated);
    if let Some(function) = target_function {
        let mut targets = counts.targets.borrow_mut();
        let target_index = if let Some(target_index) = targets
            .iter()
            .position(|target| target.function == function)
        {
            target_index
        } else {
            targets.push(ReferenceCallTargetCounts {
                function,
                ..ReferenceCallTargetCounts::default()
            });
            targets.len() - 1
        };
        let target = &mut targets[target_index];
        target.total = target.total.saturating_add(1);
        if native {
            target.native = target.native.saturating_add(1);
        }
        if native && target_same_artifact_native {
            target.same_artifact_native = target.same_artifact_native.saturating_add(1);
        }
    }
}

fn refresh_memory_length<H: Host>(context: &mut CallContext, vm: &Vm<H>) {
    if context.memory != Value::UNDEFINED
        && let Some((_, length)) = super::dispatch_fast::unshared_memory(vm, context.memory)
    {
        context.memory_length = length as u64;
    }
}

fn finish_callout(
    context: &mut CallContext,
    called: Result<Result<(), JsError>, Box<dyn std::any::Any + Send>>,
) -> i64 {
    match called {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            context.raised = Some(Raised::Error(error));
            RAISED_STATUS
        }
        Err(panic) => {
            context.raised = Some(Raised::Panic(panic));
            RAISED_STATUS
        }
    }
}

/// Resolve, check, and invoke an indirect table target in one VM callout.
unsafe extern "C" fn reference_callout<H: Host>(
    context: *mut CallContext,
    table_slot: u64,
    index: u64,
    signature_index: u64,
    site: u64,
    args: *const u64,
    out: *mut u64,
    fast_cache: *mut FastReferenceCache,
) -> i64 {
    // A stable call-site id is present even without diagnostics so the VM can
    // find the per-site monomorphic cache without a hash lookup.
    unsafe {
        reference_callout_inner::<H, false>(
            context,
            table_slot,
            index,
            signature_index,
            site,
            args,
            out,
            fast_cache,
        )
    }
}

/// Diagnostic-only reference callout. Its extra site id is emitted only when
/// Wasm JIT diagnostics were enabled at compilation time.
unsafe extern "C" fn diagnostic_reference_callout<H: Host>(
    context: *mut CallContext,
    table_slot: u64,
    index: u64,
    signature_index: u64,
    site: u64,
    args: *const u64,
    out: *mut u64,
    fast_cache: *mut FastReferenceCache,
) -> i64 {
    unsafe {
        reference_callout_inner::<H, true>(
            context,
            table_slot,
            index,
            signature_index,
            site,
            args,
            out,
            fast_cache,
        )
    }
}

/// Shared implementation, monomorphized so ordinary callouts have no
/// diagnostic counter branch or extra site lookup.
#[inline(always)]
unsafe fn reference_callout_inner<H: Host, const DIAGNOSTICS: bool>(
    context: *mut CallContext,
    table_slot: u64,
    index: u64,
    signature_index: u64,
    site: u64,
    args: *const u64,
    out: *mut u64,
    fast_cache: *mut FastReferenceCache,
) -> i64 {
    // SAFETY: generated code passes the context, table binding, table index,
    // signature index, argument words, and output storage for this call.
    let context = unsafe { &mut *context };
    // Resolving or invoking a reference can execute arbitrary JS, enter an
    // interpreted body, or grow the active Wasm memory. Revoke the raw view
    // certificate before any of those paths can run.
    invalidate_emscripten_memcpy_cache(context.emscripten_memcpy_cache);
    note_compiled_callout(context.reference_callouts);
    let vm_pointer = context.vm.cast::<Vm<H>>();
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    let program = context.program;
    let mut native_target_hit = false;
    let mut same_artifact_direct_hit = false;
    let mut target_same_artifact_native = false;
    let mut reference_target_function = None;
    let mut fast_target: Option<(usize, Value, Value, u32, u64, u64)> = None;
    let mut direct_target_root = None;
    let mut direct_status = None;
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (
            callee,
            params,
            result_type,
            same_program_function,
            direct_entry,
            cache_hit,
            table,
            table_slot,
            signature_index,
        ) = {
            // Do all heap/program inspection before entering native code. The
            // VM is re-entered through callouts, so no `&mut Vm` may live
            // across that boundary.
            let vm = unsafe { &mut *vm_pointer };
            let table_slot = u32::try_from(table_slot)
                .map_err(|_| JsError::validation("invalid Wasm table binding".into()))?;
            let signature_index = u32::try_from(signature_index)
                .map_err(|_| JsError::validation("invalid Wasm signature index".into()))?;
            let table = vm
                .heap
                .environment_slot(context.env, table_slot as usize)
                .ok_or_else(|| JsError::validation("invalid Wasm table binding".into()))?;
            let cached = usize::try_from(site).ok().and_then(|site| {
                cached_direct_reference(
                    vm,
                    context,
                    table_slot,
                    table,
                    index,
                    signature_index,
                    site,
                )
            });
            if let Some((callee, direct_entry)) = cached {
                (
                    callee,
                    smallvec::SmallVec::new(),
                    None,
                    None,
                    Some(direct_entry),
                    true,
                    table,
                    table_slot,
                    signature_index,
                )
            } else {
                let callee = vm.wasm_indirect_target(program, table, index, signature_index)?;
                let (params, result_type) = {
                    let signature = vm
                        .programs
                        .wasm_signatures(program)
                        .and_then(|signatures| signatures.type_signature(signature_index))
                        .ok_or_else(|| {
                            JsError::validation("invalid Wasm signature index".into())
                        })?;
                    (
                        signature
                            .params
                            .iter()
                            .copied()
                            .collect::<smallvec::SmallVec<[crate::WasmType; 8]>>(),
                        signature.results.first().copied(),
                    )
                };
                let same_program_function = match vm.heap.function(callee) {
                    Some(FunctionRecord {
                        kind: crate::heap::FunctionKind::User(target_program, function),
                        env,
                        ..
                    }) if *target_program == program => Some((*target_program, *function, *env)),
                    _ => None,
                };
                let direct_entry = same_program_function
                    .filter(|(target_program, _, target_env)| {
                        *target_program == program && *target_env == context.env
                    })
                    .and_then(|(_, function, _)| {
                        let compiled = compiled_reference_target(
                            vm,
                            program,
                            function as usize,
                            context.compiled,
                        )?;
                        direct_reference_entry(
                            vm,
                            compiled,
                            function as usize,
                            context.env,
                            context.memory,
                            Some((&params, result_type)),
                        )
                    });
                (
                    callee,
                    params,
                    result_type,
                    same_program_function,
                    direct_entry,
                    false,
                    table,
                    table_slot,
                    signature_index,
                )
            }
        };
        if DIAGNOSTICS {
            reference_target_function = direct_entry.as_ref().map(|entry| entry.1).or_else(|| {
                same_program_function
                    .as_ref()
                    .map(|(_, function, _)| *function as usize)
            });
        }
        if let Some((compiled, function, entry, (base, length), memory_binding, guard, globals)) =
            direct_entry
        {
            // A re-entrant import can mutate the table and collect while this
            // body is running. Keep its function cell alive until the call
            // returns so a freed slot cannot be reused before cache publish.
            let callee_root = unsafe { &mut *vm_pointer }.heap.root(callee);
            direct_target_root = Some(callee_root);
            if !cache_hit
                && let Ok(site) = usize::try_from(site)
                && !context.compiled.is_null()
            {
                let target_weak = unsafe { &*vm_pointer }.heap.weak_handle(callee);
                if let Some(target_weak) = target_weak {
                    let caller = unsafe { &*context.compiled };
                    caller.cache_indirect_call(
                        site,
                        IndirectCallCacheEntry {
                            table_slot,
                            table,
                            index,
                            signature_index,
                            target: callee,
                            target_weak,
                            program,
                            function: function as u32,
                            env: context.env,
                            compiled: Rc::downgrade(&compiled),
                            hits: 0,
                        },
                    );
                }
            }
            let native_entry_should_sample = compiled.note_direct_reference_entry(function);
            let mut nested = context.nested_entry();
            nested.compiled = Rc::as_ptr(&compiled);
            nested.reference_fast_caches = compiled.reference_fast_caches_ptr();
            nested.table = compiled.table.as_ptr();
            nested.globals = globals.as_ptr();
            nested.memory = memory_binding;
            nested.memory_length = length;
            let (guard_start, guard_end) = guard.map_or((0, 0), |(start, size)| {
                (start as usize, start as usize + size)
            });
            nested.guard_start = guard_start;
            nested.guard_end = guard_end;
            nested.call_reference_diagnostic = compiled.diagnostics.as_ref().map_or(0, |_| {
                diagnostic_reference_callout::<H> as DiagnosticReferenceCallout as usize
            });
            nested.reference_call_sites = compiled
                .diagnostics
                .as_ref()
                .map_or(std::ptr::null(), |stats| {
                    stats.reference_call_sites.as_ptr()
                });
            nested.reference_call_site_count = compiled
                .diagnostics
                .as_ref()
                .map_or(0, |stats| stats.reference_call_sites.len());
            // SAFETY: the table/signature resolver ran first; same program,
            // exact instance environment, and exact scalar signature were
            // checked above. The caller's raw argument/result buffers have
            // the Wasm ABI shape expected by this compiled entry, and `compiled`
            // keeps its executable mapping alive for the duration of the call.
            native_target_hit = true;
            let same_artifact = !context.compiled.is_null()
                && std::ptr::eq(context.compiled, Rc::as_ptr(&compiled));
            if same_artifact
                && let Some(body) = compiled
                    .table
                    .get(function)
                    .copied()
                    .filter(|body| *body != 0)
            {
                fast_target = Some((function, callee, table, table_slot, index, body as u64));
            }
            if DIAGNOSTICS {
                same_artifact_direct_hit = same_artifact;
                target_same_artifact_native = same_artifact_direct_hit;
            }
            let native_entry_sample_started =
                native_entry_should_sample.then(std::time::Instant::now);
            // SAFETY: as the direct path's call; the nested context outlives the call.
            let status = unsafe {
                enter_native(
                    &mut nested,
                    base,
                    length,
                    args,
                    out,
                    NativeEntryMode::Resumable,
                    entry,
                )
            };
            if let Some(started) = native_entry_sample_started {
                compiled.finish_native_entry_sample(function, started);
            }
            if status == RAISED_STATUS as u32 {
                match nested.raised.take() {
                    Some(Raised::Error(error)) => return Err(error),
                    Some(Raised::Panic(panic)) => std::panic::resume_unwind(panic),
                    None => {
                        return Err(JsError::validation(
                            "compiled indirect callee raised without an error".into(),
                        ));
                    }
                }
            }
            direct_status = Some(status);
            return Ok(());
        }

        let vm = unsafe { &mut *vm_pointer };
        let mut values: smallvec::SmallVec<[Value; 8]> = smallvec::SmallVec::new();
        for (offset, ty) in params.iter().enumerate() {
            let kind = Kind::of(*ty)
                .ok_or_else(|| JsError::validation("unsupported Wasm call parameter".into()))?;
            // SAFETY: the generated call frame stores one word per parameter.
            let bits = unsafe { *args.add(offset) };
            values.push(vm.encode_wasm_value(crate::WasmValue::from_bits(
                *ty,
                match kind {
                    Kind::I32 => u64::from(bits as u32),
                    Kind::I64 => bits,
                },
            )));
        }
        let compiled_result = same_program_function.and_then(|(target_program, function, env)| {
            vm.run_compiled(target_program, env, function, &values)
        });
        native_target_hit = compiled_result.is_some();
        if DIAGNOSTICS
            && native_target_hit
            && let Some(function) = reference_target_function
        {
            let target_compiled =
                compiled_reference_target(vm, program, function, context.compiled);
            target_same_artifact_native = !context.compiled.is_null()
                && target_compiled
                    .as_ref()
                    .is_some_and(|compiled| std::ptr::eq(context.compiled, Rc::as_ptr(compiled)));
        }
        let result = match compiled_result {
            Some(result) => result?,
            None => {
                let code = vm
                    .programs
                    .get(program)
                    .ok_or_else(|| JsError::validation("missing Wasm program".into()))?;
                let receiver = vm
                    .programs
                    .constant(program, VOID_RESULT_CONSTANT)
                    .ok_or_else(|| JsError::validation("missing Wasm void constant".into()))?;
                vm.call_value(&code, callee, receiver, &values)?
            }
        };
        let bits = match result_type {
            Some(ty) => match vm
                .decode_wasm_value(result, ty)?
                .bits()
                .ok_or_else(|| JsError::validation("non-scalar Wasm call result".into()))?
            {
                crate::wasm::ScalarBits::Bits32(bits) => u64::from(bits),
                crate::wasm::ScalarBits::Bits64(bits) => bits,
                crate::wasm::ScalarBits::Bits128(_) => {
                    return Err(JsError::validation("non-scalar Wasm call result".into()));
                }
            },
            None => 0,
        };
        // SAFETY: `out` points into the result word emitted for this helper.
        unsafe { *out = bits };
        Ok(())
    }));
    ACTIVE_CONTEXT.with(|current| current.set(active));
    let vm = unsafe { &mut *vm_pointer };
    if let Some((function, target, table, table_slot, index, body)) = fast_target {
        let target_still_matches = direct_target_root
            .and_then(|root| vm.heap.root_value(root))
            .is_some_and(|rooted| {
                rooted == target
                    && reference_target_matches(vm, target, program, function as u32, context.env)
            });
        if !fast_cache.is_null()
            && direct_status == Some(0)
            && called.as_ref().is_ok_and(|result| result.is_ok())
            && target_still_matches
        {
            let table_binding = vm.heap.environment_slot(context.env, table_slot as usize);
            let current_target = usize::try_from(index)
                .ok()
                .and_then(|index| vm.wasm_table_elements(table).ok()?.get(index).copied());
            if table_binding == Some(table) && current_target == Some(target) {
                // SAFETY: generated code passes a correctly aligned pointer to
                // this site's UnsafeCell in the owning artifact. The artifact is
                // retained throughout the call and native code is suspended until
                // the callout returns.
                let cache = unsafe { &mut *fast_cache };
                cache.epoch = 0;
                cache.env = context.env;
                cache.index = index;
                cache.function = function as u64;
                cache.body = body;
                cache.epoch = vm.wasm_reference_epoch.get();
            }
        }
    }
    if let Some(root) = direct_target_root {
        vm.heap.release_root(root);
    }
    if DIAGNOSTICS {
        note_reference_call_site(
            context,
            site,
            native_target_hit,
            same_artifact_direct_hit,
            reference_target_function,
            target_same_artifact_native,
        );
    }
    refresh_memory_length(context, vm);
    direct_status.map_or_else(|| finish_callout(context, called), i64::from)
}

/// Make imported call `function` for compiled code, as the interpreter's
/// `Op::Call` does.
unsafe extern "C" fn import_callout<H: Host>(
    context: *mut CallContext,
    function: u64,
    args: *const u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context `run_compiled` made, whose
    // VM is suspended in that call.
    let context = unsafe { &mut *context };
    let memcpy_cache = context.emscripten_memcpy_cache;
    // Host code can replace the captured view/method or grow memory. The fast
    // certificate cannot survive any ordinary callout.
    invalidate_emscripten_memcpy_cache(memcpy_cache);
    let vm = unsafe { &mut *context.vm.cast::<Vm<H>>() };
    let defined = vm
        .programs
        .wasm_signatures(context.program)
        .and_then(|signatures| signatures.defined_index(function as u32))
        .is_some();
    note_compiled_callout(if defined {
        context.known_callouts
    } else {
        context.import_callouts
    });
    if !defined && !context.import_callout_counts.is_null() {
        // SAFETY: this pointer is non-null only when the owning compiled
        // program enabled diagnostic counters for the full native call.
        unsafe {
            note_import_callout_target(
                context.import_callout_counts,
                context.import_callout_count,
                function,
            );
        }
    }
    // The VM's own code runs outside compiled code: no guard fault is ours.
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    let program = context.program;
    let env = context.env;
    let current_memory = context.memory;
    let memcpy_fast_path_hits = context.emscripten_memcpy_fast_path_hits;
    let memcpy_raw_i32_hits = context.emscripten_memcpy_raw_i32_hits;
    let memcpy_early_import_hits = context.emscripten_memcpy_early_import_hits;
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: compiled code passes one word per parameter and a result
        // word.
        if unsafe {
            vm.try_early_compiled_emscripten_memcpy_import(
                program,
                env,
                function,
                current_memory,
                memcpy_raw_i32_hits,
                memcpy_early_import_hits,
                args,
            )
        } {
            if jit_emscripten_memcpy_leaf_enabled()
                && !memcpy_cache.is_null()
                && let Some((base, length)) =
                    super::dispatch_fast::unshared_memory(vm, current_memory)
            {
                // Publish validity last after the helper has checked the
                // import binding, live view, method, and exact memory object.
                unsafe {
                    (*memcpy_cache).function = function;
                    (*memcpy_cache).base = base as usize;
                    (*memcpy_cache).length = length;
                    (*memcpy_cache).valid = 1;
                }
            }
            Ok(())
        } else {
            unsafe {
                vm.call_import(
                    program,
                    env,
                    function as u32,
                    current_memory,
                    memcpy_fast_path_hits,
                    memcpy_raw_i32_hits,
                    args,
                    out,
                )
            }
        }
    }));
    ACTIVE_CONTEXT.with(|current| current.set(active));
    refresh_memory_length(context, vm);
    finish_callout(context, called)
}

/// A compiled function's entry: (context, memory base, memory length,
/// arguments, result) -> status.
type Entry = unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, *mut u64) -> u32;

/// The shared native-entry ABI used by compiled Wasm functions and JS OSR regions:
/// (context, memory base, memory length, arguments or region inputs, result word) -> status.
fn native_entry_signature(
    pointer: ir::Type,
    call_conv: cranelift_codegen::isa::CallConv,
) -> Signature {
    let mut signature = Signature::new(call_conv);
    for parameter in [pointer, types::I64, types::I64, pointer, pointer] {
        signature.params.push(AbiParam::new(parameter));
    }
    signature.returns.push(AbiParam::new(types::I32));
    signature
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NativeEntryMode {
    /// Use the signal-resuming entry, required by guarded memory and poll-page faults.
    Resumable,
    /// Use the same ABI directly for a bounded leaf with no possible native fault.
    Leaf,
}

enum CompiledArgs<'a> {
    Values(&'a [Value]),
    RegisterWindow {
        caller: usize,
        base: usize,
        count: usize,
    },
}

pub(super) struct CompiledFunction {
    entry: Entry,
    params: Vec<crate::WasmType>,
    result: Option<crate::WasmType>,
    #[cfg(test)]
    native_entries: std::cell::Cell<u64>,
    #[cfg(test)]
    direct_reference_entries: std::cell::Cell<u64>,
}

/// A program's compiled functions, sharing one code mapping and one call
/// table.
#[derive(Clone)]
struct IndirectCallCacheEntry {
    table_slot: u32,
    table: Value,
    index: u64,
    signature_index: u32,
    target: Value,
    target_weak: crate::heap::WeakHandle,
    program: ProgramId,
    function: u32,
    env: Value,
    compiled: Weak<CompiledProgram>,
    hits: u64,
}

type DirectReferenceEntry = (
    Rc<CompiledProgram>,
    usize,
    Entry,
    (u64, u64),
    Value,
    Option<(*mut u8, usize)>,
    smallvec::SmallVec<[*mut Value; 8]>,
);

/// Build the per-invocation activation for a same-instance compiled indirect
/// target. Even a monomorphic cache hit comes through here so memory, guards,
/// and mutable global addresses always reflect the current instance state.
fn direct_reference_entry<H: Host>(
    vm: &mut Vm<H>,
    compiled: Rc<CompiledProgram>,
    function: usize,
    env: Value,
    context_memory: Value,
    signature: Option<(&[crate::WasmType], Option<crate::WasmType>)>,
) -> Option<DirectReferenceEntry> {
    let native = compiled.functions.get(function)?.as_ref()?;
    if signature.is_some_and(|(params, result)| {
        native.params.as_slice() != params || native.result != result
    }) {
        return None;
    }
    let entry = native.entry;
    let (memory, memory_binding, guard) = match compiled.memory_slot {
        None => ((0, 0), Value::UNDEFINED, None),
        Some(slot) => {
            let binding = vm.heap.environment_slot(env, slot as usize)?;
            if binding != context_memory {
                return None;
            }
            let (base, length) = super::dispatch_fast::unshared_memory(vm, binding)?;
            let guard = guarded_memory(vm, binding);
            if compiled.guarded && guard.is_none()
                || compiled.fixed_base.is_some_and(|fixed| {
                    guard.map(|(base, _)| displacement_base(base)) != Some(Some(fixed))
                })
            {
                return None;
            }
            ((base as u64, length as u64), binding, guard)
        }
    };
    let mut globals: smallvec::SmallVec<[*mut Value; 8]> = smallvec::SmallVec::new();
    for slot in &compiled.globals {
        let binding = vm.heap.environment_slot(env, *slot as usize)?;
        match vm.heap.get_mut(binding)? {
            Cell::WasmGlobal { value, .. } => globals.push(value),
            _ => return None,
        }
    }
    Some((
        compiled,
        function,
        entry,
        memory,
        memory_binding,
        guard,
        globals,
    ))
}

/// Pick a compiled artifact for an indirect target, preferring the artifact
/// that is already executing when it contains the target. This is the case
/// after profile-guided selective promotion closes over a hot indirect callee.
fn compiled_reference_target<H: Host>(
    vm: &Vm<H>,
    program: ProgramId,
    function: usize,
    caller: *const CompiledProgram,
) -> Option<Rc<CompiledProgram>> {
    let current_artifact = if caller.is_null() {
        None
    } else {
        vm.programs
            .compiled(program)
            .flatten()
            .filter(|compiled| std::ptr::eq(Rc::as_ptr(compiled), caller))
            .filter(|compiled| compiled.has_function(function))
            .or_else(|| {
                u32::try_from(function).ok().and_then(|function| {
                    vm.programs.wasm_tier_for_artifact_containing(
                        program,
                        function as usize,
                        caller,
                    )
                })
            })
    };
    current_artifact
        .or_else(|| {
            vm.programs
                .compiled(program)
                .flatten()
                .filter(|compiled| compiled.has_function(function))
        })
        .or_else(|| {
            u32::try_from(function)
                .ok()
                .and_then(|function| vm.programs.wasm_tier_containing(program, function))
        })
}

/// Check the cache's guards without turning any miss into a Wasm trap. The
/// ordinary resolver below remains authoritative for OOB, null, and mismatch
/// cases, and for artifacts that cannot currently activate the cached target.
fn cached_direct_reference<H: Host>(
    vm: &mut Vm<H>,
    context: &CallContext,
    table_slot: u32,
    table: Value,
    index: u64,
    signature_index: u32,
    site: usize,
) -> Option<(Value, DirectReferenceEntry)> {
    if context.compiled.is_null() {
        return None;
    }
    let caller = unsafe { &*context.compiled };
    let cache = caller.indirect_call_cache(site)?;
    if cache.table_slot != table_slot
        || cache.table != table
        || cache.index != index
        || cache.signature_index != signature_index
        || cache.program != context.program
        || cache.env != context.env
    {
        return None;
    }
    let element = usize::try_from(index)
        .ok()
        .and_then(|index| vm.wasm_table_elements(table).ok()?.get(index).copied())?;
    if element != cache.target || vm.heap.weak_value(cache.target_weak) != Some(element) {
        return None;
    }
    match vm.heap.function(element) {
        Some(FunctionRecord {
            kind: crate::heap::FunctionKind::User(program, function),
            env,
            ..
        }) if *program == cache.program && *function == cache.function && *env == cache.env => {}
        _ => return None,
    }
    caller.note_indirect_cache_hit(site);
    let compiled = cache.compiled.upgrade()?;
    let direct = direct_reference_entry(
        vm,
        compiled,
        cache.function as usize,
        context.env,
        context.memory,
        None,
    )?;
    #[cfg(test)]
    caller
        .reference_cache_hits
        .set(caller.reference_cache_hits.get().saturating_add(1));
    Some((element, direct))
}

fn reference_target_matches<H: Host>(
    vm: &Vm<H>,
    target: Value,
    program: ProgramId,
    function: u32,
    env: Value,
) -> bool {
    matches!(
        vm.heap.function(target),
        Some(FunctionRecord {
            kind: crate::heap::FunctionKind::User(target_program, target_function),
            env: target_env,
            ..
        }) if *target_program == program && *target_function == function && *target_env == env
    )
}

pub(super) struct CompiledProgram {
    _code: CodeMemory,
    table: Box<[usize]>,
    functions: Vec<Option<CompiledFunction>>,
    /// The environment slot of the memory compiled code accesses.
    memory_slot: Option<u32>,
    /// Memory accesses rely on the memory's guard reservation instead of
    /// bounds checks.
    guarded: bool,
    /// The guarded memory's base, when code addresses it as a constant
    /// displacement (see `displacement_base`); calls with another base fall
    /// back to the interpreter.
    fixed_base: Option<i32>,
    /// The environment slots of the globals compiled code accesses, in the
    /// order of the context's global addresses.
    globals: Vec<u32>,
    diagnostics: Option<WasmJitDiagnostics>,
    /// One optional monomorphic cache per actual compiled `call_indirect`
    /// site. The vector is only pointers until a site becomes hot, and each
    /// cache weakly references its callee artifact to avoid Rc cycles.
    reference_call_caches: std::cell::RefCell<Vec<Option<Box<IndirectCallCacheEntry>>>>,
    /// Stable per-site target certificates for native direct dispatch. These
    /// do not retain heap values and are invalidated by the VM epoch.
    reference_fast_caches: Box<[UnsafeCell<FastReferenceCache>]>,
    /// The owner and bytecode PC for each entry in `reference_call_caches`.
    reference_cache_sites: Vec<(usize, usize, u32)>,
    #[cfg(test)]
    reference_cache_hits: std::cell::Cell<u64>,
}

/// Counters allocated only for opt-in benchmark diagnostics. These runs are
/// intended to explain coverage; their timings are not qualification scores.
struct WasmJitDiagnostics {
    program: u32,
    total_functions: usize,
    compiled_functions: usize,
    code_bytes: usize,
    mapped_bytes: usize,
    compile_ns: u128,
    native_entry_requests: Vec<std::cell::Cell<u64>>,
    /// Deterministic 1-in-256 samples of inclusive native-entry wall time.
    native_entry_samples: Vec<std::cell::Cell<u64>>,
    /// Sum of sampled elapsed nanoseconds; nested native calls may overlap.
    native_entry_sampled_ns: Vec<std::cell::Cell<u64>>,
    interpreter_fallback_entries: Vec<std::cell::Cell<u64>>,
    compiled_to_vm_callouts: std::cell::Cell<u64>,
    compiled_import_callouts: std::cell::Cell<u64>,
    compiled_known_callouts: std::cell::Cell<u64>,
    compiled_reference_callouts: std::cell::Cell<u64>,
    emscripten_memcpy_fast_path_hits: std::cell::Cell<u64>,
    emscripten_memcpy_raw_i32_hits: std::cell::Cell<u64>,
    emscripten_memcpy_early_import_hits: std::cell::Cell<u64>,
    emscripten_memcpy_leaf_hits: std::cell::Cell<u64>,
    import_callout_names: Vec<(u32, u32, String, String)>,
    import_callout_counts: Vec<std::cell::Cell<u64>>,
    reference_call_sites: Vec<ReferenceCallSiteDiagnostics>,
}

impl Drop for CompiledProgram {
    fn drop(&mut self) {
        let Some(stats) = &self.diagnostics else {
            return;
        };
        let entries = stats
            .native_entry_requests
            .iter()
            .enumerate()
            .filter_map(|(function, count)| {
                let count = count.get();
                (count > 0).then(|| format!("[{function},{count}]"))
            })
            .collect::<Vec<_>>()
            .join(",");
        let native_entry_samples = stats
            .native_entry_samples
            .iter()
            .zip(&stats.native_entry_sampled_ns)
            .enumerate()
            .filter_map(|(function, (samples, elapsed_ns))| {
                let samples = samples.get();
                let elapsed_ns = elapsed_ns.get();
                (samples > 0).then(|| format!("[{function},{samples},{elapsed_ns}]"))
            })
            .collect::<Vec<_>>()
            .join(",");
        let fallbacks = stats
            .interpreter_fallback_entries
            .iter()
            .enumerate()
            .filter_map(|(function, count)| {
                let count = count.get();
                (count > 0).then(|| format!("[{function},{count}]"))
            })
            .collect::<Vec<_>>()
            .join(",");
        let reference_sites = stats
            .reference_call_sites
            .iter()
            .filter_map(|site| {
                let counts = site.counts.get();
                (counts.total > 0).then(|| {
                    format!(
                        "[{},{},{},{},{}]",
                        site.function, site.pc, counts.total, counts.native, counts.fallback
                    )
                })
            })
            .collect::<Vec<_>>()
            .join(",");
        let mut reference_targets = Vec::new();
        for site in &stats.reference_call_sites {
            for target in site.targets.borrow().iter() {
                if target.total > 0 {
                    reference_targets.push(format!(
                        "[{},{},{},{},{},{}]",
                        site.function,
                        site.pc,
                        target.function,
                        target.total,
                        target.native,
                        target.same_artifact_native
                    ));
                }
            }
        }
        let reference_targets = reference_targets.join(",");
        let import_callout_targets = format_import_callout_targets(
            &stats.import_callout_names,
            &stats.import_callout_counts,
        );
        let attributed_import_callouts = stats
            .import_callout_counts
            .iter()
            .map(std::cell::Cell::get)
            .fold(0u64, u64::saturating_add);
        debug_assert_eq!(
            attributed_import_callouts,
            stats.compiled_import_callouts.get(),
            "per-import attribution must account for every compiled import callout"
        );
        let same_artifact_direct_hits = stats
            .reference_call_sites
            .iter()
            .map(|site| site.counts.get().same_artifact_direct)
            .fold(0u64, u64::saturating_add);
        eprintln!(
            "QUENCH_WASM_JIT_STATS {{\"program\":{},\"compile_attempts\":1,\"compiled_functions\":{},\"total_functions\":{},\"code_bytes\":{},\"mapped_bytes\":{},\"compile_ns\":{},\"native_entry_requests\":[{}],\"native_entry_sampling_interval\":{},\"native_entry_samples\":[{}],\"interpreter_fallback_entries\":[{}],\"compiled_to_vm_callouts\":{},\"compiled_import_callouts\":{},\"compiled_known_callouts\":{},\"compiled_reference_callouts\":{},\"import_callout_targets\":[{}],\"emscripten_memcpy_direct_copy_switch\":\"{}\",\"emscripten_memcpy_direct_copy_enabled\":{},\"emscripten_memcpy_fast_path_hits\":{},\"emscripten_memcpy_raw_i32_switch\":\"{}\",\"emscripten_memcpy_raw_i32_enabled\":{},\"emscripten_memcpy_raw_i32_hits\":{},\"emscripten_memcpy_early_import_switch\":\"{}\",\"emscripten_memcpy_early_import_enabled\":{},\"emscripten_memcpy_early_import_hits\":{},\"emscripten_memcpy_leaf_switch\":\"{}\",\"emscripten_memcpy_leaf_hits\":{},\"reference_call_sites\":[{}],\"same_artifact_direct_reference_hits\":{},\"reference_call_targets\":[{}]}}",
            stats.program,
            stats.compiled_functions,
            stats.total_functions,
            stats.code_bytes,
            stats.mapped_bytes,
            stats.compile_ns,
            entries,
            JIT_NATIVE_ENTRY_SAMPLE_INTERVAL,
            native_entry_samples,
            fallbacks,
            stats.compiled_to_vm_callouts.get(),
            stats.compiled_import_callouts.get(),
            stats.compiled_known_callouts.get(),
            stats.compiled_reference_callouts.get(),
            import_callout_targets,
            super::wasm_js::emscripten_memcpy_direct_copy_switch(),
            super::wasm_js::emscripten_memcpy_direct_copy_enabled(),
            stats.emscripten_memcpy_fast_path_hits.get(),
            super::wasm_js::emscripten_memcpy_raw_i32_switch(),
            super::wasm_js::emscripten_memcpy_raw_i32_enabled(),
            stats.emscripten_memcpy_raw_i32_hits.get(),
            super::wasm_js::emscripten_memcpy_early_import_switch(),
            super::wasm_js::emscripten_memcpy_early_import_enabled(),
            stats.emscripten_memcpy_early_import_hits.get(),
            if jit_emscripten_memcpy_leaf_enabled() {
                "1"
            } else {
                "0"
            },
            stats.emscripten_memcpy_leaf_hits.get(),
            reference_sites,
            same_artifact_direct_hits,
            reference_targets,
        );
    }
}

fn jit_diagnostics_enabled() -> bool {
    std::env::var_os("QUENCH_WASM_JIT_STATS").is_some_and(|value| value == "1")
}

impl CompiledProgram {
    fn reference_fast_caches_ptr(&self) -> *mut FastReferenceCache {
        self.reference_fast_caches
            .first()
            .map_or(std::ptr::null_mut(), |cache| cache.get())
    }

    fn indirect_call_cache(&self, site: usize) -> Option<IndirectCallCacheEntry> {
        self.reference_call_caches
            .borrow()
            .get(site)?
            .as_deref()
            .cloned()
    }

    fn cache_indirect_call(&self, site: usize, entry: IndirectCallCacheEntry) {
        if let Some(slot) = self.reference_call_caches.borrow_mut().get_mut(site) {
            *slot = Some(Box::new(entry));
        }
    }

    fn note_indirect_cache_hit(&self, site: usize) {
        if let Some(Some(entry)) = self.reference_call_caches.borrow_mut().get_mut(site) {
            entry.hits = entry.hits.saturating_add(1);
        }
    }

    /// Return recently observed targets for call sites in a selective root's
    /// static closure, hottest first. The target cache already includes the
    /// exact program and instance environment guards.
    pub(super) fn profiled_indirect_targets<H: Host>(
        &self,
        vm: &Vm<H>,
        program: ProgramId,
        env: Value,
        callers: &[bool],
    ) -> Vec<usize> {
        let caches = self.reference_call_caches.borrow();
        let mut targets = self
            .reference_cache_sites
            .iter()
            .enumerate()
            .filter(|(_, (caller, _, _))| callers.get(*caller).copied().unwrap_or(false))
            .filter_map(|(site, (_, _, table_slot))| {
                let entry = caches.get(site)?.as_deref()?;
                let current_table = vm.heap.environment_slot(env, *table_slot as usize)?;
                (entry.table_slot == *table_slot
                    && entry.program == program
                    && entry.env == env
                    && entry.table == current_table
                    && vm.wasm_table_get(current_table, entry.index).ok() == Some(entry.target)
                    && vm.heap.weak_value(entry.target_weak) == Some(entry.target)
                    && reference_target_matches(
                        vm,
                        entry.target,
                        entry.program,
                        entry.function,
                        entry.env,
                    ))
                .then_some((entry.function as usize, entry.hits))
            })
            .collect::<Vec<_>>();
        targets.sort_unstable_by_key(|(_, hits)| std::cmp::Reverse(*hits));
        let mut unique = Vec::new();
        for (target, _) in targets {
            if !unique.contains(&target) {
                unique.push(target);
                if unique.len() == MAX_JIT_PROFILED_INDIRECT_TARGETS {
                    break;
                }
            }
        }
        unique
    }

    pub(super) fn clear_indirect_call_caches(&self) {
        self.reference_call_caches
            .borrow_mut()
            .iter_mut()
            .for_each(|slot| *slot = None);
    }

    #[cfg(test)]
    pub(super) fn reference_call_cache_target(&self, site: usize) -> Option<Value> {
        self.indirect_call_cache(site).map(|entry| entry.target)
    }

    #[cfg(test)]
    pub(super) fn reference_cache_hits(&self) -> u64 {
        self.reference_cache_hits.get()
    }

    #[cfg(test)]
    pub(super) fn reference_fast_cache_matches_target(
        &self,
        site: usize,
        target_function: usize,
    ) -> bool {
        let Some(cache) = self.reference_fast_caches.get(site) else {
            return false;
        };
        let Some(body) = self.table.get(target_function).copied() else {
            return false;
        };
        // The VM has returned from native execution before tests inspect this
        // cache, and only the owning artifact's callout writes the cell.
        let cache = unsafe { *cache.get() };
        cache.function == target_function as u64 && cache.body == body as u64 && body != 0
    }

    #[cfg(test)]
    pub(super) fn emscripten_memcpy_fast_path_hits(&self) -> u64 {
        self.diagnostics
            .as_ref()
            .map_or(0, |stats| stats.emscripten_memcpy_fast_path_hits.get())
    }

    #[cfg(test)]
    pub(super) fn emscripten_memcpy_raw_i32_hits(&self) -> u64 {
        self.diagnostics
            .as_ref()
            .map_or(0, |stats| stats.emscripten_memcpy_raw_i32_hits.get())
    }

    #[cfg(test)]
    pub(super) fn emscripten_memcpy_early_import_hits(&self) -> u64 {
        self.diagnostics
            .as_ref()
            .map_or(0, |stats| stats.emscripten_memcpy_early_import_hits.get())
    }

    fn note_direct_reference_entry(&self, function: usize) -> bool {
        let native_entry_should_sample = self.note_native_entry(function);
        #[cfg(test)]
        if let Some(Some(function)) = self.functions.get(function) {
            function
                .direct_reference_entries
                .set(function.direct_reference_entries.get().saturating_add(1));
        }
        native_entry_should_sample
    }

    fn note_native_entry(&self, function: usize) -> bool {
        let native_entry_should_sample = if let Some(stats) = &self.diagnostics
            && let Some(entries) = stats.native_entry_requests.get(function)
        {
            let entry_count = entries.get().saturating_add(1);
            entries.set(entry_count);
            native_entry_sample_due(entry_count)
        } else {
            false
        };
        #[cfg(test)]
        if let Some(Some(function)) = self.functions.get(function) {
            function
                .native_entries
                .set(function.native_entries.get().saturating_add(1));
        }
        native_entry_should_sample
    }

    fn finish_native_entry_sample(&self, function: usize, started: std::time::Instant) {
        let stats = self
            .diagnostics
            .as_ref()
            .expect("sampled entries require JIT diagnostics");
        let (Some(samples), Some(sampled_ns)) = (
            stats.native_entry_samples.get(function),
            stats.native_entry_sampled_ns.get(function),
        ) else {
            unreachable!("sampled function is present in diagnostic function tables");
        };
        let elapsed_ns = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        samples.set(samples.get().saturating_add(1));
        sampled_ns.set(sampled_ns.get().saturating_add(elapsed_ns));
    }

    pub(super) fn has_function(&self, function: usize) -> bool {
        self.functions.get(function).is_some_and(Option::is_some)
    }

    pub(super) fn mapped_bytes(&self) -> usize {
        self._code.length
    }

    #[cfg(test)]
    pub(super) fn reference_call_site_counts(&self) -> Option<Vec<(usize, usize, u64, u64, u64)>> {
        Some(
            self.diagnostics
                .as_ref()?
                .reference_call_sites
                .iter()
                .map(|site| {
                    let counts = site.counts.get();
                    (
                        site.function,
                        site.pc,
                        counts.total,
                        counts.native,
                        counts.fallback,
                    )
                })
                .collect(),
        )
    }

    #[cfg(test)]
    pub(super) fn reference_call_target_counts(
        &self,
    ) -> Option<Vec<(usize, usize, usize, u64, u64, u64)>> {
        Some(
            self.diagnostics
                .as_ref()?
                .reference_call_sites
                .iter()
                .flat_map(|site| {
                    site.targets
                        .borrow()
                        .iter()
                        .map(|target| {
                            (
                                site.function,
                                site.pc,
                                target.function,
                                target.total,
                                target.native,
                                target.same_artifact_native,
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .collect(),
        )
    }

    #[cfg(test)]
    pub(super) fn same_artifact_direct_reference_hits(&self) -> Option<u64> {
        Some(
            self.diagnostics
                .as_ref()?
                .reference_call_sites
                .iter()
                .map(|site| site.counts.get().same_artifact_direct)
                .fold(0u64, u64::saturating_add),
        )
    }

    pub(super) fn has_function_or_note_fallback(&self, function: usize) -> bool {
        let available = self.has_function(function);
        if !available
            && let Some(stats) = &self.diagnostics
            && let Some(fallbacks) = stats.interpreter_fallback_entries.get(function)
        {
            fallbacks.set(fallbacks.get().saturating_add(1));
        }
        available
    }

    #[cfg(test)]
    pub(super) fn native_entries(&self, function: usize) -> Option<u64> {
        self.functions
            .get(function)?
            .as_ref()
            .map(|function| function.native_entries.get())
    }

    #[cfg(test)]
    pub(super) fn direct_reference_entries(&self, function: usize) -> Option<u64> {
        self.functions
            .get(function)?
            .as_ref()
            .map(|function| function.direct_reference_entries.get())
    }
}

/// With `QUENCH_WASM_JIT_PERF_MAP` set, names each compiled Wasm function's code for
/// `perf` (`/tmp/perf-PID.map`) and writes the module's code to `/tmp/quench-jit-PID-BASE.bin`
/// for disassembly at `BASE` (diagnostics only).
fn write_perf_map(base: usize, bytes: &[u8], spans: &[(usize, usize, usize)]) {
    if std::env::var_os("QUENCH_WASM_JIT_PERF_MAP").is_none() {
        return;
    }
    use std::io::Write;
    let pid = std::process::id();
    if let Ok(mut map) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("/tmp/perf-{pid}.map"))
    {
        for &(offset, len, index) in spans {
            let _ = writeln!(map, "{:x} {len:x} wasm-function-{index}", base + offset);
        }
    }
    let _ = std::fs::write(format!("/tmp/quench-jit-{pid}-{base:x}.bin"), bytes);
}

/// Executable memory holding a program's machine code.
pub(super) struct CodeMemory {
    pointer: *mut u8,
    length: usize,
}

impl CodeMemory {
    /// The first byte of the executable mapping.
    pub(super) fn pointer(&self) -> *mut u8 {
        self.pointer
    }

    pub(super) fn new(bytes: &[u8]) -> Option<Self> {
        let length = bytes.len().max(1);
        // `mmap` rounds to whole pages. Store that rounded length explicitly
        // so the JS tier's budget reflects its executable mappings.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        let page_size = usize::try_from(page_size).ok()?;
        let length = length
            .checked_add(page_size.checked_sub(1)?)?
            .checked_div(page_size)?
            .checked_mul(page_size)?;
        // SAFETY: an anonymous private mapping; the result is checked.
        let pointer = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if pointer == libc::MAP_FAILED {
            return None;
        }
        let pointer = pointer.cast::<u8>();
        // SAFETY: the mapping is `length` writable bytes.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len()) };
        // SAFETY: the same mapping, now read-only and executable.
        if unsafe { libc::mprotect(pointer.cast(), length, libc::PROT_READ | libc::PROT_EXEC) } != 0
        {
            // SAFETY: unmapping the mapping created above.
            unsafe { libc::munmap(pointer.cast(), length) };
            return None;
        }
        Some(Self { pointer, length })
    }
}

impl Drop for CodeMemory {
    fn drop(&mut self) {
        // SAFETY: the mapping this value owns.
        unsafe { libc::munmap(self.pointer.cast(), self.length) };
    }
}

/// A native OSR region published by the shared compiler. Its lowerer is private to this
/// module; lanes only ask the region to run and inspect its exact-PC widening result.
pub(super) struct CompiledOsrRegion {
    _code: CodeMemory,
    entry: Entry,
    widenings: std::collections::BTreeMap<u32, Widening>,
}

impl CompiledOsrRegion {
    /// Runs the region on `registers` with `entry`'s record, through the native entry every
    /// compiled artifact shares (`enter_native`).
    ///
    /// # Safety
    /// `registers` is the running activation's register file, at least `Function::registers`
    /// long; `entry` was built for this region from the current values of its arrays and
    /// captures, which nothing changes while the call runs; `interrupt` is the VM's, whose
    /// poll page the region was compiled for (`compile_osr_region`).
    pub(super) unsafe fn run(
        &self,
        registers: *mut crate::value::Value,
        entry: &LoopEntry,
        interrupt: &crate::execution_interrupt::ExecutionInterrupt,
    ) -> LoopOutcome {
        let mut context = CallContext::interrupt_only(interrupt);
        let mut word = 0u64;
        // SAFETY: as the caller promises; the code reads and writes only those and the
        // result word.
        let status = unsafe {
            enter_native(
                &mut context,
                registers.addr() as u64,
                0,
                entry.words(),
                &mut word,
                NativeEntryMode::Resumable,
                self.entry,
            )
        };
        if i64::from(status) == TERMINATED_STATUS {
            LoopOutcome::Terminated
        } else {
            LoopOutcome::decode(word)
        }
    }

    pub(super) fn widening(&self, pc: u32) -> Option<Widening> {
        self.widenings.get(&pc).copied()
    }
}

/// Analyze a loop candidate through the shared region compiler's OSR mode.
pub(super) fn analyze_osr_region(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[crate::value::Value],
    live: Option<&[crate::compile::liveness::RegisterSet]>,
    header: u32,
    backedge: u32,
) -> Result<LoopRegion, RegionRejection> {
    osr_loop::region(function, decoded, constants, live, header, backedge)
}

/// The Cranelift optimization level the loop tier compiles regions at under each memory
/// policy (`MemoryPolicy` order), or none when it compiles no region. A loop region's
/// compilation is the largest transient allocation of a `--jit` run: regalloc2's state and
/// the code pages of the compiler and of the region.
///
/// Memory-first compiles no loop region, on the owner's direction of 2026-10-24 (TOD-123:
/// memory-first may trade Score for memory under `--jit`). The per-function tier, which this
/// leaves running, kept Richards, RayTrace and EarleyBoyer `--jit` below their interpreted
/// peaks. NavierStokes `--jit` peaked at 7468 KiB compiled and 3988 interpreted, Crypto at
/// 4000 and 3128 (static BOLT x86_64, memory-first, median of 5, 2026-10-24). Compiling at
/// `none`, without the e-graph pass, saved only 576 KiB on Crypto and 648 on NavierStokes
/// (`tod137-loop-tier-opt-level-…`). Throughput compiles at `speed`.
const LOOP_TIER_OPT_LEVELS: [Option<&str>; crate::MemoryPolicy::ALL.len()] = [Some("speed"), None];

/// Whether the loop tier compiles regions under `policy` (`LOOP_TIER_OPT_LEVELS`).
pub(super) fn loop_tier_compiles(policy: crate::MemoryPolicy) -> bool {
    LOOP_TIER_OPT_LEVELS[policy as usize].is_some()
}

/// Compile one admitted OSR region and publish it through the shared executable-memory path.
pub(super) fn compile_osr_region(
    region: &LoopRegion,
    observation: &Observation<'_>,
    interrupt: &crate::execution_interrupt::ExecutionInterrupt,
    policy: crate::MemoryPolicy,
) -> Option<CompiledOsrRegion> {
    let isa = super::jit_backend::target_isa(LOOP_TIER_OPT_LEVELS[policy as usize]?, false)?;
    let (function, widenings) = osr_loop::lower(
        region,
        observation,
        &*isa,
        poll_address(interrupt),
    )?;
    let (_code, entry) = compile_and_publish_entry(function, &*isa, osr_loop::MAX_CODE_BYTES)?;
    Some(CompiledOsrRegion {
        _code,
        entry,
        widenings,
    })
}

/// Compile and publish a fully lowered native entry that has no relocations.
///
/// JS scalar regions and OSR regions both use this entry contract. Wasm functions
/// with internal calls have a separate linking phase before publication.
fn compile_and_publish_entry(
    function: ir::Function,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    max_code_bytes: usize,
) -> Option<(CodeMemory, Entry)> {
    compile_native_function(function, isa, max_code_bytes, |code| {
        let memory = CodeMemory::new(code)?;
        // SAFETY: callers lower the function with `native_entry_signature`.
        let entry = unsafe { std::mem::transmute::<*mut u8, Entry>(memory.pointer()) };
        Some((memory, entry))
    })
}

/// Compile a relocation-free native function under the shared code-size policy, then
/// consume its machine-code view through the frontend's artifact representation.
fn compile_native_function<T>(
    function: ir::Function,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    max_code_bytes: usize,
    publish: impl FnOnce(&[u8]) -> Option<T>,
) -> Option<T> {
    let mut context = cranelift_codegen::Context::for_function(function);
    let compiled = match context.compile(
        isa,
        &mut cranelift_codegen::control::ControlPlane::default(),
    ) {
        Ok(compiled) => compiled,
        Err(_) => return None,
    };
    if !compiled.buffer.relocs().is_empty() {
        return None;
    }
    let code = compiled.code_buffer();
    if code.is_empty() || code.len() > max_code_bytes {
        return None;
    }
    publish(code)
}

/// A guarded JavaScript numeric function compiled from a narrow residual
/// bytecode subset. It owns one W^X executable mapping and has no heap roots.
pub(super) struct CompiledJsFunction {
    _code: CodeMemory,
    entry: CompiledJsEntry,
    params: usize,
    requires_signed_int32_arguments: bool,
    mapped_bytes: usize,
    return_constants: Vec<(u32, u32)>,
    field_argument: Option<super::js_compiled::JsJitFieldArgument>,
    numeric_field_inputs: Vec<super::js_compiled::JsJitNumericFieldInput>,
    capture_argument: Option<super::js_compiled::JsJitCaptureArgument>,
    capture_arguments: Vec<super::js_compiled::JsJitCaptureArgument>,
    #[cfg(test)]
    native_entries: std::cell::Cell<u64>,
}

#[derive(Clone, Copy)]
enum CompiledJsEntry {
    SharedNumber(Entry),
    GeneralDispatch(Entry),
    Int32Identity {
        entry: unsafe extern "C" fn(*const f64) -> f64,
        return_pc: u32,
    },
    CaptureNumber(unsafe extern "C" fn(f64) -> f64),
    NumberInterruptible(
        unsafe extern "C" fn(*const f64, *const std::sync::atomic::AtomicBool, *mut u32) -> f64,
    ),
    StringHashLoop(
        unsafe extern "C" fn(
            *const u8,
            *const usize,
            u32,
            *const std::sync::atomic::AtomicBool,
            *mut u32,
        ) -> f64,
    ),
    StringTransform {
        entry: unsafe extern "C" fn(*const u8, usize, u32) -> u32,
        transform: super::js_compiled::JsJitStringTransform,
    },
    StringReturnPc(unsafe extern "C" fn(*const f64) -> u32),
    Boolean(unsafe extern "C" fn(*const f64) -> u32),
    FieldReject {
        entry: unsafe extern "C" fn(*const f64) -> u32,
        resume_pc: usize,
        resume_register: crate::bytecode::Register,
        resume_value: Value,
    },
}

pub(super) enum CompiledJsResult {
    Number(f64),
    Int32Identity {
        value: f64,
        return_pc: u32,
    },
    StringReturnPc(u32),
    Boolean(bool),
    FieldReject {
        fast_path: bool,
        resume_pc: usize,
        resume_register: crate::bytecode::Register,
        resume_value: Value,
    },
    StringTransform(super::js_compiled::JsJitStringTransform, bool),
}

enum CompiledJsCallResult {
    Value(Value),
    ReturnConstant(u32),
}

#[derive(Clone, Copy)]
enum JsNativeCallInput<'a> {
    Arguments {
        values: &'a [f64],
        interrupt: &'a std::sync::atomic::AtomicBool,
    },
    CaptureNumber(f64),
    StringHashLoop {
        units: crate::vm::wtf16::Units<'a>,
        interrupt: &'a std::sync::atomic::AtomicBool,
    },
    StringTransform(crate::vm::wtf16::Units<'a>),
}

struct JsNativeCall<'a> {
    function: &'a CompiledJsFunction,
    input: JsNativeCallInput<'a>,
    poll_page: Option<(usize, usize)>,
    result: Option<CompiledJsResult>,
}

/// The instruction transition that returns from a general native-dispatch
/// region to the ordinary frame loop. Its `StepResult` is already executed,
/// so the dispatcher consumes it instead of replaying the bytecode.
pub(super) struct JsNativeStep {
    pub(super) instruction_pc: usize,
    pub(super) next_pc: usize,
    pub(super) result: Result<super::StepResult, JsError>,
}

struct JsStepDispatch {
    program_id: ProgramId,
    frame: usize,
    allow_inline_calls: bool,
    native_registers: *mut Value,
    register_count: usize,
    native_locals: *mut Value,
    local_count: usize,
    outcome: Option<JsNativeStep>,
}

fn js_native_frame_storage_unchanged(frame: &Frame, dispatch: &JsStepDispatch) -> bool {
    frame.registers.len() == dispatch.register_count
        && frame.registers.as_ptr() == dispatch.native_registers
        && frame.locals.len() == dispatch.local_count
        && frame.locals.as_ptr() == dispatch.native_locals
}

pub(super) enum JsNativeDispatchResult {
    Step(JsNativeStep),
    Resume(usize),
}

pub(super) enum JsTierFrameEntry {
    Interpreter,
    ContinueAt(usize),
    NativeStep(JsNativeStep),
    NativeResume(usize),
}

enum JsCalloutStep {
    Continue(usize),
    Stop(JsNativeStep),
}

unsafe extern "C" fn js_step_callout<H: Host>(
    context: *mut CallContext,
    function: u64,
    _arguments: *const u64,
    result: *mut u64,
) -> i64 {
    // The shared trampoline marks its active context while native code runs.
    // Clear it while Rust semantics and any host or guest helper run, so a
    // fault there is never mistaken for a generated-code fault.
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    // SAFETY: the compiled entry's owner keeps this context and adapter payload
    // alive through the callout.
    let context_ref = unsafe { &mut *context };
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let pc = usize::try_from(function)
            .map_err(|_| JsError::validation("native JS PC does not fit usize".into()))?;
        let dispatch = unsafe { &mut *context_ref.adapter_payload.cast::<JsStepDispatch>() };
        let vm = unsafe { &mut *context_ref.vm.cast::<Vm<H>>() };
        let program = vm
            .programs
            .get(dispatch.program_id)
            .ok_or_else(|| JsError::validation("native JS program is missing".into()))?;
        let body = program
            .function_get(vm.frames[dispatch.frame].function as usize)
            .ok_or_else(|| JsError::validation("native JS function is missing".into()))?;
        let packed = *body
            .code
            .get(pc)
            .ok_or_else(|| JsError::validation("native JS PC is outside function".into()))?;
        let instruction = super::dispatch_fast::decoded_instruction(body, packed);
        let next_pc = pc
            .checked_add(1)
            .ok_or_else(|| JsError::validation("native JS PC overflow".into()))?;

        let frame = &mut vm.frames[dispatch.frame];
        if !js_native_frame_storage_unchanged(frame, dispatch) {
            // A prior resumable callback changed the frame storage. Leave this
            // instruction untouched and finish through the ordinary dispatcher.
            frame.pc = pc;
            frame.pc_is_resume = true;
            vm.active_registers = frame.registers.as_mut_ptr();
            return Ok(JsCalloutStep::Stop(JsNativeStep {
                instruction_pc: pc,
                next_pc: pc,
                result: Ok(super::StepResult::Continue),
            }));
        }
        frame.pc = pc;
        frame.pc_is_resume = false;
        if let Err(error) = vm.check_execution_interrupt() {
            return Ok(JsCalloutStep::Stop(JsNativeStep {
                instruction_pc: pc,
                next_pc,
                result: Err(error),
            }));
        }

        if instruction.op() == Op::GetField
            && (instruction.result_flags() == 0
                || instruction.result_flags() == crate::bytecode::RETURN_REGISTER)
            && !instruction.writes_current_this()
            && (instruction.returns_from_frame() || next_pc < body.code.len())
            && let Some(FieldLookup::Atom {
                base,
                cache_site,
                ..
            }) = instruction.field_lookup()
            && let Some(receiver_register) = base.register_index()
            && let Some(receiver) = vm.frames[dispatch.frame]
                .registers
                .get(usize::from(receiver_register))
                .copied()
        {
            let probe = vm.cached_field_hit(&program, receiver, cache_site);
            if vm.programs.js_jit_diagnostics_enabled() {
                vm.programs.record_js_jit_field_cache_probe(
                    dispatch.program_id,
                    vm.frames[dispatch.frame].function,
                    matches!(&probe, super::object::FieldCacheProbe::Hit(_)),
                );
            }
            if let super::object::FieldCacheProbe::Hit(value) = probe {
                let frame = &mut vm.frames[dispatch.frame];
                if instruction.returns_from_frame() {
                    frame.pc = pc;
                    frame.pc_is_resume = false;
                    return Ok(JsCalloutStep::Stop(JsNativeStep {
                        instruction_pc: pc,
                        next_pc,
                        result: Ok(super::StepResult::Return(value)),
                    }));
                }
                let destination = frame
                    .registers
                    .get_mut(usize::from(instruction.result_register()))
                    .ok_or_else(|| {
                        JsError::validation("native JS field result register is invalid".into())
                    })?;
                *destination = value;
                frame.pc = next_pc;
                frame.pc_is_resume = true;
                if !js_native_frame_storage_unchanged(frame, dispatch) {
                    vm.active_registers = frame.registers.as_mut_ptr();
                    return Ok(JsCalloutStep::Stop(JsNativeStep {
                        instruction_pc: pc,
                        next_pc,
                        result: Ok(super::StepResult::Continue),
                    }));
                }
                return Ok(JsCalloutStep::Continue(next_pc));
            }
        }

        if instruction.op().js_denotation() == JsDenotation::FieldStore
            && instruction.result_flags() == 0
            && !instruction.returns_from_frame()
            && next_pc < body.code.len()
            && let (Some(receiver), Some(value)) = (
                vm.frames[dispatch.frame]
                    .registers
                    .get(usize::from(instruction.register_b()))
                    .copied(),
                vm.frames[dispatch.frame]
                    .registers
                    .get(usize::from(instruction.register_a()))
                    .copied(),
            )
        {
            let hit = vm.cached_field_store_hit(
                &program,
                receiver,
                instruction.atom_index(),
                value,
                instruction.cache_site_index(),
            );
            if vm.programs.js_jit_diagnostics_enabled() {
                vm.programs.record_js_jit_field_store_cache_probe(
                    dispatch.program_id,
                    vm.frames[dispatch.frame].function,
                    hit,
                );
            }
            if hit {
                let frame = &mut vm.frames[dispatch.frame];
                frame.pc = next_pc;
                frame.pc_is_resume = true;
                if !js_native_frame_storage_unchanged(frame, dispatch) {
                    vm.active_registers = frame.registers.as_mut_ptr();
                    return Ok(JsCalloutStep::Stop(JsNativeStep {
                        instruction_pc: pc,
                        next_pc,
                        result: Ok(super::StepResult::Continue),
                    }));
                }
                return Ok(JsCalloutStep::Continue(next_pc));
            }
        }

        let mut next = next_pc;
        let step = vm.step(
            &program,
            dispatch.frame,
            instruction,
            &mut next,
            dispatch.allow_inline_calls,
        );
        match step {
            Ok(super::StepResult::Continue) => {
                if next >= body.code.len() {
                    vm.frames[dispatch.frame].pc = pc;
                    vm.frames[dispatch.frame].pc_is_resume = false;
                    return Ok(JsCalloutStep::Stop(JsNativeStep {
                        instruction_pc: pc,
                        next_pc: next,
                        result: Err(JsError::validation(
                            "native JS continuation is outside its function".into(),
                        )),
                    }));
                }
                let frame = &mut vm.frames[dispatch.frame];
                frame.pc = next;
                frame.pc_is_resume = true;
                if !js_native_frame_storage_unchanged(frame, dispatch) {
                    // The operation ran once. Stop native execution and resume
                    // the interpreter at its already-computed successor.
                    frame.js_jit_skip_entry_once = next == 0;
                    vm.active_registers = frame.registers.as_mut_ptr();
                    return Ok(JsCalloutStep::Stop(JsNativeStep {
                        instruction_pc: pc,
                        next_pc: next,
                        result: Ok(super::StepResult::Continue),
                    }));
                }
                Ok(JsCalloutStep::Continue(next))
            }
            result => Ok(JsCalloutStep::Stop(JsNativeStep {
                instruction_pc: pc,
                next_pc: next,
                result,
            })),
        }
    }));
    ACTIVE_CONTEXT.with(|current| current.set(active));

    match called {
        Ok(Ok(JsCalloutStep::Continue(next))) => {
            // SAFETY: generated code supplies a valid result word.
            unsafe { result.write(next as u64) };
            0
        }
        Ok(Ok(JsCalloutStep::Stop(step))) => {
            // SAFETY: the adapter payload is unique to this native entry.
            unsafe {
                (*context_ref.adapter_payload.cast::<JsStepDispatch>()).outcome = Some(step);
                result.write(0);
            }
            1
        }
        Ok(Err(error)) => {
            let dispatch = unsafe { &mut *context_ref.adapter_payload.cast::<JsStepDispatch>() };
            dispatch.outcome = Some(JsNativeStep {
                instruction_pc: usize::try_from(function).unwrap_or(0),
                next_pc: usize::try_from(function).unwrap_or(0).saturating_add(1),
                result: Err(error),
            });
            unsafe { result.write(0) };
            1
        }
        Err(panic) => {
            context_ref.raised = Some(Raised::Panic(panic));
            RAISED_STATUS
        }
    }
}

unsafe extern "C" fn js_native_entry(
    context: *mut CallContext,
    _base: u64,
    _length: u64,
    _arguments: *const u64,
    result: *mut u64,
) -> u32 {
    // SAFETY: `CompiledJsFunction::call_through_shared_entry` owns this payload
    // and keeps all borrowed inputs alive through the trampoline call.
    let call = unsafe { &mut *(*context).adapter_payload.cast::<JsNativeCall<'_>>() };
    call.result = call.function.execute(call.input, call.poll_page);
    #[cfg(test)]
    call.function
        .native_entries
        .set(call.function.native_entries.get().saturating_add(1));
    // SAFETY: the shared trampoline caller supplies this result word. The
    // adapter result itself is carried in the stack-owned payload.
    unsafe { result.write(0) };
    if call.result.is_some() { 0 } else { 1 }
}

impl CompiledJsFunction {
    pub(super) fn from_artifact(artifact: super::js_compiled::JsJitArtifact) -> Option<Self> {
        let code = CodeMemory::new(&artifact.code)?;
        let mapped_bytes = code.length;
        let field_argument = artifact.field_argument;
        let numeric_field_inputs = artifact.numeric_field_inputs;
        let capture_arguments = artifact.capture_arguments;
        let (entry, return_constants, capture_argument) = match artifact.result {
            super::js_compiled::JsJitResultKind::SharedNumberEntry => {
                // SAFETY: the bytecode numeric-region compiler uses `native_entry_signature`.
                let entry = unsafe { std::mem::transmute::<*mut u8, Entry>(code.pointer) };
                (CompiledJsEntry::SharedNumber(entry), Vec::new(), None)
            }
            super::js_compiled::JsJitResultKind::GeneralDispatch => {
                // SAFETY: the general JS dispatcher emits the shared native-entry ABI.
                let entry = unsafe { std::mem::transmute::<*mut u8, Entry>(code.pointer) };
                (CompiledJsEntry::GeneralDispatch(entry), Vec::new(), None)
            }
            super::js_compiled::JsJitResultKind::Int32Identity(return_pc) => {
                // SAFETY: the guarded integer identity compiler emits the Number ABI.
                let entry = unsafe {
                    std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const f64) -> f64>(
                        code.pointer,
                    )
                };
                (
                    CompiledJsEntry::Int32Identity { entry, return_pc },
                    Vec::new(),
                    None,
                )
            }
            super::js_compiled::JsJitResultKind::CaptureNumber(capture) => {
                // SAFETY: the capture identity compiler emits `fn(f64) -> f64`.
                let entry = unsafe {
                    std::mem::transmute::<*mut u8, unsafe extern "C" fn(f64) -> f64>(code.pointer)
                };
                (
                    CompiledJsEntry::CaptureNumber(entry),
                    Vec::new(),
                    Some(capture),
                )
            }
            super::js_compiled::JsJitResultKind::NumberInterruptible => {
                // SAFETY: the loop compiler emits the matching guarded ABI and
                // writes completion status before every return.
                let entry = unsafe {
                    std::mem::transmute::<
                        *mut u8,
                        unsafe extern "C" fn(
                            *const f64,
                            *const std::sync::atomic::AtomicBool,
                            *mut u32,
                        ) -> f64,
                    >(code.pointer)
                };
                (
                    CompiledJsEntry::NumberInterruptible(entry),
                    Vec::new(),
                    None,
                )
            }
            super::js_compiled::JsJitResultKind::StringHashLoop => {
                // SAFETY: the string hash loop compiler emits the matching
                // code-unit pointer, length, interrupt, and status ABI.
                let entry = unsafe {
                    std::mem::transmute::<
                        *mut u8,
                        unsafe extern "C" fn(
                            *const u8,
                            *const usize,
                            u32,
                            *const std::sync::atomic::AtomicBool,
                            *mut u32,
                        ) -> f64,
                    >(code.pointer)
                };
                (CompiledJsEntry::StringHashLoop(entry), Vec::new(), None)
            }
            super::js_compiled::JsJitResultKind::StringTransform(transform) => {
                // SAFETY: the string transform compiler emits the matching code-unit ABI.
                let entry = unsafe {
                    std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const u8, usize, u32) -> u32>(
                        code.pointer,
                    )
                };
                (
                    CompiledJsEntry::StringTransform { entry, transform },
                    Vec::new(),
                    None,
                )
            }
            super::js_compiled::JsJitResultKind::StringReturnPc(return_constants) => {
                // SAFETY: the static-string compiler emits `fn(*const f64) -> u32`.
                let entry = unsafe {
                    std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const f64) -> u32>(
                        code.pointer,
                    )
                };
                (
                    CompiledJsEntry::StringReturnPc(entry),
                    return_constants,
                    None,
                )
            }
            super::js_compiled::JsJitResultKind::Boolean => {
                // SAFETY: the boolean compiler emits `fn(*const f64) -> u32`.
                let entry = unsafe {
                    std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const f64) -> u32>(
                        code.pointer,
                    )
                };
                (CompiledJsEntry::Boolean(entry), Vec::new(), None)
            }
            super::js_compiled::JsJitResultKind::FieldReject {
                resume_pc,
                resume_register,
                resume_value,
            } => {
                // SAFETY: the field-reject compiler emits
                // `fn(*const f64) -> u32`, where zero resumes the interpreter.
                let entry = unsafe {
                    std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const f64) -> u32>(
                        code.pointer,
                    )
                };
                (
                    CompiledJsEntry::FieldReject {
                        entry,
                        resume_pc: usize::try_from(resume_pc).ok()?,
                        resume_register: crate::bytecode::Register::try_from(resume_register)
                            .ok()?,
                        resume_value: match resume_value {
                            super::js_compiled::JsJitResumeValue::Number(value) => {
                                Value::number(value)
                            }
                            super::js_compiled::JsJitResumeValue::Boolean(value) => {
                                if value { Value::TRUE } else { Value::FALSE }
                            }
                        },
                    },
                    Vec::new(),
                    None,
                )
            }
        };
        Some(Self {
            _code: code,
            entry,
            params: artifact.params,
            requires_signed_int32_arguments: artifact.requires_signed_int32_arguments,
            mapped_bytes,
            return_constants,
            field_argument,
            numeric_field_inputs,
            capture_argument,
            capture_arguments,
            #[cfg(test)]
            native_entries: std::cell::Cell::new(0),
        })
    }

    /// Whether a call runs the body on its Number arguments alone, with no receiver,
    /// captures or field reads to assemble and no string result to build
    /// (`js_jit_compiled_call`).
    fn direct_callable(&self) -> bool {
        self.field_argument().is_none()
            && self.numeric_field_inputs().is_empty()
            && !self.is_capture_number()
            && self.capture_arguments().is_empty()
            && !self.is_string_transform()
            && !self.is_string_hash_loop()
            && !self.is_field_reject()
    }

    pub(super) fn params(&self) -> usize {
        self.params
    }

    pub(super) fn is_int32_identity(&self) -> bool {
        matches!(self.entry, CompiledJsEntry::Int32Identity { .. })
    }

    pub(super) fn is_general_dispatch(&self) -> bool {
        matches!(self.entry, CompiledJsEntry::GeneralDispatch(_))
    }

    pub(super) fn requires_signed_int32_arguments(&self) -> bool {
        self.requires_signed_int32_arguments
    }

    pub(super) fn field_argument(&self) -> Option<super::js_compiled::JsJitFieldArgument> {
        self.field_argument
    }

    pub(super) fn numeric_field_inputs(&self) -> &[super::js_compiled::JsJitNumericFieldInput] {
        &self.numeric_field_inputs
    }

    fn uses_numeric_field_argument(&self) -> bool {
        self.field_argument.is_some() && matches!(self.entry, CompiledJsEntry::SharedNumber(_))
    }

    fn capture_argument(&self) -> Option<super::js_compiled::JsJitCaptureArgument> {
        self.capture_argument
    }

    fn capture_arguments(&self) -> &[super::js_compiled::JsJitCaptureArgument] {
        &self.capture_arguments
    }

    pub(super) fn call(
        &self,
        arguments: &[f64],
        interrupt: &std::sync::atomic::AtomicBool,
        poll_page: Option<(usize, usize)>,
    ) -> Option<CompiledJsResult> {
        if self.is_general_dispatch() {
            return None;
        }
        if let CompiledJsEntry::SharedNumber(entry) = self.entry {
            let (status, result) = run_native_entry(
                entry,
                arguments,
                interrupt,
                poll_page,
                self.native_entry_mode(),
            );
            (status == 0).then_some(())?;
            #[cfg(test)]
            self.native_entries
                .set(self.native_entries.get().saturating_add(1));
            return Some(CompiledJsResult::Number(f64::from_bits(result)));
        }
        self.call_through_shared_entry(JsNativeCallInput::Arguments {
            values: arguments,
            interrupt,
        }, poll_page)
    }

    pub(super) fn run_general_dispatch<H: Host>(
        &self,
        vm: *mut Vm<H>,
        program: ProgramId,
        payload: *mut (),
        interrupt: *const std::sync::atomic::AtomicBool,
        poll_page: Option<(usize, usize)>,
    ) -> Option<JsNativeDispatchResult> {
        let CompiledJsEntry::GeneralDispatch(entry) = self.entry else {
            return None;
        };
        let mut context = CallContext::native_adapter(payload, interrupt, poll_page);
        context.vm = vm.cast();
        context.program = program;
        context.callout = js_step_callout::<H> as Callout as usize;
        let dispatch_frame = unsafe { (*payload.cast::<JsStepDispatch>()).frame };
        let (native_registers, register_count, native_locals, local_count) = unsafe {
            let vm = &mut *vm.cast::<Vm<H>>();
            let frame = vm.frames.get_mut(dispatch_frame)?;
            (
                frame.registers.as_mut_ptr(),
                frame.registers.len(),
                frame.locals.as_mut_ptr(),
                frame.locals.len(),
            )
        };
        unsafe {
            let dispatch = &mut *payload.cast::<JsStepDispatch>();
            dispatch.native_registers = native_registers;
            dispatch.register_count = register_count;
            dispatch.native_locals = native_locals;
            dispatch.local_count = local_count;
        }
        // Frame registers and locals are the canonical tagged homes and remain
        // rooted while native code runs. Callouts keep only raw pointers; they
        // must not hold a Rust slice across `Vm::step` or re-entrant calls.
        let locals = native_locals.cast::<u64>();
        let arguments = native_registers.cast::<u64>();
        let mut word = 0u64;
        // SAFETY: the function's bytecode, frame and adapter payload outlive
        // the native dispatch. The callout writes only `word` and its payload.
        let status = unsafe {
            enter_native(
                &mut context,
                locals as u64,
                u64::try_from(local_count).unwrap_or(u64::MAX),
                arguments,
                &mut word,
                NativeEntryMode::Resumable,
                entry,
            )
        };
        if let Some(Raised::Panic(panic)) = context.raised.take() {
            std::panic::resume_unwind(panic);
        }
        if let Some(Raised::Error(_)) = context.raised.take() {
            return None;
        }
        match status {
            1 => {
                #[cfg(test)]
                self.native_entries
                    .set(self.native_entries.get().saturating_add(1));
                unsafe { (*payload.cast::<JsStepDispatch>()).outcome.take() }
                    .map(JsNativeDispatchResult::Step)
            }
            2 => {
                let vm = unsafe { &mut *vm.cast::<Vm<H>>() };
                let frame = vm.frames.get_mut(dispatch_frame)?;
                if frame.registers.len() != register_count
                    || frame.registers.as_mut_ptr() != native_registers
                {
                    frame.pc = word as usize;
                    frame.pc_is_resume = true;
                    frame.js_jit_skip_entry_once = word == 0;
                    vm.active_registers = frame.registers.as_mut_ptr();
                }
                Some(JsNativeDispatchResult::Resume(word as usize))
            }
            _ => None,
        }
    }

    fn call_capture_number(
        &self,
        value: f64,
        poll_page: Option<(usize, usize)>,
    ) -> Option<CompiledJsResult> {
        self.call_through_shared_entry(JsNativeCallInput::CaptureNumber(value), poll_page)
    }

    fn is_capture_number(&self) -> bool {
        matches!(self.entry, CompiledJsEntry::CaptureNumber(_))
    }

    fn native_entry_mode(&self) -> NativeEntryMode {
        match self.entry {
            CompiledJsEntry::GeneralDispatch(_)
            | CompiledJsEntry::NumberInterruptible(_)
            | CompiledJsEntry::StringHashLoop(_) => {
                NativeEntryMode::Resumable
            }
            CompiledJsEntry::SharedNumber(_)
            | CompiledJsEntry::Int32Identity { .. }
            | CompiledJsEntry::CaptureNumber(_)
            | CompiledJsEntry::StringTransform { .. }
            | CompiledJsEntry::StringReturnPc(_)
            | CompiledJsEntry::Boolean(_)
            | CompiledJsEntry::FieldReject { .. } => NativeEntryMode::Leaf,
        }
    }

    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn uses_leaf_entry(&self) -> bool {
        self.native_entry_mode() == NativeEntryMode::Leaf
    }

    fn call_string_hash_loop(
        &self,
        units: crate::vm::wtf16::Units<'_>,
        interrupt: &std::sync::atomic::AtomicBool,
        poll_page: Option<(usize, usize)>,
    ) -> Option<CompiledJsResult> {
        self.call_through_shared_entry(
            JsNativeCallInput::StringHashLoop { units, interrupt },
            poll_page,
        )
    }

    fn call_string_transform(
        &self,
        units: crate::vm::wtf16::Units<'_>,
        poll_page: Option<(usize, usize)>,
    ) -> Option<CompiledJsResult> {
        self.call_through_shared_entry(JsNativeCallInput::StringTransform(units), poll_page)
    }

    fn call_through_shared_entry(
        &self,
        input: JsNativeCallInput<'_>,
        poll_page: Option<(usize, usize)>,
    ) -> Option<CompiledJsResult> {
        let interrupt = match &input {
            JsNativeCallInput::Arguments { interrupt, .. }
            | JsNativeCallInput::StringHashLoop { interrupt, .. } => {
                std::ptr::from_ref(*interrupt)
            }
            JsNativeCallInput::CaptureNumber(_) | JsNativeCallInput::StringTransform(_) => {
                std::ptr::null()
            }
        };
        let mut call = JsNativeCall {
            function: self,
            input,
            poll_page,
            result: None,
        };
        let (status, _) = run_native_adapter(
            (&mut call as *mut JsNativeCall<'_>).cast(),
            interrupt,
            poll_page,
            self.native_entry_mode(),
            js_native_entry,
        );
        (status == 0).then_some(())?;
        call.result
    }

    fn execute(
        &self,
        input: JsNativeCallInput<'_>,
        poll_page: Option<(usize, usize)>,
    ) -> Option<CompiledJsResult> {
        match (self.entry, input) {
            (
                CompiledJsEntry::Int32Identity { entry, return_pc },
                JsNativeCallInput::Arguments { values, .. },
            ) => {
                // SAFETY: the integer identity artifact emits the Number ABI.
                Some(CompiledJsResult::Int32Identity {
                    value: unsafe { entry(values.as_ptr()) },
                    return_pc,
                })
            }
            (
                CompiledJsEntry::CaptureNumber(entry),
                JsNativeCallInput::CaptureNumber(value),
            ) => {
                // SAFETY: this artifact emits `fn(f64) -> f64`.
                Some(CompiledJsResult::Number(unsafe { entry(value) }))
            }
            (
                CompiledJsEntry::NumberInterruptible(entry),
                JsNativeCallInput::Arguments { values, interrupt },
            ) => {
                let mut completed = 0_u32;
                let interrupt = poll_page.map_or_else(
                    || std::ptr::from_ref(interrupt),
                    |(start, _)| start as *const std::sync::atomic::AtomicBool,
                );
                // SAFETY: the result kind emits the matching polled Number ABI.
                let result = unsafe { entry(values.as_ptr(), interrupt, &mut completed) };
                (completed != 0).then_some(CompiledJsResult::Number(result))
            }
            (
                CompiledJsEntry::StringHashLoop(entry),
                JsNativeCallInput::StringHashLoop { units, interrupt },
            ) => {
                let mut completed = 0_u32;
                let (units_ptr, length, ascii) = match units {
                    crate::vm::wtf16::Units::Ascii(bytes) => (bytes.as_ptr(), bytes.len(), 1),
                    crate::vm::wtf16::Units::Wide(units) => {
                        (units.as_ptr().cast::<u8>(), units.len(), 0)
                    }
                };
                // SAFETY: `units` stays borrowed while generated code reads it.
                let interrupt = poll_page.map_or_else(
                    || std::ptr::from_ref(interrupt),
                    |(start, _)| start as *const std::sync::atomic::AtomicBool,
                );
                let result = unsafe { entry(units_ptr, &length, ascii, interrupt, &mut completed) };
                (completed != 0).then_some(CompiledJsResult::Number(result))
            }
            (
                CompiledJsEntry::StringTransform { entry, transform },
                JsNativeCallInput::StringTransform(units),
            ) => {
                let (units_ptr, length, ascii) = match units {
                    crate::vm::wtf16::Units::Ascii(bytes) => (bytes.as_ptr(), bytes.len(), 1),
                    crate::vm::wtf16::Units::Wide(units) => {
                        (units.as_ptr().cast::<u8>(), units.len(), 0)
                    }
                };
                // SAFETY: `units` remains borrowed while generated code reads it.
                let transformed = unsafe { entry(units_ptr, length, ascii) } != 0;
                Some(CompiledJsResult::StringTransform(transform, transformed))
            }
            (
                CompiledJsEntry::StringReturnPc(entry),
                JsNativeCallInput::Arguments { values, .. },
            ) => {
                // SAFETY: the static-string artifact emits the `u32` ABI.
                Some(CompiledJsResult::StringReturnPc(unsafe {
                    entry(values.as_ptr())
                }))
            }
            (CompiledJsEntry::Boolean(entry), JsNativeCallInput::Arguments { values, .. }) => {
                // SAFETY: the Boolean artifact emits the matching ABI.
                Some(CompiledJsResult::Boolean(unsafe {
                    entry(values.as_ptr()) != 0
                }))
            }
            (
                CompiledJsEntry::FieldReject {
                    entry,
                    resume_pc,
                    resume_register,
                    resume_value,
                },
                JsNativeCallInput::Arguments { values, .. },
            ) => {
                // SAFETY: the field-reject artifact emits `fn(*const f64) -> u32`.
                Some(CompiledJsResult::FieldReject {
                    fast_path: unsafe { entry(values.as_ptr()) != 0 },
                    resume_pc,
                    resume_register,
                    resume_value,
                })
            }
            _ => None,
        }
    }

    fn is_string_hash_loop(&self) -> bool {
        matches!(self.entry, CompiledJsEntry::StringHashLoop(_))
    }

    fn is_string_transform(&self) -> bool {
        matches!(self.entry, CompiledJsEntry::StringTransform { .. })
    }

    fn is_field_reject(&self) -> bool {
        matches!(self.entry, CompiledJsEntry::FieldReject { .. })
    }

    fn string_transform_kind(&self) -> Option<super::js_compiled::JsJitStringTransform> {
        match self.entry {
            CompiledJsEntry::StringTransform { transform, .. } => Some(transform),
            _ => None,
        }
    }

    pub(super) fn string_constant_for_return(&self, return_pc: u32) -> Option<u32> {
        self.return_constants
            .iter()
            .find_map(|(pc, constant)| (*pc == return_pc).then_some(*constant))
    }

    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn has_string_returns(&self) -> bool {
        !self.return_constants.is_empty()
    }

    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn native_entries(&self) -> u64 {
        self.native_entries.get()
    }

    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn uses_shared_entry(&self) -> bool {
        matches!(
            self.entry,
            CompiledJsEntry::SharedNumber(_) | CompiledJsEntry::GeneralDispatch(_)
        )
    }

    pub(super) fn mapped_bytes(&self) -> usize {
        self.mapped_bytes
    }
}

/// A function's derived compilation facts.
struct Plan {
    params: Vec<crate::WasmType>,
    result: Option<crate::WasmType>,
    /// Per instruction, `None` when unreachable, else what its `b` register
    /// holds on entry: the one analysis fact a denotation reads (a moved or
    /// selected value's kind, a global's binding, an import reference). The
    /// full register states exist only while planning.
    operands: Vec<Option<Slot>>,
}

impl Plan {
    /// The plan as its callers see it: only the function being compiled
    /// holds its per-instruction facts.
    fn interface(self) -> Self {
        Self {
            operands: Vec::new(),
            ..self
        }
    }

    fn reachable(&self, pc: usize) -> bool {
        self.operands[pc].is_some()
    }

    /// The analysis fact for the `b` register of reachable instruction `pc`.
    fn operand(&self, pc: usize) -> Slot {
        self.operands[pc].expect("reachable")
    }
}

/// A counted `i32` loop whose body computes two adjacent element-wise sums.
/// The fast path is admitted only when the memory ranges are statically
/// disjoint and fit memory32 without wrapping; a one-time runtime length guard
/// falls back to the original scalar loop when the memory is too short.
#[derive(Clone, Copy, Debug)]
struct AffinePairLoop {
    header: usize,
    branch: usize,
    exit: usize,
    induction: u16,
    bound: u32,
    destination: u32,
    left: u32,
    right: u32,
    max_end: u64,
}

fn affine_pair_loop(
    parameter_registers: Option<u16>,
    register_count: u16,
    plan: &Plan,
    instructions: &[WideInstruction],
    memory_register: u16,
) -> Option<AffinePairLoop> {
    for branch_pc in 1..instructions.len() {
        let branch = instructions[branch_pc];
        if branch.op() != Op::WasmJumpI32NotEqualImmediate {
            continue;
        }
        let header = branch.jump_target() as usize;
        let induction = branch.a();
        let Ok(bound) = u32::try_from(branch.b() as i16) else {
            continue;
        };
        if header >= branch_pc || bound == 0 || bound % 4 != 0 {
            continue;
        }
        let update = instructions[branch_pc - 1];
        if update.op() != Op::WasmI32AddImmediate
            || update.a() != induction
            || update.b() != induction
            || update.imm() as i32 != 2
        {
            continue;
        }
        let shift = instructions[header];
        if shift.op() != Op::WasmI32ShiftLeftImmediate
            || shift.b() != induction
            || shift.imm() != 2
            || shift.a() == induction
        {
            continue;
        }
        let scaled = shift.a();
        let Some(parameter_base) = parameter_registers else {
            continue;
        };
        if (usize::from(parameter_base)..usize::from(parameter_base) + plan.params.len())
            .contains(&usize::from(induction))
        {
            continue;
        }
        if instructions[..header]
            .iter()
            .any(|instruction| writes_register(*instruction, induction))
        {
            continue;
        }
        // The activation initializes locals to zero, which makes the
        // induction start valid only if the function reaches this header by
        // its ordinary prefix. Keep that prefix straight-line; a jump into
        // its middle could otherwise carry a value from an earlier visit to
        // the loop.
        if instructions[..header].iter().any(|instruction| {
            matches!(
                instruction.op().control_flow_layout(),
                ControlFlowLayout::Jump
                    | ControlFlowLayout::ConditionalJump
                    | ControlFlowLayout::BranchTable
                    | ControlFlowLayout::Terminal
            )
        }) {
            continue;
        }
        // The recognized loop has one straight-line body, one counted
        // backedge, and one fallthrough exit. Reject any external edge into
        // its prefix or interior so a scalar entry cannot carry a different
        // induction value through the one-time guard.
        let external_edge = instructions.iter().enumerate().any(|(pc, instruction)| {
            if (header..=branch_pc).contains(&pc) {
                return false;
            }
            let targets: Vec<usize> = match instruction.op().control_flow_layout() {
                ControlFlowLayout::Jump => vec![instruction.jump_target() as usize],
                ControlFlowLayout::ConditionalJump => {
                    vec![instruction.jump_target() as usize, pc + 1]
                }
                ControlFlowLayout::BranchTable => return true,
                ControlFlowLayout::Terminal => vec![],
                ControlFlowLayout::Call | ControlFlowLayout::Fallthrough => {
                    if instruction.returns_from_frame() {
                        vec![]
                    } else {
                        vec![pc + 1]
                    }
                }
            };
            targets
                .into_iter()
                .any(|target| target <= branch_pc && !(pc < header && target == pc + 1))
        });
        if external_edge {
            continue;
        }
        if (header..branch_pc).any(|pc| {
            !plan.reachable(pc)
                || matches!(
                    instructions[pc].op().control_flow_layout(),
                    ControlFlowLayout::Jump
                        | ControlFlowLayout::ConditionalJump
                        | ControlFlowLayout::BranchTable
                        | ControlFlowLayout::Terminal
                )
        }) || branch_pc + 1 >= instructions.len()
            || !plan.reachable(branch_pc)
            || !plan.reachable(branch_pc + 1)
        {
            continue;
        }

        let registers = usize::from(register_count);
        let mut addresses = vec![None; registers];
        let mut values = vec![None; registers];
        let mut loads = 0usize;
        let mut adds = 0usize;
        let mut stores = Vec::with_capacity(2);
        let mut address_adds = 0usize;
        let mut written = Vec::new();
        let mut supported = true;
        for (pc, instruction) in instructions
            .iter()
            .copied()
            .enumerate()
            .take(branch_pc)
            .skip(header)
        {
            if pc == header {
                written.push(instruction.a());
                continue;
            }
            let op = instruction.op();
            if op == Op::WasmI32AddImmediate {
                if pc == branch_pc - 1 {
                    written.push(instruction.a());
                    continue;
                }
                if instruction.b() != scaled || instruction.a() == scaled {
                    supported = false;
                    break;
                }
                if instruction.a() == induction {
                    supported = false;
                    break;
                }
                let offset = instruction.imm() as i32;
                if offset < 0 {
                    supported = false;
                    break;
                }
                let destination = usize::from(instruction.a());
                addresses[destination] = Some(offset as u32);
                values[destination] = None;
                address_adds += 1;
                written.push(instruction.a());
            } else if op == Op::WasmI32Load {
                if instruction.b() != memory_register
                    || instruction.imm() != 0
                    || instruction.a() == induction
                    || instruction.a() == scaled
                {
                    supported = false;
                    break;
                }
                let Some(offset) = addresses[usize::from(instruction.c())].take() else {
                    supported = false;
                    break;
                };
                let destination = usize::from(instruction.a());
                addresses[destination] = None;
                values[destination] = Some([offset, u32::MAX]);
                loads += 1;
                written.push(instruction.a());
            } else if op == Op::WasmI32Add {
                if instruction.a() == induction || instruction.a() == scaled {
                    supported = false;
                    break;
                }
                let (Some(left), Some(right)) = (
                    values[usize::from(instruction.b())].take(),
                    values[usize::from(instruction.c())].take(),
                ) else {
                    supported = false;
                    break;
                };
                if left[1] != u32::MAX || right[1] != u32::MAX {
                    supported = false;
                    break;
                }
                let destination = usize::from(instruction.a());
                addresses[destination] = None;
                values[destination] = Some([left[0], right[0]]);
                adds += 1;
                written.push(instruction.a());
            } else if op == Op::WasmI32Store {
                if instruction.b() != memory_register || instruction.imm() != 0 {
                    supported = false;
                    break;
                }
                let (Some(destination), Some(sources)) = (
                    addresses[usize::from(instruction.c())].take(),
                    values[usize::from(instruction.a())].take(),
                ) else {
                    supported = false;
                    break;
                };
                if sources[1] == u32::MAX {
                    supported = false;
                    break;
                }
                stores.push((destination, sources));
            } else {
                supported = false;
                break;
            }
        }
        if !supported || loads != 4 || adds != 2 || stores.len() != 2 || address_adds != 6 {
            continue;
        }
        let [(destination, sources), (second_destination, second_sources)] = stores.as_slice()
        else {
            continue;
        };
        if u64::from(*second_destination) != u64::from(*destination) + 4 {
            continue;
        }
        let (left, right) = if u64::from(second_sources[0]) == u64::from(sources[0]) + 4
            && u64::from(second_sources[1]) == u64::from(sources[1]) + 4
        {
            (sources[0], sources[1])
        } else if u64::from(second_sources[0]) == u64::from(sources[1]) + 4
            && u64::from(second_sources[1]) == u64::from(sources[0]) + 4
        {
            (sources[0], sources[1])
        } else {
            continue;
        };
        let range_end = |start: u32| u64::from(start) + u64::from(bound) * 4;
        let destination_end = range_end(*destination);
        let left_end = range_end(left);
        let right_end = range_end(right);
        let wasm_address_end = u64::from(u32::MAX) + 1;
        if destination_end > wasm_address_end
            || left_end > wasm_address_end
            || right_end > wasm_address_end
        {
            continue;
        }
        let disjoint = |first_start: u32, first_end: u64, second_start: u32, second_end: u64| {
            first_end <= u64::from(second_start) || second_end <= u64::from(first_start)
        };
        if !disjoint(*destination, destination_end, left, left_end)
            || !disjoint(*destination, destination_end, right, right_end)
        {
            continue;
        }
        // No loop-local register may be observed after the loop before a
        // scalar definition replaces it. This lets the vector loop jump
        // directly to the scalar loop's exit without manufacturing unused
        // interpreter temporaries.
        let mut pending = written;
        pending.sort_unstable();
        pending.dedup();
        let mut liveout_safe = true;
        for instruction in instructions.iter().copied().skip(branch_pc + 1) {
            if pending
                .iter()
                .any(|register| reads_register(instruction, *register))
            {
                liveout_safe = false;
                break;
            }
            if has_ambiguous_register_write(instruction) {
                liveout_safe = false;
                break;
            }
            pending.retain(|register| !definitely_writes_register(instruction, *register));
            if pending.is_empty() {
                break;
            }
            if !matches!(
                instruction.op().control_flow_layout(),
                ControlFlowLayout::Fallthrough | ControlFlowLayout::Terminal
            ) {
                liveout_safe = false;
                break;
            }
        }
        if !liveout_safe {
            continue;
        }
        let max_end = destination_end.max(left_end).max(right_end);
        return Some(AffinePairLoop {
            header,
            branch: branch_pc,
            exit: branch_pc + 1,
            induction,
            bound,
            destination: *destination,
            left,
            right,
            max_end,
        });
    }
    None
}

/// The register a direct access of `instruction` reads its memory through.
fn direct_access(op: Op) -> bool {
    MemoryLoad::from_direct_op(op).is_some() || MemoryStore::from_direct_op(op).is_some()
}

fn successors(function: &Function, pc: usize, instruction: WideInstruction) -> Vec<usize> {
    let mut successors = Vec::with_capacity(2);
    crate::bytecode::for_each_instruction_successor(pc, instruction, |successor| {
        if successor < function.code.len() {
            successors.push(successor);
        }
    });
    successors
}

/// The analysis context shared by every function of a program.
struct Analysis<'a, H: Host> {
    vm: &'a Vm<H>,
    program: ProgramId,
    code: &'a ResidualProgram,
    env: Value,
    /// Whether imported calls may compile: a callee may grow memory, which
    /// moves an unguarded memory's bytes.
    callouts: bool,
    /// `unreachable` is enabled only for a bounded, explicitly selected root
    /// experiment until its compile-time memory cost is measured.
    experimental_root_compilation: bool,
    /// A root-only function may call out only while callouts are safe for its
    /// current memory view. A VM callee can grow unguarded memory.
    root_only: bool,
    /// Rejection reasons exist only for explicit diagnostic runs.
    compile_rejections: Option<&'a std::cell::RefCell<Vec<Option<String>>>>,
}

impl<H: Host> Analysis<'_, H> {
    fn reject(&self, index: usize, reason: impl FnOnce() -> String) {
        let Some(rejections) = self.compile_rejections else {
            return;
        };
        let mut rejections = rejections.borrow_mut();
        if let Some(rejection) = rejections.get_mut(index)
            && rejection.is_none()
        {
            let reason = reason();
            *rejection = Some(reason.clone());
            eprintln!("QUENCH_WASM_JIT_REJECTION {index} {reason}");
        }
    }

    fn constant(&self, index: usize) -> Option<Value> {
        self.vm.programs.constant(self.program, index)
    }

    /// The machine kind of a constant: i32-encoded scalars, boxed 64-bit
    /// scalars, and any other constant (the void result) as an inert i32.
    fn constant_kind(&self, value: Value) -> Kind {
        match self.vm.decode_bits64(value) {
            Some(_) => Kind::I64,
            None => Kind::I32,
        }
    }

    fn constant_bits(&self, value: Value) -> i64 {
        match self.vm.decode_bits64(value) {
            Some(bits) => bits as i64,
            None => i64::from(value.wasm_bits32() as i32),
        }
    }

    /// The type of the global bound at environment slot `slot`.
    fn global_type(&self, slot: u32) -> Option<crate::WasmType> {
        let binding = self.vm.heap.environment_slot(self.env, slot as usize)?;
        match self.vm.heap.get(binding)? {
            Cell::WasmGlobal { ty, .. } => Some(*ty),
            _ => None,
        }
    }

    /// Whether `slot` binds an unshared memory compiled code can view.
    fn binds_unshared_memory(&self, slot: u32) -> bool {
        let Some(binding) = self.vm.heap.environment_slot(self.env, slot as usize) else {
            return false;
        };
        matches!(
            self.vm.heap.get(binding),
            Some(Cell::WasmMemory { bytes, .. })
                if matches!(**bytes, crate::wasm::memory::MemoryStorage::Unshared(_))
        )
    }

    /// Whether a bound memory has the representation supported by native
    /// linear-memory operations: unshared memory32 with ordinary Wasm pages.
    fn supports_compiled_memory(&self, slot: u32) -> bool {
        let Some(binding) = self.vm.heap.environment_slot(self.env, slot as usize) else {
            return false;
        };
        matches!(
            self.vm.heap.get(binding),
            Some(Cell::WasmMemory { bytes, header, .. })
                if matches!(**bytes, crate::wasm::memory::MemoryStorage::Unshared(_))
                    && !header.ty.memory64
                    && header.ty.page_size() as usize
                        == (1usize << crate::wasm::memory::WASM_PAGE_BITS)
        )
    }

    fn binds_table32(&self, slot: u32) -> bool {
        let Some(binding) = self.vm.heap.environment_slot(self.env, slot as usize) else {
            return false;
        };
        matches!(
            self.vm.heap.get(binding),
            Some(Cell::WasmTable { table64: false, .. })
        )
    }

    fn signature(&self, index: u32) -> Option<&crate::WasmSignature> {
        self.vm
            .programs
            .wasm_function_signature(self.program, index)
    }

    /// The signature of `function` when it is imported.
    fn import_signature(&self, function: u32) -> Option<&crate::WasmSignature> {
        let signatures = self.vm.programs.wasm_signatures(self.program)?;
        signatures
            .defined_index(function)
            .is_none()
            .then(|| signatures.get(function as usize))?
    }

    fn type_signature(&self, index: u32) -> Option<&crate::WasmSignature> {
        self.vm
            .programs
            .wasm_signatures(self.program)?
            .type_signature(index)
    }

    /// The function's plan, assuming every function in `callable` compiles.
    fn plan(&self, index: usize, callable: &[bool], memory_slot: &mut Option<u32>) -> Option<Plan> {
        let plan_started = self
            .compile_rejections
            .as_ref()
            .map(|_| std::time::Instant::now());
        let memory_before = plan_started.and_then(|_| jit_memory_snapshot());
        let function = &self.code.function(index);
        let Some(signature) = self.signature(index as u32) else {
            self.reject(index, || "missing-signature".into());
            return None;
        };
        if function.locals != 0 {
            self.reject(index, || format!("frame-locals:{}", function.locals));
            return None;
        }
        if signature.results.len() > 1 {
            self.reject(index, || {
                format!("multi-value-results:{}", signature.results.len())
            });
            return None;
        }
        let params = signature.params.clone();
        let result = signature.results.first().copied();
        if let Some(ty) = params
            .iter()
            .chain(result.iter())
            .find(|ty| Kind::of(**ty).is_none())
        {
            self.reject(index, || format!("unsupported-signature-type:{ty:?}"));
            return None;
        }
        let Some(parameter_base) = function.parameter_registers.map(usize::from) else {
            self.reject(index, || "missing-parameter-register-base".into());
            return None;
        };
        let registers = usize::from(function.registers);
        if parameter_base + params.len() > registers {
            self.reject(index, || "parameters-outside-register-window".into());
            return None;
        }
        let memory_register = super::dispatch_fast::memory_register(function);
        let instructions: Vec<WideInstruction> = function
            .code
            .iter()
            .map(|packed| super::dispatch_fast::decoded_instruction(function, *packed))
            .collect();
        let mut entry = AnalysisState::new();
        for (offset, ty) in params.iter().enumerate() {
            entry.set(
                u16::try_from(parameter_base + offset).ok()?,
                function.registers,
                Slot::Value(Kind::of(*ty)?),
            )?;
        }
        let mut states: Vec<Option<AnalysisState>> = vec![None; instructions.len()];
        states[0] = Some(entry);
        let mut work = vec![0usize];
        while let Some(pc) = work.pop() {
            let instruction = instructions[pc];
            let mut state = states[pc]
                .clone()
                .expect("queued instructions have a state");
            if instruction.op() == Op::CallKnown
                && !callable
                    .get(usize::from(instruction.known_function_index()))
                    .copied()
                    .unwrap_or(false)
                && !self.experimental_root_compilation
            {
                let callee = instruction.known_function_index();
                self.reject(index, || format!("uncompiled-callee:{callee}@{pc}"));
                return None;
            }
            if self
                .transfer(
                    function,
                    instruction,
                    &mut state,
                    callable,
                    memory_register,
                    memory_slot,
                    result,
                )
                .is_none()
            {
                let op = instruction.op();
                self.reject(index, || format!("unsupported-instruction:{op:?}@{pc}"));
                return None;
            }
            for successor in successors(function, pc, instruction) {
                if successor >= instructions.len() {
                    self.reject(index, || format!("invalid-successor:{successor}@{pc}"));
                    return None;
                }
                let changed = match &mut states[successor] {
                    Some(existing) => existing.join_from(&state),
                    target @ None => {
                        *target = Some(state.clone());
                        true
                    }
                };
                if changed {
                    work.push(successor);
                }
            }
        }
        let operands = states
            .iter()
            .zip(&instructions)
            .map(|(state, instruction)| {
                let state = state.as_ref()?;
                Some(
                    state
                        .get(instruction.b(), function.registers)
                        .unwrap_or(Slot::Conflict),
                )
            })
            .collect();
        if let Some(started) = plan_started {
            let reachable = states.iter().flatten().count();
            let stored_slots: usize = states.iter().flatten().map(|state| state.slots.len()).sum();
            let slot_capacity: usize = states
                .iter()
                .flatten()
                .map(|state| state.slots.capacity())
                .sum();
            let dense_state_bytes = reachable
                .saturating_mul(registers)
                .saturating_mul(std::mem::size_of::<Slot>());
            let sparse_state_bytes = states
                .len()
                .saturating_mul(std::mem::size_of::<Option<AnalysisState>>())
                .saturating_add(slot_capacity.saturating_mul(std::mem::size_of::<(u16, Slot)>()));
            let memory_after = jit_memory_snapshot();
            eprintln!(
                "QUENCH_WASM_JIT_PLAN_MEMORY function={index} bytecodes={} registers={registers} reachable={reachable} stored_slots={stored_slots} dense_state_bytes={dense_state_bytes} sparse_state_bytes={sparse_state_bytes} elapsed_us={} rss_kib_before={:?} rss_kib_after={:?} hwm_kib_before={:?} hwm_kib_after={:?}",
                instructions.len(),
                started.elapsed().as_micros(),
                memory_before.map(|memory| memory.0),
                memory_after.map(|memory| memory.0),
                memory_before.map(|memory| memory.1),
                memory_after.map(|memory| memory.1),
            );
        }
        Some(Plan {
            params,
            result,
            operands,
        })
    }

    /// Check `instruction` has a compiled denotation in `state`, and apply its
    /// register writes.
    #[allow(clippy::too_many_arguments)]
    fn transfer(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        callable: &[bool],
        memory_register: Option<u16>,
        memory_slot: &mut Option<u32>,
        result: Option<crate::WasmType>,
    ) -> Option<()> {
        let op = instruction.op();
        if op != Op::Return && instruction.returns_from_frame() {
            return None;
        }
        if !super::dispatch_fast::lane_fields_fit(instruction, function) {
            return None;
        }
        let value =
            |state: &AnalysisState, register: u16| match state.get(register, function.registers)? {
                Slot::Value(kind) => Some(kind),
                _ => None,
            };
        let i32_value = |state: &AnalysisState, register: u16| {
            (value(state, register)? == Kind::I32).then_some(())
        };
        let set = |state: &mut AnalysisState, register: u16, slot: Slot| {
            state.set(register, function.registers, slot)
        };
        // The memory register is read only by direct accesses through it.
        if memory_register.is_some_and(|memory| {
            !direct_access(op)
                && !matches!(op, Op::WasmMemorySize | Op::WasmMemoryFill)
                && op != Op::WasmInstanceBinding
                && crate::bytecode::InstructionField::ALL.iter().any(|field| {
                    op.field_layout(*field).reads_register()
                        && instruction.field_value(*field) == memory
                })
        }) {
            return None;
        }
        if let Some(_operator) = I32BinaryOperator::from_register_op(op) {
            i32_value(state, instruction.b())?;
            i32_value(state, instruction.c())?;
            return set(state, instruction.a(), Slot::Value(Kind::I32));
        }
        if I32BinaryOperator::from_immediate_op(op).is_some() {
            i32_value(state, instruction.b())?;
            return set(state, instruction.a(), Slot::Value(Kind::I32));
        }
        if I32BinaryOperator::from_jump_op(op).is_some() {
            i32_value(state, instruction.a())?;
            return i32_value(state, instruction.b());
        }
        if I32BinaryOperator::from_immediate_jump_op(op).is_some() {
            return i32_value(state, instruction.a());
        }
        if let Some(load) = MemoryLoad::from_direct_op(op) {
            if Some(instruction.b()) != memory_register
                || state.get(instruction.b(), function.registers) != Some(Slot::Memory)
            {
                return None;
            }
            i32_value(state, instruction.c())?;
            return set(
                state,
                instruction.a(),
                Slot::Value(Kind::of(load_type(load)?)?),
            );
        }
        if let Some(store) = MemoryStore::from_direct_op(op) {
            if Some(instruction.b()) != memory_register
                || state.get(instruction.b(), function.registers) != Some(Slot::Memory)
            {
                return None;
            }
            i32_value(state, instruction.c())?;
            return (value(state, instruction.a())? == Kind::of(store.value_type())?).then_some(());
        }
        // Result widths are read off the operators' own semantics:
        // comparisons produce an i32.
        if let Some((operator, ty)) = float_binary_operator(op, instruction.imm()) {
            let kind = Kind::of(ty)?;
            (value(state, instruction.b())? == kind).then_some(())?;
            (value(state, instruction.c())? == kind).then_some(())?;
            let comparison = operator.apply(0.0, 0.0).ty() == crate::WasmType::I32;
            let result = if comparison { Kind::I32 } else { kind };
            return set(state, instruction.a(), Slot::Value(result));
        }
        if let Some((_, ty)) = float_unary_operator(op, instruction.imm()) {
            let kind = Kind::of(ty)?;
            (value(state, instruction.b())? == kind).then_some(())?;
            return set(state, instruction.a(), Slot::Value(kind));
        }
        match op {
            Op::WasmI64Binary => {
                let operator = I64BinaryOperator::from_tag(instruction.imm())?;
                (value(state, instruction.b())? == Kind::I64).then_some(())?;
                (value(state, instruction.c())? == Kind::I64).then_some(())?;
                let result = Kind::of(operator.apply(1, 1).ok()?.ty())?;
                set(state, instruction.a(), Slot::Value(result))
            }
            Op::WasmI64Unary => {
                let operator = I64UnaryOperator::from_tag(instruction.imm())?;
                (value(state, instruction.b())? == Kind::I64).then_some(())?;
                let result = Kind::of(operator.apply(0).ty())?;
                set(state, instruction.a(), Slot::Value(result))
            }
            Op::WasmScalarConvert => {
                let operator = ScalarConversionOperator::from_tag(instruction.imm())?;
                (value(state, instruction.b())? == Kind::of(operator.source_type())?)
                    .then_some(())?;
                let result = Kind::of(operator.result_type())?;
                set(state, instruction.a(), Slot::Value(result))
            }
            Op::Move => {
                let kind = value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(kind))
            }
            Op::LoadConst => {
                let constant = self.constant(instruction.constant_index())?;
                set(
                    state,
                    instruction.a(),
                    Slot::Value(self.constant_kind(constant)),
                )
            }
            Op::WasmFillRegisters => {
                let constant = self.constant(instruction.constant_index())?;
                let kind = self.constant_kind(constant);
                let window = instruction.register_window();
                for offset in 0..window.count {
                    set(state, window.base + offset, Slot::Value(kind))?;
                }
                Some(())
            }
            Op::WasmI32Unary => {
                I32UnaryOperator::from_tag(instruction.imm())?;
                i32_value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmI32ShiftRightUnsignedAndImmediate => {
                i32_value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmI32AddImmediateJumpNonZero => {
                i32_value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmJumpI32Zero | Op::WasmJumpI32NonZero => i32_value(state, instruction.a()),
            Op::Jump => Some(()),
            Op::WasmBranchTable => {
                i32_value(state, instruction.a())?;
                let pc_entries: Vec<_> =
                    crate::bytecode::branch_table_entries(0, instruction).collect();
                // Entries are `Jump`s; validation guarantees it for
                // reachable tables.
                (!pc_entries.is_empty()).then_some(())
            }
            Op::WasmSelect => {
                let (condition, _) = instruction.register_pair();
                i32_value(state, condition)?;
                let kind = value(state, instruction.b())?;
                (value(state, instruction.c())? == kind).then_some(())?;
                set(state, instruction.a(), Slot::Value(kind))
            }
            Op::WasmMemorySize => {
                let memory = instruction.b();
                if Some(memory) != memory_register
                    || state.get(memory, function.registers) != Some(Slot::Memory)
                    || !memory_slot.is_some_and(|slot| self.supports_compiled_memory(slot))
                {
                    return None;
                }
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmMemoryFill => {
                let memory = instruction.a();
                let (destination, _) = instruction.register_pair();
                if Some(memory) != memory_register
                    || state.get(memory, function.registers) != Some(Slot::Memory)
                    || !memory_slot.is_some_and(|slot| self.supports_compiled_memory(slot))
                {
                    return None;
                }
                i32_value(state, instruction.b())?;
                i32_value(state, instruction.c())?;
                i32_value(state, destination)
            }
            Op::WasmIndirectTarget if self.callouts => {
                let Slot::Binding(table) = state.get(instruction.b(), function.registers)? else {
                    return None;
                };
                if !self.binds_table32(table) {
                    return None;
                }
                i32_value(state, instruction.c())?;
                self.type_signature(instruction.imm())?;
                set(
                    state,
                    instruction.a(),
                    Slot::IndirectReference {
                        signature: instruction.imm(),
                        table,
                    },
                )
            }
            Op::CallKnown => {
                let callee = u32::from(instruction.known_function_index());
                if !callable.get(callee as usize).copied().unwrap_or(false)
                    && (!self.experimental_root_compilation || (self.root_only && !self.callouts))
                {
                    return None;
                }
                let signature = self.signature(callee)?;
                if signature.results.len() > 1
                    && !callable.get(callee as usize).copied().unwrap_or(false)
                {
                    // The VM callout ABI currently carries at most one result.
                    // Do not admit a residual fallback that would silently
                    // discard the remaining Wasm results.
                    return None;
                }
                let window = instruction.call_window();
                if usize::from(window.count) != signature.params.len() {
                    return None;
                }
                for (offset, ty) in signature.params.iter().enumerate() {
                    (value(state, window.base + offset as u16)? == Kind::of(*ty)?).then_some(())?;
                }
                let kind = match signature.results.first() {
                    Some(ty) => Kind::of(*ty)?,
                    None => Kind::I32,
                };
                set(state, instruction.result_register(), Slot::Value(kind))
            }
            Op::WasmRefFunc if self.callouts => {
                let function = instruction.imm();
                self.import_signature(function)?;
                set(state, instruction.a(), Slot::Import(function))
            }
            Op::Call => {
                let slot = state.get(instruction.b(), function.registers)?;
                let signature = match slot {
                    Slot::Import(function) => self.import_signature(function)?,
                    Slot::IndirectReference { signature, .. } if self.callouts => {
                        self.type_signature(signature)?
                    }
                    _ => return None,
                };
                let window = instruction.call_window();
                if usize::from(window.count) != signature.params.len()
                    || signature.results.len() > 1
                {
                    return None;
                }
                for (offset, ty) in signature.params.iter().enumerate() {
                    (value(state, window.base + offset as u16)? == Kind::of(*ty)?).then_some(())?;
                }
                let kind = match signature.results.first() {
                    Some(ty) => Kind::of(*ty)?,
                    None => Kind::I32,
                };
                set(state, instruction.result_register(), Slot::Value(kind))
            }
            Op::WasmUnreachable if self.experimental_root_compilation => Some(()),
            Op::Return => match result {
                Some(ty) => (value(state, instruction.a())? == Kind::of(ty)?).then_some(()),
                None => Some(()),
            },
            Op::WasmInstanceBinding => {
                let slot = instruction.imm();
                if Some(instruction.a()) == memory_register {
                    if !self.binds_unshared_memory(slot) || memory_slot.is_some_and(|s| s != slot) {
                        return None;
                    }
                    *memory_slot = Some(slot);
                    return set(state, instruction.a(), Slot::Memory);
                }
                set(state, instruction.a(), Slot::Binding(slot))
            }
            // Compiled code reads and writes a 32-bit global's encoded value
            // in place; a 64-bit global's value is a box, so it stays
            // interpreted.
            Op::WasmGlobalGet => {
                let Slot::Binding(slot) = state.get(instruction.b(), function.registers)? else {
                    return None;
                };
                (Kind::of(self.global_type(slot)?)? == Kind::I32).then_some(())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmGlobalSet => {
                let Slot::Binding(slot) = state.get(instruction.b(), function.registers)? else {
                    return None;
                };
                (Kind::of(self.global_type(slot)?)? == Kind::I32).then_some(())?;
                (value(state, instruction.a())? == Kind::I32).then_some(())
            }
            _ => None,
        }
    }
}

/// The Wasm type a direct load produces.
fn load_type(load: MemoryLoad) -> Option<crate::WasmType> {
    Some(match load {
        MemoryLoad::I32Load
        | MemoryLoad::I32Load8S
        | MemoryLoad::I32Load8U
        | MemoryLoad::I32Load16S
        | MemoryLoad::I32Load16U => crate::WasmType::I32,
        MemoryLoad::F32Load => crate::WasmType::F32,
        MemoryLoad::I64Load
        | MemoryLoad::I64Load8S
        | MemoryLoad::I64Load8U
        | MemoryLoad::I64Load16S
        | MemoryLoad::I64Load16U
        | MemoryLoad::I64Load32S
        | MemoryLoad::I64Load32U => crate::WasmType::I64,
        MemoryLoad::F64Load => crate::WasmType::F64,
        _ => return None,
    })
}

/// Whether a direct load sign-extends its narrow word.
fn load_signed(load: MemoryLoad) -> bool {
    matches!(
        load,
        MemoryLoad::I32Load8S
            | MemoryLoad::I32Load16S
            | MemoryLoad::I64Load8S
            | MemoryLoad::I64Load16S
            | MemoryLoad::I64Load32S
    )
}

/// The width in bytes a direct store writes.
fn store_width(store: MemoryStore) -> usize {
    match store {
        MemoryStore::I32Store8 | MemoryStore::I64Store8 => 1,
        MemoryStore::I32Store16 | MemoryStore::I64Store16 => 2,
        MemoryStore::I32Store | MemoryStore::F32Store | MemoryStore::I64Store32 => 4,
        _ => 8,
    }
}

/// Disjoint state compiled code accesses: an access to one never aliases an
/// access to another, so loads of one survive stores to the other.
#[derive(Clone, Copy, Debug)]
enum Region {
    /// The instance's linear memory.
    LinearMemory,
    /// Global values, which live in heap cells.
    Globals,
}

impl Region {
    const ALL: [Self; 2] = [Self::LinearMemory, Self::Globals];
    const COUNT: usize = Self::ALL.len();
}

/// Flags of a load of context state fixed for the whole call (the stack
/// limit, the callout, global addresses): it may be reused and hoisted.
fn invariant() -> MemFlagsData {
    MemFlagsData::trusted().with_readonly().with_can_move()
}

/// The memory facts a compiled call passes after the context: the base
/// unless the program fixes it as a constant, and the length unless the
/// guard bounds accesses. One program's functions share them.
#[derive(Clone, Copy)]
struct MemoryArguments {
    base: bool,
    length: bool,
}

impl MemoryArguments {
    fn of(fixed_base: Option<i32>, guarded: bool) -> Self {
        Self {
            base: fixed_base.is_none(),
            length: !guarded,
        }
    }

    /// The passed ones of `base` and `length`, in parameter order.
    fn select(self, base: ir::Value, length: ir::Value) -> impl Iterator<Item = ir::Value> {
        [self.base.then_some(base), self.length.then_some(length)]
            .into_iter()
            .flatten()
    }

    fn count(self) -> usize {
        usize::from(self.base) + usize::from(self.length)
    }
}

/// The internal signature of a compiled function: the context, the memory
/// arguments, then the Wasm parameters.
fn internal_signature(
    memory: MemoryArguments,
    plan_params: &[crate::WasmType],
    result: Option<crate::WasmType>,
    call_conv: cranelift_codegen::isa::CallConv,
) -> Signature {
    let mut signature = Signature::new(call_conv);
    for _ in 0..1 + memory.count() {
        signature.params.push(AbiParam::new(types::I64));
    }
    for ty in plan_params {
        signature
            .params
            .push(AbiParam::new(Kind::of(*ty).expect("planned").ir()));
    }
    signature.returns.push(AbiParam::new(types::I64));
    if let Some(ty) = result {
        signature
            .returns
            .push(AbiParam::new(Kind::of(ty).expect("planned").ir()));
    }
    signature
}

/// Instructions a threaded copy may repeat between a constant's jump and the
/// branch table the constant decides; bounds the code each copy adds.
const THREAD_BUDGET: usize = 16;

/// Where a threaded copy continues once its path is done.
#[derive(Clone, Copy)]
enum ThreadEnd {
    /// The branch table's case for the constant.
    Case(usize),
    /// The successor the constant decided a branch to.
    Exit(usize),
    /// The table at the path's end, run for real: a jump that knows no
    /// selector gets its own copy of the dispatch (`dispatch_path`), so each
    /// case's back edge predicts its own successor.
    Dispatch,
}

/// Instructions all copies of an unrolled loop body may hold together, and the most copies.
/// A body of up to a fourth of the budget runs as four copies: one interrupt check, one back
/// edge and one set of loop-carried moves per four iterations (V8's optimizing tier unrolls
/// small innermost Wasm loops for the same reasons); a body of more than half of it is not
/// unrolled.
const UNROLL_BUDGET: usize = 48;
const MAX_UNROLL_FACTOR: usize = 4;

/// Bytecodes up to which a compiled Wasm program unrolls its innermost loops. Every function
/// compiles eagerly, and each copy is compiled whether its loop runs or not: in programs of
/// 17K to 70K bytecodes the copies cost more compile time than they saved (TSF's first
/// iteration +6%, gcc-loops +2.5%, HashSet +1.2% per iteration from spills), so only smaller
/// programs pay for them.
const UNROLL_PROGRAM_BYTECODES: usize = 16_384;

/// Whether `program` is small enough to unroll its innermost loops (`UNROLL_PROGRAM_BYTECODES`).
fn program_unrolls(program: &ResidualProgram) -> bool {
    // A function whose record has not been read yet (compiled again or decoded on first
    // read) is not counted.
    program
        .functions
        .iter()
        .filter_map(crate::bytecode::FunctionSlot::record)
        .map(|function| function.code.len())
        .sum::<usize>()
        <= UNROLL_PROGRAM_BYTECODES
}

/// An innermost loop unrolled at its latch: the back edge enters the next of `factor - 1`
/// copies of the body, and the last copy's back edge returns to `header` with the loop's
/// interrupt check (`Emitter::backedge`).
#[derive(Clone, Copy)]
struct UnrolledLoop {
    header: usize,
    factor: usize,
}

/// Whether `op` may sit in an unrolled body: no call, and no operation that calls out to the
/// VM or ends the body other than by a branch out of the loop.
fn unrollable_op(op: Op) -> bool {
    !matches!(
        op,
        Op::WasmMemoryGrow
            | Op::WasmMemoryFill
            | Op::WasmMemoryCopy
            | Op::WasmMemoryInit
            | Op::WasmTableGet
            | Op::WasmTableSet
            | Op::WasmTableGrow
            | Op::WasmTableFill
            | Op::WasmTableCopy
            | Op::WasmTableInit
            | Op::WasmTableSize
            | Op::WasmIndirectTarget
    )
}

/// The innermost loops to unroll, by latch: a back edge from `latch` to `header` whose body
/// `header..=latch` runs straight through but for branches that leave the loop, has no other
/// entry, and fits the budget at least twice.
fn unrollable_loops(instructions: &[WideInstruction], plan: &Plan) -> Vec<Option<UnrolledLoop>> {
    let mut entered = vec![false; instructions.len() + 1];
    for (pc, instruction) in instructions.iter().enumerate() {
        if !plan.reachable(pc) {
            continue;
        }
        match instruction.op().control_flow_layout() {
            ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump => {
                entered[instruction.jump_target() as usize] = true;
            }
            ControlFlowLayout::BranchTable => {
                for entry in crate::bytecode::branch_table_entries(pc, *instruction) {
                    entered[instructions[entry].jump_target() as usize] = true;
                }
            }
            _ => {}
        }
    }
    let mut loops = vec![None; instructions.len()];
    for (latch, instruction) in instructions.iter().enumerate() {
        let layout = instruction.op().control_flow_layout();
        if !plan.reachable(latch)
            || !matches!(layout, ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump)
        {
            continue;
        }
        let header = instruction.jump_target() as usize;
        if header > latch {
            continue;
        }
        let length = latch - header + 1;
        let factor = (UNROLL_BUDGET / length).min(MAX_UNROLL_FACTOR);
        if factor < 2 {
            continue;
        }
        let straight = (header..latch).all(|pc| {
            let body = instructions[pc];
            plan.reachable(pc)
                && unrollable_op(body.op())
                && match body.op().control_flow_layout() {
                    ControlFlowLayout::Fallthrough => true,
                    ControlFlowLayout::ConditionalJump => body.jump_target() as usize > latch,
                    _ => false,
                }
        });
        if straight && !entered[header + 1..=latch].iter().any(|&entered| entered) {
            loops[latch] = Some(UnrolledLoop { header, factor });
        }
    }
    loops
}

/// A copy of the instructions from a jump target to a branch table, for a
/// jump that leaves the table's selector holding a known constant: along
/// the copy the selector cannot change, so branches on it are decided and
/// the table becomes a direct jump (jump threading; a state machine's
/// dispatch on the state it just set disappears). The copy repeats each
/// instruction at its own PC and exits only to block leaders.
struct Thread {
    selector: u16,
    value: i32,
    path: Vec<usize>,
    end: ThreadEnd,
}

/// Whether `instruction` may write `register`; windows and other register
/// groups count as writing every register.
fn writes_register(instruction: WideInstruction, register: u16) -> bool {
    use crate::bytecode::{FieldLayout, InstructionField};
    InstructionField::ALL.iter().any(|field| {
        let operand = match field {
            InstructionField::A => instruction.a(),
            InstructionField::B => instruction.b(),
            InstructionField::C => instruction.c(),
        };
        match instruction.op().field_layout(*field) {
            FieldLayout::ResultRegister
            | FieldLayout::WriteRegister
            | FieldLayout::ReadWriteRegister => operand == register,
            FieldLayout::RegisterWindowBase
            | FieldLayout::RegisterCount
            | FieldLayout::ConstructArguments
            | FieldLayout::NumericLocalTarget => true,
            _ => false,
        }
    })
}

/// Whether the instruction definitely replaces `register` with a new value.
/// Register windows are excluded because the base alone does not prove that a
/// particular register in the group is overwritten.
fn definitely_writes_register(instruction: WideInstruction, register: u16) -> bool {
    use crate::bytecode::{FieldLayout, InstructionField};
    InstructionField::ALL.iter().any(|field| {
        let operand = instruction.field_value(*field);
        matches!(
            instruction.op().field_layout(*field),
            FieldLayout::ResultRegister
                | FieldLayout::WriteRegister
                | FieldLayout::ReadWriteRegister
        ) && operand == register
    })
}

/// Whether an instruction contains a register-group write whose exact set of
/// destinations is not represented by `definitely_writes_register`.
fn has_ambiguous_register_write(instruction: WideInstruction) -> bool {
    use crate::bytecode::{FieldLayout, InstructionField};
    InstructionField::ALL.iter().any(|field| {
        matches!(
            instruction.op().field_layout(*field),
            FieldLayout::RegisterWindowBase
                | FieldLayout::RegisterCount
                | FieldLayout::ConstructArguments
                | FieldLayout::NumericLocalTarget
        )
    })
}

/// Whether `instruction` reads `register`, including an immediate register
/// pair or a register window. Window reads are deliberately conservative:
/// they prevent a vector loop from bypassing any interpreter temporary that
/// could be observed after its exit.
fn reads_register(instruction: WideInstruction, register: u16) -> bool {
    use crate::bytecode::{FieldLayout, ImmediateLayout, InstructionField};
    let op = instruction.op();
    if InstructionField::ALL.iter().copied().any(|field| {
        let layout = op.field_layout(field);
        if !layout.reads_register() {
            return false;
        }
        match layout {
            FieldLayout::RegisterWindowBase
            | FieldLayout::RegisterCount
            | FieldLayout::ConstructArguments
            | FieldLayout::NumericLocalTarget => true,
            _ => instruction.field_value(field) == register,
        }
    }) {
        return true;
    }
    if op.immediate_layout() == ImmediateLayout::RegisterPair {
        let (first, second) = ImmediateLayout::register_pair(instruction.imm());
        return first == register || second == register;
    }
    false
}

/// Whether conditional jump `instruction` is taken when `register` holds
/// `value`, if that alone decides it.
fn decided_branch(instruction: WideInstruction, register: u16, value: i32) -> Option<bool> {
    let op = instruction.op();
    if instruction.a() != register {
        return None;
    }
    if let Some(operator) = I32BinaryOperator::from_immediate_jump_op(op) {
        let right = i32::from(instruction.b() as i16);
        return operator.evaluate(value, right).ok().map(|holds| holds != 0);
    }
    match op {
        Op::WasmJumpI32Zero => Some(value == 0),
        Op::WasmJumpI32NonZero => Some(value != 0),
        _ => None,
    }
}

/// The threaded copy from `start` for `selector` holding `value`, when one
/// reaches a branch table on the selector, or a branch the value decides,
/// within the budget.
fn thread_path(
    instructions: &[WideInstruction],
    plan: &Plan,
    start: usize,
    selector: u16,
    value: i32,
) -> Option<Thread> {
    fn walk(
        instructions: &[WideInstruction],
        plan: &Plan,
        pc: usize,
        selector: u16,
        value: i32,
        path: &mut Vec<usize>,
    ) -> Option<ThreadEnd> {
        if path.len() == THREAD_BUDGET || !plan.reachable(pc) {
            return None;
        }
        let instruction = *instructions.get(pc)?;
        if writes_register(instruction, selector) {
            return None;
        }
        path.push(pc);
        let end = match instruction.op().control_flow_layout() {
            ControlFlowLayout::BranchTable if instruction.a() == selector => {
                let entries: Vec<usize> = crate::bytecode::branch_table_entries(pc, instruction)
                    .map(|entry| instructions[entry].jump_target() as usize)
                    .collect();
                let (default, cases) = entries.split_last()?;
                let case = usize::try_from(value as u32).ok();
                Some(ThreadEnd::Case(
                    *case.and_then(|case| cases.get(case)).unwrap_or(default),
                ))
            }
            ControlFlowLayout::Fallthrough => {
                walk(instructions, plan, pc + 1, selector, value, path)
            }
            ControlFlowLayout::Jump => walk(
                instructions,
                plan,
                instruction.jump_target() as usize,
                selector,
                value,
                path,
            ),
            ControlFlowLayout::ConditionalJump => {
                let target = instruction.jump_target() as usize;
                match decided_branch(instruction, selector, value) {
                    Some(taken) => {
                        let next = if taken { target } else { pc + 1 };
                        let length = path.len();
                        walk(instructions, plan, next, selector, value, path).or_else(|| {
                            path.truncate(length);
                            Some(ThreadEnd::Exit(next))
                        })
                    }
                    None => [target, pc + 1].into_iter().find_map(|next| {
                        let length = path.len();
                        walk(instructions, plan, next, selector, value, path).or_else(|| {
                            path.truncate(length);
                            None
                        })
                    }),
                }
            }
            _ => None,
        };
        if end.is_none() {
            path.pop();
        }
        end
    }
    let mut path = Vec::new();
    let end = walk(instructions, plan, start, selector, value, &mut path)?;
    Some(Thread {
        selector,
        value,
        path,
        end,
    })
}

/// The straight-line copy from jump target `start` to the branch table it
/// reaches, within the threading budget; the table is the path's last
/// instruction.
fn dispatch_path(instructions: &[WideInstruction], plan: &Plan, start: usize) -> Option<Thread> {
    let mut path = Vec::new();
    let mut pc = start;
    loop {
        if path.len() == THREAD_BUDGET || !plan.reachable(pc) {
            return None;
        }
        let instruction = *instructions.get(pc)?;
        path.push(pc);
        pc = match instruction.op().control_flow_layout() {
            ControlFlowLayout::Fallthrough => pc + 1,
            ControlFlowLayout::Jump => instruction.jump_target() as usize,
            ControlFlowLayout::BranchTable => {
                return Some(Thread {
                    selector: instruction.a(),
                    value: 0,
                    path,
                    end: ThreadEnd::Dispatch,
                });
            }
            _ => return None,
        };
    }
}

/// The core every native body lowers through, whichever frontend's bytecode it compiles: the
/// builder, each register's typed views, the call context, and the interrupt check.
///
/// A register has one SSA variable per view type (`VIEW_TYPES`). Writing a value defines its
/// own view and the other view of the same bits (`counterpart`); a view nothing reads costs
/// nothing, since its conversion is dead. A Wasm value is its scalar; a JavaScript Number a
/// region holds unboxed is an `i32` or an `f64` view.
struct Body<'b> {
    builder: FunctionBuilder<'b>,
    /// Per register, its views, declared on first use.
    variables: Vec<Option<[Variable; VIEW_TYPES.len()]>>,
    /// The call context (`CallContext`).
    context: ir::Value,
    /// The VM's poll page when every entry runs through the fault-resuming trampoline: an
    /// interrupt check is then one read of it (`poll_address`).
    poll: Option<i64>,
    /// Each i32 read from a register's view, by the zero-extended value it was read from.
    zero_extended: std::collections::HashMap<ir::Value, ir::Value>,
    /// Where a flag check that finds termination requested goes; the frontend fills it with
    /// the terminated status (`TERMINATED_STATUS`).
    termination_block: Option<ir::Block>,
}

impl<'b> Body<'b> {
    fn new(builder: FunctionBuilder<'b>, context: ir::Value, poll: Option<i64>) -> Self {
        Self {
            builder,
            variables: Vec::new(),
            context,
            poll,
            zero_extended: std::collections::HashMap::new(),
            termination_block: None,
        }
    }

    /// The register's views.
    fn views(&mut self, register: u16) -> [Variable; VIEW_TYPES.len()] {
        let index = usize::from(register);
        if self.variables.len() <= index {
            self.variables.resize(index + 1, None);
        }
        if let Some(views) = self.variables[index] {
            return views;
        }
        let views = VIEW_TYPES.map(|ty| self.builder.declare_var(view_ir(ty)));
        self.variables[index] = Some(views);
        views
    }

    /// The register's value viewed as `ty`.
    fn read(&mut self, register: u16, ty: crate::WasmType) -> ir::Value {
        let variable = self.views(register)[view(ty)];
        let stored = self.builder.use_var(variable);
        if ty != crate::WasmType::I32 {
            return stored;
        }
        let value = self.builder.ins().ireduce(types::I32, stored);
        self.zero_extended.insert(value, stored);
        value
    }

    /// `value`, an i32, zero-extended to 64 bits: the i32 view it was read from, if any.
    fn zero_extend(&mut self, value: ir::Value) -> ir::Value {
        match self.zero_extended.get(&value) {
            Some(&wide) => wide,
            None => self.builder.ins().uextend(types::I64, value),
        }
    }

    /// The stored form of `value` in the register's view as `ty` (`view_ir`).
    fn view_value(&mut self, ty: crate::WasmType, value: ir::Value) -> ir::Value {
        match ty {
            crate::WasmType::I32 => self.zero_extend(value),
            _ => value,
        }
    }

    /// Define the register as `value` of type `ty`, in both views of its bits.
    fn write(&mut self, register: u16, ty: crate::WasmType, value: ir::Value) {
        let other = counterpart(ty);
        let converted = self
            .builder
            .ins()
            .bitcast(scalar_ir(other), MemFlagsData::new(), value);
        let stored = self.view_value(ty, value);
        let converted = self.view_value(other, converted);
        let variables = self.views(register);
        self.builder.def_var(variables[view(ty)], stored);
        self.builder.def_var(variables[view(other)], converted);
    }

    /// Checks for a termination request: a read of the poll page, which faults once one is
    /// made and resumes as the terminated status (`guard_fault`), else a test of the context's
    /// flag that branches to `termination_block`.
    fn interrupt_safepoint(&mut self) {
        if let Some(poll) = self.poll {
            // The read faults once termination is requested, and the fault resumes as the
            // termination (`guard_fault`). Atomic, so it is neither merged nor dropped.
            let page = self.builder.ins().iconst(types::I64, poll);
            self.builder
                .ins()
                .atomic_load(types::I8, MemFlagsData::trusted(), page);
            return;
        }
        // The flag's address is fixed for the call; only the flag changes.
        let pointer = self
            .builder
            .ins()
            .load(types::I64, invariant(), self.context, CONTEXT_INTERRUPT);
        let requested = self
            .builder
            .ins()
            // Atomic, so no check reuses an earlier one's load: Cranelift's alias analysis
            // would merge plain loads of the flag across the loop.
            .atomic_load(types::I8, MemFlagsData::trusted(), pointer);
        let stop = self.builder.ins().icmp_imm_u(IntCC::NotEqual, requested, 0);
        let termination = match self.termination_block {
            Some(block) => block,
            None => {
                let block = self.builder.create_block();
                self.termination_block = Some(block);
                block
            }
        };
        let next = self.builder.create_block();
        self.builder.ins().brif(stop, termination, &[], next, &[]);
        self.builder.switch_to_block(next);
        self.builder.seal_block(next);
    }
}

struct Emitter<'a, 'b, H: Host> {
    guarded: bool,
    /// When enabled, branch-only i32 comparisons remain Cranelift booleans
    /// instead of being extended to a Wasm i32 and tested again.
    direct_compare_branches: bool,
    /// Cache same-artifact Wasm body pointers at indirect-call sites.
    reference_body_cache: bool,
    /// Emit a guarded direct leaf for the canonical Emscripten memcpy import.
    emscripten_memcpy_leaf: bool,
    /// Per `Region`, its alias region in this function.
    regions: [ir::AliasRegion; Region::COUNT],
    globals: &'a [u32],
    analysis: &'a Analysis<'a, H>,
    body: Body<'b>,
    plan: &'a Plan,
    blocks: Vec<Option<ir::Block>>,
    /// Dense ids for the compiled artifact's actual indirect call sites.
    reference_site_ids: &'a [(usize, usize)],
    /// Selects the diagnostics ABI at compile time; both ABIs carry site ids.
    diagnostics_enabled: bool,
    base: ir::Value,
    /// The memory's exact length for checked accesses and bulk operations.
    length: Option<ir::Value>,
    memory: MemoryArguments,
    trap_blocks: [Option<ir::Block>; TRAPS.len()],
    /// Per callee, its imported name in this function.
    callees: Vec<Option<ir::FuncRef>>,
    callout_signature: Option<ir::SigRef>,
    plans: &'a [Option<Plan>],
    call_conv: cranelift_codegen::isa::CallConv,
    instructions: &'a [WideInstruction],
    affine_loop: Option<AffinePairLoop>,
    affine_attempt: Option<Variable>,
    /// Registers some branch table dispatches on.
    selectors: Vec<u16>,
    /// Selectors known to hold an i32 constant since the current block began.
    known: Vec<(u16, i32)>,
    /// Threaded copies by (target, selector, value); `None` where none fits.
    threads: Vec<((usize, u16, i32), Option<ir::Block>)>,
    /// Copies to emit once the function body is done.
    pending: Vec<(ir::Block, Thread)>,
    /// While a copy emits a branch, the block standing in for one target.
    redirect: Option<(usize, ir::Block)>,
    /// Whether jumps may enter threaded copies (not from within one).
    threading: bool,
    /// The innermost loops to unroll, by latch (`unrollable_loops`).
    unrolled: Vec<Option<UnrolledLoop>>,
    /// While a latch of an unrolled loop is emitted: its header and the copy its back edge
    /// enters instead, or `None` for the last copy, whose back edge returns to the header and
    /// checks for an interrupt.
    backedge: Option<(usize, Option<ir::Block>)>,
    /// Copies of an unrolled body to emit right after its latch: the block, the loop, and the
    /// copy its back edge enters.
    pending_copies: Vec<(ir::Block, UnrolledLoop, usize, Option<ir::Block>)>,
}

impl<H: Host> Emitter<'_, '_, H> {
    /// The flags of an access to `region`.
    fn flags(&self, region: Region) -> MemFlagsData {
        MemFlagsData::trusted()
            .with_endianness(ir::Endianness::Little)
            .with_alias_region(Some(self.regions[region as usize]))
    }

    fn read(&mut self, register: u16, kind: Kind) -> ir::Value {
        self.read_typed(register, kind.integer())
    }

    fn write(&mut self, register: u16, kind: Kind, value: ir::Value) {
        self.write_typed(register, kind.integer(), value);
    }

    /// The register's value viewed as `ty`.
    fn read_typed(&mut self, register: u16, ty: crate::WasmType) -> ir::Value {
        self.body.read(register, ty)
    }

    fn zero_extend(&mut self, value: ir::Value) -> ir::Value {
        self.body.zero_extend(value)
    }

    /// Define the register as `value` of type `ty` (`Body::write`), which ends what a branch
    /// table selector was known to hold.
    fn write_typed(&mut self, register: u16, ty: crate::WasmType, value: ir::Value) {
        self.known.retain(|(known, _)| *known != register);
        self.body.write(register, ty, value);
    }

    /// Define the register as the constant `bits` of width `kind`.
    fn write_constant(&mut self, register: u16, kind: Kind, bits: i64) {
        self.known.retain(|(known, _)| *known != register);
        if kind == Kind::I32 && self.selectors.contains(&register) {
            self.known.push((register, bits as i32));
        }
        let (integer, float) = match kind {
            // The i32 view holds the constant zero-extended (`view_ir`).
            Kind::I32 => (
                self.body.builder
                    .ins()
                    .iconst(types::I64, i64::from(bits as u32)),
                self.body.builder.ins().f32const(Ieee32::with_bits(bits as u32)),
            ),
            Kind::I64 => (
                self.body.builder.ins().iconst(types::I64, bits),
                self.body.builder.ins().f64const(Ieee64::with_bits(bits as u64)),
            ),
        };
        let float_type = counterpart(kind.integer());
        let variables = self.body.views(register);
        self.body.builder
            .def_var(variables[view(kind.integer())], integer);
        self.body.builder.def_var(variables[view(float_type)], float);
    }

    /// The kind of the `b` register of instruction `pc`.
    fn operand_kind(&self, pc: usize) -> Kind {
        match self.plan.operand(pc) {
            Slot::Value(kind) => kind,
            _ => Kind::I32,
        }
    }

    fn block(&mut self, pc: usize) -> ir::Block {
        *self.blocks[pc].get_or_insert_with(|| self.body.builder.create_block())
    }

    fn trap_block(&mut self, trap: crate::WasmTrap) -> ir::Block {
        let index = (trap_status(trap) - 1) as usize;
        if let Some(block) = self.trap_blocks[index] {
            return block;
        }
        let block = self.body.builder.create_block();
        self.body.builder.set_cold_block(block);
        self.trap_blocks[index] = Some(block);
        block
    }

    /// Branch to the trap when `condition` is nonzero; continue otherwise.
    fn trap_if(&mut self, condition: ir::Value, trap: crate::WasmTrap) {
        let trap = self.trap_block(trap);
        let next = self.body.builder.create_block();
        self.body.builder.ins().brif(condition, trap, &[], next, &[]);
        self.body.builder.switch_to_block(next);
        self.body.builder.seal_block(next);
    }

    fn interrupt_safepoint(&mut self) {
        self.body.interrupt_safepoint();
    }

    /// Try a full counted-loop SIMD replacement once on entry. If the memory
    /// is too short, set the attempt marker and fall through to the original
    /// scalar bytecodes; their backedge then bypasses this guard.
    fn affine_pair_gate(&mut self) {
        let candidate = self.affine_loop.expect("gate only for an affine loop");
        let attempt = self
            .affine_attempt
            .expect("affine loop has an attempt variable");
        let scalar = self.body.builder.create_block();
        let first_attempt = self.body.builder.create_block();
        let attempted = self.body.builder.use_var(attempt);
        let already_attempted = self.body.builder.ins().icmp_imm_u(IntCC::NotEqual, attempted, 0);
        self.body.builder
            .ins()
            .brif(already_attempted, scalar, &[], first_attempt, &[]);

        self.body.builder.switch_to_block(first_attempt);
        let one = self.body.builder.ins().iconst(types::I32, 1);
        self.body.builder.def_var(attempt, one);
        let memory_length = self.body.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            self.body.context,
            CONTEXT_MEMORY_LENGTH,
        );
        let minimum_length = self
            .body.builder
            .ins()
            .iconst(types::I64, candidate.max_end as i64);
        let in_bounds = self.body.builder.ins().icmp(
            IntCC::UnsignedGreaterThanOrEqual,
            memory_length,
            minimum_length,
        );
        let vector = self.body.builder.create_block();
        self.body.builder.ins().brif(in_bounds, vector, &[], scalar, &[]);

        self.body.builder.switch_to_block(vector);
        self.affine_pair_vector_loop(candidate);
        self.body.builder.switch_to_block(scalar);
    }

    /// Emit the guarded vector body. Its induction range is proven to fit
    /// memory32 without wrap, the input and output ranges are disjoint, and
    /// it polls after each original pair of writes.
    fn affine_pair_vector_loop(&mut self, candidate: AffinePairLoop) {
        let loop_block = self.body.builder.create_block();
        let index = self.body.builder.append_block_param(loop_block, types::I32);
        let initial = self.body.builder.ins().iconst(types::I32, 0);
        self.body.builder
            .ins()
            .jump(loop_block, &[ir::BlockArg::Value(initial)]);
        self.body.builder.switch_to_block(loop_block);

        let scaled = self.body.builder.ins().ishl_imm_u(index, 2);
        let scaled = self.body.builder.ins().uextend(types::I64, scaled);
        let address = |this: &mut Self, offset: u32| {
            let address = this.body.builder.ins().iadd(this.base, scaled);
            this.body.builder.ins().iadd_imm_s(address, i64::from(offset))
        };
        let left_address = address(self, candidate.left);
        let right_address = address(self, candidate.right);
        let destination_address = address(self, candidate.destination);
        let flags = self.flags(Region::LinearMemory);
        let left = self
            .body.builder
            .ins()
            .load(types::I32X4, flags, left_address, 0);
        let right = self
            .body.builder
            .ins()
            .load(types::I32X4, flags, right_address, 0);
        let sum = self.body.builder.ins().iadd(left, right);
        let first_low = self.body.builder.ins().extractlane(sum, 0);
        let first_high = self.body.builder.ins().extractlane(sum, 1);
        let first_pair = self.body.builder.ins().iconcat(first_low, first_high);
        self.body.builder
            .ins()
            .store(flags, first_pair, destination_address, 0);
        self.interrupt_safepoint();

        let second_low = self.body.builder.ins().extractlane(sum, 2);
        let second_high = self.body.builder.ins().extractlane(sum, 3);
        let second_pair = self.body.builder.ins().iconcat(second_low, second_high);
        self.body.builder
            .ins()
            .store(flags, second_pair, destination_address, 8);
        self.interrupt_safepoint();

        let next_index = self.body.builder.ins().iadd_imm_s(index, 4);
        let more =
            self.body.builder
                .ins()
                .icmp_imm_u(IntCC::NotEqual, next_index, i64::from(candidate.bound));
        let backedge = self.body.builder.create_block();
        let done = self.body.builder.create_block();
        self.body.builder.ins().brif(more, backedge, &[], done, &[]);
        self.body.builder.switch_to_block(backedge);
        self.interrupt_safepoint();
        self.body.builder
            .ins()
            .jump(loop_block, &[ir::BlockArg::Value(next_index)]);
        self.body.builder.switch_to_block(done);
        let exit = self.edge(candidate.exit);
        self.body.builder.ins().jump(exit, &[]);
    }
    fn finish_traps(&mut self) {
        if let Some(block) = self.body.termination_block {
            self.body.builder.switch_to_block(block);
            let status = self.body.builder.ins().iconst(types::I64, TERMINATED_STATUS);
            let mut values = vec![status];
            if let Some(ty) = self.plan.result {
                let kind = Kind::of(ty).expect("planned");
                values.push(self.body.builder.ins().iconst(kind.ir(), 0));
            }
            self.body.builder.ins().return_(&values);
        }
        for (index, block) in self.trap_blocks.into_iter().enumerate() {
            let Some(block) = block else { continue };
            self.body.builder.switch_to_block(block);
            let status = self.body.builder.ins().iconst(types::I64, index as i64 + 1);
            let mut values = vec![status];
            if let Some(ty) = self.plan.result {
                let kind = Kind::of(ty).expect("planned");
                values.push(self.body.builder.ins().iconst(kind.ir(), 0));
            }
            self.body.builder.ins().return_(&values);
        }
    }

    /// An integer operator at the operands' width.
    fn integer_binary(
        &mut self,
        operator: I64BinaryOperator,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        use I64BinaryOperator as B;
        let ty = self.body.builder.func.dfg.value_type(left);
        let compare = |this: &mut Self, cc: IntCC| {
            let flag = this.body.builder.ins().icmp(cc, left, right);
            this.body.builder.ins().uextend(types::I32, flag)
        };
        match operator {
            B::Add => self.body.builder.ins().iadd(left, right),
            B::Subtract => self.body.builder.ins().isub(left, right),
            B::Multiply => self.body.builder.ins().imul(left, right),
            B::And => self.body.builder.ins().band(left, right),
            B::Or => self.body.builder.ins().bor(left, right),
            B::Xor => self.body.builder.ins().bxor(left, right),
            B::ShiftLeft => self.body.builder.ins().ishl(left, right),
            B::ShiftRightSigned => self.body.builder.ins().sshr(left, right),
            B::ShiftRightUnsigned => self.body.builder.ins().ushr(left, right),
            B::RotateLeft => self.body.builder.ins().rotl(left, right),
            B::RotateRight => self.body.builder.ins().rotr(left, right),
            B::MultiplyHighSigned => self.body.builder.ins().smulhi(left, right),
            B::MultiplyHighUnsigned => self.body.builder.ins().umulhi(left, right),
            B::Equal => compare(self, IntCC::Equal),
            B::NotEqual => compare(self, IntCC::NotEqual),
            B::LessSigned => compare(self, IntCC::SignedLessThan),
            B::LessUnsigned => compare(self, IntCC::UnsignedLessThan),
            B::GreaterSigned => compare(self, IntCC::SignedGreaterThan),
            B::GreaterUnsigned => compare(self, IntCC::UnsignedGreaterThan),
            B::LessEqualSigned => compare(self, IntCC::SignedLessThanOrEqual),
            B::LessEqualUnsigned => compare(self, IntCC::UnsignedLessThanOrEqual),
            B::GreaterEqualSigned => compare(self, IntCC::SignedGreaterThanOrEqual),
            B::GreaterEqualUnsigned => compare(self, IntCC::UnsignedGreaterThanOrEqual),
            B::DivideSigned | B::RemainderSigned | B::DivideUnsigned | B::RemainderUnsigned => {
                let zero = self.body.builder.ins().icmp_imm_s(IntCC::Equal, right, 0);
                self.trap_if(zero, crate::WasmTrap::IntegerDivideByZero);
                match operator {
                    B::DivideUnsigned => self.body.builder.ins().udiv(left, right),
                    B::RemainderUnsigned => self.body.builder.ins().urem(left, right),
                    B::DivideSigned => {
                        let minimum = if ty == types::I32 {
                            i64::from(i32::MIN)
                        } else {
                            i64::MIN
                        };
                        let minimum = self.body.builder.ins().icmp_imm_s(IntCC::Equal, left, minimum);
                        let negative_one = self.body.builder.ins().icmp_imm_s(IntCC::Equal, right, -1);
                        let overflow = self.body.builder.ins().band(minimum, negative_one);
                        self.trap_if(overflow, crate::WasmTrap::IntegerOverflow);
                        self.body.builder.ins().sdiv(left, right)
                    }
                    _ => {
                        // `MIN % -1` is 0; the machine division would fault.
                        let negative_one = self.body.builder.ins().icmp_imm_s(IntCC::Equal, right, -1);
                        let one = self.body.builder.ins().iconst(ty, 1);
                        let divisor = self.body.builder.ins().select(negative_one, one, right);
                        self.body.builder.ins().srem(left, divisor)
                    }
                }
            }
        }
    }

    /// The boolean form of an i32 comparison used only by a conditional
    /// jump. Keeping this `icmp` result as a Cranelift flag lets the backend
    /// branch on the comparison directly, without materializing Wasm's 0/1
    /// value first.
    fn i32_comparison_condition(
        &mut self,
        operator: I32BinaryOperator,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        use I32BinaryOperator as B;

        let condition = match operator {
            B::Equal => IntCC::Equal,
            B::NotEqual => IntCC::NotEqual,
            B::LessSigned => IntCC::SignedLessThan,
            B::LessUnsigned => IntCC::UnsignedLessThan,
            B::GreaterSigned => IntCC::SignedGreaterThan,
            B::GreaterUnsigned => IntCC::UnsignedGreaterThan,
            B::LessEqualSigned => IntCC::SignedLessThanOrEqual,
            B::LessEqualUnsigned => IntCC::UnsignedLessThanOrEqual,
            B::GreaterEqualSigned => IntCC::SignedGreaterThanOrEqual,
            B::GreaterEqualUnsigned => IntCC::UnsignedGreaterThanOrEqual,
            _ => unreachable!("only comparison opcodes branch on an i32 comparison"),
        };
        self.body.builder.ins().icmp(condition, left, right)
    }

    /// An integer unary operator at the operand's width.
    fn integer_unary(&mut self, operator: I64UnaryOperator, value: ir::Value) -> ir::Value {
        use I64UnaryOperator as U;
        let ty = self.body.builder.func.dfg.value_type(value);
        let mut extend = |narrow_type| {
            let narrow = self.body.builder.ins().ireduce(narrow_type, value);
            self.body.builder.ins().sextend(ty, narrow)
        };
        match operator {
            U::EqualZero => {
                let flag = self.body.builder.ins().icmp_imm_s(IntCC::Equal, value, 0);
                self.body.builder.ins().uextend(types::I32, flag)
            }
            U::LeadingZeros => self.body.builder.ins().clz(value),
            U::TrailingZeros => self.body.builder.ins().ctz(value),
            U::PopulationCount => self.body.builder.ins().popcnt(value),
            U::ExtendSigned8 => extend(types::I8),
            U::ExtendSigned16 => extend(types::I16),
            U::ExtendSigned32 => extend(types::I32),
        }
    }

    /// The canonical NaN of float type `ty`, as `crate::wasm::float` produces.
    fn canonical_nan(&mut self, ty: ir::Type) -> ir::Value {
        use crate::wasm::numeric::canonical_nan_bits;
        if ty == types::F32 {
            let bits = canonical_nan_bits(f32::MANTISSA_DIGITS, u64::from(f32::INFINITY.to_bits()));
            self.body.builder.ins().f32const(Ieee32::with_bits(bits as u32))
        } else {
            let bits = canonical_nan_bits(f64::MANTISSA_DIGITS, f64::INFINITY.to_bits());
            self.body.builder.ins().f64const(Ieee64::with_bits(bits))
        }
    }

    /// A float operator at the operands' width.
    fn float_binary(
        &mut self,
        operator: F64BinaryOperator,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        use F64BinaryOperator as F;
        let compare = |this: &mut Self, cc: FloatCC| {
            let flag = this.body.builder.ins().fcmp(cc, left, right);
            this.body.builder.ins().uextend(types::I32, flag)
        };
        match operator {
            F::Add => self.body.builder.ins().fadd(left, right),
            F::Subtract => self.body.builder.ins().fsub(left, right),
            F::Multiply => self.body.builder.ins().fmul(left, right),
            F::Divide => self.body.builder.ins().fdiv(left, right),
            F::Minimum | F::Maximum => {
                // Signed zeros order as Wasm requires; a NaN operand gives
                // the canonical NaN.
                let extremum = if operator == F::Minimum {
                    self.body.builder.ins().fmin(left, right)
                } else {
                    self.body.builder.ins().fmax(left, right)
                };
                let unordered = self.body.builder.ins().fcmp(FloatCC::Unordered, left, right);
                let ty = self.body.builder.func.dfg.value_type(left);
                let nan = self.canonical_nan(ty);
                self.body.builder.ins().select(unordered, nan, extremum)
            }
            F::CopySign => self.body.builder.ins().fcopysign(left, right),
            F::Equal => compare(self, FloatCC::Equal),
            F::NotEqual => compare(self, FloatCC::NotEqual),
            F::Less => compare(self, FloatCC::LessThan),
            F::Greater => compare(self, FloatCC::GreaterThan),
            F::LessEqual => compare(self, FloatCC::LessThanOrEqual),
            F::GreaterEqual => compare(self, FloatCC::GreaterThanOrEqual),
        }
    }

    /// A float unary operator at the operand's width. The machine rounding
    /// and square root quiet a signaling NaN, as the operators do.
    fn float_unary(&mut self, operator: F64UnaryOperator, value: ir::Value) -> ir::Value {
        use F64UnaryOperator as U;
        match operator {
            U::Absolute => self.body.builder.ins().fabs(value),
            U::Negate => self.body.builder.ins().fneg(value),
            U::Ceiling => self.body.builder.ins().ceil(value),
            U::Floor => self.body.builder.ins().floor(value),
            U::Truncate => self.body.builder.ins().trunc(value),
            U::Nearest => self.body.builder.ins().nearest(value),
            U::SquareRoot => self.body.builder.ins().sqrt(value),
        }
    }

    /// A scalar conversion of `value`, already viewed as the source type.
    fn convert(&mut self, operator: ScalarConversionOperator, value: ir::Value) -> ir::Value {
        use ScalarConversionOperator as C;
        let result = scalar_ir(operator.result_type());
        let source = operator.source_type();
        match operator {
            C::WrapI64 => self.body.builder.ins().ireduce(result, value),
            C::ExtendI32Signed => self.body.builder.ins().sextend(result, value),
            C::ExtendI32Unsigned => self.body.builder.ins().uextend(result, value),
            C::TruncateF32SignedI32
            | C::TruncateF64SignedI32
            | C::TruncateF32SignedI64
            | C::TruncateF64SignedI64 => self.truncate(value, source, result, true),
            C::TruncateF32UnsignedI32
            | C::TruncateF64UnsignedI32
            | C::TruncateF32UnsignedI64
            | C::TruncateF64UnsignedI64 => self.truncate(value, source, result, false),
            C::SaturateF32SignedI32
            | C::SaturateF64SignedI32
            | C::SaturateF32SignedI64
            | C::SaturateF64SignedI64 => self.body.builder.ins().fcvt_to_sint_sat(result, value),
            C::SaturateF32UnsignedI32
            | C::SaturateF64UnsignedI32
            | C::SaturateF32UnsignedI64
            | C::SaturateF64UnsignedI64 => self.body.builder.ins().fcvt_to_uint_sat(result, value),
            C::ConvertI32SignedF32
            | C::ConvertI64SignedF32
            | C::ConvertI32SignedF64
            | C::ConvertI64SignedF64 => self.body.builder.ins().fcvt_from_sint(result, value),
            C::ConvertI32UnsignedF32
            | C::ConvertI64UnsignedF32
            | C::ConvertI32UnsignedF64
            | C::ConvertI64UnsignedF64 => self.body.builder.ins().fcvt_from_uint(result, value),
            C::PromoteF32 => self.body.builder.ins().fpromote(result, value),
            C::DemoteF64 => self.body.builder.ins().fdemote(result, value),
            C::ReinterpretF32I32
            | C::ReinterpretF64I64
            | C::ReinterpretI32F32
            | C::ReinterpretI64F64 => {
                self.body.builder
                    .ins()
                    .bitcast(result, MemFlagsData::new(), value)
            }
        }
    }

    /// A trapping truncation: NaN and out-of-range inputs trap, in the
    /// order and bounds of `crate::wasm::conversion`; the machine conversion
    /// then sees only representable values.
    fn truncate(
        &mut self,
        value: ir::Value,
        source: crate::WasmType,
        result: ir::Type,
        signed: bool,
    ) -> ir::Value {
        let wide = if source == crate::WasmType::F32 {
            self.body.builder.ins().fpromote(types::F64, value)
        } else {
            value
        };
        let nan = self.body.builder.ins().fcmp(FloatCC::Unordered, wide, wide);
        self.trap_if(nan, crate::WasmTrap::InvalidConversionToInteger);
        let truncated = self.body.builder.ins().trunc(wide);
        let (lower, limit) = crate::wasm::conversion::truncation_bounds(result.bits(), signed);
        let lower = self.body.builder.ins().f64const(lower);
        let limit = self.body.builder.ins().f64const(limit);
        let below = self.body.builder.ins().fcmp(FloatCC::LessThan, truncated, lower);
        let above = self
            .body.builder
            .ins()
            .fcmp(FloatCC::GreaterThanOrEqual, truncated, limit);
        let overflow = self.body.builder.ins().bor(below, above);
        self.trap_if(overflow, crate::WasmTrap::IntegerOverflow);
        if signed {
            self.body.builder.ins().fcvt_to_sint_sat(result, truncated)
        } else {
            self.body.builder.ins().fcvt_to_uint_sat(result, truncated)
        }
    }

    /// The host address of a checked `width`-byte access at `address + offset`.
    fn address(&mut self, address: ir::Value, offset: u32, width: usize) -> ir::Value {
        let address = self.zero_extend(address);
        if self.guarded {
            // Index plus offset plus width stays inside the reservation; past
            // the accessible prefix the access faults, and the fault handler
            // raises the out-of-bounds trap (see `guard_fault`).
            let host = self.body.builder.ins().iadd(self.base, address);
            return self.body.builder.ins().iadd_imm_s(host, i64::from(offset));
        }
        let end = self
            .body.builder
            .ins()
            .iadd_imm_s(address, i64::from(offset) + width as i64);
        let out_of_bounds = self.body.builder.ins().icmp(
            IntCC::UnsignedGreaterThan,
            end,
            self.length.expect("unguarded accesses receive the length"),
        );
        self.trap_if(out_of_bounds, crate::WasmTrap::OutOfBoundsMemory);
        let host = self.body.builder.ins().iadd(self.base, address);
        self.body.builder.ins().iadd_imm_s(host, i64::from(offset))
    }

    /// The address of the value of the global bound at `slot`.
    fn global_address(&mut self, slot: u32) -> ir::Value {
        let position = self
            .globals
            .iter()
            .position(|candidate| *candidate == slot)
            .expect("planned globals are collected");
        let table = self
            .body.builder
            .ins()
            .load(types::I64, invariant(), self.body.context, CONTEXT_GLOBALS);
        self.body.builder
            .ins()
            .load(types::I64, invariant(), table, (position * 8) as i32)
    }

    /// The binding the `b` register of instruction `pc` holds.
    fn binding_slot(&self, pc: usize) -> u32 {
        match self.plan.operand(pc) {
            Slot::Binding(slot) => slot,
            _ => unreachable!("planned global accesses read a binding"),
        }
    }

    /// A nonzero status from a call (a trap or a raised callout) unwinds
    /// this activation too.
    fn propagate(&mut self, status: ir::Value) {
        let unwound = self.body.builder.create_block();
        self.body.builder.set_cold_block(unwound);
        let next = self.body.builder.create_block();
        self.body.builder.ins().brif(status, unwound, &[], next, &[]);
        self.body.builder.switch_to_block(unwound);
        self.body.builder.seal_block(unwound);
        let mut values = vec![status];
        if let Some(ty) = self.plan.result {
            values.push(
                self.body.builder
                    .ins()
                    .iconst(Kind::of(ty).expect("planned").ir(), 0),
            );
        }
        self.body.builder.ins().return_(&values);
        self.body.builder.switch_to_block(next);
        self.body.builder.seal_block(next);
    }

    /// An imported call: arguments go to the VM in a stack array, through
    /// the context's callout.
    fn callout(&mut self, pc: usize, instruction: WideInstruction) {
        let Slot::Import(function) = self.plan.operand(pc) else {
            unreachable!("planned imported calls read an import reference");
        };
        let signature = self
            .analysis
            .import_signature(function)
            .expect("planned")
            .clone();
        self.callout_function(function, &signature, instruction);
    }

    /// A statically known Wasm callee that did not pass native planning still
    /// crosses the ordinary VM boundary. Its body can run interpreted (and
    /// can itself enter any compatible compiled function).
    fn callout_known_fallback(&mut self, function: u32, instruction: WideInstruction) {
        let signature = self
            .analysis
            .signature(function)
            .expect("known signature")
            .clone();
        // `CallKnown` stores the defined-function-local index, while the
        // ordinary VM callout and function-reference lookup use Wasm's
        // absolute function index (imports first).
        let absolute_function = self
            .analysis
            .vm
            .programs
            .wasm_signatures(self.analysis.program)
            .and_then(|signatures| {
                u32::try_from(signatures.imports().len())
                    .ok()?
                    .checked_add(function)
            })
            .expect("validated known function index");
        self.callout_function(absolute_function, &signature, instruction);
    }

    fn callout_function(
        &mut self,
        function: u32,
        signature: &crate::WasmSignature,
        instruction: WideInstruction,
    ) {
        if self.emscripten_memcpy_leaf && self.is_emscripten_memcpy_import(function, signature) {
            self.callout_emscripten_memcpy_leaf(function, instruction);
            return;
        }
        self.callout_function_generic(function, signature, instruction);
    }

    fn is_emscripten_memcpy_import(&self, function: u32, signature: &crate::WasmSignature) -> bool {
        let Some(signatures) = self
            .analysis
            .vm
            .programs
            .wasm_signatures(self.analysis.program)
        else {
            return false;
        };
        let Some(import) = usize::try_from(function)
            .ok()
            .and_then(|index| signatures.imports().get(index))
        else {
            return false;
        };
        import.name.name == "_emscripten_memcpy_js"
            && signature.params.as_slice()
                == [
                    crate::WasmType::I32,
                    crate::WasmType::I32,
                    crate::WasmType::I32,
                ]
            && signature.results.is_empty()
    }

    fn callout_emscripten_memcpy_leaf(&mut self, function: u32, instruction: WideInstruction) {
        let fast = self.body.builder.create_block();
        let slow = self.body.builder.create_block();
        let success = self.body.builder.create_block();
        let done = self.body.builder.create_block();
        let cache = self.body.builder.ins().load(
            types::I64,
            invariant(),
            self.body.context,
            CONTEXT_EMSCRIPTEN_MEMCPY_CACHE,
        );
        let valid = self.body.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            cache,
            MEMCPY_CACHE_VALID,
        );
        let valid = self.body.builder.ins().icmp_imm_u(IntCC::Equal, valid, 1);
        let cached_function = self.body.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            cache,
            MEMCPY_CACHE_FUNCTION,
        );
        let matching_function =
            self.body.builder
                .ins()
                .icmp_imm_u(IntCC::Equal, cached_function, i64::from(function));
        let valid = self.body.builder.ins().band(valid, matching_function);
        self.body.builder.ins().brif(valid, fast, &[], slow, &[]);

        self.body.builder.switch_to_block(fast);
        let window = instruction.call_window();
        let args = (0..3)
            .map(|offset| self.read(window.base + offset, Kind::I32))
            .collect::<Vec<_>>();
        let target = self.body.builder.ins().load(
            types::I64,
            invariant(),
            self.body.context,
            CONTEXT_EMSCRIPTEN_MEMCPY_LEAF,
        );
        let mut leaf = Signature::new(self.call_conv);
        leaf.params.push(AbiParam::new(types::I64));
        leaf.params.push(AbiParam::new(types::I64));
        leaf.params
            .extend((0..3).map(|_| AbiParam::new(types::I32)));
        leaf.returns.push(AbiParam::new(types::I64));
        let leaf = self.body.builder.import_signature(leaf);
        let function_value = self.body.builder.ins().iconst(types::I64, i64::from(function));
        let call = self.body.builder.ins().call_indirect(
            leaf,
            target,
            &[self.body.context, function_value, args[0], args[1], args[2]],
        );
        let status = self.body.builder.inst_results(call)[0];
        let miss = self
            .body.builder
            .ins()
            .icmp_imm_u(IntCC::Equal, status, MEMCPY_LEAF_MISS_STATUS);
        self.body.builder.ins().brif(miss, slow, &[], success, &[]);

        self.body.builder.switch_to_block(success);
        self.propagate(status);
        self.write_constant(instruction.result_register(), Kind::I32, 0);
        self.body.builder.ins().jump(done, &[]);

        self.body.builder.switch_to_block(slow);
        self.callout_function_generic(
            function,
            &crate::WasmSignature {
                params: vec![crate::WasmType::I32; 3],
                results: Vec::new(),
            },
            instruction,
        );
        self.body.builder.ins().jump(done, &[]);
        self.body.builder.switch_to_block(done);
    }

    fn callout_function_generic(
        &mut self,
        function: u32,
        signature: &crate::WasmSignature,
        instruction: WideInstruction,
    ) {
        let word = types::I64.bytes();
        let signature = signature.clone();
        let words = signature.params.len() + 1;
        let slot = self.body.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            word * words as u32,
            word.trailing_zeros() as u8,
        ));
        let window = instruction.call_window();
        for (offset, ty) in signature.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            let value = self.read(window.base + offset as u16, kind);
            let value = match kind {
                Kind::I32 => self.body.builder.ins().uextend(types::I64, value),
                Kind::I64 => value,
            };
            let address =
                self.body.builder
                    .ins()
                    .stack_addr(types::I64, slot, (offset as u32 * word) as i32);
            self.body.builder
                .ins()
                .store(MemFlagsData::trusted(), value, address, 0);
        }
        let result_offset = (signature.params.len() as u32 * word) as i32;
        let args = self.body.builder.ins().stack_addr(types::I64, slot, 0);
        let out = self
            .body.builder
            .ins()
            .stack_addr(types::I64, slot, result_offset);
        let callout_signature = match self.callout_signature {
            Some(signature) => signature,
            None => {
                let mut callout = Signature::new(self.call_conv);
                for _ in 0..4 {
                    callout.params.push(AbiParam::new(types::I64));
                }
                callout.returns.push(AbiParam::new(types::I64));
                let callout = self.body.builder.import_signature(callout);
                self.callout_signature = Some(callout);
                callout
            }
        };
        let target =
            self.body.builder
                .ins()
                .load(types::I64, invariant(), self.body.context, CONTEXT_CALLOUT);
        let function = self.body.builder.ins().iconst(types::I64, i64::from(function));
        let call = self.body.builder.ins().call_indirect(
            callout_signature,
            target,
            &[self.body.context, function, args, out],
        );
        let status = self.body.builder.inst_results(call)[0];
        self.propagate(status);
        let destination = instruction.result_register();
        match signature.results.first() {
            Some(ty) => {
                let kind = Kind::of(*ty).expect("planned");
                let bits = self
                    .body.builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), out, 0);
                let value = match kind {
                    Kind::I32 => self.body.builder.ins().ireduce(types::I32, bits),
                    Kind::I64 => bits,
                };
                self.write(destination, kind, value);
            }
            None => self.write_constant(destination, Kind::I32, 0),
        }
    }

    /// Keep the table index in native code until the combined VM callout.
    fn indirect_target(&mut self, _pc: usize, instruction: WideInstruction) {
        let index = self.read(instruction.c(), Kind::I32);
        let index = self.body.builder.ins().uextend(types::I64, index);
        self.write(instruction.a(), Kind::I64, index);
    }

    /// Resolve and call a table reference through one ordinary VM boundary.
    fn call_reference(&mut self, pc: usize, instruction: WideInstruction) {
        let Slot::IndirectReference {
            signature: signature_index,
            table,
        } = self.plan.operand(pc)
        else {
            unreachable!("planned indirect calls retain their table binding");
        };
        let wasm_signature = self
            .analysis
            .type_signature(signature_index)
            .expect("planned")
            .clone();
        let word = types::I64.bytes();
        let words = wasm_signature.params.len() + 1;
        let slot = self.body.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            word * words as u32,
            word.trailing_zeros() as u8,
        ));
        let window = instruction.call_window();
        for (offset, ty) in wasm_signature.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            let value = self.read(window.base + offset as u16, kind);
            let value = match kind {
                Kind::I32 => self.body.builder.ins().uextend(types::I64, value),
                Kind::I64 => value,
            };
            let address =
                self.body.builder
                    .ins()
                    .stack_addr(types::I64, slot, (offset as u32 * word) as i32);
            self.body.builder
                .ins()
                .store(MemFlagsData::trusted(), value, address, 0);
        }
        let result_offset = (wasm_signature.params.len() as u32 * word) as i32;
        let args = self.body.builder.ins().stack_addr(types::I64, slot, 0);
        let out = self
            .body.builder
            .ins()
            .stack_addr(types::I64, slot, result_offset);
        let index = self.read(instruction.b(), Kind::I64);
        let table_slot = self.body.builder.ins().iconst(types::I64, i64::from(table));
        let signature_index = self
            .body.builder
            .ins()
            .iconst(types::I64, i64::from(signature_index));
        let site_id = self
            .reference_site_ids
            .binary_search_by_key(&pc, |(site_pc, _)| *site_pc)
            .ok()
            .map(|index| self.reference_site_ids[index].1)
            .expect("compiled indirect call has a cache site id");
        let fast_cache_base = self.body.builder.ins().load(
            types::I64,
            invariant(),
            self.body.context,
            CONTEXT_REFERENCE_FAST_CACHES,
        );
        let fast_cache = self.body.builder.ins().iadd_imm_s(
            fast_cache_base,
            (site_id * std::mem::size_of::<FastReferenceCache>()) as i64,
        );
        let cached_epoch = self.body.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            fast_cache,
            std::mem::offset_of!(FastReferenceCache, epoch) as i32,
        );
        let cache_initialized = self
            .body.builder
            .ins()
            .icmp_imm_u(IntCC::NotEqual, cached_epoch, 0);
        let check_cache = self.body.builder.create_block();
        let check_entry = self.body.builder.create_block();
        let fast_call = self.body.builder.create_block();
        let slow_call = self.body.builder.create_block();
        let done = self.body.builder.create_block();
        let status_slot = self.body.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            types::I64.bytes() as u32,
            types::I64.bytes().trailing_zeros() as u8,
        ));
        self.body.builder
            .ins()
            .brif(cache_initialized, check_cache, &[], slow_call, &[]);

        // Table writes and collection advance the VM-local epoch. This makes
        // artifact-local certificates stale without invalidation walks through
        // recursive native frames.
        self.body.builder.switch_to_block(check_cache);
        let epoch_pointer = self.body.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            self.body.context,
            CONTEXT_REFERENCE_EPOCH,
        );
        let current_epoch =
            self.body.builder
                .ins()
                .load(types::I64, MemFlagsData::trusted(), epoch_pointer, 0);
        let epoch_matches = self
            .body.builder
            .ins()
            .icmp(IntCC::Equal, current_epoch, cached_epoch);
        let current_env =
            self.body.builder
                .ins()
                .load(types::I64, invariant(), self.body.context, CONTEXT_ENV);
        let cached_env = self.body.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            fast_cache,
            std::mem::offset_of!(FastReferenceCache, env) as i32,
        );
        let env_matches = self
            .body.builder
            .ins()
            .icmp(IntCC::Equal, current_env, cached_env);
        let cached_index = self.body.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            fast_cache,
            std::mem::offset_of!(FastReferenceCache, index) as i32,
        );
        let index_matches = self.body.builder.ins().icmp(IntCC::Equal, index, cached_index);
        let epoch_and_env_match = self.body.builder.ins().band(epoch_matches, env_matches);
        let cache_matches = self.body.builder.ins().band(epoch_and_env_match, index_matches);
        self.body.builder
            .ins()
            .brif(cache_matches, check_entry, &[], slow_call, &[]);
        self.body.builder.seal_block(check_cache);

        // A valid cache is populated only from a checked same-artifact target,
        // but keep the index and non-null tests here as a cheap corruption
        // guard before indexing the artifact's entry table.
        self.body.builder.switch_to_block(check_entry);
        let cached_function = self.body.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            fast_cache,
            std::mem::offset_of!(FastReferenceCache, function) as i32,
        );
        let function_in_range = self.body.builder.ins().icmp_imm_u(
            IntCC::UnsignedLessThan,
            cached_function,
            self.plans.len() as i64,
        );
        let body = if self.reference_body_cache {
            let load_body = self.body.builder.create_block();
            self.body.builder
                .ins()
                .brif(function_in_range, load_body, &[], slow_call, &[]);
            self.body.builder.seal_block(check_entry);
            self.body.builder.switch_to_block(load_body);
            let body = self.body.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                fast_cache,
                std::mem::offset_of!(FastReferenceCache, body) as i32,
            );
            let body_present = self.body.builder.ins().icmp_imm_u(IntCC::NotEqual, body, 0);
            self.body.builder
                .ins()
                .brif(body_present, fast_call, &[], slow_call, &[]);
            self.body.builder.seal_block(load_body);
            body
        } else {
            let lookup_entry = self.body.builder.create_block();
            self.body.builder
                .ins()
                .brif(function_in_range, lookup_entry, &[], slow_call, &[]);
            self.body.builder.seal_block(check_entry);
            self.body.builder.switch_to_block(lookup_entry);
            let artifact_table =
                self.body.builder
                    .ins()
                    .load(types::I64, invariant(), self.body.context, CONTEXT_TABLE);
            let entry_offset = self.body.builder.ins().ishl_imm_u(cached_function, 3);
            let entry_address = self.body.builder.ins().iadd(artifact_table, entry_offset);
            let entry =
                self.body.builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), entry_address, 0);
            let entry_present = self.body.builder.ins().icmp_imm_u(IntCC::NotEqual, entry, 0);
            self.body.builder
                .ins()
                .brif(entry_present, fast_call, &[], slow_call, &[]);
            self.body.builder.seal_block(lookup_entry);
            entry
        };

        self.body.builder.switch_to_block(fast_call);
        let mut direct_args = vec![self.body.context];
        if self.memory.base {
            direct_args.push(self.base);
        }
        direct_args.extend(self.length);
        for (offset, ty) in wasm_signature.params.iter().enumerate() {
            let raw = self.body.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                args,
                (offset as u32 * word) as i32,
            );
            direct_args.push(match Kind::of(*ty).expect("planned") {
                Kind::I32 => self.body.builder.ins().ireduce(types::I32, raw),
                Kind::I64 => raw,
            });
        }
        let entry_signature = self.body.builder.import_signature(internal_signature(
            self.memory,
            &wasm_signature.params,
            wasm_signature.results.first().copied(),
            self.call_conv,
        ));
        let call = self
            .body.builder
            .ins()
            .call_indirect(entry_signature, body, &direct_args);
        let fast_results = self.body.builder.inst_results(call).to_vec();
        let fast_status = fast_results[0];
        if let Some(result) = wasm_signature.results.first() {
            let value = match Kind::of(*result).expect("planned") {
                Kind::I32 => self.body.builder.ins().uextend(types::I64, fast_results[1]),
                Kind::I64 => fast_results[1],
            };
            self.body.builder
                .ins()
                .store(MemFlagsData::trusted(), value, out, 0);
        }
        let status_address = self.body.builder.ins().stack_addr(types::I64, status_slot, 0);
        self.body.builder
            .ins()
            .store(MemFlagsData::trusted(), fast_status, status_address, 0);
        self.body.builder.ins().jump(done, &[]);
        self.body.builder.seal_block(fast_call);

        self.body.builder.switch_to_block(slow_call);
        self.body.builder.set_cold_block(slow_call);
        let target_offset = if self.diagnostics_enabled {
            CONTEXT_CALL_REFERENCE_DIAGNOSTIC
        } else {
            CONTEXT_CALL_REFERENCE
        };
        let target = self
            .body.builder
            .ins()
            .load(types::I64, invariant(), self.body.context, target_offset);
        let mut call_signature = Signature::new(self.call_conv);
        for _ in 0..8 {
            call_signature.params.push(AbiParam::new(types::I64));
        }
        call_signature.returns.push(AbiParam::new(types::I64));
        let call_signature = self.body.builder.import_signature(call_signature);
        let site_id = self.body.builder.ins().iconst(types::I64, site_id as i64);
        let call = self.body.builder.ins().call_indirect(
            call_signature,
            target,
            &[
                self.body.context,
                table_slot,
                index,
                signature_index,
                site_id,
                args,
                out,
                fast_cache,
            ],
        );
        let slow_status = self.body.builder.inst_results(call)[0];
        let status_address = self.body.builder.ins().stack_addr(types::I64, status_slot, 0);
        self.body.builder
            .ins()
            .store(MemFlagsData::trusted(), slow_status, status_address, 0);
        self.body.builder.ins().jump(done, &[]);
        self.body.builder.seal_block(slow_call);

        self.body.builder.switch_to_block(done);
        self.body.builder.seal_block(done);
        let status_address = self.body.builder.ins().stack_addr(types::I64, status_slot, 0);
        let status =
            self.body.builder
                .ins()
                .load(types::I64, MemFlagsData::trusted(), status_address, 0);
        self.propagate(status);
        let destination = instruction.result_register();
        match wasm_signature.results.first() {
            Some(ty) => {
                let kind = Kind::of(*ty).expect("planned");
                let bits = self
                    .body.builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), out, 0);
                let value = match kind {
                    Kind::I32 => self.body.builder.ins().ireduce(types::I32, bits),
                    Kind::I64 => bits,
                };
                self.write(destination, kind, value);
            }
            None => self.write_constant(destination, Kind::I32, 0),
        }
    }

    /// Emit a jump, polling only for the entry transfer or a loop backedge.
    /// Every bytecode control-flow cycle has a backedge, so acyclic edges do
    /// not need an atomic load to preserve cancellation responsiveness.
    fn jump(&mut self, target: usize, interrupt_check: bool) {
        if interrupt_check {
            self.interrupt_safepoint();
        }
        let mut block = self.edge(target);
        if self.threading
            && Some(block) == self.blocks[target]
            && let Some(thread) = dispatch_path(self.instructions, self.plan, target)
        {
            block = self.body.builder.create_block();
            self.pending.push((block, thread));
        }
        self.body.builder.ins().jump(block, &[]);
    }

    /// A conditional transfer from `pc`. It checks for an interrupt when
    /// either edge leads to `pc` or before: a loop repeats through one of
    /// them (`JumpI32Zero` passes its targets swapped, so either may be the
    /// one behind). The check sits before the branch so the backedge stays
    /// direct: routing it through a block of its own measured 10% more
    /// CoreMark instructions. A loop latch (a taken target behind the
    /// fallthrough) leaves the loop through a block of its own, so values
    /// only the exit passes (merge arguments) are computed after the loop,
    /// not on every iteration.
    fn branch(&mut self, pc: usize, condition: ir::Value, target: usize, fallthrough: usize) {
        if self.polls(pc, target) || self.polls(pc, fallthrough) {
            self.interrupt_safepoint();
        }
        let taken = self.edge(target);
        let next = self.edge(fallthrough);
        if target >= fallthrough {
            self.body.builder.ins().brif(condition, taken, &[], next, &[]);
            return;
        }
        let exit = self.body.builder.create_block();
        self.body.builder.ins().brif(condition, taken, &[], exit, &[]);
        self.body.builder.switch_to_block(exit);
        self.body.builder.ins().jump(next, &[]);
    }

    /// Whether an edge from `pc` to `target` checks for an interrupt: a back edge does, except
    /// one into a copy of an unrolled body, whose last copy's back edge checks for all.
    fn polls(&self, pc: usize, target: usize) -> bool {
        target <= pc && !matches!(self.backedge, Some((header, Some(_))) if header == target)
    }

    /// The block a jump from here to `target` enters: a threaded copy when
    /// a known selector decides a branch table ahead, else the target's.
    fn edge(&mut self, target: usize) -> ir::Block {
        if let Some((redirected, block)) = self.redirect
            && redirected == target
        {
            return block;
        }
        if let Some((header, Some(copy))) = self.backedge
            && header == target
        {
            return copy;
        }
        if self.threading {
            for (selector, value) in self.known.clone() {
                if let Some(block) = self.thread(target, selector, value) {
                    return block;
                }
            }
        }
        self.block(target)
    }

    fn thread(&mut self, target: usize, selector: u16, value: i32) -> Option<ir::Block> {
        let key = (target, selector, value);
        if let Some((_, block)) = self.threads.iter().find(|(known, _)| *known == key) {
            return *block;
        }
        let thread = thread_path(self.instructions, self.plan, target, selector, value);
        let block = thread.map(|thread| {
            let block = self.body.builder.create_block();
            self.pending.push((block, thread));
            block
        });
        self.threads.push((key, block));
        block
    }

    /// Emit the threaded copy `thread` into `block`.
    /// Emit the latch at `pc`; at an unrolled loop's latch the back edge enters the first copy
    /// of the body, queued with the rest (`pending_copies`).
    fn latch(&mut self, function: &Function, pc: usize, instruction: WideInstruction) -> bool {
        let Some(unrolled) = self.unrolled[pc] else {
            return self.instruction(function, pc, instruction);
        };
        let copies = (1..unrolled.factor)
            .map(|_| self.body.builder.create_block())
            .collect::<Vec<_>>();
        for (index, &copy) in copies.iter().enumerate() {
            let next = copies.get(index + 1).copied();
            self.pending_copies.push((copy, unrolled, pc, next));
        }
        self.backedge = Some((unrolled.header, copies.first().copied()));
        let open = self.instruction(function, pc, instruction);
        self.backedge = None;
        open
    }

    /// Emit a copy of the body of the unrolled loop with latch `latch` into `block`: branches
    /// out of the loop leave through the original blocks, and the back edge enters `next`, or
    /// the header from the last copy.
    fn emit_unrolled_copy(
        &mut self,
        function: &Function,
        block: ir::Block,
        unrolled: UnrolledLoop,
        latch: usize,
        next: Option<ir::Block>,
    ) {
        self.body.builder.switch_to_block(block);
        self.known.clear();
        for pc in unrolled.header..latch {
            let instruction = self.instructions[pc];
            if instruction.op().control_flow_layout() == ControlFlowLayout::ConditionalJump {
                let continuation = self.body.builder.create_block();
                self.redirect = Some((pc + 1, continuation));
                self.instruction(function, pc, instruction);
                self.redirect = None;
                self.body.builder.switch_to_block(continuation);
                self.known.clear();
            } else {
                self.instruction(function, pc, instruction);
            }
        }
        self.backedge = Some((unrolled.header, next));
        self.instruction(function, latch, self.instructions[latch]);
        self.backedge = None;
    }

    fn emit_thread(&mut self, function: &Function, block: ir::Block, thread: &Thread) {
        self.body.builder.switch_to_block(block);
        self.interrupt_safepoint();
        for (index, &pc) in thread.path.iter().enumerate() {
            let instruction = self.instructions[pc];
            match instruction.op().control_flow_layout() {
                // A dispatch copy ends in its table, run for real.
                ControlFlowLayout::BranchTable if matches!(thread.end, ThreadEnd::Dispatch) => {
                    self.instruction(function, pc, instruction);
                }
                // The path continues at the jump's target, or the table's
                // case: nothing to emit.
                ControlFlowLayout::Jump | ControlFlowLayout::BranchTable => {}
                ControlFlowLayout::ConditionalJump => {
                    if decided_branch(instruction, thread.selector, thread.value).is_some() {
                        continue;
                    }
                    let next = thread.path[index + 1];
                    let continuation = self.body.builder.create_block();
                    self.redirect = Some((next, continuation));
                    self.instruction(function, pc, instruction);
                    self.redirect = None;
                    self.body.builder.switch_to_block(continuation);
                }
                _ => {
                    self.instruction(function, pc, instruction);
                }
            }
        }
        let (ThreadEnd::Case(target) | ThreadEnd::Exit(target)) = thread.end else {
            return;
        };
        let target = self.block(target);
        self.body.builder.ins().jump(target, &[]);
    }

    /// Emit the instruction at `pc`; returns whether control falls through.
    fn instruction(
        &mut self,
        function: &Function,
        pc: usize,
        instruction: WideInstruction,
    ) -> bool {
        let op = instruction.op();
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        if let Some(operator) = I32BinaryOperator::from_register_op(op) {
            let left = self.read(b, Kind::I32);
            let right = self.read(c, Kind::I32);
            let value = self.integer_binary(widen_binary(operator), left, right);
            self.write(a, Kind::I32, value);
            return true;
        }
        if let Some(operator) = I32BinaryOperator::from_immediate_op(op) {
            let left = self.read(b, Kind::I32);
            let right = self
                .body.builder
                .ins()
                .iconst(types::I32, i64::from(instruction.imm() as i32));
            let value = self.integer_binary(widen_binary(operator), left, right);
            self.write(a, Kind::I32, value);
            return true;
        }
        if let Some(operator) = I32BinaryOperator::from_jump_op(op) {
            let left = self.read(a, Kind::I32);
            let right = self.read(b, Kind::I32);
            let holds = if self.direct_compare_branches {
                self.i32_comparison_condition(operator, left, right)
            } else {
                self.integer_binary(widen_binary(operator), left, right)
            };
            self.branch(pc, holds, instruction.jump_target() as usize, pc + 1);
            return false;
        }
        if let Some(operator) = I32BinaryOperator::from_immediate_jump_op(op) {
            let left = self.read(a, Kind::I32);
            let right = self.body.builder.ins().iconst(types::I32, i64::from(b as i16));
            let holds = if self.direct_compare_branches {
                self.i32_comparison_condition(operator, left, right)
            } else {
                self.integer_binary(widen_binary(operator), left, right)
            };
            self.branch(pc, holds, instruction.jump_target() as usize, pc + 1);
            return false;
        }
        if let Some(load) = MemoryLoad::from_direct_op(op) {
            let result = load_type(load).expect("planned");
            let kind = Kind::of(result).expect("planned");
            let address = self.read(c, Kind::I32);
            let host = self.address(address, instruction.imm(), load.width());
            let flags = self.flags(Region::LinearMemory);
            let ty = kind.ir();
            let value = match (load.width(), load_signed(load)) {
                // A float loads straight into its view.
                _ if result != kind.integer() => {
                    self.body.builder.ins().load(scalar_ir(result), flags, host, 0)
                }
                (1, false) => self.body.builder.ins().uload8(ty, flags, host, 0),
                (1, true) => self.body.builder.ins().sload8(ty, flags, host, 0),
                (2, false) => self.body.builder.ins().uload16(ty, flags, host, 0),
                (2, true) => self.body.builder.ins().sload16(ty, flags, host, 0),
                (4, false) if kind == Kind::I64 => self.body.builder.ins().uload32(flags, host, 0),
                (4, true) if kind == Kind::I64 => self.body.builder.ins().sload32(flags, host, 0),
                _ => self.body.builder.ins().load(ty, flags, host, 0),
            };
            self.write_typed(a, result, value);
            return true;
        }
        if let Some(store) = MemoryStore::from_direct_op(op) {
            let kind = Kind::of(store.value_type()).expect("planned");
            let value = self.read_typed(a, store.value_type());
            let address = self.read(c, Kind::I32);
            let width = store_width(store);
            let host = self.address(address, instruction.imm(), width);
            let flags = self.flags(Region::LinearMemory);
            match width {
                1 => self.body.builder.ins().istore8(flags, value, host, 0),
                2 => self.body.builder.ins().istore16(flags, value, host, 0),
                4 if kind == Kind::I64 => self.body.builder.ins().istore32(flags, value, host, 0),
                _ => self.body.builder.ins().store(flags, value, host, 0),
            };
            return true;
        }
        if op == Op::WasmMemorySize {
            let bytes = self.body.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                self.body.context,
                CONTEXT_MEMORY_LENGTH,
            );
            let pages = self.body.builder.ins().ushr_imm_u(bytes, 16);
            let pages = self.body.builder.ins().ireduce(types::I32, pages);
            self.write(a, Kind::I32, pages);
            return true;
        }
        if op == Op::WasmMemoryFill {
            let (destination, _) = instruction.register_pair();
            let destination = self.read(destination, Kind::I32);
            let destination = self.body.builder.ins().uextend(types::I64, destination);
            let raw_length = self.read(c, Kind::I32);
            let length = self.body.builder.ins().uextend(types::I64, raw_length);
            let end = self.body.builder.ins().iadd(destination, length);
            let memory_length = self.body.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                self.body.context,
                CONTEXT_MEMORY_LENGTH,
            );
            let out_of_bounds =
                self.body.builder
                    .ins()
                    .icmp(IntCC::UnsignedGreaterThan, end, memory_length);
            self.trap_if(out_of_bounds, crate::WasmTrap::OutOfBoundsMemory);
            let address = self.body.builder.ins().iadd(self.base, destination);
            let raw_byte = self.read(b, Kind::I32);
            let byte = self.body.builder.ins().uextend(types::I64, raw_byte);
            let mut signature = Signature::new(self.call_conv);
            for _ in 0..3 {
                signature.params.push(AbiParam::new(types::I64));
            }
            signature.returns.push(AbiParam::new(types::I64));
            let signature = self.body.builder.import_signature(signature);
            let target = self.body.builder.ins().iconst(
                types::I64,
                compiled_memory_fill as *const () as usize as i64,
            );
            self.body.builder
                .ins()
                .call_indirect(signature, target, &[address, byte, length]);
            return true;
        }
        if let Some((operator, ty)) = float_binary_operator(op, instruction.imm()) {
            let left = self.read_typed(b, ty);
            let right = self.read_typed(c, ty);
            let value = self.float_binary(operator, left, right);
            let result = if self.body.builder.func.dfg.value_type(value) == types::I32 {
                crate::WasmType::I32
            } else {
                ty
            };
            self.write_typed(a, result, value);
            return true;
        }
        if let Some((operator, ty)) = float_unary_operator(op, instruction.imm()) {
            let input = self.read_typed(b, ty);
            let value = self.float_unary(operator, input);
            self.write_typed(a, ty, value);
            return true;
        }
        match op {
            Op::WasmI64Binary => {
                let operator = I64BinaryOperator::from_tag(instruction.imm()).expect("planned");
                let left = self.read(b, Kind::I64);
                let right = self.read(c, Kind::I64);
                let value = self.integer_binary(operator, left, right);
                let kind = Kind::of_ir(self.body.builder.func.dfg.value_type(value));
                self.write(a, kind, value);
            }
            Op::WasmI64Unary => {
                let operator = I64UnaryOperator::from_tag(instruction.imm()).expect("planned");
                let input = self.read(b, Kind::I64);
                let value = self.integer_unary(operator, input);
                let kind = Kind::of_ir(self.body.builder.func.dfg.value_type(value));
                self.write(a, kind, value);
            }
            Op::WasmScalarConvert => {
                let operator =
                    ScalarConversionOperator::from_tag(instruction.imm()).expect("planned");
                let input = self.read_typed(b, operator.source_type());
                let value = self.convert(operator, input);
                self.write_typed(a, operator.result_type(), value);
            }
            Op::Move => {
                let kind = self.operand_kind(pc);
                let value = self.read(b, kind);
                self.write(a, kind, value);
            }
            Op::LoadConst | Op::WasmFillRegisters => {
                let constant = self
                    .analysis
                    .constant(instruction.constant_index())
                    .expect("planned");
                let kind = self.analysis.constant_kind(constant);
                let bits = self.analysis.constant_bits(constant);
                if op == Op::LoadConst {
                    self.write_constant(a, kind, bits);
                } else {
                    let window = instruction.register_window();
                    for offset in 0..window.count {
                        self.write_constant(window.base + offset, kind, bits);
                    }
                }
            }
            Op::WasmI32Unary => {
                let operator = I32UnaryOperator::from_tag(instruction.imm()).expect("planned");
                let input = self.read(b, Kind::I32);
                let value = self.integer_unary(widen_unary(operator), input);
                self.write(a, Kind::I32, value);
            }
            Op::WasmI32ShiftRightUnsignedAndImmediate => {
                let input = self.read(b, Kind::I32);
                let shift = self.body.builder.ins().iconst(types::I32, i64::from(c as i16));
                let shifted =
                    self.integer_binary(I64BinaryOperator::ShiftRightUnsigned, input, shift);
                let mask = self
                    .body.builder
                    .ins()
                    .iconst(types::I32, i64::from(instruction.imm() as i32));
                let value = self.integer_binary(I64BinaryOperator::And, shifted, mask);
                self.write(a, Kind::I32, value);
            }
            Op::WasmI32AddImmediateJumpNonZero => {
                let input = self.read(b, Kind::I32);
                let addend = self.body.builder.ins().iconst(types::I32, i64::from(c as i16));
                let sum = self.body.builder.ins().iadd(input, addend);
                self.write(a, Kind::I32, sum);
                self.branch(pc, sum, instruction.jump_target() as usize, pc + 1);
                return false;
            }
            Op::WasmJumpI32Zero | Op::WasmJumpI32NonZero => {
                let value = self.read(a, Kind::I32);
                let (taken, other) = (instruction.jump_target() as usize, pc + 1);
                if op == Op::WasmJumpI32NonZero {
                    self.branch(pc, value, taken, other);
                } else {
                    self.branch(pc, value, other, taken);
                }
                return false;
            }
            Op::Jump => {
                let target = instruction.jump_target() as usize;
                self.jump(target, self.polls(pc, target));
                return false;
            }
            Op::WasmBranchTable => {
                let selector = self.read(a, Kind::I32);
                let entries: Vec<usize> = crate::bytecode::branch_table_entries(pc, instruction)
                    .map(|entry| {
                        super::dispatch_fast::decoded_instruction(function, function.code[entry])
                            .jump_target() as usize
                    })
                    .collect();
                // A table entry at or before `pc` is a backedge; a loop may
                // repeat through the table alone, so it polls like a jump.
                if entries.iter().any(|&target| target <= pc) {
                    self.interrupt_safepoint();
                }
                let (default, cases) = entries.split_last().expect("a default entry");
                let default = self.block(*default);
                let default = self.body.builder.func.dfg.block_call(default, &[]);
                let cases: Vec<_> = cases
                    .iter()
                    .map(|target| {
                        let block = self.block(*target);
                        self.body.builder.func.dfg.block_call(block, &[])
                    })
                    .collect();
                let table = ir::JumpTableData::new(default, &cases);
                let table = self.body.builder.create_jump_table(table);
                self.body.builder.ins().br_table(selector, table);
                return false;
            }
            Op::WasmSelect => {
                let (condition, _) = instruction.register_pair();
                let kind = self.operand_kind(pc);
                let condition = self.read(condition, Kind::I32);
                let taken = self.read(b, kind);
                let other = self.read(c, kind);
                let value = self.body.builder.ins().select(condition, taken, other);
                self.write(a, kind, value);
            }
            Op::CallKnown => {
                let callee = usize::from(instruction.known_function_index());
                let Some(callee_plan) = self.plans[callee].as_ref() else {
                    self.callout_known_fallback(callee as u32, instruction);
                    return true;
                };
                let window = instruction.call_window();
                let mut args = vec![self.body.context];
                if self.memory.base {
                    args.push(self.base);
                }
                args.extend(self.length);
                for (offset, ty) in callee_plan.params.iter().enumerate() {
                    let kind = Kind::of(*ty).expect("planned");
                    args.push(self.read(window.base + offset as u16, kind));
                }
                // Compiled functions share one code buffer: a direct call,
                // linked at placement (`link`).
                let target = match self.callees[callee] {
                    Some(target) => target,
                    None => {
                        let signature = internal_signature(
                            self.memory,
                            &callee_plan.params,
                            callee_plan.result,
                            self.call_conv,
                        );
                        let signature = self.body.builder.import_signature(signature);
                        let name = self
                            .body.builder
                            .func
                            .declare_imported_user_function(function_name(callee));
                        let target = self.body.builder.import_function(ir::ExtFuncData {
                            name: ir::ExternalName::User(name),
                            signature,
                            colocated: true,
                            patchable: false,
                        });
                        self.callees[callee] = Some(target);
                        target
                    }
                };
                let call = self.body.builder.ins().call(target, &args);
                let results = self.body.builder.inst_results(call).to_vec();
                self.propagate(results[0]);
                let destination = instruction.result_register();
                match callee_plan.result {
                    Some(ty) => self.write(destination, Kind::of(ty).expect("planned"), results[1]),
                    None => {
                        let zero = self.body.builder.ins().iconst(types::I32, 0);
                        self.write(destination, Kind::I32, zero);
                    }
                }
            }
            Op::WasmRefFunc => {}
            Op::WasmIndirectTarget => self.indirect_target(pc, instruction),
            Op::Call => match self.plan.operand(pc) {
                Slot::Import(_) => self.callout(pc, instruction),
                Slot::IndirectReference { .. } => self.call_reference(pc, instruction),
                _ => unreachable!("planned calls hold a supported callable"),
            },
            Op::WasmUnreachable => {
                let trap = self.trap_block(crate::WasmTrap::Unreachable);
                self.body.builder.ins().jump(trap, &[]);
                return false;
            }
            Op::Return => {
                let status = self.body.builder.ins().iconst(types::I64, 0);
                let mut values = vec![status];
                if let Some(ty) = self.plan.result {
                    values.push(self.read(a, Kind::of(ty).expect("planned")));
                }
                self.body.builder.ins().return_(&values);
                return false;
            }
            // A binding is known statically per instruction; the globals it
            // names are reached through the context.
            Op::WasmInstanceBinding => {}
            Op::WasmGlobalGet => {
                let slot = self.binding_slot(pc);
                let address = self.global_address(slot);
                let flags = self.flags(Region::Globals);
                let value = self.body.builder.ins().load(types::I32, flags, address, 0);
                self.write(a, Kind::I32, value);
            }
            Op::WasmGlobalSet => {
                let slot = self.binding_slot(pc);
                let address = self.global_address(slot);
                let value = self.read(a, Kind::I32);
                let wide = self.body.builder.ins().uextend(types::I64, value);
                let encoded = self.body.builder.ins().bor_imm_u(wide, INTEGER_TAG as i64);
                let flags = self.flags(Region::Globals);
                self.body.builder.ins().store(flags, encoded, address, 0);
            }
            _ => unreachable!("planned instructions have a denotation"),
        }
        true
    }
}

impl<'a, H: Host> Analysis<'a, H> {
    /// Compile `index` with `plan`; `None` when Cranelift refuses it or its
    /// code needs a relocation other than a call to a compiled function.
    fn compile_function(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plan: &Plan,
        plans: &[Option<Plan>],
        guarded: bool,
        fixed_base: Option<i32>,
        globals: &[u32],
        reference_site_ids: &'a [(usize, usize)],
        diagnostics_enabled: bool,
    ) -> Option<(Vec<u8>, Vec<Call>)> {
        let function = &self.code.function(index);
        let call_conv = isa.default_call_conv();
        let memory = MemoryArguments::of(fixed_base, guarded);
        let mut func = ir::Function::with_name_signature(
            UserFuncName::User(function_name(index)),
            internal_signature(memory, &plan.params, plan.result, call_conv),
        );
        let mut builder_context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let context = parameters[0];
        let mut passed = parameters[1..].iter().copied();
        // A constant base folds into each access's addressing mode and
        // frees the register a parameter would occupy.
        let base = match fixed_base {
            Some(fixed) => builder.ins().iconst(types::I64, i64::from(fixed)),
            None => passed.next().expect("a passed base"),
        };
        let length = memory
            .length
            .then(|| passed.next().expect("a passed length"));
        let wasm_parameters = &parameters[1 + memory.count()..];
        let regions = Region::ALL.map(|region| {
            builder.func.dfg.alias_regions.insert(ir::AliasRegionData {
                user_id: region as u32,
                description: format!("{region:?}").into(),
            })
        });
        let instructions: Vec<WideInstruction> = function
            .code
            .iter()
            .map(|packed| super::dispatch_fast::decoded_instruction(function, *packed))
            .collect();
        let affine_loop = (self.code.kind == crate::bytecode::ProgramKind::Wasm
            && jit_affine_pair_enabled())
        .then(|| {
            super::dispatch_fast::memory_register(function).and_then(|memory_register| {
                affine_pair_loop(
                    function.parameter_registers,
                    function.registers,
                    plan,
                    &instructions,
                    memory_register,
                )
            })
        })
        .flatten();
        if diagnostics_enabled && let Some(candidate) = affine_loop {
            eprintln!(
                "QUENCH_WASM_JIT_AFFINE_PAIR function={index} header={} branch={} induction={} bound={} destination={} left={} right={} max_end={}",
                candidate.header,
                candidate.branch,
                candidate.induction,
                candidate.bound,
                candidate.destination,
                candidate.left,
                candidate.right,
                candidate.max_end
            );
        }
        let affine_attempt = affine_loop.map(|_| {
            let variable = builder.declare_var(types::I32);
            let initial = builder.ins().iconst(types::I32, 0);
            builder.def_var(variable, initial);
            variable
        });
        let mut selectors: Vec<u16> = instructions
            .iter()
            .enumerate()
            .filter(|(pc, instruction)| {
                plan.reachable(*pc) && instruction.op() == Op::WasmBranchTable
            })
            .map(|(_, instruction)| instruction.a())
            .collect();
        selectors.sort_unstable();
        selectors.dedup();
        let mut emitter = Emitter {
            guarded,
            direct_compare_branches: jit_direct_compare_branches_enabled(),
            reference_body_cache: jit_reference_body_cache_enabled(),
            emscripten_memcpy_leaf: jit_emscripten_memcpy_leaf_enabled(),
            regions,
            globals,
            analysis: self,
            body: Body::new(builder, context, poll_address(&self.vm.execution_interrupt)),
            plan,
            blocks: vec![None; function.code.len()],
            reference_site_ids,
            diagnostics_enabled,
            base,
            length,
            memory,
            trap_blocks: [None; TRAPS.len()],
            callees: vec![None; plans.len()],
            callout_signature: None,
            plans,
            call_conv,
            instructions: &instructions,
            affine_loop,
            affine_attempt,
            selectors,
            known: Vec::new(),
            threads: Vec::new(),
            pending: Vec::new(),
            redirect: None,
            threading: true,
            unrolled: if program_unrolls(self.code) {
                unrollable_loops(&instructions, plan)
            } else {
                vec![None; instructions.len()]
            },
            backedge: None,
            pending_copies: Vec::new(),
        };
        // Registers start at i32 zero, as an activation's register file does.
        for register in 0..function.registers {
            emitter.write_constant(register, Kind::I32, 0);
            emitter.write_constant(register, Kind::I64, 0);
        }
        let parameter_base = function.parameter_registers.expect("planned");
        for (offset, ty) in plan.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            emitter.write(
                parameter_base + offset as u16,
                kind,
                wasm_parameters[offset],
            );
        }
        // Each activation checks the stack it may still use.
        let stack_pointer = emitter.body.builder.ins().get_stack_pointer(types::I64);
        let limit =
            emitter
                .body.builder
                .ins()
                .load(types::I64, invariant(), context, CONTEXT_STACK_LIMIT);
        let exhausted = emitter
            .body.builder
            .ins()
            .icmp(IntCC::UnsignedLessThan, stack_pointer, limit);
        emitter.trap_if(exhausted, crate::WasmTrap::CallStackExhausted);
        emitter.known.clear();
        emitter.jump(0, true);
        // Every jump target starts a block, created before emission so a
        // backward jump finds its target's block already placed.
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            if !plan.reachable(pc) {
                continue;
            }
            let targets = match instruction.op().control_flow_layout() {
                ControlFlowLayout::BranchTable => {
                    crate::bytecode::branch_table_entries(pc, instruction)
                        .map(|entry| instructions[entry].jump_target() as usize)
                        .collect()
                }
                ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump => {
                    vec![instruction.jump_target() as usize, pc + 1]
                }
                _ => vec![],
            };
            for target in targets {
                if target < instructions.len() && plan.reachable(target) {
                    emitter.block(target);
                }
            }
        }
        let mut open = false;
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            if !plan.reachable(pc) {
                open = false;
                continue;
            }
            let is_leader = emitter.blocks[pc].is_some() || !open;
            if is_leader {
                emitter.known.clear();
                let block = emitter.block(pc);
                if open {
                    emitter.body.builder.ins().jump(block, &[]);
                }
                emitter.body.builder.switch_to_block(block);
                if emitter
                    .affine_loop
                    .is_some_and(|candidate| candidate.header == pc)
                {
                    emitter.affine_pair_gate();
                }
            }
            open = emitter.latch(function, pc, instruction);
            // Copies of an unrolled body follow its latch in the layout, so a loop that runs a few
            // iterations stays in one stretch of code.
            for (block, unrolled, latch, next) in std::mem::take(&mut emitter.pending_copies) {
                emitter.emit_unrolled_copy(function, block, unrolled, latch, next);
            }
            // A later jump may target the next instruction; give it a block
            // now so falling into it stays explicit.
            if open && emitter.blocks.get(pc + 1).is_some_and(Option::is_some) {
                let next = emitter.block(pc + 1);
                emitter.body.builder.ins().jump(next, &[]);
                open = false;
            }
        }
        if open {
            return None;
        }
        emitter.threading = false;
        for (block, thread) in std::mem::take(&mut emitter.pending) {
            emitter.emit_thread(function, block, &thread);
        }
        emitter.finish_traps();
        emitter.body.builder.seal_all_blocks();
        emitter.body.builder.finalize(isa.frontend_config());
        let mut context = cranelift_codegen::Context::for_function(func);
        let compiled = context
            .compile(
                isa,
                &mut cranelift_codegen::control::ControlPlane::default(),
            )
            .ok()?;
        let code = compiled.code_buffer().to_vec();
        let relocations = compiled.buffer.relocs().to_vec();
        let names = context.func.params.user_named_funcs();
        let calls = relocations
            .iter()
            .map(|relocation| {
                let cranelift_codegen::FinalizedRelocTarget::ExternalName(ir::ExternalName::User(
                    name,
                )) = relocation.target
                else {
                    return None;
                };
                let name = names.get(name)?;
                (name.namespace == FUNCTION_NAMESPACE).then_some(Call {
                    offset: relocation.offset as usize,
                    kind: relocation.kind,
                    addend: relocation.addend,
                    callee: name.index as usize,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some((code, calls))
    }

    /// The C entry of `index`: unpack arguments, call through the table and
    /// store the result.
    fn compile_entry(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plan: &Plan,
        memory: MemoryArguments,
    ) -> Option<Vec<u8>> {
        let call_conv = isa.default_call_conv();
        let signature = native_entry_signature(isa.pointer_type(), call_conv);
        let mut func = ir::Function::with_name_signature(
            UserFuncName::user(ENTRY_NAMESPACE, index as u32),
            signature,
        );
        let mut builder_context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        builder.seal_block(block);
        let parameters = builder.block_params(block).to_vec();
        let (context, base, length, args, out) = (
            parameters[0],
            parameters[1],
            parameters[2],
            parameters[3],
            parameters[4],
        );
        let context = if builder.func.dfg.value_type(context) == types::I64 {
            context
        } else {
            builder.ins().uextend(types::I64, context)
        };
        let mut call_args = vec![context];
        call_args.extend(memory.select(base, length));
        for (offset, ty) in plan.params.iter().enumerate() {
            let raw = builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                args,
                (offset * 8) as i32,
            );
            call_args.push(match Kind::of(*ty).expect("planned") {
                Kind::I32 => builder.ins().ireduce(types::I32, raw),
                Kind::I64 => raw,
            });
        }
        let target_signature = builder.import_signature(internal_signature(
            memory,
            &plan.params,
            plan.result,
            call_conv,
        ));
        let table = builder
            .ins()
            .load(types::I64, MemFlagsData::trusted(), context, CONTEXT_TABLE);
        let target = builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            table,
            (index * 8) as i32,
        );
        let call = builder
            .ins()
            .call_indirect(target_signature, target, &call_args);
        let results = builder.inst_results(call).to_vec();
        if let Some(ty) = plan.result {
            let value = match Kind::of(ty).expect("planned") {
                Kind::I32 => builder.ins().uextend(types::I64, results[1]),
                Kind::I64 => results[1],
            };
            builder.ins().store(MemFlagsData::trusted(), value, out, 0);
        }
        let status = builder.ins().ireduce(types::I32, results[0]);
        builder.ins().return_(&[status]);
        builder.finalize(isa.frontend_config());
        let mut context = cranelift_codegen::Context::for_function(func);
        let compiled = context
            .compile(
                isa,
                &mut cranelift_codegen::control::ControlPlane::default(),
            )
            .ok()?;
        if !compiled.buffer.relocs().is_empty() {
            return None;
        }
        Some(compiled.code_buffer().to_vec())
    }
}

/// The guard reservation of an unshared memory binding.
/// A memory base an x86-64 addressing mode can carry as its signed 32-bit
/// displacement.
fn displacement_base(base: *mut u8) -> Option<i32> {
    i32::try_from(base as usize).ok()
}

fn guarded_memory<H: Host>(vm: &Vm<H>, binding: Value) -> Option<(*mut u8, usize)> {
    memory_bytes(vm, binding, crate::wasm::memory::LinearBytes::guard)
}

/// `query` of the bytes of the unshared memory `binding` names.
fn memory_bytes<H: Host, T>(
    vm: &Vm<H>,
    binding: Value,
    query: impl FnOnce(&crate::wasm::memory::LinearBytes) -> Option<T>,
) -> Option<T> {
    let Some(Cell::WasmMemory { bytes, .. }) = vm.heap.get(binding) else {
        return None;
    };
    let crate::wasm::memory::MemoryStorage::Unshared(state) = &**bytes else {
        return None;
    };
    query(&state.try_borrow().ok()?.bytes)
}

thread_local! {
    /// The context of the compiled call running on this thread, which a
    /// guard fault resumes from.
    static ACTIVE_CONTEXT: std::cell::Cell<*mut CallContext> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Enters compiled code: (context, base, length, arguments, result, entry)
/// -> status. It saves the caller's callee-saved registers, records its
/// stack pointer in the context, and calls `entry`; a guard fault resumes at
/// its landing, which returns the out-of-bounds status through the same
/// epilogue.
type Trampoline =
    unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, *mut u64, Entry) -> u32;

/// The trampoline and its landing, once the fault handler is installed.
struct GuardSupport {
    _code: CodeMemory,
    trampoline: Trampoline,
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    landing: usize,
    /// Where a fault on the poll page resumes: the terminated status.
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    termination: usize,
}

// SAFETY: immutable executable code, shared by every thread.
unsafe impl Send for GuardSupport {}
unsafe impl Sync for GuardSupport {}

/// The x86-64 System V trampoline, as machine code. Offsets of `RESUME` and
/// `LANDING` are into this array.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const TRAMPOLINE_CODE: [u8; 60] = [
    0x55, // push rbp
    0x48,
    0x89,
    0xe5, // mov rbp, rsp
    0x53, // push rbx
    0x41,
    0x54, // push r12
    0x41,
    0x55, // push r13
    0x41,
    0x56, // push r14
    0x41,
    0x57, // push r15
    0x48,
    0x83,
    0xec,
    0x08, // sub rsp, 8 (keeps the call aligned)
    0x48,
    0x89,
    0x67,
    CONTEXT_ENTRY_STACK, // mov [rdi + entry_stack], rsp
    0x41,
    0xff,
    0xd1, // call r9
    // RESUME (24):
    0x48,
    0x83,
    0xc4,
    0x08, // add rsp, 8
    0x41,
    0x5f, // pop r15
    0x41,
    0x5e, // pop r14
    0x41,
    0x5d, // pop r13
    0x41,
    0x5c, // pop r12
    0x5b, // pop rbx
    0x5d, // pop rbp
    0xc3, // ret
    // LANDING (39):
    0xb8,
    0,
    0,
    0,
    0, // mov eax, status (patched)
    0xe9,
    0,
    0,
    0,
    0, // jmp RESUME (patched)
    // TERMINATION (49):
    0xb8,
    0,
    0,
    0,
    0, // mov eax, terminated status (patched)
    0xe9,
    0,
    0,
    0,
    0,    // jmp RESUME (patched)
    0xcc, // int3
];

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const TRAMPOLINE_RESUME: usize = 24;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const TRAMPOLINE_LANDING: usize = 39;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const TRAMPOLINE_TERMINATION: usize = 49;

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
static PREVIOUS_SEGV: std::sync::OnceLock<libc::sigaction> = std::sync::OnceLock::new();

/// The guard support of this host, installing the fault handler on first
/// use; `None` where guard faults cannot be resumed.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn guard_support() -> Option<&'static GuardSupport> {
    static SUPPORT: std::sync::OnceLock<Option<GuardSupport>> = std::sync::OnceLock::new();
    SUPPORT
        .get_or_init(|| {
            let mut code = TRAMPOLINE_CODE;
            // Each landing loads its status and jumps back to the epilogue.
            for (landing, status) in [
                (
                    TRAMPOLINE_LANDING,
                    trap_status(crate::WasmTrap::OutOfBoundsMemory) as u32,
                ),
                (TRAMPOLINE_TERMINATION, TERMINATED_STATUS as u32),
            ] {
                code[landing + 1..landing + 5].copy_from_slice(&status.to_le_bytes());
                let after_jump = landing + 10;
                let displacement = TRAMPOLINE_RESUME as i32 - after_jump as i32;
                code[landing + 6..landing + 10].copy_from_slice(&displacement.to_le_bytes());
            }
            let memory = CodeMemory::new(&code)?;
            // SAFETY: the code implements `Trampoline` for this ABI.
            let trampoline = unsafe { std::mem::transmute::<*mut u8, Trampoline>(memory.pointer) };
            let landing = memory.pointer as usize + TRAMPOLINE_LANDING;
            let termination = memory.pointer as usize + TRAMPOLINE_TERMINATION;
            // SAFETY: installs a handler that only redirects faults inside an
            // active compiled call's guard and forwards every other fault to
            // the previous handler.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = guard_fault as *const () as usize;
                action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK | libc::SA_NODEFER;
                libc::sigemptyset(&mut action.sa_mask);
                let mut previous: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(libc::SIGSEGV, &action, &mut previous) != 0 {
                    return None;
                }
                let _ = PREVIOUS_SEGV.set(previous);
            }
            Some(GuardSupport {
                _code: memory,
                trampoline,
                landing,
                termination,
            })
        })
        .as_ref()
}

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
fn guard_support() -> Option<&'static GuardSupport> {
    None
}

fn guard_trampoline() -> Option<Trampoline> {
    guard_support().map(|support| support.trampoline)
}

/// Where compiled code polls for termination: the VM's poll page when every entry runs
/// through the fault-resuming trampoline, else nowhere, and code checks the context's
/// interrupt flag.
fn poll_address(interrupt: &crate::execution_interrupt::ExecutionInterrupt) -> Option<i64> {
    guard_support()
        .and_then(|_| interrupt.poll_page())
        .map(|(start, _)| start as i64)
}

/// Runs compiled `entry` through the shared ABI. Resumable entries use the host trampoline
/// and can return from guard or poll faults; safe JS leaves use a prechecked direct entry.
///
/// # Safety
/// `entry` is compiled code for this `Entry` signature, and `context`, `arguments` and
/// `out` are what it reads and writes for the call's duration.
unsafe fn enter_native(
    context: &mut CallContext,
    base: u64,
    length: u64,
    arguments: *const u64,
    out: *mut u64,
    mode: NativeEntryMode,
    entry: Entry,
) -> u32 {
    if mode == NativeEntryMode::Leaf {
        let marker = 0u8;
        let stack_pointer = std::hint::black_box(&marker as *const u8 as usize);
        if stack_pointer
            < context
                .stack_limit
                .saturating_add(JS_LEAF_STACK_RESERVE)
        {
            // A low stack declines the fast path, so the VM can execute the
            // instruction through its ordinary guarded dispatcher.
            return 1;
        }
        // SAFETY: leaf artifacts have no polls, heap access, or VM callouts.
        // Their fixed ABI arguments stay live through this bounded call.
        return unsafe { entry(context, base, length, arguments, out) };
    }
    match guard_trampoline() {
        Some(trampoline) => {
            let outer = ACTIVE_CONTEXT.with(|active| active.replace(context));
            // SAFETY: as the caller promises; the trampoline saves the caller's registers
            // and records where a fault resumes.
            let status = unsafe { trampoline(context, base, length, arguments, out, entry) };
            ACTIVE_CONTEXT.with(|active| active.set(outer));
            status
        }
        // SAFETY: as the caller promises.
        None => unsafe { entry(context, base, length, arguments, out) },
    }
}

/// The poll page compiled code faults on to stop for `interrupt`: only where the guard
/// trampoline can resume the fault.
fn native_poll_page(interrupt: &crate::ExecutionInterrupt) -> Option<(usize, usize)> {
    guard_trampoline()?;
    interrupt.poll_page()
}

/// The stack limit a native entry starting at the current stack pointer runs under.
fn native_stack_limit() -> usize {
    let marker = 0u8;
    let stack_pointer = std::hint::black_box(&marker as *const u8 as usize);
    stack_pointer.saturating_sub(COMPILED_STACK_BUDGET)
}

thread_local! {
    /// The context every leaf entry (`NativeEntryMode::Leaf`) runs with. A leaf makes no
    /// calls, polls or callouts, so it reads only the stack limit its entry checks and the
    /// adapter payload, which each leaf call sets before it enters (`run_leaf`); the rest
    /// stays the empty adapter context.
    static LEAF_CONTEXT: std::cell::UnsafeCell<CallContext> =
        std::cell::UnsafeCell::new(CallContext::native_adapter(std::ptr::null_mut(), std::ptr::null(), None));
}

/// Runs leaf `entry` through the shared ABI on `arguments` and the adapter `payload`.
fn run_leaf(payload: *mut (), arguments: *const u64, entry: Entry) -> (u32, u64) {
    LEAF_CONTEXT.with(|context| {
        // SAFETY: a leaf makes no calls, so no other use of this thread's leaf context
        // overlaps this one.
        let context = unsafe { &mut *context.get() };
        context.stack_limit = native_stack_limit();
        context.adapter_payload = payload;
        let mut result = 0u64;
        // SAFETY: the payload, arguments and result outlive the leaf call.
        let status = unsafe {
            enter_native(context, 0, 0, arguments, &mut result, NativeEntryMode::Leaf, entry)
        };
        (status, result)
    })
}

/// Runs a JavaScript native denotation through the shared entry ABI. The
/// artifact selects resumable or prechecked leaf mode; inputs stay borrowed.
fn run_native_adapter(
    payload: *mut (),
    interrupt: *const std::sync::atomic::AtomicBool,
    poll_page: Option<(usize, usize)>,
    mode: NativeEntryMode,
    entry: Entry,
) -> (u32, u64) {
    if mode == NativeEntryMode::Leaf {
        return run_leaf(payload, std::ptr::null(), entry);
    }
    let mut context = CallContext::native_adapter(payload, interrupt, poll_page);
    let mut result = 0u64;
    // SAFETY: the payload, context, and result remain live until the common
    // native entry returns; the JS adapter uses none of the Wasm arguments.
    let status = unsafe {
        enter_native(
            &mut context,
            0,
            0,
            std::ptr::null(),
            &mut result,
            mode,
            entry,
        )
    };
    (status, result)
}

/// Runs a JS scalar artifact whose machine code already implements the common `Entry` ABI.
/// Its Number arguments are raw f64 words and its result is written to the shared result word.
fn run_native_entry(
    entry: Entry,
    arguments: &[f64],
    interrupt: &std::sync::atomic::AtomicBool,
    poll_page: Option<(usize, usize)>,
    mode: NativeEntryMode,
) -> (u32, u64) {
    if mode == NativeEntryMode::Leaf {
        return run_leaf(std::ptr::null_mut(), arguments.as_ptr().cast::<u64>(), entry);
    }
    let interrupt = poll_page.map_or_else(
        || std::ptr::from_ref(interrupt),
        |(start, _)| start as *const std::sync::atomic::AtomicBool,
    );
    let mut context = CallContext::native_adapter(std::ptr::null_mut(), interrupt, poll_page);
    let mut result = 0u64;
    // SAFETY: the scalar region is guarded before entry, reads only `arguments`, writes one
    // result word, and its common ABI does not inspect the adapter payload.
    let status = unsafe {
        enter_native(
            &mut context,
            0,
            0,
            arguments.as_ptr().cast::<u64>(),
            &mut result,
            mode,
            entry,
        )
    };
    (status, result)
}

/// A fault inside the running compiled call's guard reservation resumes at
/// the trampoline's landing with the trampoline's saved stack; anything else
/// belongs to the previous handler.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
extern "C" fn guard_fault(signal: i32, info: *mut libc::siginfo_t, context: *mut libc::c_void) {
    let active = ACTIVE_CONTEXT.with(std::cell::Cell::get);
    // SAFETY: the kernel passes valid signal information and context.
    unsafe {
        let address = (*info).si_addr() as usize;
        if !active.is_null()
            && let Some(support) = guard_support()
        {
            let landing = if (*active).guard_start <= address && address < (*active).guard_end {
                Some(support.landing)
            } else if (*active).poll_start <= address && address < (*active).poll_end {
                Some(support.termination)
            } else {
                None
            };
            if let Some(landing) = landing {
                let context = &mut *context.cast::<libc::ucontext_t>();
                context.uc_mcontext.gregs[libc::REG_RSP as usize] = (*active).entry_stack as i64;
                context.uc_mcontext.gregs[libc::REG_RIP as usize] = landing as i64;
                return;
            }
        }
        let Some(previous) = PREVIOUS_SEGV.get() else {
            return;
        };
        match previous.sa_sigaction {
            libc::SIG_DFL | libc::SIG_IGN => {
                // Restore the previous disposition; the fault repeats on return.
                libc::sigaction(libc::SIGSEGV, previous, std::ptr::null_mut());
            }
            handler if previous.sa_flags & libc::SA_SIGINFO != 0 => {
                let handler = std::mem::transmute::<
                    usize,
                    extern "C" fn(i32, *mut libc::siginfo_t, *mut libc::c_void),
                >(handler);
                handler(signal, info, context);
            }
            handler => {
                let handler = std::mem::transmute::<usize, extern "C" fn(i32)>(handler);
                handler(signal);
            }
        }
    }
}

/// The name namespace of compiled function bodies; their C entries use
/// `ENTRY_NAMESPACE`.
const FUNCTION_NAMESPACE: u32 = 0;
const ENTRY_NAMESPACE: u32 = 1;

fn function_name(index: usize) -> ir::UserExternalName {
    ir::UserExternalName::new(FUNCTION_NAMESPACE, index as u32)
}

/// A direct call in a function body, linked once bodies are placed.
struct Call {
    /// The relocated field, from the start of the body.
    offset: usize,
    kind: cranelift_codegen::binemit::Reloc,
    addend: i64,
    callee: usize,
}

/// Point `call` in the body at `body` to the callee placed at `callee`; all
/// are offsets in one buffer, so the relative displacement is final.
fn link(bytes: &mut [u8], body: usize, call: &Call, callee: usize) -> Option<()> {
    use cranelift_codegen::binemit::Reloc;
    let site = body + call.offset;
    let displacement = callee as i64 - site as i64 + call.addend;
    match call.kind {
        Reloc::X86CallPCRel4 => {
            let field = i32::try_from(displacement).ok()?;
            bytes
                .get_mut(site..site + size_of::<i32>())?
                .copy_from_slice(&field.to_le_bytes());
        }
        Reloc::Arm64Call => {
            /// A BL immediate: a signed word displacement in the low bits.
            const IMMEDIATE_BITS: u32 = 26;
            const WORD_SHIFT: u32 = 2;
            let words = displacement >> WORD_SHIFT;
            let range = 1i64 << (IMMEDIATE_BITS - 1);
            if displacement & ((1 << WORD_SHIFT) - 1) != 0 || !(-range..range).contains(&words) {
                return None;
            }
            let field = bytes.get_mut(site..site + size_of::<u32>())?;
            let mask = (1u32 << IMMEDIATE_BITS) - 1;
            let instruction = u32::from_le_bytes(field.try_into().ok()?);
            let instruction = (instruction & !mask) | (words as u32 & mask);
            field.copy_from_slice(&instruction.to_le_bytes());
        }
        _ => return None,
    }
    Some(())
}

/// Code alignment for each compiled function.
const FUNCTION_ALIGNMENT: usize = 64;

impl<H: Host> Vm<H> {
    /// Invalidate artifact-local indirect-call certificates when VM state that
    /// native code cannot inspect directly changes: table contents or heap IDs.
    pub(super) fn invalidate_wasm_reference_caches(&self) {
        let next = self.wasm_reference_epoch.get().wrapping_add(1);
        self.wasm_reference_epoch
            .set(if next == 0 { 1 } else { next });
    }

    pub(crate) fn set_execution_mode(&mut self, mode: ExecutionMode) {
        self.execution_mode = mode;
    }

    pub(crate) fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// A compiled tier's poll page is usable only when the shared entry can
    /// resume the protected-page fault back into the interpreter.
    pub(super) fn compiled_poll_page(&self) -> Option<(usize, usize)> {
        native_poll_page(&self.execution_interrupt)
    }

    /// Whether the per-function JavaScript tier runs: compiled execution, in a build whose
    /// profiles do not count interpreted work exactly.
    pub(super) fn js_tier_runs(&self) -> bool {
        !cfg!(quench_profiled) && self.execution_mode == ExecutionMode::Compiled
    }

    /// Keep usable legacy specialized candidates first. If that tier rejects a
    /// bounded function, observe it for the complete-semantics dispatcher so
    /// lane calls can promote it instead of remaining frameless forever.
    pub(super) fn js_tier_may_claim(
        &mut self,
        program: &ResidualProgram,
        id: ProgramId,
        function: u32,
    ) -> bool {
        if !self.js_tier_runs() || program.kind == crate::bytecode::ProgramKind::Wasm {
            return false;
        }
        let Some(body) = program.function_get(function as usize) else {
            return false;
        };
        let legacy_candidate = super::js_compiled::function_candidate(body, program);
        let state = self.programs.js_tier(id, function);
        if matches!(state, Some(super::program_store::JsTierState::Unsupported)) {
            return self.programs.js_jit_diagnostics_enabled();
        }
        // Keep bounded candidates on a frame entry until the shared tier has
        // settled. Lane target caches otherwise preserve a frameless route
        // across the later promotion and no longer consult tiering.
        legacy_candidate || js_general_dispatch_candidate(program, body)
    }

    /// Whether a lane-entered frame must hand control to the general frame
    /// loop so a pending or installed generic native region can run there.
    pub(super) fn js_tier_needs_general_entry(&self, id: ProgramId, function: u32) -> bool {
        match self.programs.js_tier(id, function) {
            Some(super::program_store::JsTierState::GeneralPromotionPending) => true,
            Some(super::program_store::JsTierState::Compiled(compiled)) => {
                compiled.is_general_dispatch()
            }
            _ => false,
        }
    }

    /// The JavaScript tier at the entry of a frameless lane activation of `function`, as
    /// `js_tier_frame_entry` is at a frame's: with the native result, the PC of the
    /// instruction the activation continues at, its register and the value to write there;
    /// else `None`, and the activation runs from its entry. `parameters` are the
    /// activation's parameter registers, `this` its receiver where the function binds its
    /// own, and `env` the closure's environment, which its captures walk.
    pub(super) fn js_tier_activation_entry(
        &mut self,
        program: &ResidualProgram,
        id: ProgramId,
        function: u32,
        parameters: &[Value],
        this: Option<Value>,
        env: Value,
    ) -> Option<(usize, crate::bytecode::Register, Value)> {
        // A compiled body that reads only its parameters runs as a direct call does
        // (`js_jit_compiled_call`); the activation completes at its `Return` with the result.
        if let Some(value) = self.js_jit_compiled_call(program, (id, function), false, parameters) {
            let body = program.function_get(function as usize)?;
            let last_pc = body.code.len().checked_sub(1)?;
            let terminal = body.code.get(last_pc)?.as_wide();
            if terminal.op() == Op::Return {
                return Some((last_pc, terminal.register_a(), value));
            }
        }
        let entry = TierEntry {
            program: id,
            function,
            this,
            parameters: parameters.iter().copied().collect(),
            captures: TierCaptures::Environment(env),
            allocates: false,
        };
        let entered = self.js_jit_at_entry(program, &entry);
        if entered.is_none() {
            self.programs.record_js_jit_fallback_entry(id, function);
        }
        entered
    }

    /// The capture at `depth` and `slot` of the activation an entry reads.
    fn tier_capture(&mut self, captures: TierCaptures, depth: u16, slot: u16) -> Option<Value> {
        match captures {
            TierCaptures::Frame(frame) => self.cached_direct_capture(frame, depth, slot),
            TierCaptures::Environment(env) => {
                // A frameless activation's function has no environment of its own
                // (`lane_load_capture_walk`).
                let (slots, root) = self.walked_capture_slots(env, false, depth)?;
                // SAFETY: as `Vm::walked_capture_slots`; nothing collects before the read.
                self.owned_capture(unsafe { &*slots }, root, slot)
            }
        }
    }

    /// `tier_capture`, and for a frame the root bindings `jit_owned_capture` also reads.
    fn tier_owned_capture(&mut self, captures: TierCaptures, depth: u16, slot: u16) -> Option<Value> {
        match captures {
            TierCaptures::Frame(frame) => self.jit_owned_capture(frame, depth, slot),
            TierCaptures::Environment(_) => self.tier_capture(captures, depth, slot),
        }
    }

    /// The JavaScript tier at the entry of `frame`, an initialized frame at PC zero, made
    /// wherever a frame is entered: the general loop, and the lane's in-place frame calls
    /// (`lane_frame_call`). With the native result written to its return register, the PC
    /// of the `Return` that completes the frame; else `None`, the entry counted as an
    /// interpreter fallback, and the untouched frame runs from its entry.
    pub(super) fn js_tier_frame_entry(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        allow_inline_calls: bool,
    ) -> JsTierFrameEntry {
        if self.execution_mode != ExecutionMode::Compiled {
            return JsTierFrameEntry::Interpreter;
        }
        if self.frames[frame].js_jit_skip_entry_once {
            self.frames[frame].js_jit_skip_entry_once = false;
            return JsTierFrameEntry::Interpreter;
        }
        if allow_inline_calls
            && let Some(outcome) = self.js_jit_general_dispatch_at_entry(
                program, frame, true, false,
            )
        {
            return match outcome {
                JsNativeDispatchResult::Step(step) => JsTierFrameEntry::NativeStep(step),
                JsNativeDispatchResult::Resume(pc) => JsTierFrameEntry::NativeResume(pc),
            };
        }
        let function = self.frames[frame].function;
        let program_id = self.frames[frame].program;
        let general_candidate = program.function_get(function as usize).is_some_and(|body| {
            if !js_general_dispatch_candidate(program, body) {
                return false;
            }
            if !super::js_compiled::function_candidate(body, program) {
                return true;
            }
            if js_general_dispatch_acyclic_candidate(program, body) {
                return true;
            }
            match self.programs.js_tier(program_id, function) {
                Some(
                    super::program_store::JsTierState::GeneralPromotionPending
                    | super::program_store::JsTierState::Rejected { .. },
                ) => true,
                Some(super::program_store::JsTierState::Compiled(compiled)) => {
                    compiled.is_general_dispatch()
                }
                _ => false,
            }
        });
        let entry = self.frames.get(frame).filter(|record| record.pc == 0).and_then(|record| {
            let params = program.function_get(record.function as usize)?.params;
            Some(TierEntry {
                program: record.program,
                function: record.function,
                this: record.fixed_this.then_some(record.this),
                parameters: record.registers.iter().copied().take(usize::from(params)).collect(),
                captures: TierCaptures::Frame(frame),
                allocates: true,
            })
        });
        let legacy_candidate = program
            .function_get(function as usize)
            .is_some_and(|body| super::js_compiled::function_candidate(body, program));
        let state = self.programs.js_tier(program_id, function);
        let observes_general = general_candidate
            && (!legacy_candidate
                || matches!(
                    state,
                    Some(
                        super::program_store::JsTierState::Rejected { .. }
                            | super::program_store::JsTierState::GeneralPromotionPending
                    )
                ));
        let mut promote_general = observes_general
            && self
                .programs
                .note_js_general_dispatch_entry(program_id, function);
        if let Some((return_pc, return_register, value)) = entry
            .as_ref()
            .and_then(|entry| self.js_jit_at_entry(program, entry))
        {
            // The frame's own register file: a lane frame call reaches here before
            // `active_registers` names the frame (`lane_enter_script_frame`).
            self.frames[frame].registers[usize::from(return_register)] = value;
            return JsTierFrameEntry::ContinueAt(return_pc);
        }
        promote_general |= matches!(
            self.programs.js_tier(program_id, function),
            Some(super::program_store::JsTierState::GeneralPromotionPending)
        );
        if allow_inline_calls && general_candidate {
            if let Some(outcome) = self.js_jit_general_dispatch_at_entry(
                program, frame, true, promote_general,
            ) {
                return match outcome {
                    JsNativeDispatchResult::Step(step) => JsTierFrameEntry::NativeStep(step),
                    JsNativeDispatchResult::Resume(pc) => JsTierFrameEntry::NativeResume(pc),
                };
            }
        }
        let (program_id, function_id) = (self.frames[frame].program, self.frames[frame].function);
        self.programs
            .record_js_jit_fallback_entry(program_id, function_id);
        if program.kind != crate::bytecode::ProgramKind::Wasm
            && self.programs.js_jit_diagnostics_enabled()
            && self.frames[frame].js_jit_interpreter_started.is_none()
        {
            self.frames[frame].js_jit_interpreter_started = Some(std::time::Instant::now());
        }
        JsTierFrameEntry::Interpreter
    }

    fn js_jit_general_dispatch_at_entry(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        allow_inline_calls: bool,
        promote: bool,
    ) -> Option<JsNativeDispatchResult> {
        if !self.js_tier_runs()
            || program.kind == crate::bytecode::ProgramKind::Wasm
            || self.frames.get(frame)?.pc != 0
        {
            return None;
        }
        let program_id = self.frames[frame].program;
        let function_id = self.frames[frame].function;
        let compiled = match self.programs.js_tier(program_id, function_id) {
            Some(super::program_store::JsTierState::Compiled(compiled))
                if compiled.is_general_dispatch() => Some(compiled),
            Some(super::program_store::JsTierState::Compiled(_)
            | super::program_store::JsTierState::Unsupported) => return None,
            _ => {
                let function = program.function_get(function_id as usize)?;
                if !js_general_dispatch_candidate(program, function)
                    || !promote
                    || !self
                    .programs
                    .note_js_general_dispatch_entry(program_id, function_id)
                {
                    return None;
                }
                let compile_started = self
                    .programs
                    .js_jit_diagnostics_enabled()
                    .then(std::time::Instant::now);
                self.host.before_native_compilation();
                let artifact = compile_js_general_dispatch(program, function_id);
                self.host.after_native_compilation();
                let code_bytes = artifact.as_ref().map_or(0, |artifact| artifact.code.len());
                let rejection = artifact.is_none().then_some("general-dispatch-codegen-rejected");
                let compiled = artifact
                    .and_then(CompiledJsFunction::from_artifact)
                    .map(Rc::new);
                let installed = self
                    .programs
                    .install_js_tier(program_id, function_id, compiled.clone());
                if let Some(compile_started) = compile_started {
                    self.programs.record_js_jit_compile(
                        program_id,
                        function_id,
                        u64::try_from(compile_started.elapsed().as_nanos()).unwrap_or(u64::MAX),
                        if installed { code_bytes } else { 0 },
                        installed,
                        if installed { None } else { rejection.or(Some("shared-code-cache-budget-exhausted")) },
                    );
                }
                if !installed {
                    return None;
                }
                compiled
            }
        }?;

        let mut dispatch = JsStepDispatch {
            program_id,
            frame,
            allow_inline_calls,
            native_registers: std::ptr::null_mut(),
            register_count: 0,
            native_locals: std::ptr::null_mut(),
            local_count: 0,
            outcome: None,
        };
        let interrupt = self.execution_interrupt.atomic_flag();
        let interrupt = interrupt as *const std::sync::atomic::AtomicBool;
        let poll_page = self.compiled_poll_page();
        let outcome = compiled.run_general_dispatch(
            self as *mut Self,
            program_id,
            (&mut dispatch as *mut JsStepDispatch).cast(),
            interrupt,
            poll_page,
        )?;
        if self.programs.js_jit_diagnostics_enabled() {
            self.programs
                .record_js_jit_native_entry(program_id, function_id);
        }
        Some(outcome)
    }

    /// Run a narrow numeric linked-entry search using guarded VM field and
    /// array probes, plus the already-promoted numeric equality artifact.
    /// Every failure leaves the frame untouched for interpreter execution.
    fn js_jit_numeric_find_at_entry(
        &mut self,
        program: &ResidualProgram,
        activation: &TierEntry,
        candidate: super::js_compiled::JsJitNumericFind,
    ) -> Option<(usize, crate::bytecode::Register, Value)> {
        const MAX_STEPS: usize = 256;
        let (program_id, function_id) = (activation.program, activation.function);
        let receiver = activation.this?;
        let (key_value, index_value, hash_value) = {
            let arguments = activation.parameters.get(..3)?;
            (arguments[0], arguments[1], arguments[2])
        };
        if key_value.as_number().is_none() || index_value.as_int().is_none() {
            return None;
        }
        let Some(key_hash) = hash_value.as_number() else {
            return None;
        };
        let array = match self.cached_field_hit(program, receiver, candidate.list_cache) {
            super::object::FieldCacheProbe::Hit(value) => value,
            super::object::FieldCacheProbe::Ineligible => {
                return None;
            }
            super::object::FieldCacheProbe::OrdinaryMiss => {
                return None;
            }
        };
        let Some(mut entry) = self.direct_array_index_data(array, index_value) else {
            return None;
        };
        let mut cached_equals_function = None;
        let mut equal_arguments = [Value::UNDEFINED; 2];
        equal_arguments[0] = key_value;

        for step in 0..MAX_STEPS {
            if step & 31 == 0
                && self
                    .execution_interrupt
                    .atomic_flag()
                    .load(std::sync::atomic::Ordering::Relaxed)
            {
                return None;
            }
            if !self.truthy(entry) {
                if self.programs.js_jit_diagnostics_enabled() {
                    self.programs
                        .record_js_jit_entry(program_id, function_id, true, 3);
                    self.programs
                        .record_js_jit_numeric_find_entry(program_id, function_id);
                }
                return Some((candidate.return_pc, candidate.return_register, entry));
            }
            if self.heap.object(entry).is_none() {
                return None;
            }
            let entry_hash = match self.cached_field_hit(program, entry, candidate.hash_cache) {
                super::object::FieldCacheProbe::Hit(value) => {
                    let Some(value) = value.as_number() else {
                        return None;
                    };
                    value
                }
                super::object::FieldCacheProbe::Ineligible => {
                    return None;
                }
                super::object::FieldCacheProbe::OrdinaryMiss => {
                    return None;
                }
            };
            if entry_hash == key_hash {
                let equals_function = if let Some(equals_function) = cached_equals_function {
                    equals_function
                } else {
                    let equals = self.tier_owned_capture(
                        activation.captures,
                        candidate.equals_depth,
                        candidate.equals_slot,
                    )?;
                    let (equals_program, equals_function, _) =
                        self.in_loop_call_target(program, equals)?;
                    if equals_program != program_id {
                        return None;
                    }
                    equals_function
                };
                cached_equals_function = Some(equals_function);
                let entry_key = match self.cached_field_hit(program, entry, candidate.key_cache) {
                    super::object::FieldCacheProbe::Hit(value) => value,
                    super::object::FieldCacheProbe::Ineligible => {
                        return None;
                    }
                    super::object::FieldCacheProbe::OrdinaryMiss => {
                        return None;
                    }
                };
                if entry_key.as_number().is_none() {
                    return None;
                }
                equal_arguments[1] = entry_key;
                let equal =
                    self.js_jit_compiled_call(program, (program_id, equals_function), false, &equal_arguments);
                let Some(equal) = equal else {
                    return None;
                };
                if equal == Value::TRUE {
                    if self.programs.js_jit_diagnostics_enabled() {
                        self.programs
                            .record_js_jit_entry(program_id, function_id, true, 3);
                        self.programs
                            .record_js_jit_numeric_find_entry(program_id, function_id);
                    }
                    return Some((candidate.return_pc, candidate.return_register, entry));
                }
                if equal != Value::FALSE {
                    return None;
                }
            }
            entry = match self.cached_field_hit(program, entry, candidate.next_cache) {
                super::object::FieldCacheProbe::Hit(value) => value,
                super::object::FieldCacheProbe::Ineligible => {
                    return None;
                }
                super::object::FieldCacheProbe::OrdinaryMiss => {
                    return None;
                }
            };
        }
        None
    }

    /// Try the per-function JavaScript tier at an already-initialized frame's
    /// entry. A guard miss leaves the frame untouched so the interpreter can
    /// run from the same bytecode PC.
    pub(super) fn js_jit_at_entry(
        &mut self,
        program: &ResidualProgram,
        entry: &TierEntry,
    ) -> Option<(usize, crate::bytecode::Register, Value)> {
        if !self.js_tier_runs() || program.kind == crate::bytecode::ProgramKind::Wasm {
            return None;
        }
        let (program_id, function_id) = (entry.program, entry.function);
        let function = program.function_get(function_id as usize)?;
        let facts = self.programs.js_entry_facts(program_id, function_id)?;
        // Most entries are of functions no entry path can claim; diagnostics still count them.
        if !facts.enters && !self.programs.js_jit_diagnostics_enabled() {
            return None;
        }
        let numeric_find_candidate = facts.numeric_find.filter(|_| jit_numeric_find_enabled());
        if let Some(candidate) = numeric_find_candidate
            && let Some(result) = self.js_jit_numeric_find_at_entry(program, entry, candidate)
        {
            return Some(result);
        }
        let field_increment_candidate = facts.field_increment;
        let mut field_increment_diagnostic_entry_recorded = false;
        if let Some(candidate) = field_increment_candidate {
            let fixed_this = entry.this.is_some();
            if self.programs.js_jit_diagnostics_enabled() {
                self.programs.record_js_jit_entry(
                    program_id,
                    function_id,
                    false,
                    function.params,
                );
                self.programs.record_js_jit_field_increment_candidate(
                    program_id,
                    function_id,
                    fixed_this,
                );
                field_increment_diagnostic_entry_recorded = true;
            }
            if let Some(receiver) = entry.this
                && self.cached_field_increment_hit(
                    program,
                    receiver,
                    candidate.atom,
                    candidate.read_cache_site,
                    candidate.write_cache_site,
                    candidate.decrement,
                )
            {
                if self.programs.js_jit_diagnostics_enabled() {
                    self.programs
                        .record_js_jit_field_increment_entry(program_id, function_id);
                }
                return Some((
                    candidate.return_pc,
                    candidate.return_register,
                    Value::UNDEFINED,
                ));
            }
        }
        let field_return_candidate =
            facts.field_return;
        let mut field_return_diagnostic_entry_recorded = field_increment_diagnostic_entry_recorded;
        if let Some(candidate) = field_return_candidate {
            let fixed_this = entry.this.is_some();
            if self.programs.js_jit_diagnostics_enabled() {
                self.programs.record_js_jit_entry(
                    program_id,
                    function_id,
                    false,
                    function.params,
                );
                self.programs.record_js_jit_field_return_candidate(
                    program_id,
                    function_id,
                    fixed_this,
                );
                field_return_diagnostic_entry_recorded = true;
            }
            if let Some(receiver) = entry.this {
                if let super::object::FieldCacheProbe::Hit(value) =
                    self.cached_field_hit(program, receiver, candidate.cache_site)
                {
                    if self.programs.js_jit_diagnostics_enabled() {
                        self.programs
                            .record_js_jit_field_return_entry(program_id, function_id);
                    }
                    return Some((candidate.return_pc, candidate.return_register, value));
                }
            }
        }
        let tier = self.programs.js_tier(program_id, function_id);
        let rejected_entry = if self.programs.js_jit_diagnostics_enabled()
            && matches!(
                tier.as_ref(),
                Some(super::program_store::JsTierState::Rejected { .. })
            ) {
            self.programs
                .note_js_rejected_entry(program_id, function_id)
        } else {
            None
        };
        if matches!(
            tier.as_ref(),
            Some(
                super::program_store::JsTierState::Unsupported
                    | super::program_store::JsTierState::Rejected { .. }
            )
        ) && (!self.programs.js_jit_diagnostics_enabled() || rejected_entry.is_none())
        {
            return None;
        }
        if tier.is_none() && !super::js_compiled::function_candidate(function, program) {
            return None;
        }
        let compiled_tier = match tier.as_ref() {
            Some(super::program_store::JsTierState::Compiled(compiled)) => Some(compiled.as_ref()),
            _ => None,
        };
        let numeric_loop_candidate =
            compiled_tier.is_none() && facts.numeric_loop;
        let int32_bitwise_candidate =
            facts.int32_bitwise;
        let int32_identity_candidate =
            facts.int32_identity;
        let capture_argument = match compiled_tier {
            Some(compiled) => compiled.capture_argument(),
            None => facts.number_capture,
        };
        let mut capture_arguments: smallvec::SmallVec<
            [super::js_compiled::JsJitCaptureArgument; 4],
        > = smallvec::SmallVec::new();
        if let Some(compiled) = compiled_tier {
            capture_arguments.extend_from_slice(compiled.capture_arguments());
        } else if let Some(candidate) = facts.numeric_capture.clone() {
            capture_arguments.extend(candidate);
        }
        let capture_number = capture_argument.and_then(|capture| {
            self.tier_capture(entry.captures, capture.depth, capture.slot)
                .and_then(Value::as_number)
        });

        let params = usize::from(function.params);
        let registers = entry.parameters.get(..params)?.to_vec();
        let first_argument = registers.first().copied();
        let mut numeric_field_inputs: smallvec::SmallVec<
            [super::js_compiled::JsJitNumericFieldInput; 4],
        > = smallvec::SmallVec::new();
        if let Some(compiled) = compiled_tier {
            numeric_field_inputs.extend_from_slice(compiled.numeric_field_inputs());
        } else if let Some(inputs) = &facts.numeric_field_inputs {
            numeric_field_inputs.extend_from_slice(inputs);
        }
        let is_field_receiver_parameter = |parameter: usize| {
            numeric_field_inputs.iter().any(|input| {
                input.receiver
                    == super::js_compiled::JsJitFieldReceiver::Parameter(parameter as u16)
            })
        };
        let numeric_field_argument = match compiled_tier {
            Some(compiled) if compiled.uses_numeric_field_argument() => compiled.field_argument(),
            Some(_) => None,
            None => facts.numeric_field_argument,
        };
        let field_argument_uses_numeric_abi = numeric_field_argument.is_some();
        let field_argument = numeric_field_argument.or_else(|| match compiled_tier {
            Some(compiled) => compiled.field_argument(),
            None if numeric_field_inputs.is_empty() => facts.field_argument,
            None => None,
        });
        let field_reject_candidate = match compiled_tier {
            Some(compiled) => compiled.is_field_reject(),
            None => facts.field_reject,
        };
        let mut arguments: smallvec::SmallVec<[f64; 8]> = smallvec::SmallVec::new();
        let mut numeric = true;
        for (index, value) in registers.iter().copied().enumerate() {
            if is_field_receiver_parameter(index) {
                arguments.push(0.0);
                continue;
            }
            match value.as_number() {
                Some(value) => arguments.push(value),
                None => {
                    numeric = false;
                    break;
                }
            }
        }
        let mut capture_values: smallvec::SmallVec<[f64; 4]> = smallvec::SmallVec::new();
        let captures_numeric = capture_arguments.iter().all(|capture| {
            self.tier_capture(entry.captures, capture.depth, capture.slot)
                .and_then(Value::as_number)
                .is_some_and(|value| {
                    capture_values.push(value);
                    true
                })
        });
        if captures_numeric {
            arguments.extend(capture_values.iter().copied());
        } else {
            numeric = false;
        }
        if !numeric_field_inputs.is_empty() {
            for input in numeric_field_inputs.iter().copied() {
                let receiver = match input.receiver {
                    super::js_compiled::JsJitFieldReceiver::This => {
                        let Some(receiver) = entry.this else {
                            numeric = false;
                            break;
                        };
                        receiver
                    }
                    super::js_compiled::JsJitFieldReceiver::Parameter(parameter) => {
                        let Some(receiver) = registers.get(usize::from(parameter)).copied() else {
                            numeric = false;
                            break;
                        };
                        receiver
                    }
                    super::js_compiled::JsJitFieldReceiver::Capture { capture, .. } => {
                        let Some(receiver) =
                            self.tier_capture(entry.captures, capture.depth, capture.slot)
                        else {
                            numeric = false;
                            break;
                        };
                        receiver
                    }
                };
                match self.cached_field_hit(program, receiver, input.cache_site) {
                    super::object::FieldCacheProbe::Hit(value) => {
                        if let Some(value) = value.as_number() {
                            arguments.push(value);
                        } else {
                            numeric = false;
                            break;
                        }
                    }
                    super::object::FieldCacheProbe::OrdinaryMiss
                    | super::object::FieldCacheProbe::Ineligible => {
                        numeric = false;
                        break;
                    }
                }
            }
        }
        if let Some(field_argument) = field_argument {
            let receiver = first_argument?;
            arguments.clear();
            if !field_argument_uses_numeric_abi {
                numeric = false;
                if let super::object::FieldCacheProbe::Hit(value) =
                    self.cached_field_hit(program, receiver, field_argument.cache_site)
                {
                    if let Some(value) = value.as_number() {
                        arguments.push(value);
                        numeric = true;
                    } else if field_reject_candidate {
                        // Strict inequality against a Number is true for every
                        // non-Number value, so NaN takes the guarded early return.
                        arguments.push(f64::NAN);
                    }
                } else {
                    arguments.clear();
                }
            } else {
                numeric = true;
                // Preserve numeric arguments after the receiver. The compiler
                // keeps the object's ABI slot as a placeholder, then appends
                // the guarded field value after parameters and captures.
                for (index, value) in registers.iter().copied().enumerate() {
                    if index == 0 {
                        arguments.push(0.0);
                    } else if let Some(value) = value.as_number() {
                        arguments.push(value);
                    } else {
                        numeric = false;
                        break;
                    }
                }
                if captures_numeric {
                    arguments.extend(capture_values.iter().copied());
                } else {
                    numeric = false;
                }
                if let super::object::FieldCacheProbe::Hit(value) =
                    self.cached_field_hit(program, receiver, field_argument.cache_site)
                {
                    if let Some(number) = value.as_number() {
                        arguments.push(number);
                    } else {
                        numeric = false;
                    }
                } else {
                    numeric = false;
                }
            }
        }
        if numeric && int32_bitwise_candidate {
            numeric = arguments
                .iter()
                .all(|value| super::js_compiled::is_i32_number(*value));
        }
        let string_hash_candidate = match compiled_tier {
            Some(compiled) => compiled.is_string_hash_loop(),
            None => facts.string_hash_loop,
        } && params == 1
            && self.js_jit_string_hash_guard()
            && first_argument.is_some_and(|value| self.heap.is_string(value));
        let string_hash_units = if string_hash_candidate {
            let Some(string) = first_argument.and_then(|value| self.heap.string(value))
            else {
                unreachable!("guarded JIT string argument changed")
            };
            Some(string.units())
        } else {
            None
        };
        let string_transform_kind = match compiled_tier {
            Some(compiled) => compiled.string_transform_kind(),
            None => facts.string_transform,
        };
        // Its result allocates a string, which no lane activation may (`TierEntry::allocates`).
        let string_transform = string_transform_kind
            .filter(|_| entry.allocates)
            .filter(|kind| self.js_jit_string_transform_guard(*kind))
            .filter(|_| {
                first_argument.is_some_and(|value| self.heap.is_string(value))
            });
        let string_transform_units = string_transform.and_then(|_| {
            let Some(string) = first_argument.and_then(|value| self.heap.string(value))
            else {
                return None;
            };
            Some(string.units())
        });
        if let Some((entries, reason)) = rejected_entry {
            if !field_return_diagnostic_entry_recorded {
                self.programs
                    .record_js_jit_entry(program_id, function_id, numeric, function.params);
            }
            self.programs
                .record_js_jit_rejection(program_id, function_id, reason, entries);
            return None;
        }
        if tier.is_none()
            && capture_argument.is_none()
            && !super::js_compiled::function_candidate(function, program)
            && !int32_identity_candidate
        {
            let reason = super::js_compiled::candidate_rejection_reason(program, function_id);
            let report_entries = self
                .programs
                .reject_js_function(program_id, function_id, reason);
            if let Some(entries) = report_entries
                && self.programs.js_jit_diagnostics_enabled()
            {
                if !field_return_diagnostic_entry_recorded {
                    self.programs.record_js_jit_entry(
                        program_id,
                        function_id,
                        numeric,
                        function.params,
                    );
                }
                self.programs
                    .record_js_jit_rejection(program_id, function_id, reason, entries);
            }
            return None;
        }
        if tier.is_none()
            && !function.wide.is_empty()
            && !numeric
            && !(field_reject_candidate && arguments.len() == 1)
        {
            return None;
        }
        if self.programs.js_jit_diagnostics_enabled()
            && !field_return_diagnostic_entry_recorded
        {
            self.programs
                .record_js_jit_entry(program_id, function_id, numeric, function.params);
        }

        let compiled = match tier {
            Some(super::program_store::JsTierState::Compiled(compiled)) => {
                if (!numeric && !compiled.is_string_hash_loop() && !compiled.is_string_transform())
                    && !(compiled.is_field_reject() && arguments.len() == 1)
                    || (compiled.is_string_hash_loop() && string_hash_units.is_none())
                    || (compiled.is_string_transform() && string_transform_units.is_none())
                    || (compiled.is_capture_number() && capture_number.is_none())
                {
                    return None;
                }
                compiled
            }
            Some(
                super::program_store::JsTierState::Unsupported
                | super::program_store::JsTierState::Rejected { .. }
                | super::program_store::JsTierState::GeneralPromotionPending,
            ) => return None,
            Some(super::program_store::JsTierState::Hot { .. })
            | Some(super::program_store::JsTierState::PromotionPending)
            | None => {
                if !self.programs.note_js_function_entry(
                    program_id,
                    function_id,
                    numeric,
                    (numeric
                        && (numeric_loop_candidate
                            || field_argument.is_some()
                            || !numeric_field_inputs.is_empty()))
                        || int32_identity_candidate
                        || (field_reject_candidate && arguments.len() == 1)
                        || capture_number.is_some()
                        || (!capture_arguments.is_empty() && captures_numeric)
                        || string_hash_units.is_some()
                        || string_transform_units.is_some(),
                    js_general_dispatch_acyclic_candidate(program, function),
                ) || (!numeric
                    && !(field_reject_candidate && arguments.len() == 1)
                    && string_hash_units.is_none()
                    && string_transform_units.is_none())
                {
                    return None;
                }
                let residual = self.programs.get(program_id)?;
                let compile_started = self
                    .programs
                    .js_jit_diagnostics_enabled()
                    .then(std::time::Instant::now);
                self.host.before_native_compilation();
                let artifact = super::js_compiled::compile(&residual, function_id);
                self.host.after_native_compilation();
                let artifact_failed = artifact.is_none();
                let compile_rejection = artifact_failed.then(|| {
                    super::js_compiled::candidate_rejection_reason(&residual, function_id)
                });
                let compiled = artifact
                    .and_then(CompiledJsFunction::from_artifact)
                    .map(Rc::new);
                let compile_rejection = if artifact_failed {
                    compile_rejection
                } else if compiled.is_none() {
                    Some("executable-memory-allocation-failed")
                } else {
                    None
                };
                let installed =
                    self.programs
                        .install_js_tier(program_id, function_id, compiled.clone());
                if let Some(compile_started) = compile_started {
                    let code_bytes = if installed {
                        compiled
                            .as_ref()
                            .map_or(0, |compiled| compiled.mapped_bytes())
                    } else {
                        0
                    };
                    self.programs.record_js_jit_compile(
                        program_id,
                        function_id,
                        u64::try_from(compile_started.elapsed().as_nanos()).unwrap_or(u64::MAX),
                        code_bytes,
                        installed,
                        if !installed && compiled.is_some() {
                            Some("shared-code-cache-budget-exhausted")
                        } else {
                            compile_rejection
                        },
                    );
                }
                if !installed {
                    return None;
                }
                compiled?
            }
        };

        if compiled.is_int32_identity()
            && !arguments
                .first()
                .is_some_and(|value| super::js_compiled::is_i32_number(*value))
        {
            return None;
        }

        if compiled.params() != params {
            return None;
        }

        if self.programs.js_jit_diagnostics_enabled() {
            self.programs
                .record_js_jit_native_entry(program_id, function_id);
        }
        let poll_page = self.compiled_poll_page();
        let result = if compiled.is_capture_number() {
            compiled.call_capture_number(capture_number?, poll_page)?
        } else if compiled.is_string_hash_loop() {
            compiled.call_string_hash_loop(
                string_hash_units?,
                self.execution_interrupt.atomic_flag(),
                poll_page,
            )?
        } else if compiled.is_string_transform() {
            compiled.call_string_transform(string_transform_units?, poll_page)?
        } else {
            compiled.call(
                &arguments,
                self.execution_interrupt.atomic_flag(),
                poll_page,
            )?
        };
        match result {
            CompiledJsResult::Number(value) => {
                let last_pc = function.code.len().checked_sub(1)?;
                let terminal = function.code.get(last_pc)?.as_wide();
                if terminal.op() != Op::Return {
                    return None;
                }
                Some((last_pc, terminal.register_a(), Value::number(value)))
            }
            CompiledJsResult::Int32Identity { value, return_pc } => {
                let return_pc = usize::try_from(return_pc).ok()?;
                let instruction = function.code.get(return_pc)?.as_wide();
                if instruction.op() != Op::Return {
                    return None;
                }
                Some((return_pc, instruction.register_a(), Value::number(value)))
            }
            CompiledJsResult::StringReturnPc(return_pc) => {
                let return_pc = usize::try_from(return_pc).ok()?;
                let instruction = function.code.get(return_pc)?.as_wide();
                if instruction.op() != Op::Return {
                    return None;
                }
                let constant = compiled.string_constant_for_return(return_pc as u32)?;
                let value = self.programs.constant(program_id, constant as usize)?;
                Some((return_pc, instruction.register_a(), value))
            }
            CompiledJsResult::Boolean(value) => {
                let last_pc = function.code.len().checked_sub(1)?;
                let terminal = function.code.get(last_pc)?.as_wide();
                if terminal.op() != Op::Return {
                    return None;
                }
                Some((
                    last_pc,
                    terminal.register_a(),
                    if value { Value::TRUE } else { Value::FALSE },
                ))
            }
            CompiledJsResult::FieldReject {
                fast_path,
                resume_pc,
                resume_register,
                resume_value,
            } => fast_path.then_some((resume_pc, resume_register, resume_value)),
            CompiledJsResult::StringTransform(transform, matched) => {
                let last_pc = function.code.len().checked_sub(1)?;
                let terminal = function.code.get(last_pc)?.as_wide();
                if terminal.op() != Op::Return {
                    return None;
                }
                let value = if !matched {
                    first_argument?
                } else {
                    let Some(string) = first_argument
                        .and_then(|value| self.heap.string(value))
                        .cloned()
                    else {
                        return None;
                    };
                    let units = string.units();
                    match transform {
                        super::js_compiled::JsJitStringTransform::PrefixUnderscore => {
                            let mut output = Vec::with_capacity(units.len() + 1);
                            output.push(u16::from(b'_'));
                            output.extend(units.iter());
                            self.string_from_units(&output).ok()?
                        }
                        super::js_compiled::JsJitStringTransform::RemoveLeadingUnderscore => self
                            .string_substring(&string, 1..units.len())
                            .ok()?,
                    }
                };
                Some((last_pc, terminal.register_a(), value))
            }
        }
    }

    fn js_jit_string_hash_guard(&self) -> bool {
        let Some(atom) = self.lookup_atom("charCodeAt") else {
            return false;
        };
        let Some(method) = self.own_property(self.string_proto, atom) else {
            return false;
        };
        self.is_current_realm_native(method, Native::StringCharCodeAt)
    }

    fn js_jit_string_transform_guard(
        &self,
        transform: super::js_compiled::JsJitStringTransform,
    ) -> bool {
        if !self.js_jit_string_hash_guard() {
            return false;
        }
        if transform == super::js_compiled::JsJitStringTransform::PrefixUnderscore {
            return true;
        }
        let Some(atom) = self.lookup_atom("substr") else {
            return false;
        };
        self.own_property(self.string_proto, atom)
            .is_some_and(|method| self.is_current_realm_native(method, Native::StringSubstr))
    }

    /// The JavaScript tier's direct entry of a plain call of a same-program user function,
    /// before any callee frame is pushed: the general loop's in-loop and known calls and the
    /// lane's frame calls (`lane_frame_call`) make it. Only an already-promoted function
    /// enters; its native subset is pure and has a guarded Number ABI. A call that returns
    /// its callee's completion (`returns`) completes its caller, so it and every guard miss
    /// stay on the ordinary frame path.
    pub(super) fn js_jit_compiled_call(
        &mut self,
        program: &ResidualProgram,
        (program_id, function_id): (ProgramId, u32),
        returns: bool,
        arguments: &[Value],
    ) -> Option<Value> {
        if returns
            || !self.js_tier_runs()
            || program_id != self.active_program
            || program.kind == crate::bytecode::ProgramKind::Wasm
        {
            return None;
        }
        let (programs, interrupt) = (&mut self.programs, &self.execution_interrupt);
        let function = program.function_get(function_id as usize)?;
        let diagnostics = programs.js_jit_diagnostics_enabled();
        let native_result = {
            let compiled = match programs.js_tier_ref(program_id, function_id)? {
                super::program_store::JsTierState::Compiled(compiled) => compiled.as_ref(),
                _ => return None,
            };
            let params = usize::from(function.params);
            if !compiled.direct_callable() || compiled.params() != params || arguments.len() < params {
                return None;
            }
            direct_call(compiled, &arguments[..params], interrupt)
        };
        if diagnostics {
            programs.record_js_jit_entry(program_id, function_id, true, function.params);
            programs.record_js_jit_native_entry(program_id, function_id);
        }
        match native_result? {
            CompiledJsCallResult::Value(value) => Some(value),
            CompiledJsCallResult::ReturnConstant(constant) => {
                programs.constant(program_id, constant as usize)
            }
        }
    }

    /// The direct run of `function`'s compiled body a frameless activation keeps
    /// (`LaneView::tier_direct`): where its body reads only its parameters
    /// (`CompiledJsFunction::direct_callable`) and ends in a `Return`, which its result
    /// completes. Diagnostics count every entry through `js_jit_compiled_call` instead.
    pub(super) fn direct_tier_entry(
        &self,
        program: &ResidualProgram,
        id: ProgramId,
        function: u32,
    ) -> Option<DirectTierEntry> {
        if self.programs.js_jit_diagnostics_enabled() {
            return None;
        }
        let super::program_store::JsTierState::Compiled(compiled) = self.programs.js_tier_ref(id, function)? else {
            return None;
        };
        let body = program.function_get(function as usize)?;
        let params = usize::from(body.params);
        (compiled.direct_callable() && compiled.params() == params).then_some(())?;
        let return_pc = body.code.len().checked_sub(1)?;
        let terminal = body.code.get(return_pc)?.as_wide();
        (terminal.op() == Op::Return).then_some(())?;
        Some(DirectTierEntry {
            compiled: Rc::downgrade(compiled),
            params,
            return_pc,
            return_register: terminal.register_a(),
        })
    }

    /// Runs `entry` on a frameless activation's `parameters`, as `js_jit_compiled_call` runs
    /// the body: the `Return` to continue at, its register and the result; `None` where an
    /// argument is no Number the body's guards admit, or the tier retired the artifact.
    pub(super) fn run_direct_tier_entry(
        &mut self,
        entry: &DirectTierEntry,
        id: ProgramId,
        parameters: &[Value],
    ) -> Option<(usize, crate::bytecode::Register, Value)> {
        let compiled = entry.compiled.upgrade()?;
        let result = direct_call(&compiled, parameters.get(..entry.params)?, &self.execution_interrupt)?;
        let value = match result {
            CompiledJsCallResult::Value(value) => value,
            CompiledJsCallResult::ReturnConstant(constant) => self.programs.constant(id, constant as usize)?,
        };
        Some((entry.return_pc, entry.return_register, value))
    }

    /// Compile every function of `program` that has a compiled denotation,
    /// with `env` as the instance whose bindings fix global types and memory.
    fn compile_program(&self, program: ProgramId, env: Value) -> Option<CompiledProgram> {
        self.compile_program_with_diagnostics(program, env, jit_diagnostics_enabled())
    }

    fn compile_program_with_diagnostics(
        &self,
        program: ProgramId,
        env: Value,
        diagnostics_enabled: bool,
    ) -> Option<CompiledProgram> {
        let root = jit_root_override().ok()?;
        self.compile_program_with_diagnostics_and_root(program, env, diagnostics_enabled, root)
    }

    fn compile_program_with_diagnostics_and_root(
        &self,
        program: ProgramId,
        env: Value,
        diagnostics_enabled: bool,
        root: Option<usize>,
    ) -> Option<CompiledProgram> {
        self.compile_program_with_diagnostics_and_root_mode(
            program,
            env,
            diagnostics_enabled,
            root,
            false,
        )
    }

    /// Compile one explicitly selected Wasm function without its static
    /// `CallKnown` closure. Unselected callees use the existing VM callout
    /// path. This is an opt-in diagnostic tier; normal root compilation keeps
    /// its current closure behavior.
    fn compile_program_with_diagnostics_and_root_mode(
        &self,
        program: ProgramId,
        env: Value,
        diagnostics_enabled: bool,
        root: Option<usize>,
        root_only: bool,
    ) -> Option<CompiledProgram> {
        let code = self.programs.get(program)?;
        if code.kind != crate::bytecode::ProgramKind::Wasm {
            return None;
        }
        if root_only && root.is_none() {
            return None;
        }
        let compile_started = diagnostics_enabled.then(std::time::Instant::now);
        let compile_rejections =
            diagnostics_enabled.then(|| std::cell::RefCell::new(vec![None; code.functions.len()]));
        let count = code.functions.len();
        let allowed = if let Some(root) = root.filter(|_| root_only) {
            let mut selected = vec![false; count];
            let Some(function) = code.function_get(root) else {
                if diagnostics_enabled {
                    eprintln!("QUENCH_WASM_JIT_ROOT_REJECTION root={root} invalid-function={root}");
                }
                return None;
            };
            if function.code.len() > MAX_JIT_ROOT_BYTECODES_PER_FUNCTION {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={root} function={root} bytecodes={} limit={MAX_JIT_ROOT_BYTECODES_PER_FUNCTION}",
                        function.code.len()
                    );
                }
                return None;
            }
            selected[root] = true;
            selected
        } else if let Some(root) = root {
            let mut selected = vec![false; count];
            let mut pending = vec![root];
            while let Some(index) = pending.pop() {
                let Some(selected_function) = selected.get_mut(index) else {
                    if diagnostics_enabled {
                        eprintln!(
                            "QUENCH_WASM_JIT_ROOT_REJECTION root={root} invalid-function={index}"
                        );
                    }
                    return None;
                };
                if *selected_function {
                    continue;
                }
                let function = code.function_get(index)?;
                if function.code.len() > MAX_JIT_ROOT_BYTECODES_PER_FUNCTION {
                    if diagnostics_enabled {
                        eprintln!(
                            "QUENCH_WASM_JIT_ROOT_REJECTION root={root} function={index} bytecodes={} limit={MAX_JIT_ROOT_BYTECODES_PER_FUNCTION}",
                            function.code.len()
                        );
                    }
                    return None;
                }
                *selected_function = true;
                for packed in &function.code {
                    let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
                    if instruction.op() == Op::CallKnown {
                        pending.push(usize::from(instruction.known_function_index()));
                    }
                }
            }
            // A selective tier normally follows only statically known calls.
            // Before this promotion, however, the eager artifact has already
            // observed hot `call_indirect` targets. Add a bounded set of those
            // callees from call sites inside this root's static closure; the
            // generated call still keeps the dynamic resolver as its miss
            // path.
            if let Some(baseline) = self.programs.compiled(program).flatten() {
                let profiled_targets =
                    baseline.profiled_indirect_targets(self, program, env, &selected);
                for target in profiled_targets {
                    if target >= count || selected[target] {
                        continue;
                    }
                    if diagnostics_enabled {
                        eprintln!(
                            "QUENCH_WASM_JIT_PROFILED_INDIRECT_TARGET root={root} function={target}"
                        );
                    }
                    pending.push(target);
                }
                while let Some(index) = pending.pop() {
                    let Some(selected_function) = selected.get_mut(index) else {
                        if diagnostics_enabled {
                            eprintln!(
                                "QUENCH_WASM_JIT_ROOT_REJECTION root={root} invalid-function={index}"
                            );
                        }
                        return None;
                    };
                    if *selected_function {
                        continue;
                    }
                    let function = code.function_get(index)?;
                    if function.code.len() > MAX_JIT_ROOT_BYTECODES_PER_FUNCTION {
                        if diagnostics_enabled {
                            eprintln!(
                                "QUENCH_WASM_JIT_ROOT_REJECTION root={root} function={index} bytecodes={} limit={MAX_JIT_ROOT_BYTECODES_PER_FUNCTION}",
                                function.code.len()
                            );
                        }
                        return None;
                    }
                    *selected_function = true;
                    for packed in &function.code {
                        let instruction =
                            super::dispatch_fast::decoded_instruction(function, *packed);
                        if instruction.op() == Op::CallKnown {
                            pending.push(usize::from(instruction.known_function_index()));
                        }
                    }
                }
            }
            let selected_count = selected.iter().filter(|selected| **selected).count();
            if selected_count > MAX_JIT_ROOT_FUNCTIONS {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={root} functions={selected_count} limit={MAX_JIT_ROOT_FUNCTIONS}"
                    );
                }
                return None;
            }
            selected
        } else {
            vec![true; count]
        };
        let mut analysis = Analysis {
            vm: self,
            program,
            code: &code,
            env,
            callouts: true,
            experimental_root_compilation: root.is_some(),
            root_only,
            compile_rejections: compile_rejections.as_ref(),
        };
        // The callable set is the greatest fixpoint: assume every function
        // compiles, drop those that cannot, until nothing changes.
        let mut callable;
        let mut plans: Vec<Option<Plan>>;
        let mut memory_slot;
        let mut guarded;
        loop {
            callable = allowed.clone();
            loop {
                memory_slot = None;
                plans = (0..count)
                    .map(|index| {
                        if callable[index] {
                            analysis
                                .plan(index, &callable, &mut memory_slot)
                                .map(Plan::interface)
                        } else {
                            None
                        }
                    })
                    .collect();
                let next: Vec<bool> = plans.iter().map(Option::is_some).collect();
                if next == callable {
                    break;
                }
                callable = next;
            }
            // Accesses may rely on the guard when the memory has one and
            // this host resumes guard faults.
            guarded = guard_trampoline().is_some()
                && memory_slot.is_some_and(|slot| {
                    self.heap
                        .environment_slot(env, slot as usize)
                        .and_then(|binding| guarded_memory(self, binding))
                        .is_some()
                });
            // Callouts need a memory whose bytes stay put.
            if !analysis.callouts || guarded || memory_slot.is_none() {
                break;
            }
            analysis.callouts = false;
        }
        if plans.iter().all(Option::is_none) {
            return None;
        }
        if root.is_some_and(|root| plans.get(root).is_none_or(Option::is_none)) {
            return None;
        }
        let fixed_base = guarded
            .then(|| {
                let binding = self.heap.environment_slot(env, memory_slot? as usize)?;
                displacement_base(guarded_memory(self, binding)?.0)
            })
            .flatten();
        let opt_level = jit_root_opt_level(root, root_only).ok()?;
        if diagnostics_enabled {
            eprintln!("QUENCH_WASM_JIT_OPT_LEVEL {opt_level}");
        }
        let isa = super::jit_backend::target_isa(opt_level, false)?;
        // The full plan of a callable function, derived again where needed.
        let plan = |index: usize| analysis.plan(index, &callable, &mut memory_slot.clone());
        let mut reference_site_ids = vec![Vec::new(); count];
        let mut reference_call_sites = Vec::new();
        let mut reference_cache_sites = Vec::new();
        let mut reference_cache_count = 0usize;
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            let full_plan = plan(index)?;
            let function = &code.function(index);
            for (pc, packed) in function.code.iter().enumerate() {
                let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
                if instruction.op() == Op::Call
                    && let Slot::IndirectReference { table, .. } = full_plan.operand(pc)
                {
                    let site_id = reference_cache_count;
                    reference_cache_count += 1;
                    reference_site_ids[index].push((pc, site_id));
                    reference_cache_sites.push((index, pc, table));
                    if diagnostics_enabled {
                        reference_call_sites.push(ReferenceCallSiteDiagnostics {
                            function: index,
                            pc,
                            counts: std::cell::Cell::new(ReferenceCallSiteCounts::default()),
                            targets: std::cell::RefCell::new(Vec::new()),
                        });
                    }
                }
            }
        }
        // The globals compiled code accesses, each once.
        let mut globals: Vec<u32> = Vec::new();
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            let plan = plan(index)?;
            let function = &code.function(index);
            for (pc, packed) in function.code.iter().enumerate() {
                let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
                if !matches!(instruction.op(), Op::WasmGlobalGet | Op::WasmGlobalSet) {
                    continue;
                }
                if let Some(Slot::Binding(slot)) = plan.operands[pc]
                    && !globals.contains(&slot)
                {
                    globals.push(slot);
                }
            }
        }
        let mut bytes = Vec::new();
        let mut offsets = vec![None; count];
        let mut spans = Vec::new();
        let mut entries = vec![None; count];
        let mut calls = Vec::new();
        let place = |bytes: &mut Vec<u8>, code: Vec<u8>| {
            bytes.resize(bytes.len().next_multiple_of(FUNCTION_ALIGNMENT), 0);
            let offset = bytes.len();
            bytes.extend_from_slice(&code);
            offset
        };
        let projected_code_len = |current: usize, appended: usize| {
            current
                .checked_add(FUNCTION_ALIGNMENT - 1)
                .map(|aligned| aligned / FUNCTION_ALIGNMENT * FUNCTION_ALIGNMENT)?
                .checked_add(appended)
        };
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            let full_plan = plan(index)?;
            let body_compile_started = diagnostics_enabled.then(std::time::Instant::now);
            let body_memory_before = body_compile_started.and_then(|_| jit_memory_snapshot());
            let body_result = analysis.compile_function(
                &*isa,
                index,
                &full_plan,
                &plans,
                guarded,
                fixed_base,
                &globals,
                &reference_site_ids[index],
                diagnostics_enabled,
            );
            if let Some(started) = body_compile_started {
                let memory_after = jit_memory_snapshot();
                eprintln!(
                    "QUENCH_WASM_JIT_BODY_MEMORY function={index} elapsed_us={} rss_kib_before={:?} rss_kib_after={:?} hwm_kib_before={:?} hwm_kib_after={:?}",
                    started.elapsed().as_micros(),
                    body_memory_before.map(|memory| memory.0),
                    memory_after.map(|memory| memory.0),
                    body_memory_before.map(|memory| memory.1),
                    memory_after.map(|memory| memory.1),
                );
            }
            let (body, function_calls) = body_result?;
            let body_projected_len = projected_code_len(bytes.len(), body.len())?;
            if root.is_some() && body_projected_len > MAX_JIT_ROOT_CODE_BYTES {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={} native-code-bytes={body_projected_len} limit={MAX_JIT_ROOT_CODE_BYTES}",
                        root.expect("root mode checked")
                    );
                }
                return None;
            }
            let body_len = body.len();
            let offset = place(&mut bytes, body);
            offsets[index] = Some(offset);
            spans.push((offset, body_len, index));
            calls.extend(function_calls.into_iter().map(|call| (offset, call)));
            let memory = MemoryArguments::of(fixed_base, guarded);
            let entry = analysis.compile_entry(&*isa, index, plans[index].as_ref()?, memory)?;
            let entry_projected_len = projected_code_len(bytes.len(), entry.len())?;
            if root.is_some() && entry_projected_len > MAX_JIT_ROOT_CODE_BYTES {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={} native-code-bytes={entry_projected_len} limit={MAX_JIT_ROOT_CODE_BYTES}",
                        root.expect("root mode checked")
                    );
                }
                return None;
            }
            entries[index] = Some(place(&mut bytes, entry));
        }
        for (offset, call) in calls {
            link(&mut bytes, offset, &call, offsets[call.callee]?)?;
        }
        let memory = CodeMemory::new(&bytes)?;
        let mapped_bytes = memory.length;
        let base = memory.pointer as usize;
        write_perf_map(base, &bytes, &spans);
        let table: Box<[usize]> = offsets
            .iter()
            .map(|offset| offset.map_or(0, |offset| base + offset))
            .collect();
        let functions: Vec<Option<CompiledFunction>> = plans
            .into_iter()
            .zip(entries)
            .map(|(plan, entry)| {
                let plan = plan?;
                let entry = entry?;
                // SAFETY: the entry was compiled with the `Entry` signature in
                // the host's default calling convention.
                let entry = unsafe { std::mem::transmute::<usize, Entry>(base + entry) };
                Some(CompiledFunction {
                    entry,
                    params: plan.params,
                    result: plan.result,
                    #[cfg(test)]
                    native_entries: std::cell::Cell::new(0),
                    #[cfg(test)]
                    direct_reference_entries: std::cell::Cell::new(0),
                })
            })
            .collect();
        let compiled_functions = functions
            .iter()
            .filter(|function| function.is_some())
            .count();
        let diagnostics = diagnostics_enabled.then(|| {
            let import_callout_names = self
                .programs
                .wasm_signatures(program)
                .map(|signatures| {
                    signatures
                        .imports()
                        .iter()
                        .enumerate()
                        .map(|(function, import)| {
                            (
                                u32::try_from(function).expect("Wasm function indices fit in u32"),
                                import.name.index,
                                import.name.module.clone(),
                                import.name.name.clone(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let import_callout_counts = import_callout_names
                .iter()
                .map(|_| std::cell::Cell::new(0))
                .collect();
            WasmJitDiagnostics {
                program: program.raw(),
                total_functions: count,
                compiled_functions,
                code_bytes: bytes.len(),
                mapped_bytes,
                compile_ns: compile_started.map_or(0, |started| started.elapsed().as_nanos()),
                native_entry_requests: (0..count).map(|_| std::cell::Cell::new(0)).collect(),
                native_entry_samples: (0..count).map(|_| std::cell::Cell::new(0)).collect(),
                native_entry_sampled_ns: (0..count).map(|_| std::cell::Cell::new(0)).collect(),
                interpreter_fallback_entries: (0..count).map(|_| std::cell::Cell::new(0)).collect(),
                compiled_to_vm_callouts: std::cell::Cell::new(0),
                compiled_import_callouts: std::cell::Cell::new(0),
                compiled_known_callouts: std::cell::Cell::new(0),
                compiled_reference_callouts: std::cell::Cell::new(0),
                emscripten_memcpy_fast_path_hits: std::cell::Cell::new(0),
                emscripten_memcpy_raw_i32_hits: std::cell::Cell::new(0),
                emscripten_memcpy_early_import_hits: std::cell::Cell::new(0),
                emscripten_memcpy_leaf_hits: std::cell::Cell::new(0),
                import_callout_names,
                import_callout_counts,
                reference_call_sites,
            }
        });
        Some(CompiledProgram {
            _code: memory,
            table,
            functions,
            memory_slot,
            guarded,
            fixed_base,
            globals,
            diagnostics,
            reference_call_caches: std::cell::RefCell::new(
                std::iter::repeat_with(|| None)
                    .take(reference_cache_count)
                    .collect(),
            ),
            reference_fast_caches: (0..reference_cache_count)
                .map(|_| UnsafeCell::new(FastReferenceCache::default()))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            reference_cache_sites,
            #[cfg(test)]
            reference_cache_hits: std::cell::Cell::new(0),
        })
    }

    #[cfg(test)]
    pub(super) fn install_compiled_diagnostics_for_test(
        &mut self,
        program: ProgramId,
        env: Value,
    ) -> bool {
        let Some(compiled) = self
            .compile_program_with_diagnostics(program, env, true)
            .map(Rc::new)
        else {
            return false;
        };
        self.programs.set_compiled(program, Some(compiled));
        true
    }

    #[cfg(test)]
    pub(super) fn install_compiled_root_for_test(
        &mut self,
        program: ProgramId,
        env: Value,
        root: usize,
    ) -> bool {
        let Some(compiled) = self
            .compile_program_with_diagnostics_and_root(program, env, true, Some(root))
            .map(Rc::new)
        else {
            return false;
        };
        self.programs.set_compiled(program, Some(compiled));
        true
    }

    #[cfg(test)]
    pub(super) fn install_compiled_root_only_for_test(
        &mut self,
        program: ProgramId,
        env: Value,
        root: usize,
    ) -> bool {
        let Some(compiled) = self
            .compile_program_with_diagnostics_and_root_mode(program, env, true, Some(root), true)
            .map(Rc::new)
        else {
            return false;
        };
        self.programs.set_compiled(program, Some(compiled));
        true
    }

    /// Run known function `id` of the caller's program as native code, with
    /// arguments from the caller's register `window`. `None` leaves the call
    /// to the interpreter.
    pub(super) fn call_compiled(
        &mut self,
        caller: usize,
        id: u32,
        window: crate::bytecode::RegisterWindow,
    ) -> Option<Result<Value, JsError>> {
        if self.execution_mode != ExecutionMode::Compiled {
            return None;
        }
        let (program, env) = {
            let frame = self.frames.get(caller)?;
            (frame.program, frame.env)
        };
        self.run_compiled_inner(
            program,
            env,
            id,
            CompiledArgs::RegisterWindow {
                caller,
                base: usize::from(window.base),
                count: usize::from(window.count),
            },
        )
    }

    /// Run function `id` of `program` in instance `env` as native code, with
    /// encoded Wasm `args`. `None` leaves the call to the interpreter, which
    /// is then exactly where the call would otherwise have gone.
    pub(super) fn run_compiled(
        &mut self,
        program: ProgramId,
        env: Value,
        id: u32,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        self.run_compiled_inner(program, env, id, CompiledArgs::Values(args))
    }

    /// Convert an owned argument window or a public Wasm value slice into the
    /// raw scalar words expected by a compiled entry. The register borrow is
    /// confined to this conversion and ends before native code can call back
    /// into the VM.
    fn compiled_raw_args(
        &self,
        args: CompiledArgs<'_>,
        params: &[crate::WasmType],
    ) -> Option<smallvec::SmallVec<[u64; 8]>> {
        let values = match args {
            CompiledArgs::Values(values) => values,
            CompiledArgs::RegisterWindow {
                caller,
                base,
                count,
            } => self
                .frames
                .get(caller)?
                .registers
                .get(base..base.checked_add(count)?)?,
        };
        if values.len() != params.len() {
            return None;
        }
        let mut raw = smallvec::SmallVec::new();
        for (value, ty) in values.iter().zip(params) {
            raw.push(match Kind::of(*ty)? {
                Kind::I32 => u64::from(value.wasm_bits32()),
                Kind::I64 => self.decode_bits64(*value)?,
            });
        }
        Some(raw)
    }

    fn run_compiled_inner(
        &mut self,
        program: ProgramId,
        env: Value,
        id: u32,
        args: CompiledArgs<'_>,
    ) -> Option<Result<Value, JsError>> {
        if self.execution_mode != ExecutionMode::Compiled {
            return None;
        }
        let wasm_root_only = jit_wasm_root_only_enabled()
            && self
                .programs
                .residual(program)
                .is_some_and(|code| code.kind == crate::bytecode::ProgramKind::Wasm);
        let wasm_root_first = jit_wasm_root_first_enabled()
            && self
                .programs
                .residual(program)
                .is_some_and(|code| code.kind == crate::bytecode::ProgramKind::Wasm);
        let baseline = match self.programs.compiled(program) {
            Some(compiled) => compiled,
            None => {
                if self
                    .programs
                    .residual(program)
                    .is_some_and(|code| code.kind == crate::bytecode::ProgramKind::Wasm)
                {
                    self.host.before_native_compilation();
                }
                if wasm_root_only || wasm_root_first {
                    // This explicit diagnostic mode compiles only the
                    // function named by QUENCH_WASM_JIT_ROOT. Root-only mode
                    // keeps its callees in the VM; root-first mode allows a
                    // hot callee to enter the existing bounded tier below.
                    let diagnose = jit_diagnostics_enabled();
                    let started = diagnose.then(std::time::Instant::now);
                    let root = jit_root_override().ok().flatten();
                    let compiled = root
                        .and_then(|root| {
                            self.compile_program_with_diagnostics_and_root_mode(
                                program,
                                env,
                                diagnose,
                                Some(root),
                                true,
                            )
                        })
                        .map(Rc::new);
                    if diagnose && compiled.is_none() {
                        let mode_field = if wasm_root_first {
                            "\"root_first\":true"
                        } else {
                            "\"root_only\":true"
                        };
                        eprintln!(
                            "QUENCH_WASM_JIT_STATS {{\"program\":{},{},\"compile_failed\":true,\"root\":{},\"compile_ns\":{}}}",
                            program.raw(),
                            mode_field,
                            root.map_or_else(|| "null".to_owned(), |root| root.to_string()),
                            started
                                .expect("diagnostic timer started")
                                .elapsed()
                                .as_nanos(),
                        );
                    }
                    self.programs.set_compiled(program, compiled.clone());
                    self.programs.compiled(program).flatten()
                } else if jit_wasm_selective_enabled()
                    && self
                        .programs
                        .residual(program)
                        .is_some_and(|code| code.kind == crate::bytecode::ProgramKind::Wasm)
                {
                    // Selective mode starts cold Wasm functions in the
                    // interpreter. Hot roots are promoted after the entry
                    // gate below, avoiding module-wide cold compilation.
                    self.programs.set_compiled(program, None);
                    None
                } else {
                    let diagnose = jit_diagnostics_enabled()
                        && self
                            .programs
                            .get(program)
                            .is_some_and(|code| code.kind == crate::bytecode::ProgramKind::Wasm);
                    let started = diagnose.then(std::time::Instant::now);
                    let compiled = self.compile_program(program, env).map(Rc::new);
                    if diagnose && compiled.is_none() {
                        let total_functions = self
                            .programs
                            .get(program)
                            .map_or(0, |code| code.functions.len());
                        eprintln!(
                            "QUENCH_WASM_JIT_STATS {{\"program\":{},\"compile_attempts\":1,\"compile_failed\":true,\"total_functions\":{},\"compile_ns\":{}}}",
                            program.raw(),
                            total_functions,
                            started
                                .expect("diagnostic timer started")
                                .elapsed()
                                .as_nanos(),
                        );
                    }
                    self.programs.set_compiled(program, compiled.clone());
                    self.programs.compiled(program).flatten()
                }
            }
        };
        // Root-first mode earns further tiering only after its selected root
        // produced an artifact. If admission fails, leave the workload in the
        // interpreter instead of silently becoming ordinary selective JIT.
        let root_first_ready = !wasm_root_first || baseline.is_some();
        let mut compiled = baseline
            .filter(|compiled| compiled.has_function(id as usize))
            .or_else(|| self.programs.wasm_tier_containing(program, id));
        if compiled.is_none()
            && !wasm_root_only
            && root_first_ready
            && self.programs.residual(program).is_some_and(|code| {
                code.kind == crate::bytecode::ProgramKind::Wasm
                    && code.function_get(id as usize).is_some_and(|function| {
                        function.code.len() <= MAX_JIT_TIER_BYTECODES_PER_FUNCTION
                    })
            })
            && self.programs.note_wasm_tier_entry(program, id)
        {
            let tier = self
                .compile_program_with_diagnostics_and_root(
                    program,
                    env,
                    jit_diagnostics_enabled(),
                    Some(id as usize),
                )
                .map(Rc::new);
            if let Some(tier) = tier
                && self.programs.install_wasm_tier(program, id, tier)
            {
                compiled = self.programs.wasm_tier(program, id);
            }
        }
        let Some(compiled) = compiled else {
            self.programs
                .has_compiled_function_or_note_fallback(program, id as usize);
            return None;
        };
        let result = (|| {
            let function = compiled.functions.get(id as usize)?.as_ref()?;
            let (base, length, guard, memory_binding) = match compiled.memory_slot {
                Some(slot) => {
                    let binding = self.heap.environment_slot(env, slot as usize)?;
                    let view = super::dispatch_fast::unshared_memory(self, binding)?;
                    let guard = guarded_memory(self, binding);
                    // Code compiled without bounds checks needs this memory's guard.
                    if compiled.guarded && guard.is_none() {
                        return None;
                    }
                    if compiled.fixed_base.is_some_and(|fixed| {
                        guard.map(|(base, _)| displacement_base(base)) != Some(Some(fixed))
                    }) {
                        return None;
                    }
                    (view.0 as u64, view.1 as u64, guard, binding)
                }
                None => (0, 0, None, Value::UNDEFINED),
            };
            let raw = self.compiled_raw_args(args, &function.params)?;
            let marker = 0u8;
            let stack_pointer = std::hint::black_box(&marker as *const u8 as usize);
            let mut globals: smallvec::SmallVec<[*mut Value; 8]> = smallvec::SmallVec::new();
            for slot in &compiled.globals {
                let binding = self.heap.environment_slot(env, *slot as usize)?;
                match self.heap.get_mut(binding)? {
                    Cell::WasmGlobal { value, .. } => globals.push(value),
                    _ => return None,
                }
            }
            let (guard_start, guard_end) = guard.map_or((0, 0), |(start, size)| {
                (start as usize, start as usize + size)
            });
            let mut import_callouts = 0u64;
            let mut known_callouts = 0u64;
            let mut reference_callouts = 0u64;
            let mut emscripten_memcpy_fast_path_hits = 0u64;
            let mut emscripten_memcpy_raw_i32_hits = 0u64;
            let mut emscripten_memcpy_early_import_hits = 0u64;
            let mut emscripten_memcpy_leaf_hits = 0u64;
            let mut emscripten_memcpy_cache = EmscriptenMemcpyCache::default();
            let (
                import_callouts_ptr,
                known_callouts_ptr,
                reference_callouts_ptr,
                emscripten_memcpy_fast_path_hits_ptr,
                emscripten_memcpy_raw_i32_hits_ptr,
                emscripten_memcpy_early_import_hits_ptr,
                emscripten_memcpy_leaf_hits_ptr,
            ): (
                *mut u64,
                *mut u64,
                *mut u64,
                *mut u64,
                *mut u64,
                *mut u64,
                *mut u64,
            ) = if compiled.diagnostics.is_some() {
                (
                    &mut import_callouts as *mut u64,
                    &mut known_callouts as *mut u64,
                    &mut reference_callouts as *mut u64,
                    &mut emscripten_memcpy_fast_path_hits as *mut u64,
                    &mut emscripten_memcpy_raw_i32_hits as *mut u64,
                    &mut emscripten_memcpy_early_import_hits as *mut u64,
                    &mut emscripten_memcpy_leaf_hits as *mut u64,
                )
            } else {
                (
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            let poll = self.execution_interrupt.poll_page();
            let mut context = CallContext {
                stack_limit: stack_pointer.saturating_sub(COMPILED_STACK_BUDGET),
                interrupt: self.execution_interrupt.atomic_flag(),
                table: compiled.table.as_ptr(),
                vm: (self as *mut Self).cast(),
                env,
                entry_stack: 0,
                guard_start,
                guard_end,
                globals: globals.as_ptr(),
                callout: import_callout::<H> as Callout as usize,
                call_reference: reference_callout::<H> as ReferenceCallout as usize,
                compiled: Rc::as_ptr(&compiled),
                program,
                import_callouts: import_callouts_ptr,
                known_callouts: known_callouts_ptr,
                reference_callouts: reference_callouts_ptr,
                memory: memory_binding,
                memory_length: length,
                raised: None,
                call_reference_diagnostic: compiled.diagnostics.as_ref().map_or(0, |_| {
                    diagnostic_reference_callout::<H> as DiagnosticReferenceCallout as usize
                }),
                reference_call_sites: compiled
                    .diagnostics
                    .as_ref()
                    .map_or(std::ptr::null(), |stats| {
                        stats.reference_call_sites.as_ptr()
                    }),
                reference_call_site_count: compiled
                    .diagnostics
                    .as_ref()
                    .map_or(0, |stats| stats.reference_call_sites.len()),
                poll_start: poll.map_or(0, |(start, _)| start),
                poll_end: poll.map_or(0, |(start, bytes)| start + bytes),
                reference_epoch: self.wasm_reference_epoch.as_ptr(),
                reference_fast_caches: compiled.reference_fast_caches_ptr(),
                emscripten_memcpy_fast_path_hits: emscripten_memcpy_fast_path_hits_ptr,
                emscripten_memcpy_raw_i32_hits: emscripten_memcpy_raw_i32_hits_ptr,
                emscripten_memcpy_early_import_hits: emscripten_memcpy_early_import_hits_ptr,
                emscripten_memcpy_cache: &mut emscripten_memcpy_cache,
                emscripten_memcpy_leaf: if jit_emscripten_memcpy_leaf_enabled() {
                    compiled_emscripten_memcpy_leaf as DirectMemcpyLeaf as usize
                } else {
                    0
                },
                emscripten_memcpy_leaf_hits: emscripten_memcpy_leaf_hits_ptr,
                import_callout_counts: compiled
                    .diagnostics
                    .as_ref()
                    .map_or(std::ptr::null(), |stats| {
                        stats.import_callout_counts.as_ptr()
                    }),
                import_callout_count: compiled
                    .diagnostics
                    .as_ref()
                    .map_or(0, |stats| stats.import_callout_counts.len()),
                adapter_payload: std::ptr::null_mut(),
            };
            let mut out = 0u64;
            let native_entry_should_sample = compiled.note_native_entry(id as usize);
            let native_entry_sample_started =
                native_entry_should_sample.then(std::time::Instant::now);
            // SAFETY: the entry is compiled code for this function's
            // signature; the context, arguments and result outlive the call,
            // and the memory view stays valid because compiled code never
            // grows memory.
            let status = unsafe {
                enter_native(
                    &mut context,
                    base,
                    length,
                    raw.as_ptr(),
                    &mut out,
                    NativeEntryMode::Resumable,
                    function.entry,
                )
            };
            if let Some(started) = native_entry_sample_started {
                compiled.finish_native_entry_sample(id as usize, started);
            }
            if let Some(stats) = &compiled.diagnostics {
                stats.compiled_to_vm_callouts.set(
                    stats
                        .compiled_to_vm_callouts
                        .get()
                        .saturating_add(import_callouts)
                        .saturating_add(known_callouts)
                        .saturating_add(reference_callouts),
                );
                stats.compiled_import_callouts.set(
                    stats
                        .compiled_import_callouts
                        .get()
                        .saturating_add(import_callouts),
                );
                stats.compiled_known_callouts.set(
                    stats
                        .compiled_known_callouts
                        .get()
                        .saturating_add(known_callouts),
                );
                stats.compiled_reference_callouts.set(
                    stats
                        .compiled_reference_callouts
                        .get()
                        .saturating_add(reference_callouts),
                );
                stats.emscripten_memcpy_fast_path_hits.set(
                    stats
                        .emscripten_memcpy_fast_path_hits
                        .get()
                        .saturating_add(emscripten_memcpy_fast_path_hits),
                );
                stats.emscripten_memcpy_raw_i32_hits.set(
                    stats
                        .emscripten_memcpy_raw_i32_hits
                        .get()
                        .saturating_add(emscripten_memcpy_raw_i32_hits),
                );
                stats.emscripten_memcpy_early_import_hits.set(
                    stats
                        .emscripten_memcpy_early_import_hits
                        .get()
                        .saturating_add(emscripten_memcpy_early_import_hits),
                );
                stats.emscripten_memcpy_leaf_hits.set(
                    stats
                        .emscripten_memcpy_leaf_hits
                        .get()
                        .saturating_add(emscripten_memcpy_leaf_hits),
                );
            }
            if i64::from(status) == TERMINATED_STATUS {
                return Some(Err(JsError::execution_terminated()));
            }
            if i64::from(status) == RAISED_STATUS {
                return match context.raised.take()? {
                    Raised::Error(error) => Some(Err(error)),
                    Raised::Panic(panic) => std::panic::resume_unwind(panic),
                };
            }
            if status != 0 {
                let trap = TRAPS[status as usize - 1];
                return Some(Err(JsError::wasm_trap_error(trap)));
            }
            Some(Ok(match function.result {
                Some(ty) => match Kind::of(ty)? {
                    Kind::I32 => Value::integer(out as u32 as i32),
                    Kind::I64 => self.encode_bits64(out),
                },
                None => self.programs.constant(program, VOID_RESULT_CONSTANT)?,
            }))
        })();
        if let Some(stats) = &compiled.diagnostics
            && result.is_none()
            && let Some(fallbacks) = stats.interpreter_fallback_entries.get(id as usize)
        {
            fallbacks.set(fallbacks.get().saturating_add(1));
        }
        result
    }
}

impl<H: Host> Vm<H> {
    /// Try the exact raw-i32 Emscripten copy before the general import
    /// dispatcher. The import-callout's outer panic/context/memory handling
    /// still runs on both the fast path and every fallback.
    ///
    /// # Safety
    /// `args` points to the compiled caller's three Wasm ABI words when the
    /// module and host signatures checked below are exactly (i32, i32, i32).
    unsafe fn try_early_compiled_emscripten_memcpy_import(
        &mut self,
        program: ProgramId,
        instance_env: Value,
        function: u64,
        current_memory: Value,
        raw_i32_hits: *mut u64,
        early_import_hits: *mut u64,
        args: *const u64,
    ) -> bool {
        if !jit_emscripten_memcpy_leaf_enabled()
            && (!super::wasm_js::emscripten_memcpy_raw_i32_enabled()
                || !super::wasm_js::emscripten_memcpy_early_import_enabled())
        {
            return false;
        }
        let Ok(function) = u32::try_from(function) else {
            return false;
        };
        if self.programs.get(program).is_none() {
            return false;
        }
        let Some(signatures) = self.programs.wasm_signatures(program) else {
            return false;
        };
        let import_count = signatures.imports().len();
        let function_index = function as usize;
        if function_index >= import_count
            || signatures.defined_index(function).is_some()
            || !signatures.get(function_index).is_some_and(|signature| {
                signature.params.as_slice()
                    == [
                        crate::WasmType::I32,
                        crate::WasmType::I32,
                        crate::WasmType::I32,
                    ]
                    && signature.results.is_empty()
            })
            || !signatures
                .imports()
                .get(function_index)
                .is_some_and(|import| import.name.name == "_emscripten_memcpy_js")
        {
            return false;
        }

        // Imports are the final slots in the per-instance environment. Match
        // wasm_function_reference's mapping without switching active program
        // or constructing a wrapper Value.
        let Some(Cell::Environment { slots, .. }) = self.heap.get(instance_env) else {
            return false;
        };
        let Some(import_slot) = slots
            .len()
            .checked_sub(import_count)
            .and_then(|base| base.checked_add(function_index))
        else {
            return false;
        };
        let Some(wrapper) = self.heap.environment_slot(instance_env, import_slot) else {
            return false;
        };
        let Some(FunctionRecord {
            kind: FunctionKind::Native(Native::WasmHost),
            env: wrapper_env,
            ..
        }) = self.heap.function(wrapper)
        else {
            return false;
        };
        let wrapper_env = *wrapper_env;
        let Some(Cell::WasmHostFunction {
            target: crate::heap::WasmHostTarget::JsEmscriptenMemcpy(target),
            signature,
        }) = self.heap.get(wrapper_env)
        else {
            return false;
        };
        if signature.params.as_slice()
            != [
                crate::WasmType::I32,
                crate::WasmType::I32,
                crate::WasmType::I32,
            ]
            || !signature.results.is_empty()
        {
            return false;
        }
        let view_environment = target.view_environment;
        let view_slot = target.view_slot;

        // Match the ordinary opaque import's stack and cancellation gate. If
        // either gate fails, defer to call_import so it creates the error with
        // the full NativeActivation and realm state installed.
        let Ok(_stack) = self.enter_stack() else {
            return false;
        };
        if self.check_execution_interrupt().is_err() {
            return false;
        }
        // SAFETY: both Wasm and host signatures above prove exactly three i32
        // words are available; their low 32 bits are the signed ABI values.
        let indices = unsafe {
            [
                *args.add(0) as u32 as i32,
                *args.add(1) as u32 as i32,
                *args.add(2) as u32 as i32,
            ]
        };
        if !self.try_emscripten_memcpy_same_memory_raw_capture(
            view_environment,
            view_slot,
            current_memory,
            indices,
            raw_i32_hits,
        ) {
            return false;
        }
        // This call is invisible to guest code after the complete helper and
        // memory guards pass; retain optional profile-aggregate accounting.
        self.profile.call_target(0, 3);
        note_compiled_callout(early_import_hits);
        true
    }

    /// Call imported `function` of `program` in instance `env` with the Wasm
    /// bits at `args`, storing its result's bits at `out`.
    ///
    /// # Safety
    /// `args` holds a word per parameter of the import and `out` one word.
    unsafe fn call_import(
        &mut self,
        program: ProgramId,
        env: Value,
        function: u32,
        current_memory: Value,
        memcpy_fast_path_hits: *mut u64,
        memcpy_raw_i32_hits: *mut u64,
        args: *const u64,
        out: *mut u64,
    ) -> Result<(), JsError> {
        let code = self
            .programs
            .get(program)
            .ok_or_else(|| JsError::validation("missing compiled Wasm program".into()))?;
        let (params, result_type, module_signature_is_raw_i32) = {
            let signature = self
                .programs
                .wasm_signatures(program)
                .and_then(|signatures| signatures.get(function as usize))
                .ok_or_else(|| JsError::validation("missing Wasm import signature".into()))?;
            (
                signature
                    .params
                    .iter()
                    .copied()
                    .collect::<smallvec::SmallVec<[crate::WasmType; 8]>>(),
                signature.results.first().copied(),
                signature.params.as_slice()
                    == [
                        crate::WasmType::I32,
                        crate::WasmType::I32,
                        crate::WasmType::I32,
                    ]
                    && signature.results.is_empty(),
            )
        };
        let active_program = self.replace_active_program(program);
        let callee = self.wasm_function_reference(&code, function, env);
        self.set_active_program(active_program);
        let callee = callee?;
        let receiver = self
            .programs
            .constant(program, VOID_RESULT_CONSTANT)
            .ok_or_else(|| JsError::validation("missing Wasm void constant".into()))?;
        let defined = self
            .programs
            .wasm_signatures(program)
            .and_then(|signatures| signatures.defined_index(function));
        let imported_name_is_emscripten_memcpy = self
            .programs
            .wasm_signatures(program)
            .and_then(|signatures| signatures.imports().get(function as usize))
            .is_some_and(|import| import.name.name == "_emscripten_memcpy_js");
        let raw_i32_import = if defined.is_none()
            && module_signature_is_raw_i32
            && imported_name_is_emscripten_memcpy
            && super::wasm_js::emscripten_memcpy_raw_i32_enabled()
        {
            match self.heap.function(callee) {
                Some(FunctionRecord {
                    kind: FunctionKind::Native(Native::WasmHost),
                    env,
                    ..
                }) => match self.heap.get(*env) {
                    Some(Cell::WasmHostFunction {
                        target: crate::heap::WasmHostTarget::JsEmscriptenMemcpy(target),
                        signature,
                    }) if signature.params.as_slice()
                        == [
                            crate::WasmType::I32,
                            crate::WasmType::I32,
                            crate::WasmType::I32,
                        ]
                        && signature.results.is_empty() =>
                    {
                        Some((
                            target.function,
                            (target.view_environment, target.view_slot),
                            signature.clone(),
                        ))
                    }
                    _ => None,
                },
                _ => None,
            }
        } else {
            None
        };
        let raw_result = if let Some((target, memcpy_capture, signature)) = raw_i32_import {
            // SAFETY: the module and host signatures above prove there are
            // exactly three i32 parameters; raw Wasm ABI words carry the
            // signed i32 bit patterns in their low 32 bits.
            let raw_indices = unsafe {
                [
                    *args.add(0) as u32 as i32,
                    *args.add(1) as u32 as i32,
                    *args.add(2) as u32 as i32,
                ]
            };
            Some(self.call_compiled_js_wasm_import(
                program,
                &code,
                callee,
                target,
                Some(memcpy_capture),
                &signature,
                receiver,
                &[],
                Some(raw_indices),
                current_memory,
                memcpy_fast_path_hits,
                memcpy_raw_i32_hits,
            )?)
        } else {
            None
        };
        let result = if let Some(result) = raw_result {
            result
        } else {
            // Allocation never collects, so the encoded arguments stay live
            // until the ordinary callout roots them.
            let mut values: smallvec::SmallVec<[Value; 8]> = smallvec::SmallVec::new();
            for (offset, ty) in params.iter().enumerate() {
                // SAFETY: the caller passes a word per parameter.
                let bits = unsafe { *args.add(offset) };
                values.push(self.encode_wasm_value(crate::WasmValue::from_bits(*ty, bits)));
            }
            if let Some(defined) = defined {
                let active_program = self.replace_active_program(program);
                let compiled = self.run_compiled(program, env, defined, &values);
                self.set_active_program(active_program);
                if let Some(compiled) = compiled {
                    let result = compiled?;
                    let bits = if let Some(ty) = result_type {
                        match self.decode_wasm_value(result, ty)?.bits() {
                            Some(crate::wasm::ScalarBits::Bits32(bits)) => u64::from(bits),
                            Some(crate::wasm::ScalarBits::Bits64(bits)) => bits,
                            _ => {
                                return Err(JsError::validation(
                                    "non-scalar Wasm import result".into(),
                                ));
                            }
                        }
                    } else {
                        0
                    };
                    // SAFETY: the caller passes a result word.
                    unsafe { *out = bits };
                    return Ok(());
                }
            }
            let js_import = match self.heap.function(callee) {
                Some(FunctionRecord {
                    kind: FunctionKind::Native(Native::WasmHost),
                    env,
                    ..
                }) => match self.heap.get(*env) {
                    Some(Cell::WasmHostFunction { target, signature })
                        if values.len() == signature.params.len() =>
                    {
                        match target {
                            crate::heap::WasmHostTarget::Js(target) => {
                                Some((*target, None, signature.clone()))
                            }
                            crate::heap::WasmHostTarget::JsEmscriptenMemcpy(target) => Some((
                                target.function,
                                Some((target.view_environment, target.view_slot)),
                                signature.clone(),
                            )),
                            crate::heap::WasmHostTarget::Host(_) => None,
                        }
                    }
                    _ => None,
                },
                _ => None,
            };
            if let Some((target, memcpy_capture, signature)) = js_import {
                self.call_compiled_js_wasm_import(
                    program,
                    &code,
                    callee,
                    target,
                    memcpy_capture,
                    &signature,
                    receiver,
                    &values,
                    None,
                    current_memory,
                    memcpy_fast_path_hits,
                    memcpy_raw_i32_hits,
                )?
            } else {
                self.call_value(&code, callee, receiver, &values)?
            }
        };
        if let Some(ty) = result_type {
            let bits = match self.decode_wasm_value(result, ty)?.bits() {
                Some(crate::wasm::ScalarBits::Bits32(bits)) => u64::from(bits),
                Some(crate::wasm::ScalarBits::Bits64(bits)) => bits,
                _ => return Err(JsError::validation("non-scalar Wasm import result".into())),
            };
            // SAFETY: the caller passes a result word.
            unsafe { *out = bits };
        }
        Ok(())
    }
}

/// The tag bits of an encoded i32, which a global's value carries.
const INTEGER_TAG: u64 = crate::value::INTEGER_TAG_BITS;

/// The constant a void Wasm function returns (see the Wasm lowering).
const VOID_RESULT_CONSTANT: usize = 1;

/// `compiled`'s body run directly on `arguments`, its parameters, when each is a Number
/// its guards admit: the call a known callee makes without a frame (`js_jit_compiled_call`).
fn direct_call(
    compiled: &CompiledJsFunction,
    arguments: &[Value],
    interrupt: &crate::ExecutionInterrupt,
) -> Option<CompiledJsCallResult> {
    let int32_identity = compiled.is_int32_identity();
    if int32_identity && arguments.is_empty() {
        return None;
    }
    let mut numeric_arguments: smallvec::SmallVec<[f64; 8]> = smallvec::SmallVec::new();
    for (index, argument) in arguments.iter().enumerate() {
        // An integer Value is an int32 Number by representation; any other Number is
        // checked where the body's guards require one.
        let value = match argument.as_int() {
            Some(integer) => f64::from(integer),
            None => {
                let value = argument.as_number()?;
                let int32 = compiled.requires_signed_int32_arguments() || (int32_identity && index == 0);
                if int32 && !super::js_compiled::is_i32_number(value) {
                    return None;
                }
                value
            }
        };
        numeric_arguments.push(value);
    }
    // Only a body that polls reads the poll page.
    let poll_page = (compiled.native_entry_mode() == NativeEntryMode::Resumable)
        .then(|| native_poll_page(interrupt))
        .flatten();
    match compiled.call(&numeric_arguments, interrupt.atomic_flag(), poll_page)? {
        CompiledJsResult::Number(value) | CompiledJsResult::Int32Identity { value, .. } => {
            Some(CompiledJsCallResult::Value(Value::number(value)))
        }
        CompiledJsResult::Boolean(value) => {
            Some(CompiledJsCallResult::Value(if value { Value::TRUE } else { Value::FALSE }))
        }
        CompiledJsResult::StringReturnPc(return_pc) => {
            compiled.string_constant_for_return(return_pc).map(CompiledJsCallResult::ReturnConstant)
        }
        CompiledJsResult::StringTransform(_, _) | CompiledJsResult::FieldReject { .. } => None,
    }
}

/// A compiled body a frameless activation runs directly (`Vm::direct_tier_entry`): the
/// artifact, held weakly so that the tier retiring it ends the direct run, its parameter
/// count, and the `Return` its result completes.
pub(super) struct DirectTierEntry {
    compiled: std::rc::Weak<CompiledJsFunction>,
    params: usize,
    return_pc: usize,
    return_register: crate::bytecode::Register,
}

impl DirectTierEntry {
    /// Whether the tier retired the artifact.
    pub(super) fn retired(&self) -> bool {
        self.compiled.strong_count() == 0
    }
}

/// What the JavaScript tier reads of an activation it enters at PC zero: a VM frame's
/// (`Vm::js_tier_frame_entry`) or a frameless lane activation's
/// (`Vm::js_tier_activation_entry`). Either way a native result continues the same
/// activation, so the tier never decides how a function is called.
pub(super) struct TierEntry {
    program: ProgramId,
    function: u32,
    /// The receiver, where the function binds its own (`Frame::fixed_this`).
    this: Option<Value>,
    /// The activation's parameter registers.
    parameters: smallvec::SmallVec<[Value; 8]>,
    captures: TierCaptures,
    /// Whether a native result may allocate: not in a lane activation, where nothing collects.
    allocates: bool,
}

/// Where an entry's captures are read.
#[derive(Clone, Copy)]
pub(super) enum TierCaptures {
    /// Through the frame's environment and capture cache.
    Frame(usize),
    /// Through the closure's environment.
    Environment(Value),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Op;

    #[test]
    fn region_exit_words_round_trip_each_shared_handoff() {
        for exit in [
            RegionExit::EntryMiss(u32::MAX),
            RegionExit::Resume(17),
            RegionExit::Fallback(29),
            RegionExit::Recompile(41),
        ] {
            assert_eq!(RegionExit::decode(exit.encode()), exit);
        }
    }

    fn memcpy_leaf_context(
        cache: &mut EmscriptenMemcpyCache,
        interrupt: &std::sync::atomic::AtomicBool,
        hits: &mut u64,
    ) -> CallContext {
        CallContext {
            stack_limit: 0,
            interrupt,
            table: std::ptr::null(),
            vm: std::ptr::null_mut(),
            env: Value::NULL,
            entry_stack: 0,
            guard_start: 0,
            guard_end: 0,
            // No poll page: compiled code checks the interrupt flag.
            poll_start: 0,
            poll_end: 0,
            globals: std::ptr::null(),
            callout: 0,
            call_reference: 0,
            compiled: std::ptr::null(),
            program: ProgramId::MAIN,
            import_callouts: std::ptr::null_mut(),
            known_callouts: std::ptr::null_mut(),
            reference_callouts: std::ptr::null_mut(),
            memory: Value::NULL,
            memory_length: 0,
            raised: None,
            call_reference_diagnostic: 0,
            reference_call_sites: std::ptr::null(),
            reference_call_site_count: 0,
            reference_epoch: std::ptr::null(),
            reference_fast_caches: std::ptr::null_mut(),
            emscripten_memcpy_fast_path_hits: std::ptr::null_mut(),
            emscripten_memcpy_raw_i32_hits: std::ptr::null_mut(),
            emscripten_memcpy_early_import_hits: std::ptr::null_mut(),
            emscripten_memcpy_cache: cache,
            emscripten_memcpy_leaf: 0,
            emscripten_memcpy_leaf_hits: hits,
            import_callout_counts: std::ptr::null(),
            import_callout_count: 0,
            adapter_payload: std::ptr::null_mut(),
        }
    }

    #[test]
    fn per_import_callout_counts_use_function_indices_and_escape_json_names() {
        let names = vec![
            (0, 2, "env".to_owned(), "copy\"fast".to_owned()),
            (1, 5, "host\\api".to_owned(), "line\nfeed".to_owned()),
        ];
        let counters = [std::cell::Cell::new(0), std::cell::Cell::new(0)];
        // SAFETY: the pointer covers both initialized counters for their
        // entire use below.
        unsafe {
            note_import_callout_target(counters.as_ptr(), counters.len(), 1);
            note_import_callout_target(counters.as_ptr(), counters.len(), 1);
            note_import_callout_target(counters.as_ptr(), counters.len(), 99);
        }
        assert_eq!(counters[0].get(), 0);
        assert_eq!(counters[1].get(), 2);
        assert_eq!(
            format_import_callout_targets(&names, &counters),
            "{\"function\":1,\"import_index\":5,\"module\":\"host\\\\api\",\"name\":\"line\\nfeed\",\"count\":2}"
        );
        assert_eq!(json_string("\u{1}\t"), "\"\\u0001\\t\"");
    }

    #[test]
    fn native_entry_sampling_is_sparse_and_deterministic() {
        assert!(!native_entry_sample_due(0));
        assert!(!native_entry_sample_due(1));
        assert!(native_entry_sample_due(JIT_NATIVE_ENTRY_SAMPLE_INTERVAL));
        assert!(!native_entry_sample_due(
            JIT_NATIVE_ENTRY_SAMPLE_INTERVAL + 1
        ));
        assert!(native_entry_sample_due(
            JIT_NATIVE_ENTRY_SAMPLE_INTERVAL * 2
        ));
    }

    #[test]
    fn emscripten_memcpy_leaf_copies_overlap_and_observes_cache_miss_and_interrupt() {
        use std::sync::atomic::{AtomicBool, Ordering};

        // Wasm memory bytes: an untyped literal would be `[i32; 6]`.
        let mut bytes: [u8; 6] = [0, 1, 2, 3, 4, 5];
        let mut cache = EmscriptenMemcpyCache {
            valid: 1,
            function: 7,
            base: bytes.as_mut_ptr() as usize,
            length: bytes.len(),
        };
        let interrupt = AtomicBool::new(false);
        let mut hits = 0;
        let mut context = memcpy_leaf_context(&mut cache, &interrupt, &mut hits);

        let status = unsafe { compiled_emscripten_memcpy_leaf(&mut context, 7, 1, 0, 4) };
        assert_eq!(status, 0);
        assert_eq!(bytes, [0, 0, 1, 2, 3, 5]);
        assert_eq!(hits, 1);

        bytes = [0, 1, 2, 3, 4, 5];
        let status = unsafe { compiled_emscripten_memcpy_leaf(&mut context, 7, 0, 1, 4) };
        assert_eq!(status, 0);
        assert_eq!(bytes, [1, 2, 3, 4, 4, 5]);
        assert_eq!(hits, 2);

        bytes = [0, 1, 2, 3, 4, 5];
        let status = unsafe { compiled_emscripten_memcpy_leaf(&mut context, 7, -1, -4, 10) };
        assert_eq!(status, 0);
        assert_eq!(bytes, [0, 1, 2, 3, 4, 2]);
        assert_eq!(hits, 3);

        bytes = [0, 1, 2, 3, 4, 5];
        let status = unsafe { compiled_emscripten_memcpy_leaf(&mut context, 7, 0, 2, -1) };
        assert_eq!(status, 0);
        assert_eq!(bytes, [0, 1, 2, 3, 4, 5]);
        assert_eq!(hits, 4);

        let status = unsafe { compiled_emscripten_memcpy_leaf(&mut context, 8, 0, 1, 3) };
        assert_eq!(status, MEMCPY_LEAF_MISS_STATUS);
        assert_eq!(cache.valid, 0);
        assert_eq!(bytes, [0, 1, 2, 3, 4, 5]);
        assert_eq!(hits, 4);

        let mut interrupt_cache = EmscriptenMemcpyCache {
            valid: 1,
            function: 7,
            base: bytes.as_mut_ptr() as usize,
            length: bytes.len(),
        };
        let mut interrupt_context =
            memcpy_leaf_context(&mut interrupt_cache, &interrupt, &mut hits);
        interrupt.store(true, Ordering::Release);
        let status = unsafe { compiled_emscripten_memcpy_leaf(&mut interrupt_context, 7, 0, 1, 3) };
        assert_eq!(status, TERMINATED_STATUS);
        assert_eq!(bytes, [0, 1, 2, 3, 4, 5]);
        assert_eq!(hits, 4);
    }

    fn affine_pair_instructions(destination: u32, left: u32, right: u32) -> Vec<WideInstruction> {
        vec![
            WideInstruction::new(Op::WasmInstanceBinding, 0, 0, 0, 1),
            WideInstruction::new(Op::WasmI32ShiftLeftImmediate, 1, 2, 0, 2),
            WideInstruction::new(Op::WasmI32AddImmediate, 3, 1, 0, destination),
            WideInstruction::new(Op::WasmI32AddImmediate, 4, 1, 0, left),
            WideInstruction::new(Op::WasmI32Load, 4, 0, 4, 0),
            WideInstruction::new(Op::WasmI32AddImmediate, 5, 1, 0, right),
            WideInstruction::new(Op::WasmI32Load, 5, 0, 5, 0),
            WideInstruction::new(Op::WasmI32Add, 4, 4, 5, 0),
            WideInstruction::new(Op::WasmI32Store, 4, 0, 3, 0),
            WideInstruction::new(Op::WasmI32AddImmediate, 3, 1, 0, destination + 4),
            WideInstruction::new(Op::WasmI32AddImmediate, 4, 1, 0, left + 4),
            WideInstruction::new(Op::WasmI32Load, 4, 0, 4, 0),
            WideInstruction::new(Op::WasmI32AddImmediate, 5, 1, 0, right + 4),
            WideInstruction::new(Op::WasmI32Load, 5, 0, 5, 0),
            WideInstruction::new(Op::WasmI32Add, 4, 4, 5, 0),
            WideInstruction::new(Op::WasmI32Store, 4, 0, 3, 0),
            WideInstruction::new(Op::WasmI32AddImmediate, 2, 2, 0, 2),
            WideInstruction::new(Op::WasmJumpI32NotEqualImmediate, 2, 256, 0, 1),
            WideInstruction::new(Op::LoadConst, 3, 0, 0, 0),
            WideInstruction::new(Op::Return, 3, 0, 0, 0),
        ]
    }

    fn affine_pair_plan(instructions: &[WideInstruction]) -> Plan {
        Plan {
            params: Vec::new(),
            result: None,
            operands: vec![Some(Slot::Value(Kind::I32)); instructions.len()],
        }
    }

    #[test]
    fn affine_pair_loop_requires_static_disjoint_memory32_ranges() {
        let instructions = affine_pair_instructions(77_456, 93_840, 85_648);
        let plan = affine_pair_plan(&instructions);
        let candidate = affine_pair_loop(Some(0), 6, &plan, &instructions, 0)
            .expect("disjoint affine pair is admitted");
        assert_eq!(candidate.header, 1);
        assert_eq!(candidate.branch, 17);
        assert_eq!(candidate.induction, 2);
        assert_eq!(candidate.bound, 256);
        assert_eq!(candidate.destination, 77_456);
        assert_eq!(candidate.left, 93_840);
        assert_eq!(candidate.right, 85_648);
        assert_eq!(candidate.max_end, 94_864);

        let mut unsupported_width = affine_pair_instructions(77_456, 93_840, 85_648);
        unsupported_width[17] =
            WideInstruction::new(Op::WasmJumpI32NotEqualImmediate, 2, 254, 0, 1);
        let plan = affine_pair_plan(&unsupported_width);
        assert!(affine_pair_loop(Some(0), 6, &plan, &unsupported_width, 0).is_none());

        let overlapping = affine_pair_instructions(77_456, 93_840, 77_456);
        let plan = affine_pair_plan(&overlapping);
        assert!(affine_pair_loop(Some(0), 6, &plan, &overlapping, 0).is_none());

        let wrapping = affine_pair_instructions(u32::MAX - 100, 93_840, 85_648);
        let plan = affine_pair_plan(&wrapping);
        assert!(affine_pair_loop(Some(0), 6, &plan, &wrapping, 0).is_none());
    }

    #[test]
    fn affine_pair_loop_rejects_clobbered_induction_and_external_reentry() {
        let mut clobbered = affine_pair_instructions(77_456, 93_840, 85_648);
        clobbered[0] = WideInstruction::new(Op::WasmI32AddImmediate, 2, 2, 0, 1);
        let plan = affine_pair_plan(&clobbered);
        assert!(affine_pair_loop(Some(0), 6, &plan, &clobbered, 0).is_none());

        let mut induction_in_body = affine_pair_instructions(77_456, 93_840, 85_648);
        induction_in_body[4] = WideInstruction::new(Op::WasmI32Load, 2, 0, 4, 0);
        let plan = affine_pair_plan(&induction_in_body);
        assert!(affine_pair_loop(Some(0), 6, &plan, &induction_in_body, 0).is_none());

        let mut scaled_in_body = affine_pair_instructions(77_456, 93_840, 85_648);
        scaled_in_body[2] = WideInstruction::new(Op::WasmI32AddImmediate, 1, 1, 0, 77_456);
        let plan = affine_pair_plan(&scaled_in_body);
        assert!(affine_pair_loop(Some(0), 6, &plan, &scaled_in_body, 0).is_none());

        let mut reentry = affine_pair_instructions(77_456, 93_840, 85_648);
        reentry[18] = WideInstruction::new(Op::Jump, 0, 0, 0, 1);
        let plan = affine_pair_plan(&reentry);
        assert!(affine_pair_loop(Some(0), 6, &plan, &reentry, 0).is_none());

        let mut ambiguous_write = affine_pair_instructions(77_456, 93_840, 85_648);
        ambiguous_write[18] = WideInstruction::new(Op::WasmFillRegisters, 0, 4, 2, 0);
        let plan = affine_pair_plan(&ambiguous_write);
        assert!(affine_pair_loop(Some(0), 6, &plan, &ambiguous_write, 0).is_none());
    }

    #[test]
    fn root_only_jit_defaults_to_none_and_accepts_explicit_opt_level() {
        assert_eq!(parse_jit_root_opt_level(true, None), Ok("none"));
        assert_eq!(parse_jit_root_opt_level(false, None), Ok("speed"));
        assert_eq!(parse_jit_root_opt_level(true, Some("none")), Ok("none"));
        assert_eq!(parse_jit_root_opt_level(true, Some("speed")), Ok("speed"));
        assert_eq!(
            parse_jit_root_opt_level(true, Some("speed_and_size")),
            Ok("speed_and_size")
        );
        assert_eq!(parse_jit_root_opt_level(true, Some("invalid")), Err(()));
    }

    #[test]
    fn direct_compare_branches_match_signed_unsigned_extrema_and_fallthrough() {
        fn comparison_if(
            comparison: wasmparser::Operator<'static>,
            immediate_rhs: Option<i32>,
        ) -> Vec<wasmparser::Operator<'static>> {
            let mut operators = vec![wasmparser::Operator::LocalGet { local_index: 0 }];
            if let Some(rhs) = immediate_rhs {
                operators.push(wasmparser::Operator::I32Const { value: rhs });
            } else {
                operators.push(wasmparser::Operator::LocalGet { local_index: 1 });
            }
            operators.extend([
                comparison,
                wasmparser::Operator::If {
                    blockty: wasmparser::BlockType::Type(wasmparser::ValType::I32),
                },
                wasmparser::Operator::I32Const { value: 1 },
                wasmparser::Operator::Else,
                wasmparser::Operator::I32Const { value: 0 },
                wasmparser::Operator::End,
                wasmparser::Operator::End,
            ]);
            operators
        }

        let bodies = [
            comparison_if(wasmparser::Operator::I32LtS, None),
            comparison_if(wasmparser::Operator::I32LtU, None),
            comparison_if(wasmparser::Operator::I32LtS, Some(0)),
            comparison_if(wasmparser::Operator::I32LtU, Some(1)),
        ]
        .into_iter()
        .map(|operators| (2, 0, true, operators.into_iter().map(Ok)));
        let module = crate::Engine::lower_wasm_i32_module("direct-compare-branches", 0, bodies)
            .expect("comparison branch fixtures lower");

        // The first two functions exercise register comparisons. The latter
        // two force immediate comparisons, including unsigned comparison
        // against a positive signed-field constant.
        let cases = [
            (0, i32::MIN, i32::MAX, 1),
            (0, i32::MAX, i32::MIN, 0),
            (0, 0, 0, 0),
            (1, 0, -1, 1),
            (1, -1, 0, 0),
            (1, i32::MIN, i32::MAX, 0),
            (1, i32::MAX, i32::MIN, 1),
            (2, i32::MIN, 0, 1),
            (2, 0, 0, 0),
            (2, i32::MAX, 0, 0),
            (3, 0, 0, 1),
            (3, 1, 0, 0),
            (3, -1, 0, 0),
        ];

        for direct in [false, true] {
            let mut vm = Vm::new(crate::SystemHost);
            vm.set_execution_mode(ExecutionMode::Compiled);
            for (function_index, left, right, expected) in cases {
                let function = module
                    .select_function(function_index)
                    .expect("comparison fixture function exists");
                let actual = with_jit_direct_compare_branches_for_test(direct, || {
                    vm.execute_wasm_i32(&function, &[left, right])
                        .expect("comparison branch executes")
                });
                assert_eq!(
                    actual,
                    Some(expected),
                    "direct={direct}, function={function_index}, lhs={left}, rhs={right}"
                );
            }
        }
    }

    #[test]
    fn sparse_analysis_states_preserve_register_joins() {
        const FACTS: [Slot; 7] = [
            Slot::DEFAULT,
            Slot::Value(Kind::I64),
            Slot::Binding(7),
            Slot::Memory,
            Slot::Import(9),
            Slot::IndirectReference {
                signature: 11,
                table: 13,
            },
            Slot::Conflict,
        ];
        let states = FACTS
            .iter()
            .flat_map(|first| {
                FACTS.iter().flat_map(move |second| {
                    FACTS.iter().map(move |third| vec![*first, *second, *third])
                })
            })
            .collect::<Vec<_>>();
        let sparse = |dense: &[Slot]| {
            let registers = u16::try_from(dense.len()).expect("small test state");
            let mut state = AnalysisState::new();
            for (index, slot) in dense.iter().copied().enumerate() {
                state
                    .set(
                        u16::try_from(index).expect("small test register"),
                        registers,
                        slot,
                    )
                    .expect("register in range");
            }
            state
        };

        for left in &states {
            for right in &states {
                let mut expected = left.clone();
                let mut changed = false;
                for (slot, incoming) in expected.iter_mut().zip(right) {
                    let joined = slot.join(*incoming);
                    changed |= joined != *slot;
                    *slot = joined;
                }

                let mut actual = sparse(left);
                assert_eq!(actual.join_from(&sparse(right)), changed);
                for (index, expected) in expected.iter().copied().enumerate() {
                    assert_eq!(actual.get(index as u16, 3), Some(expected));
                }
            }
        }

        // Once a loop backedge turns a fact into Conflict, later iterations
        // must not make it precise again.
        let mut loop_state = AnalysisState::new();
        let mut incoming = AnalysisState::new();
        incoming
            .set(0, 1, Slot::Value(Kind::I64))
            .expect("register in range");
        assert!(loop_state.join_from(&incoming));
        assert_eq!(loop_state.get(0, 1), Some(Slot::Conflict));
        assert!(!loop_state.join_from(&AnalysisState::new()));
        assert!(!loop_state.join_from(&incoming));
        assert_eq!(loop_state.get(0, 1), Some(Slot::Conflict));
        assert_eq!(loop_state.get(1, 1), None);

        loop_state
            .set(0, 1, Slot::DEFAULT)
            .expect("register in range");
        assert_eq!(loop_state.get(0, 1), Some(Slot::DEFAULT));
    }

    /// Each i32 integer operator widens to its namesake, and each f32 float
    /// operator to its f64 namesake.
    #[test]
    fn narrow_selectors_name_their_wide_namesakes() {
        let mut tag = 0;
        while let Some(operator) = I32BinaryOperator::from_tag(tag) {
            assert_eq!(
                format!("{operator:?}"),
                format!("{:?}", widen_binary(operator))
            );
            tag += 1;
        }
        let mut tag = 0;
        while let Some(operator) = I32UnaryOperator::from_tag(tag) {
            assert_eq!(
                format!("{operator:?}"),
                format!("{:?}", widen_unary(operator))
            );
            tag += 1;
        }
        let mut tag = 0;
        while let Some(operator) = F32BinaryOperator::from_tag(tag) {
            let (wide, _) = float_binary_operator(Op::WasmF32Binary, tag).expect("f32 selector");
            assert_eq!(format!("{operator:?}"), format!("{wide:?}"));
            tag += 1;
        }
        let mut tag = 0;
        while let Some(operator) = F32UnaryOperator::from_tag(tag) {
            let (wide, _) = float_unary_operator(Op::WasmF32Unary, tag).expect("f32 selector");
            assert_eq!(format!("{operator:?}"), format!("{wide:?}"));
            tag += 1;
        }
    }
}
