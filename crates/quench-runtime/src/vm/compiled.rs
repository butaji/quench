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
    ControlFlowLayout, FieldLookup, Function, JsDenotation, Operand, OperandKind, ResidualProgram,
    RETURN_REGISTER, WideInstruction,
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
use cranelift_codegen::entity::{SecondaryMap, packed_option::PackedOption};
use std::ops::{Deref, DerefMut};
use std::cell::UnsafeCell;
use std::rc::{Rc, Weak};

#[path = "compiled/osr_loop.rs"]
mod osr_loop;
use crate::typed_ir;
#[path = "compiled/facts.rs"]
mod facts;
#[path = "compiled/lane_region.rs"]
mod lane_region;
#[path = "compiled/lane_unit.rs"]
mod lane_unit;
#[path = "compiled/static_loops.rs"]
mod static_loops;
pub(super) use lane_unit::{CalleeBinding, CallTarget, Frame as InlinedCall};
#[cfg(feature = "aot")]
pub(super) use lane_unit::static_call_targets;
mod object_slots;
mod frame_ssa;
mod numeric_flow;
pub(super) use object_slots::NativeFieldLoad;
#[cfg(feature = "aot")]
mod scalar_region;
#[cfg(feature = "aot")]
pub(super) use scalar_region::lower_regions as lower_static_scalar_regions;
mod wavefront;
#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
mod guard_aarch64;
pub(super) use osr_loop::{
    CALLEE_CAPTURE, THIS_CAPTURE,
    ArrayUse, FieldReceiver, FieldUse, InlineCallee, InlineFrame, InlineGuard, LoopEntry, LoopOutcome, LoopRegion,
    Observation, RegionAnalysisFailure, RegionRejection, Widening,
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
    /// As `Fallback`, at an append whose reserve needs the VM (growth or collection
    /// debt). These lifecycle exits do not mark the region's values as leaving its subset.
    Grow(u32),
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
            Self::Grow(pc) => (4, pc),
        };
        ((kind as u64) << Self::KIND_SHIFT) | payload as u64
    }

    pub(super) const fn decode(word: u64) -> Self {
        let payload = (word & Self::PAYLOAD_MASK) as u32;
        match word >> Self::KIND_SHIFT {
            1 => Self::Resume(payload),
            2 => Self::Fallback(payload),
            3 => Self::Recompile(payload),
            4 => Self::Grow(payload),
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
    /// A tier whose region PCs are not its function's hands off `reported`, its own PC of
    /// the outcome (an inlining loop region); any other reports `outcome` itself.
    pub(super) fn leave(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        outcome: RegionExit,
        reported: RegionExit,
    ) {
        let pc = match outcome {
            RegionExit::Resume(pc) | RegionExit::Fallback(pc) | RegionExit::Recompile(pc) | RegionExit::Grow(pc) => pc,
            RegionExit::EntryMiss(_) => unreachable!("entry miss does not write back"),
        };
        let word = builder.ins().iconst(types::I64, reported.encode() as i64);
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

/// Native primitive view; callers retain operands for float fallback or exact-PC recovery.
struct EmitArithmetic<'a, 'f, const CHECKED: bool> {
    builder: &'a mut FunctionBuilder<'f>,
}

macro_rules! emit_arithmetic_stages {
    ($($operation:ident => $operator:ident, $method:ident, $instruction:ident, $symbol:tt, $float:ident, $checked:ident, $overflow:ident;)+) => {
        impl super::operations::NumberArithmeticStage<ir::Value> for EmitArithmetic<'_, '_, false> {
            type Output = ir::Value;
            $(fn $method(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
                self.builder.ins().$float(left, right)
            })+
        }
        impl super::operations::NumberArithmeticStage<ir::Value> for EmitArithmetic<'_, '_, true> {
            type Output = (ir::Value, ir::Value);
            $(fn $method(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
                emit_arithmetic_stages!(@checked self, left, right, $overflow)
            })+
        }
    };
    (@checked $stage:ident, $left:ident, $right:ident, None) => {
        { let _ = ($left, $right); unreachable!("division has a Float64 result") }
    };
    (@checked $stage:ident, $left:ident, $right:ident, $opcode:ident) => {
        $stage.builder.ins().$opcode($left, $right)
    };
}
super::operations::number_arithmetic_rows!(emit_arithmetic_stages);

/// The low-word view is selected only after the shared operation proves that
/// projecting the Number result commutes with wrapping signed-int32 arithmetic.
struct EmitWord32Arithmetic<'a, 'f> { builder: &'a mut FunctionBuilder<'f> }

macro_rules! emit_word32_arithmetic_stages {
    ($($operation:ident => $operator:ident, $method:ident, $instruction:ident, $symbol:tt, $float:ident, $checked:ident, $overflow:ident;)+) => {
        impl super::operations::NumberArithmeticStage<ir::Value> for EmitWord32Arithmetic<'_, '_> {
            type Output = ir::Value;
            $(fn $method(&mut self, left: ir::Value, right: ir::Value) -> Self::Output {
                emit_word32_arithmetic_stages!(@emit self, left, right, $symbol)
            })+
        }
    };
    (@emit $stage:ident, $left:ident, $right:ident, +) => { $stage.builder.ins().iadd($left, $right) };
    (@emit $stage:ident, $left:ident, $right:ident, -) => { $stage.builder.ins().isub($left, $right) };
    (@emit $stage:ident, $left:ident, $right:ident, *) => { $stage.builder.ins().imul($left, $right) };
    (@emit $stage:ident, $left:ident, $right:ident, /) => {
        { let _ = ($left, $right); unreachable!("division has no wrapping Word32 projection") }
    };
}
super::operations::number_arithmetic_rows!(emit_word32_arithmetic_stages);

pub(super) fn emit_number_arithmetic_operation(
    builder: &mut FunctionBuilder<'_>,
    operation: super::operations::NumberArithmetic,
    left: ir::Value,
    right: ir::Value,
) -> ir::Value {
    operation.apply(&mut EmitArithmetic::<false> { builder }, left, right)
}

/// The shared Number remainder denotation, after the caller's Number guards.
/// Native entries use the host calling convention. This pure scalar helper cannot
/// enter the VM or collect, so it does not introduce an effect/recovery boundary.
pub(super) fn emit_number_remainder(
    builder: &mut FunctionBuilder<'_>,
    pointer: ir::Type,
    left: ir::Value,
    right: ir::Value,
) -> ir::Value {
    let mut signature = Signature::new(builder.func.signature.call_conv);
    signature.params.extend([AbiParam::new(types::F64), AbiParam::new(types::F64)]);
    signature.returns.push(AbiParam::new(types::F64));
    let signature = builder.import_signature(signature);
    let address = super::operations::number_remainder as *const () as usize as i64;
    let target = builder.ins().iconst(pointer, address);
    let call = builder.ins().call_indirect(signature, target, &[left, right]);
    builder.inst_results(call)[0]
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
    registers: &numeric_flow::Bits,
) -> Option<ir::Value> {
    match operand.kind()? {
        OperandKind::Register => {
            let register = operand.register_index()?;
            if !registers.contains(usize::from(register)) { return None; }
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

/// Validate forward numeric control flow in bytecode order. Accepted successors
/// are later PCs, so reachability needs no work queue or predecessor table.
fn js_numeric_control_flow(function: &Function) -> Option<numeric_flow::Bits> {
    let length = function.code.len();
    if length == 0 { return None; }
    let mut reachable = numeric_flow::Bits::new(length);
    reachable.insert(0)?;
    let mut has_branch = false;
    let mut has_local_state = false;
    for (pc, packed) in function.code.iter().copied().enumerate() {
        if !reachable.contains(pc) { continue; }
        if packed.is_wide() { return None; }
        let instruction = packed.as_wide();
        match instruction.op().control_flow_layout() {
            ControlFlowLayout::Jump if instruction.op() == crate::bytecode::Op::Jump => {}
            ControlFlowLayout::ConditionalJump
                if matches!(instruction.op(), crate::bytecode::Op::JumpBinaryFalse
                    | crate::bytecode::Op::JumpFalse | crate::bytecode::Op::JumpTrue) => { has_branch = true; }
            ControlFlowLayout::Terminal | ControlFlowLayout::Fallthrough => {}
            _ => return None,
        }
        has_local_state |= matches!(instruction.op().js_denotation(),
            JsDenotation::PlainLocalLoad | JsDenotation::PlainLocalStore);
        let mut valid = true;
        crate::bytecode::for_each_instruction_successor(pc, instruction, |successor| {
            if successor <= pc || reachable.insert(successor).is_none() { valid = false; }
        });
        if !valid { return None; }
    }
    // Local slots have no SSA join view yet. Every reachable join in this
    // single-entry, forward graph requires a conditional branch.
    if has_branch && has_local_state { return None; }
    Some(reachable)
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
    let isa = super::jit_backend::target_isa("speed")?;
    let native = lower_js_numeric_region(
        program,
        function_id,
        captures,
        numeric_field_inputs,
        int32_bitwise,
        &*isa,
    )?;
    let params = usize::from(program.function_get(function_id as usize)?.params);
    let code = compile_native_function(
        native,
        &*isa,
        super::js_compiled::MAX_CODE_BYTES,
        |code| Some(code.to_vec()),
    )
    .ok()?;
    Some(super::js_compiled::JsJitArtifact {
        code,
        params,
        requires_signed_int32_arguments: int32_bitwise,
        result: super::js_compiled::JsJitResultKind::SharedNumberEntry(NativeEntryMode::Leaf, NativeNumberResult::Float64),
        field_argument,
        numeric_field_inputs: numeric_field_inputs.map_or_else(Vec::new, |inputs| inputs.to_vec()),
        capture_arguments: captures.to_vec(),
        general_dispatch_native_pcs: Vec::new(),
    })
}

/// Bind the numeric JIT ABI to the same semantic SSA specializer used by
/// frameless JIT bodies and linked AOT regions, without a canonical VM frame.
pub(super) fn compile_js_typed_numeric_region(program: &ResidualProgram, function_id: u32)
    -> Option<super::js_compiled::JsJitArtifact> {
    let function = program.function_get(function_id as usize)?;
    let unit = lane_unit::unit(program, function_id, function, &|_, _| Vec::new(), &|_| None)?;
    let isa = super::jit_backend::target_isa("speed")?;
    let region = lane_region::lower(program, function_id, &unit,
        LaneHelpers::Numeric, &std::collections::BTreeMap::new(), &std::collections::BTreeSet::new(), &std::collections::BTreeMap::new(), &[], None, &*isa,
        lane_region::Coverage::Full, lane_region::Completion::Number)?;
    let dump = native_dump_directory().map(|_| region.function.to_string());
    let code = compile_native_function(region.function, &*isa, super::js_compiled::MAX_CODE_BYTES,
        |code| Some(code.to_vec())).ok()?;
    if let (Some(directory), Some(clif)) = (native_dump_directory(), dump) {
        dump_native_bytes(directory, function_id, &code, None, &clif);
    }
    Some(super::js_compiled::JsJitArtifact {
        code, params: usize::from(function.params), requires_signed_int32_arguments: false,
        result: super::js_compiled::JsJitResultKind::SharedNumberEntry(NativeEntryMode::Resumable, region.number_result?),
        field_argument: None, numeric_field_inputs: Vec::new(), capture_arguments: Vec::new(),
        general_dispatch_native_pcs: Vec::new(),
    })
}

/// Residualize numeric semantics without choosing executable-memory or object
/// publication. Callers establish the argument/capture guards before entry.
pub(super) fn lower_js_numeric_region(
    program: &ResidualProgram,
    function_id: u32,
    captures: &[super::js_compiled::JsJitCaptureArgument],
    numeric_field_inputs: Option<&[super::js_compiled::JsJitNumericFieldInput]>,
    int32_bitwise: bool,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
) -> Option<ir::Function> {
    let function = program.function_get(function_id as usize)?;
    let reachable = js_numeric_control_flow(function)?;
    let params = usize::from(function.params);

    let signature = native_entry_signature(isa.pointer_type(), isa.default_call_conv());
    let name = UserFuncName::user(0x5155_454e, function_id);
    let mut native = ir::Function::with_name_signature(name, signature);
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);

    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let blocks: Vec<_> = (0..function.code.len())
        .map(|pc| reachable.contains(pc).then(|| builder.create_block()))
        .collect();
    let parameters = builder.block_params(entry).to_vec();
    let context = parameters[0];
    let arguments = parameters[3];
    let result_pointer = parameters[4];
    let mut body = Body::new(builder, context, None, Int32Extension::Zero);
    let mut registers = numeric_flow::Bits::new(usize::from(function.registers));
    let mut locals = vec![None; usize::from(function.locals)];

    for index in 0..params.min(usize::from(function.registers)) {
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
        registers.insert(index)?;
    }
    let mut incoming = numeric_flow::Incoming::new(function.code.len(), &registers)?;
    body.builder.ins().jump(blocks[0]?, &[]);

    let mut returned = false;
    for (pc, packed) in function.code.iter().copied().enumerate() {
        if !reachable.contains(pc) {
            continue;
        }
        if packed.is_wide() {
            return None;
        }
        let block = blocks[pc]?;
        body.builder.switch_to_block(block);
        body.builder.seal_block(block);
        incoming.read(pc, &mut registers);
        let instruction = packed.as_wide();
        match instruction.op().control_flow_layout() {
            ControlFlowLayout::Jump => {
                let target = usize::try_from(instruction.jump_target()).ok()?;
                incoming.propagate(pc, instruction, &registers);
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
                        if !registers.contains(register) {
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
                incoming.propagate(pc, instruction, &registers);
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
                if !registers.contains(source) {
                    return None;
                }
                Some(body.read(source as u16, crate::WasmType::F64))
            }
            JsDenotation::PlainLocalStore => {
                if instruction.returns_from_frame() || instruction.result_flags() != 0 {
                    return None;
                }
                let source = usize::from(instruction.register_a());
                if !registers.contains(source) {
                    return None;
                }
                let value = body.read(source as u16, crate::WasmType::F64);
                *locals.get_mut(instruction.local_slot())? = Some(value);
                if let Some(register) = instruction.optional_register_b() {
                    body.write(register, crate::WasmType::F64, value);
                    registers.insert(usize::from(register))?;
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
                if !registers.contains(source) {
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
                } else {
                    let operation = super::operations::NumberBitwise::from_js_instruction(
                        instruction.op(),
                        op,
                    )
                    .filter(|operation| {
                        int32_bitwise
                            && operation.in_signed_int32_domain()
                    })?;
                    let left = body.builder.ins().fcvt_to_sint(types::I32, left);
                    let right = body.builder.ins().fcvt_to_sint(types::I32, right);
                    emit_number_bitwise_operation(&mut body.builder, operation, left, right)
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
                if !registers.contains(usize::from(register)) {
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
            registers.insert(destination)?;
        }
        if !terminal {
            if instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough {
                return None;
            }
            let next = pc.checked_add(1)?;
            incoming.propagate(pc, instruction, &registers);
            body.builder.ins().jump(blocks[next]?, &[]);
        }
    }
    if !returned {
        return None;
    }

    body.builder.seal_all_blocks();
    body.into_builder().finalize(isa.frontend_config());
    Some(native)
}

/// Compile a general JavaScript native dispatch loop. It carries the validated
/// bytecode PC in native code and calls the VM's shared `step` implementation
/// for each operation. This complete-semantics bridge lets us move pure
/// denotations into Cranelift without creating a second interpreter.
const MAX_JS_GENERAL_DISPATCH_BLOCKS: usize = 2048;
/// General-dispatch return statuses encode the terminal bytecode PC without
/// colliding with the shared entry's small stop, resume, or trap statuses.
const JS_GENERAL_RETURN_STATUS_BASE: u32 =
    u32::MAX - MAX_JS_GENERAL_DISPATCH_BLOCKS as u32;

const _: () = assert!(JS_GENERAL_RETURN_STATUS_BASE > TERMINATED_STATUS as u32);

pub(super) fn js_general_dispatch_candidate(_program: &ResidualProgram, function: &Function) -> bool {
    js_general_dispatch_shape(function).is_some()
}

/// Preserve OSR as the owner of legacy hot-loop promotion. The generic
/// dispatcher can still compile loops admitted through its ordinary tier path;
/// this predicate only selects the earlier fallback for rejected/profiled
/// legacy candidates.
pub(super) fn js_general_dispatch_acyclic_candidate(
    _program: &ResidualProgram,
    function: &Function,
) -> bool {
    js_general_dispatch_shape(function) == Some(true)
}

/// Whether the general baseline admits `function` (a bounded body whose jumps all have
/// targets) and, if so, whether it is acyclic (every jump goes forward): one pass over
/// the bytecode for both facts.
pub(super) fn js_general_dispatch_shape(function: &Function) -> Option<bool> {
    if function.code.is_empty() || function.code.len() > MAX_JS_GENERAL_DISPATCH_BLOCKS {
        return None;
    }
    let mut acyclic = true;
    for (pc, packed) in function.code.iter().enumerate() {
        let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
        if matches!(instruction.op(), Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse) {
            let target = usize::try_from(instruction.jump_target()).ok()?;
            acyclic &= target > pc;
        }
    }
    Some(acyclic)
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

/// Decode an already-proved Number without repeating its semantic type check.
fn emit_known_number(builder: &mut FunctionBuilder<'_>, bits: ir::Value) -> ir::Value {
    let tag = builder.ins().band_imm_u(bits, crate::value::ValueLayout::TAG_MASK as i64);
    let integer = builder.ins().icmp_imm_u(IntCC::Equal, tag, crate::value::ValueLayout::INT_TAG as i64);
    let payload = builder.ins().ireduce(types::I32, bits);
    let int = builder.ins().fcvt_from_sint(types::F64, payload);
    let double = builder.ins().bitcast(types::F64, MemFlagsData::new(), bits);
    builder.ins().select(integer, int, double)
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

/// An exact int32 projection of a Number, including its zero sign. The predicate
/// rejects fractions, nonfinite values, saturation and -0 before the view is used.
fn emit_js_exact_int32(builder: &mut FunctionBuilder<'_>, number: ir::Value) -> (ir::Value, ir::Value) {
    let integer = builder.ins().fcvt_to_sint_sat(types::I32, number);
    let round_trip = builder.ins().fcvt_from_sint(types::F64, integer);
    let bits = builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), number);
    let round_trip_bits = builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), round_trip);
    let exact = builder.ins().icmp(IntCC::Equal, bits, round_trip_bits);
    (integer, exact)
}

/// Encode a Number result with `Value::number`'s integer, double and NaN
/// representations.
fn emit_js_box_number(builder: &mut FunctionBuilder<'_>, number: ir::Value) -> ir::Value {
    let (truncated, is_integer) = emit_js_exact_int32(builder, number);
    let result_bits = builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), number);
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

/// Keep the integer tier of Quench's shared Number arithmetic denotation in
/// tagged i32 form. Overflow and non-integer representations continue through
/// the same f64 lowering below.
fn emit_js_int32_arithmetic(
    builder: &mut FunctionBuilder<'_>,
    arithmetic: super::operations::NumberArithmetic,
    left_bits: ir::Value,
    right_bits: ir::Value,
    arguments: ir::Value,
    destination: i32,
    next: ir::Block,
    float: ir::Block,
) {
    use super::operations::NumberArithmetic as A;

    let shift = i64::from(crate::value::ValueLayout::TAG_SHIFT);
    let int_tag = (crate::value::ValueLayout::INT_TAG >> shift) as i64;
    let left_tag = builder.ins().ushr_imm_u(left_bits, shift);
    let right_tag = builder.ins().ushr_imm_u(right_bits, shift);
    let left_integer = builder.ins().icmp_imm_u(IntCC::Equal, left_tag, int_tag);
    let right_integer = builder.ins().icmp_imm_u(IntCC::Equal, right_tag, int_tag);
    let both_integer = builder.ins().band(left_integer, right_integer);
    let integer = builder.create_block();
    let result = builder.create_block();
    builder.ins().brif(both_integer, integer, &[], float, &[]);

    builder.switch_to_block(integer);
    let left_narrow = builder.ins().ireduce(types::I32, left_bits);
    let right_narrow = builder.ins().ireduce(types::I32, right_bits);
    let (payload, overflow) = arithmetic.apply(
        &mut EmitArithmetic::<true> { builder }, left_narrow, right_narrow,
    );
    builder.ins().brif(overflow, float, &[], result, &[]);

    builder.switch_to_block(result);
    let payload = builder.ins().uextend(types::I64, payload);
    let integer_bits = builder
        .ins()
        .bor_imm_u(payload, crate::value::ValueLayout::INT_TAG as i64);
    let encoded = if arithmetic == A::Multiply {
        let is_zero = builder.ins().icmp_imm_s(IntCC::Equal, payload, 0);
        let zero_sign = super::operations::int32_zero_product_sign!(
            left_narrow, right_narrow, |l, r| builder.ins().bor(l, r),
            |signs| builder.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0)
        );
        let negative_zero = builder.ins().band(zero_sign, is_zero);
        let minus_zero = builder.ins().iconst(
            types::I64,
            Value::number(-0.0).to_bits() as i64,
        );
        builder.ins().select(negative_zero, minus_zero, integer_bits)
    } else {
        integer_bits
    };
    builder.ins().store(
        MemFlagsData::trusted(),
        encoded,
        arguments,
        destination,
    );
    builder.ins().jump(next, &[]);
}

enum JsCalloutPc {
    Static(usize),
    #[cfg(feature = "aot")]
    Dynamic(ir::Value),
}

impl From<usize> for JsCalloutPc {
    fn from(pc: usize) -> Self { Self::Static(pc) }
}

fn emit_js_step_fallback(
    builder: &mut FunctionBuilder<'_>,
    context: ir::Value,
    callout: ir::Value,
    callout_signature: ir::SigRef,
    result_pointer: ir::Value,
    stopped_block: ir::Block,
    continue_block: ir::Block,
    pc: impl Into<JsCalloutPc>,
) {
    let pc_value = match pc.into() {
        JsCalloutPc::Static(pc) => builder.ins().iconst(types::I64, pc as i64),
        #[cfg(feature = "aot")]
        JsCalloutPc::Dynamic(pc) => pc,
    };
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

/// Continue native code only when one guard holds. All failures share the exact-PC
/// VM fallback emitted after the direct path.
fn emit_js_guard(
    builder: &mut FunctionBuilder<'_>,
    condition: ir::Value,
    fallback: ir::Block,
) {
    let passed = builder.create_block();
    builder.ins().brif(condition, passed, &[], fallback, &[]);
    builder.switch_to_block(passed);
    builder.seal_block(passed);
}

/// Byte offset of a Value element in native address arithmetic.
fn emit_value_index_offset(builder: &mut FunctionBuilder<'_>, index: ir::Value) -> ir::Value {
    let stride = std::mem::size_of::<Value>();
    assert!(stride.is_power_of_two());
    // Widen before scaling: an in-bounds int32 index can have a byte offset
    // larger than u32::MAX. Scaling in the key's type would wrap to another slot.
    let index = match builder.func.dfg.value_type(index) {
        types::I32 => builder.ins().uextend(types::I64, index),
        types::I64 => index,
        _ => unreachable!("Value element indices are i32 or i64"),
    };
    builder.ins().ishl_imm_u(index, i64::from(stride.trailing_zeros()))
}

/// The present element `key` of the dense own-array `base`, both tagged: the record and
/// its current backing reloaded per access, so receiver changes and copy-on-write writes are
/// visible. Any other base, key or a hole leaves through `fallback`, with nothing read.
pub(super) fn emit_dense_element_read(
    builder: &mut FunctionBuilder<'_>,
    payload: ir::Value,
    base: ir::Value,
    key: ir::Value,
    fallback: ir::Block,
) -> ir::Value {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    use crate::value::ValueLayout;

    let family = builder
        .ins()
        .band_imm_u(base, ValueLayout::HEAP_FAMILY_MASK as i64);
    let is_array = builder.ins().icmp_imm_u(
        IntCC::Equal,
        family,
        ValueLayout::DIRECT_ARRAY_TAG as i64,
    );
    emit_js_guard(builder, is_array, fallback);

    let index = builder.ins().ireduce(types::I32, key);
    let key_tag = builder
        .ins()
        .band_imm_u(key, ValueLayout::TAG_MASK as i64);
    let is_integer = builder
        .ins()
        .icmp_imm_u(IntCC::Equal, key_tag, ValueLayout::INT_TAG as i64);
    emit_js_guard(builder, is_integer, fallback);
    let nonnegative = builder
        .ins()
        .icmp_imm_s(IntCC::SignedGreaterThanOrEqual, index, 0);
    emit_js_guard(builder, nonnegative, fallback);

    let record_id = builder.ins().ireduce(types::I32, base);
    let record_id = builder.ins().uextend(types::I64, record_id);
    let record_in_bounds = builder.ins().icmp_imm_u(
        IntCC::UnsignedLessThan,
        record_id,
        i64::try_from(ARRAY.record_capacity).expect("array record capacity fits i64"),
    );
    emit_js_guard(builder, record_in_bounds, fallback);

    let record_base_address = builder.ins().iadd_imm_s(
        payload,
        i64::try_from(std::mem::offset_of!(JsStepDispatch, array_record_base))
            .expect("JS dispatch offset fits i64"),
    );
    let record_base = builder.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        record_base_address,
        0,
    );
    let base_is_present = builder
        .ins()
        .icmp_imm_u(IntCC::NotEqual, record_base, 0);
    emit_js_guard(builder, base_is_present, fallback);

    let record_size = i64::try_from(ARRAY.record_size).expect("array record size fits i64");
    let record_offset = builder.ins().imul_imm_u(record_id, record_size);
    let record = builder.ins().iadd(record_base, record_offset);
    let elements = builder.ins().iadd_imm_s(
        record,
        i64::try_from(ARRAY.elements_offset).expect("array elements offset fits i64"),
    );
    let header = builder.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        elements,
        i32::try_from(ARRAY.elements_header_offset).expect("header pointer offset fits i32"),
    );
    let header_is_present = builder
        .ins()
        .icmp_imm_u(IntCC::NotEqual, header, 0);
    emit_js_guard(builder, header_is_present, fallback);

    let length = builder.ins().load(
        types::I32,
        MemFlagsData::trusted(),
        header,
        i32::try_from(ARRAY.header_length_offset).expect("array length offset fits i32"),
    );
    let index_is_present = builder
        .ins()
        .icmp(IntCC::UnsignedLessThan, index, length);
    emit_js_guard(builder, index_is_present, fallback);

    let value_offset = emit_value_index_offset(builder, index);
    let values = builder.ins().iadd_imm_s(
        header,
        i64::try_from(ARRAY.header_values_offset).expect("array values offset fits i64"),
    );
    let value_address = builder.ins().iadd(values, value_offset);
    let value = builder.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        value_address,
        0,
    );
    let is_present = builder.ins().icmp_imm_u(
        IntCC::NotEqual,
        value,
        Value::DELETED.to_bits() as i64,
    );
    emit_js_guard(builder, is_present, fallback);

    value
}

/// Lower a present dense own-array read. The receiver and key come from the
/// canonical tagged frame on every execution; the record and its current
/// backing are also reloaded per access so receiver changes and COW writes are
/// visible. Every failed proof leaves the destination untouched and resumes
/// `Vm::step` at this GetIndex PC.
#[allow(clippy::too_many_arguments)]
fn emit_js_dense_array_get(
    builder: &mut FunctionBuilder<'_>,
    arguments: ir::Value,
    payload: ir::Value,
    base_offset: i32,
    key: ir::Value,
    destination_offset: i32,
    next: ir::Block,
    context: ir::Value,
    callout: ir::Value,
    callout_signature: ir::SigRef,
    result_pointer: ir::Value,
    stopped_block: ir::Block,
    continue_block: ir::Block,
    pc: usize,
) {
    let fallback = builder.create_block();
    let base = builder.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        arguments,
        base_offset,
    );
    let value = emit_dense_element_read(builder, payload, base, key, fallback);
    builder.ins().store(
        MemFlagsData::trusted(),
        value,
        arguments,
        destination_offset,
    );
    builder.ins().jump(next, &[]);
    builder.switch_to_block(fallback);
    emit_js_step_fallback(
        builder,
        context,
        callout,
        callout_signature,
        result_pointer,
        stopped_block,
        continue_block,
        pc,
    );
}

/// Read a cached arena slot. Own fields require plain receiver storage; reads may instead
/// use an immediate object-record prototype whose exact shape matches
/// the cached holder and whose slot is in the extras arena. These are the same shape proofs
/// used by the VM's own-field and ordinary-holder cache hits. The receiver, holder, cache and
/// arena are read on every execution; every unsupported or invalidated case falls back.
#[derive(Clone, Copy)]
enum JsFieldReceiver {
    Register(i32),
    FixedThis,
    Value(ir::Value),
}

fn js_field_receiver(base: FieldBase, register_offset: &impl Fn(u16) -> Option<i32>) -> Option<JsFieldReceiver> {
    if base == FieldBase::THIS {
        Some(JsFieldReceiver::FixedThis)
    } else {
        base.register_index().and_then(register_offset).map(JsFieldReceiver::Register)
    }
}

fn emit_js_fixed_this(builder: &mut FunctionBuilder<'_>, payload: ir::Value, fallback: ir::Block) -> ir::Value {
    let slot = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, fixed_this_slot));
    let initialized = builder.ins().icmp_imm_u(IntCC::NotEqual, slot, 0);
    emit_js_guard(builder, initialized, fallback);
    builder.ins().load(types::I64, MemFlagsData::trusted(), slot, 0)
}

/// Flat field input bound once for a no-GC, layout-preserving region. Values
/// may change; the field table, arena and record reservation cannot.
#[derive(Clone, Copy)]
struct FieldSlotBinding {
    receiver: ir::Value,
    holder: ir::Value,
    slot: FieldSlot,
    records: ir::Value,
    values: ir::Value,
    length: ir::Value,
}

/// A region binds its slot once; a single operation decodes it only after
/// storage admission so failed packed/shape probes do no slot work.
#[derive(Clone, Copy)]
enum FieldSlot {
    Row(ir::Value),
    #[cfg(feature = "aot")]
    Bound(ir::Value),
}

impl FieldSlot {
    fn read(self, builder: &mut FunctionBuilder<'_>) -> ir::Value {
        match self {
            #[cfg(feature = "aot")]
            Self::Bound(value) => value,
            Self::Row(row) => {
                use super::NATIVE_FIELD_CACHE_LAYOUT as CACHE;
                let stored = builder.ins().load(types::I32, MemFlagsData::trusted(), row, CACHE.slot_offset as i32);
                let slot = builder.ins().bxor_imm_u(stored, i64::from(CACHE.slot_complement));
                let slot = builder.ins().band_imm_u(slot, i64::from(u16::MAX));
                builder.ins().uextend(types::I64, slot)
            }
        }
    }
}

fn emit_js_field_binding(builder: &mut FunctionBuilder<'_>, payload: ir::Value,
    site: u16, miss: ir::Block) -> FieldSlotBinding
{
    use super::NATIVE_FIELD_CACHE_LAYOUT as CACHE;
    let entries = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, field_entries));
    let records = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
    let (values, length) = emit_js_property_values(builder, payload);
    // ObjectHeader owns record-base admission, including reused region proofs.
    for base in [entries, values] {
        let present = builder.ins().icmp_imm_u(IntCC::NotEqual, base, 0);
        emit_js_guard(builder, present, miss);
    }
    let entry = builder.ins().iadd_imm_s(entries,
        i64::try_from(usize::from(site) * CACHE.entry_size).expect("field entry offset"));
    let word = |builder: &mut FunctionBuilder<'_>, offset: usize, complement: u32| {
        let stored = builder.ins().load(types::I32, MemFlagsData::trusted(), entry, offset as i32);
        builder.ins().bxor_imm_u(stored, i64::from(complement))
    };
    let receiver = word(builder, CACHE.receiver_offset, CACHE.receiver_complement);
    let holder = word(builder, CACHE.holder_offset, CACHE.holder_complement);
    FieldSlotBinding { receiver, holder, slot: FieldSlot::Row(entry), records, values, length }
}

// `packed_slot`, when requested, receives a proved own-slot address holding a narrow
// u32. The ordinary return is always an arena address holding a tagged u64. Neither
// projection survives an operation that can change storage or collect the object.
fn emit_js_cached_field_slot(
    builder: &mut FunctionBuilder<'_>,
    arguments: ir::Value,
    payload: ir::Value,
    receiver: JsFieldReceiver,
    site: u16,
    allow_holder: bool,
    fallback: ir::Block,
    binding: Option<FieldSlotBinding>,
    header: Option<object_slots::ObjectHeader>,
    packed_slot: Option<ir::Block>,
) -> (ir::Value, ir::Value) {
    use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
    use super::NATIVE_FIELD_CACHE_LAYOUT as CACHE;

    let base = match receiver {
        JsFieldReceiver::Register(offset) =>
            builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, offset),
        JsFieldReceiver::FixedThis => emit_js_fixed_this(builder, payload, fallback),
        JsFieldReceiver::Value(value) => value,
    };
    let record_id = builder.ins().ireduce(types::I32, base);
    let record_id = builder.ins().uextend(types::I64, record_id);

    let binding = binding.unwrap_or_else(|| emit_js_field_binding(builder, payload, site, fallback));
    let FieldSlotBinding { holder, receiver: receiver_shape, slot, records, values, length: values_len } = binding;
    let own = builder.ins().icmp_imm_u(IntCC::Equal, holder, i64::from(CACHE.own_holder));
    if !allow_holder {
        emit_js_guard(builder, own, fallback);
    }

    let object_slots::ObjectHeader { record: object, shape, inline } = header
        .unwrap_or_else(|| object_slots::ObjectHeader::prove(builder, base, records, None, fallback));
    // The exact shape includes dictionary storage, which cannot match an own-data row.
    let same_shape = builder.ins().icmp(IntCC::Equal, shape, receiver_shape);
    emit_js_guard(builder, same_shape, fallback);
    // A holder read uses only the receiver's shape and prototype link. Its own
    // property representation is unrelated to the holder's slot address.
    let own_storage = |builder: &mut FunctionBuilder<'_>| {
        if let Some(packed_slot) = packed_slot {
            let packed = builder.create_block();
            let arena = builder.create_block();
            builder.ins().brif(inline, packed, &[], arena, &[]);
            builder.switch_to_block(packed);
            let storage = builder.ins().load(types::I64, MemFlagsData::trusted(), object,
                OBJECT.payload_offset as i32);
            let tag = builder.ins().band_imm_u(storage, OBJECT.extras_tag_mask as i64);
            let plain = builder.ins().icmp_imm_u(IntCC::NotEqual, tag, OBJECT.extras_tag as i64);
            emit_js_guard(builder, plain, fallback);
            let slot = slot.read(builder);
            let fits = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, slot,
                OBJECT.inline_property_count as i64);
            emit_js_guard(builder, fits, fallback);
            let offset = builder.ins().imul_imm_u(slot, std::mem::size_of::<u32>() as i64);
            let address = builder.ins().iadd(object, offset);
            let address = builder.ins().iadd_imm_s(address, OBJECT.payload_offset as i64);
            builder.ins().jump(packed_slot, &[ir::BlockArg::Value(address)]);
            builder.seal_block(packed);
            builder.seal_block(arena);
            builder.switch_to_block(arena);
        }
        if packed_slot.is_none() {
            let arena_storage = builder.ins().icmp_imm_u(IntCC::Equal, inline, 0);
            emit_js_guard(builder, arena_storage, fallback);
        }
        // Plain storage has the arena start alone in the payload's lower half.
        let storage = builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            object,
            i32::try_from(OBJECT.payload_offset).expect("payload offset fits i32"),
        );
        let upper = builder.ins().ushr_imm_u(storage, i64::from(u32::BITS));
        let plain = builder.ins().icmp_imm_u(IntCC::Equal, upper, 0);
        emit_js_guard(builder, plain, fallback);
        storage
    };
    let storage = if allow_holder {
        let own_block = builder.create_block();
        let holder_block = builder.create_block();
        let join = builder.create_block();
        builder.append_block_param(join, types::I64);
        builder.ins().brif(own, own_block, &[], holder_block, &[]);
        builder.switch_to_block(own_block);
        let storage = own_storage(builder);
        builder.ins().jump(join, &[ir::BlockArg::Value(storage)]);

        builder.switch_to_block(holder_block);
        let meta = builder.ins().load(types::I32, MemFlagsData::trusted(), object,
            OBJECT.meta_offset as i32);
        let proto = builder.ins().band_imm_u(meta, i64::from(OBJECT.proto_mask));
        let in_start = builder.ins().icmp_imm_u(
            IntCC::UnsignedGreaterThanOrEqual, proto, i64::from(OBJECT.proto_record_base),
        );
        let in_end = builder.ins().icmp_imm_u(
            IntCC::UnsignedLessThan, proto, i64::from(OBJECT.proto_record_end),
        );
        let object_proto = builder.ins().band(in_start, in_end);
        emit_js_guard(builder, object_proto, fallback);
        let proto_id = builder.ins().iadd_imm_s(proto, -i64::from(OBJECT.proto_record_base));
        let proto_id = builder.ins().uextend(types::I64, proto_id);
        let proto_offset = builder.ins().imul_imm_u(
            proto_id, i64::try_from(OBJECT.record_size).expect("object record size fits i64"),
        );
        let prototype = builder.ins().iadd(records, proto_offset);
        let proto_shape = builder.ins().load(
            types::I32, MemFlagsData::trusted(), prototype,
            i32::try_from(OBJECT.auxiliary_offset).expect("auxiliary offset fits i32"),
        );
        let same_holder = builder.ins().icmp(IntCC::Equal, proto_shape, holder);
        emit_js_guard(builder, same_holder, fallback);
        let proto_meta = builder.ins().load(
            types::I32, MemFlagsData::trusted(), prototype,
            i32::try_from(OBJECT.meta_offset).expect("meta offset fits i32"),
        );
        let inline = builder.ins().band_imm_u(proto_meta, i64::from(OBJECT.inline_storage_mask));
        let arena = builder.ins().icmp_imm_u(IntCC::Equal, inline, 0);
        emit_js_guard(builder, arena, fallback);
        let proto_storage = builder.ins().load(
            types::I64, MemFlagsData::trusted(), prototype,
            i32::try_from(OBJECT.payload_offset).expect("payload offset fits i32"),
        );
        let tag = builder.ins().band_imm_u(proto_storage, OBJECT.extras_tag_mask as i64);
        let extras = builder.ins().icmp_imm_u(IntCC::Equal, tag, OBJECT.extras_tag as i64);
        emit_js_guard(builder, extras, fallback);
        let extras_pointer = builder.ins().band_imm_u(proto_storage, !OBJECT.extras_tag_mask as i64);
        let present = builder.ins().icmp_imm_u(IntCC::NotEqual, extras_pointer, 0);
        emit_js_guard(builder, present, fallback);
        let start = builder.ins().load(
            types::I32, MemFlagsData::trusted(), extras_pointer,
            i32::try_from(OBJECT.extras_arena_start_offset).expect("extras offset fits i32"),
        );
        let start = builder.ins().uextend(types::I64, start);
        builder.ins().jump(join, &[ir::BlockArg::Value(start)]);
        builder.seal_block(own_block);
        builder.seal_block(holder_block);
        builder.seal_block(join);
        builder.switch_to_block(join);
        builder.block_params(join)[0]
    } else {
        own_storage(builder)
    };
    let slot = slot.read(builder);
    let position = builder.ins().iadd(storage, slot);
    let in_arena = builder.ins().icmp(IntCC::UnsignedLessThan, position, values_len);
    emit_js_guard(builder, in_arena, fallback);
    let value_offset = builder.ins().imul_imm_u(
        position,
        i64::try_from(OBJECT.property_value_size).expect("property value size fits i64"),
    );
    (builder.ins().iadd(values, value_offset), record_id)
}

/// A word of the running dispatch's payload (`JsStepDispatch`).
fn emit_js_payload_word(builder: &mut FunctionBuilder<'_>, payload: ir::Value, offset: usize) -> ir::Value {
    let offset = i32::try_from(offset).expect("JS dispatch offset fits i32");
    builder.ins().load(types::I64, MemFlagsData::trusted(), payload, offset)
}

fn emit_js_property_values(builder: &mut FunctionBuilder<'_>, payload: ir::Value) -> (ir::Value, ir::Value) {
    let header = builder.ins().load(types::I64, invariant(), payload,
        std::mem::offset_of!(JsStepDispatch, property_buffer) as i32);
    let layout = crate::value_vec::NATIVE_PROPERTY_BUFFER_LAYOUT;
    let values = builder.ins().load(types::I64, MemFlagsData::trusted(), header, layout.base as i32);
    let length = builder.ins().load(types::I64, MemFlagsData::trusted(), header, layout.length as i32);
    (values, length)
}

/// The VM step that resumes at this PC on any failed proof (`emit_js_step_fallback`), in
/// its own block after the direct path.
struct JsExactFallback {
    context: ir::Value,
    callout: ir::Value,
    callout_signature: ir::SigRef,
    result_pointer: ir::Value,
    stopped_block: ir::Block,
    continue_block: ir::Block,
    pc: usize,
}

impl JsExactFallback {
    fn emit(&self, builder: &mut FunctionBuilder<'_>, fallback: ir::Block) {
        builder.switch_to_block(fallback);
        emit_js_step_fallback(
            builder,
            self.context,
            self.callout,
            self.callout_signature,
            self.result_pointer,
            self.stopped_block,
            self.continue_block,
            self.pc,
        );
    }
}

/// Lower a cached data-field read (`emit_js_cached_field_slot`); the destination stays untouched
/// on a failed proof, which resumes `Vm::step` at this GetField PC and may fill the cache.
#[allow(clippy::too_many_arguments)]
fn emit_js_own_field_get(
    builder: &mut FunctionBuilder<'_>,
    arguments: ir::Value,
    payload: ir::Value,
    receiver: JsFieldReceiver,
    site: u16,
    destination_offset: i32,
    next: ir::Block,
    exact: &JsExactFallback,
) {
    let fallback = builder.create_block();
    let (address, _) = emit_js_cached_field_slot(builder, arguments, payload, receiver, site, true, fallback, None, None, None);
    let value = builder.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
    builder.ins().store(MemFlagsData::trusted(), value, arguments, destination_offset);
    builder.ins().jump(next, &[]);
    exact.emit(builder, fallback);
}

/// Lower a returning data-field read: the value `emit_js_cached_field_slot` finds becomes
/// the frame's result as the native `Return` makes it, a requested interrupt first taking
/// the exact-PC callout; any failed proof resumes `Vm::step` at this GetField PC.
fn emit_js_own_field_return(
    builder: &mut RegionCore<'_>,
    arguments: ir::Value,
    payload: ir::Value,
    receiver: JsFieldReceiver,
    site: u16,
    result_pointer: ir::Value,
    exact: &JsExactFallback,
) {
    let fallback = builder.create_block();
    let (address, _) = emit_js_cached_field_slot(builder, arguments, payload, receiver, site, true, fallback, None, None, None);
    let value = builder.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
    emit_js_guarded_return(builder, value, result_pointer, exact.pc, fallback);
    exact.emit(builder, fallback);
}

/// Return a computed value, preserving the exact-PC interruption fallback.
fn emit_js_guarded_return(
    builder: &mut RegionCore<'_>, value: ir::Value, result_pointer: ir::Value,
    pc: usize, fallback: ir::Block,
) {
    let returns = builder.create_block();
    let interrupted = builder.interrupt_requested();
    builder.ins().brif(interrupted, fallback, &[], returns, &[]);
    builder.switch_to_block(returns);
    builder.seal_block(returns);
    builder.ins().store(MemFlagsData::trusted(), value, result_pointer, 0);
    let status = builder.ins().iconst(
        types::I32,
        i64::from(JS_GENERAL_RETURN_STATUS_BASE)
            + i64::try_from(pc).expect("bounded general-dispatch PC"),
    );
    builder.ins().return_(&[status]);
}

/// Call a lane helper (`LaneHelpers`) with the payload's VM and residual first.
fn emit_lane_call(builder: &mut FunctionBuilder<'_>, helper: ir::Value, arguments: &[ir::Value]) -> ir::Value {
    let mut signature = Signature::new(builder.func.signature.call_conv);
    signature.params.extend(arguments.iter().map(|_| AbiParam::new(types::I64)));
    signature.returns.push(AbiParam::new(types::I64));
    let signature = builder.import_signature(signature);
    let call = builder.ins().call_indirect(signature, helper, arguments);
    builder.inst_results(call)[0]
}

fn emit_lane_helper_call(
    builder: &mut FunctionBuilder<'_>,
    payload: ir::Value,
    helper: ir::Value,
    arguments: &[ir::Value],
) -> ir::Value {
    let vm = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, vm));
    let residual = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, residual));
    // All current helpers fit inline; larger future ABIs spill without a limit.
    let mut values = smallvec::SmallVec::<[ir::Value; 8]>::from_slice(&[vm, residual]);
    values.extend_from_slice(arguments);
    emit_lane_call(builder, helper, &values)
}

/// Lower an own data-field store as the lane's store hit makes it
/// (`Vm::primary_own_field_store_hit`, `Vm::cached_field_store_hit`): in specialized
/// execution, of a name no method cache names (whose store would invalidate it), to the slot
/// `emit_js_cached_field_slot` finds. A heap reference is stored only when the owner's mark
/// proves the write barrier would not log it. Every failed proof resumes `Vm::step` at the
/// original store PC with nothing written.
#[allow(clippy::too_many_arguments)]
fn emit_js_own_field_set(
    builder: &mut FunctionBuilder<'_>,
    arguments: ir::Value,
    payload: ir::Value,
    receiver: JsFieldReceiver,
    value: ir::Value,
    atom: Atom,
    site: u16,
    result_destination: Option<i32>,
    next: ir::Block,
    fallback: ir::Block,
    exact: &JsExactFallback,
) {
    let specialized = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, stores_specialized));
    let specialized = builder.ins().icmp_imm_u(IntCC::NotEqual, specialized, 0);
    emit_js_guard(builder, specialized, fallback);
    // The atom's bit among the names method caches hold (`Vm::method_caches_may_name`).
    let word_index = atom / u64::BITS;
    let words = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, method_cache_atoms));
    let words_len = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, method_cache_atom_words));
    let word_present = builder.create_block();
    let name_free = builder.create_block();
    let covered = builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, words_len, i64::from(word_index));
    builder.ins().brif(covered, word_present, &[], name_free, &[]);
    builder.switch_to_block(word_present);
    builder.seal_block(word_present);
    let word = builder.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        words,
        i32::try_from(word_index as usize * std::mem::size_of::<u64>()).expect("atom word offset fits i32"),
    );
    let named = builder.ins().band_imm_u(word, (1_u64 << (atom % u64::BITS)) as i64);
    let unnamed = builder.ins().icmp_imm_u(IntCC::Equal, named, 0);
    builder.ins().brif(unnamed, name_free, &[], fallback, &[]);
    builder.switch_to_block(name_free);
    builder.seal_block(name_free);
    let (address, record_id) =
        emit_js_cached_field_slot(builder, arguments, payload, receiver, site, false, fallback, None, None, None);
    emit_unlogged_property_store_guard(builder, payload, record_id, value, fallback);
    builder.ins().store(MemFlagsData::trusted(), value, address, 0);
    if let Some(destination) = result_destination {
        builder.ins().store(MemFlagsData::trusted(), value, arguments, destination);
    }
    builder.ins().jump(next, &[]);
    exact.emit(builder, fallback);
}

/// The shared RecordSpace::store_logs test: an immediate or unmarked owner
/// needs no barrier. Otherwise the caller must perform the logged store.
fn emit_unlogged_property_store_guard(
    builder: &mut FunctionBuilder<'_>, payload: ir::Value, record_id: ir::Value,
    value: ir::Value, fallback: ir::Block,
) {
    use crate::value::ValueLayout;
    let tag = builder.ins().band_imm_u(value, ValueLayout::TAG_MASK as i64);
    let immediate = builder.ins().icmp_imm_u(IntCC::NotEqual, tag, ValueLayout::HEAP_TAG as i64);
    // A marked owner would be logged by RecordSpace::store_logs; otherwise the
    // barrier is a no-op. Marks and block bases are refreshed after each callout.
    let heap_value = builder.create_block();
    let write = builder.create_block();
    builder.ins().brif(immediate, write, &[], heap_value, &[]);
    builder.switch_to_block(heap_value);
    use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
    let blocks = emit_js_payload_word(
        builder, payload, std::mem::offset_of!(JsStepDispatch, object_mark_blocks),
    );
    let count = emit_js_payload_word(
        builder, payload, std::mem::offset_of!(JsStepDispatch, object_mark_block_count),
    );
    let block = builder.ins().ushr_imm_u(
        record_id, i64::from(OBJECT.mark_records_per_block.trailing_zeros()),
    );
    let block_live = builder.ins().icmp(IntCC::UnsignedLessThan, block, count);
    emit_js_guard(builder, block_live, fallback);
    let block_offset = builder.ins().imul_imm_u(
        block, i64::try_from(OBJECT.mark_block_size).expect("mark block size fits i64"),
    );
    let block_address = builder.ins().iadd(blocks, block_offset);
    let within = builder.ins().band_imm_u(
        record_id, i64::try_from(OBJECT.mark_records_per_block - 1).expect("block mask fits i64"),
    );
    let word_index = builder.ins().ushr_imm_u(
        within, i64::from(OBJECT.mark_word_bits.trailing_zeros()),
    );
    let word_offset = builder.ins().imul_imm_u(
        word_index, i64::try_from(std::mem::size_of::<u64>()).expect("word size fits i64"),
    );
    let word_address = builder.ins().iadd(block_address, word_offset);
    let marks = builder.ins().load(
        types::I64, MemFlagsData::trusted(), word_address,
        i32::try_from(OBJECT.mark_offset).expect("mark offset fits i32"),
    );
    let bit_index = builder.ins().band_imm_u(
        within, i64::try_from(OBJECT.mark_word_bits - 1).expect("word mask fits i64"),
    );
    let one = builder.ins().iconst(types::I64, 1);
    let mask = builder.ins().ishl(one, bit_index);
    let marked = builder.ins().band(marks, mask);
    let unmarked = builder.ins().icmp_imm_u(IntCC::Equal, marked, 0);
    emit_js_guard(builder, unmarked, fallback);
    builder.ins().jump(write, &[]);
    builder.seal_block(heap_value);
    builder.seal_block(write);
    builder.switch_to_block(write);
}

/// Read the VM's atomic cancellation flag at a JS loop backedge. The caller
/// sends a set flag to the exact-PC `Vm::step` callout, which reports the same
/// interruption as the interpreter before consuming the branch.
fn emit_js_interrupt_requested(builder: &mut RegionCore<'_>) -> ir::Value {
    builder.interrupt_requested()
}

/// What a lane body knows of its receiver from entry on: nothing in a body's run allocates
/// or changes a shape (calls and stores that add properties leave it), so the record, exact
/// shape and storage mode found at entry hold for the whole run.
struct LaneReceiver {
    object: Variable,
    shape: Variable,
    inline: Variable,
    properties: Variable,
    properties_len: Variable,
}

/// Decode an already proved narrow integer without a process-local lookup table.
fn emit_narrow_integer_read(builder: &mut FunctionBuilder<'_>, narrow: ir::Value) -> ir::Value {
    let value = builder.ins().sshr_imm_u(narrow, i64::from(crate::value::NarrowLayout::CODE_SHIFT));
    let value = builder.ins().uextend(types::I64, value);
    builder.ins().bor_imm_u(value, crate::value::ValueLayout::INT_TAG as i64)
}

/// Encode a proved tagged int32; widening remains the shared store's responsibility.
fn emit_narrow_integer_write(builder: &mut FunctionBuilder<'_>, value: ir::Value,
    fallback: ir::Block) -> ir::Value {
    use crate::value::NarrowLayout as NARROW;
    let int = builder.ins().ireduce(types::I32, value);
    let half_range = 1_i64 << (NARROW::INTEGER_BITS - 1);
    let biased = builder.ins().iadd_imm_s(int, half_range);
    let fits = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, biased, half_range << 1);
    emit_js_guard(builder, fits, fallback);
    let shifted = builder.ins().ishl_imm_u(int, i64::from(NARROW::CODE_SHIFT));
    builder.ins().bor_imm_u(shifted, i64::from(NARROW::INTEGER_FLAG))
}

/// Decode a packed property using the canonical NarrowLayout, transferring
/// its tagged value to `join`'s sole parameter. Proof lifetimes belong to callers.
fn emit_packed_object_slot_read(
    builder: &mut FunctionBuilder<'_>,
    payload: ir::Value,
    slot: u16,
    join: ir::Block,
) {
    debug_assert!(usize::from(slot) < crate::heap::NATIVE_OBJECT_LAYOUT.inline_property_count);
    let half = builder.ins().ushr_imm_u(payload, i64::from(slot) * i64::from(u32::BITS));
    let narrow = builder.ins().ireduce(types::I32, half);
    let integer = builder.ins().band_imm_u(narrow, i64::from(crate::value::NarrowLayout::INTEGER_FLAG));
    let integer_block = builder.create_block();
    let reference_block = builder.create_block();
    builder.ins().brif(integer, integer_block, &[], reference_block, &[]);
    builder.switch_to_block(integer_block);
    builder.seal_block(integer_block);
    let encoded = emit_narrow_integer_read(builder, narrow);
    builder.ins().jump(join, &[ir::BlockArg::Value(encoded)]);
    builder.switch_to_block(reference_block);
    builder.seal_block(reference_block);
    let code = builder.ins().ushr_imm_u(narrow, i64::from(crate::value::NarrowLayout::CODE_SHIFT));
    let code = builder.ins().band_imm_u(code, i64::from(crate::value::NarrowLayout::CODE_MASK));
    let code = builder.ins().uextend(types::I64, code);
    let offset = builder.ins().ishl_imm_u(code, i64::from(std::mem::size_of::<u64>().trailing_zeros()));
    let table = builder.ins().iconst(types::I64, crate::value::NarrowLayout::upper_words() as i64);
    let address = builder.ins().iadd(table, offset);
    let upper = builder.ins().load(types::I64, invariant(), address, 0);
    let lower = builder.ins().ushr_imm_u(narrow, i64::from(crate::value::NarrowLayout::INDEX_SHIFT));
    let lower = builder.ins().uextend(types::I64, lower);
    let encoded = builder.ins().bor(upper, lower);
    builder.ins().jump(join, &[ir::BlockArg::Value(encoded)]);
}

/// Establish a lane body's `LaneReceiver` once, at entry: `NO_FIELD_RECEIVER` as its shape
/// where the receiver is no direct object or the heap bases are absent, which no folded
/// field matches.
fn emit_lane_receiver(builder: &mut FunctionBuilder<'_>, payload: ir::Value, this: ir::Value) -> LaneReceiver {
    use crate::value::ValueLayout;
    let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
    let receiver = LaneReceiver {
        object: builder.declare_var(types::I64),
        shape: builder.declare_var(types::I32),
        inline: builder.declare_var(types::I32),
        properties: builder.declare_var(types::I64),
        properties_len: builder.declare_var(types::I64),
    };
    let records = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
    let (properties, properties_len) = emit_js_property_values(builder, payload);
    builder.def_var(receiver.properties, properties);
    builder.def_var(receiver.properties_len, properties_len);
    let none = builder.ins().iconst(types::I32, i64::from(super::NO_FIELD_RECEIVER));
    let zero = builder.ins().iconst(types::I32, 0);
    let null = builder.ins().iconst(types::I64, 0);
    builder.def_var(receiver.shape, none);
    builder.def_var(receiver.inline, zero);
    builder.def_var(receiver.object, null);
    let found = builder.create_block();
    let done = builder.create_block();
    let family = builder.ins().band_imm_u(this, ValueLayout::HEAP_FAMILY_MASK as i64);
    let object = builder.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_OBJECT_TAG as i64);
    let record_id = builder.ins().ireduce(types::I32, this);
    let in_range = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, record_id, layout.record_capacity as i64);
    let has_records = builder.ins().icmp_imm_u(IntCC::NotEqual, records, 0);
    let has_properties = builder.ins().icmp_imm_u(IntCC::NotEqual, properties, 0);
    let ok = builder.ins().band(object, in_range);
    let ok = builder.ins().band(ok, has_records);
    let ok = builder.ins().band(ok, has_properties);
    builder.ins().brif(ok, found, &[], done, &[]);
    builder.switch_to_block(found);
    let record_id = builder.ins().uextend(types::I64, record_id);
    let offset = builder.ins().imul_imm_u(record_id, layout.record_size as i64);
    let address = builder.ins().iadd(records, offset);
    // The exact shape carries the dictionary bit, so a dictionary never matches.
    let shape = builder.ins().load(types::I32, MemFlagsData::trusted(), address, layout.auxiliary_offset as i32);
    let meta = builder.ins().load(types::I32, MemFlagsData::trusted(), address, layout.meta_offset as i32);
    let inline = builder.ins().band_imm_u(meta, i64::from(layout.inline_storage_mask));
    builder.def_var(receiver.object, address);
    builder.def_var(receiver.shape, shape);
    builder.def_var(receiver.inline, inline);
    builder.ins().jump(done, &[]);
    builder.switch_to_block(done);
    receiver
}

/// The host-specific semantic helpers a lane body calls where its native path does not
/// apply: the VM's own field-cache read and its logged own-field store, each the rule the
/// lane's handlers run (`Vm::cached_field_hit`, `Vm::primary_own_field_store_hit`).
macro_rules! lane_helper_inventory {
    ($host:ident; $($name:ident => $address:expr),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum LaneHelper { $($name),+ }
        impl LaneHelper {
            const COUNT: usize = [$(stringify!($name)),+].len();
            fn addresses<$host: Host>() -> &'static [*const (); Self::COUNT] {
                &[$($address as *const ()),+]
            }
        }
    };
}
lane_helper_inventory! { H;
    FunctionCapture => lane_function_capture_read::<H>,
    FieldGet => lane_field_get::<H>,
    FieldSet => lane_field_set::<H>,
    Call => super::dispatch_fast::lane_body_call::<H>,
    IndexSet => lane_index_set::<H>,
    GuardedCall => lane_guarded_call::<H>,
    FunctionBinding => lane_function_binding::<H>,
    MaterializeInline => lane_materialize_inline::<H>,
    InterruptOwner => lane_interrupt_owner,
    GlobalGet => lane_global_get::<H>,
}

unsafe extern "C" fn lane_interrupt_owner(context: u64) -> u64 {
    // SAFETY: the running entry binds this owner for its whole native call.
    let context = unsafe { &*(context as *const CallContext) };
    u64::from(unsafe { context.interrupt_owner.as_ref() }.is_some_and(|owner| owner.exclusive_and_clear()))
}

/// Recovery is the first operation allowed to allocate a full callee frame.
/// The native prefix has published both register files before entering here;
/// ordinary activation setup cannot collect or invoke guest code for these
/// verified plain callees with an already admitted object receiver.
unsafe extern "C" fn lane_materialize_inline<H: Host>(
    vm: u64, residual: u64, payload: u64, function: u64, call_pc: u64,
    source_pc: u64, registers: u64,
) -> u64 {
    #[cfg(test)]
    INLINE_MATERIALIZATIONS.with(|count| count.set(count.get() + 1));
    #[cfg(test)]
    INLINE_LAST_MATERIALIZATION.with(|site| site.set(Some((function as u32, source_pc as u32))));
    // SAFETY: the native entry owns all three bindings through this call.
    let (vm, program, dispatch) = unsafe {
        (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram),
            &mut *(payload as *mut JsStepDispatch))
    };
    let caller = dispatch.frame;
    let call_pc = call_pc as usize;
    let function = function as u32;
    let body = unsafe { &*dispatch.body };
    let instruction = super::dispatch_fast::decoded_instruction(body, body.code[call_pc]);
    let callee = vm.read(caller, instruction.register_b());
    let receiver = vm.read(caller, instruction.register_c());
    let env = vm.heap.function(callee).expect("admitted inline closure").env;
    let passed: *const [Value] = vm.register_window(caller, instruction.call_window());
    vm.frames[caller].pc = call_pc + 1;
    vm.frames[caller].pc_is_resume = true;
    // SAFETY: setup only pushes a boxed frame; the caller's window is stable.
    let result = vm.push_general_user_frame(program, function, env, receiver,
        unsafe { &*passed }, super::CallContext::user_function(function, callee))
        .map(|call_guard| {
            let frame = vm.frames.last_mut().expect("recovered callee frame");
            // SAFETY: the compiled snapshot contains this verified body's register count.
            let snapshot = unsafe { std::slice::from_raw_parts(registers as *const Value, frame.registers.len()) };
            frame.registers.copy_from_slice(snapshot);
            frame.pc = source_pc as usize;
            frame.pc_is_resume = true;
            StepResult::PushFrame { destination: instruction.result_register(), call_guard,
                completion: super::CallCompletion::Value }
        });
    dispatch.outcome = Some(JsNativeStep { instruction_pc: call_pc, next_pc: call_pc + 1, result });
    0
}

/// A typed residual has materialized its exact callee continuation in the
/// dispatch payload rather than returning another source PC to execute.
const JS_INLINE_MATERIALIZED_PC: u32 = u32::MAX;

#[cfg(test)]
thread_local! {
    static FIELD_HELPER_CALLS: std::cell::Cell<[usize; 2]> = const { std::cell::Cell::new([0; 2]) };
    static INLINE_MATERIALIZATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static INLINE_LAST_MATERIALIZATION: std::cell::Cell<Option<(u32, u32)>> = const { std::cell::Cell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LaneHelpers {
    Numeric,
    Process([usize; LaneHelper::COUNT]),
    #[cfg(feature = "aot")]
    Linked(u32),
    #[cfg(feature = "aot")]
    Bound(ir::Value),
}
impl LaneHelpers {
    fn address<'f>(self, builder: impl ir::InstBuilder<'f>, helper: LaneHelper) -> ir::Value {
        match self {
            Self::Numeric => match helper {
                LaneHelper::InterruptOwner => builder.iconst(types::I64, lane_interrupt_owner as *const () as usize as i64),
                _ => unreachable!("frameless Number regions have no VM helper capability"),
            },
            Self::Process(addresses) => builder.iconst(types::I64, addresses[helper as usize] as i64),
            #[cfg(feature = "aot")]
            Self::Bound(table) => {
                builder.load(types::I64, invariant(), table,
                    (helper as usize * std::mem::size_of::<usize>()) as i32)
            },
            #[cfg(feature = "aot")]
            Self::Linked(_) => unreachable!("linked helpers bind at region entry"),
        }
    }
}

/// Immutable host-specific helper addresses, resolved by the running image.
/// The table and every code pointer have process lifetime; obtaining it has no effects.
#[cfg(feature = "native-execution")]
pub fn native_js_lane_helpers<H: Host>() -> *const () {
    LaneHelper::addresses::<H>().as_ptr().cast()
}

#[cfg(feature = "aot")]
pub(super) fn native_lane_helpers_signature(isa: &dyn cranelift_codegen::isa::TargetIsa) -> ir::Signature {
    let mut signature = Signature::new(isa.default_call_conv());
    signature.returns.push(AbiParam::new(isa.pointer_type()));
    signature
}

#[derive(Clone, Copy)]
struct NullishComparison {
    source: Register,
    expected: Value,
    strict: bool,
    negated: bool,
}

impl NullishComparison {
    fn known_outcome(self, ty: typed_ir::Type) -> Option<bool> {
        use typed_ir::Type;
        let equal = if ty.is_subtype(Type::language(crate::ValueType::Null)) {
            !self.strict || self.expected == Value::NULL
        } else if ty.is_subtype(Type::language(crate::ValueType::Undefined)) {
            !self.strict || self.expected == Value::UNDEFINED
        } else if ty.is_subtype(Type::NUMBER.join(Type::language(crate::ValueType::Boolean))) {
            false
        } else {
            return None;
        };
        Some(if self.negated { equal } else { !equal })
    }
}

/// Resolve the constant operand and equality denotation before execution.
fn nullish_comparison(constants: &[crate::bytecode::Constant], instruction: WideInstruction,
    register_type: impl Fn(Register) -> typed_ir::Type) -> Option<NullishComparison> {
    if instruction.op() != Op::JumpBinaryFalse { return None; }
    let nullish = |operand: Operand| {
        let ty = facts::operand_type(constants, operand, &register_type);
        if ty == typed_ir::Type::language(crate::ValueType::Null) { Some(Value::NULL) }
        else if ty == typed_ir::Type::language(crate::ValueType::Undefined) { Some(Value::UNDEFINED) }
        else { None }
    };
    let (left, right) = (instruction.operand_b(), instruction.operand_c());
    let (expected, source) = nullish(left).zip(right.register_index())
        .or_else(|| nullish(right).zip(left.register_index()))?;
    let (strict, negated) = super::operations::equality_operator(instruction.binary_operator_field())?;
    Some(NullishComparison { source, expected, strict, negated })
}

/// Whether JumpBinaryFalse takes its target, preserving IsHTMLDDA fallback.
fn emit_nullish_branch(builder: &mut FunctionBuilder<'_>, value: ir::Value,
    comparison: NullishComparison, fallback: ir::Block) -> ir::Value {
    use crate::value::ValueLayout;
    let equal = if comparison.strict {
        builder.ins().icmp_imm_u(IntCC::Equal, value, comparison.expected.to_bits() as i64)
    } else {
        let tag = builder.ins().band_imm_u(value, ValueLayout::TAG_MASK as i64);
        let immediate = builder.ins().icmp_imm_u(IntCC::NotEqual, tag, ValueLayout::HEAP_TAG as i64);
        let family = builder.ins().band_imm_u(value, ValueLayout::HEAP_FAMILY_MASK as i64);
        let object = builder.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_OBJECT_TAG as i64);
        let array = builder.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_ARRAY_TAG as i64);
        let record = builder.ins().bor(object, array);
        let safe = builder.ins().bor(immediate, record);
        emit_js_guard(builder, safe, fallback);
        let null = builder.ins().icmp_imm_u(IntCC::Equal, value, Value::NULL.to_bits() as i64);
        let undefined = builder.ins().icmp_imm_u(IntCC::Equal, value, Value::UNDEFINED.to_bits() as i64);
        builder.ins().bor(null, undefined)
    };
    if comparison.negated { equal } else { builder.ins().icmp_imm_u(IntCC::Equal, equal, 0) }
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
    NullishFalse(NullishComparison),
}

#[cfg(feature = "aot")]
pub(super) struct StaticTypedRegion {
    pub(super) header: usize,
    pub(super) function: ir::Function,
}

#[cfg(feature = "aot")]
pub(super) fn lower_static_typed_regions(program: &ResidualProgram, function_id: u32, targets: &[Option<u32>],
    isa: &dyn cranelift_codegen::isa::TargetIsa, helper_symbol: u32) -> Vec<StaticTypedRegion>
{
    if isa.pointer_type() != types::I64 { return Vec::new(); }
    let Some(function) = program.function_get(function_id as usize) else { return Vec::new(); };
    if function_id == 0 || !js_general_dispatch_candidate(program, function) { return Vec::new(); }
    // Source-PC recovery materializes one callee frame. Static regions inline
    // root calls of leaves; the same unit owns all origins and recovery masks.
    let leaf = |function: u32| program.function_get(function as usize)
        .is_some_and(|body| body.instructions().all(|instruction| instruction.op() != Op::Call));
    let Some(unit) = lane_unit::unit(program, function_id, function, &|caller, pc| (caller == function_id)
        .then(|| targets.get(pc as usize).copied().flatten()).flatten().filter(|callee| leaf(*callee))
        .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
        &|id| program.function_get(id as usize)) else { return Vec::new(); };
    let mut headers = smallvec::SmallVec::<[usize; 4]>::new();
    for (pc, instruction) in unit.instructions.iter().enumerate() {
        if matches!(instruction.op().control_flow_layout(), ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump) {
            let target = instruction.jump_target() as usize;
            if target <= pc && matches!(unit.origins.get(target), Some(lane_unit::Origin::Caller(_))) {
                headers.push(target);
            }
        }
    }
    headers.sort_unstable();
    headers.dedup();
    let has_calls = unit.instructions.iter().any(|instruction|
        instruction.op().control_flow_layout() == ControlFlowLayout::Call);
    let scalar = !has_calls && unit.instructions.iter().all(|instruction|
        instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary
            || matches!(instruction.op(), Op::Nop | Op::LoadConst | Op::Move | Op::IncDec
                | Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse | Op::Return)
            || (instruction.op() == Op::LoadCapture && instruction.result_flags() == 0 && instruction.capture_depth() == 0));
    let lower = |coverage: lane_region::Coverage| {
        let entry = coverage.entry();
        let lane_unit::Origin::Caller(header) = unit.origins[entry] else { return None; };
        lane_region::lower(program, function_id, &unit, LaneHelpers::Linked(helper_symbol),
            &std::collections::BTreeMap::new(), &std::collections::BTreeSet::new(), &std::collections::BTreeMap::new(), &[], None, isa,
            coverage, lane_region::Completion::SourcePc).filter(|region| region.native != 0)
            .map(|mut region| {
                region.function.name = UserFuncName::user(lane_region::LANE_REGION_NAMESPACE, header);
                StaticTypedRegion { header: header as usize, function: region.function }
            })
    };
    // Preserve pure preheaders in the SSA graph. Otherwise, first try the
    // complete region previously admitted by the whole-suffix contract.
    if scalar && !headers.is_empty() && let Some(region) = lower(lane_region::Coverage::Full) {
        return vec![region];
    }
    if let Some(header) = headers.first().copied().or_else(|| (has_calls || !unit.frames.is_empty()).then_some(0))
        && let Some(region) = lower(lane_region::Coverage::FullFrom(header)) {
        return vec![region];
    }
    // Independent loop entries share the original unit PC/frame IDs. Prefix
    // lowering stops before an opaque operation and returns its exact origin;
    // neither initialization nor a preceding loop is replayed on re-entry.
    headers.into_iter().filter_map(|header| lower(lane_region::Coverage::PrefixFrom(header))).collect()
}

#[cfg_attr(not(feature = "aot"), allow(dead_code))]
#[derive(Clone, Copy)]
pub(super) struct StaticTypedRegionEntry<'a> {
    pub(super) header: usize,
    pub(super) function: &'a ir::Function,
}

pub(super) fn compile_js_general_dispatch(
    program: &ResidualProgram,
    function_id: u32,
    unresolved_names_are_global: bool,
) -> Result<super::js_compiled::JsJitArtifact, &'static str> {
    let isa = super::jit_backend::target_isa("speed")
        .ok_or("target-isa-unavailable")?;
    let (native, general_dispatch_native_pcs) = lower_js_general_dispatch(
        program, function_id, unresolved_names_are_global, &*isa,
        JsGeneralCallout::Dynamic, &[], &[],
    )?;
    let function = program.function_get(function_id as usize).ok_or("missing-function")?;
    let code = compile_native_function(
        native,
        &*isa,
        super::js_compiled::MAX_CODE_BYTES,
        |code| Some(code.to_vec()),
    )
    .map_err(NativeCompileRejection::reason)?;
    Ok(super::js_compiled::JsJitArtifact {
        code,
        params: usize::from(function.params),
        requires_signed_int32_arguments: false,
        result: super::js_compiled::JsJitResultKind::GeneralDispatch,
        field_argument: None,
        numeric_field_inputs: Vec::new(),
        capture_arguments: Vec::new(),
        general_dispatch_native_pcs,
    })
}

#[derive(Clone, Copy)]
pub(super) enum JsGeneralCallout {
    Dynamic,
    #[cfg(feature = "aot")]
    Relocatable { fallback: u32 },
    #[cfg(all(feature = "aot", test))]
    Address(usize),
}

impl JsGeneralCallout {
    #[cfg(feature = "aot")]
    fn fixed_operations(self) -> bool { !matches!(self, Self::Dynamic) }
}

#[cfg(feature = "aot")]
pub(super) enum NativeStopControl { Unbounded, Unproven }

/// A relocatable typed region and its guarded entry inputs. JIT general
/// dispatch supplies no static regions; its hot region publisher owns entry.
#[cfg_attr(not(feature = "aot"), allow(dead_code))]
pub(super) struct StaticRegionEntry<'a> {
    pub(super) range: std::ops::Range<usize>,
    pub(super) target: StaticRegionTarget,
    pub(super) fields: &'a [FieldUse],
    /// Complete canonical register footprint of the verified no-GC residual.
    pub(super) frame: &'a [Register],
}

#[cfg_attr(not(feature = "aot"), allow(dead_code))]
#[derive(Clone, Copy)]
pub(super) enum StaticRegionTarget {
    Symbol(u32),
    #[cfg(test)]
    Address(usize),
}

pub(super) fn lower_js_general_dispatch(
    program: &ResidualProgram,
    function_id: u32,
    unresolved_names_are_global: bool,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    callout_kind: JsGeneralCallout,
    _scalar_regions: &[StaticRegionEntry<'_>],
    _typed_regions: &[StaticTypedRegionEntry<'_>],
) -> Result<(ir::Function, Vec<u8>), &'static str> {
    if program.kind == crate::bytecode::ProgramKind::Wasm || function_id == 0 {
        return Err("ineligible-program-or-root-function");
    }
    if isa.pointer_type() != types::I64 {
        return Err("general-dispatch-requires-64-bit-pointers");
    }
    #[cfg(feature = "aot")]
    if !_typed_regions.is_empty() && !callout_kind.fixed_operations() {
        return Err("typed-region-requires-fixed-native-control");
    }
    let function = program
        .function_get(function_id as usize)
        .ok_or("missing-function")?;
    let static_global_names = unresolved_names_are_global
        && function.names_resolve_statically(program);
    let instruction_count = function.code.len();
    if !js_general_dispatch_candidate(program, function) {
        return Err("bytecode-candidate-rejected");
    }

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
    let mut builder = RegionCore::new(builder, context, None);
    let locals = parameters[1];
    let local_count = parameters[2];
    let arguments = parameters[3];
    let result_pointer = parameters[4];
    let stopped_block = builder.create_block();
    let continue_block = builder.create_block();
    let continue_pc = builder.append_block_param(continue_block, types::I64);
    #[cfg(feature = "aot")]
    let typed_continue = (!_typed_regions.is_empty()).then(|| {
        let block = builder.create_block();
        let pc = builder.append_block_param(block, types::I64);
        (block, pc)
    });
    let invalid_pc = builder.create_block();
    let invalid_pc_value = builder.append_block_param(invalid_pc, types::I64);
    let callout = match callout_kind {
        JsGeneralCallout::Dynamic => builder
            .ins()
            .load(types::I64, invariant(), context, CONTEXT_CALLOUT),
        #[cfg(feature = "aot")]
        JsGeneralCallout::Relocatable { fallback: symbol, .. } => {
            let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
                super::native_object::NATIVE_FUNCTION_NAMESPACE, symbol,
            ));
            let address = builder.create_global_value(ir::GlobalValueData::Symbol {
                name: ir::ExternalName::user(name),
                offset: ir::immediates::Imm64::new(0),
                colocated: false,
                tls: false,
            });
            builder.ins().symbol_value(isa.pointer_type(), address)
        }
        #[cfg(all(feature = "aot", test))]
        JsGeneralCallout::Address(address) => builder.ins().iconst(isa.pointer_type(), address as i64),
    };
    let blocks: Vec<_> = (0..instruction_count)
        .map(|_| builder.create_block())
        .collect();
    let entries = std::borrow::Cow::Borrowed(blocks.as_slice());
    #[cfg(feature = "aot")]
    let (entries, typed_entries) = {
        let mut entries = entries;
        for entry in _scalar_regions {
            entries.to_mut()[entry.range.start] = builder.create_block();
        }
        let typed_entries: Vec<_> = _typed_regions.iter().copied().map(|unit| {
            let fallback = entries[unit.header];
            entries.to_mut()[unit.header] = builder.create_block();
            (unit, fallback)
        }).collect();
        (entries, typed_entries)
    };
    let mut general_dispatch_native_pcs = vec![0; instruction_count];
    // Start from the caller's exact resume PC. The artifact is shared by normal
    // function entry, call-return re-entry and hot-loop OSR; its canonical frame
    // homes remain the same in all three cases.
    let payload = builder.ins().load(
        types::I64,
        invariant(),
        context,
        std::mem::offset_of!(CallContext, adapter_payload) as i32,
    );
    let start_pc_address = builder.ins().iadd_imm_s(
        payload,
        std::mem::offset_of!(JsStepDispatch, start_pc) as i64,
    );
    let start_pc = builder
        .ins()
        .load(types::I64, MemFlagsData::trusted(), start_pc_address, 0);
    let mut callout_signature = Signature::new(isa.default_call_conv());
    for _ in 0..4 {
        callout_signature.params.push(AbiParam::new(types::I64));
    }
    callout_signature.returns.push(AbiParam::new(types::I64));
    let callout_signature = builder.import_signature(callout_signature);
    let mut frame_flow = frame_ssa::FrameFlow::new(arguments, function.registers);
    #[cfg(feature = "aot")]
    let control = if callout_kind.fixed_operations() {
        // The bound belongs to this activation and never changes through a
        // semantic callout. Divide once: normal native code is unbounded;
        // bounded execution uses the same fixed semantic operations.
        let stop_pc = builder.ins().load(types::I64, invariant(), payload,
            std::mem::offset_of!(JsStepDispatch, stop_pc) as i32);
        let unbounded = builder.ins().icmp_imm_u(IntCC::Equal, stop_pc, JS_NO_STOP_PC as i64);
        let bounded = builder.create_block();
        let pc = builder.append_block_param(bounded, types::I64);
        builder.ins().brif(unbounded, continue_block, &[ir::BlockArg::Value(start_pc)],
            bounded, &[ir::BlockArg::Value(start_pc)]);
        builder.switch_to_block(bounded);
        let run = builder.create_block();
        let in_range = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, pc, instruction_count as i64);
        builder.ins().brif(in_range, run, &[], invalid_pc, &[ir::BlockArg::Value(pc)]);
        builder.switch_to_block(run);
        emit_js_step_fallback(&mut builder, context, callout, callout_signature,
            result_pointer, stopped_block, bounded, JsCalloutPc::Dynamic(pc));
        NativeStopControl::Unbounded
    } else {
        builder.ins().jump(continue_block, &[ir::BlockArg::Value(start_pc)]);
        NativeStopControl::Unproven
    };
    #[cfg(not(feature = "aot"))]
    builder.ins().jump(continue_block, &[ir::BlockArg::Value(start_pc)]);
    builder.switch_to_block(stopped_block);
    // A nonzero callout status has a rooted `JsStepDispatch` result in the
    // adapter payload. The status itself only tells the outer entry to stop.
    let stopped = builder.ins().iconst(types::I32, i64::from(NATIVE_JS_STOPPED));
    builder.ins().return_(&[stopped]);

    builder.switch_to_block(invalid_pc);
    builder.ins().store(
        MemFlagsData::trusted(),
        invalid_pc_value,
        result_pointer,
        0,
    );
    let out_of_range = builder.ins().iconst(types::I32, i64::from(NATIVE_JS_INVALID_PC));
    builder.ins().return_(&[out_of_range]);

    let register_offset = |register: u16| -> Option<i32> {
        if register >= function.registers {
            return None;
        }
        i32::try_from(usize::from(register).checked_mul(std::mem::size_of::<u64>())?).ok()
    };

    let resume_entries: Vec<_> = (0..instruction_count).map(|_| builder.create_block()).collect();
    for (pc, block) in blocks.iter().copied().enumerate() {
        builder.switch_to_block(block);
        let packed = function.code[pc];
        let instruction = super::dispatch_fast::decoded_instruction(function, packed);
        let next = entries.get(pc + 1).copied();
        if instruction.op().js_denotation() == JsDenotation::Return
            && instruction.result_flags() == 0
            && !instruction.writes_current_this()
            && let Some(source) = register_offset(instruction.register_a())
        {
            let fallback = builder.create_block();
            let resume = builder.create_block();
            let interrupted = emit_js_interrupt_requested(&mut builder);
            builder.ins().brif(interrupted, fallback, &[], resume, &[]);
            builder.switch_to_block(resume);
            let value = builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                arguments,
                source,
            );
            builder
                .ins()
                .store(MemFlagsData::trusted(), value, result_pointer, 0);
            let status = builder.ins().iconst(
                types::I32,
                i64::from(JS_GENERAL_RETURN_STATUS_BASE)
                    + i64::try_from(pc).expect("bounded general-dispatch PC"),
            );
            builder.ins().return_(&[status]);
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
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }
        if static_global_names
            && matches!(instruction.op(), Op::LoadName | Op::LoadNameCall | Op::LoadNameTypeof)
            && instruction.result_flags() == 0
            && !instruction.returns_from_frame()
            && !instruction.writes_current_this()
            && program
                .atoms
                .get(instruction.atom_index() as usize)
                .is_some_and(|name| {
                    !name.starts_with('\0') && !crate::bytecode::is_intrinsic_name(name)
                })
            && next.is_some()
        {
            // The shared callout takes a cached data-property hit where the
            // global environment proves it side-effect-free; every lexical,
            // accessor, and miss case resumes Vm::step at this exact PC.
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
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }
        if instruction.op() == Op::GetIndex
            && instruction.result_flags() == 0
            && !instruction.returns_from_frame()
            && !instruction.writes_current_this()
            && instruction.numeric_local_target().is_none()
            && let Some(next) = next
            && let Some(base_offset) = instruction
                .operand_b()
                .register_index()
                .and_then(register_offset)
            && js_tagged_operand_supported(program, instruction.operand_c(), &register_offset)
            && let Some(destination_offset) = register_offset(instruction.result_register())
        {
            let key = read_js_tagged_operand(
                &mut builder,
                program,
                instruction.operand_c(),
                arguments,
                &register_offset,
            )
            .expect("prevalidated tagged GetIndex key");
            emit_js_dense_array_get(
                &mut builder,
                arguments,
                payload,
                base_offset,
                key,
                destination_offset,
                next,
                context,
                callout,
                callout_signature,
                result_pointer,
                stopped_block,
                continue_block,
                pc,
            );
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }
        // A fused field chain and `this` sink makes no guest effects until the
        // cached data reads and the own store have all passed their guards.
        if instruction.op() == Op::GetField
            && instruction.result_flags() == crate::bytecode::SET_THIS_REGISTER
            && instruction.writes_current_this()
            && !instruction.returns_from_frame()
            && let Some(next) = next
            && let Some(FieldLookup::Site(site)) = instruction.field_lookup()
            && let Some(site) = program.field_sites.get(site)
            && let Some((sink_atom, sink_cache)) = site.sink
            && let Some(receiver) = js_field_receiver(site.base, &register_offset)
            && let Some(destination) = register_offset(instruction.result_register())
        {
            let fallback = builder.create_block();
            let (address, _) = emit_js_cached_field_slot(
                &mut builder, arguments, payload, receiver, site.first.1, true, fallback, None, None, None,
            );
            let mut value = builder.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
            if let Some((_, cache)) = site.second {
                let (address, _) = emit_js_cached_field_slot(
                    &mut builder, arguments, payload, JsFieldReceiver::Value(value), cache, true, fallback, None, None, None,
                );
                value = builder.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
            }
            let exact = JsExactFallback {
                context, callout, callout_signature, result_pointer,
                stopped_block, continue_block, pc,
            };
            emit_js_own_field_set(
                &mut builder, arguments, payload, JsFieldReceiver::FixedThis, value,
                sink_atom, sink_cache, Some(destination), next, fallback, &exact,
            );
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }
        if instruction.op() == Op::GetField
            && instruction.result_flags() == 0
            && !instruction.returns_from_frame()
            && !instruction.writes_current_this()
            && instruction.numeric_local_target().is_none()
            && let Some(next) = next
            && let Some(FieldLookup::Atom { base, cache_site, .. }) = instruction.field_lookup()
            && let Some(receiver) = js_field_receiver(base, &register_offset)
            && let Some(destination_offset) = register_offset(instruction.result_register())
        {
            let exact = JsExactFallback {
                context,
                callout,
                callout_signature,
                result_pointer,
                stopped_block,
                continue_block,
                pc,
            };
            emit_js_own_field_get(&mut builder, arguments, payload, receiver, cache_site, destination_offset, next, &exact);
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }
        // A GetField that returns its value (`return object.field`), as a getter body is: the
        // read, then the native `Return`'s completion.
        if instruction.op() == Op::GetField
            && (instruction.result_flags() == 0
                || instruction.result_flags() == crate::bytecode::RETURN_REGISTER)
            && instruction.returns_from_frame()
            && !instruction.writes_current_this()
            && instruction.numeric_local_target().is_none()
            && let Some(FieldLookup::Atom { base, cache_site, .. }) = instruction.field_lookup()
            && let Some(receiver) = js_field_receiver(base, &register_offset)
        {
            let exact = JsExactFallback {
                context,
                callout,
                callout_signature,
                result_pointer,
                stopped_block,
                continue_block,
                pc,
            };
            emit_js_own_field_return(&mut builder, arguments, payload, receiver, cache_site, result_pointer, &exact);
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }
        let store_receiver = match instruction.op() {
            Op::SetField | Op::SetFieldStrict => register_offset(instruction.register_b()).map(JsFieldReceiver::Register),
            Op::SetThisField | Op::SetThisFieldStrict => Some(JsFieldReceiver::FixedThis),
            _ => None,
        };
        if let Some(receiver) = store_receiver
            && instruction.result_flags() == 0
            && !instruction.returns_from_frame()
            && !instruction.writes_current_this()
            && let Some(next) = next
            && let Some(value_offset) = register_offset(instruction.register_a())
        {
            let fallback = builder.create_block();
            let value = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, value_offset);
            let exact = JsExactFallback {
                context,
                callout,
                callout_signature,
                result_pointer,
                stopped_block,
                continue_block,
                pc,
            };
            emit_js_own_field_set(
                &mut builder,
                arguments,
                payload,
                receiver,
                value,
                instruction.atom_index(),
                instruction.cache_site_index(),
                None,
                next,
                fallback,
                &exact,
            );
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }
        if instruction.op() == Op::LoadThis
            && instruction.result_flags() == 0
            && !instruction.returns_from_frame()
            && !instruction.writes_current_this()
            && instruction.numeric_local_target().is_none()
            && let Some(next) = next
            && let Some(destination) = register_offset(instruction.result_register())
        {
            let fallback = builder.create_block();
            let value = emit_js_fixed_this(&mut builder, payload, fallback);
            builder.ins().store(MemFlagsData::trusted(), value, arguments, destination);
            builder.ins().jump(next, &[]);
            builder.switch_to_block(fallback);
            emit_js_step_fallback(
                &mut builder, context, callout, callout_signature, result_pointer,
                stopped_block, continue_block, pc,
            );
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }
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
                    if let Some(target) = target.filter(|target| *target < instruction_count)
                        && let Some(target) = entries.get(target).copied()
                    {
                        if usize::try_from(instruction.jump_target())
                            .is_ok_and(|target| target <= pc)
                        {
                            let fallback = builder.create_block();
                            let resume = builder.create_block();
                            let interrupted = emit_js_interrupt_requested(&mut builder);
                            builder.ins().brif(interrupted, fallback, &[], resume, &[]);
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
                            builder.switch_to_block(resume);
                        }
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
                    let constant_index = instruction.constant_index();
                    let byte_offset = program
                        .constants
                        .get(constant_index)
                        .and_then(|_| {
                            constant_index.checked_mul(std::mem::size_of::<Value>())
                        })
                        .and_then(|offset| i32::try_from(offset).ok());
                    if let (Some(byte_offset), Some(destination)) =
                        (byte_offset, register_offset(instruction.result_register()))
                    {
                        let fast = builder.create_block();
                        let fallback = builder.create_block();
                        let constant_count = builder.ins().load(
                            types::I64,
                            MemFlagsData::trusted(),
                            context,
                            CONTEXT_JS_CONSTANT_COUNT,
                        );
                        let in_bounds = builder.ins().icmp_imm_u(
                            IntCC::UnsignedGreaterThan,
                            constant_count,
                            i64::try_from(constant_index)
                                .expect("validated JS constant index"),
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
                        let constants = builder.ins().load(
                            types::I64,
                            MemFlagsData::trusted(),
                            context,
                            CONTEXT_JS_CONSTANTS,
                        );
                        let value = builder.ins().load(
                            types::I64,
                            MemFlagsData::trusted(),
                            constants,
                            byte_offset,
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
                JsDenotation::CapturedValue
                    if instruction.capture_depth() == 0
                        && instruction.result_flags() == 0
                        && !instruction.returns_from_frame()
                        && !instruction.writes_current_this()
                        && instruction.numeric_local_target().is_none() =>
                {
                    if let Some(destination) = register_offset(instruction.result_register()) {
                        use crate::value::ValueLayout;
                        let fallback = builder.create_block();
                        let values = emit_js_payload_word(
                            &mut builder, payload, std::mem::offset_of!(JsStepDispatch, capture_values),
                        );
                        let len = emit_js_payload_word(
                            &mut builder, payload, std::mem::offset_of!(JsStepDispatch, capture_len),
                        );
                        let slot = usize::from(instruction.capture_slot());
                        let in_bounds = builder.ins().icmp_imm_u(
                            IntCC::UnsignedGreaterThan, len, i64::try_from(slot).expect("capture slot fits i64"),
                        );
                        emit_js_guard(&mut builder, in_bounds, fallback);
                        let offset = i32::try_from(slot * std::mem::size_of::<Value>())
                            .expect("capture slot offset fits i32");
                        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), values, offset);
                        let tag = builder.ins().band_imm_u(value, ValueLayout::TAG_MASK as i64);
                        let ordinary = builder.ins().icmp_imm_u(IntCC::NotEqual, tag, ValueLayout::INTERNAL_TAG as i64);
                        emit_js_guard(&mut builder, ordinary, fallback);
                        builder.ins().store(MemFlagsData::trusted(), value, arguments, destination);
                        builder.ins().jump(next, &[]);
                        builder.switch_to_block(fallback);
                        emit_js_step_fallback(
                            &mut builder, context, callout, callout_signature, result_pointer,
                            stopped_block, continue_block, pc,
                        );
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
            general_dispatch_native_pcs[pc] = 1;
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
                if let Some(comparison) = nullish_comparison(&program.constants, instruction, |_| typed_ir::Type::ANY)
                    .filter(|comparison| register_offset(comparison.source).is_some()) {
                    Some(JsGeneralBranch::NullishFalse(comparison))
                } else {
                    (super::operations::NumberRelation::from_operator(operator).is_some()
                        && js_tagged_operand_supported(program, left, &register_offset)
                        && js_tagged_operand_supported(program, right, &register_offset))
                    .then_some(JsGeneralBranch::NumberFalse { operator, left, right })
                }
            }
            _ => None,
        };
        // Only a recognized branch has a jump target to read.
        if let (Some(next), Some(direct_branch)) = (next, direct_branch)
            && let Ok(target) = usize::try_from(instruction.jump_target())
            && target < instruction_count
            && let Some(target_block) = entries.get(target).copied()
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
                JsGeneralBranch::NullishFalse(comparison) => {
                    let source = register_offset(comparison.source).expect("prevalidated branch register");
                    let value = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, source);
                    emit_nullish_branch(&mut builder, value, comparison, fallback)
                }
            };
            if target <= pc {
                let resume = builder.create_block();
                let interrupted = emit_js_interrupt_requested(&mut builder);
                builder.ins().brif(interrupted, fallback, &[], resume, &[]);
                builder.switch_to_block(resume);
            }
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
            general_dispatch_native_pcs[pc] = 1;
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
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }

        // `++`/`--` of a Number is that Number plus or minus one (ECMA-262 13.4); any other
        // operand takes the exact-PC fallback before ToNumeric.
        if instruction.op() == Op::IncDec
            && instruction.numeric_local_target().is_none()
            && instruction.result_flags() == 0
            && !instruction.returns_from_frame()
            && !instruction.writes_current_this()
            && let Some(next) = next
            && let Some(decrement) = instruction.boolean_flag()
            && let Some(source) = register_offset(instruction.register_b())
            && let Some(destination) = register_offset(instruction.result_register())
        {
            let arithmetic = if decrement {
                super::operations::NumberArithmetic::Subtract
            } else {
                super::operations::NumberArithmetic::Add
            };
            let fallback = builder.create_block();
            let float = builder.create_block();
            let bits = builder.ins().load(types::I64, MemFlagsData::trusted(), arguments, source);
            let one = builder.ins().iconst(types::I64, Value::integer(1).to_bits() as i64);
            emit_js_int32_arithmetic(&mut builder, arithmetic, bits, one, arguments, destination, next, float);
            builder.switch_to_block(float);
            let number = emit_js_tagged_number(&mut builder, bits, fallback);
            let unit = builder.ins().f64const(Ieee64::with_bits(1.0_f64.to_bits()));
            let number = emit_number_arithmetic_operation(&mut builder, arithmetic, number, unit);
            let encoded = emit_js_box_number(&mut builder, number);
            builder.ins().store(MemFlagsData::trusted(), encoded, arguments, destination);
            builder.ins().jump(next, &[]);
            builder.switch_to_block(fallback);
            emit_js_step_fallback(&mut builder, context, callout, callout_signature, result_pointer, stopped_block, continue_block, pc);
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }

        // Where an arithmetic result goes and where control continues. A lane body completes
        // an instruction that returns its result at the function's terminal `Return`: the
        // activation ends, so that record's register is free, and the lane resumes there.
        let numeric_binary = instruction.op().js_denotation() == JsDenotation::NumericBinary;
        let (result_target, result_next) = if !numeric_binary {
            (None, None)
        } else if !instruction.returns_from_frame() {
            (Some(instruction.result_register()), next)
        } else {
            (None, None)
        };
        // The operator is read only once the op is a numeric binary.
        let arithmetic = numeric_binary
            .then(|| {
                super::operations::NumberArithmetic::from_js_instruction(
                    instruction.op(),
                    instruction.binary_operator(),
                )
            })
            .flatten();
        let operands = numeric_binary.then(|| (instruction.operand_b(), instruction.operand_c()));
        let bitwise = numeric_binary
            .then(|| {
                super::operations::NumberBitwise::from_js_instruction(
                    instruction.op(),
                    instruction.binary_operator(),
                )
            })
            .flatten();
        // A returning result's flags are the return bit alone (`result_target`).
        let result_flags_plain = instruction.result_flags() == 0
            || (instruction.returns_from_frame() && result_target.is_some());
        let numeric_result = numeric_binary
            && arithmetic.is_some()
            && instruction.numeric_local_target().is_none()
            && result_flags_plain
            && !instruction.writes_current_this()
            && result_target.and_then(register_offset).is_some()
            && operands.is_some_and(|(left, right)| {
                js_tagged_operand_supported(program, left, &register_offset)
                    && js_tagged_operand_supported(program, right, &register_offset)
            });
        if let (true, Some(next), Some(arithmetic), Some((left_operand, right_operand))) =
            (numeric_result, result_next, arithmetic, operands)
        {
            let fallback = builder.create_block();
            let destination = result_target
                .and_then(register_offset)
                .expect("prevalidated JS numeric destination");
            let left_bits = read_js_tagged_operand(
                &mut builder,
                program,
                left_operand,
                arguments,
                &register_offset,
            )
            .expect("prevalidated JS numeric operand");
            let right_bits = read_js_tagged_operand(
                &mut builder,
                program,
                right_operand,
                arguments,
                &register_offset,
            )
            .expect("prevalidated JS numeric operand");
            if matches!(
                arithmetic,
                super::operations::NumberArithmetic::Add
                    | super::operations::NumberArithmetic::Subtract
                    | super::operations::NumberArithmetic::Multiply
            ) {
                let float = builder.create_block();
                emit_js_int32_arithmetic(
                    &mut builder,
                    arithmetic,
                    left_bits,
                    right_bits,
                    arguments,
                    destination,
                    next,
                    float,
                );
                builder.switch_to_block(float);
            }
            let left = emit_js_tagged_number(&mut builder, left_bits, fallback);
            let right = emit_js_tagged_number(&mut builder, right_bits, fallback);
            let number = emit_number_arithmetic_operation(&mut builder, arithmetic, left, right);
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
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }

        // Bitwise Number operators have an exact native domain when both
        // operands already carry Quench's signed-int32 Number tag. Any value
        // requiring ToPrimitive/ToNumeric or a BigInt operation takes the
        // existing exact-PC VM fallback before either operand is coerced.
        let bitwise_result = numeric_binary
            && bitwise.is_some()
            && instruction.numeric_local_target().is_none()
            && result_flags_plain
            && !instruction.writes_current_this()
            && result_target.and_then(register_offset).is_some()
            && operands.is_some_and(|(left, right)| {
                js_tagged_operand_supported(program, left, &register_offset)
                    && js_tagged_operand_supported(program, right, &register_offset)
            });
        if let (true, Some(next), Some(operation), Some((left_operand, right_operand))) =
            (bitwise_result, result_next, bitwise, operands)
        {
            let fallback = builder.create_block();
            let native = builder.create_block();
            let destination = result_target
                .and_then(register_offset)
                .expect("prevalidated JS bitwise destination");
            let left_bits = read_js_tagged_operand(
                &mut builder,
                program,
                left_operand,
                arguments,
                &register_offset,
            )
            .expect("prevalidated JS bitwise operand");
            let right_bits = read_js_tagged_operand(
                &mut builder,
                program,
                right_operand,
                arguments,
                &register_offset,
            )
            .expect("prevalidated JS bitwise operand");
            let shift = i64::from(crate::value::ValueLayout::TAG_SHIFT);
            let int_tag = (crate::value::ValueLayout::INT_TAG >> shift) as i64;
            let left_tag = builder.ins().ushr_imm_u(left_bits, shift);
            let right_tag = builder.ins().ushr_imm_u(right_bits, shift);
            let left_integer = builder
                .ins()
                .icmp_imm_u(IntCC::Equal, left_tag, int_tag);
            let right_integer = builder
                .ins()
                .icmp_imm_u(IntCC::Equal, right_tag, int_tag);
            let both_integer = builder.ins().band(left_integer, right_integer);
            builder
                .ins()
                .brif(both_integer, native, &[], fallback, &[]);

            builder.switch_to_block(native);
            let left = builder.ins().ireduce(types::I32, left_bits);
            let right = builder.ins().ireduce(types::I32, right_bits);
            let encoded = if operation
                == super::operations::NumberBitwise::ShiftRightUnsigned
            {
                let number = emit_number_bitwise_operation(&mut builder, operation, left, right);
                emit_js_box_number(&mut builder, number)
            } else {
                let result = emit_number_bitwise_bits(&mut builder, operation, left, right);
                let payload = builder.ins().uextend(types::I64, result);
                builder
                    .ins()
                    .bor_imm_u(payload, crate::value::ValueLayout::INT_TAG as i64)
            };
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
            general_dispatch_native_pcs[pc] = 1;
            continue;
        }

        // Value-producing Number relations use the same guarded tagged-number
        // domain as arithmetic. The VM remains responsible for string order,
        // coercion, and any effectful or throwing non-number comparisons.
        let relation = numeric_binary
            .then(|| {
                super::operations::NumberRelation::from_operator(instruction.binary_operator())
            })
            .flatten();
        let relation_returns = instruction.returns_from_frame();
        let relation_result = numeric_binary
            && relation.is_some()
            && instruction.numeric_local_target().is_none()
            && (instruction.result_flags() == 0
                || (relation_returns && instruction.result_flags() == RETURN_REGISTER))
            && !instruction.writes_current_this()
            && (relation_returns || register_offset(instruction.result_register()).is_some())
            && (relation_returns || next.is_some())
            && operands.is_some_and(|(left, right)| {
                js_tagged_operand_supported(program, left, &register_offset)
                    && js_tagged_operand_supported(program, right, &register_offset)
            });
        if let (true, Some(_relation), Some((left_operand, right_operand))) =
            (relation_result, relation, operands)
        {
            let fallback = builder.create_block();
            let left_bits = read_js_tagged_operand(
                &mut builder,
                program,
                left_operand,
                arguments,
                &register_offset,
            )
            .expect("prevalidated JS relation left operand");
            let left = emit_js_tagged_number(&mut builder, left_bits, fallback);
            let right_bits = read_js_tagged_operand(
                &mut builder,
                program,
                right_operand,
                arguments,
                &register_offset,
            )
            .expect("prevalidated JS relation right operand");
            let right = emit_js_tagged_number(&mut builder, right_bits, fallback);
            let holds = emit_number_relation(
                &mut builder,
                instruction.binary_operator(),
                left,
                right,
                NumberRelationDomain::Float64,
            )
            .expect("prevalidated JavaScript Number relation");
            let true_value = builder.ins().iconst(types::I64, Value::TRUE.to_bits() as i64);
            let false_value = builder
                .ins()
                .iconst(types::I64, Value::FALSE.to_bits() as i64);
            let encoded = builder.ins().select(holds, true_value, false_value);
            if relation_returns {
                emit_js_guarded_return(&mut builder, encoded, result_pointer, pc, fallback);
            } else {
                let destination = register_offset(instruction.result_register())
                    .expect("prevalidated JS relation destination");
                builder.ins().store(MemFlagsData::trusted(), encoded, arguments, destination);
                builder.ins().jump(next.expect("prevalidated JS relation continuation"), &[]);
            }
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
            general_dispatch_native_pcs[pc] = 1;
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
    #[cfg(feature = "aot")]
    for entry in _scalar_regions {
        let gate = typed_entries.iter().find(|(unit, _)| unit.header == entry.range.start)
            .map_or(entries[entry.range.start], |(_, fallback)| *fallback);
        let call = scalar_region::emit_entry(&mut builder, isa, entry, gate,
            context, payload, arguments, result_pointer, &blocks, &entries, invalid_pc, &control);
        frame_flow.residual(call, entry.frame);
    }
    #[cfg(feature = "aot")]
    let mut typed_returns = Vec::new();
    #[cfg(feature = "aot")]
    for (unit, fallback) in &typed_entries {
        builder.switch_to_block(entries[unit.header]);
        let run = builder.create_block();
        let specialized = emit_js_payload_word(&mut builder, payload, std::mem::offset_of!(JsStepDispatch, stores_specialized));
        let admitted = builder.ins().icmp_imm_u(IntCC::NotEqual, specialized, 0);
        builder.ins().brif(admitted, run, &[], *fallback, &[]);
        builder.switch_to_block(run);
        let signature = builder.import_signature(native_entry_signature(isa.pointer_type(), isa.default_call_conv()));
        let name = builder.func.declare_imported_user_function(unit.function.name.get_user()
            .expect("typed residual has an internal symbol").clone());
        let target = builder.import_function(ir::ExtFuncData { name: ir::ExternalName::user(name),
            signature, colocated: false, patchable: false });
        let call = builder.ins().call(target, &parameters);
        // After inlining ordinary exits preserve SSA edges. Exact callee
        // materialization owns a canonical frame boundary below.
        let pc = builder.inst_results(call)[0];
        typed_returns.push((unit.header, pc));
        let pc = builder.ins().uextend(types::I64, pc);
        builder.ins().jump(typed_continue.expect("typed continuation").0, &[ir::BlockArg::Value(pc)]);
    }
    let live = program.register_root_masks(function_id);
    for (pc, resume) in resume_entries.iter().copied().enumerate() {
        builder.switch_to_block(resume);
        frame_flow.resume(&mut builder, live, pc);
        builder.ins().jump(entries[pc], &[]);
    }
    builder.switch_to_block(continue_block);
    emit_js_pc_dispatch(&mut builder, continue_pc, &resume_entries, invalid_pc);
    #[cfg(feature = "aot")]
    if let Some((block, pc)) = typed_continue {
        builder.switch_to_block(block);
        let materialized = builder.ins().icmp_imm_u(IntCC::Equal, pc, i64::from(JS_INLINE_MATERIALIZED_PC));
        let source = builder.create_block();
        builder.ins().brif(materialized, stopped_block, &[], source, &[]);
        builder.switch_to_block(source);
        let targets: Vec<_> = entries.iter().copied().enumerate().map(|(pc, entry)|
            typed_entries.iter().find(|(unit, _)| unit.header == pc).map_or(entry, |(_, fallback)| *fallback)).collect();
        emit_js_pc_dispatch(&mut builder, pc, &targets, invalid_pc);
    }
    builder.seal_block(continue_block);
    builder.seal_all_blocks();
    builder.into_builder().finalize(isa.frontend_config());
    #[cfg(feature = "aot")]
    if !typed_entries.is_empty() {
        use cranelift_codegen::inline::{Inline, InlineCommand};
        struct TypedResidual<'a>(&'a [StaticTypedRegionEntry<'a>]);
        impl Inline for TypedResidual<'_> {
            fn inline(&mut self, caller: &ir::Function, _: ir::Inst, _: ir::Opcode,
                callee: ir::FuncRef, _: &[ir::Value]) -> InlineCommand<'_> {
                let ir::ExternalName::User(name) = caller.dfg.ext_funcs[callee].name else { return InlineCommand::KeepCall };
                let Some(unit) = self.0.iter().find(|unit|
                    Some(&caller.params.user_named_funcs()[name]) == unit.function.name.get_user()) else { return InlineCommand::KeepCall; };
                InlineCommand::Inline { callee: std::borrow::Cow::Borrowed(unit.function), visit_callee: false }
            }
        }
        for unit in _typed_regions {
            cranelift_codegen::verify_function(unit.function, isa).map_err(|_| "typed-residual-verification-failed")?;
        }
        // Cranelift's append-only instruction arena separates the cloned,
        // verified helper calls from the caller's guest/semantic callouts.
        let first_inlined = native.dfg.num_insts();
        let mut context = cranelift_codegen::Context::for_function(native);
        if !context.inline(TypedResidual(_typed_regions)).map_err(|_| "typed-residual-inlining-failed")? {
            return Err("typed-residual-call-not-found");
        }
        native = context.func;
        // The residual returns a statically fixed source PC on every exit.
        // Link those edges before frame promotion instead of dispatching a
        // known PC through a memory/result join and another jump table.
        for (header, result) in typed_returns {
            let result = native.dfg.resolve_aliases(result);
            let ir::ValueDef::Param(join, parameter) = native.dfg.value_def(result) else {
                return Err("typed-residual-return-is-not-a-join");
            };
            let exits: Vec<_> = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
                .filter_map(|instruction| {
                    let ir::InstructionData::Jump { destination, .. } = native.dfg.insts[instruction] else { return None };
                    if destination.block(&native.dfg.value_lists) != join { return None; }
                    let ir::BlockArg::Value(value) = destination.args(&native.dfg.value_lists).nth(parameter)? else { return None };
                    let ir::ValueDef::Result(constant, _) = native.dfg.value_def(native.dfg.resolve_aliases(value)) else { return None };
                    let ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } = native.dfg.insts[constant] else { return None };
                    let pc = usize::try_from(imm.bits()).ok()?;
                    if pc == JS_INLINE_MATERIALIZED_PC as usize { return Some((instruction, stopped_block)); }
                    let target = *entries.get(pc)?;
                    let target = typed_entries.iter().find(|(unit, _)| unit.header == header && header == pc)
                        .map_or(target, |(_, fallback)| *fallback);
                    Some((instruction, target))
                }).collect();
            for (instruction, target) in exits {
                let destination = native.dfg.block_call(target, &[]);
                native.dfg.insts[instruction] = ir::InstructionData::Jump { opcode: ir::Opcode::Jump, destination };
            }
        }
        // Static exit linking can orphan the synthetic return/dispatch join.
        // Discard it before dirty-state analysis so unreachable edges cannot
        // inflate the live frame footprint of a real semantic callout.
        let mut context = cranelift_codegen::Context::for_function(native);
        context.compute_cfg();
        context.compute_domtree();
        context.eliminate_unreachable_code(isa).map_err(|_| "typed-residual-cfg-pruning-failed")?;
        native = context.func;
        for instruction in native.layout.blocks().flat_map(|block| native.layout.block_insts(block)) {
            if instruction.as_u32() as usize >= first_inlined && native.dfg.insts[instruction].opcode().is_call() {
                let materializes = if native.dfg.insts[instruction].opcode() == ir::Opcode::CallIndirect {
                    let target = native.dfg.resolve_aliases(native.dfg.inst_args(instruction)[0]);
                    match native.dfg.value_def(target) {
                        ir::ValueDef::Result(load, _) => matches!(native.dfg.insts[load],
                            ir::InstructionData::Load { offset, .. }
                                if i32::from(offset) == LaneHelper::MaterializeInline as i32 * std::mem::size_of::<usize>() as i32),
                        _ => false,
                    }
                } else { false };
                if materializes {
                    frame_flow.residual(instruction, &(0..function.registers).collect::<Vec<_>>());
                } else { frame_flow.no_frame(instruction); }
            }
        }
    }
    frame_flow.promote(&mut native, callout, live, isa.frontend_config());
    Ok((native, general_dispatch_native_pcs))
}

/// Exact-PC dispatch; callers choose whether the edge admits canonical homes
/// or carries already-verified SSA state. Invalid PCs share one diagnostic exit.
fn emit_js_pc_dispatch(builder: &mut FunctionBuilder<'_>, pc: ir::Value,
    targets: &[ir::Block], invalid: ir::Block) {
    let select = builder.create_block();
    let valid = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, pc, targets.len() as i64);
    builder.ins().brif(valid, select, &[], invalid, &[ir::BlockArg::Value(pc)]);
    builder.switch_to_block(select);
    let selector = builder.ins().ireduce(types::I32, pc);
    let cases: Vec<_> = targets.iter().map(|target| builder.func.dfg.block_call(*target, &[])).collect();
    let default = builder.func.dfg.block_call(invalid, &[ir::BlockArg::Value(pc)]);
    let table = builder.create_jump_table(ir::JumpTableData::new(default, &cases));
    builder.ins().br_table(selector, table);
}

/// Canonical, owner-free description of the inputs that affect GeneralDispatch
/// code generation. Keep this in lockstep with compile_js_general_dispatch:
/// emitted instructions depend on decoded bytecode, frame layout, static-name
/// mode, and referenced constant variants/payloads. The key records the full
/// payload for values that lowering can embed and only the variant for payloads
/// that the generated code reads from the active program at runtime.
pub(super) fn js_general_dispatch_cache_key(
    program: &ResidualProgram,
    function_id: u32,
    unresolved_names_are_global: bool,
) -> Option<Box<[u8]>> {
    use std::fmt::Write as _;

    if program.kind == crate::bytecode::ProgramKind::Wasm || function_id == 0 {
        return None;
    }
    let function = program.function_get(function_id as usize)?;
    let static_global_names = unresolved_names_are_global
        && function.names_resolve_statically(program);
    let mut key = String::with_capacity(
        function
            .code
            .len()
            .saturating_mul(24)
            .min(super::program_store::JS_GENERAL_ARTIFACT_CACHE_MAX_KEY_BYTES),
    );
    writeln!(
        key,
        "gd-v1:{}:{}:{}:{}:{}",
        static_global_names as u8,
        function.params,
        function.locals,
        function.registers,
        function.code.len()
    )
    .ok()?;
    for instruction in function.instructions() {
        writeln!(
            key,
            "{}:{}:{}:{}:{}",
            instruction.op() as u16,
            instruction.a(),
            instruction.b(),
            instruction.c(),
            instruction.imm()
        )
        .ok()?;
        if key.len() > super::program_store::JS_GENERAL_ARTIFACT_CACHE_MAX_KEY_BYTES {
            return None;
        }

        // Name fast-path admission checks the current program's atom text.
        // Include that text because two programs may encode different names at
        // the same atom index while sharing identical bytecode.
        if matches!(
            instruction.op(),
            Op::LoadName | Op::LoadNameCall | Op::LoadNameTypeof
        ) {
            let name = program.atoms.get(instruction.atom_index() as usize)?;
            if name.len()
                > super::program_store::JS_GENERAL_ARTIFACT_CACHE_MAX_KEY_BYTES
                    .saturating_sub(key.len())
            {
                return None;
            }
            write!(key, "A:{}:", name.len()).ok()?;
            key.push_str(name);
            key.push('\n');
        }

        // Only constants addressed by this function can affect its lowering.
        // Keep the operand/index in the key as well as the value because the
        // native PC map and compile-time tagged-operand decisions are per PC.
        if instruction.op().immediate_role() == crate::bytecode::ImmediateRole::ConstantIndex {
            let index = instruction.constant_index();
            append_js_general_constant_key(&mut key, program, index)?;
        }
        for field in crate::bytecode::InstructionField::ALL {
            if !instruction
                .op()
                .field_layout(*field)
                .is_operand_field()
            {
                continue;
            }
            let operand = crate::bytecode::Operand(instruction.field_value(*field));
            if operand.kind() == Some(crate::bytecode::OperandKind::Constant) {
                let index = operand.payload();
                write!(key, "O:{}:{}:", *field as u8, index).ok()?;
                append_js_general_constant_key(&mut key, program, usize::from(index))?;
            }
        }
        if key.len() > super::program_store::JS_GENERAL_ARTIFACT_CACHE_MAX_KEY_BYTES {
            return None;
        }
    }
    Some(key.into_bytes().into_boxed_slice())
}

fn append_js_general_constant_key(
    key: &mut String,
    program: &ResidualProgram,
    index: usize,
) -> Option<()> {
    use std::fmt::Write as _;

    match program.constants.get(index)? {
        crate::bytecode::Constant::Number(value) => {
            writeln!(key, "N:{}", value.to_bits()).ok()?;
        }
        crate::bytecode::Constant::String(_) => key.push_str("S;\n"),
        crate::bytecode::Constant::StringUnits(_) => key.push_str("U;\n"),
        crate::bytecode::Constant::BigInt(_) => key.push_str("B;\n"),
        crate::bytecode::Constant::WasmBits64(_) => key.push_str("W;\n"),
        crate::bytecode::Constant::WasmV128(_) => key.push_str("V;\n"),
        crate::bytecode::Constant::Boolean(value) => {
            writeln!(key, "b:{}", *value as u8).ok()?;
        }
        crate::bytecode::Constant::Null => key.push_str("0;\n"),
        crate::bytecode::Constant::Undefined => key.push_str("u;\n"),
    }
    Some(())
}

/// How the shared VM executes guest functions. The dynamic modes share the
/// interpreter/JIT; NativeOnly requires linked code. All use the same heap,
/// frames and host APIs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionMode {
    /// The shared interpreter: general loop and fast lane.
    #[default]
    Interpreted,
    /// Hot supported JavaScript functions and Wasm functions run as native code.
    Compiled,
    /// Require linked guest code at every entry; reject interpreter/JIT fallback.
    /// This execution check does not itself remove compiler code from the binary.
    #[cfg(feature = "native-execution")]
    NativeOnly,
}

impl ExecutionMode {
    // Expose the build capability to callers in host crates, so unavailable
    // fallback branches cannot retain an interpreter or source compiler.
    #[inline(always)]
    pub(super) fn allows_dynamic_execution(self) -> bool {
        crate::DYNAMIC_EXECUTION_ENABLED && match self {
            Self::Interpreted | Self::Compiled => true,
            #[cfg(feature = "native-execution")]
            Self::NativeOnly => false,
        }
    }
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

/// The machine type of a register's view. Unsigned address offsets stay extended
/// across loop parameters: Cranelift elides extension of x64 arithmetic results,
/// but not loop parameters. Signed arithmetic stays i32 and widens at consumers,
/// so loop-carried values do not require a sign extension on every definition.
fn view_ir(ty: crate::WasmType, extension: Int32Extension) -> ir::Type {
    match ty {
        crate::WasmType::I32 if extension == Int32Extension::Zero => types::I64,
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
    /// An unsigned memory32 index plus a validated memory32 offset, without wrapping.
    /// Moves preserve this proof; arithmetic and mixed joins discard it.
    Memory32Address,
    /// A 128-bit SIMD value. Only SIMD instructions, moves and vector accesses read it, so
    /// it never meets a call, a return or a scalar view (`Emitter::vector`).
    Vector,
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
        match (self, other) {
            (a, b) if a == b => a,
            (Self::Memory32Address, Self::Value(Kind::I64))
            | (Self::Value(Kind::I64), Self::Memory32Address) => Self::Value(Kind::I64),
            _ => Self::Conflict,
        }
    }
}

/// Register facts at one bytecode position. Most Wasm temporaries have the
/// default i32 representation, so retaining a full `registers`-sized vector
/// at every reachable instruction wastes memory. This stores only deviations
/// from that default; joins apply the same `Slot` lattice to every register.
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

/// An opt-in JIT switch the environment sets ("1" or "true"), read once per process: a
/// compiled call consults several, and reading the environment on each call cost more than
/// the calls (HashSet-wasm `--jit`: `getenv` was 68% of the run's instructions).
struct EnvSwitch {
    name: &'static str,
    enabled: std::sync::OnceLock<bool>,
}

impl EnvSwitch {
    const fn new(name: &'static str) -> Self {
        Self { name, enabled: std::sync::OnceLock::new() }
    }

    fn enabled(&self) -> bool {
        *self.enabled.get_or_init(|| {
            std::env::var(self.name).is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
        })
    }
}

static JIT_WASM_SELECTIVE: EnvSwitch = EnvSwitch::new(JIT_WASM_SELECTIVE_ENV);
static JIT_DIRECT_COMPARE_BRANCHES: EnvSwitch = EnvSwitch::new(JIT_DIRECT_COMPARE_BRANCHES_ENV);
static JIT_AFFINE_PAIR: EnvSwitch = EnvSwitch::new(JIT_AFFINE_PAIR_ENV);
static JIT_REFERENCE_BODY_CACHE: EnvSwitch = EnvSwitch::new(JIT_REFERENCE_BODY_CACHE_ENV);
static JIT_EMSCRIPTEN_MEMCPY_LEAF: EnvSwitch = EnvSwitch::new(JIT_EMSCRIPTEN_MEMCPY_LEAF_ENV);
static JIT_ROOT_ONLY: EnvSwitch = EnvSwitch::new(JIT_ROOT_ONLY_ENV);
static JIT_ROOT_FIRST: EnvSwitch = EnvSwitch::new(JIT_ROOT_FIRST_ENV);

fn jit_root_override() -> Result<Option<usize>, ()> {
    #[cfg(test)]
    if let Some(mode) = WASM_JIT_MODE_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return Ok(mode.root);
    }
    static ROOT: std::sync::OnceLock<Result<Option<usize>, ()>> = std::sync::OnceLock::new();
    *ROOT.get_or_init(|| {
        let Some(root) = std::env::var_os(JIT_ROOT_ENV) else {
            return Ok(None);
        };
        root.to_str()
            .ok_or(())?
            .parse::<usize>()
            .map(Some)
            .map_err(|_| ())
    })
}

/// Whether Wasm modules stay selective for good, compiling only their hot roots
/// (`WasmModuleCompile::NEVER`): an A/B arm against the default, which compiles a long runner
/// whole.
pub(super) fn jit_wasm_selective_enabled() -> bool {
    JIT_WASM_SELECTIVE.enabled()
}

fn jit_direct_compare_branches_enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = WASM_JIT_DIRECT_COMPARE_BRANCHES_TEST_OVERRIDE.with(std::cell::Cell::get)
    {
        return enabled;
    }
    JIT_DIRECT_COMPARE_BRANCHES.enabled()
}

/// Keep the screened affine pair loop opt-in; the benchmark harness can still
/// run a same-binary, same-host comparison arm with it enabled.
fn jit_affine_pair_enabled() -> bool {
    JIT_AFFINE_PAIR.enabled()
}

fn jit_reference_body_cache_enabled() -> bool {
    JIT_REFERENCE_BODY_CACHE.enabled()
}

fn jit_emscripten_memcpy_leaf_enabled() -> bool {
    JIT_EMSCRIPTEN_MEMCPY_LEAF.enabled()
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
    JIT_ROOT_ONLY.enabled()
}

fn jit_wasm_root_first_enabled() -> bool {
    #[cfg(test)]
    if let Some(mode) = WASM_JIT_MODE_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return mode.root_first;
    }
    JIT_ROOT_FIRST.enabled()
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
    static WASM_JIT_EAGER_UNREACHABLE_REJECTED_FOR_TEST: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// Whether the eager artifact compiles functions holding `unreachable` (C asserts and aborts,
/// which most Emscripten functions hold): the trap lowers to a return of the trap status. A test
/// may reject them to keep such a function for the tier.
fn eager_unreachable_admitted() -> bool {
    #[cfg(test)]
    if WASM_JIT_EAGER_UNREACHABLE_REJECTED_FOR_TEST.with(std::cell::Cell::get) {
        return false;
    }
    true
}

#[cfg(test)]
pub(super) fn with_eager_unreachable_rejected_for_test<R>(run: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            WASM_JIT_EAGER_UNREACHABLE_REJECTED_FOR_TEST.with(|rejected| rejected.set(self.0));
        }
    }
    let previous =
        WASM_JIT_EAGER_UNREACHABLE_REJECTED_FOR_TEST.with(|rejected| rejected.replace(true));
    let _restore = Restore(previous);
    run()
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
pub(super) struct CallContext {
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
    /// Rooted materialized JS constants for the active program. Appended to
    /// preserve all existing generated-code ABI offsets. ProgramStore does not
    /// replace a running program's constant allocation.
    js_constants: *const Value,
    js_constant_count: usize,
    /// Borrowed only during a running entry; a non-reentrant region may prove unique ownership.
    interrupt_owner: *const crate::ExecutionInterrupt,
}

impl CallContext {
    fn native_adapter(
        adapter_payload: *mut (),
        interrupt: *const std::sync::atomic::AtomicBool,
        poll_page: Option<(usize, usize)>,
    ) -> Self {
        let mut context = Self {
            stack_limit: 0,
            interrupt: std::ptr::null(),
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
            poll_start: 0,
            poll_end: 0,
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
            js_constants: std::ptr::null(),
            js_constant_count: 0,
            interrupt_owner: std::ptr::null(),
        };
        context.bind_native_adapter(adapter_payload, interrupt, poll_page);
        context
    }

    fn bind_native_adapter(
        &mut self, payload: *mut (), interrupt: *const std::sync::atomic::AtomicBool,
        poll_page: Option<(usize, usize)>,
    ) {
        self.stack_limit = native_stack_limit();
        self.interrupt = interrupt;
        self.adapter_payload = payload;
        self.poll_start = poll_page.map_or(0, |(start, _)| start);
        self.poll_end = poll_page.map_or(0, |(start, bytes)| start + bytes);
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
            js_constants: std::ptr::null(),
            js_constant_count: 0,
            interrupt_owner: interrupt,
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
            js_constants: self.js_constants,
            js_constant_count: self.js_constant_count,
            interrupt_owner: self.interrupt_owner,
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
const CONTEXT_JS_CONSTANTS: i32 = std::mem::offset_of!(CallContext, js_constants) as i32;
const CONTEXT_JS_CONSTANT_COUNT: i32 =
    std::mem::offset_of!(CallContext, js_constant_count) as i32;
const MEMCPY_CACHE_VALID: i32 = std::mem::offset_of!(EmscriptenMemcpyCache, valid) as i32;
const MEMCPY_CACHE_FUNCTION: i32 = std::mem::offset_of!(EmscriptenMemcpyCache, function) as i32;

/// The callout's signature: (context, function index, arguments, result) ->
/// status; arguments and result are Wasm bits, one word each.
type Callout = unsafe extern "C" fn(*mut CallContext, u64, *const u64, *mut u64) -> i64;
type OsrProbeCallout = unsafe extern "C" fn(*mut CallContext, u64, u64, *mut u64) -> i64;

#[repr(C)]
struct OsrProbePayload {
    region: *const LoopRegion,
    entry: *mut LoopEntry,
    function: u32,
    frame: usize,
    program: Option<Rc<ResidualProgram>>,
    diagnostics_enabled: bool,
}

/// The OSR probe's first word is otherwise a cache site, which is bounded by `u16`.
/// `u64::MAX` selects an initialized frame-owned local read; the second word is its slot.
const OSR_INITIALIZED_LOCAL_READ: u64 = u64::MAX;
/// Reserve one dense append without storing a value or collecting.
const OSR_RESERVE_ARRAY_APPEND: u64 = u64::MAX - 1;
/// Read a present own dense element without allocation, coercion or collection.
const OSR_DENSE_ARRAY_READ: u64 = u64::MAX - 2;
/// Store the value the scratch word holds into an own data property of the receiver, through
/// its site's cache entry (`Vm::cached_field_store_hit`): the operation word is this flag,
/// the atom shifted by `OSR_FIELD_STORE_ATOM_SHIFT` and the site. A store never collects or
/// runs guest code, and a miss returns before it writes.
pub(super) const OSR_FIELD_STORE_ATOM_SHIFT: u32 = u16::BITS;
const OSR_PROBE_TAG_SHIFT: u32 = OSR_FIELD_STORE_ATOM_SHIFT + crate::bytecode::Atom::BITS;
const OSR_PROBE_TAG_MASK: u64 = u64::MAX << OSR_PROBE_TAG_SHIFT;
pub(super) const OSR_FIELD_STORE: u64 = 1 << OSR_PROBE_TAG_SHIFT;

/// A no-guest-effect JavaScript OSR helper. Capacity reserve may relocate a backing but
/// never stores a guest value or collects; it refreshes all entry aliases and exits on debt.
/// For a field probe, `receiver_bits` is passed in
/// the ABI's arguments word as a raw `Value` and the first word is its cache site. For an
/// initialized-local read, the first word is `OSR_INITIALIZED_LOCAL_READ` and the second is
/// a local slot. A miss returns before a guest-visible operation runs.
unsafe extern "C" fn osr_probe<H: Host>(
    context: *mut CallContext,
    cache_site: u64,
    receiver_bits: u64,
    result: *mut u64,
) -> i64 {
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    // SAFETY: `CompiledOsrRegion::run` owns this context, VM and payload until this
    // synchronous callback returns. The helper performs no guest call and retains no pointer.
    let context_ref = unsafe { &mut *context };
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if context_ref.vm.is_null() || result.is_null() {
            return None;
        }
        // SAFETY: the stack payload lives through this native entry and owns a residual Rc,
        // avoiding a ProgramStore lookup/refcount on every probed field read.
        let payload = unsafe { &*context_ref.adapter_payload.cast::<OsrProbePayload>() };
        let program = payload.program.as_deref()?;
        let vm = unsafe { &mut *context_ref.vm.cast::<Vm<H>>() };
        if cache_site == OSR_DENSE_ARRAY_READ {
            // The generated caller seeds the scratch word with the tagged key. Neither
            // lookup nor a miss allocates, collects, invokes guest code, or retains pointers.
            // The current array record/backing is resolved anew after every native store.
            let key = Value::from_bits(unsafe { result.read() });
            return vm.dense_array_element(Value::from_bits(receiver_bits), key);
        }
        if cache_site == OSR_RESERVE_ARRAY_APPEND {
            let receiver = Value::from_bits(receiver_bits);
            let (old, elements, length, capacity) = vm.reserve_native_array_append(receiver, osr_loop::MAX_INDEXED_LENGTH)?;
            // SAFETY: both pointers refer to the current synchronous native invocation.
            // No guest code or collector ran, so all entry identities remain rooted and live.
            unsafe { (&mut *payload.entry).refresh_array_backing(&*payload.region, old, elements, length, capacity) };
            let due = vm.heap.should_collect();
            if payload.diagnostics_enabled && std::env::var_os("QUENCH_JS_JIT_TRACE_OSR_REJECT").is_some() {
                eprintln!("QUENCH_JS_JIT_OSR_ARRAY_RESERVE function={} length={length} capacity={capacity} gc_due={due}", payload.function);
            }
            // Collection is never performed with native SSA values unrooted. A debt exit
            // writes live values back at this SetIndex before the VM performs its store.
            return (!due).then_some(Value::UNDEFINED);
        }
        if cache_site & OSR_PROBE_TAG_MASK == OSR_FIELD_STORE {
            let site = cache_site as u16;
            let atom = ((cache_site & !OSR_FIELD_STORE) >> OSR_FIELD_STORE_ATOM_SHIFT) as crate::bytecode::Atom;
            // SAFETY: generated code seeded the scratch word with the boxed value.
            let value = Value::from_bits(unsafe { result.read() });
            let receiver = Value::from_bits(receiver_bits);
            return vm.primary_own_field_store_hit::<true>(program, receiver, atom, value, site,
                crate::heap::PropertyStoreLayout::Preserve).then_some(Value::UNDEFINED);
        }
        if cache_site == OSR_INITIALIZED_LOCAL_READ {
            if vm.lane_has_activations()
                || vm.frames.get(payload.frame).is_none_or(|frame| frame.function != payload.function)
            {
                return None;
            }
            let slot = usize::try_from(receiver_bits).ok()?;
            return vm.initialized_frame_binding(program, payload.frame, slot);
        }
        let site = u16::try_from(cache_site).ok()?;
        // The ABI slot is used as a raw 64-bit Value, never as an address.
        let receiver = Value::from_bits(receiver_bits);
        let probe = vm.cached_field_hit(program, receiver, site);
        if payload.diagnostics_enabled {
            let function = payload.function;
            vm.programs.record_js_jit_field_cache_probe(
                context_ref.program,
                function,
                site,
                matches!(&probe, super::object::FieldCacheProbe::Hit(_)),
            );
        }
        match probe {
            super::object::FieldCacheProbe::Hit(value) => Some(value),
            // A site of several shapes answers from its polymorphic entries, as the lane's
            // polymorphic read does; neither probe runs guest code.
            super::object::FieldCacheProbe::OrdinaryMiss
            | super::object::FieldCacheProbe::Ineligible => vm.polymorphic_field_hit(program, receiver, site),
        }
    }));
    ACTIVE_CONTEXT.with(|current| current.set(active));

    match called {
        Ok(Some(value)) => {
            // SAFETY: generated code supplies a valid result word.
            unsafe { result.write(value.to_bits()) };
            0
        }
        Ok(None) => 1,
        Err(panic) => {
            context_ref.raised = Some(Raised::Panic(panic));
            RAISED_STATUS
        }
    }
}

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

/// A target certificate for same-artifact indirect calls: element `index` of instance `env`'s
/// table in binding `table` is this artifact's `function`, of type `signature`, whose code is
/// `body`. An
/// artifact's call sites share its certificates, one per table index's low bits
/// (`reference_dispatch_entries`). The body pointer is owned by this artifact's code mapping.
/// The VM publishes the epoch last after a successful target return and a table-slot recheck;
/// a nonzero epoch is the validity bit.
#[repr(C)]
#[derive(Clone, Copy)]
struct FastReferenceCache {
    epoch: u64,
    env: Value,
    table: u64,
    index: u64,
    signature: u64,
    function: u64,
    body: u64,
}

/// Certificates an artifact keeps per indirect call site (`reference_dispatch_entries`): a site
/// whose targets vary dispatches through all of them (JetStream richards-wasm sends each task
/// through one site to one of four functions; a certificate per site held only the last).
const REFERENCE_DISPATCH_ENTRIES_PER_SITE: usize = 4;

/// The most certificates an artifact keeps, however many sites it has: a direct-mapped cache
/// whose collisions cost only a callout.
const MAX_REFERENCE_DISPATCH_ENTRIES: usize = 256;

/// The certificates of an artifact with `sites` indirect call sites, indexed by the table
/// index's low bits: a power of two, none without sites.
fn reference_dispatch_entries(sites: usize) -> usize {
    if sites == 0 {
        return 0;
    }
    (sites * REFERENCE_DISPATCH_ENTRIES_PER_SITE)
        .next_power_of_two()
        .min(MAX_REFERENCE_DISPATCH_ENTRIES)
}

impl Default for FastReferenceCache {
    fn default() -> Self {
        Self {
            epoch: 0,
            env: Value::NULL,
            table: 0,
            index: 0,
            signature: 0,
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

/// Whether `field` of `instruction` is the unused second operand of a one-operand SIMD
/// operator, which lowering leaves naming register 0 and nothing reads.
fn unary_simd_spare(instruction: WideInstruction, field: crate::bytecode::InstructionField) -> bool {
    instruction.op() == Op::WasmSimd
        && field == crate::bytecode::InstructionField::C
        && crate::wasm::simd::SimdKernel::of(instruction.imm())
            .is_some_and(|(kernel, _)| kernel.right.is_none())
}

/// The machine type compiled code holds a Wasm vector in (`Slot::Vector`).
const VECTOR_IR: ir::Type = types::I8X16;

/// The words of a SIMD kernel callout (`compiled_simd_kernel`), by position: each operand
/// and the result take two, low then high; a scalar sits in the low word.
const SIMD_KERNEL_LEFT: usize = 0;
const SIMD_KERNEL_RIGHT: usize = 2;
const SIMD_KERNEL_RESULT: usize = 4;
const SIMD_KERNEL_WORDS: usize = 6;

/// Applies the SIMD operator `selector` names to the operand words at `words` and writes
/// its result there (`SIMD_KERNEL_RESULT`): the kernel the interpreter runs, so compiled
/// code needs no lowering of its own for an operator.
unsafe extern "C" fn compiled_simd_kernel(selector: u64, words: *mut u64) {
    let (kernel, lane) =
        crate::wasm::simd::SimdKernel::of(selector as u32).expect("planned SIMD operator");
    // SAFETY: the emitter passes its slot of SIMD_KERNEL_WORDS words.
    let words = unsafe { std::slice::from_raw_parts_mut(words, SIMD_KERNEL_WORDS) };
    let operand = |at: usize| u128::from(words[at]) | (u128::from(words[at + 1]) << u64::BITS);
    let result = kernel.apply_bits(operand(SIMD_KERNEL_LEFT), operand(SIMD_KERNEL_RIGHT), lane);
    words[SIMD_KERNEL_RESULT] = result as u64;
    words[SIMD_KERNEL_RESULT + 1] = (result >> u64::BITS) as u64;
}

/// `memory.copy` within one memory, whose ranges the caller checked: overlapping ranges copy
/// as if through a temporary buffer.
unsafe extern "C" fn compiled_memory_copy(destination: u64, source: u64, length: u64) -> i64 {
    // Wasm32 operands are zero-extended before this call; they fit every supported host's
    // address size.
    unsafe {
        std::ptr::copy(
            source as usize as *const u8,
            destination as usize as *mut u8,
            length as usize,
        );
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
    entry_count > 0 && entry_count.is_multiple_of(JIT_NATIVE_ENTRY_SAMPLE_INTERVAL)
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
                cache.table = u64::from(table_slot);
                cache.index = index;
                cache.signature = signature_index;
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
pub(super) fn native_entry_signature(
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
pub(super) enum NativeEntryMode {
    /// Use the signal-resuming entry, required by guarded memory and poll-page faults.
    Resumable,
    /// Use the same ABI directly for a bounded leaf with no possible native fault.
    Leaf,
}

/// The verified Number result's machine view in the common native result word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeNumberResult { Int32, Float64 }

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
    /// Target certificates for native direct dispatch, by table index
    /// (`reference_dispatch_entries`). These do not retain heap values and are
    /// invalidated by the VM epoch.
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
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("QUENCH_WASM_JIT_STATS").is_some_and(|value| value == "1"))
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
        index: usize,
        target_function: usize,
    ) -> bool {
        let entries = self.reference_fast_caches.len();
        let Some(cache) = (entries != 0)
            .then(|| self.reference_fast_caches.get(index & (entries - 1)))
            .flatten()
        else {
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
        Self::assemble(bytes.len(), |_, code| {
            code.copy_from_slice(bytes);
            Ok(())
        })
        .ok()
    }

    /// Map `length` writable bytes, let `fill` write the code knowing its final
    /// address, then make the mapping read-only and executable. Copy-and-patch
    /// publication needs the address to resolve PC-relative continuations.
    pub(super) fn assemble(
        length: usize,
        fill: impl FnOnce(*mut u8, &mut [u8]) -> Result<(), String>,
    ) -> Result<Self, String> {
        let requested = length;
        let length = length.max(1);
        // `mmap` rounds to whole pages. Store that rounded length explicitly
        // so the JS tier's budget reflects its executable mappings.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        let page_size = usize::try_from(page_size).map_err(|_| "page size is unavailable")?;
        let length = length
            .checked_next_multiple_of(page_size)
            .ok_or("code mapping size overflow")?;
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
            return Err("code mapping failed".into());
        }
        let memory = Self { pointer: pointer.cast::<u8>(), length };
        // SAFETY: the mapping is `length >= requested` writable bytes owned by `memory`.
        let code = unsafe { std::slice::from_raw_parts_mut(memory.pointer, requested) };
        fill(memory.pointer, code)?;
        // SAFETY: the same mapping, now read-only and executable.
        if unsafe { libc::mprotect(memory.pointer.cast(), length, libc::PROT_READ | libc::PROT_EXEC) } != 0 {
            return Err("code mapping could not be made executable".into());
        }
        Ok(memory)
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
    /// Own-field descriptors embed shape IDs. Shape compaction retires this code
    /// through its lane view before execution can resume with remapped IDs.
    pub(super) embeds_shape_ids: bool,
}

impl CompiledOsrRegion {
    /// Runs the region on `registers` with `entry`'s record, through the native entry every
    /// compiled artifact shares (`enter_native`).
    ///
    /// # Safety
    /// `registers` is the running activation's register file, at least `Function::registers`
    /// long; `entry` was built for this region from the current values of its arrays and
    /// captures. Capacity helpers may refresh array backing words without collecting;
    /// `vm` and `interrupt` belong to
    /// this lane activation, which excludes other mutable VM access while native code runs.
    pub(super) unsafe fn run<H: Host>(
        &self,
        registers: *mut crate::value::Value,
        entry: &mut LoopEntry,
        region: &LoopRegion,
        vm: *mut Vm<H>,
        program: ProgramId,
        function: u32,
        frame: usize,
        interrupt: *const crate::execution_interrupt::ExecutionInterrupt,
    ) -> LoopOutcome {
        // SAFETY: the caller keeps the VM alive and excludes other mutable VM access while
        // native code runs. The interrupt reference is used only to construct raw ABI fields.
        let mut context = CallContext::interrupt_only(unsafe { &*interrupt });
        // Keep the immutable residual alive independently of the mutable VM borrow while the
        // native region invokes its no-guest-effect field or initialized-local probe.
        let (program_data, diagnostics_enabled) = unsafe {
            let vm = &*vm;
            (vm.programs.get(program), vm.programs.js_jit_diagnostics_enabled())
        };
        let mut payload = OsrProbePayload {
            region,
            entry,
            function,
            frame,
            program: program_data,
            diagnostics_enabled,
        };
        context.vm = vm.cast();
        context.program = program;
        context.callout = osr_probe::<H> as OsrProbeCallout as usize;
        context.adapter_payload = (&mut payload as *mut OsrProbePayload).cast();
        let mut word = 0u64;
        // SAFETY: as the caller promises; the code reads and writes only those and the
        // result word.
        let status = unsafe {
            enter_native(
                &mut context,
                registers.addr() as u64,
                0,
                entry.words_mut(),
                &mut word,
                NativeEntryMode::Resumable,
                self.entry,
            )
        };
        if let Some(Raised::Panic(panic)) = context.raised.take() {
            std::panic::resume_unwind(panic);
        }
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

/// Analyze a loop candidate whose calls run inlined (`osr_loop::inline_region`), each
/// callee from `resolve` of the cached field its function is read from.
#[allow(clippy::too_many_arguments)]
pub(super) fn analyze_inlining_osr_region<'a>(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[crate::value::Value],
    sites: &[crate::bytecode::FieldSite],
    live: &crate::compile::liveness::RegisterMasks,
    header: u32,
    backedge: u32,
    resolve: &mut dyn FnMut(&[FieldUse], Option<u16>) -> Option<InlineCallee<'a>>,
) -> Option<LoopRegion> {
    osr_loop::inline_region(function, decoded, constants, sites, live, header, backedge, resolve)
}

/// Analyze a loop candidate through the shared region compiler's OSR mode.
/// Analyze a loop candidate and preserve the function PC that blocked admission.
pub(super) fn analyze_osr_region_detailed(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[crate::value::Value],
    sites: &[crate::bytecode::FieldSite],
    live: Option<&std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
    header: u32,
    backedge: u32,
) -> Result<LoopRegion, RegionAnalysisFailure> {
    osr_loop::region(function, decoded, constants, sites, live, header, backedge)
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

/// Whether the JavaScript function tiers (the per-function tier and the general-dispatch
/// tier, and the loop tier's site analysis) compile under each memory policy (`MemoryPolicy`
/// order). Memory-first compiles
/// none, on the owner's direction that it may trade Score for memory under `--jit` (TOD-123).
/// Under memory-first their code and regalloc2 state set the `--jit` peaks: the general-dispatch
/// tier took V8-v7 Crypto `--jit` from 3220 to 5496 KiB (median, bolted static x86_64). With
/// both off, every V8-v7 `--jit` row peaked 84-380 KiB lower, at its no-JIT level. Scores moved
/// both ways: EarleyBoyer +125%, Splay +15%, RayTrace +11%, NavierStokes +8%, but DeltaBlue -9%,
/// Richards -7%, RegExp -7% (same unbolted static binary, 5 runs, 2026-10-24).
const JS_FUNCTION_TIERS: [bool; crate::MemoryPolicy::ALL.len()] = [true, false];

/// Whether the JavaScript function tiers compile under `policy` (`JS_FUNCTION_TIERS`).
pub(super) fn js_function_tiers_compile(policy: crate::MemoryPolicy) -> bool {
    #[cfg(feature = "gc-tuning")]
    {
        // Measurement builds may turn the function tiers off under any policy
        // (`QUENCH_JS_JIT_TIERS=0`), to separate their `--jit` peak from the loop tier's.
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let enabled = *ENABLED.get_or_init(|| {
            std::env::var("QUENCH_JS_JIT_TIERS").map_or(true, |value| value != "0")
        });
        if !enabled {
            return false;
        }
    }
    JS_FUNCTION_TIERS[policy as usize]
}

/// Compile one admitted OSR region and publish it through the shared executable-memory path.
pub(super) fn compile_osr_region(
    region: &LoopRegion,
    observation: &Observation<'_>,
    interrupt: &crate::execution_interrupt::ExecutionInterrupt,
    policy: crate::MemoryPolicy,
    track_backedges: bool,
) -> Option<CompiledOsrRegion> {
    #[cfg(feature = "gc-tuning")]
    if loop_tier_region_budget_spent() {
        return None;
    }
    let isa = super::jit_backend::target_isa(LOOP_TIER_OPT_LEVELS[policy as usize]?)?;
    let verified = osr_loop::verify_region(region, observation).ok()?;
    // A row-recurrence stencil nest computes its rows in pairs (`wavefront`); any other region
    // takes the loop tier's lowering.
    let wavefront = wavefront::recognize(region)
        .and_then(|nest| wavefront::lower(&verified, &nest, &*isa, poll_address(interrupt)));
    let (function, widenings) = match wavefront {
        Some(function) => (function, std::collections::BTreeMap::new()),
        None => osr_loop::lower_verified(verified, observation, &*isa, poll_address(interrupt), track_backedges)?,
    };
    let clif = native_dump_directory().map(|_| function.display().to_string());
    let (_code, entry) = compile_and_publish_entry(function, &*isa, osr_loop::MAX_CODE_BYTES)?;
    if let (Some(directory), Some(clif)) = (native_dump_directory(), clif) {
        dump_native_code(directory, region.header, &_code, &clif);
    }
    Some(CompiledOsrRegion {
        _code,
        entry,
        widenings,
        embeds_shape_ids: observation.native_field_loads.iter().any(|loads| !loads.is_empty()),
    })
}

/// The process's peak resident set in KiB, for `QUENCH_JS_JIT_PEAK_TRACE`.
#[cfg(feature = "gc-tuning")]
fn process_peak_rss_kib() -> i64 {
    // SAFETY: `getrusage` writes into the struct this call owns.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: as above; RUSAGE_SELF names this process.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return 0;
    }
    // Darwin counts bytes, Linux KiB.
    #[cfg(target_vendor = "apple")]
    {
        usage.ru_maxrss / 1024
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        usage.ru_maxrss
    }
}

/// Whether the loop tier has compiled as many regions as `QUENCH_LOOP_TIER_MAX_REGIONS`
/// allows; unlimited when unset. Measurement builds only (`gc-tuning`): it exists to trade
/// region compilation against the `--jit` peak that regalloc2's state sets
/// (`LOOP_TIER_OPT_LEVELS`).
#[cfg(feature = "gc-tuning")]
fn loop_tier_region_budget_spent() -> bool {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COMPILED: AtomicUsize = AtomicUsize::new(0);
    static LIMIT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let limit = *LIMIT.get_or_init(|| {
        std::env::var("QUENCH_LOOP_TIER_MAX_REGIONS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(usize::MAX)
    });
    COMPILED.fetch_add(1, Ordering::Relaxed) >= limit
}

/// The directory `QUENCH_JS_JIT_DUMP` names, read once per process: each loop region
/// compiled writes its machine code and its CLIF there (`dump_native_code`), for attaching
/// instruction-level profiles to native code.
fn native_dump_directory() -> Option<&'static std::path::Path> {
    static DIRECTORY: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    DIRECTORY.get_or_init(|| std::env::var_os("QUENCH_JS_JIT_DUMP").map(std::path::PathBuf::from)).as_deref()
}

/// Writes a compiled loop region's executable mapping as `loop-<n>.bin` and its CLIF as
/// `loop-<n>.clif` into `directory`, numbered in compilation order, and reports where the
/// mapping lives on stderr. A failed write leaves the run as it is.
fn dump_native_code(directory: &std::path::Path, header: u32, code: &CodeMemory, clif: &str) {
    // SAFETY: the mapping is `code.length` readable bytes while `code` lives.
    let bytes = unsafe { std::slice::from_raw_parts(code.pointer(), code.length) };
    dump_native_bytes(directory, header, bytes, Some(code.pointer()), clif);
}

/// Publication is diagnostic data, not a reason to allocate executable memory.
fn dump_native_bytes(directory: &std::path::Path, header: u32, bytes: &[u8], address: Option<*mut u8>, clif: &str) {
    static COMPILED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let number = COMPILED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let _ = std::fs::write(directory.join(format!("loop-{number:03}.bin")), bytes);
    let _ = std::fs::write(directory.join(format!("loop-{number:03}.clif")), clif);
    match address {
        Some(address) => eprintln!("QUENCH_JS_JIT_DUMP loop-{number:03} header={header} address={address:p} bytes={}", bytes.len()),
        None => eprintln!("QUENCH_JS_JIT_DUMP loop-{number:03} header={header} unpublished bytes={}", bytes.len()),
    }
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
    .ok()
}

/// Eager publication has no code-cache admission budget: all requested bodies
/// must compile or the whole program fails. Owners release code and operands together.
#[cfg(feature = "aot")]
pub(super) fn compile_eager_native<H: Host>(
    program: ResidualProgram, options: super::native_graph::NativeJsCodegenOptions,
) -> Result<crate::NativeJsProgram<H>, String> {
    #[cfg(feature = "jit")]
    if options.backend == super::native_graph::NativeJsBackend::Stencil && super::native_stencil::available() {
        return super::native_stencil::compile_program(program).map(|(native, _)| native);
    }
    use super::specialize::Specializer;
    program.freeze_native_diagnostics();
    let isa = super::jit_backend::target_isa_for("speed", super::jit_backend::CodePublication::RelocatableObject)
        .ok_or("native target unavailable")?;
    let extension = EagerCraneliftExtension::<H> { isa, options, entries: Vec::new(), owners: Vec::new(), host: std::marker::PhantomData };
    let (entries, owners) = super::specialize::CallThreaded(extension).specialize(&program)?;
    // SAFETY: every entry was lowered from its exact function for host H.
    unsafe { crate::NativeJsProgram::from_entries(program, entries, Rc::new(owners)) }
}

/// The call-threaded generating extension for live Cranelift code: semantic
/// entries and operand records are bound by address, so nothing is linked.
#[cfg(feature = "aot")]
struct EagerCraneliftExtension<H> {
    isa: std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>,
    options: super::native_graph::NativeJsCodegenOptions,
    entries: Vec<crate::NativeJsFunctionEntry>,
    /// Each function's code, private regions first, and the sites whose
    /// operand records that code addresses.
    owners: Vec<(Vec<CodeMemory>, Box<[super::specialize::InstructionSite]>)>,
    host: std::marker::PhantomData<fn(&mut H)>,
}

#[cfg(feature = "aot")]
impl<H: Host> super::specialize::CallThreadedExtension for EagerCraneliftExtension<H> {
    type Residual = (Vec<crate::NativeJsFunctionEntry>, Vec<(Vec<CodeMemory>, Box<[super::specialize::InstructionSite]>)>);

    fn function(&mut self, function_id: u32, sites: Vec<super::specialize::InstructionSite>) -> Result<(), String> {
        let isa = &*self.isa;
        let sites = sites.into_boxed_slice();
        let mut code = Vec::new();
        if let Some(regions) = self.options.regions(sites.len()) {
            for region in regions.iter().cloned() {
                code.push(compile_eager_body::<H>(&sites, function_id, isa, Some(region))?);
            }
            let mut signature = None;
            let wrapper = super::native_graph::lower_region_dispatch(function_id, sites.len(),
                self.options.region_instruction_budget.get(), regions.len(), isa,
                |builder, index, context, pc, result| {
                    let signature = *signature.get_or_insert_with(|| builder.import_signature(
                        native_entry_signature(isa.pointer_type(), isa.default_call_conv())));
                    let target = builder.ins().iconst(isa.pointer_type(), code[index].pointer() as usize as i64);
                    let arguments = super::native_graph::region_call_arguments(builder, isa, context, pc, result);
                    let call = builder.ins().call_indirect(signature, target, &arguments);
                    builder.inst_results(call)[0]
                })?;
            code.push(publish_eager_graph(wrapper, isa)?);
        } else { code.push(compile_eager_body::<H>(&sites, function_id, isa, None)?); }
        // SAFETY: the final body or wrapper implements the public native entry
        // ABI; owner retains every private region and immutable operand array.
        self.entries.push(unsafe { std::mem::transmute::<*mut u8, crate::NativeJsFunctionEntry>(code.last().expect("native entry compiled").pointer()) });
        self.owners.push((code, sites));
        Ok(())
    }

    fn finish(self) -> Result<Self::Residual, String> {
        Ok((self.entries, self.owners))
    }
}

#[cfg(feature = "aot")]
fn compile_eager_body<H: Host>(
    sites: &[super::specialize::InstructionSite], function_id: u32, isa: &dyn cranelift_codegen::isa::TargetIsa,
    region: Option<std::ops::Range<usize>>,
) -> Result<CodeMemory, String> {
    let mut signature = None;
    let native = super::native_graph::lower_body(sites, function_id, isa, region,
        |builder, site, context, result| {
            let signature = *signature.get_or_insert_with(|| builder.import_signature(native_js_operation_signature(isa)));
            let target = builder.ins().iconst(isa.pointer_type(), site.op.native_semantic_entry::<H>() as usize as i64);
            let arguments = builder.ins().iconst(isa.pointer_type(), site.operands.as_ptr() as i64);
            let pc = builder.ins().iconst(types::I64, site.pc as i64);
            let call = builder.ins().call_indirect(signature, target, &[context, pc, arguments, result]);
            builder.inst_results(call)[0]
        })?;
    publish_eager_graph(native, isa).map_err(|error| format!("native function {function_id}: {error}"))
}

#[cfg(feature = "aot")]
fn publish_eager_graph(function: ir::Function, isa: &dyn cranelift_codegen::isa::TargetIsa) -> Result<CodeMemory, String> {
    super::jit_backend::compile_function(function, isa, None, |compiled, _| {
        if !compiled.buffer.relocs().is_empty() { return Err(NativeCompileRejection::Relocations); }
        CodeMemory::new(compiled.code_buffer()).ok_or(NativeCompileRejection::Publish)
    }).map_err(|error| error.reason().to_owned())
}

use super::jit_backend::NativeCompileRejection;

/// Publication policy for relocation-free JIT entries. Compilation itself keeps
/// symbolic references for consumers that link functions before publication.
fn compile_native_function<T>(
    function: ir::Function,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    max_code_bytes: usize,
    publish: impl FnOnce(&[u8]) -> Option<T>,
) -> Result<T, NativeCompileRejection> {
    super::jit_backend::compile_function(function, isa, Some(max_code_bytes), |compiled, _| {
        if !compiled.buffer.relocs().is_empty() {
            return Err(NativeCompileRejection::Relocations);
        }
        publish(compiled.code_buffer()).ok_or(NativeCompileRejection::Publish)
    })
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
    general_dispatch_native_pcs: Vec<u8>,
    /// Confirmed entry guards that rejected a completed specialized artifact.
    /// This is distinct from `call` returning `None`, which can mean an interrupt.
    specialized_guard_misses: std::cell::Cell<u8>,
    #[cfg(test)]
    native_entries: std::cell::Cell<u64>,
    #[cfg(test)]
    general_dispatch_island_steps: std::cell::Cell<u64>,
    #[cfg(test)]
    general_dispatch_global_name_cache_hits: std::cell::Cell<u64>,
}

#[derive(Clone, Copy)]
enum CompiledJsEntry {
    SharedNumber { entry: Entry, mode: NativeEntryMode, result: NativeNumberResult },
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
    Number(Value),
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
    start_pc: usize,
    #[cfg(feature = "native-execution")]
    stop_pc: usize,
    native_registers: *mut Value,
    register_count: usize,
    native_locals: *mut Value,
    local_count: usize,
    native_pcs: *const u8,
    native_pc_count: usize,
    /// Canonical receiver slot when `this` has already been initialized. Callouts
    /// may move frames or initialize a derived constructor, so refresh this before
    /// returning to generated code. Direct field reads do not cross a safepoint.
    fixed_this_slot: *const Value,
    /// Dense, function-owned capture slots for this frame and collection epoch. Refresh
    /// after every semantic callout; generated reads do not cross a safepoint.
    capture_values: *const Value,
    capture_len: usize,
    /// Stable base of this VM's direct-array record reservation. The field is
    /// refreshed after VM callouts because the reservation may be lazy.
    array_record_base: usize,
    /// The heap facts generated field reads use (`emit_js_own_field_get`): the object
    /// record reservation, the property value arena and its length, and the field cache
    /// entries of `program_id`'s sites; zero where absent. A callout may grow or compact any
    /// of them, so each is read again after one (`refresh_heap_bases`).
    object_record_base: usize,
    object_mark_blocks: usize,
    object_mark_block_count: usize,
    /// Canonical allocator header in this entry's live VM. Its address is fixed
    /// through callouts; growth/compaction update its base/length in place.
    property_buffer: usize,
    field_entries: usize,
    /// The names method caches hold (`Vm::method_caches_may_name`), one bit each, and its
    /// length in words; and whether the VM and program run specialized, which a cached store
    /// requires (`Vm::cached_field_store_hit`).
    method_cache_atoms: usize,
    method_cache_atom_words: usize,
    stores_specialized: usize,
    /// A lane body's receiver (`GeneralDispatchMode::Lane`); `DELETED` elsewhere.
    receiver: u64,
    /// A lane body's VM and running residual, which its semantic helpers take.
    vm: usize,
    residual: usize,
    /// Immutable function admitted with `residual` by `JsNativeActivation`.
    body: *const crate::bytecode::Function,
    /// A lane body's scratch window (`LaneFrame::scratch`), where a leave inside an inlined
    /// callee writes the callee's registers.
    inlined: usize,
    /// A lane body's running lane context, which its native calls make activations in.
    lane_context: usize,
    /// The record a lane body's completion runs (`LANE_BODY_RETURNED`).
    returned_pc: usize,
    /// A lane run's free lane-stack values, where a direct call places its callee's
    /// registers (`DirectCallee`): the next and the end of the stack's capacity.
    native_top: usize,
    native_end: usize,
    /// The next free and the end of the run's shadow frames (`LaneFrame::shadow`).
    shadow_top: usize,
    shadow_end: usize,
    #[cfg(test)]
    island_steps: u64,
    #[cfg(test)]
    global_name_cache_hits: u64,
    outcome: Option<JsNativeStep>,
}

/// A general-dispatch body specialized for frameless lane activations
/// (`GeneralDispatchMode::Lane`): it runs on the activation's own registers, so they stay
/// the interpreter's exact state, and leaves at the PC of any instruction it does not run.
/// The length below which a lane body without loops or inlined calls declines
/// (`Vm::compile_lane_method`).
const LANE_BODY_MIN_INSTRUCTIONS: usize = 24;

pub(super) struct LaneMethod {
    _code: CodeMemory,
    entry: Entry,
    /// An OSR-only body keeps normal calls on their baseline entry.
    first_entry: bool,
    recovery_space: super::dispatch_fast::LaneRecoverySpace,
    /// The lowering requires an owned, resumable poll-page capability.
    fault_poll: bool,
    /// Whether its code names shape ids as constants, which a shape compaction renumbers:
    /// the compaction unlinks it (`LaneView::invalidate_shape_bindings`), so no entry checks them.
    pub(super) names_shapes: bool,
    /// The calls it runs inlined (`lane_unit::unit`), by frame.
    pub(super) inlined: Vec<InlinedCall>,
    /// The scratch registers a leave inside an inlined call fills (`LaneFrame::scratch`): one
    /// per register of the inlined callees, the unit's past the caller's.
    scratch_width: usize,
    caller_registers: u16,
    /// The loop header PC of its function a run may also start at (`Vm::run_lane_method`).
    pub(super) loop_header: Option<u32>,
    /// Whether a call's entry of it pays for itself (`LaneMethod::amortizes_entry`).
    amortizes: bool,
    /// The callees it calls by their compiled entries, which its code names.
    pub(super) direct: Box<[DirectCallee]>,
    /// Whether every call it makes is inlined or direct: under a direct caller's native
    /// frames the lane's helper call misses (`dispatch_fast::lane_body_call`), so only such
    /// a body is called directly.
    closed: bool,
}

impl LaneMethod {
    pub(super) fn supports_first_entry(&self) -> bool { self.first_entry }
    pub(super) fn recovery_space(&self) -> super::dispatch_fast::LaneRecoverySpace { self.recovery_space }
    /// The registers of the function it runs, which an inlined callee's follow.
    pub(super) fn caller_registers(&self) -> u16 {
        self.caller_registers
    }

    /// The address of its compiled entry, which a direct caller calls (`DirectCallee`).
    pub(super) fn entry_address(&self) -> usize {
        self.entry as usize
    }

    /// Whether an entry, which costs a few hundred instructions the lane's own run of a
    /// short leaf never pays, has work to amortize it: a loop, an inlined call, or length
    /// (`LANE_BODY_MIN_INSTRUCTIONS`). A short leaf runs in the lane, or inlined.
    pub(super) fn amortizes_entry(&self) -> bool {
        self.first_entry && self.amortizes
    }

    /// Whether a direct caller may call it (`LaneMethod::closed`).
    pub(super) fn calls_only_natively(&self) -> bool {
        self.first_entry && self.closed
    }
}

/// Weakly published code. Retirement runs only outside native execution, before
/// a dead callee locator can be reused; it releases the code, not a guest root.
/// A direct caller holds its callee's publication (`DirectCallee::method`) and is
/// retired with it (`ProgramStore::retire_dead_native_calls`).
pub(super) struct PublishedLaneMethod(std::cell::UnsafeCell<Option<LaneMethod>>);

impl PublishedLaneMethod {
    pub(super) fn new(method: LaneMethod) -> Self { Self(std::cell::UnsafeCell::new(Some(method))) }
    pub(super) fn method(&self) -> Option<&LaneMethod> {
        // SAFETY: retirement requires that no method reference remains live.
        unsafe { &*self.0.get() }.as_ref()
    }
    /// # Safety
    /// No native body is running and no reference returned by `method` is held.
    pub(super) unsafe fn retire(&self) {
        // SAFETY: the lifecycle edge excludes active native code and borrowers.
        unsafe { &mut *self.0.get() }.take();
    }
}

/// Where a run of a lane body starts (`Vm::run_lane_method`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LaneStart {
    First,
    /// Its loop header (`LaneMethod::loop_header`), from the lane's backward jump there.
    LoopHeader,
}

/// Where a lane body leaves: at a PC of its own function, or at a PC of an inlined callee
/// whose activation the lane then makes (`Vm::lane_body_inlined_activation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LaneExit {
    At(usize),
    Inlined { frame: usize, pc: u32 },
    Terminated,
    /// A native call made its callee's activation and the lane continues in it
    /// (`dispatch_fast::lane_body_call`).
    Callee,
    /// The activation completes with `value`, by the `Return` or the returning call at `pc`.
    Returned { pc: usize, value: Value },
}

/// The bit of a lane body's result word that marks a leave inside an inlined frame, whose
/// index sits above the callee PC's word.
const INLINED_EXIT: u64 = 1 << 63;
/// The result word of a leave into a native call's callee activation.
const CALLEE_EXIT: u64 = 1 << 62;
/// The status of a lane body whose activation completes: its result word holds the value,
/// and `JsStepDispatch::returned_pc` the record that completes it (`LaneExit::Returned`). It
/// sits below the general dispatch's return statuses and above every small status.
pub(super) const LANE_BODY_RETURNED: u32 = JS_GENERAL_RETURN_STATUS_BASE - 1;

/// What a lane body's native call (`dispatch_fast::lane_body_call`) did: completed, with its
/// result in the call's register; missed before any effect, so the body leaves at the call;
/// or left the lane in the callee's activation it made.
pub(super) const LANE_CALL_COMPLETED: u64 = 0;
pub(super) const LANE_CALL_MISSED: u64 = 1;
pub(super) const LANE_CALL_LEFT: u64 = 2;
/// Termination is terminal: the outer native body must not reconstruct or replay a call.
pub(super) const LANE_CALL_TERMINATED: u64 = 3;

impl LaneExit {
    /// The caller PC a leave or completion is at, with a completion's value written to the
    /// register its record names, where a test reads a `Return`'s result.
    #[cfg(test)]
    pub(super) fn settled(self, registers: &mut [Value], instructions: &[crate::bytecode::WideInstruction]) -> Option<usize> {
        if let Self::Returned { pc, value } = self {
            registers[usize::from(instructions[pc].register_a() & crate::bytecode::REGISTER_MASK)] = value;
        }
        self.caller_pc()
    }
    #[cfg(test)]
    pub(super) fn caller_pc(self) -> Option<usize> {
        match self { Self::At(pc) | Self::Returned { pc, .. } => Some(pc), Self::Inlined { .. } | Self::Callee | Self::Terminated => None }
    }
    pub(super) fn word(self) -> u64 {
        match self {
            Self::At(pc) => pc as u64,
            Self::Inlined { frame, pc } => INLINED_EXIT | (frame as u64) << u32::BITS | u64::from(pc),
            Self::Callee => CALLEE_EXIT,
            Self::Returned { .. } | Self::Terminated => unreachable!("a completion is a status, not a leave word"),
        }
    }

    fn decode(word: u64) -> Self {
        if word == CALLEE_EXIT {
            return Self::Callee;
        }
        if word & INLINED_EXIT == 0 {
            return Self::At(usize::try_from(word).unwrap_or(0));
        }
        Self::Inlined { frame: ((word & !INLINED_EXIT) >> u32::BITS) as usize, pc: word as u32 }
    }
}

/// Lane bodies compiled by this process, for tests that a lane activation ran natively.
#[cfg(test)]
pub(super) static LANE_METHODS_COMPILED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Calls lane bodies compiled by this process inline, for tests that inlining happened.
#[cfg(test)]
pub(super) static LANE_CALLS_INLINED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A lane body's callout. Every unsupported instruction and failed guard leaves native
/// code here, with nothing of that instruction done; the lane executes it next.
unsafe extern "C" fn lane_method_exit(_context: u64, pc: u64, result: u64, _unused: u64) -> i64 {
    // SAFETY: the body passes the entry's result word, which `run_lane_method` owns.
    unsafe { (result as *mut u64).write(pc) };
    i64::from(NATIVE_JS_STOPPED)
}

/// A lane body's field read where its own-slot path does not apply: the VM's cache probe
/// (`Vm::cached_field_hit`), which reads prototype holders too; `DELETED` on any miss, which
/// leaves native code for the lane's full read.
unsafe extern "C" fn lane_field_get<H: Host>(vm: u64, residual: u64, receiver: u64, site: u64) -> u64 {
    #[cfg(test)] FIELD_HELPER_CALLS.with(|counts| { let mut values = counts.get(); values[0] += 1; counts.set(values); });
    // SAFETY: the payload holds the running VM and residual, live and unborrowed while the
    // body runs; the probe reads caches and never allocates.
    let (vm, residual) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram)) };
    match vm.cached_field_hit(residual, Value::from_bits(receiver), site as u16) {
        super::object::FieldCacheProbe::Hit(value) => value.to_bits(),
        _ => Value::DELETED.to_bits(),
    }
}

/// A logged own-field store that preserves every native storage projection.
/// Migration declines before writing, leaving the full store to the lane.
unsafe extern "C" fn lane_field_set<H: Host>(vm: u64, residual: u64, receiver: u64, atom: u64, site: u64, value: u64) -> u64 {
    #[cfg(test)] FIELD_HELPER_CALLS.with(|counts| { let mut values = counts.get(); values[1] += 1; counts.set(values); });
    // SAFETY: as `lane_field_get`; a hit stores into an existing slot and allocates nothing.
    let (vm, residual) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram)) };
    u64::from(vm.primary_own_field_store_hit::<true>(
        residual,
        Value::from_bits(receiver),
        atom as Atom,
        Value::from_bits(value),
        site as u16,
        crate::heap::PropertyStoreLayout::Preserve,
    ))
}

/// The same guarded call operation used by native callouts. It never creates
/// a callee frame, invokes guests, collects, or changes a physical layout.
unsafe extern "C" fn lane_guarded_call<H: Host>(vm: u64, residual: u64, program: u64, callee: u64, receiver: u64) -> u64 {
    // SAFETY: the admitted VM/program outlive this no-GC native region.
    let program_id = program as u32;
    let (vm, program) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram)) };
    vm.guarded_call_values(program, ProgramId::from_raw(program_id), Value::from_bits(callee), Value::from_bits(receiver))
        .unwrap_or(Value::DELETED).to_bits()
}

/// Admit the actual closure for a statically selected body without entering it.
unsafe extern "C" fn lane_function_binding<H: Host>(vm: u64, _residual: u64, program: u64, callee: u64, function: u64) -> u64 {
    // SAFETY: the linked unit owns this VM/program throughout the no-GC operation.
    let vm = unsafe { &*(vm as *const Vm<H>) };
    let matches = !vm.direct_eval && !vm.parameter_eval && vm.same_realm_user_target(Value::from_bits(callee))
        .is_some_and(|(owner, id, _)| owner == ProgramId::from_raw(program as u32) && u64::from(id) == function);
    // The admitted body is a leaf. Check its guest-call/interrupt permission
    // here; no nested guest call occurs anywhere in the admitted native body.
    u64::from(matches && vm.enter_guarded_call().is_some())
}

/// Static call-site obligations discharged once during lowering. The helper
/// admits the newly dynamic callee/receiver and its no-frame execution plan.
fn guarded_call_instruction(instruction: WideInstruction) -> bool {
    instruction.op() == Op::Call && instruction.result_flags() == 0
        && !instruction.direct_eval() && !instruction.parameter_eval()
        && instruction.call_window().count == 0
}

/// A lane body's global name read (`Vm::lane_global_read`): its value, or `Value::DELETED`
/// for one the lane's own read would not take, which leaves the body at the read.
unsafe extern "C" fn lane_global_get<H: Host>(vm: u64, residual: u64, atom: u64, site: u64) -> u64 {
    // SAFETY: as `lane_field_get`; the read runs no guest code and allocates nothing.
    let (vm, residual) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram)) };
    vm.lane_global_read(residual, atom as Atom, site as u16).unwrap_or(Value::DELETED).to_bits()
}

/// A lane body's element store: the lane's own in-place replacement of a record array's
/// element (`Vm::try_replace_record_element`) or fill of a hole it may take
/// (`Vm::try_add_record_element`), whose miss changes nothing and leaves the body at the store.
unsafe extern "C" fn lane_index_set<H: Host>(vm: u64, _residual: u64, object: u64, key: u64, value: u64) -> u64 {
    // SAFETY: as `lane_field_get`; a replacement allocates nothing.
    let vm = unsafe { &mut *(vm as *mut Vm<H>) };
    let (object, key, value) = (Value::from_bits(object), Value::from_bits(key), Value::from_bits(value));
    u64::from(vm.try_replace_record_element(object, key, value) || vm.try_add_record_element(object, key, value))
}

/// Admit a depth-zero closure capture without walking, allocating or invoking
/// guest code. Root projections and uninitialized bindings decline exactly as
/// the interpreter's direct-capture projection does.
unsafe extern "C" fn lane_function_capture_read<H: Host>(vm: u64, callee: u64, slot: u64) -> u64 {
    // SAFETY: the lane owns this VM while generated code runs; no collection or
    // reentry occurs while it holds the closure's environment slots.
    let vm = unsafe { &*(vm as *const Vm<H>) };
    let read = || {
        let env = vm.heap.function(Value::from_bits(callee))?.env;
        let (slots, root) = vm.environment_capture_slots(env, false, 0)?;
        vm.owned_capture(slots, root, u16::try_from(slot).ok()?)
    };
    read().unwrap_or(Value::DELETED).to_bits()
}

impl<H: Host> Vm<H> {
    /// The PCs of `unit` whose by-name loads read the global scope as the lane's own does
    /// (`dispatch_fast::lane_load_name_first`): in a function whose unresolved names are
    /// global and resolve statically, a name no internal binding owns.
    fn lane_global_reads(&self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit) -> std::collections::BTreeSet<usize> {
        let global = |function: u32| program.function_get(function as usize)
            .is_some_and(|body| body.names_resolve_statically(program))
            && self.programs.unresolved_names_are_global(self.active_program, function);
        unit.instructions.iter().enumerate()
            .filter(|(_, instruction)| matches!(instruction.op(), Op::LoadName | Op::LoadNameCall))
            .filter(|(pc, instruction)| {
                let atom = instruction.atom_index();
                let function = match unit.origins[*pc] {
                    lane_unit::Origin::Caller(_) => function,
                    lane_unit::Origin::Inlined { frame, .. } => unit.frames[frame].target.function,
                };
                global(function) && (!self.atom_class(atom).contains(super::object::AtomClass::INTERNAL)
                    || crate::bytecode::INTERNAL_GLOBAL_BINDINGS.contains(&self.atom_name(atom)))
            })
            .map(|(pc, _)| pc)
            .collect()
    }

    /// The value a global read of `atom` through cache `site` takes as the lane's own read
    /// does (`dispatch_fast::lane_load_global_name`): its global lexical binding's, else the
    /// global object's cached data property; none for a binding in its TDZ or a miss.
    pub(super) fn lane_global_read(&mut self, code: &ResidualProgram, atom: Atom, site: u16) -> Option<Value> {
        if self.realm.global_lexical_declarations.contains(&atom) {
            return self.realm.global_lexical_bindings.get(&atom).copied().filter(|value| !value.is_deleted());
        }
        match self.cached_field_hit(code, self.realm.globals, site) {
            super::object::FieldCacheProbe::Hit(value) => Some(value),
            _ => None,
        }
    }

    /// The field cache sites `unit` reads or writes, each with the exact shapes and slots its
    /// entries prove now: a read's own or prototype-held (`Vm::native_field_read_descriptors`),
    /// a store's own (`Vm::native_field_load_descriptors`); a nested lookup names one site
    /// per read of its chain, and its sink's store.
    fn lane_unit_fields(&self, program: &ResidualProgram, unit: &lane_unit::Unit) -> std::collections::BTreeMap<u16, Vec<NativeFieldLoad>> {
        unit.instructions
            .iter()
            .flat_map(|instruction| -> Vec<(u16, bool)> {
                match instruction.op() {
                    Op::GetField => match instruction.field_lookup() {
                        Some(FieldLookup::Atom { cache_site, .. }) => vec![(cache_site, true)],
                        Some(FieldLookup::Site(index)) => program.field_sites.get(index).map_or_else(Vec::new, |site| {
                            let reads = [Some(site.first), site.second].into_iter().flatten().map(|(_, cache)| (cache, true));
                            reads.chain(site.sink.map(|(_, cache)| (cache, false))).collect()
                        }),
                        None => Vec::new(),
                    },
                    Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict => vec![(instruction.cache_site_index(), false)],
                    _ => Vec::new(),
                }
            })
            .map(|(site, read)| (site, if read { self.native_field_read_descriptors(site) } else { self.native_field_load_descriptors(site) }))
            .filter(|(_, shapes)| !shapes.is_empty())
            .collect()
    }

    /// Specialize `function` for lane activations, admitting a body only where its first
    /// instruction runs natively: an entry that leaves at once is pure cost.
    pub(super) fn compile_lane_method(
        &mut self,
        program: &ResidualProgram,
        function: u32,
        targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
        captures: &dyn Fn(Value) -> (usize, usize),
        direct: Vec<Option<DirectCallee>>,
        loop_header: Option<u32>,
    ) -> Option<LaneMethod> {
        self.host.before_native_compilation();
        let result = (|| {
            let helpers = LaneHelpers::Process((*LaneHelper::addresses::<H>()).map(|address| address as usize));
            let body = program.function_get(function as usize)?;
            let mut budget = lane_unit::MAX_UNIT_INLINED_INSTRUCTIONS;
            let (unit, region, fields, direct, direct_sites) = loop {
                // A pure numeric body observes neither closure environment nor identity.
                // Bind its definition at the edge, as AOT does, so fresh closures share
                // the same residual while recovery retries the actual caller value.
                let valid_bindings = std::cell::Cell::new(true);
                let bindings = |caller, pc| {
                    let mut bound = targets(caller, pc);
                    for target in &mut bound {
                        let numeric = program.function_get(target.function as usize).is_some_and(|body|
                            body.plain_activation(program) && lane_region::restartable_numeric_function(program, body,
                                &body.instructions().collect::<Vec<_>>()));
                        if numeric {
                            if let Some(callee) = target.identity()
                                && !self.same_realm_user_target(callee).is_some_and(|(owner, function, _)|
                                    owner == self.active_program && function == target.function) {
                                valid_bindings.set(false);
                            }
                            target.callee = CalleeBinding::Function;
                        }
                    }
                    // Different closures of the same pure definition are one case.
                    let mut kept = 0;
                    for index in 0..bound.len() {
                        let target = bound[index];
                        if !bound[..kept].contains(&target) { bound[kept] = target; kept += 1; }
                    }
                    bound.truncate(kept);
                    bound
                };
                // Captured stores already use the shared frameless kernel.
                let unit = lane_unit::with_budget(program, function, body, budget, &bindings, &|callee| program.function_get(callee as usize)
                    .filter(|callee| super::js_compiled::captured_field_store_candidate(program, callee).is_none()))?;
                if !valid_bindings.get() { return None; }
                if !unit.frames.iter().all(|frame| match frame.target.callee {
                    CalleeBinding::Identity(callee) => self.lane_inlined_target_admitted(program, callee, frame.target.function),
                    CalleeBinding::Function => frame.recovery == lane_unit::CallRecovery::RestartCall
                        && program.function_get(frame.target.function as usize).is_some_and(|body|
                            lane_region::restartable_numeric_function(program, body, &body.instructions().collect::<Vec<_>>())),
                }) {
                    return None;
                }
                let loop_entry = match loop_header {
                    Some(header) => Some(unit.origins.iter().position(|origin| *origin == lane_unit::Origin::Caller(header))?),
                    None => None,
                };
                let fields = self.lane_unit_fields(program, &unit);
                let global_reads = self.lane_global_reads(program, function, &unit);
                // The calls the unit still makes (none it inlines) whose callees have bodies.
                // Each candidate owns one contiguous stable slice. A rejected candidate
                // releases it; replanning keeps the original targets available.
                let (pcs, direct): (Vec<usize>, Vec<DirectCallee>) = unit.instructions.iter().enumerate()
                    .filter(|(_, instruction)| instruction.op() == Op::Call)
                    .filter_map(|(pc, _)| match unit.origins[pc] {
                        lane_unit::Origin::Caller(origin) => direct.get(origin as usize)?.as_ref().cloned().map(|callee| (pc, callee)),
                        lane_unit::Origin::Inlined { .. } => None,
                    })
                    .unzip();
                let direct: Box<[DirectCallee]> = direct.into_boxed_slice();
                let direct_sites: std::collections::BTreeMap<usize, *const DirectCallee> =
                    pcs.iter().zip(direct.iter()).map(|(pc, callee)| (*pc, std::ptr::from_ref(callee))).collect();
                let frame_captures: Vec<(usize, usize)> = unit.frames.iter()
                    .map(|frame| frame.target.identity().map_or((0, 0), captures)).collect();
                let allow_first_entry = loop_header.is_none() || self.programs.js_jit_lane_methods_enabled();
                match lane_region::compile(program, function, &unit, helpers, &fields, &global_reads, &direct_sites,
                    &frame_captures, loop_entry, allow_first_entry) {
                    Ok(region) => break (unit, region, fields, direct, direct_sites),
                    Err(lane_region::LaneDecline::Rejected(reason))
                        if reason.exceeds_code_budget() && !unit.frames.is_empty() && budget > 0 => budget /= 2,
                    Err(_) => return None,
                }
            };
            if region.native == 0 {
                return None;
            }
            let loops = unit.instructions.iter().enumerate().any(|(pc, instruction)| {
                instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget
                    && (instruction.imm() as usize) <= pc
            });
            let amortizes = loops || !unit.frames.is_empty() || unit.instructions.len() >= LANE_BODY_MIN_INSTRUCTIONS;
            let code = CodeMemory::new(&region.code)?;
            if let (Some(directory), Some(clif)) = (native_dump_directory(), region.clif.as_deref()) {
                dump_native_code(directory, function, &code, clif);
            }
            // SAFETY: the general dispatcher emits the shared native-entry ABI.
            let entry = unsafe { std::mem::transmute::<*mut u8, Entry>(code.pointer()) };
            #[cfg(test)]
            LANE_METHODS_COMPILED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            #[cfg(test)]
            LANE_CALLS_INLINED.fetch_add(unit.frames.len(), std::sync::atomic::Ordering::Relaxed);
            // A body that names a callee's body retires with it (`DirectCallee::method`).
            let names_shapes = !fields.is_empty() || !direct.is_empty();
            let scratch_width = usize::from(unit.registers - unit.caller_registers);
            let caller_registers = unit.caller_registers;
            let closed = unit.instructions.iter().enumerate()
                .all(|(pc, instruction)| instruction.op() != Op::Call || direct_sites.contains_key(&pc));
            let fault_poll = region.fault_poll || direct.iter().any(|callee| callee.method.method().is_some_and(|method| method.fault_poll));
            let recovery_space = super::dispatch_fast::LaneRecoverySpace::inlined(&unit.frames);
            Some(LaneMethod { _code: code, entry, first_entry: region.first_entry, recovery_space, fault_poll, names_shapes, inlined: unit.frames, scratch_width, caller_registers, loop_header, amortizes, direct, closed })
        })();
        self.host.after_native_compilation();
        result
    }

    /// Run a lane body on an activation's `registers`; the PC the lane continues at. A
    /// native `Return` leaves at its own record, whose register already holds the result.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_lane_method(
        &mut self,
        method: &LaneMethod,
        start: LaneStart,
        direct_calls: bool,
        program: ProgramId,
        residual: &ResidualProgram,
        registers: *mut Value,
        receiver: Value,
        captures: *const crate::heap::EnvironmentSlots,
        lane_context: *mut (),
    ) -> LaneExit {
        let frame = lane_frame();
        // SAFETY: the activation's capture slots outlive its body's run, which collects nothing.
        let (capture_values, capture_len) = unsafe { captures.as_ref() }
            .and_then(crate::heap::EnvironmentSlots::native_dense_view)
            .unwrap_or((std::ptr::null(), 0));
        // SAFETY: the frame lives for the thread and is never moved once made; no reference
        // into it is held across the body's run, which may enter it again
        // (`run_nested_lane_method`).
        let context = unsafe {
            if (*frame).scratch.len() < method.scratch_width {
                (*frame).scratch.resize(method.scratch_width, Value::UNDEFINED);
            }
            let dispatch = &raw mut (*frame).dispatch;
            LaneEntry {
                lane_context: lane_context.addr(),
                native_registers: registers,
                receiver: receiver.to_bits(),
                capture_values,
                capture_len,
                residual: residual as *const ResidualProgram as usize,
            }
            .install(&mut *dispatch);
            (*dispatch).inlined = (*frame).scratch.as_mut_ptr().addr();
            (*dispatch).program_id = program;
            (*dispatch).refresh_object_bases(self);
            (*dispatch).stores_specialized = usize::from(self.specialized && residual.specialized);
            let vm = self as *mut Self;
            (*dispatch).vm = vm.addr();
            (*dispatch).bind_property_buffer(vm);
            // A run starts with no native frames; its direct calls place their callees past
            // the stack's values.
            let (top, end, activations) = self.lane_stack_spare();
            ((*dispatch).native_top, (*dispatch).native_end) = (top.addr(), end.addr());
            let shadow = (*frame).shadow.as_mut_ptr();
            // A run whose lane may not make the frames' activations makes no direct call.
            let frames = if direct_calls { SHADOW_FRAMES.min(activations) } else { 0 };
            ((*dispatch).shadow_top, (*dispatch).shadow_end) = (shadow.addr(), shadow.add(frames).addr());
            let context = &raw mut (*frame).context;
            (*context).adapter_payload = dispatch.cast();
            (*context).interrupt = self.execution_interrupt.atomic_flag() as *const _;
            (*context).interrupt_owner = &self.execution_interrupt;
            context
        };
        let mut word = 0_u64;
        // SAFETY: the body was compiled for this function, never allocates or collects, and
        // reads only the registers, constants and heap bases set above.
        let start = match start {
            LaneStart::First => 0,
            LaneStart::LoopHeader => {
                debug_assert!(method.loop_header.is_some(), "a loop entry runs a body compiled with one");
                1
            }
        };
        let status = if method.fault_poll {
            let poll = native_poll_page(&self.execution_interrupt);
            // SAFETY: this entry owns the TLS binding until it returns. Reentrant
            // entries checkpoint it in `run_nested_lane_method`.
            let context = unsafe { &mut *context };
            context.bind_native_adapter(context.adapter_payload,
                self.execution_interrupt.atomic_flag() as *const _, poll);
            unsafe { enter_native(context, start, 0, registers.cast_const().cast(),
                &mut word, NativeEntryMode::Resumable, method.entry) }
        } else {
            unsafe { (method.entry)(&mut *context, start, 0, registers.cast_const().cast(), &mut word) }
        };
        if status == LANE_BODY_RETURNED {
            // SAFETY: as above; the body wrote the record before it returned.
            let pc = unsafe { (*frame).dispatch.returned_pc };
            return LaneExit::Returned { pc, value: Value::from_bits(word) };
        }
        Self::lane_method_exit(status, word)
    }

    /// The native frames the last run left inside, outermost first, which the lane now
    /// materializes (`ShadowFrame`); none once taken.
    pub(super) fn take_shadow_frames() -> &'static [ShadowFrame] {
        let frame = lane_frame();
        // SAFETY: no lane body runs while the lane reads the frame, and the frames read stay
        // as they are until the next run, which the lane starts only after materializing them.
        unsafe {
            let base = (*frame).shadow.as_mut_ptr();
            let top = (*frame).dispatch.shadow_top as *mut ShadowFrame;
            let count = if top.is_null() { 0 } else { top.offset_from(base) as usize };
            (*frame).dispatch.shadow_top = base.addr();
            std::slice::from_raw_parts(base, count)
        }
    }

    /// Whether a running body has native frames below the innermost: a helper's activation
    /// would sit under them, so a call from one takes the body's exit instead
    /// (`dispatch_fast::lane_body_call`).
    pub(super) fn lane_shadow_active() -> bool {
        let frame = lane_frame();
        // SAFETY: as `take_shadow_frames`; the running body wrote the top before its call.
        unsafe { (*frame).dispatch.shadow_top != (*frame).shadow.as_mut_ptr().addr() }
    }

    /// `run_lane_method` from inside a running body (`dispatch_fast::lane_body_call`): the
    /// caller's body continues with the per-entry words it entered with. Its callee runs in
    /// the same program (`lane_crosses`) and allocates nothing, so the program's bases stand.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_nested_lane_method(
        &mut self,
        method: &LaneMethod,
        program: ProgramId,
        residual: &ResidualProgram,
        registers: *mut Value,
        receiver: Value,
        captures: *const crate::heap::EnvironmentSlots,
        lane_context: *mut (),
    ) -> LaneExit {
        let frame = lane_frame();
        // SAFETY: as `run_lane_method`; the caller's entry made the frame.
        let saved = unsafe { LaneEntry::of(&(*frame).dispatch) };
        // Reentrant entries lend the same TLS context. Preserve the outer
        // recovery landing and its binding before the inner entry overwrites them.
        let binding = unsafe {
            let context = &(*frame).context;
            (context.stack_limit, context.entry_stack, context.interrupt,
                context.adapter_payload, context.poll_start, context.poll_end, context.interrupt_owner)
        };
        let exit = self.run_lane_method(method, LaneStart::First, true, program, residual, registers, receiver, captures, lane_context);
        // SAFETY: as above; the callee's run may have grown the scratch window.
        unsafe {
            saved.install(&mut (*frame).dispatch);
            let context = &mut (*frame).context;
            (context.stack_limit, context.entry_stack, context.interrupt,
                context.adapter_payload, context.poll_start, context.poll_end, context.interrupt_owner) = binding;
            (*frame).dispatch.inlined = (*frame).scratch.as_mut_ptr().addr();
        }
        exit
    }

    /// Hands `write` each of the `count` registers an inlined callee its lane body left
    /// inside held, as the body wrote them to the scratch window (`LaneFrame::scratch`).
    pub(super) fn lane_scratch(start: usize, count: usize, mut write: impl FnMut(u16, Value)) {
        LANE_FRAME.with(|frame| {
            // SAFETY: as in `run_lane_method`; no lane body runs while the lane reads it.
            let Some(frame) = (unsafe { &*frame.get() }).as_ref() else { return };
            for (register, value) in frame.scratch[start..start + count].iter().enumerate() {
                write(register as u16, *value);
            }
        });
    }
}

/// A callee a lane body calls by its compiled entry (`lane_region`'s direct call): the
/// target the lane would activate, fixed by the callee's identity, which the call guards,
/// and its specialized body. The caller's body owns it.
#[derive(Clone)]
pub(super) struct DirectCallee {
    pub(super) target: super::dispatch_fast::ScriptCallTarget,
    /// The callee's published body (`LaneBody::Specialized`): the caller's code calls its entry,
    /// so the caller retires with it, at a shape compaction (`LaneMethod::names_shapes`) or at
    /// a lifecycle edge (`ProgramStore::retire_dead_native_calls`).
    pub(super) method: Rc<PublishedLaneMethod>,
    /// The caller's call record, which the callee's activation returns to.
    pub(super) call: usize,
    pub(super) callee: Value,
    pub(super) registers: u16,
    /// The values past its registers a leave inside a call it inlines takes
    /// and the activations that recovery needs, proved free with its own.
    pub(super) reserve: super::dispatch_fast::LaneRecoverySpace,
    /// The callee binds a sloppy receiver: the call takes only a direct object one directly.
    pub(super) sloppy: bool,
    /// The callee's dense capture slots (`EnvironmentSlots::native_dense_view`).
    pub(super) capture_values: *const Value,
    pub(super) capture_len: usize,
}

/// A native frame of a direct call (`DirectCallee`), which the lane materializes as an
/// activation only if a run leaves inside it (`Vm::take_shadow_frames`).
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct ShadowFrame {
    /// The caller's call record.
    pub(super) call: usize,
    pub(super) callee: *const DirectCallee,
    /// The callee's registers, in the lane stack's capacity past the caller's.
    pub(super) window: *mut Value,
    pub(super) this: u64,
}

impl ShadowFrame {
    const EMPTY: Self = Self { call: 0, callee: std::ptr::null(), window: std::ptr::null_mut(), this: 0 };
}

/// Direct calls a run nests natively: past them, a call takes the lane's helper path. The
/// machine stack each costs a frame; the bound keeps a native recursion within it.
const SHADOW_FRAMES: usize = 64;

/// The words of `JsStepDispatch` one lane body entry owns (`Vm::run_lane_method`).
#[derive(Clone, Copy)]
struct LaneEntry {
    lane_context: usize,
    native_registers: *mut Value,
    receiver: u64,
    capture_values: *const Value,
    capture_len: usize,
    residual: usize,
}

impl LaneEntry {
    fn of(dispatch: &JsStepDispatch) -> Self {
        Self {
            lane_context: dispatch.lane_context,
            native_registers: dispatch.native_registers,
            receiver: dispatch.receiver,
            capture_values: dispatch.capture_values,
            capture_len: dispatch.capture_len,
            residual: dispatch.residual,
        }
    }

    fn install(self, dispatch: &mut JsStepDispatch) {
        dispatch.lane_context = self.lane_context;
        dispatch.native_registers = self.native_registers;
        dispatch.receiver = self.receiver;
        dispatch.capture_values = self.capture_values;
        dispatch.capture_len = self.capture_len;
        dispatch.residual = self.residual;
    }
}

/// The context and payload every lane body on this thread runs with
/// (`Vm::run_lane_method`). Thread facts are set once; each entry writes its own.
struct LaneFrame {
    context: CallContext,
    dispatch: JsStepDispatch,
    /// The registers of an inlined callee its lane body left inside.
    scratch: Vec<Value>,
    /// The native frames of the bodies a run calls directly, outermost first, from
    /// `dispatch.shadow_top` down (`ShadowFrame`).
    shadow: Box<[ShadowFrame; SHADOW_FRAMES]>,
}

impl LaneFrame {
    fn new() -> Self {
        let mut context = CallContext::native_adapter(std::ptr::null_mut(), std::ptr::null(), None);
        context.callout = lane_method_exit as unsafe extern "C" fn(u64, u64, u64, u64) -> i64 as usize;
        Self {
            context,
            dispatch: JsStepDispatch {
                program_id: ProgramId::MAIN,
                frame: usize::MAX,
                allow_inline_calls: false,
                start_pc: 0,
                #[cfg(feature = "native-execution")]
                stop_pc: JS_NO_STOP_PC,
                native_registers: std::ptr::null_mut(),
                register_count: 0,
                native_locals: std::ptr::null_mut(),
                local_count: 0,
                native_pcs: std::ptr::null(),
                native_pc_count: 0,
                fixed_this_slot: std::ptr::null(),
                capture_values: std::ptr::null(),
                capture_len: 0,
                array_record_base: 0,
                object_record_base: 0,
                object_mark_blocks: 0,
                object_mark_block_count: 0,
                property_buffer: 0,
                field_entries: 0,
                method_cache_atoms: 0,
                method_cache_atom_words: 0,
                stores_specialized: 0,
                receiver: Value::DELETED.to_bits(),
                vm: 0,
                residual: 0,
                body: std::ptr::null(),
                inlined: 0,
                lane_context: 0,
                returned_pc: 0,
                native_top: 0,
                native_end: 0,
                shadow_top: 0,
                shadow_end: 0,
                #[cfg(test)]
                island_steps: 0,
                #[cfg(test)]
                global_name_cache_hits: 0,
                outcome: None,
            },
            scratch: Vec::new(),
            shadow: Box::new([ShadowFrame::EMPTY; SHADOW_FRAMES]),
        }
    }
}

thread_local! {
    static LANE_FRAME: std::cell::UnsafeCell<Option<LaneFrame>> = const { std::cell::UnsafeCell::new(None) };
}

/// This thread's lane frame, made at its first lane body run.
fn lane_frame() -> *mut LaneFrame {
    let frame = LANE_FRAME.with(std::cell::UnsafeCell::get);
    // SAFETY: the frame is reached only through this pointer, on its thread, and no
    // reference into it outlives the statement that makes it.
    unsafe { (*frame).get_or_insert_with(LaneFrame::new) as *mut LaneFrame }
}

impl<H: Host> Vm<H> {
    fn lane_method_exit(status: u32, word: u64) -> LaneExit {
        match status {
            status if i64::from(status) == TERMINATED_STATUS => LaneExit::Terminated,
            NATIVE_JS_STOPPED => LaneExit::decode(word),
            status if status >= JS_GENERAL_RETURN_STATUS_BASE => LaneExit::At((status - JS_GENERAL_RETURN_STATUS_BASE) as usize),
            // Dispatch reaches an invalid PC only before any instruction ran.
            _ => LaneExit::At(0),
        }
    }
}

/// The frame/register/root and exception boundary shared by JIT and linked entries.
fn run_js_native_entry<H: Host>(
    entry: Entry, vm: *mut Vm<H>, program: ProgramId, payload: *mut (),
    interrupt: *const std::sync::atomic::AtomicBool, poll_page: Option<(usize, usize)>,
    native_pcs: &[u8],
) -> Option<(u32, u64)> {
    // Taking the idle slot leaves reentrant calls no access to this live context.
    let mut context = match unsafe { (*vm).js_native_context.take() } {
        Some(mut context) => {
            context.bind_native_adapter(payload, interrupt, poll_page);
            context
        }
        None => Box::new(CallContext::native_adapter(payload, interrupt, poll_page)),
    };
    context.vm = vm.cast();
    context.interrupt_owner = unsafe { &(*vm).execution_interrupt };
    context.program = program;
    // Linked entries import their fixed semantic operations directly.
    if crate::DYNAMIC_EXECUTION_ENABLED {
        context.callout = js_step_callout::<H> as Callout as usize;
    }
    let constants = unsafe { &(*vm.cast::<Vm<H>>()).programs }.constants(program);
    context.js_constants = constants.as_ptr();
    context.js_constant_count = constants.len();
    let (native_registers, native_locals, local_count) = unsafe {
        let dispatch = &mut *payload.cast::<JsStepDispatch>();
        dispatch.native_pcs = native_pcs.as_ptr();
        dispatch.native_pc_count = native_pcs.len();
        dispatch.bind_property_buffer(vm);
        (dispatch.native_registers, dispatch.native_locals, dispatch.local_count)
    };
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
    let raised = context.raised.take();
    // Idle JS adapters own no guest roots or raised completions. Nested
    // entries may already have returned a reusable context; retain only one.
    let idle = unsafe { &mut (*vm).js_native_context };
    if idle.is_none() {
        *idle = Some(context);
    }
    match raised {
        Some(Raised::Panic(panic)) => std::panic::resume_unwind(panic),
        Some(Raised::Error(_)) => return None,
        None => {}
    }
    Some((status, word))
}

/// Owns immutable metadata for one resumable native activation. Holding the
/// program prevents unique-owner record release while its dispatch borrows it.
/// Reentrant entries own separate activations, never a VM-local ID in TLS.
struct JsNativeActivation {
    _program: Rc<ResidualProgram>,
    dispatch: JsStepDispatch,
}

impl JsNativeActivation {
    /// Construct and consume one owned activation in the entry's scope. The
    /// large dispatch does not cross an optional aggregate-return boundary.
    #[inline(always)]
    fn with<H: Host, R>(vm: &mut Vm<H>, program_id: ProgramId, frame: usize, start_pc: usize,
        allow_inline_calls: bool, run: impl FnOnce(&mut Self, &mut Vm<H>) -> R) -> Option<R> {
        let program = vm.programs.get(program_id)?;
        let body = program.function_get(vm.frames[frame].function as usize)? as *const _;
        let activation = &mut vm.frames[frame];
        let mut native = Self { dispatch: JsStepDispatch {
            program_id, frame, allow_inline_calls, start_pc,
            #[cfg(feature = "native-execution")]
            stop_pc: JS_NO_STOP_PC,
            native_registers: activation.registers.as_mut_ptr(),
            register_count: activation.registers.len(),
            native_locals: activation.locals.as_mut_ptr(),
            local_count: activation.locals.len(),
            native_pcs: std::ptr::null(), native_pc_count: 0, fixed_this_slot: std::ptr::null(),
            capture_values: std::ptr::null(), capture_len: 0,
            array_record_base: 0, object_record_base: 0, object_mark_blocks: 0,
            object_mark_block_count: 0, property_buffer: 0,
            field_entries: 0, method_cache_atoms: 0, method_cache_atom_words: 0,
            stores_specialized: 0,
            receiver: Value::DELETED.to_bits(),
            vm: vm as *mut Vm<H> as usize,
            residual: Rc::as_ptr(&program) as usize,
            body,
            inlined: 0,
            lane_context: 0,
            returned_pc: 0,
            native_top: 0,
            native_end: 0,
            shadow_top: 0,
            shadow_end: 0,
            #[cfg(test)]
            island_steps: 0,
            #[cfg(test)]
            global_name_cache_hits: 0,
            outcome: None,
        }, _program: program };
        native.dispatch.refresh_heap_bases(vm, &native._program);
        Some(run(&mut native, vm))
    }
}

impl JsStepDispatch {
    /// The VM stays in place throughout this entry. Derive the owner address
    /// from the same full-VM pointer as semantic callouts, without creating a
    /// shared borrow that later mutable callouts would invalidate. Rebind on
    /// every entry, including reuse of the thread-local lane frame by another VM.
    fn bind_property_buffer<H: Host>(&mut self, vm: *mut Vm<H>) {
        let offset = std::mem::offset_of!(Vm<H>, heap) + crate::heap::Heap::PROPERTY_BUFFER_OFFSET;
        self.property_buffer = vm.cast::<u8>().wrapping_add(offset) as usize;
    }

    /// Object loads, stores and barriers share these bases in lane and general code.
    /// Allocation or collection between native entries may move any of them.
    fn refresh_object_bases<H: Host>(&mut self, vm: &Vm<H>) {
        self.object_record_base = vm.heap.object_record_base().map_or(0, |base| base as usize);
        self.array_record_base = vm.heap.array_record_base().map_or(0, |base| base as usize);
        let (blocks, count) = vm.heap.native_object_mark_blocks();
        (self.object_mark_blocks, self.object_mark_block_count) = (blocks as usize, count);
        let atoms = vm.method_cache_atoms.words();
        (self.method_cache_atoms, self.method_cache_atom_words) = (atoms.as_ptr() as usize, atoms.len());
    }

    /// Refresh the additional frame and array views read by the general tier.
    fn refresh_heap_bases<H: Host>(&mut self, vm: &Vm<H>, program: &ResidualProgram) {
        self.refresh_object_bases(vm);
        self.fixed_this_slot = vm.frames.get(self.frame)
            .filter(|frame| frame.fixed_this)
            .map_or(std::ptr::null(), |frame| &frame.this as *const Value);
        (self.capture_values, self.capture_len) = vm.frames.get(self.frame)
            .and_then(|_| vm.current_capture_slots(self.frame))
            .and_then(|(slots, root)| {
                if root.is_some() { return None; }
                // SAFETY: the frame's current memo owns these slots through this
                // collection epoch. Only dense, non-projected storage is admitted.
                unsafe { &*slots }.native_dense_view()
            })
            .unwrap_or((std::ptr::null(), 0));
        self.field_entries = vm.program_field_entries(self.program_id).unwrap_or(0);
        self.stores_specialized = usize::from(vm.specialized && program.specialized);
    }
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

/// Dynamic dispatch selects its opcode from the instruction, not a specialization.
const DYNAMIC_JS_OPCODE: usize = Op::COUNT;

unsafe extern "C" fn js_step_callout<H: Host>(
    context: *mut CallContext,
    function: u64,
    arguments: *const u64,
    result: *mut u64,
) -> i64 {
    // SAFETY: the native entry owns the context and result storage.
    unsafe { js_semantic_callout::<H, false, DYNAMIC_JS_OPCODE>(context, function, arguments, result) }
}

struct StaticJsOperation<H: Host> {
    opcode: Op,
    execute: fn(&mut Vm<H>, &ResidualProgram, usize, WideInstruction, &mut usize, bool)
        -> Result<StepResult, JsError>,
}

/// A statically selected semantic helper's C ABI. Operands use `NativeJsOperands`.
#[cfg(feature = "native-execution")]
pub type NativeJsOperationEntry = unsafe extern "C" fn(
    *mut std::ffi::c_void, u64, *const u64, *mut u64,
) -> i64;

/// Resolve a compiler opcode identity to its derived semantic helper. The opcode
/// inventory generates this mapping; linked entries never dispatch an opcode.
#[cfg(feature = "native-execution")]
pub fn native_js_operation_entry_for<H: Host>(opcode: usize) -> Option<NativeJsOperationEntry> {
    Op::from_index(opcode).map(Op::native_semantic_entry::<H>)
}

#[cfg(feature = "aot")]
pub(super) fn native_js_operation_signature(
    isa: &dyn cranelift_codegen::isa::TargetIsa,
) -> Signature {
    let pointer = isa.pointer_type();
    let mut signature = Signature::new(isa.default_call_conv());
    for parameter in [pointer, types::I64, pointer, pointer] {
        signature.params.push(AbiParam::new(parameter));
    }
    signature.returns.push(AbiParam::new(types::I64));
    signature
}

pub(super) const NATIVE_JS_STOPPED: u32 = 1;
pub(super) const NATIVE_JS_INVALID_PC: u32 = 2;
#[cfg(feature = "native-execution")]
const JS_NO_STOP_PC: usize = usize::MAX;

#[cfg(feature = "profile-native-ops")]
fn native_operation_fallback_kind<H: Host>(vm: &Vm<H>, frame: usize, instruction: WideInstruction) -> &'static str {
    if instruction.op() == Op::JumpBinaryFalse {
        let Some((strict, _)) = super::operations::equality_operator(instruction.binary_operator_field()) else {
            return "branch-relation";
        };
        let registers = &vm.frames[frame].registers;
        let constants = vm.programs.constants(vm.frames[frame].program);
        let value = |operand: Operand| match operand.kind() {
            Some(OperandKind::Register) => registers.get(usize::from(operand.payload())).copied(),
            Some(OperandKind::Constant) => constants.get(usize::from(operand.payload())).copied(),
            _ => None,
        };
        let (Some(left), Some(right)) = (value(instruction.operand_b()), value(instruction.operand_c())) else {
            return "branch-computed-operand";
        };
        return if left.is_null() || left.is_undefined() || right.is_null() || right.is_undefined() {
            if strict { "branch-strict-nullish" } else { "branch-loose-nullish" }
        } else if left.as_bool().is_some() || right.as_bool().is_some() {
            if strict { "branch-strict-boolean" } else { "branch-loose-boolean" }
        } else if left.direct_object_index().is_some() || right.direct_object_index().is_some() {
            "branch-direct-object"
        } else {
            "branch-other-equality"
        };
    }
    if instruction.op() == Op::LoadCapture {
        if instruction.capture_depth() != 0 {
            return "capture-outer-depth";
        }
        let Some((slots, root)) = vm.current_capture_slots(frame) else {
            return "capture-cache-miss";
        };
        // SAFETY: current_capture_slots checked the frame and collection epoch.
        let slots = unsafe { &*slots };
        return match slots.state(usize::from(instruction.capture_slot())) {
            Some(crate::heap::EnvironmentSlotState::Owned(value)) if !value.is_deleted() => {
                if vm.owned_capture(slots, root, instruction.capture_slot()).is_none() {
                    "capture-root-projection"
                } else if root.is_some() {
                    "capture-root-owned"
                } else {
                    "capture-function-owned"
                }
            }
            Some(crate::heap::EnvironmentSlotState::Owned(_)) => "capture-tdz",
            Some(crate::heap::EnvironmentSlotState::Shared(_)) => "capture-shared",
            None => "capture-slot-miss",
        };
    }
    if instruction.op() != Op::GetField {
        return "other";
    }
    if vm.frames[frame].function == 0 {
        return "root";
    }
    if instruction.writes_current_this() || instruction.numeric_local_target().is_some()
        || !(instruction.result_flags() == 0
            || (instruction.returns_from_frame() && instruction.result_flags() == RETURN_REGISTER))
    {
        return "result-flags";
    }
    let Some(FieldLookup::Atom { base, cache_site, .. }) = instruction.field_lookup() else {
        return "nested";
    };
    let receiver = if base == FieldBase::THIS {
        if !vm.frames[frame].fixed_this { return "uninitialized-this"; }
        vm.frames[frame].this
    } else if let Some(register) = base.register_index() {
        let Some(value) = vm.frames[frame].registers.get(usize::from(register)) else { return "invalid-register"; };
        *value
    } else {
        return "unsupported-base";
    };
    if receiver.direct_object_index().is_none() {
        return "non-direct-object";
    }
    let Some(object) = vm.heap.object(receiver) else { return "missing-object"; };
    if object.inline_properties().is_some() {
        "inline-storage"
    } else if object.plain_storage_start().is_none() {
        "extras-storage"
    } else {
        if vm.active_program != vm.frames[frame].program {
            return "inactive-program";
        }
        let cache = vm.field_cache_entry(cache_site);
        if cache.holder != NO_FIELD_HOLDER {
            if cache.receiver != object.exact_shape() {
                "prototype-receiver-shape-miss"
            } else if let Some(record) = object.proto_link().object_record_id() {
                let holder = vm.heap.object_record_by_id(record);
                if holder.exact_shape() != cache.holder {
                    "prototype-holder-shape-miss"
                } else if holder.inline_properties().is_some() {
                    "prototype-holder-inline"
                } else if holder.extras_storage_start().is_some() {
                    "prototype-holder-extras"
                } else if holder.plain_storage_start().is_some() {
                    "prototype-holder-plain"
                } else {
                    "prototype-holder-other-storage"
                }
            } else {
                "prototype-non-record"
            }
        } else if cache.receiver != object.exact_shape() {
            "own-shape-miss"
        } else {
            "own-shape-match"
        }
    }
}

#[cfg(feature = "aot")]
pub(super) fn native_js_start_pc(builder: &mut FunctionBuilder<'_>, context: ir::Value) -> ir::Value {
    let pointer = builder.func.signature.params[0].value_type;
    let payload = builder.ins().load(
        pointer, MemFlagsData::trusted(), context,
        std::mem::offset_of!(CallContext, adapter_payload) as i32,
    );
    builder.ins().load(
        types::I64, MemFlagsData::trusted(), payload,
        std::mem::offset_of!(JsStepDispatch, start_pc) as i32,
    )
}

/// Execute one compile-time-selected semantic operation and return its exact
/// continuation. This entry never batches subsequent bytecodes.
///
/// # Safety
/// `context` must be a live shared native-entry context for `H`, with its
/// frame, program and adapter payload rooted through this call. `result` must
/// point to a writable result word. `arguments` must point to an aligned,
/// live `NativeJsOperands` from this compiler build, matching the instruction
/// at the supplied PC. Operand structure is resolved before native execution.
#[cfg(feature = "native-execution")]
pub unsafe extern "C" fn native_js_operation_entry<H: Host, const OPCODE: usize>(
    context: *mut std::ffi::c_void,
    pc: u64,
    arguments: *const u64,
    result: *mut u64,
) -> i64 {
    #[cfg(feature = "profile-native-ops")]
    super::native_op_profile::record(OPCODE);
    // SAFETY: forwarded from this entry's native ABI contract. Invalid opcode
    // constants are reported inside the shared exception boundary.
    unsafe { js_semantic_callout::<H, true, OPCODE>(context.cast(), pc, arguments, result) }
}

unsafe fn js_semantic_callout<H: Host, const SINGLE_OPERATION: bool, const OPCODE: usize>(
    context: *mut CallContext,
    function: u64,
    arguments: *const u64,
    result: *mut u64,
) -> i64 {
    // Keep the opcode static through the shared exception/continuation boundary,
    // so specialization resolves the operation and its guards in this body.
    let operation = const {
        if SINGLE_OPERATION {
            match Op::from_index(OPCODE) {
                Some(opcode) => Some(StaticJsOperation {
                    opcode,
                    execute: Vm::<H>::resolve_step_operation(opcode),
                }),
                None => None,
            }
        } else {
            None
        }
    };
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
        // SAFETY: the owning JsNativeActivation admits both pointers once and
        // retains the shared program until native execution (including reentry)
        // finishes. Record release requires unique ownership of that program.
        let program = unsafe { &*(dispatch.residual as *const ResidualProgram) };
        let body = unsafe { &*dispatch.body };
        #[cfg(feature = "native-execution")]
        if SINGLE_OPERATION && dispatch.stop_pc == pc {
            let frame = &mut vm.frames[dispatch.frame];
            frame.pc = pc;
            frame.pc_is_resume = true;
            return Ok(JsCalloutStep::Stop(JsNativeStep {
                instruction_pc: pc, next_pc: pc, result: Ok(StepResult::Continue),
            }));
        }
        let packed = *body
            .code
            .get(pc)
            .ok_or_else(|| JsError::validation("native JS PC is outside function".into()))?;
        let instruction = if SINGLE_OPERATION {
            let selected = operation.as_ref().ok_or_else(||
                JsError::validation("invalid native semantic opcode".into()))?;
            // Linked operands are already resolved. Admission needs only the
            // source opcode, not a second decoding of every operand field.
            let source_opcode = if packed.is_wide() {
                body.wide[packed.wide_index()].op()
            } else {
                packed.op()
            };
            if selected.opcode != source_opcode {
                return Err(JsError::validation("native semantic opcode does not match instruction".into()));
            }
            if arguments.is_null() {
                return Err(JsError::validation("native semantic operands are missing".into()));
            }
            #[cfg(feature = "profile-native-ops")]
            {
                let source_instruction = super::dispatch_fast::decoded_instruction(body, packed);
                super::native_op_profile::record_site(
                    dispatch.program_id.raw(), vm.frames[dispatch.frame].function, pc, source_instruction,
                    body.name.or(body.inferred_name)
                        .and_then(|atom| program.atoms.get(atom as usize)),
                    || native_operation_fallback_kind(vm, dispatch.frame, source_instruction),
                );
            }
            // SAFETY: linked code supplies resolved operands under the entry's
            // ABI contract; the source opcode check precedes all guest effects.
            unsafe { &*arguments.cast::<super::native_operands::NativeJsOperands>() }
                .instruction(selected.opcode)
        } else {
            super::dispatch_fast::decoded_instruction(body, packed)
        };
        let next_pc = pc
            .checked_add(1)
            .ok_or_else(|| JsError::validation("native JS PC overflow".into()))?;

        let native_pc = !SINGLE_OPERATION && !dispatch.native_pcs.is_null()
            && pc < dispatch.native_pc_count
            // SAFETY: the compiled artifact owns this byte map for the entire
            // native callout and the PC is bounded by its recorded length.
            && unsafe { *dispatch.native_pcs.add(pc) != 0 };
        if !SINGLE_OPERATION && native_pc
            && matches!(instruction.op(), Op::LoadName | Op::LoadNameCall | Op::LoadNameTypeof)
            && !vm.realm.global_lexical_declarations.contains(&instruction.atom_index())
            && program
                .atoms
                .get(instruction.atom_index() as usize)
                .is_some_and(|name| {
                    !name.starts_with('\0') && !crate::bytecode::is_intrinsic_name(name)
                })
        {
            let globals = vm.realm.globals;
            let cache_site = instruction.cache_site_index();
            let probe = vm.cached_field_hit(program, globals, cache_site);
            if vm.programs.js_jit_diagnostics_enabled() {
                vm.programs.record_js_jit_field_cache_probe(
                    dispatch.program_id,
                    vm.frames[dispatch.frame].function,
                    cache_site,
                    matches!(&probe, super::object::FieldCacheProbe::Hit(_)),
                );
            }
            if let super::object::FieldCacheProbe::Hit(value) = probe {
                #[cfg(test)]
                {
                    dispatch.global_name_cache_hits =
                        dispatch.global_name_cache_hits.saturating_add(1);
                }
                let frame = &mut vm.frames[dispatch.frame];
                frame.registers[usize::from(instruction.result_register())] = value;
                if instruction.op() == Op::LoadNameCall {
                    frame.registers[usize::from(instruction.register_b())] = Value::UNDEFINED;
                }
                frame.pc = next_pc;
                frame.pc_is_resume = true;
                return Ok(JsCalloutStep::Continue(next_pc));
            }
        }

        let frame = &mut vm.frames[dispatch.frame];
        // Entry admits these homes, and every effectful continuation validates
        // them before native code resumes. No frame-storage mutation occurs
        // between that edge and this callout; polling here duplicates its proof.
        frame.pc = pc;
        frame.pc_is_resume = false;
        if let Err(error) = vm.check_execution_interrupt() {
            return Ok(JsCalloutStep::Stop(JsNativeStep {
                instruction_pc: pc,
                next_pc,
                result: Err(error),
            }));
        }

        if !SINGLE_OPERATION && instruction.op() == Op::GetField
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
            let probe = vm.cached_field_hit(program, receiver, cache_site);
            if vm.programs.js_jit_diagnostics_enabled() {
                vm.programs.record_js_jit_field_cache_probe(
                    dispatch.program_id,
                    vm.frames[dispatch.frame].function,
                    cache_site,
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
                return Ok(JsCalloutStep::Continue(next_pc));
            }
        }

        if !SINGLE_OPERATION && instruction.op().js_denotation() == JsDenotation::FieldStore
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
                program,
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
                return Ok(JsCalloutStep::Continue(next_pc));
            }
        }

        let frame_depth = vm.frames.len();
        let frame_function = vm.frames[dispatch.frame].function;
        let mut current_pc = pc;
        let mut current_instruction = instruction;
        let mut next = next_pc;
        loop {
            let layout = current_instruction.op().control_flow_layout();
            let is_call = matches!(current_instruction.op(), Op::Call | Op::CallArray);
            let diagnostics_enabled = is_call && vm.programs.js_jit_diagnostics_enabled();
            let direct_call_target = if diagnostics_enabled && is_call {
                vm.frames[dispatch.frame]
                    .registers
                    .get(usize::from(current_instruction.register_b()))
                    .copied()
                    .and_then(|callee| vm.heap.function(callee))
                    .and_then(|record| match &record.kind {
                        crate::heap::FunctionKind::User(program, function) => {
                            Some((program.raw(), *function))
                        }
                        _ => None,
                    })
            } else {
                None
            };
            let sample_call = diagnostics_enabled
                && is_call
                && vm.programs.should_sample_js_jit_call_site(
                    dispatch.program_id,
                    vm.frames[dispatch.frame].function,
                    current_pc,
                    current_instruction.op(),
                );
            let call_sample_started = sample_call.then(std::time::Instant::now);
            let capture = if current_instruction.op() == Op::LoadCapture {
                vm.cached_direct_capture(
                    dispatch.frame,
                    current_instruction.capture_depth(),
                    current_instruction.capture_slot(),
                )
            } else {
                None
            };
            #[cfg(feature = "native-execution")]
            let guarded_call_result = if SINGLE_OPERATION && dispatch.allow_inline_calls && is_call {
                vm.native_guarded_call(program, dispatch.program_id, dispatch.frame, current_instruction)
            } else {
                None
            };
            #[cfg(not(feature = "native-execution"))]
            let guarded_call_result: Option<Value> = None;
            // Static AOT helpers and JIT dispatch offer the same guarded frame
            // transition before falling back to the call's semantic operation.
            let in_loop_call = if guarded_call_result.is_none()
                && dispatch.allow_inline_calls
                && is_call
            {
                vm.try_inline_user_call(program, dispatch.frame, current_instruction, next)
            } else {
                None
            };
            let step = if let Some(value) = capture {
                let destination = vm.frames[dispatch.frame]
                    .registers
                    .get_mut(usize::from(current_instruction.result_register()))
                    .ok_or_else(|| JsError::validation(
                        "native JS capture result register is invalid".into(),
                    ))?;
                *destination = value;
                Ok(super::StepResult::Continue)
            } else if let Some(value) = guarded_call_result {
                let destination = vm.frames[dispatch.frame].registers
                    .get_mut(usize::from(current_instruction.result_register()))
                    .ok_or_else(|| JsError::validation("native JS call result register is invalid".into()))?;
                *destination = value;
                Ok(super::StepResult::Continue)
            } else if let Some(step) = in_loop_call {
                step
            } else if SINGLE_OPERATION {
                (operation.as_ref().expect("validated semantic operation").execute)(
                    vm, program, dispatch.frame, current_instruction, &mut next,
                    dispatch.allow_inline_calls,
                )
            } else {
                (Vm::<H>::resolve_step_operation(current_instruction.op()))(
                    vm,
                    program,
                    dispatch.frame,
                    current_instruction,
                    &mut next,
                    dispatch.allow_inline_calls,
                )
            };
            if is_call && diagnostics_enabled {
                let outcome = match &step {
                    Ok(super::StepResult::Continue) => {
                        super::program_store::JsJitCallOutcome::Continue
                    }
                    Ok(super::StepResult::TailCall) => {
                        super::program_store::JsJitCallOutcome::TailCall
                    }
                    Ok(super::StepResult::PushFrame { .. }) => {
                        super::program_store::JsJitCallOutcome::PushFrame
                    }
                    Ok(super::StepResult::Return(_)) => {
                        super::program_store::JsJitCallOutcome::Return
                    }
                    Ok(super::StepResult::Await { .. }) => {
                        super::program_store::JsJitCallOutcome::Await
                    }
                    Ok(super::StepResult::Yield { .. }) => {
                        super::program_store::JsJitCallOutcome::Yield
                    }
                    Err(_) => super::program_store::JsJitCallOutcome::Error,
                };
                vm.programs.record_js_jit_call_site(
                    dispatch.program_id,
                    frame_function,
                    current_pc,
                    current_instruction.op(),
                    direct_call_target,
                    outcome,
                    call_sample_started.map(|started| {
                        u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
                    }),
                );
            }
            match step {
                Ok(super::StepResult::Continue) => {
                    if next >= body.code.len() {
                        vm.frames[dispatch.frame].pc = current_pc;
                        vm.frames[dispatch.frame].pc_is_resume = false;
                        return Ok(JsCalloutStep::Stop(JsNativeStep {
                            instruction_pc: current_pc,
                            next_pc: next,
                            result: Err(JsError::validation(
                                "native JS continuation is outside its function".into(),
                            )),
                        }));
                    }
                    let frame = &mut vm.frames[dispatch.frame];
                    frame.pc = next;
                    frame.pc_is_resume = true;
                    if guarded_call_result.is_some() {
                        // A hit is the shared no-frame/no-GC operation's proof:
                        // only an existing numeric field may have changed. Frame,
                        // capture, arena and cache storage still use their admitted
                        // addresses; checking and rebinding them repeats that proof.
                        return Ok(JsCalloutStep::Continue(next));
                    }
                    if !js_native_frame_storage_unchanged(frame, dispatch) {
                        // The operation ran once. Return the transition to the
                        // frame owner at its already-computed successor.
                        frame.js_jit_skip_entry_once = next == 0;
                        vm.active_registers = frame.registers.as_mut_ptr();
                        return Ok(JsCalloutStep::Stop(JsNativeStep {
                            instruction_pc: current_pc,
                            next_pc: next,
                            result: Ok(super::StepResult::Continue),
                        }));
                    }
                    if vm.frames.len() != frame_depth
                        || vm.frames[dispatch.frame].function != frame_function
                    {
                        // Do not carry a native continuation across a call or
                        // return that changed the active VM frame.
                        return Ok(JsCalloutStep::Stop(JsNativeStep {
                            instruction_pc: current_pc,
                            next_pc: next,
                            result: Ok(super::StepResult::Continue),
                        }));
                    }

                    let fallthrough = layout == ControlFlowLayout::Fallthrough
                        && next == current_pc.saturating_add(1);
                    let resumes_native = next < dispatch.native_pc_count
                        // SAFETY: the compiler emits one byte for every PC in
                        // this function and the artifact outlives this entry.
                        && unsafe { *dispatch.native_pcs.add(next) != 0 };
                    if SINGLE_OPERATION || !fallthrough || resumes_native {
                        // Only generated code consumes these raw views. A stopped
                        // dispatch discards them, and a batched VM operation will
                        // refresh once at the boundary before native execution.
                        dispatch.refresh_heap_bases(vm, program);
                        return Ok(JsCalloutStep::Continue(next));
                    }

                    current_pc = next;
                    let Some(packed) = body.code.get(current_pc).copied() else {
                        return Ok(JsCalloutStep::Stop(JsNativeStep {
                            instruction_pc: current_pc,
                            next_pc: current_pc,
                            result: Err(JsError::validation(
                                "native JS continuation is outside function".into(),
                            )),
                        }));
                    };
                    current_instruction =
                        super::dispatch_fast::decoded_instruction(body, packed);
                    next = current_pc
                        .checked_add(1)
                        .ok_or_else(|| JsError::validation("native JS PC overflow".into()))?;
                    let frame = &mut vm.frames[dispatch.frame];
                    frame.pc = current_pc;
                    frame.pc_is_resume = false;
                    if let Err(error) = vm.check_execution_interrupt() {
                        return Ok(JsCalloutStep::Stop(JsNativeStep {
                            instruction_pc: current_pc,
                            next_pc: next,
                            result: Err(error),
                        }));
                    }
                    #[cfg(test)]
                    {
                        let dispatch = unsafe {
                            &mut *context_ref.adapter_payload.cast::<JsStepDispatch>()
                        };
                        dispatch.island_steps = dispatch.island_steps.saturating_add(1);
                    }
                }
                result => {
                    return Ok(JsCalloutStep::Stop(JsNativeStep {
                        instruction_pc: current_pc,
                        next_pc: next,
                        result,
                    }));
                }
            }
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
        let general_dispatch_native_pcs = artifact.general_dispatch_native_pcs;
        let (entry, return_constants, capture_argument) = match artifact.result {
            super::js_compiled::JsJitResultKind::SharedNumberEntry(mode, result) => {
                // SAFETY: the bytecode numeric-region compiler uses `native_entry_signature`.
                let entry = unsafe { std::mem::transmute::<*mut u8, Entry>(code.pointer) };
                (CompiledJsEntry::SharedNumber { entry, mode, result }, Vec::new(), None)
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
            general_dispatch_native_pcs,
            specialized_guard_misses: std::cell::Cell::new(0),
            #[cfg(test)]
            native_entries: std::cell::Cell::new(0),
            #[cfg(test)]
            general_dispatch_island_steps: std::cell::Cell::new(0),
            #[cfg(test)]
            general_dispatch_global_name_cache_hits: std::cell::Cell::new(0),
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

    /// Record an entry rejected by a specialized artifact's explicit guard.
    /// The returned value is bounded and saturates at the promotion threshold.
    pub(super) fn note_specialized_guard_miss(&self, threshold: u8) -> u8 {
        let misses = self.specialized_guard_misses.get().saturating_add(1).min(threshold);
        self.specialized_guard_misses.set(misses);
        misses
    }

    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn specialized_guard_misses(&self) -> u8 {
        self.specialized_guard_misses.get()
    }

    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn general_dispatch_island_steps(&self) -> u64 {
        self.general_dispatch_island_steps.get()
    }

    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn general_dispatch_global_name_cache_hits(&self) -> u64 {
        self.general_dispatch_global_name_cache_hits.get()
    }

    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn general_dispatch_pc_is_native(&self, pc: usize) -> bool {
        self.general_dispatch_native_pcs.get(pc) == Some(&1)
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
        self.field_argument.is_some() && matches!(self.entry, CompiledJsEntry::SharedNumber { .. })
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
        interrupt: &crate::ExecutionInterrupt,
        poll_page: Option<(usize, usize)>,
    ) -> Option<CompiledJsResult> {
        if self.is_general_dispatch() {
            return None;
        }
        let required = self.params + self.capture_arguments.len() + self.numeric_field_inputs.len()
            + usize::from(self.uses_numeric_field_argument() && self.numeric_field_inputs.is_empty());
        if arguments.len() < required { return None; }
        if let CompiledJsEntry::SharedNumber { entry, result: format, .. } = self.entry {
            let (status, result) = run_native_entry(
                entry,
                arguments,
                interrupt,
                poll_page,
                self.native_entry_mode(),
            );
            #[cfg(test)]
            self.native_entries
                .set(self.native_entries.get().saturating_add(1));
            (status == 0).then_some(())?;
            return Some(CompiledJsResult::Number(match format {
                NativeNumberResult::Int32 => Value::integer(result as u32 as i32),
                NativeNumberResult::Float64 => Value::number(f64::from_bits(result)),
            }));
        }
        self.call_through_shared_entry(JsNativeCallInput::Arguments {
            values: arguments,
            interrupt: interrupt.atomic_flag(),
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
        let (status, word) = run_js_native_entry(entry, vm, program, payload, interrupt, poll_page, &self.general_dispatch_native_pcs)?;
        // The payload retains the pre-entry register homes for storage-change recovery.
        let dispatch = unsafe { &*payload.cast::<JsStepDispatch>() };
        let dispatch_frame = dispatch.frame;
        let register_count = dispatch.register_count;
        let native_registers = dispatch.native_registers;
        #[cfg(test)]
        self.general_dispatch_island_steps.set(
            self.general_dispatch_island_steps
                .get()
                .saturating_add(unsafe { (*payload.cast::<JsStepDispatch>()).island_steps }),
        );
        #[cfg(test)]
        self.general_dispatch_global_name_cache_hits.set(
            self.general_dispatch_global_name_cache_hits
                .get()
                .saturating_add(unsafe {
                    (*payload.cast::<JsStepDispatch>()).global_name_cache_hits
                }),
        );
        if let Some(instruction_pc) = status
            .checked_sub(JS_GENERAL_RETURN_STATUS_BASE)
            .and_then(|pc| usize::try_from(pc).ok())
            .filter(|pc| *pc < self.general_dispatch_native_pcs.len())
        {
            #[cfg(test)]
            self.native_entries
                .set(self.native_entries.get().saturating_add(1));
            return Some(JsNativeDispatchResult::Step(JsNativeStep {
                instruction_pc,
                next_pc: instruction_pc.saturating_add(1),
                result: Ok(super::StepResult::Return(Value::from_bits(word))),
            }));
        }
        match status {
            NATIVE_JS_STOPPED => {
                #[cfg(test)]
                self.native_entries
                    .set(self.native_entries.get().saturating_add(1));
                unsafe { (*payload.cast::<JsStepDispatch>()).outcome.take() }
                    .map(JsNativeDispatchResult::Step)
            }
            NATIVE_JS_INVALID_PC => {
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
            CompiledJsEntry::SharedNumber { mode, .. } => mode,
            CompiledJsEntry::GeneralDispatch(_)
            | CompiledJsEntry::NumberInterruptible(_)
            | CompiledJsEntry::StringHashLoop(_) => {
                NativeEntryMode::Resumable
            }
            CompiledJsEntry::Int32Identity { .. }
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
                Some(CompiledJsResult::Number(Value::number(unsafe { entry(value) })))
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
                (completed != 0).then(|| CompiledJsResult::Number(Value::number(result)))
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
                (completed != 0).then(|| CompiledJsResult::Number(Value::number(result)))
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
                    entry(values.as_ptr()) & super::js_compiled::BOOLEAN_REGION_RESULT_MASK != 0
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
            CompiledJsEntry::SharedNumber { .. } | CompiledJsEntry::GeneralDispatch(_)
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
    /// Per instruction, `None` when unreachable, else the denotation's operand
    /// fact: `c` for vector memory accesses (effective address), `b` otherwise
    /// (a moved value's kind, a global's binding, an import reference). The
    /// full register states exist only while planning.
    operands: Vec<Option<Slot>>,
}

impl Plan {
    /// The plan as its callers see it: only the function being compiled
    /// holds its per-instruction facts.
    fn interface(&self) -> Self {
        Self {
            params: self.params.clone(),
            result: self.result,
            operands: Vec::new(),
        }
    }

    fn reachable(&self, pc: usize) -> bool {
        self.operands[pc].is_some()
    }

    /// The selected operand fact of reachable instruction `pc`.
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
        let (left, right) = if (u64::from(second_sources[0]) == u64::from(sources[0]) + 4
            && u64::from(second_sources[1]) == u64::from(sources[1]) + 4)
            || (u64::from(second_sources[0]) == u64::from(sources[1]) + 4
                && u64::from(second_sources[1]) == u64::from(sources[0]) + 4)
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

    /// The slot a constant defines: a vector's, or its machine kind's (`constant_kind`).
    fn constant_slot(&self, value: Value) -> Slot {
        if self.vm.heap.v128(value).is_some() {
            Slot::Vector
        } else {
            Slot::Value(self.constant_kind(value))
        }
    }

    /// A vector constant's bits.
    fn constant_vector(&self, value: Value) -> Option<u128> {
        self.vm.heap.v128(value).map(u128::from_le_bytes)
    }

    /// Check `register` names the function's memory for a vector access: the memory
    /// register, or a register an instance binding loaded the same unshared memory into
    /// (lowering binds the memory afresh for each such access), which then is the
    /// function's compiled memory (`memory_slot`).
    fn vector_memory(
        &self,
        function: &Function,
        state: &AnalysisState,
        register: u16,
        memory_register: Option<u16>,
        memory_slot: &mut Option<u32>,
    ) -> Option<()> {
        match state.get(register, function.registers)? {
            Slot::Memory if Some(register) == memory_register => {}
            Slot::Binding(slot)
                if self.binds_unshared_memory(slot) && memory_slot.is_none_or(|known| known == slot) =>
            {
                *memory_slot = Some(slot);
            }
            _ => return None,
        }
        memory_slot.is_some_and(|slot| self.supports_compiled_memory(slot)).then_some(())
    }

    /// The byte offset a vector access adds to its address (`Op::WasmMemoryAddress`).
    fn memory_offset(&self, index: usize) -> Option<u64> {
        let offset = self.vm.decode_bits64(self.constant(index)?)?;
        u32::try_from(offset).ok().map(u64::from)
    }

    /// A shuffle's lane selectors: sixteen byte indices into its two operands.
    fn shuffle_lanes(&self, index: usize) -> Option<[u8; crate::wasm::V128_BYTES]> {
        self.vm.heap.v128(self.constant(index)?)
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
                let operand = match instruction.op() {
                    Op::WasmMemoryLoad | Op::WasmMemoryStore => instruction.c(),
                    _ => instruction.b(),
                };
                Some(state.get(operand, function.registers).unwrap_or(Slot::Conflict))
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
                Slot::Memory32Address => Some(Kind::I64),
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
                && !matches!(
                    op,
                    Op::WasmMemorySize
                        | Op::WasmMemoryFill
                        | Op::WasmMemoryCopy
                        | Op::WasmMemoryAddress
                        | Op::WasmMemoryLoad
                        | Op::WasmMemoryStore
                )
                && op != Op::WasmInstanceBinding
                && crate::bytecode::InstructionField::ALL.iter().any(|field| {
                    op.field_layout(*field).reads_register()
                        && instruction.field_value(*field) == memory
                        && !unary_simd_spare(instruction, *field)
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
            Op::Move => match state.get(instruction.b(), function.registers)? {
                slot @ (Slot::Vector | Slot::Memory32Address) => {
                    set(state, instruction.a(), slot)
                }
                _ => {
                    let kind = value(state, instruction.b())?;
                    set(state, instruction.a(), Slot::Value(kind))
                }
            },
            Op::LoadConst => {
                let constant = self.constant(instruction.constant_index())?;
                set(state, instruction.a(), self.constant_slot(constant))
            }
            Op::WasmFillRegisters => {
                let constant = self.constant(instruction.constant_index())?;
                let slot = self.constant_slot(constant);
                let window = instruction.register_window();
                for offset in 0..window.count {
                    set(state, window.base + offset, slot)?;
                }
                Some(())
            }
            // A vector access's effective address: a 32-bit address plus the access's offset,
            // as 64 bits, so the sum cannot wrap.
            Op::WasmMemoryAddress => {
                self.vector_memory(function, state, instruction.c(), memory_register, memory_slot)?;
                self.memory_offset(instruction.constant_index())?;
                i32_value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Memory32Address)
            }
            Op::WasmMemoryLoad | Op::WasmMemoryStore => {
                self.vector_memory(function, state, instruction.b(), memory_register, memory_slot)?;
                if value(state, instruction.c())? != Kind::I64 {
                    return None;
                }
                if op == Op::WasmMemoryLoad {
                    (crate::wasm::memory::MemoryLoad::from_tag(instruction.imm())?
                        == crate::wasm::memory::MemoryLoad::V128Load)
                        .then_some(())?;
                    set(state, instruction.a(), Slot::Vector)
                } else {
                    (crate::wasm::memory::MemoryStore::from_tag(instruction.imm())?
                        == crate::wasm::memory::MemoryStore::V128Store)
                        .then_some(())?;
                    (state.get(instruction.a(), function.registers)? == Slot::Vector).then_some(())
                }
            }
            Op::WasmSimd => {
                let (kernel, lane) = crate::wasm::simd::SimdKernel::of(instruction.imm())?;
                let operand = |register: u16, ty: crate::WasmType| match ty {
                    crate::WasmType::V128 => {
                        (state.get(register, function.registers)? == Slot::Vector).then_some(())
                    }
                    ty => (value(state, register)? == Kind::of(ty)?).then_some(()),
                };
                operand(instruction.b(), kernel.left)?;
                if let Some(right) = kernel.right {
                    operand(instruction.c(), right)?;
                }
                let result = match kernel.result_type(lane) {
                    crate::WasmType::V128 => Slot::Vector,
                    ty => Slot::Value(Kind::of(ty)?),
                };
                set(state, instruction.a(), result)
            }
            Op::WasmSimdShuffle => {
                self.shuffle_lanes(instruction.constant_index())?;
                for register in [instruction.b(), instruction.c()] {
                    (state.get(register, function.registers)? == Slot::Vector).then_some(())?;
                }
                set(state, instruction.a(), Slot::Vector)
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
            // A copy within the function's one memory: both memory operands name it.
            Op::WasmMemoryCopy => {
                let (destination, source) = instruction.register_pair();
                if Some(instruction.a()) != memory_register
                    || Some(instruction.b()) != memory_register
                    || state.get(instruction.a(), function.registers) != Some(Slot::Memory)
                    || !memory_slot.is_some_and(|slot| self.supports_compiled_memory(slot))
                {
                    return None;
                }
                i32_value(state, destination)?;
                i32_value(state, source)?;
                i32_value(state, instruction.c())
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
            Op::WasmUnreachable
                if self.experimental_root_compilation || eager_unreachable_admitted() =>
            {
                Some(())
            }
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
/// branch table the constant decides; bounds the code each copy adds. A
/// dispatch copy repeats its table too, which counts as its edges: TSF's
/// 290-way state table, copied at each of its 503 jumps, compiled one
/// function to 10.6 MB of edge moves (3.5 s, 220 MiB peak); charged by its
/// edges it compiles in 0.1 s.
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
                // The copy repeats every edge of the table, each a jump with its own moves.
                let edges = crate::bytecode::branch_table_entries(pc, instruction).count();
                if path.len() - 1 + edges > THREAD_BUDGET {
                    return None;
                }
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

/// Bind the activation's stable interrupt capability. The pointer is invariant;
/// the byte it addresses remains atomic and must be read at every safepoint.
fn native_interrupt_pointer(builder: &mut FunctionBuilder<'_>, context: ir::Value, fault_poll: bool) -> ir::Value {
    let offset = if fault_poll { std::mem::offset_of!(CallContext, poll_start) as i32 }
        else { CONTEXT_INTERRUPT };
    builder.ins().load(types::I64, invariant(), context, offset)
}

/// The common native region core: builder, call context, interrupt check and its
/// termination block. Frontends keep their own state representation and exit semantics.
struct RegionCore<'b> {
    builder: FunctionBuilder<'b>,
    context: ir::Value,
    /// The VM's poll page when every entry runs through the fault-resuming trampoline.
    poll: Option<ir::Value>,
    /// Where a flag check that finds termination requested goes; the frontend fills it with
    /// the terminated status (`TERMINATED_STATUS`).
    termination_block: Option<ir::Block>,
}

impl<'b> RegionCore<'b> {
    fn new(mut builder: FunctionBuilder<'b>, context: ir::Value, poll: Option<i64>) -> Self {
        let poll = poll.map(|_| native_interrupt_pointer(&mut builder, context, true));
        Self {
            builder,
            context,
            poll,
            termination_block: None,
        }
    }

    fn into_builder(self) -> FunctionBuilder<'b> {
        self.builder
    }

    /// Read the call context's interrupt flag. A JS region uses the result to
    /// take its exact-PC callout; scalar regions use it for termination exits.
    fn interrupt_requested(&mut self) -> ir::Value {
        let context = self.context;
        let pointer = native_interrupt_pointer(&mut self.builder, context, false);
        let requested = self
            .builder
            .ins()
            .atomic_load(types::I8, MemFlagsData::trusted(), pointer);
        self.builder
            .ins()
            .icmp_imm_u(IntCC::NotEqual, requested, 0)
    }

    /// Checks for a termination request: a read of the poll page, which faults once one is
    /// made and resumes as the terminated status (`guard_fault`), else a test of the context's
    /// flag that branches to `termination_block`.
    fn interrupt_safepoint(&mut self) {
        if let Some(poll) = self.poll {
            // The read faults once termination is requested, and the fault resumes as the
            // termination (`guard_fault`). Atomic, so it is neither merged nor dropped.
            self.builder
                .ins()
                .atomic_load(types::I8, MemFlagsData::trusted(), poll);
            return;
        }
        let stop = self.interrupt_requested();
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

impl<'b> Deref for RegionCore<'b> {
    type Target = FunctionBuilder<'b>;

    fn deref(&self) -> &Self::Target {
        &self.builder
    }
}

impl<'b> DerefMut for RegionCore<'b> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.builder
    }
}

/// Scalar Wasm and speculative JS numeric register views layered on the common region core.
///
/// A register has one SSA variable per view type (`VIEW_TYPES`). Writing a value defines its
/// own view and the other view of the same bits (`counterpart`); a view nothing reads costs
/// nothing, since its conversion is dead. A Wasm value is its scalar; a JavaScript Number a
/// region holds unboxed is an `i32` or an `f64` view.
struct Body<'b> {
    core: RegionCore<'b>,
    /// Per register, its views, declared on first use.
    variables: Vec<Option<[Variable; VIEW_TYPES.len()]>>,
    /// Whether i32 views retain an unsigned address offset or a signed scalar.
    int32_extension: Int32Extension,
    /// Each i32 read from a register's view, by the extended value it was read from.
    extended: SecondaryMap<ir::Value, PackedOption<ir::Value>>,
}

/// The interpretation of a register's i32 view and its widening at consumers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Int32Extension {
    /// A Wasm i32 is an unsigned address offset.
    Zero,
    /// A JS Number held as an i32 stays narrow; an element access sign-extends it.
    Sign,
}

impl<'b> Body<'b> {
    fn new(builder: FunctionBuilder<'b>, context: ir::Value, poll: Option<i64>, int32_extension: Int32Extension) -> Self {
        Self {
            core: RegionCore::new(builder, context, poll),
            variables: Vec::new(),
            int32_extension,
            extended: SecondaryMap::new(),
        }
    }

    fn into_builder(self) -> FunctionBuilder<'b> {
        self.core.into_builder()
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
        let extension = self.int32_extension;
        let views = VIEW_TYPES.map(|ty| self.builder.declare_var(view_ir(ty, extension)));
        self.variables[index] = Some(views);
        views
    }

    /// The register's value viewed as `ty`.
    fn read(&mut self, register: u16, ty: crate::WasmType) -> ir::Value {
        let variable = self.views(register)[view(ty)];
        let stored = self.builder.use_var(variable);
        if ty != crate::WasmType::I32 || self.int32_extension == Int32Extension::Sign {
            return stored;
        }
        let value = self.builder.ins().ireduce(types::I32, stored);
        self.extended[value] = stored.into();
        value
    }

    /// `value`, an i32, extended to 64 bits as `extension` does: the i32 view it was read
    /// from, if the view holds it so.
    fn extend(&mut self, value: ir::Value, extension: Int32Extension) -> ir::Value {
        match self.extended[value].expand() {
            Some(wide) if extension == self.int32_extension => wide,
            _ => match extension {
                Int32Extension::Zero => self.builder.ins().uextend(types::I64, value),
                Int32Extension::Sign => self.builder.ins().sextend(types::I64, value),
            },
        }
    }

    /// `value`, an i32, zero-extended to 64 bits.
    fn zero_extend(&mut self, value: ir::Value) -> ir::Value {
        self.extend(value, Int32Extension::Zero)
    }

    /// The stored form of `value` in the register's view as `ty` (`view_ir`).
    fn view_value(&mut self, ty: crate::WasmType, value: ir::Value) -> ir::Value {
        match ty {
            crate::WasmType::I32 if self.int32_extension == Int32Extension::Zero => self.zero_extend(value),
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

    fn interrupt_safepoint(&mut self) {
        self.core.interrupt_safepoint();
    }
}

impl<'b> Deref for Body<'b> {
    type Target = RegionCore<'b>;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl<'b> DerefMut for Body<'b> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
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
    /// The artifact's certificate count less one (`reference_dispatch_entries`), which selects
    /// a table index's certificate.
    reference_dispatch_mask: u64,
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
    /// Per register, its 128-bit SIMD variable (`Slot::Vector`), declared at its first
    /// vector definition; scalar views never hold a vector.
    vectors: Vec<Option<Variable>>,
    /// The words a SIMD kernel callout reads its operands from and writes its result to
    /// (`compiled_simd_kernel`), made at the first callout.
    simd_words: Option<ir::StackSlot>,
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
                self.body.core.builder
                    .ins()
                    .iconst(types::I64, i64::from(bits as u32)),
                self.body.core.builder.ins().f32const(Ieee32::with_bits(bits as u32)),
            ),
            Kind::I64 => (
                self.body.core.builder.ins().iconst(types::I64, bits),
                self.body.core.builder.ins().f64const(Ieee64::with_bits(bits as u64)),
            ),
        };
        let float_type = counterpart(kind.integer());
        let variables = self.body.views(register);
        self.body.core.builder
            .def_var(variables[view(kind.integer())], integer);
        self.body.core.builder.def_var(variables[view(float_type)], float);
    }

    /// The kind of the `b` register of instruction `pc`.
    fn operand_kind(&self, pc: usize) -> Kind {
        match self.plan.operand(pc) {
            Slot::Value(kind) => kind,
            Slot::Memory32Address => Kind::I64,
            _ => Kind::I32,
        }
    }

    fn block(&mut self, pc: usize) -> ir::Block {
        *self.blocks[pc].get_or_insert_with(|| self.body.core.builder.create_block())
    }

    fn trap_block(&mut self, trap: crate::WasmTrap) -> ir::Block {
        let index = (trap_status(trap) - 1) as usize;
        if let Some(block) = self.trap_blocks[index] {
            return block;
        }
        let block = self.body.core.builder.create_block();
        self.body.core.builder.set_cold_block(block);
        self.trap_blocks[index] = Some(block);
        block
    }

    /// Branch to the trap when `condition` is nonzero; continue otherwise.
    fn trap_if(&mut self, condition: ir::Value, trap: crate::WasmTrap) {
        let trap = self.trap_block(trap);
        let next = self.body.core.builder.create_block();
        self.body.core.builder.ins().brif(condition, trap, &[], next, &[]);
        self.body.core.builder.switch_to_block(next);
        self.body.core.builder.seal_block(next);
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
        let scalar = self.body.core.builder.create_block();
        let first_attempt = self.body.core.builder.create_block();
        let attempted = self.body.core.builder.use_var(attempt);
        let already_attempted = self.body.core.builder.ins().icmp_imm_u(IntCC::NotEqual, attempted, 0);
        self.body.core.builder
            .ins()
            .brif(already_attempted, scalar, &[], first_attempt, &[]);

        self.body.core.builder.switch_to_block(first_attempt);
        let one = self.body.core.builder.ins().iconst(types::I32, 1);
        self.body.core.builder.def_var(attempt, one);
        let memory_length = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            self.body.core.context,
            CONTEXT_MEMORY_LENGTH,
        );
        let minimum_length = self
            .body.core.builder
            .ins()
            .iconst(types::I64, candidate.max_end as i64);
        let in_bounds = self.body.core.builder.ins().icmp(
            IntCC::UnsignedGreaterThanOrEqual,
            memory_length,
            minimum_length,
        );
        let vector = self.body.core.builder.create_block();
        self.body.core.builder.ins().brif(in_bounds, vector, &[], scalar, &[]);

        self.body.core.builder.switch_to_block(vector);
        self.affine_pair_vector_loop(candidate);
        self.body.core.builder.switch_to_block(scalar);
    }

    /// Emit the guarded vector body. Its induction range is proven to fit
    /// memory32 without wrap, the input and output ranges are disjoint, and
    /// it polls after each original pair of writes.
    fn affine_pair_vector_loop(&mut self, candidate: AffinePairLoop) {
        let loop_block = self.body.core.builder.create_block();
        let index = self.body.core.builder.append_block_param(loop_block, types::I32);
        let initial = self.body.core.builder.ins().iconst(types::I32, 0);
        self.body.core.builder
            .ins()
            .jump(loop_block, &[ir::BlockArg::Value(initial)]);
        self.body.core.builder.switch_to_block(loop_block);

        let scaled = self.body.core.builder.ins().ishl_imm_u(index, 2);
        let scaled = self.body.core.builder.ins().uextend(types::I64, scaled);
        let address = |this: &mut Self, offset: u32| {
            let address = this.body.core.builder.ins().iadd(this.base, scaled);
            this.body.core.builder.ins().iadd_imm_s(address, i64::from(offset))
        };
        let left_address = address(self, candidate.left);
        let right_address = address(self, candidate.right);
        let destination_address = address(self, candidate.destination);
        let flags = self.flags(Region::LinearMemory);
        let left = self
            .body.core.builder
            .ins()
            .load(types::I32X4, flags, left_address, 0);
        let right = self
            .body.core.builder
            .ins()
            .load(types::I32X4, flags, right_address, 0);
        let sum = self.body.core.builder.ins().iadd(left, right);
        let first_low = self.body.core.builder.ins().extractlane(sum, 0);
        let first_high = self.body.core.builder.ins().extractlane(sum, 1);
        let first_pair = self.body.core.builder.ins().iconcat(first_low, first_high);
        self.body.core.builder
            .ins()
            .store(flags, first_pair, destination_address, 0);
        self.interrupt_safepoint();

        let second_low = self.body.core.builder.ins().extractlane(sum, 2);
        let second_high = self.body.core.builder.ins().extractlane(sum, 3);
        let second_pair = self.body.core.builder.ins().iconcat(second_low, second_high);
        self.body.core.builder
            .ins()
            .store(flags, second_pair, destination_address, 8);
        self.interrupt_safepoint();

        let next_index = self.body.core.builder.ins().iadd_imm_s(index, 4);
        let more =
            self.body.core.builder
                .ins()
                .icmp_imm_u(IntCC::NotEqual, next_index, i64::from(candidate.bound));
        let backedge = self.body.core.builder.create_block();
        let done = self.body.core.builder.create_block();
        self.body.core.builder.ins().brif(more, backedge, &[], done, &[]);
        self.body.core.builder.switch_to_block(backedge);
        self.interrupt_safepoint();
        self.body.core.builder
            .ins()
            .jump(loop_block, &[ir::BlockArg::Value(next_index)]);
        self.body.core.builder.switch_to_block(done);
        let exit = self.edge(candidate.exit);
        self.body.core.builder.ins().jump(exit, &[]);
    }
    fn finish_traps(&mut self) {
        if let Some(block) = self.body.termination_block {
            self.body.core.builder.switch_to_block(block);
            let status = self.body.core.builder.ins().iconst(types::I64, TERMINATED_STATUS);
            let mut values = vec![status];
            if let Some(ty) = self.plan.result {
                let kind = Kind::of(ty).expect("planned");
                values.push(self.body.core.builder.ins().iconst(kind.ir(), 0));
            }
            self.body.core.builder.ins().return_(&values);
        }
        for (index, block) in self.trap_blocks.into_iter().enumerate() {
            let Some(block) = block else { continue };
            self.body.core.builder.switch_to_block(block);
            let status = self.body.core.builder.ins().iconst(types::I64, index as i64 + 1);
            let mut values = vec![status];
            if let Some(ty) = self.plan.result {
                let kind = Kind::of(ty).expect("planned");
                values.push(self.body.core.builder.ins().iconst(kind.ir(), 0));
            }
            self.body.core.builder.ins().return_(&values);
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
        let ty = self.body.core.builder.func.dfg.value_type(left);
        let compare = |this: &mut Self, cc: IntCC| {
            let flag = this.body.core.builder.ins().icmp(cc, left, right);
            this.body.core.builder.ins().uextend(types::I32, flag)
        };
        match operator {
            B::Add => self.body.core.builder.ins().iadd(left, right),
            B::Subtract => self.body.core.builder.ins().isub(left, right),
            B::Multiply => self.body.core.builder.ins().imul(left, right),
            B::And => self.body.core.builder.ins().band(left, right),
            B::Or => self.body.core.builder.ins().bor(left, right),
            B::Xor => self.body.core.builder.ins().bxor(left, right),
            B::ShiftLeft => self.body.core.builder.ins().ishl(left, right),
            B::ShiftRightSigned => self.body.core.builder.ins().sshr(left, right),
            B::ShiftRightUnsigned => self.body.core.builder.ins().ushr(left, right),
            B::RotateLeft => self.body.core.builder.ins().rotl(left, right),
            B::RotateRight => self.body.core.builder.ins().rotr(left, right),
            B::MultiplyHighSigned => self.body.core.builder.ins().smulhi(left, right),
            B::MultiplyHighUnsigned => self.body.core.builder.ins().umulhi(left, right),
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
                let zero = self.body.core.builder.ins().icmp_imm_s(IntCC::Equal, right, 0);
                self.trap_if(zero, crate::WasmTrap::IntegerDivideByZero);
                match operator {
                    B::DivideUnsigned => self.body.core.builder.ins().udiv(left, right),
                    B::RemainderUnsigned => self.body.core.builder.ins().urem(left, right),
                    B::DivideSigned => {
                        let minimum = if ty == types::I32 {
                            i64::from(i32::MIN)
                        } else {
                            i64::MIN
                        };
                        let minimum = self.body.core.builder.ins().icmp_imm_s(IntCC::Equal, left, minimum);
                        let negative_one = self.body.core.builder.ins().icmp_imm_s(IntCC::Equal, right, -1);
                        let overflow = self.body.core.builder.ins().band(minimum, negative_one);
                        self.trap_if(overflow, crate::WasmTrap::IntegerOverflow);
                        self.body.core.builder.ins().sdiv(left, right)
                    }
                    _ => {
                        // `MIN % -1` is 0; the machine division would fault.
                        let negative_one = self.body.core.builder.ins().icmp_imm_s(IntCC::Equal, right, -1);
                        let one = self.body.core.builder.ins().iconst(ty, 1);
                        let divisor = self.body.core.builder.ins().select(negative_one, one, right);
                        self.body.core.builder.ins().srem(left, divisor)
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
        self.body.core.builder.ins().icmp(condition, left, right)
    }

    /// An integer unary operator at the operand's width.
    fn integer_unary(&mut self, operator: I64UnaryOperator, value: ir::Value) -> ir::Value {
        use I64UnaryOperator as U;
        let ty = self.body.core.builder.func.dfg.value_type(value);
        let mut extend = |narrow_type| {
            let narrow = self.body.core.builder.ins().ireduce(narrow_type, value);
            self.body.core.builder.ins().sextend(ty, narrow)
        };
        match operator {
            U::EqualZero => {
                let flag = self.body.core.builder.ins().icmp_imm_s(IntCC::Equal, value, 0);
                self.body.core.builder.ins().uextend(types::I32, flag)
            }
            U::LeadingZeros => self.body.core.builder.ins().clz(value),
            U::TrailingZeros => self.body.core.builder.ins().ctz(value),
            U::PopulationCount => self.body.core.builder.ins().popcnt(value),
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
            self.body.core.builder.ins().f32const(Ieee32::with_bits(bits as u32))
        } else {
            let bits = canonical_nan_bits(f64::MANTISSA_DIGITS, f64::INFINITY.to_bits());
            self.body.core.builder.ins().f64const(Ieee64::with_bits(bits))
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
            let flag = this.body.core.builder.ins().fcmp(cc, left, right);
            this.body.core.builder.ins().uextend(types::I32, flag)
        };
        match operator {
            F::Add => self.body.core.builder.ins().fadd(left, right),
            F::Subtract => self.body.core.builder.ins().fsub(left, right),
            F::Multiply => self.body.core.builder.ins().fmul(left, right),
            F::Divide => self.body.core.builder.ins().fdiv(left, right),
            F::Minimum | F::Maximum => {
                // Signed zeros order as Wasm requires; a NaN operand gives
                // the canonical NaN.
                let extremum = if operator == F::Minimum {
                    self.body.core.builder.ins().fmin(left, right)
                } else {
                    self.body.core.builder.ins().fmax(left, right)
                };
                let unordered = self.body.core.builder.ins().fcmp(FloatCC::Unordered, left, right);
                let ty = self.body.core.builder.func.dfg.value_type(left);
                let nan = self.canonical_nan(ty);
                self.body.core.builder.ins().select(unordered, nan, extremum)
            }
            F::CopySign => self.body.core.builder.ins().fcopysign(left, right),
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
            U::Absolute => self.body.core.builder.ins().fabs(value),
            U::Negate => self.body.core.builder.ins().fneg(value),
            U::Ceiling => self.body.core.builder.ins().ceil(value),
            U::Floor => self.body.core.builder.ins().floor(value),
            U::Truncate => self.body.core.builder.ins().trunc(value),
            U::Nearest => self.body.core.builder.ins().nearest(value),
            U::SquareRoot => self.body.core.builder.ins().sqrt(value),
        }
    }

    /// A scalar conversion of `value`, already viewed as the source type.
    fn convert(&mut self, operator: ScalarConversionOperator, value: ir::Value) -> ir::Value {
        use ScalarConversionOperator as C;
        let result = scalar_ir(operator.result_type());
        let source = operator.source_type();
        match operator {
            C::WrapI64 => self.body.core.builder.ins().ireduce(result, value),
            C::ExtendI32Signed => self.body.core.builder.ins().sextend(result, value),
            C::ExtendI32Unsigned => self.body.core.builder.ins().uextend(result, value),
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
            | C::SaturateF64SignedI64 => self.body.core.builder.ins().fcvt_to_sint_sat(result, value),
            C::SaturateF32UnsignedI32
            | C::SaturateF64UnsignedI32
            | C::SaturateF32UnsignedI64
            | C::SaturateF64UnsignedI64 => self.body.core.builder.ins().fcvt_to_uint_sat(result, value),
            C::ConvertI32SignedF32
            | C::ConvertI64SignedF32
            | C::ConvertI32SignedF64
            | C::ConvertI64SignedF64 => self.body.core.builder.ins().fcvt_from_sint(result, value),
            C::ConvertI32UnsignedF32
            | C::ConvertI64UnsignedF32
            | C::ConvertI32UnsignedF64
            | C::ConvertI64UnsignedF64 => self.body.core.builder.ins().fcvt_from_uint(result, value),
            C::PromoteF32 => self.body.core.builder.ins().fpromote(result, value),
            C::DemoteF64 => self.body.core.builder.ins().fdemote(result, value),
            C::ReinterpretF32I32
            | C::ReinterpretF64I64
            | C::ReinterpretI32F32
            | C::ReinterpretI64F64 => {
                self.body.core.builder
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
            self.body.core.builder.ins().fpromote(types::F64, value)
        } else {
            value
        };
        let nan = self.body.core.builder.ins().fcmp(FloatCC::Unordered, wide, wide);
        self.trap_if(nan, crate::WasmTrap::InvalidConversionToInteger);
        let truncated = self.body.core.builder.ins().trunc(wide);
        let (lower, limit) = crate::wasm::conversion::truncation_bounds(result.bits(), signed);
        let lower = self.body.core.builder.ins().f64const(lower);
        let limit = self.body.core.builder.ins().f64const(limit);
        let below = self.body.core.builder.ins().fcmp(FloatCC::LessThan, truncated, lower);
        let above = self
            .body.core.builder
            .ins()
            .fcmp(FloatCC::GreaterThanOrEqual, truncated, limit);
        let overflow = self.body.core.builder.ins().bor(below, above);
        self.trap_if(overflow, crate::WasmTrap::IntegerOverflow);
        if signed {
            self.body.core.builder.ins().fcvt_to_sint_sat(result, truncated)
        } else {
            self.body.core.builder.ins().fcvt_to_uint_sat(result, truncated)
        }
    }

    /// The host address of a checked `width`-byte access at `address + offset`.
    fn address(&mut self, address: ir::Value, offset: u32, width: usize) -> ir::Value {
        let address = self.zero_extend(address);
        if self.guarded {
            // Index plus offset plus width stays inside the reservation; past
            // the accessible prefix the access faults, and the fault handler
            // raises the out-of-bounds trap (see `guard_fault`).
            let host = self.body.core.builder.ins().iadd(self.base, address);
            return self.body.core.builder.ins().iadd_imm_s(host, i64::from(offset));
        }
        let end = self
            .body.core.builder
            .ins()
            .iadd_imm_s(address, i64::from(offset) + width as i64);
        // A callout may grow the memory while its reserved base stays fixed.
        // Read the refreshed length rather than the entry-time argument.
        let length = if self.analysis.callouts {
            self.body.core.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                self.body.core.context,
                CONTEXT_MEMORY_LENGTH,
            )
        } else {
            self.length.expect("unguarded accesses receive the length")
        };
        let out_of_bounds = self.body.core.builder.ins().icmp(
            IntCC::UnsignedGreaterThan,
            end,
            length,
        );
        self.trap_if(out_of_bounds, crate::WasmTrap::OutOfBoundsMemory);
        let host = self.body.core.builder.ins().iadd(self.base, address);
        self.body.core.builder.ins().iadd_imm_s(host, i64::from(offset))
    }

    /// The address of the value of the global bound at `slot`.
    fn global_address(&mut self, slot: u32) -> ir::Value {
        let position = self
            .globals
            .iter()
            .position(|candidate| *candidate == slot)
            .expect("planned globals are collected");
        let table = self
            .body.core.builder
            .ins()
            .load(types::I64, invariant(), self.body.core.context, CONTEXT_GLOBALS);
        self.body.core.builder
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
        let unwound = self.body.core.builder.create_block();
        self.body.core.builder.set_cold_block(unwound);
        let next = self.body.core.builder.create_block();
        self.body.core.builder.ins().brif(status, unwound, &[], next, &[]);
        self.body.core.builder.switch_to_block(unwound);
        self.body.core.builder.seal_block(unwound);
        let mut values = vec![status];
        if let Some(ty) = self.plan.result {
            values.push(
                self.body.core.builder
                    .ins()
                    .iconst(Kind::of(ty).expect("planned").ir(), 0),
            );
        }
        self.body.core.builder.ins().return_(&values);
        self.body.core.builder.switch_to_block(next);
        self.body.core.builder.seal_block(next);
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
        let fast = self.body.core.builder.create_block();
        let slow = self.body.core.builder.create_block();
        let success = self.body.core.builder.create_block();
        let done = self.body.core.builder.create_block();
        let cache = self.body.core.builder.ins().load(
            types::I64,
            invariant(),
            self.body.core.context,
            CONTEXT_EMSCRIPTEN_MEMCPY_CACHE,
        );
        let valid = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            cache,
            MEMCPY_CACHE_VALID,
        );
        let valid = self.body.core.builder.ins().icmp_imm_u(IntCC::Equal, valid, 1);
        let cached_function = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            cache,
            MEMCPY_CACHE_FUNCTION,
        );
        let matching_function =
            self.body.core.builder
                .ins()
                .icmp_imm_u(IntCC::Equal, cached_function, i64::from(function));
        let valid = self.body.core.builder.ins().band(valid, matching_function);
        self.body.core.builder.ins().brif(valid, fast, &[], slow, &[]);

        self.body.core.builder.switch_to_block(fast);
        let window = instruction.call_window();
        let args = (0..3)
            .map(|offset| self.read(window.base + offset, Kind::I32))
            .collect::<Vec<_>>();
        let target = self.body.core.builder.ins().load(
            types::I64,
            invariant(),
            self.body.core.context,
            CONTEXT_EMSCRIPTEN_MEMCPY_LEAF,
        );
        let mut leaf = Signature::new(self.call_conv);
        leaf.params.push(AbiParam::new(types::I64));
        leaf.params.push(AbiParam::new(types::I64));
        leaf.params
            .extend((0..3).map(|_| AbiParam::new(types::I32)));
        leaf.returns.push(AbiParam::new(types::I64));
        let leaf = self.body.core.builder.import_signature(leaf);
        let function_value = self.body.core.builder.ins().iconst(types::I64, i64::from(function));
        let call = self.body.core.builder.ins().call_indirect(
            leaf,
            target,
            &[self.body.core.context, function_value, args[0], args[1], args[2]],
        );
        let status = self.body.core.builder.inst_results(call)[0];
        let miss = self
            .body.core.builder
            .ins()
            .icmp_imm_u(IntCC::Equal, status, MEMCPY_LEAF_MISS_STATUS);
        self.body.core.builder.ins().brif(miss, slow, &[], success, &[]);

        self.body.core.builder.switch_to_block(success);
        self.propagate(status);
        self.write_constant(instruction.result_register(), Kind::I32, 0);
        self.body.core.builder.ins().jump(done, &[]);

        self.body.core.builder.switch_to_block(slow);
        self.callout_function_generic(
            function,
            &crate::WasmSignature {
                params: vec![crate::WasmType::I32; 3],
                results: Vec::new(),
            },
            instruction,
        );
        self.body.core.builder.ins().jump(done, &[]);
        self.body.core.builder.switch_to_block(done);
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
        let slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            word * words as u32,
            word.trailing_zeros() as u8,
        ));
        let window = instruction.call_window();
        for (offset, ty) in signature.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            let value = self.read(window.base + offset as u16, kind);
            let value = match kind {
                Kind::I32 => self.body.core.builder.ins().uextend(types::I64, value),
                Kind::I64 => value,
            };
            let address =
                self.body.core.builder
                    .ins()
                    .stack_addr(types::I64, slot, (offset as u32 * word) as i32);
            self.body.core.builder
                .ins()
                .store(MemFlagsData::trusted(), value, address, 0);
        }
        let result_offset = (signature.params.len() as u32 * word) as i32;
        let args = self.body.core.builder.ins().stack_addr(types::I64, slot, 0);
        let out = self
            .body.core.builder
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
                let callout = self.body.core.builder.import_signature(callout);
                self.callout_signature = Some(callout);
                callout
            }
        };
        let target =
            self.body.core.builder
                .ins()
                .load(types::I64, invariant(), self.body.core.context, CONTEXT_CALLOUT);
        let function = self.body.core.builder.ins().iconst(types::I64, i64::from(function));
        let call = self.body.core.builder.ins().call_indirect(
            callout_signature,
            target,
            &[self.body.core.context, function, args, out],
        );
        let status = self.body.core.builder.inst_results(call)[0];
        self.propagate(status);
        let destination = instruction.result_register();
        match signature.results.first() {
            Some(ty) => {
                let kind = Kind::of(*ty).expect("planned");
                let bits = self
                    .body.core.builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), out, 0);
                let value = match kind {
                    Kind::I32 => self.body.core.builder.ins().ireduce(types::I32, bits),
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
        let index = self.body.core.builder.ins().uextend(types::I64, index);
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
        let slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            word * words as u32,
            word.trailing_zeros() as u8,
        ));
        let window = instruction.call_window();
        for (offset, ty) in wasm_signature.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            let value = self.read(window.base + offset as u16, kind);
            let value = match kind {
                Kind::I32 => self.body.core.builder.ins().uextend(types::I64, value),
                Kind::I64 => value,
            };
            let address =
                self.body.core.builder
                    .ins()
                    .stack_addr(types::I64, slot, (offset as u32 * word) as i32);
            self.body.core.builder
                .ins()
                .store(MemFlagsData::trusted(), value, address, 0);
        }
        let result_offset = (wasm_signature.params.len() as u32 * word) as i32;
        let args = self.body.core.builder.ins().stack_addr(types::I64, slot, 0);
        let out = self
            .body.core.builder
            .ins()
            .stack_addr(types::I64, slot, result_offset);
        let index = self.read(instruction.b(), Kind::I64);
        let table_slot = self.body.core.builder.ins().iconst(types::I64, i64::from(table));
        let signature_index = self
            .body.core.builder
            .ins()
            .iconst(types::I64, i64::from(signature_index));
        let site_id = self
            .reference_site_ids
            .binary_search_by_key(&pc, |(site_pc, _)| *site_pc)
            .ok()
            .map(|index| self.reference_site_ids[index].1)
            .expect("compiled indirect call has a cache site id");
        let fast_cache_base = self.body.core.builder.ins().load(
            types::I64,
            invariant(),
            self.body.core.context,
            CONTEXT_REFERENCE_FAST_CACHES,
        );
        // The certificate of the index's low bits (`reference_dispatch_entries`).
        let entry = self
            .body.core.builder
            .ins()
            .band_imm_u(index, self.reference_dispatch_mask as i64);
        let entry_offset = self
            .body.core.builder
            .ins()
            .imul_imm_u(entry, std::mem::size_of::<FastReferenceCache>() as i64);
        let fast_cache = self.body.core.builder.ins().iadd(fast_cache_base, entry_offset);
        let cached_epoch = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            fast_cache,
            std::mem::offset_of!(FastReferenceCache, epoch) as i32,
        );
        let cache_initialized = self
            .body.core.builder
            .ins()
            .icmp_imm_u(IntCC::NotEqual, cached_epoch, 0);
        let check_cache = self.body.core.builder.create_block();
        let check_entry = self.body.core.builder.create_block();
        let fast_call = self.body.core.builder.create_block();
        let slow_call = self.body.core.builder.create_block();
        let done = self.body.core.builder.create_block();
        let status_slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            types::I64.bytes(),
            types::I64.bytes().trailing_zeros() as u8,
        ));
        self.body.core.builder
            .ins()
            .brif(cache_initialized, check_cache, &[], slow_call, &[]);

        // Table writes and collection advance the VM-local epoch. This makes
        // artifact-local certificates stale without invalidation walks through
        // recursive native frames.
        self.body.core.builder.switch_to_block(check_cache);
        let epoch_pointer = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            self.body.core.context,
            CONTEXT_REFERENCE_EPOCH,
        );
        let current_epoch =
            self.body.core.builder
                .ins()
                .load(types::I64, MemFlagsData::trusted(), epoch_pointer, 0);
        let epoch_matches = self
            .body.core.builder
            .ins()
            .icmp(IntCC::Equal, current_epoch, cached_epoch);
        let current_env =
            self.body.core.builder
                .ins()
                .load(types::I64, invariant(), self.body.core.context, CONTEXT_ENV);
        let cached_env = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            fast_cache,
            std::mem::offset_of!(FastReferenceCache, env) as i32,
        );
        let env_matches = self
            .body.core.builder
            .ins()
            .icmp(IntCC::Equal, current_env, cached_env);
        let cached_index = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            fast_cache,
            std::mem::offset_of!(FastReferenceCache, index) as i32,
        );
        let index_matches = self.body.core.builder.ins().icmp(IntCC::Equal, index, cached_index);
        let cached_signature = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            fast_cache,
            std::mem::offset_of!(FastReferenceCache, signature) as i32,
        );
        let signature_matches = self
            .body.core.builder
            .ins()
            .icmp(IntCC::Equal, signature_index, cached_signature);
        let cached_table = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            fast_cache,
            std::mem::offset_of!(FastReferenceCache, table) as i32,
        );
        let table_matches = self.body.core.builder.ins().icmp(IntCC::Equal, table_slot, cached_table);
        let signature_matches = self.body.core.builder.ins().band(signature_matches, table_matches);
        let epoch_and_env_match = self.body.core.builder.ins().band(epoch_matches, env_matches);
        let index_and_signature_match = self.body.core.builder.ins().band(index_matches, signature_matches);
        let cache_matches = self
            .body.core.builder
            .ins()
            .band(epoch_and_env_match, index_and_signature_match);
        self.body.core.builder
            .ins()
            .brif(cache_matches, check_entry, &[], slow_call, &[]);
        self.body.core.builder.seal_block(check_cache);

        // A valid cache is populated only from a checked same-artifact target,
        // but keep the index and non-null tests here as a cheap corruption
        // guard before indexing the artifact's entry table.
        self.body.core.builder.switch_to_block(check_entry);
        let cached_function = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            fast_cache,
            std::mem::offset_of!(FastReferenceCache, function) as i32,
        );
        let function_in_range = self.body.core.builder.ins().icmp_imm_u(
            IntCC::UnsignedLessThan,
            cached_function,
            self.plans.len() as i64,
        );
        let body = if self.reference_body_cache {
            let load_body = self.body.core.builder.create_block();
            self.body.core.builder
                .ins()
                .brif(function_in_range, load_body, &[], slow_call, &[]);
            self.body.core.builder.seal_block(check_entry);
            self.body.core.builder.switch_to_block(load_body);
            let body = self.body.core.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                fast_cache,
                std::mem::offset_of!(FastReferenceCache, body) as i32,
            );
            let body_present = self.body.core.builder.ins().icmp_imm_u(IntCC::NotEqual, body, 0);
            self.body.core.builder
                .ins()
                .brif(body_present, fast_call, &[], slow_call, &[]);
            self.body.core.builder.seal_block(load_body);
            body
        } else {
            let lookup_entry = self.body.core.builder.create_block();
            self.body.core.builder
                .ins()
                .brif(function_in_range, lookup_entry, &[], slow_call, &[]);
            self.body.core.builder.seal_block(check_entry);
            self.body.core.builder.switch_to_block(lookup_entry);
            let artifact_table =
                self.body.core.builder
                    .ins()
                    .load(types::I64, invariant(), self.body.core.context, CONTEXT_TABLE);
            let entry_offset = self.body.core.builder.ins().ishl_imm_u(cached_function, 3);
            let entry_address = self.body.core.builder.ins().iadd(artifact_table, entry_offset);
            let entry =
                self.body.core.builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), entry_address, 0);
            let entry_present = self.body.core.builder.ins().icmp_imm_u(IntCC::NotEqual, entry, 0);
            self.body.core.builder
                .ins()
                .brif(entry_present, fast_call, &[], slow_call, &[]);
            self.body.core.builder.seal_block(lookup_entry);
            entry
        };

        self.body.core.builder.switch_to_block(fast_call);
        let mut direct_args = vec![self.body.core.context];
        if self.memory.base {
            direct_args.push(self.base);
        }
        direct_args.extend(self.length);
        for (offset, ty) in wasm_signature.params.iter().enumerate() {
            let raw = self.body.core.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                args,
                (offset as u32 * word) as i32,
            );
            direct_args.push(match Kind::of(*ty).expect("planned") {
                Kind::I32 => self.body.core.builder.ins().ireduce(types::I32, raw),
                Kind::I64 => raw,
            });
        }
        let entry_signature = self.body.core.builder.import_signature(internal_signature(
            self.memory,
            &wasm_signature.params,
            wasm_signature.results.first().copied(),
            self.call_conv,
        ));
        let call = self
            .body.core.builder
            .ins()
            .call_indirect(entry_signature, body, &direct_args);
        let fast_results = self.body.core.builder.inst_results(call).to_vec();
        let fast_status = fast_results[0];
        if let Some(result) = wasm_signature.results.first() {
            let value = match Kind::of(*result).expect("planned") {
                Kind::I32 => self.body.core.builder.ins().uextend(types::I64, fast_results[1]),
                Kind::I64 => fast_results[1],
            };
            self.body.core.builder
                .ins()
                .store(MemFlagsData::trusted(), value, out, 0);
        }
        let status_address = self.body.core.builder.ins().stack_addr(types::I64, status_slot, 0);
        self.body.core.builder
            .ins()
            .store(MemFlagsData::trusted(), fast_status, status_address, 0);
        self.body.core.builder.ins().jump(done, &[]);
        self.body.core.builder.seal_block(fast_call);

        self.body.core.builder.switch_to_block(slow_call);
        self.body.core.builder.set_cold_block(slow_call);
        let target_offset = if self.diagnostics_enabled {
            CONTEXT_CALL_REFERENCE_DIAGNOSTIC
        } else {
            CONTEXT_CALL_REFERENCE
        };
        let target = self
            .body.core.builder
            .ins()
            .load(types::I64, invariant(), self.body.core.context, target_offset);
        let mut call_signature = Signature::new(self.call_conv);
        for _ in 0..8 {
            call_signature.params.push(AbiParam::new(types::I64));
        }
        call_signature.returns.push(AbiParam::new(types::I64));
        let call_signature = self.body.core.builder.import_signature(call_signature);
        let site_id = self.body.core.builder.ins().iconst(types::I64, site_id as i64);
        let call = self.body.core.builder.ins().call_indirect(
            call_signature,
            target,
            &[
                self.body.core.context,
                table_slot,
                index,
                signature_index,
                site_id,
                args,
                out,
                fast_cache,
            ],
        );
        let slow_status = self.body.core.builder.inst_results(call)[0];
        let status_address = self.body.core.builder.ins().stack_addr(types::I64, status_slot, 0);
        self.body.core.builder
            .ins()
            .store(MemFlagsData::trusted(), slow_status, status_address, 0);
        self.body.core.builder.ins().jump(done, &[]);
        self.body.core.builder.seal_block(slow_call);

        self.body.core.builder.switch_to_block(done);
        self.body.core.builder.seal_block(done);
        let status_address = self.body.core.builder.ins().stack_addr(types::I64, status_slot, 0);
        let status =
            self.body.core.builder
                .ins()
                .load(types::I64, MemFlagsData::trusted(), status_address, 0);
        self.propagate(status);
        let destination = instruction.result_register();
        match wasm_signature.results.first() {
            Some(ty) => {
                let kind = Kind::of(*ty).expect("planned");
                let bits = self
                    .body.core.builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), out, 0);
                let value = match kind {
                    Kind::I32 => self.body.core.builder.ins().ireduce(types::I32, bits),
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
            block = self.body.core.builder.create_block();
            self.pending.push((block, thread));
        }
        self.body.core.builder.ins().jump(block, &[]);
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
            self.body.core.builder.ins().brif(condition, taken, &[], next, &[]);
            return;
        }
        let exit = self.body.core.builder.create_block();
        self.body.core.builder.ins().brif(condition, taken, &[], exit, &[]);
        self.body.core.builder.switch_to_block(exit);
        self.body.core.builder.ins().jump(next, &[]);
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
            let block = self.body.core.builder.create_block();
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
            .map(|_| self.body.core.builder.create_block())
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
        self.body.core.builder.switch_to_block(block);
        self.known.clear();
        for pc in unrolled.header..latch {
            let instruction = self.instructions[pc];
            if instruction.op().control_flow_layout() == ControlFlowLayout::ConditionalJump {
                let continuation = self.body.core.builder.create_block();
                self.redirect = Some((pc + 1, continuation));
                self.instruction(function, pc, instruction);
                self.redirect = None;
                self.body.core.builder.switch_to_block(continuation);
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
        self.body.core.builder.switch_to_block(block);
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
                    let continuation = self.body.core.builder.create_block();
                    self.redirect = Some((next, continuation));
                    self.instruction(function, pc, instruction);
                    self.redirect = None;
                    self.body.core.builder.switch_to_block(continuation);
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
        self.body.core.builder.ins().jump(target, &[]);
    }

    /// Emit the instruction at `pc`; returns whether control falls through.
    /// The SIMD variable of `register`.
    fn vector(&mut self, register: u16) -> Variable {
        let index = usize::from(register);
        if self.vectors.len() <= index {
            self.vectors.resize(index + 1, None);
        }
        if let Some(variable) = self.vectors[index] {
            return variable;
        }
        let variable = self.body.core.builder.declare_var(VECTOR_IR);
        self.vectors[index] = Some(variable);
        variable
    }

    fn read_vector(&mut self, register: u16) -> ir::Value {
        let variable = self.vector(register);
        self.body.core.builder.use_var(variable)
    }

    /// Define `register` as the vector `value`, of any lane shape.
    fn write_vector(&mut self, register: u16, value: ir::Value) {
        let value = self.lanes(VECTOR_IR, value);
        let variable = self.vector(register);
        self.known.retain(|(known, _)| *known != register);
        self.body.core.builder.def_var(variable, value);
    }

    /// `value`, a vector, viewed with the lane shape `ty`: the same bits.
    fn lanes(&mut self, ty: ir::Type, value: ir::Value) -> ir::Value {
        if self.body.core.builder.func.dfg.value_type(value) == ty {
            return value;
        }
        let flags = MemFlagsData::new().with_endianness(ir::Endianness::Little);
        self.body.core.builder.ins().bitcast(ty, flags, value)
    }

    /// Address and effect flags for a full-width vector access. A proven memory32
    /// address fits the guard reservation, including the vector's last byte. Keep
    /// this access trapping: even an unused load must fault before later effects.
    fn vector_address(&mut self, pc: usize, register: u16) -> (ir::Value, MemFlagsData) {
        let address = self.read(register, Kind::I64);
        if self.guarded && self.plan.operand(pc) == Slot::Memory32Address {
            let flags = MemFlagsData::new()
                .with_trap_code(Some(ir::TrapCode::HEAP_OUT_OF_BOUNDS))
                .with_endianness(ir::Endianness::Little)
                .with_alias_region(Some(self.regions[Region::LinearMemory as usize]));
            return (self.body.core.builder.ins().iadd(self.base, address), flags);
        }
        let end = self
            .body.core.builder
            .ins()
            .iadd_imm_u(address, crate::wasm::V128_BYTES as i64);
        let memory_length = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            self.body.core.context,
            CONTEXT_MEMORY_LENGTH,
        );
        let out_of_bounds =
            self.body.core.builder
                .ins()
                .icmp(IntCC::UnsignedGreaterThan, end, memory_length);
        self.trap_if(out_of_bounds, crate::WasmTrap::OutOfBoundsMemory);
        let address = self.body.core.builder.ins().iadd(self.base, address);
        (address, self.flags(Region::LinearMemory))
    }

    /// The instructions that define or read vectors (`Slot::Vector`); `None` for any other.
    fn vector_instruction(&mut self, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        match instruction.op() {
            Op::Move if self.plan.operand(pc) == Slot::Vector => {
                let value = self.read_vector(b);
                self.write_vector(a, value);
            }
            Op::LoadConst | Op::WasmFillRegisters => {
                let constant = self.analysis.constant(instruction.constant_index())?;
                let bits = self.analysis.constant_vector(constant)?;
                let registers = if instruction.op() == Op::LoadConst {
                    a..a + 1
                } else {
                    let window = instruction.register_window();
                    window.base..window.base + window.count
                };
                for register in registers {
                    let value = self.vector_constant(bits);
                    self.write_vector(register, value);
                }
            }
            Op::WasmMemoryAddress => {
                let offset = self
                    .analysis
                    .memory_offset(instruction.constant_index())
                    .expect("planned");
                let address = self.read(b, Kind::I32);
                let address = self.body.core.builder.ins().uextend(types::I64, address);
                // A memory32 offset is below 2^32, so the 64-bit sum cannot wrap.
                let address = self.body.core.builder.ins().iadd_imm_u(address, offset as i64);
                self.write(a, Kind::I64, address);
            }
            Op::WasmMemoryLoad => {
                let (address, flags) = self.vector_address(pc, c);
                let value = self.body.core.builder.ins().load(VECTOR_IR, flags, address, 0);
                self.write_vector(a, value);
            }
            Op::WasmMemoryStore => {
                let (address, flags) = self.vector_address(pc, c);
                let value = self.read_vector(a);
                self.body.core.builder.ins().store(flags, value, address, 0);
            }
            Op::WasmSimdShuffle => {
                let lanes = self
                    .analysis
                    .shuffle_lanes(instruction.constant_index())
                    .expect("planned");
                let mask = self
                    .body.core.builder
                    .func
                    .dfg
                    .immediates
                    .push(ir::ConstantData::from(lanes.to_vec()));
                let (left, right) = (self.read_vector(b), self.read_vector(c));
                let value = self.body.core.builder.ins().shuffle(left, right, mask);
                self.write_vector(a, value);
            }
            Op::WasmSimd => self.simd(pc, instruction),
            _ => return None,
        }
        Some(true)
    }

    fn vector_constant(&mut self, bits: u128) -> ir::Value {
        let constant = self
            .body.core.builder
            .func
            .dfg
            .constants
            .insert(ir::ConstantData::from(bits.to_le_bytes().to_vec()));
        self.body.core.builder.ins().vconst(VECTOR_IR, constant)
    }

    /// A SIMD operator: one Cranelift instruction where one has its exact semantics, else
    /// the operator's own kernel (`SimdKernel::apply_bits`), which the interpreter runs too.
    fn simd(&mut self, pc: usize, instruction: WideInstruction) {
        use crate::wasm::simd::SimdOperator as S;
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        let (operator, _) = S::from_selector(instruction.imm()).expect("planned");
        // Lane-wise integer arithmetic, bitwise logic and shifts: Cranelift reduces a vector
        // shift's count modulo the lane width, as Wasm does.
        let lanes = match operator {
            S::V128And | S::V128Or | S::V128Xor | S::V128AndNot | S::V128Not => Some(VECTOR_IR),
            S::I8x16Add | S::I8x16Sub => Some(types::I8X16),
            S::I16x8Add | S::I16x8Sub | S::I16x8Shl | S::I16x8ShrU | S::I16x8ShrS => {
                Some(types::I16X8)
            }
            S::I32x4Add | S::I32x4Sub | S::I32x4Shl | S::I32x4ShrU | S::I32x4ShrS => {
                Some(types::I32X4)
            }
            S::I64x2Add | S::I64x2Sub | S::I64x2Shl | S::I64x2ShrU | S::I64x2ShrS => {
                Some(types::I64X2)
            }
            _ => None,
        };
        if let Some(ty) = lanes {
            let left = self.read_vector(b);
            let left = self.lanes(ty, left);
            let builder = &mut self.body.core.builder;
            let value = match operator {
                S::V128Not => builder.ins().bnot(left),
                S::I16x8Shl | S::I32x4Shl | S::I64x2Shl
                | S::I16x8ShrU | S::I32x4ShrU | S::I64x2ShrU
                | S::I16x8ShrS | S::I32x4ShrS | S::I64x2ShrS => {
                    let count = self.read(c, Kind::I32);
                    let builder = &mut self.body.core.builder;
                    match operator {
                        S::I16x8Shl | S::I32x4Shl | S::I64x2Shl => builder.ins().ishl(left, count),
                        S::I16x8ShrU | S::I32x4ShrU | S::I64x2ShrU => {
                            builder.ins().ushr(left, count)
                        }
                        _ => builder.ins().sshr(left, count),
                    }
                }
                _ => {
                    let right = self.read_vector(c);
                    let right = self.lanes(ty, right);
                    let builder = &mut self.body.core.builder;
                    match operator {
                        S::V128And => builder.ins().band(left, right),
                        S::V128Or => builder.ins().bor(left, right),
                        S::V128Xor => builder.ins().bxor(left, right),
                        S::V128AndNot => builder.ins().band_not(left, right),
                        S::I8x16Add | S::I16x8Add | S::I32x4Add | S::I64x2Add => {
                            builder.ins().iadd(left, right)
                        }
                        _ => builder.ins().isub(left, right),
                    }
                }
            };
            self.write_vector(a, value);
            return;
        }
        if matches!(operator, S::I64x2ExtMulLowI32x4U) {
            let (left, right) = (self.read_vector(b), self.read_vector(c));
            let (left, right) = (self.lanes(types::I32X4, left), self.lanes(types::I32X4, right));
            let builder = &mut self.body.core.builder;
            let left = builder.ins().uwiden_low(left);
            let right = builder.ins().uwiden_low(right);
            let value = builder.ins().imul(left, right);
            self.write_vector(a, value);
            return;
        }
        self.simd_kernel(pc, instruction);
    }

    /// `simd` through the kernel: operands to words, `compiled_simd_kernel`, the result back.
    fn simd_kernel(&mut self, _pc: usize, instruction: WideInstruction) {
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        let (kernel, lane) = crate::wasm::simd::SimdKernel::of(instruction.imm()).expect("planned");
        let word = types::I64.bytes();
        let slot = match self.simd_words {
            Some(slot) => slot,
            None => {
                let slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
                    ir::StackSlotKind::ExplicitSlot,
                    SIMD_KERNEL_WORDS as u32 * word,
                    crate::wasm::V128_BYTES.trailing_zeros() as u8,
                ));
                self.simd_words = Some(slot);
                slot
            }
        };
        let operand = |emitter: &mut Self, register: u16, ty: crate::WasmType, at: usize| {
            let offset = (at as u32 * word) as i32;
            let (low, high) = match ty {
                crate::WasmType::V128 => {
                    let value = emitter.read_vector(register);
                    let value = emitter.lanes(types::I64X2, value);
                    let builder = &mut emitter.body.core.builder;
                    (builder.ins().extractlane(value, 0), builder.ins().extractlane(value, 1))
                }
                ty => {
                    let kind = Kind::of(ty).expect("planned");
                    let value = emitter.read(register, kind);
                    let builder = &mut emitter.body.core.builder;
                    let value = if kind == Kind::I32 {
                        builder.ins().uextend(types::I64, value)
                    } else {
                        value
                    };
                    (value, builder.ins().iconst(types::I64, 0))
                }
            };
            let builder = &mut emitter.body.core.builder;
            builder.ins().stack_store(types::I64, low, slot, offset);
            builder.ins().stack_store(types::I64, high, slot, offset + word as i32);
        };
        operand(self, b, kernel.left, SIMD_KERNEL_LEFT);
        if let Some(right) = kernel.right {
            operand(self, c, right, SIMD_KERNEL_RIGHT);
        }
        let mut signature = Signature::new(self.call_conv);
        signature.params.push(AbiParam::new(types::I64));
        signature.params.push(AbiParam::new(types::I64));
        let signature = self.body.core.builder.import_signature(signature);
        let target = self.body.core.builder.ins().iconst(
            types::I64,
            compiled_simd_kernel as *const () as usize as i64,
        );
        let selector = self
            .body.core.builder
            .ins()
            .iconst(types::I64, i64::from(instruction.imm()));
        let words = self.body.core.builder.ins().stack_addr(types::I64, slot, 0);
        self.body.core.builder
            .ins()
            .call_indirect(signature, target, &[selector, words]);
        let result = (SIMD_KERNEL_RESULT as u32 * word) as i32;
        match kernel.result_type(lane) {
            crate::WasmType::V128 => {
                let value = self.body.core.builder.ins().stack_load(types::I64, VECTOR_IR, slot, result);
                self.write_vector(a, value);
            }
            ty => {
                let kind = Kind::of(ty).expect("planned");
                let value = self.body.core.builder.ins().stack_load(types::I64, types::I64, slot, result);
                let value = if kind == Kind::I32 {
                    self.body.core.builder.ins().ireduce(types::I32, value)
                } else {
                    value
                };
                self.write(a, kind, value);
            }
        }
    }

    fn instruction(
        &mut self,
        function: &Function,
        pc: usize,
        instruction: WideInstruction,
    ) -> bool {
        let op = instruction.op();
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        if let Some(done) = self.vector_instruction(pc, instruction) {
            return done;
        }
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
                .body.core.builder
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
            let right = self.body.core.builder.ins().iconst(types::I32, i64::from(b as i16));
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
                    self.body.core.builder.ins().load(scalar_ir(result), flags, host, 0)
                }
                (1, false) => self.body.core.builder.ins().uload8(ty, flags, host, 0),
                (1, true) => self.body.core.builder.ins().sload8(ty, flags, host, 0),
                (2, false) => self.body.core.builder.ins().uload16(ty, flags, host, 0),
                (2, true) => self.body.core.builder.ins().sload16(ty, flags, host, 0),
                (4, false) if kind == Kind::I64 => self.body.core.builder.ins().uload32(flags, host, 0),
                (4, true) if kind == Kind::I64 => self.body.core.builder.ins().sload32(flags, host, 0),
                _ => self.body.core.builder.ins().load(ty, flags, host, 0),
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
                1 => self.body.core.builder.ins().istore8(flags, value, host, 0),
                2 => self.body.core.builder.ins().istore16(flags, value, host, 0),
                4 if kind == Kind::I64 => self.body.core.builder.ins().istore32(flags, value, host, 0),
                _ => self.body.core.builder.ins().store(flags, value, host, 0),
            };
            return true;
        }
        if op == Op::WasmMemorySize {
            let bytes = self.body.core.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                self.body.core.context,
                CONTEXT_MEMORY_LENGTH,
            );
            let pages = self.body.core.builder.ins().ushr_imm_u(bytes, 16);
            let pages = self.body.core.builder.ins().ireduce(types::I32, pages);
            self.write(a, Kind::I32, pages);
            return true;
        }
        if op == Op::WasmMemoryCopy {
            let (destination, source) = instruction.register_pair();
            let destination = self.read(destination, Kind::I32);
            let destination = self.body.core.builder.ins().uextend(types::I64, destination);
            let source = self.read(source, Kind::I32);
            let source = self.body.core.builder.ins().uextend(types::I64, source);
            let raw_length = self.read(c, Kind::I32);
            let length = self.body.core.builder.ins().uextend(types::I64, raw_length);
            let memory_length = self.body.core.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                self.body.core.context,
                CONTEXT_MEMORY_LENGTH,
            );
            // Both ranges are checked before any byte moves; 32-bit operands cannot overflow
            // their 64-bit sums.
            for start in [destination, source] {
                let end = self.body.core.builder.ins().iadd(start, length);
                let out_of_bounds =
                    self.body.core.builder
                        .ins()
                        .icmp(IntCC::UnsignedGreaterThan, end, memory_length);
                self.trap_if(out_of_bounds, crate::WasmTrap::OutOfBoundsMemory);
            }
            let destination = self.body.core.builder.ins().iadd(self.base, destination);
            let source = self.body.core.builder.ins().iadd(self.base, source);
            let mut signature = Signature::new(self.call_conv);
            for _ in 0..3 {
                signature.params.push(AbiParam::new(types::I64));
            }
            signature.returns.push(AbiParam::new(types::I64));
            let signature = self.body.core.builder.import_signature(signature);
            let target = self.body.core.builder.ins().iconst(
                types::I64,
                compiled_memory_copy as *const () as usize as i64,
            );
            self.body.core.builder
                .ins()
                .call_indirect(signature, target, &[destination, source, length]);
            return true;
        }
        if op == Op::WasmMemoryFill {
            let (destination, _) = instruction.register_pair();
            let destination = self.read(destination, Kind::I32);
            let destination = self.body.core.builder.ins().uextend(types::I64, destination);
            let raw_length = self.read(c, Kind::I32);
            let length = self.body.core.builder.ins().uextend(types::I64, raw_length);
            let end = self.body.core.builder.ins().iadd(destination, length);
            let memory_length = self.body.core.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                self.body.core.context,
                CONTEXT_MEMORY_LENGTH,
            );
            let out_of_bounds =
                self.body.core.builder
                    .ins()
                    .icmp(IntCC::UnsignedGreaterThan, end, memory_length);
            self.trap_if(out_of_bounds, crate::WasmTrap::OutOfBoundsMemory);
            let address = self.body.core.builder.ins().iadd(self.base, destination);
            let raw_byte = self.read(b, Kind::I32);
            let byte = self.body.core.builder.ins().uextend(types::I64, raw_byte);
            let mut signature = Signature::new(self.call_conv);
            for _ in 0..3 {
                signature.params.push(AbiParam::new(types::I64));
            }
            signature.returns.push(AbiParam::new(types::I64));
            let signature = self.body.core.builder.import_signature(signature);
            let target = self.body.core.builder.ins().iconst(
                types::I64,
                compiled_memory_fill as *const () as usize as i64,
            );
            self.body.core.builder
                .ins()
                .call_indirect(signature, target, &[address, byte, length]);
            return true;
        }
        if let Some((operator, ty)) = float_binary_operator(op, instruction.imm()) {
            let left = self.read_typed(b, ty);
            let right = self.read_typed(c, ty);
            let value = self.float_binary(operator, left, right);
            let result = if self.body.core.builder.func.dfg.value_type(value) == types::I32 {
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
                let kind = Kind::of_ir(self.body.core.builder.func.dfg.value_type(value));
                self.write(a, kind, value);
            }
            Op::WasmI64Unary => {
                let operator = I64UnaryOperator::from_tag(instruction.imm()).expect("planned");
                let input = self.read(b, Kind::I64);
                let value = self.integer_unary(operator, input);
                let kind = Kind::of_ir(self.body.core.builder.func.dfg.value_type(value));
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
                let shift = self.body.core.builder.ins().iconst(types::I32, i64::from(c as i16));
                let shifted =
                    self.integer_binary(I64BinaryOperator::ShiftRightUnsigned, input, shift);
                let mask = self
                    .body.core.builder
                    .ins()
                    .iconst(types::I32, i64::from(instruction.imm() as i32));
                let value = self.integer_binary(I64BinaryOperator::And, shifted, mask);
                self.write(a, Kind::I32, value);
            }
            Op::WasmI32AddImmediateJumpNonZero => {
                let input = self.read(b, Kind::I32);
                let addend = self.body.core.builder.ins().iconst(types::I32, i64::from(c as i16));
                let sum = self.body.core.builder.ins().iadd(input, addend);
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
                let default = self.body.core.builder.func.dfg.block_call(default, &[]);
                let cases: Vec<_> = cases
                    .iter()
                    .map(|target| {
                        let block = self.block(*target);
                        self.body.core.builder.func.dfg.block_call(block, &[])
                    })
                    .collect();
                let table = ir::JumpTableData::new(default, &cases);
                let table = self.body.core.builder.create_jump_table(table);
                self.body.core.builder.ins().br_table(selector, table);
                return false;
            }
            Op::WasmSelect => {
                let (condition, _) = instruction.register_pair();
                let kind = self.operand_kind(pc);
                let condition = self.read(condition, Kind::I32);
                let taken = self.read(b, kind);
                let other = self.read(c, kind);
                let value = self.body.core.builder.ins().select(condition, taken, other);
                self.write(a, kind, value);
            }
            Op::CallKnown => {
                let callee = usize::from(instruction.known_function_index());
                let Some(callee_plan) = self.plans[callee].as_ref() else {
                    self.callout_known_fallback(callee as u32, instruction);
                    return true;
                };
                let window = instruction.call_window();
                let mut args = vec![self.body.core.context];
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
                        let signature = self.body.core.builder.import_signature(signature);
                        let name = self
                            .body.core.builder
                            .func
                            .declare_imported_user_function(function_name(callee));
                        let target = self.body.core.builder.import_function(ir::ExtFuncData {
                            name: ir::ExternalName::User(name),
                            signature,
                            colocated: true,
                            patchable: false,
                        });
                        self.callees[callee] = Some(target);
                        target
                    }
                };
                let call = self.body.core.builder.ins().call(target, &args);
                let results = self.body.core.builder.inst_results(call).to_vec();
                self.propagate(results[0]);
                let destination = instruction.result_register();
                match callee_plan.result {
                    Some(ty) => self.write(destination, Kind::of(ty).expect("planned"), results[1]),
                    None => {
                        let zero = self.body.core.builder.ins().iconst(types::I32, 0);
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
                self.body.core.builder.ins().jump(trap, &[]);
                return false;
            }
            Op::Return => {
                let status = self.body.core.builder.ins().iconst(types::I64, 0);
                let mut values = vec![status];
                if let Some(ty) = self.plan.result {
                    values.push(self.read(a, Kind::of(ty).expect("planned")));
                }
                self.body.core.builder.ins().return_(&values);
                return false;
            }
            // A binding is known statically per instruction; the globals it
            // names are reached through the context.
            Op::WasmInstanceBinding => {}
            Op::WasmGlobalGet => {
                let slot = self.binding_slot(pc);
                let address = self.global_address(slot);
                let flags = self.flags(Region::Globals);
                let value = self.body.core.builder.ins().load(types::I32, flags, address, 0);
                self.write(a, Kind::I32, value);
            }
            Op::WasmGlobalSet => {
                let slot = self.binding_slot(pc);
                let address = self.global_address(slot);
                let value = self.read(a, Kind::I32);
                let wide = self.body.core.builder.ins().uextend(types::I64, value);
                let encoded = self.body.core.builder.ins().bor_imm_u(wide, INTEGER_TAG as i64);
                let flags = self.flags(Region::Globals);
                self.body.core.builder.ins().store(flags, encoded, address, 0);
            }
            _ => unreachable!("planned instructions have a denotation"),
        }
        true
    }
}

/// The machine code of a built function and its calls to other compiled functions; `None`
/// when Cranelift refuses it or its code needs a relocation other than such a call. It reads
/// only the function and the target, so any thread may run it.
fn compile_built(
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    func: ir::Function,
) -> Option<(Vec<u8>, Vec<Call>)> {
    super::jit_backend::compile_function(func, isa, None, |compiled, function| {
        let code = compiled.code_buffer().to_vec();
        let names = function.params.user_named_funcs();
        let calls = compiled.buffer
            .relocs()
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
            .collect::<Option<Vec<_>>>()
            .ok_or(NativeCompileRejection::Relocations)?;
        Ok((code, calls))
    })
    .ok()
}

/// Built functions queued for each compile worker: one compiling and one ready keeps every
/// worker busy while the building thread stays ahead, and bounds the Cranelift functions
/// alive at once (each holds its whole body's IR) to a few per worker.
const BUILT_FUNCTIONS_QUEUED_PER_WORKER: usize = 2;

/// The Cranelift IR instructions the compile workers may hold at once, queued and compiling,
/// under each memory policy (`MemoryPolicy`). Every worker holds its function's whole IR and
/// Cranelift's working set grows with it, so the parallel compile of a whole module peaks
/// with the functions in flight; a function heavier than the bound compiles alone. A bound
/// on the count of queued functions did not move the peak (one queued per worker instead
/// of two, memory-first `--jit`, tsf-wasm 43.5-44.5 MB either way).
/// Memory-first bounds them at 50,000, about a third of tsf-wasm's 157k (its largest function
/// 53.6k). JetStream `--jit`, memory-first, unpinned x86_64, 4 cores, median of 6,
/// 2026-10-27: tsf-wasm max RSS 46.6 -> 42.0 MB, Score 23.2 -> 22.1, First 11.2 -> 10.0;
/// sqlite3-wasm 73.6 -> 58.4 MB, Score 16.7 -> 16.2, First 2.96 -> 2.80. Throughput keeps
/// every worker fed (unbounded; the same rows, median of 8, were unchanged).
const WASM_COMPILE_INSTRUCTIONS_IN_FLIGHT: [Option<usize>; crate::MemoryPolicy::ALL.len()] =
    [None, Some(50_000)];

/// A function's compiled body with its calls, and its compiled entry (`build_entry`).
type CompiledBody = (Vec<u8>, Vec<Call>, Vec<u8>);

/// The compile result of the function at an index, with its instruction count, which leaves
/// the in-flight bound when the result arrives.
type CompiledBuilt = (usize, usize, Option<CompiledBody>);

/// A built function's body and entry, compiled (`compile_built`, `compile_entry`).
fn compile_body_and_entry(
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    func: ir::Function,
    entry: ir::Function,
) -> Option<CompiledBody> {
    let (code, calls) = compile_built(isa, func)?;
    Some((code, calls, compile_entry(isa, entry)?))
}

/// Each of `count` functions' compiled body, by index, from the built functions `build`
/// yields in any order: the building stays on this thread, which owns the VM, and the
/// compiling, which needs only the function and the target, runs on one worker per
/// available CPU, holding at most `policy`'s `WASM_COMPILE_INSTRUCTIONS_IN_FLIGHT`. `None`
/// when any function fails to build or compile, as the sequential artifact fails.
fn compile_built_functions(
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    count: usize,
    policy: crate::MemoryPolicy,
    build: impl Iterator<Item = Option<(usize, ir::Function, ir::Function)>>,
) -> Option<Vec<Option<CompiledBody>>> {
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(count);
    let mut bodies: Vec<Option<CompiledBody>> = std::iter::repeat_with(|| None).take(count).collect();
    if workers <= 1 {
        for built in build {
            let (index, func, entry) = built?;
            bodies[index] = Some(compile_body_and_entry(isa, func, entry)?);
        }
        return Some(bodies);
    }
    let bound = WASM_COMPILE_INSTRUCTIONS_IN_FLIGHT[policy as usize];
    let failed = std::sync::atomic::AtomicBool::new(false);
    let (work, queue) =
        std::sync::mpsc::sync_channel::<(usize, ir::Function, ir::Function)>(workers * BUILT_FUNCTIONS_QUEUED_PER_WORKER);
    let queue = std::sync::Mutex::new(queue);
    let (done, results) = std::sync::mpsc::channel::<CompiledBuilt>();
    let mut place = |(index, _, body): CompiledBuilt| bodies[index] = body;
    let built_all = std::thread::scope(|scope| {
        for _ in 0..workers {
            let (queue, done, failed) = (&queue, done.clone(), &failed);
            scope.spawn(move || {
                loop {
                    let next = queue.lock().ok().and_then(|queue| queue.recv().ok());
                    let Some((index, func, entry)) = next else { break };
                    let instructions = func.dfg.num_insts();
                    let body = if failed.load(std::sync::atomic::Ordering::Relaxed) {
                        None
                    } else {
                        compile_body_and_entry(isa, func, entry)
                    };
                    if body.is_none() {
                        failed.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    let _ = done.send((index, instructions, body));
                }
            });
        }
        drop(done);
        let mut built_all = true;
        // The instructions and the count of the functions sent and not yet compiled.
        let (mut in_flight, mut pending) = (0, 0);
        for built in build {
            let Some((index, func, entry)) = built else {
                built_all = false;
                break;
            };
            let instructions = func.dfg.num_insts();
            while pending > 0 && bound.is_some_and(|bound| in_flight + instructions > bound) {
                let Ok(result) = results.recv() else { break };
                in_flight -= result.1;
                pending -= 1;
                place(result);
            }
            if failed.load(std::sync::atomic::Ordering::Relaxed) || work.send((index, func, entry)).is_err() {
                break;
            }
            in_flight += instructions;
            pending += 1;
        }
        drop(work);
        built_all
    });
    if !built_all || failed.into_inner() {
        return None;
    }
    results.into_iter().for_each(place);
    Some(bodies)
}

impl<'a, H: Host> Analysis<'a, H> {
    /// The Cranelift function of `index` with `plan`, ready for `compile_built`; `None` when
    /// a construct has no lowering.
    fn build_function(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plan: &Plan,
        plans: &[Option<Plan>],
        guarded: bool,
        fixed_base: Option<i32>,
        globals: &[u32],
        reference_site_ids: &'a [(usize, usize)],
        reference_dispatch_mask: u64,
        diagnostics_enabled: bool,
    ) -> Option<ir::Function> {
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
            body: Body::new(builder, context, poll_address(&self.vm.execution_interrupt), Int32Extension::Zero),
            plan,
            blocks: vec![None; function.code.len()],
            reference_site_ids,
            reference_dispatch_mask,
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
            vectors: Vec::new(),
            simd_words: None,
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
        let stack_pointer = emitter.body.core.builder.ins().get_stack_pointer(types::I64);
        let limit =
            emitter
                .body.core.builder
                .ins()
                .load(types::I64, invariant(), context, CONTEXT_STACK_LIMIT);
        let exhausted = emitter
            .body.core.builder
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
                    emitter.body.core.builder.ins().jump(block, &[]);
                }
                emitter.body.core.builder.switch_to_block(block);
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
                emitter.body.core.builder.ins().jump(next, &[]);
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
        emitter.body.core.builder.seal_all_blocks();
        emitter.body.into_builder().finalize(isa.frontend_config());
        Some(func)
    }

    /// The C entry of `index`: unpack arguments, call through the table and
    /// store the result.
    /// The native entry of function `index` with `plan`: the trampoline a VM call enters,
    /// which loads the arguments and calls the body through the table. Built here and
    /// compiled beside the body (`compile_built_functions`), as it reads nothing but these.
    fn build_entry(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plan: &Plan,
        memory: MemoryArguments,
    ) -> ir::Function {
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
        func
    }
}

/// The compiled code of an entry `build_entry` built, which calls only through the table.
fn compile_entry(isa: &dyn cranelift_codegen::isa::TargetIsa, func: ir::Function) -> Option<Vec<u8>> {
    let mut context = cranelift_codegen::Context::for_function(func);
    let compiled = context.compile(isa, &mut cranelift_codegen::control::ControlPlane::default()).ok()?;
    if !compiled.buffer.relocs().is_empty() {
        return None;
    }
    Some(compiled.code_buffer().to_vec())
}

/// The guard reservation of an unshared memory binding.
/// A memory base an x86-64 addressing mode can carry as its signed 32-bit
/// displacement.
fn displacement_base(base: *mut u8) -> Option<i32> {
    i32::try_from(base as usize).ok()
}

/// The bytes of the unshared memory `binding` names and their guard reservation, if any,
/// read under one borrow: a native entry needs both (`run_compiled_inner`). `None` when the
/// memory is shared or its bytes are borrowed by a suspended host access, which the general
/// path reports exactly.
fn memory_and_guard<H: Host>(
    vm: &Vm<H>,
    binding: Value,
) -> Option<((*mut u8, usize), Option<(*mut u8, usize)>)> {
    let Some(Cell::WasmMemory { bytes, .. }) = vm.heap.get(binding) else {
        return None;
    };
    let crate::wasm::memory::MemoryStorage::Unshared(state) = &**bytes else {
        return None;
    };
    let mut state = state.try_borrow_mut().ok()?;
    let guard = state.guard();
    let view = (state.as_mut_ptr(), state.len());
    (!view.0.is_null()).then_some((view, guard))
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
    let bytes = state.try_borrow().ok()?;
    query(&bytes)
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
    _code: Option<CodeMemory>,
    trampoline: Trampoline,
    #[cfg(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_os = "macos")))]
    landing: usize,
    /// Where a fault on the poll page resumes: the terminated status.
    #[cfg(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_os = "macos")))]
    termination: usize,
}

/// The host ABI can recover owned guard/poll faults through a native entry.
const FAULT_POLL_SUPPORTED: bool = cfg!(any(
    all(target_arch = "x86_64", target_os = "linux"),
    all(target_arch = "aarch64", target_os = "macos"),
));

/// Compose a host's POSIX signal delivery with owned native-fault recovery.
/// Prepare before installing the host observer, then let recovery consume its
/// owned faults before enqueuing ordinary host signal notifications.
#[cfg(unix)]
pub struct NativeFaultHandler;

#[cfg(unix)]
impl NativeFaultHandler {
    pub fn prepare(signal: i32) {
        if matches!(signal, libc::SIGSEGV | libc::SIGBUS) { let _ = guard_support(); }
    }

    /// # Safety
    /// Arguments must be the original POSIX SA_SIGINFO handler arguments on
    /// the interrupted thread. No locks, allocation or guest callbacks occur.
    pub unsafe fn resume(signal: i32, info: *mut libc::siginfo_t, context: *mut libc::c_void) -> bool {
        #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
        if signal == libc::SIGSEGV { return unsafe { resume_guard_fault(info, context) }; }
        #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
        if matches!(signal, libc::SIGSEGV | libc::SIGBUS) { return unsafe { guard_aarch64::resume_fault(info, context) }; }
        let _ = (signal, info, context);
        false
    }
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

/// Publish forwarding before a handler can run; revoke partial installations.
#[cfg(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_os = "macos")))]
fn install_guard_signals(signals: &[(i32, &std::sync::OnceLock<libc::sigaction>)],
    handler: usize, flags: i32) -> Option<()> {
    for (index, (signal, saved)) in signals.iter().enumerate() {
        // SAFETY: sigaction supplies a complete old disposition before we publish
        // ours. Installation owns this one-time process lifecycle transition.
        let installed = unsafe {
            let mut previous: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(*signal, std::ptr::null(), &mut previous) != 0
                || saved.set(previous).is_err() { false }
            else {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = handler;
                action.sa_flags = flags;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(*signal, &action, std::ptr::null_mut()) == 0
            }
        };
        if !installed {
            for (signal, saved) in &signals[..index] {
                // SAFETY: each earlier entry was installed above; restore its owner.
                if unsafe { libc::sigaction(*signal, saved.get().unwrap(), std::ptr::null_mut()) } != 0 {
                    std::process::abort();
                }
            }
            return None;
        }
    }
    Some(())
}

#[cfg(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_os = "macos")))]
unsafe fn forward_guard_fault(signal: i32, info: *mut libc::siginfo_t, context: *mut libc::c_void,
    previous: &libc::sigaction) {
    // SAFETY: forwarding the original signal ABI to its published disposition.
    unsafe {
        match previous.sa_sigaction {
            libc::SIG_IGN => {},
            libc::SIG_DFL => {
                libc::sigaction(signal, previous, std::ptr::null_mut());
                libc::raise(signal);
            }
            handler if previous.sa_flags & libc::SA_SIGINFO != 0 => {
                std::mem::transmute::<usize, extern "C" fn(i32, *mut libc::siginfo_t, *mut libc::c_void)>(handler)(signal, info, context);
            }
            handler => { std::mem::transmute::<usize, extern "C" fn(i32)>(handler)(signal); }
        }
    }
}

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
            install_guard_signals(&[(libc::SIGSEGV, &PREVIOUS_SEGV)],
                guard_fault as *const () as usize, libc::SA_SIGINFO | libc::SA_ONSTACK | libc::SA_NODEFER)?;
            Some(GuardSupport {
                _code: Some(memory),
                trampoline,
                landing,
                termination,
            })
        })
        .as_ref()
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
fn guard_support() -> Option<&'static GuardSupport> { guard_aarch64::support() }

#[cfg(not(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_os = "macos"))))]
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
    interrupt: &crate::ExecutionInterrupt,
    poll_page: Option<(usize, usize)>,
    mode: NativeEntryMode,
) -> (u32, u64) {
    if mode == NativeEntryMode::Leaf {
        return run_leaf(std::ptr::null_mut(), arguments.as_ptr().cast::<u64>(), entry);
    }
    let flag = poll_page.map_or_else(
        || std::ptr::from_ref(interrupt.atomic_flag()),
        |(start, _)| start as *const std::sync::atomic::AtomicBool,
    );
    let mut context = CallContext::native_adapter(std::ptr::null_mut(), flag, poll_page);
    context.interrupt_owner = interrupt;
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
    // SAFETY: supplied by the signal ABI.
    if unsafe { resume_guard_fault(info, context) } { return; }
    let Some(previous) = PREVIOUS_SEGV.get() else { std::process::abort(); };
    unsafe { forward_guard_fault(signal, info, context, previous); }
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
unsafe fn resume_guard_fault(info: *mut libc::siginfo_t, context: *mut libc::c_void) -> bool {
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
                return true;
            }
        }
    }
    false
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

    /// Compile each Wasm module whole at its first entry (`Runtime::compile_wasm_eagerly`).
    pub(crate) fn compile_wasm_eagerly(&mut self) {
        self.programs.compile_wasm_modules_at_first_entry();
    }

    pub(crate) fn native_execution_rejections(&self) -> u64 {
        self.native_execution_rejections
    }

    pub(crate) fn record_native_execution_rejection(&mut self) {
        self.native_execution_rejections = self.native_execution_rejections.saturating_add(1);
    }

    pub(crate) fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// A compiled tier's poll page is usable only when the shared entry can
    /// resume the protected-page fault back into the interpreter.
    pub(super) fn compiled_poll_page(&self) -> Option<(usize, usize)> {
        native_poll_page(&self.execution_interrupt)
    }

    /// Whether the JavaScript function tiers run: compiled execution under a memory policy
    /// that compiles them (`JS_FUNCTION_TIERS`), in a build whose profiles do not count
    /// interpreted work exactly.
    pub(super) fn js_tier_runs(&self) -> bool {
        !cfg!(quench_profiled)
            && self.execution_mode == ExecutionMode::Compiled
            && js_function_tiers_compile(self.memory_policy())
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
        let Some(facts) = self.programs.js_entry_facts(id, function) else {
            return false;
        };
        let state = self.programs.js_tier(id, function);
        if matches!(state, Some(super::program_store::JsTierState::Unsupported)) {
            return self.programs.js_jit_diagnostics_enabled();
        }
        // A loop-free candidate shorter than a native entry's overhead
        // (`JS_TIER_LANE_ENTRY_RECORDS`) runs cheaper as the frameless activation itself.
        let pays_in_lane = |body: &crate::bytecode::Function| {
            body.code.len() >= JS_TIER_LANE_ENTRY_RECORDS || body.loops()
        };
        let candidate = facts.candidate && program.function_get(function as usize).is_some_and(pays_in_lane);
        // Keep bounded candidates on a frame entry until the shared tier has
        // settled. Lane target caches otherwise preserve a frameless route
        // across the later promotion and no longer consult tiering.
        // Inlining may erase a short body's calls; a call-free candidate still has
        // to pay for native entry under the same cost policy.
        candidate || self.js_general_tier_takes(program, function, &facts)
            || (self.programs.js_jit_lane_methods_enabled() && (!facts.candidate || facts.has_calls))
    }

    /// Whether the general baseline may take `function`: a general candidate the tier admits
    /// (`ProgramStore::general_tier_admits`). Every general-tier path on entry asks this
    /// first, so a function it may not take costs those paths nothing.
    fn js_general_tier_takes(
        &self,
        program: &ResidualProgram,
        function: u32,
        facts: &super::js_compiled::JsEntryFacts,
    ) -> bool {
        facts.general
            && program.function_get(function as usize).is_some_and(|body| {
                self.programs
                    .general_tier_admits(body.code.len(), facts.general_acyclic, self.memory_policy())
            })
    }

    /// Whether the general tier holds `function`: pending, due or compiled.
    pub(super) fn js_tier_holds_for_general(&self, id: ProgramId, function: u32) -> bool {
        match self.programs.js_tier(id, function) {
            Some(
                super::program_store::JsTierState::GeneralPromotionPending { .. }
                | super::program_store::JsTierState::GeneralCompileDue,
            ) => true,
            Some(super::program_store::JsTierState::Compiled(compiled)) => compiled.is_general_dispatch(),
            _ => false,
        }
    }

    /// Whether a lane-entered frame must hand control to the general frame
    /// loop so a pending or installed generic native region can run there.
    pub(super) fn js_tier_needs_general_entry(&self, id: ProgramId, function: u32) -> bool {
        match self.programs.js_tier(id, function) {
            Some(super::program_store::JsTierState::GeneralCompileDue) => true,
            Some(super::program_store::JsTierState::Compiled(compiled)) => {
                compiled.is_general_dispatch()
            }
            _ => false,
        }
    }

    /// Notes an entry of `function` with the general tier where the tier observes it: a
    /// general candidate, and for a legacy candidate only once its numeric profile has
    /// declined. Returns whether it is a general candidate and whether the general baseline
    /// is due to run or compile at this entry (`ProgramStore::note_js_general_dispatch_entry`).
    fn js_tier_note_general_entry(
        &mut self,
        program: &ResidualProgram,
        program_id: ProgramId,
        function: u32,
        facts: &super::js_compiled::JsEntryFacts,
    ) -> (bool, bool) {
        let Some(body) = program
            .function_get(function as usize)
            .filter(|_| self.js_general_tier_takes(program, function, facts))
        else {
            return (false, false);
        };
        let state = self.programs.js_tier(program_id, function);
        let general_state = matches!(
            state,
            Some(
                super::program_store::JsTierState::GeneralPromotionPending { .. }
                    | super::program_store::JsTierState::GeneralCompileDue
                    | super::program_store::JsTierState::Rejected { .. }
            )
        );
        let general_candidate = facts.general
            && (!facts.candidate
                || facts.general_acyclic
                || general_state
                || matches!(state, Some(super::program_store::JsTierState::Compiled(compiled)) if compiled.is_general_dispatch()));
        if !general_candidate {
            return (false, false);
        }
        // A legacy candidate is observed only once its numeric profile has declined.
        let observes = !facts.candidate || general_state;
        // The tier manager prices the baseline against the entries so far.
        let due = observes && self.programs.note_js_general_dispatch_entry(program_id, function, body.code.len(), self.memory_policy());
        (true, due)
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
            facts: self.programs.js_entry_facts(id, function)?,
        };
        let mut specialized_guard_miss = false;
        let entered = self.js_jit_at_entry(program, &entry, &mut specialized_guard_miss);
        if entered.is_none() {
            if specialized_guard_miss {
                let instructions = program
                    .function_get(function as usize)
                    .map_or(0, |body| body.code.len());
                self.programs.note_js_specialized_guard_miss(
                    id,
                    function,
                    instructions,
                );
            } else {
                self.js_tier_note_general_entry(program, id, function, &entry.facts);
            }
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

    /// The callee's depth-0 capture under the same environment binding as the lane.
    pub(super) fn environment_capture(&mut self, env: Value, slot: u16) -> Option<Value> {
        self.tier_capture(TierCaptures::Environment(env), 0, slot)
    }

    /// Read guarded numeric captures without running guest code.
    fn append_numeric_tier_captures<I>(
        &mut self,
        captures: TierCaptures,
        inputs: I,
        arguments: &mut smallvec::SmallVec<[f64; 8]>,
    ) -> Option<()>
    where
        I: IntoIterator<Item = (u16, u16)>,
    {
        for (depth, slot) in inputs {
            arguments.push(self.tier_capture(captures, depth, slot)?.as_number()?);
        }
        Some(())
    }

    /// Append only cached data-property Numbers; a miss leaves guest effects to the VM.
    fn append_numeric_tier_fields<I>(
        &mut self,
        program: &ResidualProgram,
        this: Option<Value>,
        parameters: &[Value],
        captures: TierCaptures,
        inputs: I,
        arguments: &mut smallvec::SmallVec<[f64; 8]>,
    ) -> Option<()>
    where
        I: IntoIterator<Item = (u16, super::js_compiled::JsJitFieldReceiver, u16)>,
    {
        let field_base = arguments.len();
        for (index, (cache_site, receiver, source)) in inputs.into_iter().enumerate() {
            let source = usize::from(source);
            if source < index {
                arguments.push(*arguments.get(field_base + source)?);
                continue;
            }
            if source != index { return None; }
            let receiver = match receiver {
                super::js_compiled::JsJitFieldReceiver::This => this?,
                super::js_compiled::JsJitFieldReceiver::Parameter(parameter) => {
                    *parameters.get(usize::from(parameter))?
                }
                super::js_compiled::JsJitFieldReceiver::Capture { capture, .. } => {
                    self.tier_capture(captures, capture.depth, capture.slot)?
                }
            };
            let super::object::FieldCacheProbe::Hit(value) =
                self.cached_field_hit(program, receiver, cache_site) else { return None; };
            arguments.push(value.as_number()?);
        }
        Some(())
    }

    /// Run a guarded linked operation without allocating a callee frame.
    /// A miss precedes guest effects and leaves the call to its ordinary path.
    #[cfg(feature = "native-execution")]
    fn native_guarded_call(
        &mut self,
        program: &ResidualProgram,
        program_id: ProgramId,
        frame: usize,
        instruction: WideInstruction,
    ) -> Option<Value> {
        if !guarded_call_instruction(instruction)
            || frame + 1 != self.frames.len()
            || self.frames[frame].function == super::ROOT_FUNCTION_ID
        {
            return None;
        }
        let callee = *self.frames[frame].registers.get(usize::from(instruction.register_b()))?;
        let receiver = *self.frames[frame].registers.get(usize::from(instruction.register_c()))?;
        self.guarded_call_values(program, program_id, callee, receiver)
    }

    /// One no-frame, no-GC call operation for linked AOT and JIT regions.
    /// A miss runs no guest operation; a hit preserves every storage layout.
    fn guarded_call_values(&mut self, program: &ResidualProgram, program_id: ProgramId,
        callee: Value, receiver: Value) -> Option<Value> {
        if self.direct_eval || self.parameter_eval { return None; }
        receiver.direct_object_index()?;
        let (target_program, function, env) = self.in_loop_call_target(program, callee)?;
        if target_program != program_id { return None; }
        #[cfg(feature = "native-execution")]
        let plan = self.programs.native_js_code(program_id).and_then(|code| code.guarded_calls())
            .and_then(|calls| calls.get(function));
        #[cfg(feature = "native-execution")]
        if let Some(super::native_program::NativeGuardedCall::Boolean(region)) = plan {
            use super::native_program::NativeBooleanRegion;
            use super::js_compiled::JsJitFieldReceiver;
            let _guard = self.enter_guarded_call()?;
            let captures = region.capture_slots[..usize::from(region.capture_count)]
                .iter().map(|slot| (0, *slot));
            let fields = region.field_sites[..usize::from(region.field_count)].iter()
                .zip(&region.field_sources[..usize::from(region.field_count)])
                .map(|(site, source)| (*site, JsJitFieldReceiver::This, *source));
            let scope = TierCaptures::Environment(env);
            let mut arguments: smallvec::SmallVec<[f64; 8]> = smallvec::SmallVec::new();
            self.append_numeric_tier_captures(scope, captures, &mut arguments)?;
            self.append_numeric_tier_fields(program, Some(receiver), &[], scope, fields, &mut arguments)?;
            if region.flags & NativeBooleanRegion::SIGNED_INT32_INPUTS != 0
                && !arguments.iter().all(|value| super::js_compiled::is_i32_number(*value))
            {
                return None;
            }
            // SAFETY: the image binds this entry to these validated input descriptors;
            // the pure region receives only scalar Numbers and performs no callout.
            let encoded = unsafe { (region.entry)(arguments.as_ptr()) };
            if self.execution_interrupt.atomic_flag().load(std::sync::atomic::Ordering::Relaxed) {
                return None;
            }
            return Some(if encoded & super::js_compiled::BOOLEAN_REGION_RESULT_MASK != 0 {
                Value::TRUE
            } else {
                Value::FALSE
            })
        }
        #[cfg(feature = "native-execution")]
        let store = match plan {
            Some(super::native_program::NativeGuardedCall::CapturedStore(store)) =>
                Some((store.capture_slot, store.atom, store.cache_site, store.update())),
            _ => None,
        };
        #[cfg(not(feature = "native-execution"))]
        let store: Option<(u16, Atom, u16, Option<super::operations::NumberBitwise>)> = None;
        let (slot, atom, site, update) = store.or_else(|| {
            // Reuse the JIT's authoritative static entry facts; the execution-only
            // image consumes its bound plans and retains no JIT analysis.
            if !crate::DYNAMIC_EXECUTION_ENABLED { return None; }
            let facts = self.programs.js_entry_facts(program_id, function)?;
            let store = facts.captured_store?;
            Some((store.capture_slot, store.atom, store.cache_site, store.update))
        })?;
        let _guard = self.enter_guarded_call()?;
        self.captured_number_field_store(program, receiver, TierCaptures::Environment(env), slot, atom, site, update)?;
        Some(Value::UNDEFINED)
    }

    /// Entry obligations for a selected no-frame callee, before its first effect.
    #[inline(always)]
    fn enter_guarded_call(&self) -> Option<crate::stack::GuestCallGuard> {
        let guard = crate::stack::GuestCallGuard::enter().ok()?;
        (!self.execution_interrupt.atomic_flag().load(std::sync::atomic::Ordering::Relaxed)).then_some(guard)
    }

    /// The store proof shared by tier entries and statically linked call entries.
    /// A miss precedes the write; Numbers need no collector reference barrier.
    #[allow(clippy::too_many_arguments)]
    fn captured_number_field_store(
        &mut self,
        program: &ResidualProgram,
        receiver: Value,
        captures: TierCaptures,
        slot: u16,
        atom: Atom,
        site: u16,
        update: Option<super::operations::NumberBitwise>,
    ) -> Option<()> {
        let value = self.tier_capture(captures, 0, slot)?;
        let update = match update {
            Some(operation) => Some((operation, value.as_int()?)),
            None => { value.as_number()?; None },
        };
        // SAFETY: receiver and capture remain rooted by the active call. This
        // proof may log the owner but cannot collect or invoke guest code.
        let pointer = unsafe { self.cached_field_store_pointer(program, receiver, atom, site) }?;
        let value = if let Some((operation, right)) = update {
            // The writable own-data proof identifies the same named slot the
            // source Get reads. No getter, proxy or second cache is involved.
            let left = unsafe { pointer.read() }.as_int()?;
            super::operations::number_bitwise_result(operation, left, right)
        } else { value };
        // SAFETY: the exact writable own-data slot was proved above. Only a
        // non-reference Number is written, before any allocation or guest call.
        unsafe { pointer.write(value); }
        Some(())
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
        let function = self.frames[frame].function;
        let program_id = self.frames[frame].program;
        // Most entries are of functions no tier path can take.
        let Some(facts) = self
            .programs
            .js_entry_facts(program_id, function)
            .filter(|facts| facts.enters || self.programs.js_jit_diagnostics_enabled() || self.js_general_tier_takes(program, function, facts))
        else {
            return JsTierFrameEntry::Interpreter;
        };
        let general = self.js_general_tier_takes(program, function, &facts);
        if allow_inline_calls
            && general
            && let Some(outcome) = self.js_jit_general_dispatch_at_entry(
                program, frame, 0, true, false,
            )
        {
            return match outcome {
                JsNativeDispatchResult::Step(step) => JsTierFrameEntry::NativeStep(step),
                JsNativeDispatchResult::Resume(pc) => JsTierFrameEntry::NativeResume(pc),
            };
        }
        let entry = self.frames.get(frame).filter(|record| record.pc == 0).and_then(|record| {
            let params = program.function_get(record.function as usize)?.params;
            Some(TierEntry {
                program: record.program,
                function: record.function,
                this: record.fixed_this.then_some(record.this),
                parameters: record.registers.iter().copied().take(usize::from(params)).collect(),
                captures: TierCaptures::Frame(frame),
                allocates: true,
                facts: Rc::clone(&facts),
            })
        });
        let (general_candidate, mut promote_general) =
            self.js_tier_note_general_entry(program, program_id, function, &facts);
        let mut specialized_guard_miss = false;
        let compiled_entry = entry.as_ref().and_then(|entry| {
            self.js_jit_at_entry(program, entry, &mut specialized_guard_miss)
        });
        if let Some((return_pc, return_register, value)) = compiled_entry {
            // The frame's own register file: a lane frame call reaches here before
            // `active_registers` names the frame (`lane_enter_script_frame`).
            self.frames[frame].registers[usize::from(return_register)] = value;
            return JsTierFrameEntry::ContinueAt(return_pc);
        }
        if specialized_guard_miss {
            let instructions = program
                .function_get(function as usize)
                .map_or(0, |body| body.code.len());
            promote_general |= self.programs.note_js_specialized_guard_miss(
                program_id,
                function,
                instructions,
            );
        }
        if allow_inline_calls && (general_candidate || promote_general)
            && let Some(outcome) = self.js_jit_general_dispatch_at_entry(
                program, frame, 0, true, promote_general,
            ) {
                return match outcome {
                    JsNativeDispatchResult::Step(step) => JsTierFrameEntry::NativeStep(step),
                    JsNativeDispatchResult::Resume(pc) => JsTierFrameEntry::NativeResume(pc),
                };
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

    pub(super) fn js_jit_general_dispatch_at_entry(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        start_pc: usize,
        allow_inline_calls: bool,
        promote: bool,
    ) -> Option<JsNativeDispatchResult> {
        if !self.js_tier_runs()
            || !self.programs.js_jit_general_dispatch_enabled()
            || program.kind == crate::bytecode::ProgramKind::Wasm
            || start_pc >= program.function_get(self.frames.get(frame)?.function as usize)?.code.len()
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
                if !promote || !js_general_dispatch_candidate(program, function) {
                    return None;
                }
                let compile_started = self
                    .programs
                    .js_jit_diagnostics_enabled()
                    .then(std::time::Instant::now);
                let unresolved_names_are_global = self
                    .programs
                    .unresolved_names_are_global(program_id, function_id);
                let cache_key = self
                    .programs
                    .js_general_artifact_cache_enabled()
                    .then(|| {
                        js_general_dispatch_cache_key(
                            program,
                            function_id,
                            unresolved_names_are_global,
                        )
                    })
                    .flatten();
                let cached = cache_key
                    .as_deref()
                    .and_then(|key| self.programs.cached_js_general_artifact(key));
                let cache_hit = cached.is_some();
                let cache_key_available = cache_key.is_some();
                let (compiled, compile_rejection, code_bytes, artifact_was_created) =
                    if let Some(compiled) = cached {
                        (Some(compiled), None, 0, false)
                    } else {
                        self.host.before_native_compilation();
                        let (artifact, rejection) = match compile_js_general_dispatch(
                            program,
                            function_id,
                            unresolved_names_are_global,
                        ) {
                            Ok(artifact) => (Some(artifact), None),
                            Err(rejection) => (None, Some(rejection)),
                        };
                        self.host.after_native_compilation();
                        let code_bytes = artifact.as_ref().map_or(0, |artifact| artifact.code.len());
                        let artifact_was_created = artifact.is_some();
                        let compiled = artifact
                            .and_then(CompiledJsFunction::from_artifact)
                            .map(Rc::new);
                        (compiled, rejection, code_bytes, artifact_was_created)
                    };
                let rejection = compile_rejection.or_else(|| {
                    (artifact_was_created && compiled.is_none())
                        .then_some("native-code-mapping-failed")
                });
                let installed = self
                    .programs
                    .install_js_tier_with_cache(
                        program_id,
                        function_id,
                        compiled.clone(),
                        cache_key,
                        false,
                    );
                if let Some(compile_started) = compile_started {
                    self.programs.record_js_jit_general_compile(
                        program_id,
                        function_id,
                        u64::try_from(compile_started.elapsed().as_nanos()).unwrap_or(u64::MAX),
                        if installed { code_bytes } else { 0 },
                        installed,
                        if installed { None } else { rejection.or(Some("shared-code-cache-budget-exhausted")) },
                    );
                }
                if cache_key_available {
                    self.programs.record_js_jit_general_artifact_cache(
                        program_id,
                        function_id,
                        cache_hit,
                    );
                }
                if !installed {
                    return None;
                }
                compiled
            }
        }?;

        JsNativeActivation::with(self, program_id, frame, start_pc, allow_inline_calls, |activation, vm| {
            let dispatch = &mut activation.dispatch;
            // Native dispatch starts at the next unexecuted bytecode. Callouts
            // overwrite this with the exact failing PC before invoking VM semantics.
            vm.frames[frame].pc = start_pc;
            vm.frames[frame].pc_is_resume = false;
            let interrupt = vm.execution_interrupt.atomic_flag();
            let interrupt = interrupt as *const std::sync::atomic::AtomicBool;
            let poll_page = vm.compiled_poll_page();
            let outcome = compiled.run_general_dispatch(
                vm as *mut Self,
                program_id,
                (dispatch as *mut JsStepDispatch).cast(),
                interrupt,
                poll_page,
            )?;
            if vm.programs.js_jit_diagnostics_enabled() {
                vm.programs
                    .record_js_jit_native_entry(program_id, function_id);
            }
            Some(outcome)
        }).flatten()
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
        let key_hash = hash_value.as_number()?;
        let array = match self.cached_field_hit(program, receiver, candidate.list_cache) {
            super::object::FieldCacheProbe::Hit(value) => value,
            super::object::FieldCacheProbe::Ineligible => {
                return None;
            }
            super::object::FieldCacheProbe::OrdinaryMiss => {
                return None;
            }
        };
        let mut entry = self.direct_array_index_data(array, index_value)?;
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
            self.heap.object(entry)?;
            let entry_hash = match self.cached_field_hit(program, entry, candidate.hash_cache) {
                super::object::FieldCacheProbe::Hit(value) => value.as_number()?,
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
                entry_key.as_number()?;
                equal_arguments[1] = entry_key;
                let equal =
                    self.js_jit_compiled_call(program, (program_id, equals_function), false, &equal_arguments);
                let equal = equal?;
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
        specialized_guard_miss: &mut bool,
    ) -> Option<(usize, crate::bytecode::Register, Value)> {
        if !self.js_tier_runs() || program.kind == crate::bytecode::ProgramKind::Wasm {
            return None;
        }
        let (program_id, function_id) = (entry.program, entry.function);
        let function = program.function_get(function_id as usize)?;
        let facts = &*entry.facts;
        // Most entries are of functions no entry path can claim; diagnostics still count them.
        #[cfg(test)]
        if !self.programs.js_entry_specializations_enabled() { return None; }
        if !facts.enters && !self.programs.js_jit_diagnostics_enabled() {
            return None;
        }
        if let Some(store) = facts.captured_store
            && let Some(receiver) = entry.this
            && self.captured_number_field_store(program, receiver, entry.captures,
                store.capture_slot, store.atom, store.cache_site, store.update).is_some()
        {
            if self.programs.js_jit_diagnostics_enabled() {
                self.programs.record_js_jit_captured_store_entry(program_id, function_id);
            }
            return Some((function.code.len() - 1, store.return_register, Value::UNDEFINED));
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
            if let Some(receiver) = entry.this
                && let super::object::FieldCacheProbe::Hit(value) =
                    self.cached_field_hit(program, receiver, candidate.cache_site)
                {
                    if self.programs.js_jit_diagnostics_enabled() {
                        self.programs
                            .record_js_jit_field_return_entry(program_id, function_id);
                    }
                    return Some((candidate.return_pc, candidate.return_register, value));
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
        let mut capture_values: smallvec::SmallVec<[f64; 8]> = smallvec::SmallVec::new();
        let captures_numeric = self.append_numeric_tier_captures(
            entry.captures, capture_arguments.iter().map(|capture| (capture.depth, capture.slot)),
            &mut capture_values,
        ).is_some();
        if captures_numeric {
            arguments.extend(capture_values.iter().copied());
        } else {
            numeric = false;
        }
        if self.append_numeric_tier_fields(
            program, entry.this, &registers, entry.captures,
            numeric_field_inputs.iter().map(|input| (input.cache_site, input.receiver, input.source)), &mut arguments,
        ).is_none() {
            numeric = false;
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
            let string = first_argument.and_then(|value| self.heap.string(value))?;
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
                    if !compiled.is_general_dispatch() {
                        *specialized_guard_miss = true;
                    }
                    return None;
                }
                compiled
            }
            Some(
                super::program_store::JsTierState::Unsupported
                | super::program_store::JsTierState::Rejected { .. }
                | super::program_store::JsTierState::GeneralPromotionPending { .. }
                | super::program_store::JsTierState::GeneralCompileDue,
            ) => return None,
            Some(super::program_store::JsTierState::Hot { .. })
            | Some(super::program_store::JsTierState::PromotionPending)
            | None => {
                let general_candidate =
                    self.js_general_tier_takes(program, function_id, facts);
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
                    general_candidate,
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
                #[cfg(feature = "gc-tuning")]
                let peak_before = (std::env::var_os("QUENCH_JS_JIT_PEAK_TRACE").is_some())
                    .then(process_peak_rss_kib);
                let artifact = super::js_compiled::compile(&residual, function_id);
                #[cfg(feature = "gc-tuning")]
                if let Some(before) = peak_before {
                    let after = process_peak_rss_kib();
                    eprintln!(
                        "QUENCH_JS_JIT_PEAK function={function_id} succeeded={} peak_before_kib={before} peak_after_kib={after}",
                        artifact.is_some(),
                    );
                }
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
                        .install_js_tier(
                            program_id,
                            function_id,
                            compiled.clone(),
                            artifact_failed
                                && js_general_dispatch_acyclic_candidate(program, function),
                        );
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
            if !compiled.is_general_dispatch() {
                *specialized_guard_miss = true;
            }
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
                &self.execution_interrupt,
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
                Some((last_pc, terminal.register_a(), value))
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
                    let string = first_argument
                        .and_then(|value| self.heap.string(value))
                        .cloned()?;
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
            if !compiled.direct_callable() || compiled.params() != params {
                return None;
            }
            direct_call(compiled, arguments, interrupt)
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
        let result = direct_call(&compiled, parameters, &self.execution_interrupt)?;
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
        // Each callable function's full plan from the pass that settled the set, which its
        // compilation reads, and the interfaces its callers read.
        let mut full_plans: Vec<Option<Plan>>;
        let mut plans: Vec<Option<Plan>>;
        let mut memory_slot;
        let mut guarded;
        loop {
            callable = allowed.clone();
            loop {
                memory_slot = None;
                full_plans = (0..count)
                    .map(|index| {
                        if callable[index] {
                            analysis.plan(index, &callable, &mut memory_slot)
                        } else {
                            None
                        }
                    })
                    .collect();
                plans = full_plans.iter().map(|plan| plan.as_ref().map(Plan::interface)).collect();
                let next: Vec<bool> = plans.iter().map(Option::is_some).collect();
                if next == callable {
                    break;
                }
                callable = next;
            }
            // Accesses may rely on the guard when the memory has one and
            // this host resumes guard faults.
            let stable_memory = memory_slot.is_some_and(|slot| {
                self.heap
                    .environment_slot(env, slot as usize)
                    .and_then(|binding| guarded_memory(self, binding))
                    .is_some()
            });
            guarded = guard_trampoline().is_some() && stable_memory;
            // Callouts need a stable base, not fault-based bounds checks.
            // Hosts without a fault trampoline retain explicit bounds checks.
            if !analysis.callouts || stable_memory || memory_slot.is_none() {
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
        let isa = super::jit_backend::target_isa(opt_level)?;
        let mut reference_site_ids = vec![Vec::new(); count];
        let mut reference_call_sites = Vec::new();
        let mut reference_cache_sites = Vec::new();
        let mut reference_cache_count = 0usize;
        // The globals compiled code accesses, each once.
        let mut globals: Vec<u32> = Vec::new();
        // One pass over each callable function's full plan finds both what every body's
        // compilation reads first: its indirect reference sites and the globals it accesses.
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            let full_plan = full_plans[index].as_ref()?;
            let function = &code.function(index);
            for (pc, packed) in function.code.iter().enumerate() {
                let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
                match instruction.op() {
                    Op::Call => {
                        if let Slot::IndirectReference { table, .. } = full_plan.operand(pc) {
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
                    Op::WasmGlobalGet | Op::WasmGlobalSet => {
                        if let Some(Slot::Binding(slot)) = full_plan.operands[pc]
                            && !globals.contains(&slot)
                        {
                            globals.push(slot);
                        }
                    }
                    _ => {}
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
        // Largest first: the longest compile starts at once and the rest fill around it, so
        // the slowest function does not finish last on one worker (bodies are placed by index).
        let mut build_order: Vec<usize> = (0..count).filter(|&index| plans[index].is_some()).collect();
        build_order.sort_by_key(|&index| std::cmp::Reverse(code.function(index).code.len()));
        let build = build_order.into_iter().map(|index| {
            let full_plan = full_plans[index].as_ref()?;
            let body_build_started = diagnostics_enabled.then(std::time::Instant::now);
            let body_memory_before = body_build_started.and_then(|_| jit_memory_snapshot());
            let built = analysis.build_function(
                &*isa,
                index,
                full_plan,
                &plans,
                guarded,
                fixed_base,
                &globals,
                &reference_site_ids[index],
                reference_dispatch_entries(reference_cache_count).saturating_sub(1) as u64,
                diagnostics_enabled,
            );
            if let Some(started) = body_build_started {
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
            let entry = analysis.build_entry(&*isa, index, plans[index].as_ref()?, MemoryArguments::of(fixed_base, guarded));
            Some((index, built?, entry))
        });
        let mut bodies = compile_built_functions(&*isa, count, self.memory_policy(), build)?;
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            let (body, function_calls, entry) = bodies[index].take()?;
            let body_projected_len = projected_code_len(bytes.len(), body.len())?;
            if let Some(root) = root
                && body_projected_len > MAX_JIT_ROOT_CODE_BYTES
            {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={root} native-code-bytes={body_projected_len} limit={MAX_JIT_ROOT_CODE_BYTES}",
                    );
                }
                return None;
            }
            let body_len = body.len();
            let offset = place(&mut bytes, body);
            offsets[index] = Some(offset);
            spans.push((offset, body_len, index));
            calls.extend(function_calls.into_iter().map(|call| (offset, call)));
            let entry_projected_len = projected_code_len(bytes.len(), entry.len())?;
            if let Some(root) = root
                && entry_projected_len > MAX_JIT_ROOT_CODE_BYTES
            {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={root} native-code-bytes={entry_projected_len} limit={MAX_JIT_ROOT_CODE_BYTES}",
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
            reference_fast_caches: (0..reference_dispatch_entries(reference_cache_count))
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
        let mut baseline = match self.programs.compiled(program) {
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
                    self.programs.settle_wasm_module_tier(program);
                    self.programs.compiled(program).flatten()
                } else if self
                    .programs
                    .residual(program)
                    .is_some_and(|code| code.kind == crate::bytecode::ProgramKind::Wasm)
                    && self.programs.wasm_module_starts_interpreted(program)
                {
                    // A Wasm module starts in the interpreter and compiles whole once it runs
                    // long (`WasmModuleTier`); a small one compiles whole below, at once.
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
                    self.programs.settle_wasm_module_tier(program);
                    self.programs.compiled(program).flatten()
                }
            }
        };
        // A selective module whose interpreted work came due compiles whole (`WasmModuleTier`).
        if baseline.is_none()
            && !wasm_root_only
            && !wasm_root_first
            && matches!(self.programs.compiled(program), Some(None))
            && self.programs.note_wasm_module_entry(program)
            && self.programs.claim_wasm_module_escalation(program)
        {
            // The module's code is about to run natively, so the interpreter's views of it go
            // first, and the compile reuses their memory, unless a frame of it is on the stack,
            // whose lane run may hold their records, or a lane activation deferred beside this
            // call (`runs_beside_activations`), a frame not on it. An interpreted run derives
            // them again.
            if !self.deferred_activations_run(program) && !self.frames.iter().any(|frame| frame.program == program) {
                self.programs.release_lane_views(program);
            }
            let started = jit_diagnostics_enabled().then(std::time::Instant::now);
            let compiled = self.compile_program(program, env).map(Rc::new);
            if let Some(started) = started {
                let bytecodes: usize = self.programs.residual(program).map_or(0, |code| {
                    (0..code.functions.len())
                        .filter_map(|index| code.function_get(index))
                        .map(|function| function.code.len())
                        .sum()
                });
                eprintln!(
                    "QUENCH_WASM_JIT_STATS {{\"program\":{},\"module_escalated\":true,\"compiled\":{},\"bytecodes\":{},\"compile_ns\":{}}}",
                    program.raw(),
                    compiled.is_some(),
                    bytecodes,
                    started.elapsed().as_nanos(),
                );
            }
            self.programs.set_compiled(program, compiled);
            baseline = self.programs.compiled(program).flatten();
        }
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
            && self.programs.wasm_tier_has_room()
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
                    let (view, guard) = memory_and_guard(self, binding)?;
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
                js_constants: std::ptr::null(),
                js_constant_count: 0,
                interrupt_owner: std::ptr::null(),
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
/// The lane records a loop-free body runs below which a frameless activation is cheaper than
/// the JavaScript tier's native entry of it. A direct native entry (`lane_tier_prologue`,
/// `direct_call`, the shared entry) costs about 380 machine instructions past the
/// activation, and a lane record about 20 (callgrind, typescript-lib under `--jit`,
/// 2026-10-28: 9.7M native entries of 3- to 12-instruction predicates such as
/// `isLineBreak`), so a body shorter than this cannot recoup its entries.
const JS_TIER_LANE_ENTRY_RECORDS: usize = 20;

fn direct_call(
    compiled: &CompiledJsFunction,
    arguments: &[Value],
    interrupt: &crate::ExecutionInterrupt,
) -> Option<CompiledJsCallResult> {
    let int32_identity = compiled.is_int32_identity();
    let mut numeric_arguments: smallvec::SmallVec<[f64; 8]> = smallvec::SmallVec::new();
    for (index, argument) in arguments.iter().take(compiled.params()).enumerate() {
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
    match compiled.call(&numeric_arguments, interrupt, poll_page)? {
        CompiledJsResult::Number(value) => Some(CompiledJsCallResult::Value(value)),
        CompiledJsResult::Int32Identity { value, .. } => Some(CompiledJsCallResult::Value(Value::number(value))),
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
/// artifact, held weakly so that the tier retiring it ends the direct run,
/// and the `Return` its result completes. The artifact owns the input signature.
pub(super) struct DirectTierEntry {
    compiled: std::rc::Weak<CompiledJsFunction>,
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
    /// The function's entry facts, looked up once per entry (`ProgramStore::js_entry_facts`).
    facts: Rc<super::js_compiled::JsEntryFacts>,
}

/// Where an entry's captures are read.
#[derive(Clone, Copy)]
pub(super) enum TierCaptures {
    /// Through the frame's environment and capture cache.
    Frame(usize),
    /// Through the closure's environment.
    Environment(Value),
}

#[cfg(feature = "native-execution")]
impl<H: Host> Vm<H> {
    /// A registered program always yields a completed transition or an error;
    /// a malformed native exit must never select interpreter execution.
    pub(super) fn js_linked_frame_entry(
        &mut self, frame: usize, start_pc: usize, stop_pc: Option<usize>, allow_inline_calls: bool,
    ) -> Option<JsNativeStep> {
        let program = self.frames[frame].program;
        let function = self.frames[frame].function;
        let code = self.programs.native_js_code(program)?.clone();
        if stop_pc == Some(start_pc) {
            return Some(JsNativeStep {
                instruction_pc: start_pc, next_pc: start_pc, result: Ok(StepResult::Continue),
            });
        }
        let failure = |message: String| JsNativeStep {
            instruction_pc: start_pc, next_pc: start_pc, result: Err(JsError::validation(message)),
        };
        JsNativeActivation::with(self, program, frame, start_pc, allow_inline_calls, |activation, vm| {
            // SAFETY: the activation owns the immutable function admitted above.
            let instruction_count = unsafe { (*activation.dispatch.body).code.len() };
            let Some(entry) = code.entries().get(function as usize).copied() else {
                return Some(failure("linked native function is missing".into()));
            };
            let dispatch = &mut activation.dispatch;
            dispatch.stop_pc = stop_pc.unwrap_or(JS_NO_STOP_PC);
            vm.frames[frame].pc = start_pc;
            vm.frames[frame].pc_is_resume = false;
            // SAFETY: NativeJsProgram's constructor binds this function to H and the
            // shared ABI; the opaque and internal context pointers have the same ABI.
            let entry = unsafe { std::mem::transmute::<super::native_program::NativeJsFunctionEntry, Entry>(entry) };
            let interrupt = vm.execution_interrupt.atomic_flag() as *const _;
            let poll_page = vm.compiled_poll_page();
            let exited = run_js_native_entry(entry, vm as *mut Self, program,
                (dispatch as *mut JsStepDispatch).cast(), interrupt, poll_page, &[]);
            match exited {
                Some((NATIVE_JS_STOPPED, _)) => Some(dispatch.outcome.take().unwrap_or_else(||
                    failure("linked native function stopped without a transition".into()))),
                Some((status, word)) if status.checked_sub(JS_GENERAL_RETURN_STATUS_BASE)
                    .and_then(|pc| usize::try_from(pc).ok())
                    .is_some_and(|pc| pc < instruction_count) => {
                    let pc = (status - JS_GENERAL_RETURN_STATUS_BASE) as usize;
                    Some(JsNativeStep {
                        instruction_pc: pc,
                        next_pc: pc + 1,
                        result: Ok(StepResult::Return(Value::from_bits(word))),
                    })
                }
                Some((status, word)) => Some(failure(format!("invalid linked native exit: status {status}, continuation {word}"))),
                None => Some(failure("linked native entry failed".into())),
            }
        }).unwrap_or_else(|| Some(failure("linked native function metadata is missing".into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Op;

    #[cfg(feature = "aot")]
    #[test]
    fn invariant_receiver_headers_dominate_native_loop_backedges() {
        let program = crate::Engine::specialize(
            "function scan(o,n,dead){let sum=0;while(n>0){sum=sum+o.left;n=n-1;}return sum;dead.left;}",
            "invariant-header.js").unwrap();
        let isa = super::super::jit_backend::target_isa_for("speed",
            super::super::jit_backend::CodePublication::RelocatableObject).unwrap();
        let region = lower_static_typed_regions(&program, 1, &[], &*isa, 0).into_iter().next().unwrap();
        let native = &region.function;
        let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
        let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
        let mut headers = Vec::new();
        for block in native.layout.blocks() {
            if !dom.is_reachable(block) { continue; }
            headers.extend(cfg.succ_iter(block).filter(|target| dom.is_reachable(*target)
                && dom.dominates(*target, block, &native.layout)));
        }
        assert!(!headers.is_empty(), "the regression must contain a native backedge");
        let families: Vec<_> = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
            .filter(|instruction| matches!(native.dfg.insts[*instruction],
                ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm }
                    if i64::from(imm) as u64 == crate::value::ValueLayout::DIRECT_OBJECT_TAG)).collect();
        assert_eq!(families.len(), 2, "one this admission and one invariant argument proof");
        for proof in families {
            for header in &headers {
                assert!(dom.dominates(proof, *header, &native.layout),
                    "invariant receiver proof {proof} must precede loop {header}:\n{native}");
            }
        }
    }

    #[cfg(all(target_arch = "aarch64", any(feature = "jit", feature = "aot")))]
    #[test]
    fn aarch64_logical_right_shift_selection_preserves_all_integer_widths() {
        // A64 Logical (shifted register): fixed class bits, N=0; shift kind
        // occupies bits23:22. The test checks actual emitted instructions.
        const LOGICAL_CLASS_MASK: u32 = 0x1f20_0000;
        const LOGICAL_CLASS: u32 = 0x0a00_0000;
        const SHIFT_KIND_OFFSET: u32 = 22;
        const SHIFT_KIND_MASK: u32 = 0b11 << SHIFT_KIND_OFFSET;
        const LSR_KIND: u32 = 0b01 << SHIFT_KIND_OFFSET;
        const ASR_KIND: u32 = 0b10 << SHIFT_KIND_OFFSET;
        const TEST_NAMESPACE: u32 = u32::from_be_bytes(*b"QLSH");
        use crate::vm::operations::NumberBitwise;
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let width = ty.bits();
            let mask = u64::MAX >> (u64::BITS - width);
            for operation in [NumberBitwise::And, NumberBitwise::Or, NumberBitwise::Xor] { for signed in [false, true] { for reversed in [false, true] {
                for count in [0_u64, 1, 17, 31, 32, 63, 64, u64::MAX] {
                    let mut signature = ir::Signature::new(isa.default_call_conv());
                    signature.params.extend([AbiParam::new(types::I64), AbiParam::new(types::I64)]);
                    signature.returns.push(AbiParam::new(types::I64));
                    let mut function = ir::Function::with_name_signature(UserFuncName::user(TEST_NAMESPACE, 0), signature);
                    let mut context = FunctionBuilderContext::new();
                    let mut builder = FunctionBuilder::new(&mut function, &mut context);
                    let block = builder.create_block();
                    builder.append_block_params_for_function_params(block);
                    builder.switch_to_block(block);
                    let args = builder.block_params(block).to_vec();
                    let left = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
                    let right = if ty == types::I64 { args[1] } else { builder.ins().ireduce(ty, args[1]) };
                    let shifted = if signed { builder.ins().sshr_imm_u(right, count as i64) }
                        else { builder.ins().ushr_imm_u(right, count as i64) };
                    let (left, right) = if reversed { (shifted, left) } else { (left, shifted) };
                    let result = match operation {
                        NumberBitwise::And => builder.ins().band(left, right),
                        NumberBitwise::Or => builder.ins().bor(left, right),
                        NumberBitwise::Xor => builder.ins().bxor(left, right),
                        _ => unreachable!("logical operation matrix"),
                    };
                    let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
                    builder.ins().return_(&[result]);
                    builder.seal_all_blocks();
                    builder.finalize(isa.frontend_config());
                    let mut compiler = cranelift_codegen::Context::for_function(function);
                    let compiled = compiler.compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
                    let bytes = compiled.buffer.data();
                    if width >= u32::BITS && count == 17 {
                        let kind = if signed { ASR_KIND } else { LSR_KIND };
                        assert!(bytes.chunks_exact(std::mem::size_of::<u32>()).any(|bytes| {
                            let word = u32::from_le_bytes(bytes.try_into().unwrap());
                            word & LOGICAL_CLASS_MASK == LOGICAL_CLASS && word & SHIFT_KIND_MASK == kind
                        }), "right shift must fold: {ty} operation{operation:?} signed{signed} reversed{reversed}");
                    }
                    let memory = CodeMemory::new(bytes).unwrap();
                    // SAFETY: this verified native function has exactly two
                    // u64 inputs and one u64 result under the host C ABI.
                    let entry = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64) -> u64>(memory.pointer()) };
                    for left in [0, 1, u64::MAX, 1_u64 << (width - 1)] {
                        for right in [0, 1, u64::MAX, 1_u64 << (width - 1)] {
                            let shift = count as u32 & (width - 1);
                            let right = right & mask;
                            let shifted = if signed {
                                let widened = ((right << (u64::BITS - width)) as i64) >> (u64::BITS - width);
                                (widened >> shift) as u64 & mask
                            } else { right >> shift };
                            let expected = match operation { NumberBitwise::And => left & shifted, NumberBitwise::Or => left | shifted,
                                NumberBitwise::Xor => left ^ shifted, _ => unreachable!("logical operation matrix") } & mask;
                            assert_eq!(unsafe { entry(left, right) }, expected,
                                "{ty} operation{operation:?} signed{signed} reversed{reversed} shift{count}");
                        }
                    }
                }
            }}}
        }
    }

    #[cfg(all(feature = "aot", target_arch = "aarch64"))]
    fn assert_aarch64_cfg_edges(code: &cranelift_codegen::CompiledCode) {
        // A64 direct branch encodings; this fixture has no calls, jump tables
        // or literal islands. Compare the metadata with actual branch targets.
        const B_MASK: u32 = 0xfc00_0000;
        const B: u32 = 0x1400_0000;
        const B_COND_MASK: u32 = 0xff00_0010;
        const B_COND: u32 = 0x5400_0000;
        const CB_MASK: u32 = 0x7e00_0000;
        const CB: u32 = 0x3400_0000;
        const RET_MASK: u32 = 0xffff_fc1f;
        const RET: u32 = 0xd65f_0000;
        const RETAA: u32 = 0xd65f_0bff;
        const RETAB: u32 = 0xd65f_0fff;
        const BRANCH_BITS: u32 = 26;
        const CONDITIONAL_BITS: u32 = 19;
        const CONDITIONAL_SHIFT: u32 = 5;
        let bytes = code.buffer.data();
        let mut actual = Vec::new();
        for (index, &start) in code.bb_starts.iter().enumerate() {
            let end = code.bb_starts.get(index + 1).copied().unwrap_or(bytes.len() as u32);
            let mut terminal = false;
            for pc in (start..end).step_by(std::mem::size_of::<u32>()) {
                let word = u32::from_le_bytes(bytes[pc as usize..pc as usize + std::mem::size_of::<u32>()].try_into().unwrap());
                let branch = if word & B_MASK == B { terminal = true; Some((word, BRANCH_BITS)) }
                    else if word & B_COND_MASK == B_COND || word & CB_MASK == CB {
                        Some((word >> CONDITIONAL_SHIFT, CONDITIONAL_BITS))
                    } else { None };
                if let Some((immediate, bits)) = branch {
                    let signed = ((immediate << (u32::BITS - bits)) as i32) >> (u32::BITS - bits);
                    actual.push((start, (i64::from(pc) + i64::from(signed) * std::mem::size_of::<u32>() as i64) as u32));
                }
                terminal |= word & RET_MASK == RET || word == RETAA || word == RETAB;
                if terminal { break; }
            }
            if !terminal && end < bytes.len() as u32 { actual.push((start, end)); }
        }
        actual.sort_unstable(); actual.dedup();
        let mut recorded = code.bb_edges.clone();
        recorded.sort_unstable(); recorded.dedup();
        assert_eq!(recorded, actual, "machine CFG must follow physical blocks: starts{:?}, bytes{:x?}, {:?}",
            code.bb_starts, bytes.chunks_exact(4).map(|word| u32::from_le_bytes(word.try_into().unwrap())).collect::<Vec<_>>(), code.vcode);
    }

    #[cfg(feature = "aot")]
    #[test]
    fn countdown_dependent_loops_keep_semantic_bounds_and_unchecked_entries() {
        let source = r#"
            function coupled(n){n=n|0;let i=0;for(;i<n;i=(i+1)|0){}let j=0;for(;j<i;j=(j+1)|0){}return(i+j)|0;}
            function unchecked(n){n=n|0;let i=0;do{i=(i+1)|0;}while(i<n);return i;}
            function reconverged(n){n=n|0;let i=0;if(!(i<n))globalThis.seen++;do{i=(i+1)|0;}while(i<n);return i;}
            globalThis.seen=0;
            for(const n of [-3,0,1,2,17,257]){
                if(coupled(n)!==(n>0?n*2:0))throw 'dependent bound';
                if(unchecked(n)!==(n>0?n:1))throw 'unchecked entry';
                if(reconverged(n)!==(n>0?n:1))throw 'reconverged entry';
            }
            if(seen!==2)throw 'entry effects';
            throw 42;
        "#;
        let program = crate::Engine::specialize(source, "countdown-dependencies.cjs").unwrap();
        let module = crate::lower_native_js_module(&program, "countdown_dependencies").unwrap();
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        let coupled = module.functions.into_iter().find_map(|function| match function {
            super::super::native_object::NativeFunction::Definition { name, function }
                if name == "countdown_dependencies_function_1" => Some(function), _ => None,
        }).unwrap();
        let mut context = cranelift_codegen::Context::for_function(coupled);
        context.optimize(&*isa, &mut Default::default()).unwrap();
        context.compute_loop_analysis();
        assert!(context.func.layout.blocks().filter(|block| context.loop_analysis.innermost_loop(*block).is_some())
            .flat_map(|block| context.func.layout.block_insts(block))
            .any(|inst| context.func.dfg.insts[inst].opcode() == ir::Opcode::Isub),
            "at least one independent counter must use the countdown path\n{}", context.func);
        let native = linked_optimized_test_program(program);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(42.0)), "{error}");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn conditional_xor_masks_preserve_integer_words_and_bit_positions() {
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let word = u64::MAX >> (u64::BITS - ty.bits());
            let sign = 1_u64 << (ty.bits() - 1);
            for tested in (0..ty.bits()).map(|bit| 1_u64 << bit).chain([0, word]) { for invert in [false, true] {
              for comparison in [None, Some(IntCC::Equal), Some(IntCC::NotEqual)] {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.extend([AbiParam::new(types::I64); 3]);
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::testcase("conditional_xor"), signature);
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                let args = builder.block_params(entry).to_vec();
                let values: Vec<_> = args.into_iter().map(|value| if ty == types::I64 { value }
                    else { builder.ins().ireduce(ty, value) }).collect();
                let condition = builder.ins().band_imm_u(values[0], tested as i64);
                let condition = comparison.map_or(condition, |cc| builder.ins().icmp_imm_u(cc, condition, 0));
                let changed = builder.ins().bxor(values[1], values[2]);
                let result = if invert { builder.ins().select(condition, values[1], changed) }
                    else { builder.ins().select(condition, changed, values[1]) };
                let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
                builder.ins().return_(&[result]);
                builder.seal_all_blocks(); builder.finalize(isa.frontend_config());
                let mut compiler = cranelift_codegen::Context::for_function(function);
                compiler.compile(&*isa, &mut Default::default()).unwrap();
                if tested.is_power_of_two() {
                    assert!(!compiler.func.layout.blocks().flat_map(|block| compiler.func.layout.block_insts(block))
                        .any(|inst| compiler.func.dfg.insts[inst].opcode() == ir::Opcode::Select), "{ty} mask{tested} invert{invert}: {}", compiler.func);
                }
                let memory = CodeMemory::new(compiler.compiled_code().unwrap().buffer.data()).unwrap();
                // SAFETY: verified native ABI accepts and returns machine words; mapping stays live.
                let run: unsafe extern "C" fn(u64, u64, u64) -> u64 = unsafe { std::mem::transmute(memory.pointer()) };
                let sources: Vec<_> = if ty == types::I8 { (0..=word).collect() }
                    else { vec![0, 1, sign, word, tested, word ^ tested] };
                let deltas: Vec<_> = if ty == types::I8 { (0..=word).collect() }
                    else { vec![0, 1, sign, word, tested, word ^ tested] };
                for source in sources { for base in [0, 1, sign, word] { for &delta in &deltas {
                    let expected = base ^ if (source & tested != 0) ^ invert ^ (comparison == Some(IntCC::Equal)) { delta } else { 0 };
                    assert_eq!(unsafe { run(source, base, delta) }, expected, "{ty} mask{tested} invert{invert} source{source} base{base} delta{delta}");
                }}}
            }}}
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn conditional_integer_update_keeps_countdown_after_poll_unswitch() {
        for body in [
            "n|=0;x|=0;for(let i=0;i<n;i=(i+1)|0){if((x&1)===0)x=x>>1;else x=(x>>1)^feedback;}return x|0;",
            "const state={count:n|0,control:x|0};while(state.count>0){state.count=(state.count-1)|0;if((state.control&1)===0)state.control=state.control>>1;else state.control=(state.control>>1)^feedback;}return state.control|0;",
        ] {
        let source = format!(r#"
            const feedback=7;
            function fold(n,x){{{body}}}
            if(fold(0,-1)!==-1||fold(-1,17)!==17||fold(1,-1)!==-8||fold(3,3)!==6)throw 'integer fold';
            throw 42;
        "#);
        let program = crate::Engine::specialize(&source, "conditional-countdown.cjs").unwrap();
        let module = crate::lower_native_js_module(&program, "conditional_countdown").unwrap();
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        let function = module.functions.into_iter().find_map(|function| match function {
            super::super::native_object::NativeFunction::Definition { name, function }
                if name == "conditional_countdown_function_1" => Some(function), _ => None,
        }).unwrap();
        let counters = super::super::jit_backend::compile_function(function, &*isa, None, |_, native| {
            let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
            let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
            let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
            loops.compute(native, &cfg, &dom);
            Ok(loops.loops().filter(|&lp| {
                let hot: Vec<_> = native.layout.blocks().filter(|block| loops.innermost_loop(*block) == Some(lp))
                    .flat_map(|block| native.layout.block_insts(block)).collect();
                hot.iter().any(|&inst| native.dfg.insts[inst].opcode() == ir::Opcode::Isub)
                    && !hot.iter().any(|&inst| (FAULT_POLL_SUPPORTED && native.dfg.insts[inst].opcode() == ir::Opcode::AtomicLoad)
                        || matches!(native.dfg.insts[inst], ir::InstructionData::IntCompare {
                            cond: IntCC::SignedLessThan | IntCC::SignedGreaterThanOrEqual
                                | IntCC::SignedGreaterThan | IntCC::SignedLessThanOrEqual, .. }))
            }).count())
        }).unwrap();
        assert!(counters > 0, "conditional update must retain a countdown without polls after loop splitting");
        let native = linked_optimized_test_program(program);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(42.0)), "{error}");
        }
    }

    #[test]
    fn signed_shift_pairs_preserve_width_masking_and_shared_inputs() {
        use cranelift_codegen::settings::Configurable;
        let mut flags = cranelift_codegen::settings::builder();
        // Exercise instruction selection directly, including widths for which
        // the middle-end can otherwise replace the pair with an IR extension.
        flags.set("opt_level", "none").unwrap();
        flags.set("enable_verifier", "true").unwrap();
        let isa = cranelift_native::builder().unwrap().finish(cranelift_codegen::settings::Flags::new(flags)).unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let width = ty.bits();
            let mask = u64::MAX >> (u64::BITS - width);
            for amount in 0..width { for delta in [0, 1] { for shared in [false, true] { for masked in [false, true] {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.push(AbiParam::new(types::I64));
                signature.params.push(AbiParam::new(types::I64));
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::testcase("signed_shift_pair"), signature);
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                let input = builder.block_params(entry)[0];
                let filter = builder.block_params(entry)[1];
                let filter = if ty == types::I64 { filter } else { builder.ins().ireduce(ty, filter) };
                let input = if ty == types::I64 { input } else { builder.ins().ireduce(ty, input) };
                let left = builder.ins().ishl_imm_u(input, i64::from(amount) - i64::from(width));
                let right = builder.ins().sshr_imm_u(left, i64::from(amount + width + delta));
                let result = if shared { builder.ins().bxor(left, right) } else { right };
                let result = if masked { builder.ins().band(result, filter) } else { result };
                let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
                builder.ins().return_(&[result]);
                builder.seal_all_blocks(); builder.finalize(isa.frontend_config());
                let mut compiler = cranelift_codegen::Context::for_function(function);
                compiler.compile(&*isa, &mut Default::default()).unwrap();
                let code = compiler.compiled_code().unwrap().buffer.data();
                #[cfg(target_arch = "aarch64")]
                if ty == types::I64 && amount != 0 && delta == 0 && !shared {
                    // A64 Bitfield class, ignoring sf/opc and operand fields.
                    const BITFIELD_CLASS_MASK: u32 = 0x1f80_0000;
                    const BITFIELD_CLASS: u32 = 0x1300_0000;
                    const SIGNED_BITFIELD_MASK: u32 = 0x7f80_0000;
                    let bitfields = code.chunks_exact(std::mem::size_of::<u32>())
                        .filter(|word| u32::from_le_bytes((*word).try_into().unwrap()) & BITFIELD_CLASS_MASK == BITFIELD_CLASS).count();
                    assert_eq!(bitfields, 1, "one signed extract replaces both shifts, amount={amount}");
                    assert!(code.chunks_exact(std::mem::size_of::<u32>()).any(|word|
                        u32::from_le_bytes(word.try_into().unwrap()) & SIGNED_BITFIELD_MASK == BITFIELD_CLASS),
                        "logical consumers must retain the signed extract, amount={amount} masked={masked}");
                }
                let memory = CodeMemory::new(code).unwrap();
                // SAFETY: verified two-word argument/one-word result ABI and live code.
                let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64) -> u64>(memory.pointer()) };
                let retained_sign = 1_u64 << (width - amount - 1);
                for value in (0_u64..=255).chain([retained_sign, retained_sign - 1, mask, mask >> 1, !mask, u64::MAX]) { for filter in [0, 1, mask, !mask] {
                    let left = value.wrapping_shl(amount) & mask;
                    let signed = (left << (u64::BITS - width)) as i64 >> (u64::BITS - width);
                    let right = (signed >> ((amount + delta) & (width - 1))) as u64 & mask;
                    let expected = if shared { left ^ right } else { right };
                    let expected = if masked { expected & filter } else { expected };
                    assert_eq!(unsafe { run(value, filter) }, expected, "{ty} shift={amount} delta={delta} shared={shared} masked={masked} value={value}");
                }}
            }}}}
        }
    }

    #[test]
    fn masked_integer_conditions_preserve_width_polarity_and_other_uses() {
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let word_mask = u64::MAX >> (u64::BITS - ty.bits());
            let sign_bit = 1_u64 << (ty.bits() - 1);
            for mask in [0, 1, sign_bit, sign_bit | 1, word_mask, 0x55 & word_mask] {
                for condition in [None, Some(IntCC::Equal), Some(IntCC::NotEqual)] {
                    let mut signature = Signature::new(isa.default_call_conv());
                    signature.params.extend([AbiParam::new(types::I64); 3]);
                    signature.returns.push(AbiParam::new(types::I64));
                    let mut function = ir::Function::with_name_signature(UserFuncName::testcase("masked_condition"), signature);
                    let mut context = FunctionBuilderContext::new();
                    let mut builder = FunctionBuilder::new(&mut function, &mut context);
                    let entry = builder.create_block();
                    builder.append_block_params_for_function_params(entry);
                    builder.switch_to_block(entry);
                    let args = builder.block_params(entry).to_vec();
                    let source = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
                    let masked = builder.ins().band_imm_u(source, mask as i64);
                    let predicate = match condition {
                        None => masked,
                        Some(condition) => builder.ins().icmp_imm_u(condition, masked, 0),
                    };
                    let selected = builder.ins().select(predicate, args[1], args[2]);
                    // Keep a second consumer: folding the predicate must not remove its value.
                    let masked = if ty == types::I64 { masked } else { builder.ins().uextend(types::I64, masked) };
                    let result = builder.ins().bxor(selected, masked);
                    builder.ins().return_(&[result]);
                    builder.seal_all_blocks();
                    builder.finalize(isa.frontend_config());
                    let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
                    let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64, u64) -> u64>(code.pointer()) };
                    for source in (0..=255).chain([sign_bit, sign_bit - 1, word_mask, !word_mask, u64::MAX]) {
                        let masked = source & mask;
                        let choose_first = if condition == Some(IntCC::Equal) { masked == 0 } else { masked != 0 };
                        for (first, second) in [(0, u64::MAX), (sign_bit, 1)] {
                            let expected = (if choose_first { first } else { second }) ^ masked;
                            assert_eq!(unsafe { run(source, first, second) }, expected,
                                "{ty} source={source:x} mask={mask:x} condition={condition:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn rotated_countdown_preserves_empty_loops_and_early_exit_values() {
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
          for observe_counter in [false, true] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 3]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut function = ir::Function::with_name_signature(UserFuncName::testcase("rotated_counter"), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let entry = builder.create_block();
            let header = builder.create_block();
            let body = builder.create_block();
            let advance = builder.create_block();
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(header, ty);
            builder.append_block_param(header, ty);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let mut words = Vec::new();
            for value in args { words.push(if ty == types::I64 { value } else { builder.ins().ireduce(ty, value) }); }
            let zero = builder.ins().iconst(ty, 0);
            builder.ins().jump(header, &[words[0].into(), zero.into()]);
            builder.switch_to_block(header);
            let counter = builder.block_params(header)[0];
            let checksum = builder.block_params(header)[1];
            let more = builder.ins().icmp(IntCC::SignedLessThan, counter, words[1]);
            builder.ins().brif(more, body, &[], exit, &[]);
            builder.switch_to_block(body);
            let stopped = builder.ins().icmp(IntCC::Equal, if observe_counter { counter } else { checksum }, words[2]);
            builder.ins().brif(stopped, exit, &[], advance, &[]);
            builder.switch_to_block(advance);
            let updated = if observe_counter { builder.ins().bxor(checksum, counter) }
                else { builder.ins().iadd_imm_u(checksum, 1) };
            let next = builder.ins().iadd_imm_u(counter, 1);
            builder.ins().jump(header, &[next.into(), updated.into()]);
            builder.switch_to_block(exit);
            let result = builder.ins().iadd(counter, checksum);
            let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            if !observe_counter {
                let converted = super::super::jit_backend::compile_function(function.clone(), &*isa, None, |_, native| {
                    let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
                    let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
                    let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
                    loops.compute(native, &cfg, &dom);
                    Ok(native.layout.blocks().filter(|&block| loops.innermost_loop(block).is_some())
                        .flat_map(|block| native.layout.block_insts(block))
                        .any(|inst| native.dfg.insts[inst].opcode() == ir::Opcode::Isub))
                }).unwrap();
                assert!(converted, "{ty}: rotated loop must use shared countdown lowering");
            }
            let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
            let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64, u64) -> u64>(code.pointer()) };
            let sign_bit = 1_u64 << (ty.bits() - 1);
            let mask = u64::MAX >> (u64::BITS - ty.bits());
            let signed = |value:u64| ((value << (u64::BITS - ty.bits())) as i64) >> (u64::BITS - ty.bits());
            for start in [0, 1, sign_bit - 1, sign_bit, sign_bit + 1, mask] {
                for bound in [0, 1, sign_bit - 1, sign_bit, mask] {
                    for distance in [0, 1, 17, mask.min(257)] {
                        let stop = if observe_counter { start.wrapping_add(distance) & mask } else { distance & mask };
                        let (mut counter, mut checksum) = (start, 0);
                        while signed(counter) < signed(bound) && (if observe_counter { counter } else { checksum }) != stop {
                            checksum = if observe_counter { checksum ^ counter } else { checksum.wrapping_add(1) & mask };
                            counter = counter.wrapping_add(1) & mask;
                        }
                        assert_eq!(unsafe { run(start, bound, stop) }, counter.wrapping_add(checksum) & mask,
                            "{ty}: start={start:x} bound={bound:x} stop={stop:x}");
                    }
                }
            }
          }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn countdown_induction_preserves_words_checkpoints_and_labels() {
        use cranelift_codegen::settings::Configurable;
        use std::sync::atomic::AtomicU8;
        const MAX_EXECUTED_TRIPS: u128 = 257;
        const PREVIOUS_LABEL: u32 = 0;
        const NEXT_LABEL: u32 = 1;
        const PREVIOUS_USE: u32 = 101;
        const NEXT_USE: u32 = 102;
        let mut flags = cranelift_codegen::settings::builder();
        flags.set("opt_level", "speed").unwrap();
        flags.set("enable_verifier", "true").unwrap();
        flags.set("machine_code_cfg_info", "true").unwrap();
        let isa = cranelift_native::builder().unwrap().finish(cranelift_codegen::settings::Flags::new(flags)).unwrap();
        let mut executions = 0;
        let mut optimized = 0;
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let width = ty.bits();
            let mask = u64::MAX >> (u64::BITS - width);
            let min = if width == i64::BITS { i64::MIN } else { -(1_i64 << (width - 1)) };
            let max = if width == i64::BITS { i64::MAX } else { (1_i64 << (width - 1)) - 1 };
            let starts: Vec<_> = if ty == types::I8 { (min..=max).collect() }
                else { vec![min, min + 1, -7, -1, 0, 1, max - 17, max - 1] };
            for step in [1_i64, -1] { for &start in &starts {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.extend([AbiParam::new(types::I64); 3]);
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::testcase("countdown_words"), signature);
                function.collect_debug_info();
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let entry = builder.create_block();
                let header = builder.create_block();
                let comparison = builder.create_block();
                let backedge = builder.create_block();
                let empty = builder.create_block();
                let done = builder.create_block();
                let interrupted = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.append_block_param(header, ty);
                builder.switch_to_block(entry);
                builder.set_srcloc(ir::SourceLoc::new(0));
                let args = builder.block_params(entry).to_vec();
                let bound = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
                let initial = builder.ins().iconst(ty, start);
                let condition = if step > 0 { IntCC::SignedLessThan } else { IntCC::SignedGreaterThan };
                let enters = builder.ins().icmp(condition, initial, bound);
                builder.ins().brif(enters, header, &[initial.into()], empty, &[]);
                builder.switch_to_block(header);
                let previous = builder.block_params(header)[0];
                builder.set_val_label(previous, ir::ValueLabel::from_u32(PREVIOUS_LABEL));
                let next = builder.ins().iadd_imm_u(previous, step);
                builder.set_val_label(next, ir::ValueLabel::from_u32(NEXT_LABEL));
                let requested = builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), args[1]);
                builder.ins().brif(requested, interrupted, &[], comparison, &[]);
                builder.switch_to_block(comparison);
                let continues = builder.ins().icmp(condition, next, bound);
                builder.ins().brif(continues, backedge, &[], done, &[]);
                builder.switch_to_block(backedge);
                builder.ins().jump(header, &[next.into()]);
                let widen = |builder: &mut FunctionBuilder<'_>, value| if ty == types::I64 { value }
                    else { builder.ins().uextend(types::I64, value) };
                for block in [interrupted, done] {
                    builder.switch_to_block(block);
                    builder.set_srcloc(ir::SourceLoc::new(PREVIOUS_USE));
                    let old = widen(&mut builder, previous);
                    builder.ins().store(MemFlagsData::trusted(), old, args[2], 0);
                    builder.set_srcloc(ir::SourceLoc::new(NEXT_USE));
                    let new = widen(&mut builder, next);
                    builder.ins().store(MemFlagsData::trusted(), new, args[2], std::mem::size_of::<u64>() as i32);
                    builder.set_srcloc(ir::SourceLoc::new(0));
                    builder.ins().return_(&[new]);
                }
                builder.switch_to_block(empty);
                let initial = widen(&mut builder, initial);
                builder.ins().store(MemFlagsData::trusted(), initial, args[2], 0);
                builder.ins().store(MemFlagsData::trusted(), initial, args[2], std::mem::size_of::<u64>() as i32);
                builder.ins().return_(&[initial]);
                builder.seal_all_blocks(); builder.finalize(isa.frontend_config());
                let mut compiler = cranelift_codegen::Context::for_function(function);
                compiler.compile(&*isa, &mut Default::default()).unwrap();
                let code = compiler.compiled_code().unwrap();
                let memory = CodeMemory::new(code.buffer.data()).unwrap();
                for (label, location) in [(PREVIOUS_LABEL, PREVIOUS_USE), (NEXT_LABEL, NEXT_USE)] {
                    let uses: Vec<_> = code.buffer.get_srclocs_sorted().iter().filter(|source|
                        source.loc == ir::SourceLoc::new(location) && source.start < source.end).collect();
                    if uses.is_empty() { continue; }
                    let ranges = code.value_labels_ranges.get(&ir::ValueLabel::from_u32(label)).unwrap_or_else(||
                        panic!("{ty} start{start} missing label{label}: {:?}\n{}", compiler.func.dfg.values_labels, compiler.func));
                    for source in uses {
                        assert!(ranges.iter().any(|range| range.start <= source.start && source.start < range.end),
                            "{ty} start{start} label{label} at{}: {ranges:?}", source.start);
                    }
                }
                let native = &compiler.func;
                let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
                let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
                let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
                loops.compute(native, &cfg, &dom);
                let hot: Vec<_> = native.layout.blocks().filter(|block| loops.innermost_loop(*block).is_some())
                    .flat_map(|block| native.layout.block_insts(block)).collect();
                let countdown = hot.iter().filter(|inst| native.dfg.insts[**inst].opcode() == ir::Opcode::Isub).count();
                if start == 0 || (step > 0 && start == min) { assert_eq!(countdown, 1, "{native}"); }
                if countdown != 0 {
                    assert_eq!(countdown, 1, "{native}");
                    assert!(!hot.iter().any(|inst| matches!(native.dfg.insts[*inst], ir::InstructionData::IntCompare {
                        cond: IntCC::SignedLessThan | IntCC::SignedGreaterThanOrEqual
                            | IntCC::SignedGreaterThan | IntCC::SignedLessThanOrEqual, .. })), "{native}");
                    optimized += 1;
                }
                // SAFETY: the verified ABI takes a word, a live atomic byte,
                // and two writable word cells, returning one word.
                let run = unsafe { std::mem::transmute::<*mut u8,
                    unsafe extern "C" fn(u64, *const AtomicU8, *mut u64) -> u64>(memory.pointer()) };
                let bounds: Vec<_> = if ty == types::I8 { (min..=max).collect() } else {
                    [min, start.saturating_sub(17), start.saturating_sub(2), start.saturating_sub(1), start, start.saturating_add(1), start.saturating_add(2),
                        start.saturating_add(17), max].into_iter().filter(|bound| *bound >= min && *bound <= max).collect()
                };
                for bound in bounds { for requested in [0_u8, 1] {
                    let distance = ((i128::from(bound) - i128::from(start)) * i128::from(step)).max(0) as u128;
                    let enters = distance != 0;
                    if requested == 0 && distance > MAX_EXECUTED_TRIPS { continue; }
                    let expected = if !enters { start } else if requested != 0 { start + step } else { bound };
                    let previous = if !enters || requested != 0 { start } else { bound - step };
                    let flag = AtomicU8::new(requested);
                    let mut cells = [0_u64; 2];
                    assert_eq!(unsafe { run(bound as u64 & mask, &flag, cells.as_mut_ptr()) }, expected as u64 & mask,
                        "{ty} start{start} bound{bound} step{step} request{requested}");
                    assert_eq!(cells, [previous as u64 & mask, expected as u64 & mask]);
                    executions += 1;
                }}
            }}
        }
        assert!(executions >= (1_usize << u8::BITS) * (1_usize << u8::BITS) * 4);
        assert!(optimized >= 3 * 4, "each width must optimize both directions from zero and increasing from signed minimum");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn loop_carried_updates_reuse_the_comparison_value() {
        use std::sync::atomic::AtomicU8;
        use cranelift_codegen::settings::Configurable;
        const TEST_NAMESPACE: u32 = u32::from_be_bytes(*b"QLCU");
        const BOUND_LABEL_INDEX: u32 = 0;
        let bound_label = ir::ValueLabel::from_u32(BOUND_LABEL_INDEX);
        const PROBE_SOURCE_OFFSET: u32 = 42;
        let probe_location = ir::SourceLoc::new(PROBE_SOURCE_OFFSET);
        let mut flags = cranelift_codegen::settings::builder();
        flags.set("opt_level", "speed").unwrap();
        flags.set("machine_code_cfg_info", "true").unwrap();
        let isa = cranelift_native::builder().unwrap().finish(cranelift_codegen::settings::Flags::new(flags)).unwrap();
        for ty in [types::I32, types::I64] { for step in [1_i64, -1, 7, -7] {
            let mut signature = ir::Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 4]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut function = ir::Function::with_name_signature(UserFuncName::user(TEST_NAMESPACE, 0), signature);
            function.collect_debug_info();
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let entry = builder.create_block();
            let header = builder.create_block();
            let comparison = builder.create_block();
            let backedge = builder.create_block();
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(header, ty);
            builder.append_block_param(exit, ty);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let narrow = |builder: &mut FunctionBuilder<'_>, value| if ty == types::I32 {
                builder.ins().ireduce(ty, value)
            } else { value };
            let start = narrow(&mut builder, args[0]);
            let bound = narrow(&mut builder, args[1]);
            builder.set_srcloc(ir::SourceLoc::new(0));
            builder.set_val_label(bound, bound_label);
            let empty = builder.ins().icmp(IntCC::Equal, start, bound);
            builder.ins().brif(empty, exit, &[start.into()], header, &[start.into()]);
            builder.switch_to_block(header);
            let previous = builder.block_params(header)[0];
            let next = builder.ins().iadd_imm_u(previous, step);
            let stop = builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), args[2]);
            builder.ins().brif(stop, exit, &[next.into()], comparison, &[]);
            builder.switch_to_block(comparison);
            let done = builder.ins().icmp(IntCC::Equal, next, bound);
            builder.ins().brif(done, exit, &[next.into()], backedge, &[]);
            builder.switch_to_block(backedge);
            builder.set_srcloc(probe_location);
            let observed = if ty == types::I32 { builder.ins().uextend(types::I64, bound) } else { bound };
            builder.ins().store(MemFlagsData::trusted(), observed, args[3], 0);
            builder.set_srcloc(ir::SourceLoc::new(0));
            builder.ins().jump(header, &[next.into()]);
            builder.switch_to_block(exit);
            let result = builder.block_params(exit)[0];
            let result = if ty == types::I32 { builder.ins().uextend(types::I64, result) } else { result };
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            let mut compiler = cranelift_codegen::Context::for_function(function);
            compiler.set_disasm(true);
            let compiled = compiler.compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
            let locations = compiled.value_labels_ranges.get(&bound_label).expect("live bound label");
            assert!(!locations.is_empty());
            assert!(locations.windows(2).all(|pair| pair[0].end <= pair[1].start));
            let probes: Vec<_> = compiled.buffer.get_srclocs_sorted().iter()
                .filter(|source| source.loc == probe_location && source.start < source.end).collect();
            assert!(!probes.is_empty(), "the relocated backedge must retain its probe");
            for probe in probes {
                assert!(locations.iter().any(|range| range.start <= probe.start && probe.end <= range.end),
                    "bound debug location must cover its relocated use: {}..{}, {locations:?}", probe.start, probe.end);
            }
            assert!(compiled.bb_starts.windows(2).all(|pair| pair[0] < pair[1]));
            #[cfg(target_arch = "aarch64")]
            assert_aarch64_cfg_edges(compiled);
            let memory = CodeMemory::new(compiled.buffer.data()).unwrap();
            let native = &compiler.func;
            let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
            let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
            let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
            loops.compute(native, &cfg, &dom);
            assert_eq!(loops.loops().count(), 1);
            let updates = native.layout.blocks().filter(|block| loops.innermost_loop(*block).is_some())
                .flat_map(|block| native.layout.block_insts(block))
                .filter(|inst| native.dfg.insts[*inst].opcode() == ir::Opcode::Iadd).count();
            assert_eq!(updates, 1, "one update serves comparison and backedge: {ty} step{step}\n{native}");
            // SAFETY: the verified function has four word inputs and one word
            // output; the pointers name a live atomic byte and writable probe.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(u64, u64, *const AtomicU8, *mut u64) -> u64>(memory.pointer()) };
            let mask = u64::MAX >> (u64::BITS - ty.bits());
            for seed in [0_u64, 1, u64::MAX, i32::MAX as u64, i64::MAX as u64] {
                for trips in [0_u64, 1, 2, 17, 257] { for requested in [0_u8, 1] {
                    let seed = seed & mask;
                    let bound = seed.wrapping_add(trips.wrapping_mul(step as u64)) & mask;
                    let flag = AtomicU8::new(requested);
                    let expected = if trips == 0 { seed } else if requested != 0 {
                        seed.wrapping_add(step as u64) & mask
                    } else { bound };
                    let mut probe = 0;
                    assert_eq!(unsafe { run(seed, bound, &flag, &mut probe) }, expected,
                        "{ty} step{step} seed{seed} trips{trips} stop{requested}");
                    assert_eq!(probe, if trips > 1 && requested == 0 { bound } else { 0 });
                }}
            }
        }}
    }

    #[cfg(feature = "aot")]
    #[test]
    fn scalar_loop_rotation_preserves_trip_counts_and_exact_header_recovery() {
        let source = r#"
            function count(n){n=n|0;let result=0;for(let i=0;i<n;i=(i+1)|0)result=(result+1)|0;return result;}
            function fractional(){let result=0;for(let i=0;i<2.5;i=(i+1)|0)result=(result+1)|0;return result;}
            function notANumber(){let result=0;for(let i=0;i<(0/0);i=(i+1)|0)result=(result+1)|0;return result;}
            function mutable(n,limit){let result=0;for(let i=0;i<n;i=(i+1)|0){result=(result+1)|0;if(i===2)n=limit;}return result;}
            for(const n of [-1,0,1,2,17])if(count(n)!==(n>0?n:0))throw 'trip count';
            if(fractional()!==3||notANumber()!==0)throw 'floating condition';
            let probes=0,limit={valueOf(){probes++;return 3;}};
            if(mutable(5,limit)!==3||probes!==1)throw 'header recovery';
            throw 42;
        "#;
        let program = crate::Engine::specialize(source, "loop-rotation.cjs").unwrap();
        let id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "count")).unwrap();
        let targets = static_call_targets(&program);
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        let region = lower_static_typed_regions(&program, id as u32, &targets[id], &*isa, 0).into_iter().next().unwrap();
        let native = &region.function;
        let comparisons = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
            .filter(|instruction| matches!(native.dfg.insts[*instruction],
                ir::InstructionData::IntCompare { cond: IntCC::SignedLessThan, .. })).count();
        assert_eq!(comparisons, 2, "the zero-trip guard and rotated latch share the original comparison");
        let entry = native.layout.entry_block().unwrap();
        let context = native.dfg.block_params(entry)[0];
        let interrupt_offset = if FAULT_POLL_SUPPORTED { std::mem::offset_of!(CallContext, poll_start) }
            else { std::mem::offset_of!(CallContext, interrupt) };
        let owners: Vec<_> = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
            .filter(|instruction| matches!(native.dfg.insts[*instruction],
                ir::InstructionData::Load { arg, offset, .. } if arg == context
                    && i32::from(offset) == interrupt_offset as i32)).collect();
        assert_eq!(owners.len(), 1, "resolve the stable interrupt owner once per region");
        let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
        let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
        for block in native.layout.blocks() {
            if !dom.is_reachable(block) { continue; }
            for header in cfg.succ_iter(block).filter(|target| dom.dominates(*target, block, &native.layout)) {
                assert!(dom.dominates(owners[0], header, &native.layout), "interrupt owner must precede the loop");
            }
        }
        let native = linked_optimized_test_program(program);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(42.0)), "{error}");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn scalar_rotated_loop_observes_termination_requested_after_entry() {
        use std::{sync::mpsc, thread, time::Duration};
        const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
        fn started(context: &mut crate::NativeContext<'_, crate::test_host::TestHost>,
            _: crate::RootId, _: &[crate::RootId]) -> Result<crate::RootId, crate::RootedError> {
            context.host_mut().state::<mpsc::Sender<()>>().send(()).unwrap();
            Ok(context.undefined())
        }
        for source in [
            "function spin(){started();let x=1;for(let i=0;i<1/0;i=(i+1)|0){x=x^(x<<13);}return x;}spin();",
            "function run(){function spin(x){for(let i=0;i<1/0;i=(i+1)|0)x=x^(x<<13);return x;}started();for(let n=0;n<2;n++)spin(n);}run();",
        ] {
            let (ready, entered) = mpsc::channel::<()>();
            let (handle_sender, handle_receiver) = mpsc::channel();
            let (completed, completion) = mpsc::channel();
            let worker = thread::Builder::new().stack_size(crate::WORKER_STACK_SIZE).spawn(move || {
                let program = crate::Engine::specialize(source, "rotated-interrupt.cjs").unwrap();
                let native = linked_optimized_test_program(program);
                let host = crate::test_host::TestHost::default()
                    .with_natives(crate::host_functions![global "started" (0) => started])
                    .with_state(Rc::new(ready));
                let mut vm = Vm::new(host);
                vm.set_execution_mode(ExecutionMode::NativeOnly);
                handle_sender.send(vm.execution_interrupt.clone()).unwrap();
                let budget = crate::stack::GuestCallGuard::remaining();
                let error = vm.execute_native_program(&native, true).unwrap_err();
                assert!(error.is_execution_terminated(), "{error}");
                assert_eq!(crate::stack::GuestCallGuard::remaining(), budget);
                completed.send(()).unwrap();
            }).unwrap();
            let handle = handle_receiver.recv_timeout(WAIT_TIMEOUT).unwrap();
            entered.recv_timeout(WAIT_TIMEOUT).unwrap();
            thread::sleep(Duration::from_millis(20));
            handle.request_termination();
            completion.recv_timeout(WAIT_TIMEOUT).expect("native infinite loop must observe the live interrupt");
            worker.join().unwrap();
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn scalar_word32_pairs_preserve_overflow_rounding_and_live_numbers() {
        let source = r#"
            function add(a,b){let out=0;for(let i=0;i<1;i++){out=(a+b)|0;}return out;}
            function sub(a,b){let out=0;for(let i=0;i<1;i++){out=(a-b)|0;}return out;}
            function mul(a,b){let out=0;for(let i=0;i<1;i++){out=(a*b)|0;}return out;}
            function observed(a,b){let sum=0,bits=0;for(let i=0;i<1;i++){sum=a+b;bits=sum|0;}return sum;}
            if(add(2147483647,1)!==-2147483648 || sub(-2147483648,1)!==2147483647)throw 'overflow';
            if(mul(2147483647,2147483647)!==0)throw 'rounded multiply';
            if(observed(2147483647,1)!==2147483648)throw 'live intermediate';
            if(add(1.5,2)!==3 || add(NaN,1)!==0 || sub(Infinity,1)!==0 ||
                add('1','2')!==12 || !Object.is(add(-0,-0),0))throw 'number fallback';
            let conversions=0;
            let object={valueOf(){conversions++;return 2147483647;}};
            if(add(object,1)!==-2147483648 || conversions!==1)throw 'coercion replay';
            throw 42;
        "#;
        let program = crate::Engine::specialize(source, "word32-pairs.cjs").unwrap();
        let id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "add")).unwrap();
        let targets = static_call_targets(&program);
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        let region = lower_static_typed_regions(&program, id as u32, &targets[id], &*isa, 0).into_iter().next().unwrap();
        assert_eq!(region.header, 0, "scalar preheader definitions must enter the same SSA graph");
        assert!(!region.function.layout.blocks().flat_map(|block| region.function.layout.block_insts(block))
            .any(|instruction| region.function.dfg.insts[instruction].opcode() == ir::Opcode::Fadd),
            "the unobserved add/ToInt32 pair must stay Word32");
        let native = linked_optimized_test_program(program);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(42.0)), "{error}");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn typed_owner_export_after_private_warmup_keeps_native_termination() {
        use std::{sync::mpsc, thread, time::Duration};
        const WAIT: Duration = Duration::from_secs(10);
        fn expose(c: &mut crate::NativeContext<'_, crate::test_host::TestHost>, _: crate::RootId,
            _: &[crate::RootId]) -> Result<crate::RootId, crate::RootedError> {
            let handle = c.execution_interrupt_handle();
            c.host_mut().state::<mpsc::Sender<crate::ExecutionInterrupt>>().send(handle).unwrap();
            Ok(c.undefined())
        }
        for linked in [false, true] {
            let (send_handle, receive_handle) = mpsc::channel::<crate::ExecutionInterrupt>();
            let (send_done, receive_done) = mpsc::channel();
            let worker = thread::Builder::new().stack_size(crate::WORKER_STACK_SIZE).spawn(move || {
                let program = crate::Engine::specialize(
                    "function spin(n,x){n=n|0;x=x|0;for(let i=0;i<n;i=(i+1)|0){x=x^(x<<13);x=x^(x>>>17);x=x^(x<<5);}return x|0;}let result=0;for(let warm=0;warm<64;warm++)result^=spin(2000,warm);expose();result=spin(2147483647,result);",
                    "late-interrupt-owner.js").unwrap();
                let host = crate::test_host::TestHost::default()
                    .with_natives(crate::host_functions![global "expose" (0) => expose])
                    .with_state(Rc::new(send_handle));
                let mut vm = Vm::new(host);
                vm.set_execution_mode(if linked { ExecutionMode::NativeOnly } else { ExecutionMode::Compiled });
                let result = if linked { vm.execute_native_program(&linked_optimized_test_program(program), true) }
                    else { vm.execute(&program) };
                assert!(result.unwrap_err().is_execution_terminated());
                send_done.send(()).unwrap();
            }).unwrap();
            let handle = receive_handle.recv_timeout(WAIT).expect("warmup must reach the exporting callback");
            thread::sleep(Duration::from_millis(20));
            handle.request_termination();
            receive_done.recv_timeout(WAIT).expect("previously private code must poll after authority escapes");
            worker.join().unwrap();
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_integer_loop_preserves_call_results() {
        let source = r#"
            const feedback = 0xD008;
            function state(n, seed) {
                n |= 0; let x = seed | 0;
                for (let i = 0; i < n; i = (i + 1) | 0) {
                    if ((x & 1) === 0) x = x >> 1;
                    else x = (x >> 1) ^ feedback;
                }
                return x | 0;
            }
            for (let warm = 0; warm < 64; warm++) state(1, warm);
            print(state(0, 123456789));
            print(state(17, 123456789));
            print(state(-1, -2147483648));
            print(state(1, -1));
        "#;
        for result in ["x | 0", "x >>> 0", "x + 0.5", "x < 0"] {
            let body = source.replace("x | 0;", &format!("{result};"));
            for source in [body.clone(), format!("(function () {{ {body} }})();")] {
                let program = crate::Engine::specialize(&source, "integer-loop-result.js").unwrap();
                let mut expected = Vm::new(crate::test_host::TestHost::default());
                expected.set_execution_mode(ExecutionMode::Interpreted);
                expected.execute(&program).unwrap();
                let native = linked_optimized_test_program(program);
                let mut vm = Vm::new(crate::test_host::TestHost::default());
                vm.set_execution_mode(ExecutionMode::NativeOnly);
                vm.execute_native_program(&native, true).unwrap();
                assert_eq!(*vm.host.lines(), *expected.host.lines(), "{source}");
            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn invariant_capture_reads_preserve_mutation_coercion_and_initialization() {
        let source = r#"
            function make(feedback) {
                return {
                    run(n, x) {
                        n |= 0; x |= 0;
                        for (let i = 0; i < n; i = (i + 1) | 0) x = (x >> 1) ^ feedback;
                        return x | 0;
                    },
                    set(value) { feedback = value; }
                };
            }
            const pipe = make(0xD008);
            for (let warm = 0; warm < 64; warm++) pipe.run(2, warm);
            for (const value of [0, -1, 1.5, 4294967295, '7', null, undefined]) {
                pipe.set(value); print(pipe.run(17, 123456789));
            }
            let conversions = 0;
            pipe.set({valueOf() { conversions++; pipe.set(3); return 7; }});
            print(pipe.run(3, 123)); print(conversions);
            pipe.set(1n);
            print(pipe.run(0, 123));
            try { pipe.run(1, 123); } catch (error) { print(error instanceof TypeError); }
            {
                function before(n) {
                    let x = 0;
                    for (let i = 0; i < n; i = (i + 1) | 0) x ^= later;
                    return x | 0;
                }
                print(before(0));
                try { before(1); } catch (error) { print(error instanceof ReferenceError); }
                const later = 7;
                print(before(1));
            }
            let changing = 1;
            function update() { changing = (changing + 1) | 0; }
            function calls(n) {
                let x = 0;
                for (let i = 0; i < n; i = (i + 1) | 0) { x ^= changing; update(); }
                return x | 0;
            }
            function stores(n) {
                let x = 0;
                for (let i = 0; i < n; i = (i + 1) | 0) { x ^= changing; changing = i; }
                return x | 0;
            }
            print(calls(17)); print(stores(17));
        "#;
        let program = crate::Engine::specialize(source, "capture-invariants.js").unwrap();
        let mut expected = Vm::new(crate::test_host::TestHost::default());
        expected.set_execution_mode(ExecutionMode::Interpreted);
        expected.execute(&program).unwrap();
        for mode in [ExecutionMode::Compiled, ExecutionMode::NativeOnly] {
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.set_execution_mode(mode);
            if mode == ExecutionMode::NativeOnly {
                vm.execute_native_program(&linked_optimized_test_program(program.clone()), true).unwrap();
            } else { vm.execute(&program).unwrap(); }
            assert_eq!(*vm.host.lines(), *expected.host.lines(), "{mode:?}");
        }
    }

    #[cfg(feature = "aot")]
    fn linked_optimized_test_program(program: ResidualProgram) -> crate::NativeJsProgram<crate::test_host::TestHost> {
        let targets = static_call_targets(&program);
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        let mut owners = Vec::new();
        let mut entries = Vec::new();
        for (id, targets) in targets.iter().enumerate().take(program.function_count()) {
            let mut regions = lower_static_typed_regions(&program, id as u32, targets, &*isa, 0);
            for region in &mut regions {
                // Bind the pure helper-table resolver as the image linker does.
                let instructions: Vec<_> = region.function.layout.blocks()
                    .flat_map(|block| region.function.layout.block_insts(block)).collect();
                for instruction in instructions {
                    if region.function.dfg.insts[instruction].opcode() == ir::Opcode::Call {
                        region.function.dfg.insts[instruction] = ir::InstructionData::UnaryImm {
                            opcode: ir::Opcode::Iconst,
                            imm: ir::immediates::Imm64::new(native_js_lane_helpers::<crate::test_host::TestHost>() as usize as i64),
                        };
                    }
                }
            }
            let typed: Vec<_> = regions.iter().map(|region|
                StaticTypedRegionEntry {header:region.header,function:&region.function}).collect();
            let lowered = lower_js_general_dispatch(&program,id as u32,false,&*isa,
                JsGeneralCallout::Address(fixed_test_semantic as *const () as usize),&[], &typed);
            let (memory,entry,operands)=match lowered {
                Ok((body,_)) => {
                    let (memory,entry)=compile_and_publish_entry(body,&*isa,usize::MAX).unwrap();
                    (memory,entry,Vec::new())
                }
                Err(_) => live_native_function(&program,id as u32,None),
            };
            entries.push(unsafe {std::mem::transmute::<Entry,crate::NativeJsFunctionEntry>(entry)});
            owners.push((memory,operands));
        }
        // SAFETY: all entries use this exact program/host ABI and owner retains code.
        unsafe {crate::NativeJsProgram::from_entries(program,entries,Rc::new(owners))}.unwrap()
    }

    #[cfg(feature = "aot")]
    #[test]
    fn function_bound_osr_owns_definitions_without_retaining_closure_identities() {
        let program = crate::Engine::specialize(
            "function pipe(){function step(x){return (x+1)|0;}let x=0;for(let i=0;i<3;i=(i+1)|0)x=step(x);return x;}",
            "definition-lifetime.js").unwrap();
        let caller = program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "pipe")).unwrap();
        let callee = program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "step")).unwrap();
        let body = program.function(caller);
        let header = body.instructions().enumerate().find_map(|(pc, instruction)| {
            if instruction.op() != Op::Jump { return None; }
            let target = instruction.jump_target();
            (target as usize <= pc).then_some(target)
        }).unwrap();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let method = vm.compile_lane_method(&program, caller as u32, &|function, pc|
            (function == caller as u32 && body.instructions().nth(pc as usize).is_some_and(|instruction| instruction.op() == Op::Call))
                .then_some(CallTarget { function: callee as u32, callee: CalleeBinding::Function }).into_iter().collect(),
            &|_| (0, 0), Vec::new(), Some(header)).unwrap();
        assert_eq!(method.inlined.len(), 1);
        assert_eq!(method.inlined[0].target.callee, CalleeBinding::Function);
        assert_eq!(method.recovery_space().activations, 1);
        assert!(!method.supports_first_entry());
        assert!(!method.amortizes_entry());
        assert!(!method.calls_only_natively());
        let published = vm.programs.publish_lane_method(method);
        // No native execution or method borrower remains at either event.
        unsafe { vm.programs.retire_dead_native_calls(|_| false); }
        assert!(published.method().is_some(), "a pure definition embeds no closure locator");
        unsafe { vm.programs.retire_native_calls(); }
        assert!(published.method().is_none(), "realm/reset still retires the publication");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn independent_typed_loops_preserve_effect_boundaries_and_local_call_binding() {
        let source = r#"
            let effects='';
            function pipe(count,seed,hook){
                function step(x){x=x|0;for(let j=0;j<1;j=(j+1)|0)x=(x^(x<<3))|0;return x|0;}
                for(let warm=0;warm<3;warm++)step(warm);
                hook();
                let rounds=count,value=seed;
                for(let i=0;i<rounds;i=(i+1)|0)value=step(value);
                return value;
            }
            function oracle(n,x){for(let i=0;i<n;i++)x=(x^(x<<3))|0;return x;}
            let hook=()=>{effects+='h';};
            if(pipe(8,17,hook)!==oracle(8,17)||effects!=='h')throw 'separated loops';
            let coercions=0;
            let input={valueOf(){effects+='v';coercions++;return 7;}};
            if(pipe(3,input,hook)!==oracle(3,7)||effects!=='hhv'||coercions!==1)throw 'coercion replay';
            for(let value of [1.5,-0,2147483648,Infinity,NaN]){
                if(pipe(3,value,()=>{})!==oracle(3,value))throw 'numeric entry fallback';
            }
            if(pipe(3.25,7,()=>{})!==oracle(3.25,7))throw 'fractional loop bound';
            let untouched={valueOf(){throw 'zero-trip coercion';}};
            if(pipe(0,untouched,()=>{})!==untouched||pipe(NaN,untouched,()=>{})!==untouched)throw 'zero-trip identity';
            for(let value of [1n,Symbol()]){
                try{pipe(2,value,()=>{});throw 'missing type error';}
                catch(e){if(!(e instanceof TypeError))throw e;}
            }
            let sentinel={};
            let bad={valueOf(){throw sentinel;}};
            try{pipe(2,bad,()=>{});throw 'missing coercion throw';}catch(e){if(e!==sentinel)throw e;}
            try{pipe(2,3,()=>{effects+='t';throw sentinel;});throw 'missing throw';}
            catch(e){if(e!==sentinel||effects!=='hhvt')throw 'opaque throw';}
            function rebound(n){
                function plus(x){for(let j=0;j<1;j++)x=(x+1)|0;return x;}
                let x=0;
                for(let i=0;i<n;i++){
                    if(i===2)plus=function(x){return (x+10)|0;};
                    x=plus(x);
                }
                return x;
            }
            if(rebound(4)!==22)throw 'rebound callee';
            throw 42;
        "#;
        for typescript in [false, true] {
            let annotated = source.replace("function pipe(count,seed,hook)",
                "function pipe(count:number,seed:number,hook:()=>void)");
            let program = if typescript { crate::Engine::specialize_typescript(&annotated, "separate.cts") }
                else { crate::Engine::specialize(source, "separate.cjs") }.unwrap();
            let id = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "pipe")).unwrap();
            let targets = static_call_targets(&program);
            let body = program.function(id);
            let hinted = body.instructions().enumerate().filter(|(pc, instruction)|
                instruction.op() == Op::Call && targets[id][*pc].is_some()).count();
            assert_eq!(hinted, 2, "local closure hints must reach both loops across the opaque call");
            let unit = lane_unit::unit(&program, id as u32, body, &|caller, pc|
                (caller == id as u32).then(|| targets[id][pc as usize]).flatten()
                    .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
                &|id| program.function_get(id as usize)).unwrap();
            assert_eq!(unit.frames.len(), 2, "both numeric calls must inline: {}", program.disassemble());
            let isa = super::super::jit_backend::target_isa("speed").unwrap();
            let regions = lower_static_typed_regions(&program, id as u32, &targets[id], &*isa, 0);
            assert_eq!(regions.len(), 2, "each independent caller loop needs an entry");
            assert_ne!(regions[0].function.name, regions[1].function.name, "inliner keys must be distinct");
            for region in &regions {
                let function = &region.function;
                let bindings: Vec<_> = function.layout.blocks().flat_map(|block| function.layout.block_insts(block))
                    .filter(|instruction| {
                        if function.dfg.insts[*instruction].opcode() != ir::Opcode::CallIndirect { return false; }
                        let target = function.dfg.resolve_aliases(function.dfg.inst_args(*instruction)[0]);
                        let ir::ValueDef::Result(load, _) = function.dfg.value_def(target) else { return false; };
                        matches!(function.dfg.insts[load], ir::InstructionData::Load { offset, .. }
                            if i32::from(offset) == LaneHelper::FunctionBinding as i32 * std::mem::size_of::<usize>() as i32)
                    }).collect();
                assert_eq!(bindings.len(), 1, "one actual callee admission per region: {function}\n{}", program.disassemble());
                let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(function);
                let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(function, &cfg);
                for block in function.layout.blocks() {
                    if !dom.is_reachable(block) { continue; }
                    for header in cfg.succ_iter(block).filter(|target| dom.dominates(*target, block, &function.layout)) {
                        assert!(dom.dominates(bindings[0], header, &function.layout), "callee admission must precede every loop");
                    }
                }
            }
            let native = linked_optimized_test_program(program);
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.execute_native_program(&native, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::integer(42)), "{error}");
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn bound_packed_integer_fields_avoid_per_iteration_helpers() {
        for (count, typescript) in [1000, 10000].into_iter()
            .flat_map(|count| [false, true].map(move |typescript| (count, typescript))) {
            let expected = (0..count).fold(123456789_i32, |value, _|
                (value >> 1) ^ if value & 1 != 0 { 0xD008 } else { 0 });
            let source = format!(r#"(function() {{
                function advance(state) {{
                    state.count = (state.count - 1) | 0;
                    state.control = (state.control >> 1) ^ ((state.control & 1) ? 0xD008 : 0);
                }}
                function run(n, seed) {{
                    const state = {{count: n | 0, control: seed | 0}};
                    while(state.count > 0) advance(state);
                    return state.control | 0;
                }}
                for(let i=0;i<64;i++) run(1,i);
                if(run({count},123456789)!=={expected}) throw 'incorrect update';
                throw 42;
            }})();"#);
            let program = if typescript { crate::Engine::specialize_typescript(&source, "packed-fields.cts") }
                else { crate::Engine::specialize(&source, "packed-fields.cjs") }.unwrap();
            let native = linked_optimized_test_program(program);
            FIELD_HELPER_CALLS.with(|counts| counts.set([0; 2]));
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.execute_native_program(&native, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::integer(42)), "{error}");
            let [reads, writes] = FIELD_HELPER_CALLS.with(std::cell::Cell::get);
            assert!(reads <= 2, "only entry misses may call the read helper: {reads}");
            assert_eq!(writes, 0, "packed integer stores must stay native");
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn bound_packed_fields_preserve_widening_and_exact_effects() {
        let half_range = 1_i64 << (crate::value::NarrowLayout::INTEGER_BITS - 1);
        let source = format!(r#"
            function step(state, delta) {{
                state.count = (state.count + 1) | 0;
                state.value = state.value + delta;
                return state.value;
            }}
            let s = {{count: 0, value: 0}};
            for(let i=0;i<64;i++) step(s,1);
            for(const start of [{half_range}-1, -{half_range}, 0.5, -0, 'x']) for(const delta of [-1,1]) {{
                s = {{count: 0, value: start}};
                if(step(s,delta)!==start+delta || s.count!==1) throw 'widening';
                if(step(s,-delta)!==(start+delta)+(-delta) || s.count!==2) throw 'second write';
            }}
            let calls=0, marker={{}};
            s={{count:0,value:{{valueOf(){{calls++;throw marker;}}}}}};
            try {{step(s,1);throw 'missing throw';}} catch(e) {{if(e!==marker)throw e;}}
            if(calls!==1 || s.count!==1) throw 'replayed prefix';
            s={{count:0,value:7}};
            let stored;
            Object.defineProperty(s,'value',{{get(){{calls++;return 7;}},set(v){{stored=v;}},configurable:true}});
            if(step(s,2)!==7 || stored!==9 || calls!==3 || s.count!==1) throw 'accessor';
            delete s.value;
            Object.setPrototypeOf(s,{{value:5}});
            if(step(s,3)!==8 || s.value!==8 || s.count!==2) throw 'inherited';
            s={{count:0,value:7}};
            step(s,0);
            Object.defineProperty(s,'value',{{writable:false}});
            if(step(s,2)!==7 || s.count!==2) throw 'sloppy readonly';
            function strictStep(state) {{
                'use strict';
                state.count = (state.count + 1) | 0;
                state.value = state.value + 1;
            }}
            s={{count:0,value:0}};
            for(let i=0;i<64;i++) strictStep(s);
            Object.defineProperty(s,'value',{{writable:false}});
            try {{strictStep(s);throw 'missing readonly throw';}} catch(e) {{if(!(e instanceof TypeError))throw e;}}
            if(s.count!==65 || s.value!==64) throw 'strict readonly prefix';
            throw 42;
        "#);
        for typescript in [false, true] {
            let program = if typescript { crate::Engine::specialize_typescript(&source, "packed-transitions.cts") }
                else { crate::Engine::specialize(&source, "packed-transitions.cjs") }.unwrap();
            let native = linked_optimized_test_program(program);
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.execute_native_program(&native, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::integer(42)), "{error}");
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn captured_call_hints_share_native_inlining_and_exact_recovery() {
        let source = r#"
            function make(delta) {
                let step = function step(state) {
                    state.count = (state.count - 1) | 0;
                    state.value = state.value + delta;
                };
                function run(seed) {
                    const state = {count: 20, value: seed};
                    globalThis.last = state;
                    while (state.count > 0) step(state);
                    return state.value;
                }
                return {run, replace(value) {step = value;}};
            }
            const a = make(2), b = make(5);
            if (a.run(3) !== 43 || b.run(3) !== 103) throw 'closure environment';
            a.replace(function replacement(state) {state.count--; state.value += 100;});
            if (a.run(3) !== 2003 || b.run(3) !== 103) throw 'replacement';
            let calls = 0;
            const marker = {};
            const failing = make({valueOf() {calls++; throw marker;}});
            try {failing.run(3); throw 'missing throw';} catch (error) {if (error !== marker) throw error;}
            if (calls !== 1 || last.count !== 19 || last.value !== 3) throw 'replayed prefix';
            throw 42;
        "#;
        for typescript in [false, true] {
            let program = if typescript { crate::Engine::specialize_typescript(source, "captured-target.cts") }
                else { crate::Engine::specialize(source, "captured-target.cjs") }.unwrap();
            let id = |name: &str| program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
            let (caller, callee) = (id("run"), id("step"));
            let targets = static_call_targets(&program);
            assert!(targets[caller].contains(&Some(callee as u32)), "capture must supply the shared inliner");
            let unit = lane_unit::unit(&program, caller as u32, program.function(caller), &|function, pc|
                targets[function as usize].get(pc as usize).copied().flatten()
                    .map(|function| CallTarget {function, callee: CalleeBinding::Function}).into_iter().collect(),
                &|function| program.function_get(function as usize)).unwrap();
            assert_eq!(unit.frames.len(), 1);
            assert_eq!(unit.frames[0].recovery, lane_unit::CallRecovery::Materialize);
            let isa = super::super::jit_backend::target_isa("speed").unwrap();
            assert!(!lower_static_typed_regions(&program, caller as u32, &targets[caller], &*isa, 0).is_empty());
            let native = linked_optimized_test_program(program);
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.execute_native_program(&native, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::integer(42)), "{error}");
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn effectful_inline_recovery_materializes_exact_callee_state() {
        let program = crate::Engine::specialize(
            "let p={};p.consume=function(input){this.count=this.count+1;let value=input.value;return value+this.count;};function run(owner,input){let value=owner.consume(input);return value+3;}",
            "exact-inline.js").unwrap();
        let caller = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
        let targets = static_call_targets(&program);
        let unit = lane_unit::unit(&program, caller as u32, program.function(caller), &|function, pc| targets[function as usize]
            .get(pc as usize).copied().flatten()
            .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
            &|id| program.function_get(id as usize)).unwrap();
        assert_eq!(unit.frames.len(), 1);
        assert_eq!(unit.frames[0].recovery, lane_unit::CallRecovery::Materialize);
        let isa = super::super::jit_backend::target_isa_for("speed",
            super::super::jit_backend::CodePublication::RelocatableObject).unwrap();
        let region = lower_static_typed_regions(&program, caller as u32, &targets[caller], &*isa, 0).into_iter().next()
            .expect("a write followed by a fallible read has exact recovery");
        let offset = LaneHelper::MaterializeInline as i32 * std::mem::size_of::<usize>() as i32;
        assert!(region.function.layout.blocks().flat_map(|block| region.function.layout.block_insts(block))
            .any(|instruction| matches!(region.function.dfg.insts[instruction],
                ir::InstructionData::Load { offset: actual, .. } if i32::from(actual) == offset)));
        assert!(!region.function.sized_stack_slots.is_empty(), "callee snapshots have native stack homes");
        crate::lower_native_js_module(&program, "exact_inline").expect("materialized exits link and verify");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn materialized_inline_parameter_survives_gc_reflection_and_throw() {
        let source = r#"
            let keep={value:42}, marker={}; let p={};
            p.consume=function consume(input){this.count=this.count+1;input=input.next;return input.value+this.count;};
            let runner={run:function run(owner,input){let result=owner.consume(input);return result+3;}};
            let owner=Object.create(p);owner.count=0;
            let leaf={value:7},input={next:leaf};
            for(let i=0;i<20;i++)runner.run(owner,input);
            Object.defineProperty(leaf,'value',{get(){
                $262.gc();
                if(p.consume.arguments[0]!==leaf||p.consume.caller!==runner.run)throw 'reflection';
                return keep.value;
            },configurable:true});
            if(runner.run(owner,input)!==66||owner.count!==21)throw 'prefix';
            Object.defineProperty(leaf,'value',{get(){$262.gc();throw marker;}});
            try{runner.run(owner,input);throw 'missing throw';}catch(e){if(e!==marker)throw e;}
            if(owner.count!==22)throw 'replay';throw keep.value;
        "#;
        for (typescript, terminal) in [false, true].into_iter()
            .flat_map(|typescript| [false, true].map(|terminal| (typescript, terminal))) {
            let source = if terminal {
                source.replace("let result=owner.consume(input);return result+3;", "return owner.consume(input);")
                    .replace("!==66", "!==63")
            } else { source.to_owned() };
            let program = if typescript { crate::Engine::specialize_typescript(&source, "exact-gc.cts") }
                else { crate::Engine::specialize(&source, "exact-gc.cjs") }.unwrap();
            let caller = program.functions.iter().position(|body|
                body.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
            let targets = static_call_targets(&program);
            let isa = super::super::jit_backend::target_isa("speed").unwrap();
            let mut region = lower_static_typed_regions(&program, caller as u32, &targets[caller], &*isa, 0).into_iter().next().unwrap();
            let consume = program.functions.iter().position(|body|
                body.header().name.is_some_and(|name| &program.atoms[name as usize] == "consume")).unwrap();
            let read = program.function(consume).instructions().position(|instruction|
                instruction.op() == Op::GetField && matches!(instruction.field_lookup(),
                    Some(FieldLookup::Atom { atom, .. }) if &program.atoms[atom as usize] == "value")).unwrap();
            // Bind the image's pure helper-table resolver to this test host.
            // All other lowering and recovery are the production paths.
            let instructions: Vec<_> = region.function.layout.blocks()
                .flat_map(|block| region.function.layout.block_insts(block)).collect();
            for instruction in instructions {
                if region.function.dfg.insts[instruction].opcode() == ir::Opcode::Call {
                    region.function.dfg.insts[instruction] = ir::InstructionData::UnaryImm {
                        opcode: ir::Opcode::Iconst,
                        imm: ir::immediates::Imm64::new(native_js_lane_helpers::<crate::test_host::TestHost>() as usize as i64),
                    };
                }
            }
            let mut entries = Vec::new();
            let mut owners = Vec::new();
            for id in 0..program.function_count() {
                let (memory, entry, operands) = if id == caller {
                    let (body, _) = lower_js_general_dispatch(&program, id as u32, false, &*isa,
                        JsGeneralCallout::Address(fixed_test_semantic as *const () as usize), &[],
                        &[StaticTypedRegionEntry { header: region.header, function: &region.function }]).unwrap();
                    let (memory, entry) = compile_and_publish_entry(body, &*isa, usize::MAX).unwrap();
                    (memory, entry, Vec::new())
                } else { live_native_function(&program, id as u32, None) };
                // SAFETY: entries are lowered for this exact host/program ABI.
                entries.push(unsafe { std::mem::transmute::<Entry, crate::NativeJsFunctionEntry>(entry) });
                owners.push((memory, operands));
            }
            // SAFETY: owner retains all code and operation operands above.
            let native = unsafe { crate::NativeJsProgram::from_entries(program, entries, Rc::new(owners)) }.unwrap();
            let before = INLINE_MATERIALIZATIONS.with(std::cell::Cell::get);
            let mut vm = Vm::new(crate::test_host::TestHost::default().with_test262());
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.execute_native_program(&native, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::number(42.0)), "{error}");
            assert!(INLINE_MATERIALIZATIONS.with(std::cell::Cell::get) > before,
                "the exact callee materialization path must actually execute");
            assert_eq!(INLINE_LAST_MATERIALIZATION.with(std::cell::Cell::get), Some((consume as u32, read as u32)),
                "recovery must occur at the read after the native prefix store and parameter update");
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_typed_regions_share_register_ir_and_runtime_bound_helpers() {
        let program = crate::Engine::specialize(
            "function tail(node) { node.seen=1; while(node.next!==null) node=node.next; return node; }",
            "typed-loop.js",
        ).unwrap();
        let isa = super::super::jit_backend::target_isa_for("speed",
            super::super::jit_backend::CodePublication::RelocatableObject).unwrap();
        let residual = lower_static_typed_regions(&program, 1, &[], &*isa, 0).into_iter().next().expect("call-free typed loop");
        assert!(residual.header > 0, "loop admission must skip the effectful prefix");
        let has_field_helper = |native: &ir::Function, resolver_symbol: u32| {
            let load_base = |value: ir::Value, expected_offset: i32| {
                let ir::ValueDef::Result(instruction, _) = native.dfg.value_def(native.dfg.resolve_aliases(value)) else { return None };
                let ir::InstructionData::Load { offset, .. } = native.dfg.insts[instruction] else { return None };
                (i32::from(offset) == expected_offset).then(|| native.dfg.inst_args(instruction)[0])
            };
            native.layout.blocks().flat_map(|block| native.layout.block_insts(block)).any(|instruction| {
                if native.dfg.insts[instruction].opcode() != ir::Opcode::CallIndirect { return false; }
                let target = native.dfg.inst_args(instruction)[0];
                let Some(table) = load_base(target, LaneHelper::FieldGet as i32 * std::mem::size_of::<usize>() as i32) else { return false };
                let ir::ValueDef::Result(resolver, _) = native.dfg.value_def(native.dfg.resolve_aliases(table)) else { return false };
                let ir::InstructionData::Call { func_ref, .. } = native.dfg.insts[resolver] else { return false };
                let ir::ExternalName::User(name) = native.dfg.ext_funcs[func_ref].name else { return false };
                native.params.user_named_funcs()[name] == ir::UserExternalName::new(
                    super::super::native_object::NATIVE_FUNCTION_NAMESPACE, resolver_symbol)
            })
        };
        assert!(has_field_helper(&residual.function, 0), "field helper must come from the linked runtime resolver");
        let module = crate::lower_native_js_module(&program, "typed_loop_test").unwrap();
        let caller = module.functions.iter().find_map(|function| match function {
            super::super::native_object::NativeFunction::Definition { name, function }
                if name == "typed_loop_test_function_1" => Some(function), _ => None,
        }).unwrap();
        let resolver = module.functions.iter().position(|function| matches!(function,
            super::super::native_object::NativeFunction::Import { name, .. }
                if name == crate::NATIVE_JS_LANE_HELPERS_SYMBOL)).unwrap() as u32;
        assert!(has_field_helper(caller, resolver), "the exact field-helper chain must be inlined into the caller");
        let dynamic_pc_tables = caller.layout.blocks().flat_map(|block| caller.layout.block_insts(block))
            .filter(|instruction| caller.dfg.insts[*instruction].opcode() == ir::Opcode::BrTable).count();
        assert!(dynamic_pc_tables <= 1, "only canonical source-PC admission may retain a dynamic dispatcher");
        assert!(!module.functions.iter().any(|function| matches!(function,
            super::super::native_object::NativeFunction::Definition { name, .. } if name.ends_with("_typed_region"))),
            "the inlined residual must not retain a second native body");
        let leaf = crate::Engine::specialize("function id(x) { return x; } id(1);", "leaf.js").unwrap();
        let leaf = crate::lower_native_js_module(&leaf, "leaf_test").unwrap();
        assert!(!leaf.functions.iter().any(|function| matches!(function,
            super::super::native_object::NativeFunction::Import { name, .. }
                if name == crate::NATIVE_JS_LANE_HELPERS_SYMBOL)), "units alone own helper-resolver imports");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn function_binding_requires_call_budget_and_clear_interrupt() {
        let (native, _, _) = linked_test_program("throw function leaf(){};", false);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let callee = vm.execute_native_program(&native, true).unwrap_err().thrown_value().unwrap();
        let (program, function, _) = vm.same_realm_user_target(callee).unwrap();
        let admitted = |vm: &Vm<crate::test_host::TestHost>| unsafe {
            lane_function_binding::<crate::test_host::TestHost>(vm as *const _ as u64, 0,
                u64::from(program.raw()), callee.to_bits(), u64::from(function))
        };
        assert_eq!(admitted(&vm), 1);
        {
            let _limit = crate::stack::GuestCallLimitGuard::enter(1);
            let _active = crate::stack::GuestCallGuard::enter().unwrap();
            assert_eq!(admitted(&vm), 0, "no store may run after call-depth exhaustion");
        }
        assert_eq!(admitted(&vm), 1);
        vm.execution_interrupt.request_termination();
        assert_eq!(admitted(&vm), 0, "termination must precede the inlined store");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn typed_loop_reentry_keeps_prefix_effects_outside_inlined_frames() {
        let program = crate::Engine::specialize(
            "let mask=2;let proto={};proto.mark=function(){this.state=this.state|mask;};function walk(node){node.seen=node.seen+1;while(node.next!==null){node=node.next;node.mark();}return node;}",
            "inline-loop-entry.js").unwrap();
        let caller = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "walk")).unwrap();
        let targets = static_call_targets(&program);
        let unit = lane_unit::unit(&program, caller as u32, program.function(caller), &|function, pc| targets[function as usize]
            .get(pc as usize).copied().flatten()
            .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
            &|id| program.function_get(id as usize)).unwrap();
        assert_eq!(unit.frames.len(), 1);
        let isa = super::super::jit_backend::target_isa_for("speed",
            super::super::jit_backend::CodePublication::RelocatableObject).unwrap();
        let region = lower_static_typed_regions(&program, caller as u32, &targets[caller], &*isa, 0).into_iter().next()
            .expect("closed loop with an inlined store");
        assert!(region.header > 0, "prefix store cannot execute again on loop reentry");
        assert!(program.function(caller).instructions().any(|instruction|
            instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget
                && instruction.jump_target() as usize == region.header), "entry is an original source target");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn typed_field_chains_commit_the_sink_before_the_destination() {
        let program = crate::Engine::specialize(
            "function route(base){this.current=base.next.value;this.finish();return this.current;}",
            "fused-typed.js").unwrap();
        let function = program.function(1);
        assert!(function.instructions().any(|instruction| instruction.op() == Op::GetField
            && instruction.writes_current_this() && matches!(instruction.field_lookup(), Some(FieldLookup::Site(_)))),
            "fixture must exercise a fused read/read/sink instruction");
        let isa = super::super::jit_backend::target_isa_for("speed",
            super::super::jit_backend::CodePublication::RelocatableObject).unwrap();
        assert!(lower_static_typed_regions(&program, 1, &[], &*isa, 0).into_iter().next().is_some(),
            "fused field sites must use the shared typed region");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn function_bound_pure_predicates_share_argument_mapping_and_restartable_recovery() {
        let program = crate::Engine::specialize(
            "function make(mask){let o={};o.pureProbe=function(n){return (this.state&mask)===n;};return o;} function run(o,n){o.stamp=o.stamp+1;let r=o.pureProbe(n);if(r)return 11;return 12;}",
            "pure-binding.js").unwrap();
        let caller = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
        let targets = static_call_targets(&program);
        let unit = lane_unit::unit(&program, caller as u32, program.function(caller), &|function, pc| targets[function as usize]
            .get(pc as usize).copied().flatten()
            .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
            &|id| program.function_get(id as usize)).unwrap();
        assert_eq!(unit.frames.len(), 1);
        assert_eq!(unit.frames[0].arguments, 1);
        let isa = super::super::jit_backend::target_isa_for("speed",
            super::super::jit_backend::CodePublication::RelocatableObject).unwrap();
        assert!(lower_static_typed_regions(&program, caller as u32, &targets[caller], &*isa, 0).into_iter().next().is_some());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn static_function_bindings_inline_captures_at_the_call_edge_without_scratch_frames() {
        let program = crate::Engine::specialize(
            "function make(mask){let o={};o.uniqueMark=function(){this.state=this.state|mask;};return o;} function run(o){o.stamp=o.stamp+1;o.uniqueMark();return o.state;}",
            "function-binding.js").unwrap();
        let caller = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
        let targets = static_call_targets(&program);
        let unit = lane_unit::unit(&program, caller as u32, program.function(caller), &|function, pc| targets[function as usize]
            .get(pc as usize).copied().flatten()
            .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
            &|id| program.function_get(id as usize)).unwrap();
        assert_eq!(unit.frames.len(), 1, "the static method hint must reach the shared inliner");
        assert_eq!(unit.frames[0].target.callee, CalleeBinding::Function);
        let isa = super::super::jit_backend::target_isa_for("speed",
            super::super::jit_backend::CodePublication::RelocatableObject).unwrap();
        let region = lower_static_typed_regions(&program, caller as u32, &targets[caller], &*isa, 0).into_iter().next()
            .expect("restartable captured store inline region");
        let scratch_offset = std::mem::offset_of!(JsStepDispatch, inlined) as i32;
        assert!(!region.function.layout.blocks().flat_map(|block| region.function.layout.block_insts(block))
            .any(|instruction| matches!(region.function.dfg.insts[instruction],
                ir::InstructionData::Load { offset, .. } if i32::from(offset) == scratch_offset)),
            "caller-PC recovery needs no materialized callee scratch frame");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_field_probes_share_a_dominating_object_header() {
        let program = crate::Engine::specialize(
            "function sum(o){let a=o.left;let b=o.right;o.mark();return a+b;}",
            "shared-header.js").unwrap();
        let isa = super::super::jit_backend::target_isa_for("speed",
            super::super::jit_backend::CodePublication::RelocatableObject).unwrap();
        let region = lower_static_typed_regions(&program, 1, &[], &*isa, 0).into_iter().next().expect("typed field/call region");
        let families = region.function.layout.blocks()
            .flat_map(|block| region.function.layout.block_insts(block))
            .filter(|instruction| matches!(region.function.dfg.insts[*instruction],
                ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm }
                    if i64::from(imm) as u64 == crate::value::ValueLayout::DIRECT_OBJECT_TAG))
            .count();
        assert_eq!(families, 2,
            "one receiver-entry probe and one argument-object proof serve all three fields");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn typed_region_exits_before_terminal_calls_with_arguments() {
        let program = crate::Engine::specialize(
            "function pipe(object){object.seen=object.seen+1;return object.finish(3);}",
            "terminal-call.js").unwrap();
        let isa = super::super::jit_backend::target_isa_for("speed",
            super::super::jit_backend::CodePublication::RelocatableObject).unwrap();
        let body = program.function_get(1).unwrap();
        assert!(body.instructions().any(|instruction| instruction.op() == Op::Call
            && instruction.returns_from_frame() && instruction.call_window().count == 1));
        let region = lower_static_typed_regions(&program, 1, &[], &*isa, 0).into_iter().next().expect("typed prefix before terminal call");
        assert_eq!(region.header, 0);
        let nonterminal = crate::Engine::specialize(
            "function pipe(object){object.finish(3);return object.seen;}", "nonterminal-call.js").unwrap();
        assert!(lower_static_typed_regions(&nonterminal, 1, &[], &*isa, 0).into_iter().next().is_none(),
            "unsupported nonterminal calls still require a full effect/recovery boundary");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn guarded_calls_form_typed_regions_from_function_entry() {
        let program = crate::Engine::specialize(
            "let mask=2; function mark(){this.state=this.state|mask;} function pipe(object){object.mark();return object.state;}",
            "guarded-pipe.js").unwrap();
        let function = program.functions.iter().position(|function|
            function.header().name.is_some_and(|name| &program.atoms[name as usize] == "pipe")).unwrap();
        let isa = super::super::jit_backend::target_isa_for("speed",
            super::super::jit_backend::CodePublication::RelocatableObject).unwrap();
        let region = lower_static_typed_regions(&program, function as u32, &[], &*isa, 0).into_iter().next().expect("guarded call region");
        assert_eq!(region.header, 0, "call pipes start at the function's entry");
        let helper_offset = LaneHelper::GuardedCall as i32 * std::mem::size_of::<usize>() as i32;
        assert!(region.function.layout.blocks().flat_map(|block| region.function.layout.block_insts(block)).any(|instruction| {
            let ir::InstructionData::Load { offset, .. } = region.function.dfg.insts[instruction] else { return false };
            i32::from(offset) == helper_offset
        }), "the region must contain the shared no-frame call operation");
        let module = crate::lower_native_js_module(&program, "guarded_pipe").unwrap();
        let name = format!("guarded_pipe_function_{function}");
        let caller = module.functions.iter().find_map(|body| match body {
            super::super::native_object::NativeFunction::Definition { name: candidate, function } if *candidate == name => Some(function),
            _ => None,
        }).unwrap();
        assert!(caller.layout.blocks().flat_map(|block| caller.layout.block_insts(block)).any(|instruction|
            caller.dfg.insts[instruction].opcode() == ir::Opcode::CallIndirect));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_holder_read_does_not_require_receiver_arena_storage() {
        let source = "function read(o){return o.method;} function Box(){} Box.prototype.method=42; Box.prototype.pad=0; globalThis.box=new Box(); box.own=1; read(box);";
        let program = crate::Engine::specialize(source, "packed-holder.js").unwrap();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.execute(&program).unwrap();
        let atom = vm.lookup_atom("box").unwrap();
        let receiver = vm.own_property(vm.realm.globals, atom).unwrap();
        assert!(vm.heap.object(receiver).unwrap().inline_properties().is_some());
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        let function = program.functions.iter().position(|function|
            function.header().name.is_some_and(|name| &program.atoms[name as usize] == "read")).unwrap();
        let site = program.function(function).instructions().find_map(|instruction| match instruction.field_lookup() {
            Some(FieldLookup::Atom { cache_site, .. }) => Some(cache_site), _ => None,
        }).unwrap();
        let cache = vm.field_cache_entry(site);
        assert_ne!(cache.holder, super::super::NO_FIELD_HOLDER);
        assert_eq!(vm.record_field_entry_hit(cache, receiver), Some(Value::number(42.0)));
        let prototype = vm.heap.object(receiver).unwrap().proto();
        let holder = vm.heap.object(prototype).unwrap();
        assert_eq!(holder.exact_shape(), cache.holder);
        assert!(holder.extras_storage_start().is_some(), "fixture must exercise an arena-backed prototype");

        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 4]);
        signature.returns.push(AbiParam::new(types::I64));
        let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut native, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let binding = FieldSlotBinding {
            receiver: builder.ins().iconst(types::I32, i64::from(cache.receiver)),
            holder: builder.ins().iconst(types::I32, i64::from(cache.holder)),
            slot: FieldSlot::Bound(builder.ins().iconst(types::I64, i64::from(cache.slot))),
            records: parameters[0], values: parameters[1], length: parameters[2],
        };
        let miss = builder.create_block();
        let unused = builder.ins().iconst(types::I64, 0);
        let (address, _) = emit_js_cached_field_slot(&mut builder, unused, unused,
            JsFieldReceiver::Value(parameters[3]), site, true, miss, Some(binding), None, None);
        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
        builder.ins().return_(&[value]);
        builder.switch_to_block(miss);
        let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
        builder.ins().return_(&[deleted]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        let memory = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: the four inputs name this VM's live reservation, arena and
        // receiver. No guest call or collection occurs while the code runs.
        let run = unsafe { std::mem::transmute::<*mut u8,
            unsafe extern "C" fn(usize, *const Value, usize, u64) -> u64>(memory.pointer()) };
        let (values, length) = vm.heap.native_property_values();
        let records = vm.heap.object_record_base().unwrap() as usize;
        assert_eq!(Value::from_bits(unsafe { run(records, values, length, receiver.to_bits()) }), Value::number(42.0));
        // A receiver that has not proved the cached shape cannot read this slot.
        assert_eq!(unsafe { run(records, values, length, Value::NULL.to_bits()) }, Value::DELETED.to_bits());
        assert_eq!(unsafe { run(records, values, length, vm.realm.globals.to_bits()) }, Value::DELETED.to_bits());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn activation_admits_only_authoritative_this_and_dense_function_captures() {
        let mut vm = semantic_vm("throw 7;");
        let receiver = vm.object();
        vm.frames[0].this = receiver;
        vm.frames[0].fixed_this = false;
        JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, |activation, _| {
            assert!(activation.dispatch.fixed_this_slot.is_null());
        }).unwrap();
        vm.frames[0].fixed_this = true;
        let slots = crate::heap::EnvironmentSlots::from(vec![Value::number(7.0)]);
        vm.frames[0].capture_cache = super::super::CaptureCache {
            env: vm.frames[0].env, captured: vm.frames[0].captured,
            epoch: vm.heap.collection_epoch(), slots: &slots, root: None,
        };
        JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, |activation, vm| {
            assert_eq!(activation.dispatch.fixed_this_slot, &vm.frames[0].this as *const Value);
            assert!(!activation.dispatch.capture_values.is_null());
        }).unwrap();
        vm.frames[0].capture_cache.root = Some(Some(ProgramId::MAIN.raw()));
        JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, |activation, _| {
            assert!(activation.dispatch.capture_values.is_null());
        }).unwrap();
    }

    #[cfg(feature = "aot")]
    #[test]
    fn typed_native_loops_preserve_stores_in_native_only_and_unspecialized_execution() {
        let source = "function tail(node) { while(node.next!==null) { node.mark=node.mark+1; node=node.next; } return node; } const end={next:null,mark:0}; const head={next:end,mark:0}; for(let i=0;i<40;i++) { if(tail(head)!==end) throw 'identity'; } throw head.mark;";
        for specialized in [false, true] {
            let mut program = crate::Engine::specialize(source, "native-loop-store.js").unwrap();
            program.specialized = specialized;
            let native = crate::NativeJsProgram::<crate::test_host::TestHost>::compile_eager(program).unwrap();
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.execute_native_program(&native, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::number(40.0)));
        }
    }

    #[test]
    fn direct_boolean_bitwise_entry_guards_non_int32_numbers() {
        let program = crate::Engine::specialize(
            "function flags(value) { if ((value & 1) !== 0) return true; return false; }",
            "direct-bitwise-guard.js",
        ).unwrap();
        let function = program.functions.iter().position(|function| {
            function.header().name.or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "flags")
        }).unwrap() as u32;
        let artifact = super::super::js_compiled::compile(&program, function).unwrap();
        let compiled = CompiledJsFunction::from_artifact(artifact).unwrap();
        assert!(compiled.direct_callable());
        let CompiledJsEntry::Boolean(entry) = &compiled.entry else {
            panic!("boolean region should use the boolean entry ABI");
        };
        // SAFETY: the region's input is a guarded signed-int32 Number.
        let yes = unsafe { entry([1.0].as_ptr()) };
        let no = unsafe { entry([0.0].as_ptr()) };
        assert_eq!(yes & super::super::js_compiled::BOOLEAN_REGION_RESULT_MASK, 1);
        assert_eq!(no & super::super::js_compiled::BOOLEAN_REGION_RESULT_MASK, 0);
        let yes_pc = (yes >> super::super::js_compiled::BOOLEAN_REGION_RESULT_BITS) as usize;
        let no_pc = (no >> super::super::js_compiled::BOOLEAN_REGION_RESULT_BITS) as usize;
        assert_ne!(yes_pc, no_pc);
        for pc in [yes_pc, no_pc] {
            assert_eq!(program.function(function as usize).code[pc].as_wide().op(), Op::Return);
        }
        let interrupt = crate::ExecutionInterrupt::default();
        assert!(matches!(direct_call(&compiled, &[Value::number(1.0)], &interrupt),
            Some(CompiledJsCallResult::Value(Value::TRUE))));
        assert!(direct_call(&compiled, &[Value::number(1.5)], &interrupt).is_none());
        assert!(direct_call(&compiled, &[Value::number(4_294_967_297.0)], &interrupt).is_none());
    }

    #[test]
    fn numeric_result_signature_joins_typed_returns_and_preserves_float_values() {
        let integer = "function choose(n,x){n=n|0;x=x|0;if(n<0)return x|0;for(let i=0;i<n;i=(i+1)|0)x=(x+1)|0;return x|0;}";
        let mixed = "function choose(n,x){n=n|0;if(n<0)return x|0;return x*1;}";
        for typescript in [false, true] {
            for (source, format) in [(integer, NativeNumberResult::Int32), (mixed, NativeNumberResult::Float64)] {
                let source = if typescript { source.replace("(n,x)", "(n:number,x:number):number") } else { source.to_owned() };
                let program = if typescript { crate::Engine::specialize_typescript(&source, "result-signature.ts") }
                    else { crate::Engine::specialize(&source, "result-signature.js") }.unwrap();
                let artifact = compile_js_typed_numeric_region(&program, 1).expect("shared typed return region");
                assert!(matches!(artifact.result, super::super::js_compiled::JsJitResultKind::SharedNumberEntry(_, actual) if actual == format));
                let compiled = CompiledJsFunction::from_artifact(artifact).unwrap();
                let interrupt = crate::ExecutionInterrupt::default();
                for n in [-1, 0, 3] { for x in [i32::MIN, -1, 0, i32::MAX] {
                    let expected = if format == NativeNumberResult::Int32 && n >= 0 { x.wrapping_add(n) } else { x };
                    let result = direct_call(&compiled, &[Value::integer(n), Value::integer(x)], &interrupt);
                    assert!(matches!(result, Some(CompiledJsCallResult::Value(value)) if value.to_bits() == Value::integer(expected).to_bits()),
                        "{source}:n={n},x={x}");
                }}
                if format == NativeNumberResult::Float64 {
                    for x in [-0.0, 1.5, f64::MIN_POSITIVE, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                        let result = direct_call(&compiled, &[Value::integer(1), Value::number(x)], &interrupt);
                        assert!(matches!(result, Some(CompiledJsCallResult::Value(value)) if value.to_bits() == Value::number(x).to_bits()),
                            "mixed result preserves {x:?}");
                    }
                }
                for value in [Value::UNDEFINED, Value::TRUE, Value::NULL] {
                    assert!(direct_call(&compiled, &[value, Value::integer(1)], &interrupt).is_none(), "TS annotations are not input proofs");
                }
                interrupt.request_termination();
                assert!(direct_call(&compiled, &[Value::integer(1), Value::integer(1)], &interrupt).is_none());
            }
        }
    }

    #[test]
    fn numeric_jit_entry_uses_shared_typed_word32_loops() {
        let source = r#"
            function kernel(n,x){n=n|0;x=x|0;for(let i=0;i<n;i=(i+1)|0){x=x^(x<<13);x=x^(x>>>17);x=x^(x<<5);}return x|0;}
        "#;
        for program in [crate::Engine::specialize(source, "numeric-entry.js").unwrap(),
            crate::Engine::specialize_typescript(&source.replace("(n,x)", "(n:number,x:number):number"), "numeric-entry.cts").unwrap()] {
        let artifact = compile_js_typed_numeric_region(&program, 1).expect("shared typed numeric entry");
        let compiled = CompiledJsFunction::from_artifact(artifact).unwrap();
        assert!(matches!(compiled.native_entry_mode(), NativeEntryMode::Resumable));
        let interrupt = crate::ExecutionInterrupt::default();
        assert!(direct_call(&compiled, &[], &interrupt).is_none());
        assert!(direct_call(&compiled, &[Value::number(17.0)], &interrupt).is_none());
        assert!(compiled.call(&[17.0], &interrupt, None).is_none());
        assert!(matches!(direct_call(&compiled, &[Value::number(17.0), Value::number(0.0), Value::UNDEFINED], &interrupt),
            Some(CompiledJsCallResult::Value(value)) if value == Value::number(0.0)), "extra unused inputs are ignored");
        for value in [Value::UNDEFINED, Value::NULL, Value::TRUE] {
            assert!(direct_call(&compiled, &[value, Value::number(1.0)], &interrupt).is_none(), "annotations are not input proofs");
        }
        for count in [-1, 0, 1, 2, 17, 257] {
            for seed in [0, 1, 123456789, i32::MIN, i32::MAX] {
                let mut expected = seed as u32;
                for _ in 0..count.max(0) {
                    expected ^= expected << 13;
                    expected ^= expected >> 17;
                    expected ^= expected << 5;
                }
                let actual = direct_call(&compiled, &[Value::number(f64::from(count)), Value::number(f64::from(seed))], &interrupt);
                assert!(matches!(actual, Some(CompiledJsCallResult::Value(value)) if value == Value::number(f64::from(expected as i32))),
                    "n={count} seed={seed}");
            }
        }
        for value in [1.5, -1.5, -0.0, 4294967295.0, 4294967297.0] {
            let mut expected = 1_u32;
            for _ in 0..(crate::value::number_to_u32(value) as i32) {
                expected ^= expected << 13;
                expected ^= expected >> 17;
                expected ^= expected << 5;
            }
            assert!(matches!(direct_call(&compiled, &[Value::number(value), Value::integer(1)], &interrupt),
                Some(CompiledJsCallResult::Value(actual)) if actual == Value::integer(expected as i32)),
                "explicit ToInt32 accepts {value:?}");
        }
        for value in [f64::NAN, f64::INFINITY, crate::value::EXACT_I64_TRUNCATION_LIMIT] {
            assert!(direct_call(&compiled, &[Value::number(value), Value::number(1.0)], &interrupt).is_none(),
                "unsupported ToInt32 input must decline exactly");
        }
        let inputs = [Value::integer(257), Value::integer(123456789)];
        let Some(CompiledJsCallResult::Value(expected)) = direct_call(&compiled, &inputs, &interrupt) else { panic!("private result") };
        let shared = interrupt.clone();
        assert!(matches!(direct_call(&compiled, &inputs, &interrupt), Some(CompiledJsCallResult::Value(value)) if value == expected));
        drop(shared);
        assert!(matches!(direct_call(&compiled, &inputs, &interrupt), Some(CompiledJsCallResult::Value(value)) if value == expected));
        let replacement = crate::ExecutionInterrupt::default();
        assert!(matches!(direct_call(&compiled, &inputs, &replacement), Some(CompiledJsCallResult::Value(value)) if value == expected));
        replacement.request_termination();
        assert!(direct_call(&compiled, &inputs, &replacement).is_none());
        let effectful = crate::Engine::specialize("function f(n,o){for(let i=0;i<n;i++)o.x++;return n;}", "numeric-effects.js").unwrap();
        assert!(compile_js_typed_numeric_region(&effectful, 1).is_none());
        let patterned = crate::Engine::specialize("function f({n}){let sum=0;for(let i=0;i<n;i++)sum++;return sum;}", "numeric-pattern.js").unwrap();
        assert!(compile_js_typed_numeric_region(&patterned, 1).is_none(), "scalar ABI requires the verified parameter mapping");
        for returned in ["42", "n"] {
            let source = format!("function f(n){{for(let i=0;i<n;i++){{}}return {returned};}}");
            let program = crate::Engine::specialize(&source, "numeric-terminal.js").unwrap();
            let artifact = super::super::js_compiled::compile(&program, 1).expect("existing numeric return coverage");
            let native = CompiledJsFunction::from_artifact(artifact).unwrap();
            let expected = if returned == "42" { 42.0 } else { 7.0 };
            assert!(matches!(direct_call(&native, &[Value::number(7.0)], &interrupt),
                Some(CompiledJsCallResult::Value(value)) if value == Value::number(expected)),
                "unsupported fused terminals must retain an executable fallback");
        }
        interrupt.request_termination();
        assert!(direct_call(&compiled, &[Value::number(17.0), Value::number(1.0)], &interrupt).is_none());
        }
    }

    #[cfg(all(feature = "native-execution", not(any(feature = "jit", feature = "aot"))))]
    #[test]
    fn static_lane_call_helper_declines_without_dynamic_context() {
        // Execution-only images lend no interpreter lane, so even a null context
        // must decline before inspecting it or publishing a dynamic continuation.
        let status = unsafe { super::super::dispatch_fast::lane_body_call::<crate::test_host::TestHost>(0, 0, 0, 0) };
        assert_eq!(status, LANE_CALL_MISSED);
    }

    #[test]
    fn nested_lane_entry_restores_outer_fault_binding() {
        if !FAULT_POLL_SUPPORTED { return; }
        let program = crate::Engine::specialize(
            "function count(n){n=n|0;let sum=0;for(let i=0;i<n;i=(i+1)|0)sum=(sum+1)|0;return sum;}",
            "nested-poll-binding.js").unwrap();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let method = vm.compile_lane_method(&program, 1, &|_, _| Vec::new(),
            &|_| (0, 0), Vec::new(), None).expect("polling lane body");
        assert!(method.fault_poll);
        let frame = lane_frame();
        // The real outer landing must survive an inner trampoline overwriting it.
        // This sentinel is never entered: the inner entry installs its own landing.
        let outer_landing = (&program as *const ResidualProgram).addr();
        let original_landing = unsafe { std::mem::replace(&mut (*frame).context.entry_stack, outer_landing) };
        let before = unsafe {
            let context = &(*frame).context;
            (context.stack_limit, context.entry_stack, context.interrupt,
                context.adapter_payload, context.poll_start, context.poll_end, context.interrupt_owner)
        };
        let mut registers = vec![Value::number(7.0); usize::from(method.caller_registers)];
        let result = vm.run_nested_lane_method(&method, ProgramId::MAIN, &program,
            registers.as_mut_ptr(), Value::DELETED, std::ptr::null(), std::ptr::null_mut());
        assert!(matches!(result, LaneExit::Returned { value, .. } if value == Value::number(7.0)));
        let after = unsafe {
            let context = &(*frame).context;
            (context.stack_limit, context.entry_stack, context.interrupt,
                context.adapter_payload, context.poll_start, context.poll_end, context.interrupt_owner)
        };
        assert_eq!(before, after, "outer native recovery binding is borrowed, not replaced");
        unsafe { (*frame).context.entry_stack = original_landing; }

        unsafe extern "C" fn outer(context: *mut CallContext, _: u64, _: u64,
            arguments: *const u64, out: *mut u64) -> u32 {
            // Only borrowed raw inputs live across the poll: recovery skips no destructor.
            let inputs = unsafe { std::slice::from_raw_parts(arguments, 4) };
            let vm = unsafe { &mut *(inputs[0] as *mut Vm<crate::test_host::TestHost>) };
            let method = unsafe { &*(inputs[1] as *const LaneMethod) };
            let program = unsafe { &*(inputs[2] as *const ResidualProgram) };
            let registers = inputs[3] as *mut Value;
            let result = vm.run_nested_lane_method(method, ProgramId::MAIN, program,
                registers, Value::DELETED, std::ptr::null(), std::ptr::null_mut());
            let LaneExit::Returned { value, .. } = result else { return NATIVE_JS_STOPPED; };
            unsafe { out.write(value.to_bits()); }
            vm.execution_interrupt.request_termination();
            // The restored outer landing must own this fault, after the inner landing died.
            let poll = unsafe { (*context).poll_start as *const std::sync::atomic::AtomicBool };
            unsafe { (*poll).load(std::sync::atomic::Ordering::Acquire); }
            0
        }
        let inputs = [&mut vm as *mut _ as u64, &method as *const _ as u64,
            &program as *const _ as u64, registers.as_mut_ptr() as u64];
        let page = native_poll_page(&vm.execution_interrupt).expect("owned poll page");
        let context = unsafe { &mut (*frame).context };
        context.bind_native_adapter(context.adapter_payload,
            vm.execution_interrupt.atomic_flag(), Some(page));
        let mut out = 0;
        let status = unsafe { enter_native(context, 0, 0, inputs.as_ptr(), &mut out,
            NativeEntryMode::Resumable, outer) };
        assert_eq!(i64::from(status), TERMINATED_STATUS);
        assert_eq!(Value::from_bits(out), Value::number(7.0));
    }

    #[test]
    fn numeric_jit_shared_entry_observes_live_termination() {
        use std::{sync::mpsc, thread, time::Duration};
        const TIMEOUT: Duration = Duration::from_secs(5);
        fn started(context: &mut crate::NativeContext<'_, crate::test_host::TestHost>,
            _: crate::RootId, _: &[crate::RootId]) -> Result<crate::RootId, crate::RootedError> {
            context.host_mut().state::<mpsc::Sender<()>>().send(()).unwrap();
            Ok(context.undefined())
        }
        let (ready, entered) = mpsc::channel::<()>();
        let (send_handle, receive_handle) = mpsc::channel();
        let (complete, finished) = mpsc::channel();
        let worker = thread::Builder::new().stack_size(crate::WORKER_STACK_SIZE).spawn(move || {
            let program = crate::Engine::specialize(r#"
                function spin(n){let x=1;for(let i=0;i<n;i=(i+1)|0)x=x^(x<<13);return x;}
                for(let warm=0;warm<64;warm++)spin(1);
                started();spin(Infinity);
            "#, "numeric-jit-interrupt.js").unwrap();
            let host = crate::test_host::TestHost::default()
                .with_natives(crate::host_functions![global "started" (0) => started]).with_state(Rc::new(ready));
            let mut vm = Vm::new(host);
            vm.set_execution_mode(ExecutionMode::Compiled);
            send_handle.send(vm.execution_interrupt.clone()).unwrap();
            let budget = crate::stack::GuestCallGuard::remaining();
            let error = vm.execute(&program).unwrap_err();
            assert!(error.is_execution_terminated(), "{error}");
            assert_eq!(crate::stack::GuestCallGuard::remaining(), budget);
            let Some(super::super::program_store::JsTierState::Compiled(native)) =
                vm.programs.js_tier(ProgramId::MAIN, 1) else { panic!("numeric function must compile"); };
            assert!(matches!(native.entry, CompiledJsEntry::SharedNumber { mode: NativeEntryMode::Resumable, .. }));
            #[cfg(not(quench_profiled))]
            assert!(native.native_entries() > 0);
            complete.send(()).unwrap();
        }).unwrap();
        let handle = receive_handle.recv_timeout(TIMEOUT).unwrap();
        entered.recv_timeout(TIMEOUT).unwrap();
        thread::sleep(Duration::from_millis(20));
        handle.request_termination();
        finished.recv_timeout(TIMEOUT).expect("shared numeric JIT loop must observe live cancellation");
        worker.join().unwrap();
    }

    #[test]
    fn scalar_core_binds_poll_pointer_before_loop_but_keeps_atomic_reads() {
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        let signature = native_entry_signature(isa.pointer_type(), isa.default_call_conv());
        let mut native = ir::Function::with_name_signature(UserFuncName::testcase("bound_poll"), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut native, &mut context);
        let entry = builder.create_block();
        let header = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let activation = builder.block_params(entry)[0];
        // The old address establishes fault-poll availability, not executable identity.
        let interrupt = crate::ExecutionInterrupt::default();
        let Some(address) = poll_address(&interrupt) else { return };
        let mut core = RegionCore::new(builder, activation, Some(address));
        core.ins().jump(header, &[]);
        core.switch_to_block(header);
        core.interrupt_safepoint();
        core.interrupt_safepoint();
        core.ins().jump(header, &[]);
        core.seal_all_blocks();
        core.into_builder().finalize(isa.frontend_config());
        cranelift_codegen::verify_function(&native, &*isa).unwrap();
        let owners: Vec<_> = native.layout.block_insts(entry).filter_map(|inst| {
            matches!(native.dfg.insts[inst], ir::InstructionData::Load { arg, offset, .. }
                if arg == activation && i32::from(offset) == std::mem::offset_of!(CallContext, poll_start) as i32)
                .then(|| native.dfg.inst_results(inst)[0])
        }).collect();
        assert_eq!(owners.len(), 1, "bind the activation's pointer once before the loop");
        let polls: Vec<_> = native.layout.block_insts(header).filter(|&inst|
            native.dfg.insts[inst].opcode() == ir::Opcode::AtomicLoad).collect();
        assert_eq!(polls.len(), 2, "poll reads must remain separate observable operations");
        for inst in polls { assert_eq!(native.dfg.inst_args(inst), &[owners[0]]); }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn guarded_boolean_region_emits_a_relocatable_object() {
        let program = crate::Engine::specialize(
            "function make(mask, expected) { function check() { return (this.value & mask) !== 0 || this.value == expected; } return check; }",
            "object-boolean-region.js",
        ).unwrap();
        let id = program.functions.iter().position(|function| {
            function.header().name.or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "check")
        }).unwrap() as u32;
        let isa = super::super::jit_backend::target_isa_for(
            "speed", super::super::jit_backend::CodePublication::RelocatableObject,
        ).unwrap();
        let region = super::super::js_compiled::lower_number_domain_boolean_tree(
            &program, id, program.function(id as usize), &*isa,
        ).unwrap();
        assert!(region.requires_signed_int32_arguments);
        assert_eq!(region.capture_arguments.len(), 2);
        assert!(!region.numeric_field_inputs.is_empty());
        let object = super::super::native_object::emit_native_object(
            "boolean_region", vec![super::super::native_object::NativeFunction::Definition {
                name: "quench_boolean_region_test".into(), function: region.function,
            }],
        ).unwrap();
        assert!(!object.is_empty());
    }

    #[cfg(feature = "aot")]
    fn semantic_vm(source: &str) -> Vm<crate::test_host::TestHost> {
        let program = crate::Engine::specialize(source, "semantic-entry.js").unwrap();
        semantic_program(program)
    }

    #[cfg(feature = "aot")]
    fn semantic_program(program: ResidualProgram) -> Vm<crate::test_host::TestHost> {
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.initialize_for_snapshot(&program).unwrap();
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        let body = program.function(0);
        let mut frame = Box::new(Frame::empty(ProgramId::MAIN, 0));
        frame.registers.resize(usize::from(body.registers), Value::UNDEFINED);
        frame.locals.resize(usize::from(body.locals), Value::UNDEFINED);
        frame.initialized_registers = frame.registers.len();
        vm.frames.push(frame);
        vm.active_registers = vm.frames[0].registers.as_mut_ptr();
        vm
    }

    #[cfg(feature = "aot")]
    fn semantic_instruction(vm: &Vm<crate::test_host::TestHost>, opcode: Op) -> (usize, WideInstruction) {
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        let body = program.function(0);
        body.code.iter().copied().enumerate().find_map(|(pc, packed)| {
            let instruction = super::super::dispatch_fast::decoded_instruction(body, packed);
            (instruction.op() == opcode).then_some((pc, instruction))
        }).expect("test operation exists")
    }

    #[cfg(feature = "aot")]
    fn invoke_semantic(
        vm: &mut Vm<crate::test_host::TestHost>, pc: usize,
        entry: unsafe extern "C" fn(*mut std::ffi::c_void, u64, *const u64, *mut u64) -> i64,
    ) -> (i64, u64, Option<JsNativeStep>) {
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        let body = program.function(0);
        let instruction = super::super::dispatch_fast::decoded_instruction(body, body.code[pc]);
        let operands = super::super::NativeJsOperands::new(
            instruction.a(), instruction.b(), instruction.c(), instruction.imm(),
        );
        with_semantic_context(vm, pc, |context, result| {
            // SAFETY: the context owner roots the frame; operands and result live through this call.
            unsafe { entry(context.cast(), pc as u64, operands.as_ptr(), result) }
        })
    }

    #[cfg(feature = "aot")]
    fn with_semantic_context(
        vm: &mut Vm<crate::test_host::TestHost>, pc: usize,
        run: impl FnOnce(*mut CallContext, *mut u64) -> i64,
    ) -> (i64, u64, Option<JsNativeStep>) {
        JsNativeActivation::with(vm, ProgramId::MAIN, 0, pc, false, |activation, vm| {
            let dispatch = &mut activation.dispatch;
            let mut context = CallContext::native_adapter(
                (dispatch as *mut JsStepDispatch).cast(),
                vm.execution_interrupt.atomic_flag(), None,
            );
            let vm = vm as *mut Vm<crate::test_host::TestHost>;
            context.vm = vm.cast();
            dispatch.bind_property_buffer(vm);
            context.program = ProgramId::MAIN;
            let mut result = 0;
            let status = run(&mut context, &mut result);
            (status, result, dispatch.outcome.take())
        }).unwrap()
    }

    #[cfg(feature = "aot")]
    fn live_native_graph(vm: &Vm<crate::test_host::TestHost>) -> (CodeMemory, Entry, Vec<super::super::specialize::InstructionSite>) {
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        live_native_function(&program, 0, None)
    }

    #[cfg(feature = "aot")]
    fn partitioned_native_graph(vm: &Vm<crate::test_host::TestHost>) -> (crate::PreparedScript, Entry) {
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        let options = crate::NativeJsCodegenOptions { region_instruction_budget: std::num::NonZeroUsize::new(1).unwrap(), backend: crate::NativeJsBackend::Cranelift };
        let native = crate::NativeJsProgram::compile_eager_with_options((*program).clone(), options).unwrap();
        let prepared = vm.prepare_native_script(native).unwrap();
        let entry = prepared.native.as_ref().unwrap().entries()[0];
        // SAFETY: only the opaque context pointer type differs; the entry is
        // compiled for this exact program and H, and prepared owns all regions.
        let entry = unsafe { std::mem::transmute::<crate::NativeJsFunctionEntry, Entry>(entry) };
        (prepared, entry)
    }

    #[cfg(feature = "aot")]
    pub(super) fn live_native_function(program: &ResidualProgram, function_id: u32, entries: Option<*mut u64>)
        -> (CodeMemory, Entry, Vec<super::super::specialize::InstructionSite>)
    {
        let sites = super::super::specialize::instruction_sites(super::super::specialize::Division::Offline, program.function(function_id as usize));
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        let mut signature = None;
        let native = super::super::native_graph::lower_body(&sites, function_id, &*isa, None,
            |builder, site, context, result| {
                if site.pc == 0 && let Some(entries) = entries {
                    let pointer = builder.ins().iconst(isa.pointer_type(), entries as i64);
                    let previous = builder.ins().load(types::I64, MemFlagsData::trusted(), pointer, 0);
                    let count = builder.ins().iadd_imm_s(previous, 1);
                    builder.ins().store(MemFlagsData::trusted(), count, pointer, 0);
                }
                let signature = *signature.get_or_insert_with(|| builder.import_signature(native_js_operation_signature(&*isa)));
                let target = site.op.native_semantic_entry::<crate::test_host::TestHost>() as usize;
                let target = builder.ins().iconst(isa.pointer_type(), target as i64);
                let arguments = builder.ins().iconst(isa.pointer_type(), site.operands.as_ptr() as i64);
                let pc = builder.ins().iconst(types::I64, site.pc as i64);
                let call = builder.ins().call_indirect(signature, target, &[context, pc, arguments, result]);
                builder.inst_results(call)[0]
            }).unwrap();
        // No code size budget is needed by this focused publication check; the
        // production object backend handles relocations instead of live addresses.
        let (memory, entry) = compile_and_publish_entry(native, &*isa, usize::MAX)
            .expect("native semantic graph compiles");
        (memory, entry, sites)
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_activation_owns_metadata_without_retaining_it_between_entries() {
        let vm_source = "throw 7;";
        let mut first_vm = semantic_vm(vm_source);
        let mut second_vm = semantic_vm(vm_source);
        let weak = JsNativeActivation::with(&mut first_vm, ProgramId::MAIN, 0, 0, false, |first, _| {
            JsNativeActivation::with(&mut second_vm, ProgramId::MAIN, 0, 0, false, |second, _| {
                assert_eq!(first.dispatch.program_id, second.dispatch.program_id);
                assert!(!Rc::ptr_eq(&first._program, &second._program));
                assert_eq!(first.dispatch.residual, Rc::as_ptr(&first._program) as usize);
                assert_eq!(first.dispatch.body, first._program.function(0) as *const _);
                assert_ne!(first.dispatch.body, second.dispatch.body);
                let weak = Rc::downgrade(&first._program);
                let mut program = first._program.clone();
                assert!(Rc::get_mut(&mut program).is_none(), "admitted records cannot be uniquely released");
                weak
            }).unwrap()
        }).unwrap();
        // Only the VM owns the program after the native scope returns.
        assert_eq!(weak.strong_count(), 1);
        drop(first_vm);
        assert!(weak.upgrade().is_none());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_entry_panic_transport_does_not_poison_the_next_entry() {
        unsafe extern "C" fn report_panic(
            context: *mut CallContext, _: u64, _: u64, _: *const u64, _: *mut u64,
        ) -> u32 {
            // Model a panic already caught by the semantic callback's FFI boundary.
            unsafe { (*context).raised = Some(Raised::Panic(Box::new("native panic"))) };
            u32::try_from(RAISED_STATUS).unwrap()
        }
        let mut vm = semantic_vm("throw 7;");
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, |activation, vm| {
                let dispatch = &mut activation.dispatch;
                let interrupt = vm.execution_interrupt.atomic_flag() as *const _;
                run_js_native_entry(report_panic, vm, ProgramId::MAIN,
                    (dispatch as *mut JsStepDispatch).cast(), interrupt, None, &[])
            })
        })).unwrap_err();
        assert_eq!(panic.downcast_ref::<&str>(), Some(&"native panic"));

        let (_memory, entry, _operands) = live_native_graph(&vm);
        JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, |activation, vm| {
            let dispatch = &mut activation.dispatch;
            let interrupt = vm.execution_interrupt.atomic_flag() as *const _;
            let (status, _) = run_js_native_entry(entry, vm, ProgramId::MAIN,
                (dispatch as *mut JsStepDispatch).cast(), interrupt, None, &[]).unwrap();
            assert_eq!(status, NATIVE_JS_STOPPED);
            let Err(error) = dispatch.outcome.take().unwrap().result else { panic!("expected guest throw") };
            assert_eq!(error.thrown_value(), Some(Value::number(7.0)));
        }).unwrap();
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_semantic_graph_runs_root_branches_and_loops() {
        let mut vm = semantic_vm("globalThis.count = 0; while (globalThis.count < 4) { globalThis.count++; } throw globalThis.count;");
        let (_memory, entry, _operands) = live_native_graph(&vm);
        let (status, _, outcome) = with_semantic_context(&mut vm, 0, |context, result| {
            // SAFETY: the shared context, native code and operand data outlive this entry.
            unsafe { i64::from(entry(context, 0, 0, std::ptr::null(), result)) }
        });
        assert_eq!(status, i64::from(NATIVE_JS_STOPPED));
        let Err(error) = outcome.expect("graph stops at throw").result else { panic!("expected throw") };
        assert_eq!(error.thrown_value(), Some(Value::number(4.0)));
    }

    #[cfg(feature = "aot")]
    fn linked_test_program(source: &str, typescript: bool) -> (
        crate::NativeJsProgram<crate::test_host::TestHost>, Rc<[std::cell::Cell<u64>]>,
        std::rc::Weak<dyn std::any::Any>,
    ) {
        let program = if typescript {
            crate::Engine::specialize_typescript(source, "linked.cts")
        } else {
            crate::Engine::specialize(source, "linked.cjs")
        }.unwrap();
        linked_test_residual(program)
    }

    #[cfg(feature = "aot")]
    fn linked_test_residual(program: crate::ResidualProgram) -> (
        crate::NativeJsProgram<crate::test_host::TestHost>, Rc<[std::cell::Cell<u64>]>,
        std::rc::Weak<dyn std::any::Any>,
    ) {
        let counts: Rc<[std::cell::Cell<u64>]> = (0..program.function_count())
            .map(|_| std::cell::Cell::new(0)).collect();
        let mut owners = Vec::new();
        let mut entries = Vec::new();
        for id in 0..program.function_count() {
            let (memory, entry, operands) = live_native_function(&program, id as u32, Some(counts[id].as_ptr()));
            // SAFETY: the public and internal native entries differ only in the
            // Rust spelling of their opaque first pointer parameter.
            entries.push(unsafe { std::mem::transmute::<Entry, crate::NativeJsFunctionEntry>(entry) });
            owners.push((memory, operands));
        }
        let owner: Rc<dyn std::any::Any> = Rc::new((owners, counts.clone()));
        let weak = Rc::downgrade(&owner);
        // SAFETY: every function was lowered from this program for TestHost;
        // owner retains the code, operands and counter storage it addresses.
        let native = unsafe { crate::NativeJsProgram::from_entries(program, entries, owner) }.unwrap();
        (native, counts, weak)
    }

    #[cfg(feature = "aot")]
    unsafe extern "C" fn fixed_test_semantic(
        context: *mut CallContext, pc: u64, _: *const u64, result: *mut u64,
    ) -> i64 {
        // SAFETY: the test activation retains this body and the native graph
        // checks its PC before selecting a fixed semantic operation.
        let instruction = unsafe {
            let dispatch = &*(*context).adapter_payload.cast::<JsStepDispatch>();
            let body = &*dispatch.body;
            super::super::dispatch_fast::decoded_instruction(body, body.code[pc as usize])
        };
        let operands = super::super::native_operands::NativeJsOperands::new(
            instruction.a(), instruction.b(), instruction.c(), instruction.imm());
        // The adapter supplies live addresses where production object emission
        // supplies linker relocations; both select the same semantic entries.
        unsafe { (instruction.op().native_semantic_entry::<crate::test_host::TestHost>())(
            context.cast(), pc, operands.as_ptr(), result) }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_general_entry_divides_stop_control_once_and_preserves_generator_parameters() {
        let sources = [
            "let trace=''; function* values(x=(trace+='p',1)) { trace+='b'; yield x; trace+='r'; return 2; } const g=values(); if(trace!=='p') throw 'early body'; const first=g.next(); const last=g.next(); if(first.value!==1||first.done||last.value!==2||!last.done) throw 'bad yield'; throw trace;",
            "let trace=''; function fail(){trace+='p';throw 7;} function* values(x=fail()){trace+='bad';yield x;} try{values();throw 'missed parameter error';}catch(e){if(e!==7)throw e;} throw trace;",
        ];
        for typescript in [false, true] {
            for (source, expected) in sources.iter().zip(["pbr", "p"]) {
                let program = if typescript { crate::Engine::specialize_typescript(source, "bounded.cts") }
                    else { crate::Engine::specialize(source, "bounded.cjs") }.unwrap();
                let isa = super::super::jit_backend::target_isa("speed").unwrap();
                let mut owners = Vec::new();
                let mut entries = Vec::new();
                for id in 0..program.function_count() {
                    let (memory, entry, operands) = if id == 0 {
                        live_native_function(&program, id as u32, None)
                    } else {
                        let (native, _) = lower_js_general_dispatch(&program, id as u32, false, &*isa,
                            JsGeneralCallout::Address(fixed_test_semantic as *const () as usize), &[], &[]).unwrap();
                        let entry_block = native.layout.entry_block().unwrap();
                        let context = native.dfg.block_params(entry_block)[0];
                        let instructions: Vec<_> = native.layout.blocks().flat_map(|block| native.layout.block_insts(block)).collect();
                        let payload = instructions.iter().find_map(|inst| match native.dfg.insts[*inst] {
                            ir::InstructionData::Load { arg, offset, .. }
                                if native.dfg.resolve_aliases(arg) == context
                                    && i32::from(offset) == std::mem::offset_of!(CallContext, adapter_payload) as i32 =>
                                Some(native.dfg.first_result(*inst)),
                            _ => None,
                        }).unwrap();
                        assert_eq!(instructions.iter().filter(|inst| matches!(native.dfg.insts[**inst],
                            ir::InstructionData::Load { arg, offset, .. }
                                if native.dfg.resolve_aliases(arg) == payload
                                    && i32::from(offset) == std::mem::offset_of!(JsStepDispatch, stop_pc) as i32)).count(), 1,
                            "the activation stop fact is read once, never per instruction");
                        let (memory, entry) = compile_and_publish_entry(native, &*isa, usize::MAX).unwrap();
                        (memory, entry, Vec::new())
                    };
                    // SAFETY: only the spelling of the opaque context differs.
                    entries.push(unsafe { std::mem::transmute::<Entry, crate::NativeJsFunctionEntry>(entry) });
                    owners.push((memory, operands));
                }
                let owner: Rc<dyn std::any::Any> = Rc::new(owners);
                // SAFETY: this owner retains every entry lowered for this exact program/host.
                let native = unsafe { crate::NativeJsProgram::from_entries(program, entries, owner) }.unwrap();
                let mut vm = Vm::new(crate::test_host::TestHost::default());
                vm.set_execution_mode(ExecutionMode::NativeOnly);
                let error = vm.execute_native_program(&native, true).unwrap_err();
                assert_eq!(vm.heap.string(error.thrown_value().unwrap()).unwrap().to_string(), expected);
            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_global_scripts_preserve_lexicals_completion_and_invocation_names() {
        let mut compiler = crate::NativeJsCompiler::new(true);
        let entry = compiler.compile_script("", "seed.js", Default::default()).unwrap();
        let sources = ["const frozenLexical = 41; frozenLexical + 1;", "frozenLexical;", "/* empty */"];
        let mut counters = Vec::new();
        let artifacts = sources.into_iter().map(|source| {
            let key = crate::NativeSourceKey::global_script(true, source);
            let residual = compiler.compile_source(&key).unwrap();
            let (native, counts, _) = linked_test_residual(residual);
            counters.push(counts);
            (key, native)
        }).collect::<Vec<_>>();
        let entry = compiler.finish(entry).unwrap();
        let host = crate::test_host::TestHost::default().with_native_programs(move |name, parts| {
            Ok(artifacts.iter().find(|(key, _)| key.name() == name && key.parts().eq(parts.iter().copied()))
                .map(|(_, native)| native.clone()))
        });
        let mut vm = Vm::new(host);
        vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let context = vm.embedding_program().unwrap();
        assert_eq!(vm.eval_script_source(&context, sources[0], "first-invocation.js").unwrap(), Value::number(42.0));
        let first_id = super::super::ProgramId::from_raw(u32::try_from(vm.programs.len() - 1).unwrap());
        assert_eq!(vm.programs.get(first_id).unwrap().source_name, "first-invocation.js");
        assert_eq!(vm.eval_script_source(&context, sources[1], "second-invocation.js").unwrap(), Value::number(41.0));
        assert_eq!(counters[0][0].get(), 1);
        assert_eq!(counters[1][0].get(), 1);
        let last = vm.programs.len();
        assert_eq!(vm.eval_script_source(&context, sources[2], "empty.js").unwrap(), Value::UNDEFINED);
        assert_eq!(vm.programs.len(), last);
        let error = vm.eval_script_source(&context, "let unknown = ;", "unknown.js").unwrap_err();
        assert!(error.to_string().contains("source compilation is unavailable"), "{error}");
        assert!(vm.lookup_atom("unknown").is_none());
        // The same linked declaration still performs per-invocation instantiation.
        let error = vm.eval_script_source(&context, sources[0], "duplicate.js").unwrap_err();
        assert!(error.to_string().contains("SyntaxError"), "{error}");
        assert_eq!(counters[0][0].get(), 1);
        let mut context = crate::NativeContext::new(&mut vm);
        let realm = context.create_realm_rooted().unwrap();
        let result = context.evaluate_script_in_realm_rooted(realm, sources[0], "foreign-realm.js").unwrap();
        let expected = context.number(42.0);
        assert!(context.strict_equal_rooted(result, expected).unwrap());
        assert_eq!(counters[0][0].get(), 2);
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_global_scripts_defer_syntax_failures_and_reject_module_code() {
        for module in [false, true] {
            let mut compiler = crate::NativeJsCompiler::new(true);
            let entry = compiler.compile_script("", "seed.js", Default::default()).unwrap();
            let source = "let invalid = ;";
            let native = module.then(|| linked_test_residual(compiler.compile_module("export {};", "module.js").unwrap()).0);
            let entry = compiler.finish(entry).unwrap();
            let host = crate::test_host::TestHost::default().with_native_programs(move |name, parts| {
                assert_eq!(name, crate::GLOBAL_SCRIPT_SOURCE_NAME);
                assert_eq!(parts, ["specialized", crate::GLOBAL_SCRIPT_GOAL, source]);
                match &native {
                    Some(native) => Ok(Some(native.clone())),
                    None => Err(crate::NativeSourceError::Syntax("deferred script syntax".into())),
                }
            });
            let mut vm = Vm::new(host);
            vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let context = vm.embedding_program().unwrap();
            let error = vm.eval_script_source(&context, source, "invocation.js").unwrap_err();
            let expected = if module { "different parse goal" } else { "SyntaxError: deferred script syntax" };
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_host_scripts_preserve_code_through_preparation_and_compiled_handles() {
        for typescript in [false, true] {
            let mut compiler = crate::NativeJsCompiler::new(true);
            let entry = compiler.compile_script("", "seed.js", crate::HostScriptOptions::default()).unwrap();
            let source = if typescript {
                "function answer(value: unknown): unknown { return (value as number) + 1; } answer('x');"
            } else {
                "function answer(value) { return value + 1; } answer('x');"
            };
            let options = crate::HostScriptOptions { typescript, ..Default::default() };
            let residual = compiler.compile_script(source, "library", options).unwrap();
            let entry = compiler.finish(entry).unwrap();
            let (native, counts, weak) = linked_test_residual(residual);
            let goal = options.cache_goal();
            let host = crate::test_host::TestHost::default().with_native_programs(move |name, parts| {
                assert_eq!(name, "library");
                assert_eq!(parts, ["specialized", &goal, source]);
                Ok(Some(native.clone()))
            });
            let mut vm = Vm::new(host);
            vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
            let prepared = vm.prepare_embedding_specialized_script_retaining(
                source, "library", None, None, false, typescript,
            ).unwrap();
            for _ in 0..2 {
                let result = vm.evaluate_embedding_prepared_script(&prepared).unwrap();
                assert_eq!(vm.heap.string(result).unwrap().to_string(), "x1");
            }
            assert!(counts.iter().all(|count| count.get() > 0));
            assert!(weak.upgrade().is_some());
            if !typescript {
                let compiled = vm.compile_embedding_host_script(source, "library", (0, 0)).unwrap();
                let before = counts[0].get();
                for _ in 0..2 {
                    let result = vm.evaluate_embedding_compiled_script(&compiled).unwrap();
                    assert_eq!(vm.heap.string(result).unwrap().to_string(), "x1");
                }
                assert!(counts[0].get() > before);
                let mut context = crate::NativeContext::new(&mut vm);
                let first = context.create_realm_rooted().unwrap();
                let second = context.create_realm_rooted().unwrap();
                for global in [first, first, second] {
                    let result = context.evaluate_compiled_script_in_realm_rooted(global, &compiled).unwrap();
                    let expected = context.string_rooted("x1");
                    assert!(context.strict_equal_rooted(result, expected).unwrap());
                }

            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_host_wrappers_and_bootstrap_use_exact_source_keys() {
        for bootstrap in [false, true] {
            let mut compiler = crate::NativeJsCompiler::new(true);
            let entry = compiler.compile_script("", "seed.js", Default::default()).unwrap();
            let prefix = "(function() {\n";
            let body = "return 42;";
            let suffix = "\n})();";
            let source = "globalThis.bootResult = 42;";
            let residual = if bootstrap {
                compiler.compile_host_script(source, "unit").unwrap()
            } else {
                compiler.compile_library_body(prefix, std::sync::Arc::from(body), suffix, "unit").unwrap()
            };
            let entry = compiler.finish(entry).unwrap();
            let (native, counts, _) = linked_test_residual(residual);
            let host = crate::test_host::TestHost::default().with_native_programs(move |name, parts| {
                assert_eq!(name, "unit");
                if bootstrap { assert_eq!(parts, ["specialized", source]); }
                else { assert_eq!(parts, ["specialized", prefix, body, suffix]); }
                Ok(Some(native.clone()))
            });
            let mut vm = Vm::new(host);
            vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
            if bootstrap {
                let context = vm.embedding_program().unwrap();
                vm.eval_host_script(&context, source, "unit").unwrap();
                let atom = vm.lookup_atom("bootResult").unwrap();
                assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::number(42.0)));
            } else {
                let prepared = vm.prepare_embedding_library_body_kept(
                    prefix, crate::bytecode::SourceText::Shared(std::sync::Arc::from(body)), suffix, "unit",
                ).unwrap();
                assert_eq!(vm.evaluate_embedding_prepared_script(&prepared).unwrap(), Value::number(42.0));
            }
            assert!(counts.iter().all(|count| count.get() > 0));
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_host_script_rejects_atom_remapping_before_effects() {
        let (native, counts, _) = linked_test_program("globalThis.nativeEffect = 42;", false);
        let vm = semantic_vm("globalThis.differentNamespace = 0;");
        let error = match vm.prepare_native_script(native) {
            Ok(_) => panic!("incompatible native atoms were accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("atom IDs"));
        assert!(counts.iter().all(|count| count.get() == 0));
        assert!(vm.lookup_atom("nativeEffect").is_none());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_host_script_rejects_specialization_mismatch_and_resolver_errors() {
        let (native, counts, _) = linked_test_program("", false);
        let entry = crate::Engine::specialize_unspecialized("", "unspecialized.js").unwrap();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
        let error = match vm.prepare_native_script(native) {
            Ok(_) => panic!("incompatible specialization was accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("specialization"));
        assert!(counts.iter().all(|count| count.get() == 0));

        let host = crate::test_host::TestHost::default().with_native_programs(|_, _| Err("broken linked image".into()));
        let mut vm = Vm::new(host);
        let entry = crate::Engine::specialize("", "entry.js").unwrap();
        vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
        let error = match vm.prepare_embedding_specialized_script("globalThis.effect = 1;", "unit", None) {
            Ok(_) => panic!("resolver error silently fell back to compilation"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("broken linked image"));
        assert!(vm.lookup_atom("effect").is_none());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn runtime_native_resolver_preserves_host_fallback_and_refuses_snapshot_loss() {
        let mut compiler = crate::NativeJsCompiler::new(true);
        let entry = compiler.compile_script("", "seed.js", Default::default()).unwrap();
        let unit = compiler.compile_script("throw 42;", "unit.js", Default::default()).unwrap();
        let (native, _, _) = linked_test_residual(unit);
        let calls = Rc::new(std::cell::Cell::new(0));
        let observed = calls.clone();
        let host = crate::test_host::TestHost::default().with_native_programs(move |_, _| {
            observed.set(observed.get() + 1);
            Ok(Some(native.clone()))
        });
        let mut vm = Vm::new(host);
        vm.initialize_shared(&Rc::new(compiler.finish(entry).unwrap()), super::super::builtins::RealmScope::Complete).unwrap();
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        vm.set_native_program_resolver(|_, _| Ok(None));
        let prepared = vm.prepare_embedding_specialized_script("throw 42;", "unit.js", None).unwrap();
        let error = vm.evaluate_embedding_prepared_script(&prepared).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(42.0)));
        assert_eq!(calls.get(), 1);
        assert!(vm.write_snapshot(|_, _| Ok(())).unwrap_err().to_string().contains("native source resolver"));
        vm.set_native_program_resolver(|_, _| Err(crate::NativeSourceError::InvalidImage("incompatible runtime catalog".into())));
        let error = match vm.prepare_embedding_specialized_script("throw 42;", "unit.js", None) {
            Ok(_) => panic!("resolver error fell back to the host"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("incompatible runtime catalog"));
        assert_eq!(calls.get(), 1);
    }

    #[cfg(feature = "native-execution")]
    #[test]
    fn native_only_rejects_unlinked_roots_before_guest_effects() {
        let program = crate::Engine::specialize("globalThis.effect = 1;", "unlinked.js").unwrap();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute(&program).unwrap_err();
        assert!(error.to_string().contains("native guest code required"));
        let atom = vm.lookup_atom("effect").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, atom), None);
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_only_runs_linked_calls_but_rejects_dynamic_function_bodies() {
        let (native, counts, _) = linked_test_program("function answer(x) { return x + 1; } throw answer(41);", false);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(42.0)));
        assert!(counts.iter().all(|count| count.get() > 0));

        let (native, _, _) = linked_test_program("globalThis.effect = 0; (new Function(globalThis.body))();", false);
        let host = crate::test_host::TestHost::default().with_install(|context| {
            let global = context.global_root()?;
            let key = context.string_rooted("body");
            let body = context.string_rooted("globalThis.effect = 1;");
            context.set_property_rooted(global, key, body, global)?;
            Ok(())
        });
        let mut vm = Vm::new(host);
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert!(error.to_string().contains("native guest code required"));
        let atom = vm.lookup_atom("effect").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::number(0.0)));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_function_artifacts_preserve_identity_kinds_and_conversion_order() {
        use crate::{DynamicFunctionKind as Kind, NativeSourceCompilation as Compilation, NativeSourceKey};
        let source = r#"
            globalThis.result = 40;
            function make() { let result = 100; return Function('x', 'return result + x;'); }
            const first = make();
            const second = new Function('x', 'return result + x;');
            if (first === second || first.prototype === second.prototype) throw 'identity';
            if (first.name !== 'anonymous' || first.length !== 1 || first(2) !== 42) throw 'scope';
            if (first.toString() !== 'function anonymous(x\n) {\nreturn result + x;\n}') throw 'source';
            const commented = Function('a/*', '#!comment*/', 'return a;');
            if (commented(42) !== 42 || commented.length !== 1) throw 'parameter comment';
            const generator = (function*(){}).constructor('x', 'yield x;');
            if (generator(42).next().value !== 42) throw 'generator';
            const async = (async function(){}).constructor('x', 'return x;');
            if (!(async(42) instanceof Promise)) throw 'async';
            const asyncGenerator = (async function*(){}).constructor('x', 'yield x;');
            if (!(asyncGenerator(42).next() instanceof Promise)) throw 'async generator';
            let trace = '';
            try { Function({toString() { trace += 'p'; return 'a) {}); globalThis.injected = 1; (function('; }},
                           {toString() { trace += 'b'; return 'return 1;'; }}); throw 'accepted'; }
            catch (error) { if (!(error instanceof SyntaxError) || trace !== 'pb') throw 'syntax/order'; }
            if ('injected' in globalThis) throw 'parameter injection';
            trace = '';
            try { Function({toString() { trace += 'p'; return '#!bad'; }},
                           {toString() { trace += 'b'; return 'return 1;'; }}); }
            catch (error) { if (!(error instanceof SyntaxError) || trace !== 'pb') throw 'hashbang order'; }

            const cause = {};
            try { Function({toString() { throw cause; }}, 'return 1;'); }
            catch (error) { if (error !== cause) throw 'coercion cause'; }
            if (false) Function('let invalid = ;');
            throw 42;
        "#;
        let mut compiler = crate::NativeJsCompiler::new(true);
        let entry = compiler.compile_script(source, "entry.js", Default::default()).unwrap();
        let requests = [
            (Kind::Ordinary, "x", "return result + x;"),
            (Kind::Generator, "x", "yield x;"),
            (Kind::Async, "x", "return x;"),
            (Kind::AsyncGenerator, "x", "yield x;"),
            (Kind::Ordinary, "a) {}); globalThis.injected = 1; (function(", "return 1;"),
            (Kind::Ordinary, "", "let invalid = ;"),
            (Kind::Ordinary, "#!bad", "return 1;"),
            (Kind::Ordinary, "a/*,#!comment*/", "return a;"),
        ];
        let artifacts = requests.into_iter().map(|(kind, parameters, body)| {
            let key = NativeSourceKey::function_constructor(true, kind, parameters, body);
            let artifact = match compiler.compile_function_source(&key).unwrap() {
                Compilation::Program(program) => Ok(linked_test_residual(program).0),
                Compilation::SyntaxError(message) => Err(crate::NativeSourceError::Syntax(message)),
            };
            (key, artifact)
        }).collect::<Vec<_>>();
        let entry = linked_test_residual(compiler.finish(entry).unwrap()).0;
        let host = crate::test_host::TestHost::default().with_native_programs(move |name, parts| {
            Ok(match artifacts.iter().find(|(key, _)| key.name() == name && key.parts().eq(parts.iter().copied())) {
                Some((_, artifact)) => Some(artifact.clone()?),
                None => None,
            })
        });
        let mut vm = Vm::new(host);
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&entry, true).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(42.0)), "{error}");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_only_rejects_source_before_parsing_and_preserves_function_argument_coercion() {
        for source in [
            "eval('let compilationOnly = ;');",
            "(0, eval)('let compilationOnly = ;');",
            "new Function({toString() { globalThis.converted = 1; return 'let compilationOnly = ;'; }});",
        ] {
            let (native, _, _) = linked_test_program(source, false);
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.execute_native_program(&native, true).unwrap_err();
            assert!(error.to_string().contains("source compilation is unavailable"), "{source}: {error}");
            assert!(vm.lookup_atom("compilationOnly").is_none());
            if source.starts_with("new Function") {
                let atom = vm.lookup_atom("converted").unwrap();
                assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::number(1.0)));
            }
        }
        let (native, _, _) = linked_test_program("throw eval(42);", false);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        assert_eq!(vm.execute_native_program(&native, true).unwrap_err().thrown_value(), Some(Value::number(42.0)));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_frames_call_return_and_unwind_without_replaying_effects() {
        for typescript in [false, true] {
            let (native, counts, _) = linked_test_program(
                "let trace = ''; function fail() { trace += 't'; throw 7; } function recover() { try { fail(); } catch (e) { trace += 'c'; return e + 1; } finally { trace += 'f'; } } if (recover() !== 8) throw 'bad return'; throw trace;",
                typescript,
            );
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            let Err(error) = vm.execute_native_program(&native, true) else { panic!("expected trace throw") };
            let value = error.thrown_value().unwrap();
            assert_eq!(vm.heap.string(value).unwrap().to_string(), "tcf");
            assert!(counts.iter().all(|count| count.get() > 0), "every called function enters native code");
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_calls_share_the_dispatch_frame_transition() {
        for typescript in [false, true] {
            let (native, _, _) = linked_test_program(
                "function target(value) { return value + 1; } function caller() { const a = target(20); const b = target(20, ...[]); return a + b; } throw caller();",
                typescript,
            );
            #[cfg(not(quench_profiled))]
            let program = native.residual();
            #[cfg(not(quench_profiled))]
            let caller = program.functions.iter().position(|body| {
                body.header().name.is_some_and(|name| &program.atoms[name as usize] == "caller")
            }).unwrap() as u32;
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.initialize_shared(&Rc::new(native.residual().clone()), super::super::builtins::RealmScope::Complete).unwrap();
            let prepared = vm.prepare_native_script(native).unwrap();
            vm.programs.attach_native_js(ProgramId::MAIN, prepared.native.unwrap()).unwrap();
            #[cfg(not(quench_profiled))]
            {
                vm.programs.enable_js_jit_diagnostics();
                // Linked code skips JIT admission, which normally creates this row.
                vm.programs.record_js_jit_entry(ProgramId::MAIN, caller, false, 0);
            }
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.run_main(&prepared.program, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::number(42.0)));
            #[cfg(not(quench_profiled))]
            {
                let sites = vm.programs.js_jit_call_site_counts(ProgramId::MAIN, caller).unwrap();
                for opcode in [Op::Call, Op::CallArray] {
                    assert!(sites.iter().any(|(_, op, _, outcomes, ..)| {
                        *op == opcode as u16
                            && outcomes[super::super::program_store::JsJitCallOutcome::PushFrame as usize] > 0
                    }), "linked {opcode:?} must use the shared frame transition");
                }
            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_generator_stops_after_parameters_and_resumes_yield() {
        let (native, counts, _) = linked_test_program(
            "let trace = ''; function* values(x = (trace += 'p', 1)) { trace += 'b'; yield x; trace += 'r'; return 2; } let g = values(); if (trace !== 'p') throw 'body ran during parameters'; let first = g.next(); let last = g.next(); if (first.value !== 1 || first.done || last.value !== 2 || !last.done) throw 'bad yield'; throw trace;",
            false,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let Err(error) = vm.execute_native_program(&native, true) else { panic!("expected trace throw") };
        let value = error.thrown_value().unwrap();
        assert_eq!(vm.heap.string(value).unwrap().to_string(), "pbr");
        assert!(counts.iter().all(|count| count.get() > 0));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_terminal_calls_preserve_constructor_results_and_order() {
        for typescript in [false, true] {
            let (native, counts, _) = linked_test_program(
                "let trace = ''; function add(x) { trace += 'a'; return x + 1; } function wrap(x) { trace += 'w'; return add(x); } function Box(x) { trace += 'b'; this.value = wrap(x); } let box = new Box(41); if (Object.getPrototypeOf(box) !== Box.prototype || box.value !== 42 || trace !== 'bwa') throw 'bad construction'; throw box.value;",
                typescript,
            );
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            let Err(error) = vm.execute_native_program(&native, true) else { panic!("expected result throw") };
            assert_eq!(error.thrown_value(), Some(Value::number(42.0)));
            assert!(counts.iter().all(|count| count.get() > 0));
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_async_frame_resumes_after_await_and_finishes_jobs() {
        let (native, counts, _) = linked_test_program(
            "globalThis.answer = 0; async function work() { try { globalThis.answer = await Promise.resolve(7); } finally { globalThis.finished = true; } } work();",
            false,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.execute_native_program(&native, true).unwrap();
        for (name, expected) in [("answer", Value::number(7.0)), ("finished", Value::TRUE)] {
            let atom = vm.lookup_atom(name).unwrap();
            assert_eq!(vm.own_property(vm.realm.globals, atom), Some(expected));
        }
        assert!(counts.iter().all(|count| count.get() > 0));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_typed_call_preserves_an_asserted_strings_actual_value() {
        let (native, counts, _) = linked_test_program(
            "function add(x: number): number { return x + 1; } throw add('x' as unknown as number);", true,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let Err(error) = vm.execute_native_program(&native, true) else { panic!("expected throw") };
        assert_eq!(vm.heap.string(error.thrown_value().unwrap()).unwrap().to_string(), "x1");
        assert!(counts.iter().all(|count| count.get() > 0));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_generator_preserves_parameter_errors_before_body_entry() {
        let (native, counts, _) = linked_test_program(
            "function* values({x}) { throw 'body ran'; } let caught = false; try { values(null); } catch (e) { caught = e instanceof TypeError; } if (!caught) throw 'missing parameter error'; throw 'parameter';", false,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let Err(error) = vm.execute_native_program(&native, true) else { panic!("expected throw") };
        assert_eq!(vm.heap.string(error.thrown_value().unwrap()).unwrap().to_string(), "parameter");
        assert!(counts.iter().all(|count| count.get() > 0));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_callbacks_collect_and_preserve_object_identity() {
        for typescript in [false, true] {
            let (native, counts, _) = linked_test_program(
                "const keep = {id: 7}; const marker = {}; function callback(x) { $262.gc(); if (x === marker) throw marker; return x; } function outer() { for (let n = 0; n < 16; n++) { try { [marker].map(callback); throw 'missing callback throw'; } catch (error) { if (error !== marker) throw error; } const values = [keep, keep].map(callback); if (values[0] !== keep || values[1] !== keep) throw 'identity changed'; } return keep.id; } throw outer();",
                typescript,
            );
            let mut vm = Vm::new(crate::test_host::TestHost::default().with_test262());
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.execute_native_program(&native, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::number(7.0)));
            assert!(counts.iter().all(|count| count.get() > 0));
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_capture_cache_preserves_tdz_mutation_and_collection() {
        let (native, counts, _) = linked_test_program(
            "function outer() { const read = () => { let result; for (let i = 0; i < 32; i++) { result = value; if (i === 7) $262.gc(); } return result; }; let caught = false; try { read(); } catch (e) { caught = e instanceof ReferenceError; } if (!caught) throw 'tdz'; let value = {n: 1}; if (read() !== value) throw 'identity'; value = {n: 2}; return read().n; } throw outer();",
            false,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default().with_test262());
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(2.0)));
        assert!(counts.iter().all(|count| count.get() > 0));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_code_owner_survives_the_program_handle_until_vm_reset() {
        let (native, counts, weak) = linked_test_program("globalThis.saved = () => 42;", false);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.execute_native_program(&native, true).unwrap();
        drop(native);
        assert!(weak.upgrade().is_some());
        let atom = vm.lookup_atom("saved").unwrap();
        let saved = vm.own_property(vm.realm.globals, atom).unwrap();
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        assert_eq!(vm.call_value(&program, saved, Value::UNDEFINED, &[]).unwrap(), Value::number(42.0));
        assert!(counts.iter().all(|count| count.get() > 0));
        let replacement = crate::Engine::specialize("0;", "replacement.js").unwrap();
        vm.execute(&replacement).unwrap();
        assert!(weak.upgrade().is_none(), "reset releases code with the old program");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_deferred_jobs_keep_their_code_until_the_host_checkpoint() {
        let (native, counts, weak) = linked_test_program(
            "globalThis.answer = 0; Promise.resolve().then(() => { globalThis.answer = 42; });", false,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.execute_native_program(&native, false).unwrap();
        drop(native);
        assert!(weak.upgrade().is_some());
        let atom = vm.lookup_atom("answer").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::number(0.0)));
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        vm.drain_jobs(&program).unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::number(42.0)));
        assert!(counts.iter().all(|count| count.get() > 0));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_semantic_graph_rejects_full_width_invalid_resume_pc_before_effects() {
        let mut vm = semantic_vm("globalThis.effect = 1;");
        let (_memory, entry, _operands) = live_native_graph(&vm);
        let (_prepared, partitioned) = partitioned_native_graph(&vm);
        let invalid = (u64::from(u32::MAX) + 1) as usize;
        for entry in [entry, partitioned] {
            let (status, word, outcome) = with_semantic_context(&mut vm, invalid, |context, result| {
                // SAFETY: as above; the out-of-range PC is data, not a pointer.
                unsafe { i64::from(entry(context, 0, 0, std::ptr::null(), result)) }
            });
            assert_eq!(status, i64::from(NATIVE_JS_INVALID_PC));
            assert_eq!(word, invalid as u64);
            assert!(outcome.is_none());
        }
        let effect = vm.lookup_atom("effect").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, effect), None);
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_semantic_graph_does_not_trust_typescript_assertions() {
        let program = crate::Engine::specialize_typescript(
            "(globalThis as any).input = 'x' as unknown as number; throw (globalThis as any).input + 1;",
            "native-assertion.cts",
        ).unwrap();
        let mut vm = semantic_program(program);
        let (_memory, entry, _operands) = live_native_graph(&vm);
        let (_prepared, partitioned) = partitioned_native_graph(&vm);
        for entry in [entry, partitioned] {
            let (status, _, outcome) = with_semantic_context(&mut vm, 0, |context, result| {
                // SAFETY: the rooted frame, entries and operands live through the call.
                unsafe { i64::from(entry(context, 0, 0, std::ptr::null(), result)) }
            });
            assert_eq!(status, i64::from(NATIVE_JS_STOPPED));
            let Err(error) = outcome.expect("graph stops at throw").result else { panic!("expected throw") };
            let value = error.thrown_value().expect("original thrown value");
            assert_eq!(vm.heap.string(value).expect("JS string concatenation").to_string(), "x1");
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn static_semantic_entry_executes_one_store_and_returns_to_native_control() {
        let mut vm = semantic_vm("globalThis.x = 1; globalThis.y = 2;");
        let (pc, instruction) = semantic_instruction(&vm, Op::SetField);
        vm.frames[0].registers[usize::from(instruction.register_b())] = vm.realm.globals;
        let value = Value::number(1.0);
        vm.frames[0].registers[usize::from(instruction.register_a())] = value;
        let (status, next, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::SetField as usize }>);
        assert_eq!(status, 0);
        assert_eq!(next, (pc + 1) as u64);
        assert!(outcome.is_none());
        assert_eq!(vm.own_property(vm.realm.globals, instruction.atom_index()), Some(value));
        let y = vm.lookup_atom("y").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, y), None);
    }

    #[cfg(feature = "aot")]
    #[test]
    fn static_semantic_entry_rejects_mismatched_or_invalid_opcodes_before_effects() {
        let mut vm = semantic_vm("globalThis.x = 1;");
        let (pc, _) = semantic_instruction(&vm, Op::SetField);
        let before = vm.frames[0].registers.clone();
        for entry in [
            native_js_operation_entry::<crate::test_host::TestHost, { Op::LoadConst as usize }>,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::COUNT }>,
            native_js_operation_entry::<crate::test_host::TestHost, { usize::MAX }>,
        ] {
            let (status, _, outcome) = invoke_semantic(&mut vm, pc, entry);
            assert_eq!(status, 1);
            assert!(outcome.unwrap().result.is_err());
            assert_eq!(vm.frames[0].registers, before);
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn static_semantic_entry_admits_wide_opcodes_without_decoding_source_operands() {
        let mut program = crate::Engine::specialize("throw 7;", "wide-semantic-entry.js").unwrap();
        let body = &mut program.functions[0];
        let pc = body.instructions().position(|instruction| instruction.op() == Op::Throw).unwrap();
        let instruction = super::super::dispatch_fast::decoded_instruction(body, body.code[pc]);
        let wide_index = body.wide.len();
        body.wide.push(instruction);
        body.code[pc] = crate::bytecode::Instr::wide(wide_index).unwrap();
        let mut vm = semantic_program(program);
        vm.frames[0].registers[usize::from(instruction.register_a())] = Value::number(7.0);
        let (status, _, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::SetField as usize }>);
        assert_eq!(status, 1);
        assert!(outcome.unwrap().result.is_err());
        assert_eq!(vm.frames[0].registers[usize::from(instruction.register_a())], Value::number(7.0));
        let (status, _, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::Throw as usize }>);
        assert_eq!(status, 1);
        let outcome = outcome.unwrap();
        assert_eq!(outcome.instruction_pc, pc);
        let Err(error) = outcome.result else { panic!("wide Throw must stop with its original value") };
        assert_eq!(error.thrown_value(), Some(Value::number(7.0)));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn static_semantic_entry_preserves_thrown_value_and_exact_pc() {
        let mut vm = semantic_vm("throw 7;");
        let (pc, instruction) = semantic_instruction(&vm, Op::Throw);
        let value = Value::number(7.0);
        vm.frames[0].registers[usize::from(instruction.register_a())] = value;
        let (status, _, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::Throw as usize }>);
        assert_eq!(status, 1);
        let outcome = outcome.unwrap();
        assert_eq!(outcome.instruction_pc, pc);
        assert_eq!(outcome.next_pc, pc + 1);
        let Err(error) = outcome.result else { panic!("semantic throw must be abrupt") };
        assert_eq!(error.thrown_value(), Some(value));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn static_semantic_entry_checks_termination_before_guest_effects() {
        let mut vm = semantic_vm("globalThis.x = 1;");
        let (pc, instruction) = semantic_instruction(&vm, Op::SetField);
        vm.frames[0].registers[usize::from(instruction.register_b())] = vm.realm.globals;
        vm.frames[0].registers[usize::from(instruction.register_a())] = Value::number(1.0);
        vm.execution_interrupt.request_termination();
        let (status, _, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::SetField as usize }>);
        assert_eq!(status, 1);
        let outcome = outcome.unwrap();
        assert_eq!(outcome.instruction_pc, pc);
        let Err(error) = outcome.result else { panic!("terminated entry must stop") };
        assert!(error.is_execution_terminated());
        assert_eq!(vm.own_property(vm.realm.globals, instruction.atom_index()), None);
    }

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
            js_constants: std::ptr::null(),
            js_constant_count: 0,
            interrupt_owner: std::ptr::null(),
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
    #[cfg(all(target_arch = "x86_64", unix))]
    fn native_value_index_offset_preserves_large_indices() {
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        for ty in [types::I32, types::I64] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.push(AbiParam::new(types::I64));
            signature.returns.push(AbiParam::new(types::I64));
            let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let block = builder.create_block();
            builder.append_block_params_for_function_params(block);
            builder.switch_to_block(block);
            builder.seal_block(block);
            let index = builder.block_params(block)[0];
            let index = if ty == types::I32 { builder.ins().ireduce(ty, index) } else { index };
            let offset = emit_value_index_offset(&mut builder, index);
            builder.ins().return_(&[offset]);
            builder.finalize(isa.frontend_config());
            let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
            // SAFETY: the generated function has this signature, accesses no memory,
            // and its executable allocation remains live throughout these calls.
            let offset = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64) -> u64>(code.pointer()) };
            for index in [0, 1, (1 << 29) - 1, 1 << 29, (1 << 29) + 1, i32::MAX as u64] {
                assert_eq!(unsafe { offset(index) }, index * std::mem::size_of::<Value>() as u64, "{ty}: index {index}");
            }
        }
    }

    #[test]
    #[cfg(all(target_arch = "x86_64", unix))]
    fn native_checked_signed_multiply_preserves_product_and_overflow() {
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        for ty in [types::I16, types::I32, types::I64] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 3]);
            let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let block = builder.create_block();
            builder.append_block_params_for_function_params(block);
            builder.switch_to_block(block);
            builder.seal_block(block);
            let args = builder.block_params(block).to_vec();
            let left = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
            let right = if ty == types::I64 { args[1] } else { builder.ins().ireduce(ty, args[1]) };
            let (product, overflow) = builder.ins().smul_overflow(left, right);
            let product = if ty == types::I64 { product } else { builder.ins().sextend(types::I64, product) };
            let overflow = builder.ins().uextend(types::I64, overflow);
            builder.ins().store(MemFlagsData::trusted(), product, args[2], 0);
            builder.ins().store(MemFlagsData::trusted(), overflow, args[2], std::mem::size_of::<i64>() as i32);
            builder.ins().return_(&[]);
            builder.finalize(isa.frontend_config());
            let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
            // SAFETY: the generated signature is (i64, i64, pointer) -> void; its only
            // memory accesses write the two i64 slots supplied below. Code stays live.
            let multiply = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(i64, i64, *mut i64)>(code.pointer()) };
            let bits = ty.bits();
            let min = -(1i128 << (bits - 1));
            let max = (1i128 << (bits - 1)) - 1;
            let cases = [min, min + 1, -46341, -2, -1, 0, 1, 2, 46341, max - 1, max];
            for left in cases.into_iter().filter(|value| (min..=max).contains(value)) {
                for right in cases.into_iter().filter(|value| (min..=max).contains(value)) {
                    let exact = left * right;
                    let wrapped = (exact << (128 - bits)) >> (128 - bits);
                    let mut actual = [0i64; 2];
                    unsafe { multiply(left as i64, right as i64, actual.as_mut_ptr()) };
                    assert_eq!(actual, [wrapped as i64, i64::from(!(min..=max).contains(&exact))], "{ty}: {left} * {right}");
                }
            }
        }
    }

    #[test]
    #[cfg(all(target_arch = "x86_64", unix))]
    fn native_vector_rotations_preserve_near_matches_and_unaligned_accesses() {
        #[derive(Clone, Copy, Debug)]
        enum Shape { Rotate, DistinctInputs, SignedRight, NonComplementaryShift }
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        // A baseline x86-64 target has no optional SSSE3: it must retain
        // the existing shifts/OR lowering while producing the same values.
        let baseline = cranelift_codegen::isa::lookup(isa.triple().clone())
            .unwrap().finish(isa.flags().clone()).unwrap();
        for isa in [isa, baseline] {
        let cases = [0, 1, 2, u32::MAX as u64, 1 << 32, 1 << 63, i64::MAX as u64, u64::MAX, 0x0123_4567_89ab_cdef, 0xaaaa_5555_aaaa_5555];
        for rotate in 0..u64::BITS {
            for reversed in [false, true] {
                for shape in [Shape::Rotate, Shape::DistinctInputs, Shape::SignedRight, Shape::NonComplementaryShift] {
                    let mut signature = Signature::new(isa.default_call_conv());
                    signature.params.extend([AbiParam::new(types::I64); 3]);
                    let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
                    let mut context = FunctionBuilderContext::new();
                    let mut builder = FunctionBuilder::new(&mut function, &mut context);
                    let block = builder.create_block();
                    builder.append_block_params_for_function_params(block);
                    builder.switch_to_block(block);
                    builder.seal_block(block);
                    let args = builder.block_params(block).to_vec();
                    let memory = MemFlagsData::new().with_notrap();
                    let cast = MemFlagsData::new().with_endianness(ir::Endianness::Little);
                    let first = builder.ins().load(types::I8X16, memory, args[0], 0);
                    let first = builder.ins().bitcast(types::I64X2, cast, first);
                    let second = if matches!(shape, Shape::DistinctInputs) {
                        let value = builder.ins().load(types::I8X16, memory, args[1], 0);
                        builder.ins().bitcast(types::I64X2, cast, value)
                    } else { first };
                    let left_amount = u64::BITS - rotate - u32::from(matches!(shape, Shape::NonComplementaryShift));
                    let left_count = builder.ins().iconst(types::I32, i64::from(left_amount));
                    let right_count = builder.ins().iconst(types::I32, i64::from(rotate));
                    let left = builder.ins().ishl(first, left_count);
                    let right = if matches!(shape, Shape::SignedRight) {
                        builder.ins().sshr(second, right_count)
                    } else { builder.ins().ushr(second, right_count) };
                    let left = builder.ins().bitcast(types::I8X16, cast, left);
                    let right = builder.ins().bitcast(types::I8X16, cast, right);
                    let value = if reversed { builder.ins().bor(right, left) } else { builder.ins().bor(left, right) };
                    builder.ins().store(memory, value, args[2], 0);
                    builder.ins().return_(&[]);
                    builder.finalize(isa.frontend_config());
                    let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
                    // SAFETY: generated code accesses exactly sixteen bytes at
                    // each pointer, promises no alignment, and stays live here.
                    let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const u8, *const u8, *mut u8)>(code.pointer()) };
                    for offset in [0, 1, 7] {
                        for (index, input) in cases.into_iter().enumerate() {
                            let left = [input, cases[(index + 3) % cases.len()]];
                            let right = if matches!(shape, Shape::DistinctInputs) { [!left[1], !left[0]] } else { left };
                            let mut first = [0u8; 32];
                            let mut second = [0u8; 32];
                            let mut output = [0xa5u8; 32];
                            for lane in 0..2 {
                                let start = offset + lane * std::mem::size_of::<u64>();
                                first[start..start + 8].copy_from_slice(&left[lane].to_le_bytes());
                                second[start..start + 8].copy_from_slice(&right[lane].to_le_bytes());
                            }
                            unsafe { run(first.as_ptr().add(offset), second.as_ptr().add(offset), output.as_mut_ptr().add(offset)) };
                            for lane in 0..2 {
                                let right = if matches!(shape, Shape::SignedRight) { (right[lane] as i64).wrapping_shr(rotate) as u64 } else { right[lane].wrapping_shr(rotate) };
                                let expected = left[lane].wrapping_shl(left_amount) | right;
                                let start = offset + lane * std::mem::size_of::<u64>();
                                assert_eq!(&output[start..start + 8], &expected.to_le_bytes(), "{shape:?}, rotate={rotate}, reversed={reversed}, offset={offset}");
                            }
                            assert!(output[..offset].iter().chain(output[offset + 16..].iter()).all(|byte| *byte == 0xa5));
                        }
                    }
                }
            }
        }
        }
    }

    #[test]
    fn native_alias_regions_preserve_overlapping_untagged_accesses() {
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        for tagged_load in [false, true] {
            for tagged_store in [false, true] {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.push(AbiParam::new(types::I64));
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
                let region = function.dfg.alias_regions.insert(ir::AliasRegionData {
                    user_id: 0,
                    description: "overlapping test accesses".into(),
                });
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let block = builder.create_block();
                builder.append_block_params_for_function_params(block);
                builder.switch_to_block(block);
                builder.seal_block(block);
                let address = builder.block_params(block)[0];
                let load_flags = MemFlagsData::trusted().with_alias_region(tagged_load.then_some(region));
                let store_flags = MemFlagsData::trusted().with_alias_region(tagged_store.then_some(region));
                let first = builder.ins().load(types::I64, load_flags, address, 0);
                let zero = builder.ins().iconst(types::I64, 0);
                builder.ins().store(store_flags, zero, address, 0);
                let second = builder.ins().load(types::I64, load_flags, address, 0);
                let result = builder.ins().iadd(first, second);
                builder.ins().return_(&[result]);
                builder.finalize(isa.frontend_config());
                let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
                // SAFETY: the generated function reads and writes one live u64,
                // and its executable mapping remains alive throughout each call.
                let run = unsafe {
                    std::mem::transmute::<*mut u8, unsafe extern "C" fn(*mut u64) -> u64>(code.pointer())
                };
                for input in [0, 1, 42, i64::MAX as u64, u64::MAX] {
                    let mut memory = input;
                    assert_eq!(unsafe { run(&mut memory) }, input, "load tagged={tagged_load}, store tagged={tagged_store}");
                    assert_eq!(memory, 0);
                }
            }
        }
    }

    #[test]
    fn native_unsigned_widening_multiply_preserves_lane_products() {
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        // Exhaust both single-input shuffle encodings and all lane selectors,
        // including the unaffected cases, against scalar unsigned products.
        for shuffle in std::iter::once(None).chain(
            (0..=u8::MAX).flat_map(|mask| [Some((mask, false)), Some((mask, true))]),
        ) {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 3]);
            let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let block = builder.create_block();
            builder.append_block_params_for_function_params(block);
            builder.switch_to_block(block);
            builder.seal_block(block);
            let args = builder.block_params(block).to_vec();
            let mut input = |address| {
                let value = builder.ins().load(types::I8X16, MemFlagsData::trusted(), address, 0);
                let value = if let Some((selector, rhs)) = shuffle {
                    let lanes: Vec<u8> = (0..16).map(|byte| {
                        ((selector >> ((byte / 4) * 2)) & 3) * 4 + byte % 4 + if rhs { 16 } else { 0 }
                    }).collect();
                    let mask = builder.func.dfg.immediates.push(ir::ConstantData::from(lanes));
                    let poison = builder.func.dfg.constants.insert(ir::ConstantData::from(vec![0x5a; 16]));
                    let poison = builder.ins().vconst(types::I8X16, poison);
                    let (left, right) = if rhs { (poison, value) } else { (value, poison) };
                    builder.ins().shuffle(left, right, mask)
                } else { value };
                builder.ins().bitcast(types::I32X4, MemFlagsData::new().with_endianness(ir::Endianness::Little), value)
            };
            let left = input(args[0]);
            let right = input(args[1]);
            let left = builder.ins().uwiden_low(left);
            let right = builder.ins().uwiden_low(right);
            let product = builder.ins().imul(left, right);
            builder.ins().store(MemFlagsData::trusted(), product, args[2], 0);
            builder.ins().return_(&[]);
            builder.finalize(isa.frontend_config());
            let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
            // SAFETY: generated code reads two four-u32 inputs and writes two u64 outputs.
            // All arrays below outlive the call, as does the executable code mapping.
            let multiply = unsafe {
                std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const u32, *const u32, *mut u64)>(code.pointer())
            };
            let cases = [0, 1, 2, 0xffff, 0x10000, 0x7fff_ffff, 0x8000_0000, u32::MAX];
            for a in cases {
                for b in cases {
                    // Different lane values and poisoned unused lanes expose lane selection
                    // errors as well as accidental signed extension or truncation.
                    let left = [a, !b, u32::MAX, 0];
                    let right = [b, !a, 0, u32::MAX];
                    let mut actual = [0u64; 2];
                    unsafe { multiply(left.as_ptr(), right.as_ptr(), actual.as_mut_ptr()) };
                    let indices = shuffle.map_or([0, 1], |(selector, _)| [usize::from(selector & 3), usize::from((selector >> 2) & 3)]);
                    let expected = indices.map(|index| u64::from(left[index]) * u64::from(right[index]));
                    assert_eq!(actual, expected, "shuffle {shuffle:?}, inputs {left:?} * {right:?}");
                }
            }
        }
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
        assert_eq!(Slot::Memory32Address.join(Slot::Memory32Address), Slot::Memory32Address);
        assert_eq!(Slot::Memory32Address.join(Slot::Value(Kind::I64)), Slot::Value(Kind::I64));
        assert_eq!(Slot::Value(Kind::I64).join(Slot::Memory32Address), Slot::Value(Kind::I64));
        assert_eq!(Slot::Memory32Address.join(Slot::DEFAULT), Slot::Conflict);
        const FACTS: [Slot; 9] = [
            Slot::DEFAULT,
            Slot::Value(Kind::I64),
            Slot::Memory32Address,
            Slot::Vector,
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
