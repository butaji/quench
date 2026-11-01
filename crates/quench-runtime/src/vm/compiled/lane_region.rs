//! The lane region specializer: Futamura's first projection of the interpreter onto one
//! JavaScript function, run on the function's frameless lane activations under Ershov's
//! online division (docs/v2/specialization.md).
//!
//! What the division makes static is folded, and each dynamic fact is proved once:
//!
//! - Every bytecode register is an SSA value in machine registers. The register file is
//!   read at entry and written back only where the region leaves.
//! - Shared semantic Type/Fact proofs select unboxed i32/f64 views. Read-only numeric
//!   inputs are proved once at region entry; definitions and CFG edges carry the proof.
//!   Tagged bits are rebuilt only where a tagged consumer or recovery needs them.
//! - Region-wide facts are proved at entry: the receiver's record, shape and storage, the
//!   shapes the receiver sites fold, the method-cache state stores need, and capture reads.
//! - Field sites fold their cache entries (shape and slot) and constants fold to immediates.
//!
//! The region leaves at the exact PC of any instruction it does not run, before any of
//! that instruction's effects, with the register file holding the interpreter's state; the
//! lane resumes there. Nothing in a run allocates, collects or calls guest code.

use super::*;
use crate::bytecode::FieldLayout;
use crate::typed_ir::{BindingTime, Fact, Projection, Proof, Representation, Type};
use super::object_slots::{CheckedObject, ObjectHeader, PackedAccess};
use std::collections::{BTreeMap, BTreeSet};

const LANE_REGION_NAMESPACE: u32 = u32::from_be_bytes(*b"QLRG");

/// Semantic proof and active machine view; tagged bits may also be current.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RegisterFact {
    value: Fact,
    tagged_current: bool,
}

impl RegisterFact {
    fn definition(ty: Type, representation: Representation) -> Self {
        Self { value: Fact { ty, representation, binding: BindingTime::Dynamic,
            proof: Proof::Definition, projection: Projection::Exact },
            tagged_current: representation == Representation::Tagged }
    }

    fn canonical() -> Self {
        Self { value: Fact::tagged(BindingTime::Location, Proof::CanonicalValue), tagged_current: true }
    }
}

struct Registers {
    tagged: Vec<Variable>,
    int: Vec<Variable>,
    float: Vec<Variable>,
}

/// The compile-time state of the instruction being lowered.
#[derive(Clone)]
struct State {
    values: Vec<RegisterFact>,
    /// Registers proved to hold a direct object, this block: its record address and shape.
    shapes: BTreeMap<Register, (ir::Value, ir::Value, ir::Value)>,
    /// A fixed receiver (`None`) or register's complete own-slot projection.
    layouts: BTreeMap<(Option<Register>, u32), CheckedObject>,
    /// Registers holding the receiver, whose facts the entry proved.
    receivers: BTreeSet<Register>,
}

impl State {
    fn canonical(registers: usize) -> Self {
        Self { values: vec![RegisterFact::canonical(); registers], shapes: BTreeMap::new(),
            layouts: BTreeMap::new(), receivers: BTreeSet::new() }
    }
}

/// Region-wide facts proved at entry.
struct Entry {
    receiver: LaneReceiver,
    /// Per folded receiver shape, whether the receiver has it.
    receiver_shapes: BTreeMap<u32, Variable>,
    /// Per stored atom, whether the run admits direct stores of it.
    store_atoms: BTreeMap<Atom, Variable>,
    /// Per capture slot read, its value or `DELETED`.
    captures: BTreeMap<u16, Variable>,
    this: Variable,
}

pub(super) struct LaneRegion {
    pub(super) code: Vec<u8>,
    /// Instructions the region runs natively, for admission.
    pub(super) native: usize,
    pub(super) clif: Option<String>,
}

struct Lowering<'a, 'f> {
    builder: FunctionBuilder<'f>,
    program: &'a ResidualProgram,
    unit: &'a super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &'a [Vec<NativeFieldLoad>],
    context: ir::Value,
    payload: ir::Value,
    registers_base: ir::Value,
    result: ir::Value,
    registers: Registers,
    written: BTreeSet<Register>,
    blocks: Vec<Option<ir::Block>>,
    exits: BTreeMap<u32, ir::Block>,
    entry: Option<Entry>,
    native: usize,
    /// Guard failure blocks to fill once the region is lowered (`guard_exit`).
    pending: Vec<(ir::Block, State, u32)>,
    /// Whether the unit makes native calls (`Lowering::call`): a callee may change `this`,
    /// so no read or store folds the receiver facts its entry proved.
    native_calls: bool,
    /// The registers' types at each block leader (`facts::analyze`).
    facts: super::facts::Facts,
    live: Option<&'a crate::compile::liveness::RegisterMasks>,
    valid: bool,
}

fn register_offset(register: Register) -> i32 {
    i32::from(register) * std::mem::size_of::<u64>() as i32
}

fn int_tagged(builder: &mut FunctionBuilder<'_>, int: ir::Value) -> ir::Value {
    let payload = builder.ins().uextend(types::I64, int);
    builder.ins().bor_imm_u(payload, crate::value::ValueLayout::INT_TAG as i64)
}

fn is_int_tagged(builder: &mut FunctionBuilder<'_>, bits: ir::Value) -> ir::Value {
    let shift = i64::from(crate::value::ValueLayout::TAG_SHIFT);
    let tag = builder.ins().ushr_imm_u(bits, shift);
    builder.ins().icmp_imm_u(IntCC::Equal, tag, (crate::value::ValueLayout::INT_TAG >> shift) as i64)
}

/// The registers an instruction writes, by its operand layouts.
fn writes(instruction: WideInstruction) -> Vec<Register> {
    let mut written = Vec::new();
    for field in crate::bytecode::InstructionField::ALL {
        let layout = instruction.op().field_layout(*field);
        let value = match field {
            crate::bytecode::InstructionField::A => instruction.a(),
            crate::bytecode::InstructionField::B => instruction.b(),
            crate::bytecode::InstructionField::C => instruction.c(),
        };
        match layout {
            FieldLayout::ResultRegister | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister => {
                written.push(value & crate::bytecode::REGISTER_MASK)
            }
            _ => {}
        }
    }
    written
}

impl<'a, 'f> Lowering<'a, 'f> {
    fn ins(&mut self) -> cranelift_frontend::FuncInstBuilder<'_, 'f> {
        self.builder.ins()
    }

    fn tagged(&mut self, state: &mut State, register: Register) -> ir::Value {
        let index = usize::from(register);
        let fact = state.values[index];
        if !fact.tagged_current {
            let bits = match fact.value.representation {
                Representation::Int32 => {
                    let int = self.builder.use_var(self.registers.int[index]);
                    int_tagged(&mut self.builder, int)
                }
                Representation::Float64 => {
                    let number = self.builder.use_var(self.registers.float[index]);
                    emit_js_box_number(&mut self.builder, number)
                }
                Representation::Tagged => unreachable!("tagged view is current"),
            };
            self.builder.def_var(self.registers.tagged[index], bits);
            state.values[index].tagged_current = true;
        }
        self.builder.use_var(self.registers.tagged[index])
    }

    fn define(&mut self, state: &mut State, register: Register, value: ir::Value, fact: RegisterFact) {
        self.valid &= fact.value.verify();
        let index = usize::from(register);
        let variable = match fact.value.representation {
            Representation::Tagged => self.registers.tagged[index],
            Representation::Int32 => self.registers.int[index],
            Representation::Float64 => self.registers.float[index],
        };
        self.builder.def_var(variable, value);
        state.values[index] = fact;
        state.shapes.remove(&register);
        state.layouts.retain(|(owner, _), _| *owner != Some(register));
        state.receivers.remove(&register);
    }

    fn set_tagged(&mut self, state: &mut State, register: Register, bits: ir::Value) {
        self.define(state, register, bits, RegisterFact::definition(Type::ANY, Representation::Tagged));
    }

    fn set_int(&mut self, state: &mut State, register: Register, int: ir::Value) {
        self.define(state, register, int, RegisterFact::definition(Type::INT32, Representation::Int32));
    }

    fn set_number(&mut self, state: &mut State, register: Register, number: ir::Value) {
        self.define(state, register, number, RegisterFact::definition(Type::NUMBER, Representation::Float64));
    }

    /// Prove an exact int32 view once, preserving the canonical bits for recovery.
    fn int(&mut self, state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
        let index = usize::from(register);
        if state.values[index].value.representation == Representation::Int32 {
            return self.builder.use_var(self.registers.int[index]);
        }
        let bits = self.tagged(state, register);
        if !state.values[index].value.ty.is_subtype(Type::INT32) {
            let integer = is_int_tagged(&mut self.builder, bits);
            emit_js_guard(&mut self.builder, integer, otherwise);
        }
        let int = self.ins().ireduce(types::I32, bits);
        self.builder.def_var(self.registers.int[index], int);
        state.values[index] = RegisterFact { tagged_current: true,
            ..RegisterFact::definition(Type::INT32, Representation::Int32) };
        int
    }

    /// Prove Number only for a new untrusted value; later uses read its unboxed view.
    fn number(&mut self, state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
        let index = usize::from(register);
        let fact = state.values[index].value;
        match fact.representation {
            Representation::Float64 => self.builder.use_var(self.registers.float[index]),
            Representation::Int32 => {
                let int = self.builder.use_var(self.registers.int[index]);
                self.ins().fcvt_from_sint(types::F64, int)
            }
            Representation::Tagged => {
                let bits = self.tagged(state, register);
                let number = if fact.ty.is_subtype(Type::NUMBER) {
                    emit_known_number(&mut self.builder, bits)
                } else {
                    emit_js_tagged_number(&mut self.builder, bits, otherwise)
                };
                self.builder.def_var(self.registers.float[index], number);
                state.values[index] = RegisterFact { tagged_current: true,
                    ..RegisterFact::definition(Type::NUMBER, Representation::Float64) };
                number
            }
        }
    }

    fn canonicalize(&mut self, state: &mut State, pc: usize) {
        for register in 0..state.values.len() {
            if self.is_live(pc, register as Register) && !state.values[register].tagged_current {
                self.tagged(state, register as Register);
            }
        }
    }

    /// Leave at `pc` with the interpreter's state: every written register back in the file.
    fn exit(&mut self, state: &mut State, pc: u32) {
        self.canonicalize(state, pc as usize);
        let block = match self.exits.get(&pc) {
            Some(block) => *block,
            None => {
                let block = self.builder.create_block();
                self.builder.set_cold_block(block);
                self.exits.insert(pc, block);
                block
            }
        };
        self.ins().jump(block, &[]);
    }

    /// A guard's failure block: it leaves at `pc` from the state where the guard sits. It is
    /// filled after the region, since the current block is still open.
    fn guard_exit(&mut self, state: &State, pc: u32) -> ir::Block {
        let block = self.builder.create_block();
        self.builder.set_cold_block(block);
        self.pending.push((block, state.clone(), pc));
        block
    }

    fn continue_at(&mut self, state: &mut State, pc: usize) {
        match self.blocks.get(pc).copied().flatten() {
            Some(block) => {
                self.conform(state, pc);
                self.ins().jump(block, &[]);
            }
            None => self.exit(state, pc as u32),
        }
    }

    /// Conform the machine view and discharge the target's semantic requirements.
    fn conform(&mut self, state: &mut State, target: usize) {
        let Some(types) = self.facts.at(target).map(<[Type]>::to_vec) else {
            self.canonicalize(state, target);
            return;
        };
        for (index, ty) in types.iter().copied().enumerate() {
            let register = index as Register;
            if !self.is_live(target, register) { continue; }
            if ty == Type::INT32 && state.values[index].value.representation != Representation::Int32 {
                let exit = self.guard_exit(state, target as u32);
                self.int(state, register, exit);
            } else if ty == Type::NUMBER && state.values[index].value.representation != Representation::Float64 {
                let exit = self.guard_exit(state, target as u32);
                let number = self.number(state, register, exit);
                self.builder.def_var(self.registers.float[index], number);
            } else if ty != Type::INT32 && ty != Type::NUMBER {
                self.tagged(state, register);
            }
            // A fact this edge does not prove and cannot guard leaves at the target, exactly:
            // the edge loses native code, never the body (AGENTS.md, check once).
            if !state.values[index].value.ty.is_subtype(ty) {
                let exit = self.guard_exit(state, target as u32);
                self.builder.ins().jump(exit, &[]);
                let unreached = self.builder.create_block();
                self.builder.switch_to_block(unreached);
                return;
            }
        }
    }

    fn entry_state(&mut self, pc: usize, registers: usize) -> State {
        let mut state = State::canonical(registers);
        if let Some(types) = self.facts.at(pc) {
            for (index, ty) in types.iter().copied().enumerate() {
                if !self.is_live(pc, index as Register) { continue; }
                let representation = if ty == Type::INT32 { Representation::Int32 }
                    else if ty == Type::NUMBER { Representation::Float64 }
                    else { Representation::Tagged };
                let mut fact = RegisterFact::definition(ty, representation);
                if pc == 0 {
                    fact.value.binding = BindingTime::Location;
                    fact.value.proof = if ty == Type::ANY { Proof::CanonicalValue } else { Proof::EntryGuard };
                }
                self.valid &= fact.value.verify();
                state.values[index] = fact;
            }
        }
        state
    }

    fn is_live(&self, pc: usize, register: Register) -> bool {
        self.live.is_none_or(|live| live.is_live(pc, register))
    }

    fn operand_number(&mut self, state: &mut State, operand: Operand, otherwise: ir::Block) -> Option<ir::Value> {
        match operand.kind()? {
            OperandKind::Register => Some(self.number(state, operand.register_index()?, otherwise)),
            OperandKind::Constant => {
                let value = self.program.constants.get(usize::from(operand.payload()))
                    .and_then(super::facts::immediate)?.as_number()?;
                Some(self.ins().f64const(value))
            }
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    /// An operand's int32 value, statically where it is a constant int32.
    fn operand_int(&mut self, state: &mut State, operand: Operand, otherwise: ir::Block) -> Option<ir::Value> {
        match operand.kind()? {
            OperandKind::Register => Some(self.int(state, operand.register_index()?, otherwise)),
            OperandKind::Constant => {
                let constant = self.program.constants.get(usize::from(operand.payload()))?;
                let value = super::facts::immediate(constant)?;
                if super::facts::is_int32(value) {
                    Some(self.ins().iconst(types::I32, i64::from(value.wasm_bits32() as i32)))
                } else {
                    self.ins().jump(otherwise, &[]);
                    let dead = self.builder.create_block();
                    self.builder.switch_to_block(dead);
                    Some(self.ins().iconst(types::I32, 0))
                }
            }
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    fn operand_supported(&self, operand: Operand) -> bool {
        match operand.kind() {
            Some(OperandKind::Register) => operand.register_index().is_some_and(|r| r < self.unit.registers),
            Some(OperandKind::Constant) => self
                .program
                .constants
                .get(usize::from(operand.payload()))
                .and_then(super::facts::immediate)
                .is_some(),
            _ => false,
        }
    }
}

/// The PCs that begin basic blocks: the entry, every jump target and every instruction
/// after a jump.
fn leaders(instructions: &[WideInstruction]) -> Vec<bool> {
    let mut leaders = vec![false; instructions.len()];
    if let Some(first) = leaders.first_mut() {
        *first = true;
    }
    for (pc, instruction) in instructions.iter().enumerate() {
        if matches!(instruction.op().control_flow_layout(), ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump) {
            if let Some(target) = usize::try_from(instruction.jump_target()).ok().filter(|t| *t < leaders.len()) {
                leaders[target] = true;
            }
            if let Some(next) = leaders.get_mut(pc + 1) {
                *next = true;
            }
        }
    }
    leaders
}

/// Specializes `unit` (`lane_unit::unit`), whose PCs `fields` gives the folded receiver
/// fields of.
pub(super) fn compile(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &[Vec<NativeFieldLoad>],
) -> Option<LaneRegion> {
    let isa = super::jit_backend::target_isa("speed")?;
    let (native, count) = lower(program, function_id, unit, helpers, fields, &*isa)?;
    let clif = native_dump_directory().map(|_| native.to_string());
    let code = compile_native_function(native, &*isa, super::js_compiled::MAX_CODE_BYTES, |code| Some(code.to_vec())).ok()?;
    Some(LaneRegion { code, native: count, clif })
}

/// Build the typed residual and validate every value/CFG edge before publication.
fn lower(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &[Vec<NativeFieldLoad>],
    isa: &dyn cranelift_codegen::isa::TargetIsa,
) -> Option<(ir::Function, usize)> {
    let function = program.function_get(function_id as usize)?;
    if fields.iter().flatten().any(|field| !field.valid()) { return None; }
    if unit.instructions.is_empty() || unit.instructions.len() > MAX_JS_GENERAL_DISPATCH_BLOCKS {
        return None;
    }
    let signature = native_entry_signature(isa.pointer_type(), isa.default_call_conv());
    let mut native = ir::Function::with_name_signature(UserFuncName::user(LANE_REGION_NAMESPACE, function_id), signature);
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let entry_block = builder.create_block();
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    let parameters = builder.block_params(entry_block).to_vec();
    let (context, registers_base, result) = (parameters[0], parameters[3], parameters[4]);
    let payload = builder.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        context,
        std::mem::offset_of!(CallContext, adapter_payload) as i32,
    );
    let count = usize::from(unit.registers);
    // Original masks address the caller's PCs. Inlined units conservatively
    // retain caller/callee state until a shared remapped mask is available.
    let live = if unit.frames.is_empty() { program.register_root_masks(function_id) } else { None };
    let registers = Registers {
        tagged: (0..count).map(|_| builder.declare_var(types::I64)).collect(),
        int: (0..count).map(|_| builder.declare_var(types::I32)).collect(),
        float: (0..count).map(|_| builder.declare_var(types::F64)).collect(),
    };
    let zero = builder.ins().iconst(types::I32, 0);
    let float_zero = builder.ins().f64const(0.0);
    let undefined = builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
    for register in 0..count {
        let bits = if register < usize::from(unit.caller_registers)
            && live.is_none_or(|live| live.is_live(0, register as Register)) {
            builder.ins().load(types::I64, MemFlagsData::trusted(), registers_base, register_offset(register as Register))
        } else {
            undefined
        };
        builder.def_var(registers.tagged[register], bits);
        builder.def_var(registers.int[register], zero);
        builder.def_var(registers.float[register], float_zero);
    }
    let leaders = leaders(&unit.instructions);
    let blocks: Vec<_> = leaders.iter().map(|leader| leader.then(|| builder.create_block())).collect();
    let mut lowering = Lowering {
        builder,
        program,
        unit,
        helpers,
        fields,
        context,
        payload,
        registers_base,
        result,
        registers,
        written: BTreeSet::new(),
        blocks,
        exits: BTreeMap::new(),
        entry: None,
        native: 0,
        pending: Vec::new(),
        native_calls: unit.instructions.iter().any(|instruction| instruction.op() == Op::Call),
        facts: super::facts::Facts::default(),
        live,
        valid: true,
    };
    let instructions = unit.instructions.clone();
    for instruction in &instructions {
        lowering.written.extend(writes(*instruction).into_iter().filter(|r| usize::from(*r) < count));
    }
    let mut initial = vec![Type::ANY; count];
    for instruction in &instructions {
        let (operands, required) = match instruction.op() {
            Op::JumpBinaryFalse if nullish_comparison(&program.constants, *instruction, |_| Type::ANY).is_some() => continue,
            Op::JumpBinaryFalse => {
                let operands = [instruction.operand_b(), instruction.operand_c()];
                if super::operations::equality_operator(instruction.binary_operator_field()).is_some()
                    && !operands.iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
                        && program.constants.get(usize::from(operand.payload())).and_then(super::facts::immediate)
                            .and_then(Value::as_number).is_some()) { continue; }
                let floating = operands.iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
                    && super::facts::operand_type(&program.constants, *operand, |_| Type::ANY) == Type::NUMBER);
                (operands, if floating { Type::NUMBER } else { Type::INT32 })
            }
            op if op.js_denotation() == JsDenotation::NumericBinary => {
                let operator = instruction.binary_operator();
                let required = if super::operations::NumberBitwise::from_js_instruction(op, operator).is_some() { Type::INT32 }
                    else if super::operations::NumberArithmetic::from_js_instruction(op, operator).is_some() { Type::NUMBER }
                    else { continue };
                ([instruction.operand_b(), instruction.operand_c()], required)
            }
            _ => continue,
        };
        if operands.iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
            && program.constants.get(usize::from(operand.payload())).and_then(super::facts::immediate)
                .and_then(Value::as_number).is_none()) { continue; }
        for register in operands.into_iter().filter_map(Operand::register_index)
            .filter(|r| (function.parameter_registers.is_some_and(|base|
                *r >= base && *r - base < function.params)
                || function.local_registers.iter().any(|entry|
                    entry.local < function.params && entry.register == *r))
                && !lowering.written.contains(r)) {
            let current = &mut initial[usize::from(register)];
            if *current != Type::INT32 { *current = required; }
        }
    }
    lowering.facts = super::facts::analyze(&initial, &instructions, &leaders, &program.constants, &writes);
    lowering.entry = Some(lowering.entry_facts(&instructions));
    let first = lowering.blocks[0].expect("pc 0 leads");
    let mut state = State::canonical(count);
    lowering.conform(&mut state, 0);
    lowering.builder.ins().jump(first, &[]);
    let mut reachable = false;
    for (pc, instruction) in instructions.iter().copied().enumerate() {
        if let Some(block) = lowering.blocks[pc] {
            if reachable {
                lowering.conform(&mut state, pc);
                lowering.builder.ins().jump(block, &[]);
            }
            lowering.builder.switch_to_block(block);
            state = lowering.entry_state(pc, count);
            reachable = true;
        }
        if !reachable {
            continue;
        }
        reachable = lowering.instruction(&mut state, pc, instruction, &instructions);
    }
    if reachable {
        lowering.exit(&mut state, instructions.len() as u32);
    }
    for (block, mut leaving, pc) in std::mem::take(&mut lowering.pending) {
        lowering.builder.switch_to_block(block);
        lowering.exit(&mut leaving, pc);
    }
    // The exit blocks: write back every caller register the unit writes, and an inlined
    // callee's registers to the scratch window it is made from, then leave at the origin.
    let exits: Vec<_> = lowering.exits.iter().map(|(pc, block)| (*pc, *block)).collect();
    for (pc, block) in exits {
        lowering.builder.switch_to_block(block);
        for register in lowering.written.clone().into_iter().filter(|register| *register < unit.caller_registers) {
            if !lowering.is_live(pc as usize, register) { continue; }
            let bits = lowering.builder.use_var(lowering.registers.tagged[usize::from(register)]);
            lowering.builder.ins().store(MemFlagsData::trusted(), bits, lowering.registers_base, register_offset(register));
        }
        let word = match unit.origins.get(pc as usize).copied().unwrap_or(super::lane_unit::Origin::Caller(unit.end)) {
            super::lane_unit::Origin::Caller(pc) => u64::from(pc),
            super::lane_unit::Origin::Inlined { frame, pc } => {
                let inlined = unit.frames[frame];
                let scratch = emit_js_payload_word(&mut lowering.builder, lowering.payload, std::mem::offset_of!(JsStepDispatch, inlined));
                for register in 0..inlined.registers {
                    let bits = lowering.builder.use_var(lowering.registers.tagged[usize::from(inlined.base + register)]);
                    lowering.builder.ins().store(MemFlagsData::trusted(), bits, scratch, register_offset(register));
                }
                LaneExit::Inlined { frame, pc }.word()
            }
        };
        let pc_value = lowering.builder.ins().iconst(types::I64, word as i64);
        lowering.builder.ins().store(MemFlagsData::trusted(), pc_value, lowering.result, 0);
        let status = lowering.builder.ins().iconst(types::I32, i64::from(NATIVE_JS_STOPPED));
        lowering.builder.ins().return_(&[status]);
    }
    if !lowering.valid { return None; }
    let native_count = lowering.native;
    lowering.builder.seal_all_blocks();
    lowering.builder.finalize(isa.frontend_config());
    Some((native, native_count))
}

impl Lowering<'_, '_> {
    fn entry_facts(&mut self, instructions: &[WideInstruction]) -> Entry {
        let receiver_offset = i32::try_from(std::mem::offset_of!(JsStepDispatch, receiver)).expect("dispatch offset fits i32");
        let receiver = emit_lane_receiver(&mut self.builder, self.payload, receiver_offset);
        let this_bits = self.builder.ins().load(types::I64, MemFlagsData::trusted(), self.payload, receiver_offset);
        let this = self.builder.declare_var(types::I64);
        self.builder.def_var(this, this_bits);
        let mut receiver_shapes = BTreeMap::new();
        for field in self.fields.iter().flatten() {
            if receiver_shapes.contains_key(&field.shape) {
                continue;
            }
            let shape = self.builder.use_var(receiver.shape);
            let same = self.builder.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
            let variable = self.builder.declare_var(types::I8);
            self.builder.def_var(variable, same);
            receiver_shapes.insert(field.shape, variable);
        }
        // A direct store of an atom needs a specialized run and no method cache naming it;
        // nothing in a run changes either (`emit_js_own_field_set`).
        let mut store_atoms = BTreeMap::new();
        for instruction in instructions {
            if matches!(instruction.op(), Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict) && !store_atoms.contains_key(&instruction.atom_index()) {
                let atom = instruction.atom_index();
                let admitted = self.emit_store_atom_admitted(atom);
                let variable = self.builder.declare_var(types::I8);
                self.builder.def_var(variable, admitted);
                store_atoms.insert(atom, variable);
            }
        }
        let mut captures = BTreeMap::new();
        for instruction in instructions {
            if instruction.op() == Op::LoadCapture && crate::bytecode::ImmediateLayout::capture_depth(instruction.imm()) == 0 {
                let slot = crate::bytecode::ImmediateLayout::capture_slot(instruction.imm());
                if captures.contains_key(&slot) {
                    continue;
                }
                let slots = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, captures));
                let slot_value = self.builder.ins().iconst(types::I64, i64::from(slot));
                let mut signature = Signature::new(self.builder.func.signature.call_conv);
                signature.params.extend([AbiParam::new(types::I64), AbiParam::new(types::I64)]);
                signature.returns.push(AbiParam::new(types::I64));
                let signature = self.builder.import_signature(signature);
                let helper = self.builder.ins().iconst(types::I64, lane_capture_read as unsafe extern "C" fn(u64, u64) -> u64 as usize as i64);
                let call = self.builder.ins().call_indirect(signature, helper, &[slots, slot_value]);
                let value = self.builder.inst_results(call)[0];
                let variable = self.builder.declare_var(types::I64);
                self.builder.def_var(variable, value);
                captures.insert(slot, variable);
            }
        }
        Entry { receiver, receiver_shapes, store_atoms, captures, this }
    }

    fn emit_store_atom_admitted(&mut self, atom: Atom) -> ir::Value {
        let specialized = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, stores_specialized));
        let specialized = self.builder.ins().icmp_imm_u(IntCC::NotEqual, specialized, 0);
        let word_index = atom / u64::BITS;
        let words = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, method_cache_atoms));
        let words_len = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, method_cache_atom_words));
        let covered = self.builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, words_len, i64::from(word_index));
        let present = self.builder.create_block();
        let join = self.builder.create_block();
        let free = self.builder.append_block_param(join, types::I8);
        let one = self.builder.ins().iconst(types::I8, 1);
        self.builder.ins().brif(covered, present, &[], join, &[ir::BlockArg::Value(one)]);
        self.builder.switch_to_block(present);
        let word = self.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            words,
            i32::try_from(word_index as usize * std::mem::size_of::<u64>()).expect("atom word offset fits i32"),
        );
        let named = self.builder.ins().band_imm_u(word, (1_u64 << (atom % u64::BITS)) as i64);
        let unnamed = self.builder.ins().icmp_imm_u(IntCC::Equal, named, 0);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(unnamed)]);
        self.builder.switch_to_block(join);
        self.builder.ins().band(specialized, free)
    }

    /// Lower the instruction at `pc`; whether control falls through to the next.
    fn instruction(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, instructions: &[WideInstruction]) -> bool {
        let pc32 = pc as u32;
        let plain = instruction.result_flags() == 0;
        let ran = match instruction.op() {
            Op::Call if plain && !instruction.direct_eval() && !instruction.parameter_eval() => Some(self.call(state, pc, instruction)),
            Op::Nop if plain => match self.unit.markers.iter().position(|marker| *marker == pc) {
                Some(frame) => Some(self.inline_call(state, pc, frame)),
                None => Some(true),
            },
            Op::LoadConst if plain => {
                let value = self.program.constants.get(instruction.constant_index()).and_then(super::facts::immediate);
                value.map(|value| {
                    let ty = Type::immediate(value);
                    let (representation, native) = if let Some(int) = value.as_int() {
                        (Representation::Int32, self.ins().iconst(types::I32, i64::from(int)))
                    } else if let Some(number) = value.as_number() {
                        (Representation::Float64, self.ins().f64const(number))
                    } else {
                        (Representation::Tagged, self.ins().iconst(types::I64, value.to_bits() as i64))
                    };
                    let mut fact = RegisterFact::definition(ty, representation);
                    fact.value.binding = BindingTime::Static;
                    fact.value.proof = Proof::Static;
                    self.define(state, instruction.result_register(), native, fact);
                    true
                })
            }
            Op::Move if plain => {
                self.copy(state, instruction.result_register(), instruction.register_b());
                Some(true)
            }
            Op::LoadThis if plain && !instruction.writes_current_this() => {
                let exit = self.guard_exit(state, pc32);
                let entry = self.entry.as_ref().expect("entry facts");
                let this = self.builder.use_var(entry.this);
                let bound = self.ins().icmp_imm_u(IntCC::NotEqual, this, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, bound, exit);
                self.set_tagged(state, instruction.result_register(), this);
                state.receivers.insert(instruction.result_register());
                Some(true)
            }
            Op::LoadCapture if plain && crate::bytecode::ImmediateLayout::capture_depth(instruction.imm()) == 0 => {
                let slot = crate::bytecode::ImmediateLayout::capture_slot(instruction.imm());
                let variable = self.entry.as_ref().and_then(|entry| entry.captures.get(&slot)).copied();
                variable.map(|variable| {
                    let exit = self.guard_exit(state, pc32);
                    let value = self.builder.use_var(variable);
                    let read = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
                    emit_js_guard(&mut self.builder, read, exit);
                    self.set_tagged(state, instruction.result_register(), value);
                    true
                })
            }
            Op::GetField if plain && !instruction.writes_current_this() => self.get_field(state, pc, instruction),
            Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict if plain => self.set_field(state, pc, instruction),
            op if op.js_denotation() == JsDenotation::NumericBinary && instruction.numeric_local_target().is_none() => {
                self.binary(state, pc, instruction, instructions)
            }
            Op::IncDec if plain && instruction.numeric_local_target().is_none() => {
                let decrement = instruction.boolean_flag();
                decrement.map(|decrement| {
                    let exit = self.guard_exit(state, pc32);
                    let value = self.int(state, instruction.register_b(), exit);
                    let (result, overflow) = if decrement {
                        let one = self.ins().iconst(types::I32, 1);
                        self.ins().ssub_overflow(value, one)
                    } else {
                        let one = self.ins().iconst(types::I32, 1);
                        self.ins().sadd_overflow(value, one)
                    };
                    emit_js_guard_not(&mut self.builder, overflow, exit);
                    self.set_int(state, instruction.result_register(), result);
                    true
                })
            }
            Op::Jump if plain => {
                let target = instruction.jump_target() as usize;
                if target <= pc {
                    self.interrupt_check(state, pc32);
                }
                self.continue_at(state, target);
                return false;
            }
            Op::JumpFalse | Op::JumpTrue if plain => {
                let take_when_truthy = instruction.op() == Op::JumpTrue;
                let exit = self.guard_exit(state, pc32);
                let register = instruction.register_a();
                let fact = state.values[usize::from(register)].value;
                let truthy = match fact.representation {
                    Representation::Int32 => {
                        let int = self.builder.use_var(self.registers.int[usize::from(register)]);
                        self.ins().icmp_imm_u(IntCC::NotEqual, int, 0)
                    }
                    Representation::Float64 => {
                        let number = self.builder.use_var(self.registers.float[usize::from(register)]);
                        emit_number_truthiness(&mut self.builder, number)
                    }
                    Representation::Tagged => {
                        let bits = self.tagged(state, register);
                        if fact.ty.is_subtype(Type::language(crate::ValueType::Boolean)) {
                            self.ins().icmp_imm_u(IntCC::Equal, bits, Value::TRUE.to_bits() as i64)
                        } else {
                            emit_js_tagged_truthiness(&mut self.builder, bits, exit)
                        }
                    }
                };
                let taken = if take_when_truthy { truthy } else { self.ins().icmp_imm_u(IntCC::Equal, truthy, 0) };
                self.branch(state, pc, instruction.jump_target() as usize, taken);
                return false;
            }
            Op::JumpBinaryFalse if plain => {
                let operator = instruction.binary_operator_field();
                let (left, right) = (instruction.operand_b(), instruction.operand_c());
                if let Some(comparison) = nullish_comparison(&self.program.constants, instruction,
                    |register| state.values[usize::from(register)].value.ty) {
                    let ty = state.values[usize::from(comparison.source)].value.ty;
                    let taken = if let Some(taken) = comparison.known_outcome(ty) {
                        self.ins().iconst(types::I8, i64::from(taken))
                    } else {
                        let exit = self.guard_exit(state, pc32);
                        let value = self.tagged(state, comparison.source);
                        emit_nullish_branch(&mut self.builder, value, comparison, exit)
                    };
                    let target = instruction.jump_target() as usize;
                    if target <= pc { self.interrupt_check(state, pc32); }
                    self.branch(state, pc, target, taken);
                    return false;
                }
                if super::operations::NumberRelation::from_operator(operator).is_none()
                    || !self.operand_supported(left)
                    || !self.operand_supported(right)
                    || [left, right].iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
                        && !super::facts::operand_type(&self.program.constants, *operand, |_| Type::ANY).is_subtype(Type::NUMBER))
                {
                    None
                } else {
                    let exit = self.guard_exit(state, pc32);
                    let integer = [left, right].iter().all(|operand| super::facts::operand_type(
                        &self.program.constants, *operand, |register| state.values[usize::from(register)].value.ty)
                        .is_subtype(Type::INT32));
                    let (left, right, domain) = if integer {
                        (self.operand_int(state, left, exit).expect("supported operand"),
                            self.operand_int(state, right, exit).expect("supported operand"), NumberRelationDomain::Int32)
                    } else {
                        (self.operand_number(state, left, exit).expect("numeric operand"),
                            self.operand_number(state, right, exit).expect("numeric operand"), NumberRelationDomain::Float64)
                    };
                    let holds = emit_number_relation(&mut self.builder, operator, left, right, domain)
                        .expect("a numeric relation");
                    let taken = self.ins().icmp_imm_u(IntCC::Equal, holds, 0);
                    let target = instruction.jump_target() as usize;
                    if target <= pc {
                        self.interrupt_check(state, pc32);
                    }
                    self.branch(state, pc, target, taken);
                    return false;
                }
            }
            _ => None,
        };
        match ran {
            Some(falls) => {
                self.native += 1;
                falls
            }
            None => {
                self.exit(state, pc32);
                false
            }
        }
    }

    /// `source`'s value into `destination`, with every fact proved of it.
    fn copy(&mut self, state: &mut State, destination: Register, source: Register) {
        let fact = state.values[usize::from(source)];
        let variable = match fact.value.representation {
            Representation::Tagged => self.registers.tagged[usize::from(source)],
            Representation::Int32 => self.registers.int[usize::from(source)],
            Representation::Float64 => self.registers.float[usize::from(source)],
        };
        let value = self.builder.use_var(variable);
        let tagged = (fact.tagged_current && fact.value.representation != Representation::Tagged)
            .then(|| self.builder.use_var(self.registers.tagged[usize::from(source)]));
        let shape = state.shapes.get(&source).copied();
        let receiver = state.receivers.contains(&source);
        let layouts: Vec<_> = state.layouts.iter().filter_map(|((owner, shape), projection)|
            (*owner == Some(source)).then_some((*shape, *projection))).collect();
        self.define(state, destination, value, fact);
        if let Some(bits) = tagged { self.builder.def_var(self.registers.tagged[usize::from(destination)], bits); }
        if let Some(shape) = shape { state.shapes.insert(destination, shape); }
        for (shape, projection) in layouts { state.layouts.insert((Some(destination), shape), projection); }
        if receiver { state.receivers.insert(destination); }
    }

    /// The call marker of inlined `frame` (`lane_unit::unit`): its callee is the function
    /// the call's cache linked and its receiver a direct object, which a plain call binds as
    /// `this`; anything else leaves at the call, before its effects. The callee's parameters
    /// take the call's arguments and its other registers `undefined`, as its activation
    /// would (`lane_push_passed`).
    fn inline_call(&mut self, state: &mut State, pc: usize, frame: usize) -> bool {
        let inlined = self.unit.frames[frame];
        let exit = self.guard_exit(state, pc as u32);
        let callee = self.tagged(state, inlined.callee_register);
        let same = self.ins().icmp_imm_u(IntCC::Equal, callee, inlined.target.callee.to_bits() as i64);
        emit_js_guard(&mut self.builder, same, exit);
        let receiver = self.tagged(state, inlined.receiver);
        let family = self.ins().band_imm_u(receiver, crate::value::ValueLayout::HEAP_FAMILY_MASK as i64);
        let object = self.ins().icmp_imm_u(IntCC::Equal, family, crate::value::ValueLayout::DIRECT_OBJECT_TAG as i64);
        emit_js_guard(&mut self.builder, object, exit);
        for register in 0..inlined.registers {
            let destination = inlined.base + register;
            let undefined = self.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
            self.set_tagged(state, destination, undefined);
        }
        let callee = self.program.function(inlined.target.function as usize);
        for argument in 0..inlined.arguments.min(callee.params) {
            if let Some(register) = callee.parameter_registers.map(|base| base + argument)
                .or_else(|| callee.promoted_register(argument)) {
                self.copy(state, inlined.base + register, inlined.window + argument);
            }
        }
        true
    }

    /// A call the unit does not inline, made natively (`dispatch_fast::lane_body_call`) from
    /// the caller's registers, which it writes back first: a completed call's result is in
    /// its register; a missed one leaves at the call; one that left the lane in its callee's
    /// activation leaves the body there too (`LaneExit::Callee`).
    fn call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(super::lane_unit::Origin::Caller(call_pc)) = self.unit.origins.get(pc).copied() else {
            self.exit(state, pc as u32);
            return false;
        };
        let missed = self.guard_exit(state, pc as u32);
        self.canonicalize(state, pc);
        let caller_registers = self.unit.caller_registers;
        for register in self.written.clone().into_iter().filter(|register| *register < caller_registers) {
            if !self.is_live(pc, register) { continue; }
            let bits = self.builder.use_var(self.registers.tagged[usize::from(register)]);
            let base = self.registers_base;
            self.ins().store(MemFlagsData::trusted(), bits, base, register_offset(register));
        }
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
        let call_pc = self.ins().iconst(types::I64, i64::from(call_pc));
        let status = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.call, &[context, call_pc]);
        let completed = self.builder.create_block();
        let other = self.builder.create_block();
        let left = self.builder.create_block();
        self.builder.set_cold_block(left);
        let done = self.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_COMPLETED as i64);
        self.ins().brif(done, completed, &[], other, &[]);
        self.builder.switch_to_block(other);
        let miss = self.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_MISSED as i64);
        self.ins().brif(miss, missed, &[], left, &[]);
        self.builder.switch_to_block(left);
        let word = self.ins().iconst(types::I64, LaneExit::Callee.word() as i64);
        let result = self.result;
        self.ins().store(MemFlagsData::trusted(), word, result, 0);
        let stopped = self.ins().iconst(types::I32, i64::from(NATIVE_JS_STOPPED));
        self.ins().return_(&[stopped]);
        self.builder.switch_to_block(completed);
        // The callee may have changed any object: block-local proofs of them retire.
        state.shapes.clear();
        state.layouts.clear();
        let destination = instruction.result_register();
        let base = self.registers_base;
        let bits = self.ins().load(types::I64, MemFlagsData::trusted(), base, register_offset(destination));
        self.set_tagged(state, destination, bits);
        true
    }

    fn interrupt_check(&mut self, state: &State, pc: u32) {
        let exit = self.guard_exit(state, pc);
        let context = self.context;
        let flag = self.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            context,
            std::mem::offset_of!(CallContext, interrupt) as i32,
        );
        let requested = self.ins().load(types::I8, MemFlagsData::trusted(), flag, 0);
        let clear = self.ins().icmp_imm_u(IntCC::Equal, requested, 0);
        emit_js_guard(&mut self.builder, clear, exit);
    }

    fn branch(&mut self, state: &mut State, pc: usize, target: usize, taken: ir::Value) {
        let taken_block = self.builder.create_block();
        let fall_block = self.builder.create_block();
        self.ins().brif(taken, taken_block, &[], fall_block, &[]);
        self.builder.switch_to_block(taken_block);
        let mut taken_state = state.clone();
        self.continue_at(&mut taken_state, target);
        self.builder.switch_to_block(fall_block);
        let mut fall_state = state.clone();
        self.continue_at(&mut fall_state, pc + 1);
    }

    fn get_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let Some(FieldLookup::Atom { base, cache_site, .. }) = instruction.field_lookup() else { return None };
        if instruction.returns_from_frame() {
            return None;
        }
        let destination = instruction.result_register();
        let exit = self.guard_exit(state, pc as u32);
        let shapes = self.fields.get(pc).cloned().unwrap_or_default();
        let register = base.register_index();
        if register.is_some_and(|register| register >= self.unit.registers) {
            return None;
        }
        let receiver = !self.native_calls
            && (base == crate::bytecode::FieldBase::THIS
                || register.is_some_and(|register| state.receivers.contains(&register)));
        if !shapes.is_empty() && (receiver || register.is_some()) {
            // The site's proved shapes and slots, folded: one shape load of the object (the
            // receiver's was proved at entry), a compare per shape, a direct read.
            let (object, shape, inline) = if receiver {
                let entry = self.entry.as_ref().expect("entry facts");
                (
                    self.builder.use_var(entry.receiver.object),
                    self.builder.use_var(entry.receiver.shape),
                    self.builder.use_var(entry.receiver.inline),
                )
            } else {
                self.object_record(state, register.expect("a register base"), exit)
            };
            let owner = if receiver { None } else { register };
            let value = self.shape_switch_read(state, owner, object, shape, inline, &shapes, exit)?;
            self.set_tagged(state, destination, value);
            return Some(true);
        }
        let object_bits = if register.is_none() {
            let entry = self.entry.as_ref().expect("entry facts");
            self.builder.use_var(entry.this)
        } else {
            self.tagged(state, register?)
        };
        let value = self.field_probe(object_bits, cache_site, exit);
        self.set_tagged(state, destination, value);
        Some(true)
    }

    /// Read the slot `shapes` names for the record's `shape`; `exit` for any other shape.
    fn shape_switch_read(
        &mut self,
        state: &mut State,
        owner: Option<Register>,
        object: ir::Value,
        shape: ir::Value,
        inline: ir::Value,
        shapes: &[NativeFieldLoad],
        exit: ir::Block,
    ) -> Option<ir::Value> {
        let entry = self.entry.as_ref().expect("entry facts");
        let properties = self.builder.use_var(entry.receiver.properties);
        let properties_len = self.builder.use_var(entry.receiver.properties_len);
        if let [field] = shapes {
            let key = (owner, field.shape);
            let projection = match state.layouts.get(&key).copied() {
                Some(projection) if usize::from(field.slot) < projection.storage_len => projection,
                _ => {
                    let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
                    emit_js_guard(&mut self.builder, same, exit);
                    let projection = CheckedObject::prove(&mut self.builder, object, inline, properties,
                        properties_len, *field, PackedAccess::Reload, exit)?;
                    state.layouts.insert(key, projection);
                    projection
                }
            };
            return Some(projection.read(&mut self.builder, field.slot));
        }
        let join = self.builder.create_block();
        let value = self.builder.append_block_param(join, types::I64);
        for field in shapes {
            let matched = self.builder.create_block();
            let next = self.builder.create_block();
            let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
            self.ins().brif(same, matched, &[], next, &[]);
            self.builder.switch_to_block(matched);
            let projection = CheckedObject::prove(&mut self.builder, object, inline, properties,
                properties_len, *field, PackedAccess::Reload, exit)?;
            let read = projection.read(&mut self.builder, field.slot);
            self.ins().jump(join, &[ir::BlockArg::Value(read)]);
            self.builder.switch_to_block(next);
        }
        self.ins().jump(exit, &[]);
        self.builder.switch_to_block(join);
        Some(value)
    }

    /// The record address, exact shape and inline-storage bits of the direct object in
    /// `register`, proved once per block (`State::shapes`).
    fn object_record(&mut self, state: &mut State, register: Register, exit: ir::Block) -> (ir::Value, ir::Value, ir::Value) {
        if let Some(known) = state.shapes.get(&register).copied() { return known; }
        let bits = self.tagged(state, register);
        let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
        let header = ObjectHeader::prove(&mut self.builder, bits, records, exit);
        let projection = (header.record, header.shape, header.inline);
        state.shapes.insert(register, projection);
        state.values[usize::from(register)] = RegisterFact::definition(
            Type::language(crate::ValueType::Object), Representation::Tagged);
        projection
    }

    /// The VM's own field-cache read (`Vm::cached_field_hit`), prototype holders included;
    /// a miss leaves through `exit`.
    fn field_probe(&mut self, object: ir::Value, site: u16, exit: ir::Block) -> ir::Value {
        let site = self.ins().iconst(types::I64, i64::from(site));
        let value = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.field_get, &[object, site]);
        let hit = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, hit, exit);
        value
    }

    fn set_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let source = instruction.register_a();
        if source >= self.unit.registers || instruction.writes_current_this() || instruction.returns_from_frame() {
            return None;
        }
        let register = if matches!(instruction.op(), Op::SetField | Op::SetFieldStrict) {
            Some(instruction.register_b())
        } else if let Some(super::lane_unit::Origin::Inlined { frame, .. }) = self.unit.origins.get(pc) {
            Some(self.unit.frames[*frame].receiver)
        } else { None };
        if register.is_some_and(|register| register >= self.unit.registers) { return None; }
        let receiver = register.is_none() || (!self.native_calls && register.is_some_and(|register| state.receivers.contains(&register)));
        let owner = if receiver { None } else { register };
        let exit = self.guard_exit(state, pc as u32);
        let atom = instruction.atom_index();
        let barrier_free = state.values[usize::from(source)].value.ty.is_subtype(Type::NUMBER);
        let value = self.tagged(state, source);
        if self.native_calls && register.is_none() {
            let this = self.builder.use_var(self.entry.as_ref().expect("entry facts").this);
            self.store_helper(this, atom, instruction.cache_site_index(), value, exit);
            return Some(true);
        }
        let object_bits = if receiver {
            self.builder.use_var(self.entry.as_ref().expect("entry facts").this)
        } else { self.tagged(state, register.expect("register receiver")) };
        let field = self.fields.get(pc).and_then(|shapes| (shapes.len() == 1).then(|| shapes[0]));
        let header = if field.is_some() && !receiver {
            Some(self.object_record(state, register.expect("register receiver"), exit))
        } else { None };
        let entry = self.entry.as_ref().expect("entry facts");
        if let Some(field) = field {
            let projection = state.layouts.get(&(owner, field.shape)).copied();
            let admitted = self.builder.use_var(entry.store_atoms[&atom]);
            let (object, shape, inline) = header.unwrap_or_else(|| (
                self.builder.use_var(entry.receiver.object),
                self.builder.use_var(entry.receiver.shape),
                self.builder.use_var(entry.receiver.inline),
            ));
            let properties = self.builder.use_var(entry.receiver.properties);
            let properties_len = self.builder.use_var(entry.receiver.properties_len);
            let direct = self.builder.create_block();
            let general = self.builder.create_block();
            let done = self.builder.create_block();
            let ok = if projection.is_some() { admitted } else {
                let same = if receiver { self.builder.use_var(entry.receiver_shapes[&field.shape]) }
                    else { self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape)) };
                self.ins().band(same, admitted)
            };
            self.ins().brif(ok, direct, &[], general, &[]);
            self.builder.switch_to_block(direct);
            if !barrier_free {
                let record_id = self.ins().ireduce(types::I32, object_bits);
                let record_id = self.ins().uextend(types::I64, record_id);
                let payload = self.payload;
                emit_unlogged_property_store_guard(&mut self.builder, payload, record_id, value, general);
            }
            let projection = match projection {
                Some(projection) => projection,
                None => {
                    let plan = NativeFieldLoad { storage: Some(crate::heap::PlainPropertyStorage::Arena), ..field };
                    CheckedObject::prove(&mut self.builder, object, inline, properties,
                        properties_len, plan, PackedAccess::Reload, general)?
                }
            };
            let Some(address) = projection.arena_address(&mut self.builder, field.slot, general) else { return None; };
            self.ins().store(MemFlagsData::trusted(), value, address, 0);
            self.ins().jump(done, &[]);
            self.builder.switch_to_block(general);
            self.store_helper(object_bits, atom, instruction.cache_site_index(), value, exit);
            self.ins().jump(done, &[]);
            self.builder.switch_to_block(done);
            return Some(true);
        }
        self.store_helper(object_bits, atom, instruction.cache_site_index(), value, exit);
        Some(true)
    }

    /// The lane's logged own-field store hit (`Vm::primary_own_field_store_hit`), barrier
    /// included; anything else leaves through `exit`.
    fn store_helper(&mut self, object: ir::Value, atom: Atom, site: u16, value: ir::Value, exit: ir::Block) {
        let atom = self.ins().iconst(types::I64, i64::from(atom));
        let site = self.ins().iconst(types::I64, i64::from(site));
        let stored = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.field_set, &[object, atom, site, value]);
        let ok = self.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
        emit_js_guard(&mut self.builder, ok, exit);
    }

    fn binary(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, instructions: &[WideInstruction]) -> Option<bool> {
        let returns = instruction.returns_from_frame();
        if instruction.writes_current_this() || (instruction.result_flags() != 0 && !returns) {
            return None;
        }
        // A returning result completes at the terminal `Return`, whose register is free.
        let (destination, after) = if returns {
            let terminal = instructions.len().checked_sub(1)?;
            let last = instructions[terminal];
            (last.op() == Op::Return && last.result_flags() == 0).then_some((last.register_a(), Some(terminal)))?
        } else {
            (instruction.result_register(), None)
        };
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        if !self.operand_supported(left) || !self.operand_supported(right) {
            return None;
        }
        let operator = instruction.binary_operator();
        let arithmetic = super::operations::NumberArithmetic::from_js_instruction(instruction.op(), operator);
        let bitwise = super::operations::NumberBitwise::from_js_instruction(instruction.op(), operator);
        if [left, right].iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
            && self.program.constants.get(usize::from(operand.payload())).and_then(super::facts::immediate).and_then(Value::as_number).is_none()) {
            return None;
        }
        let exit = self.guard_exit(state, pc as u32);
        match (arithmetic, bitwise) {
            (Some(arithmetic), _) => {
                let float_operand = [left, right].iter().any(|operand| match operand.kind() {
                    Some(OperandKind::Register) => state.values[usize::from(operand.payload())].value.representation == Representation::Float64,
                    Some(OperandKind::Constant) => self.program.constants.get(usize::from(operand.payload()))
                        .and_then(super::facts::immediate).is_some_and(|value| !super::facts::is_int32(value)),
                    _ => false,
                });
                if arithmetic == super::operations::NumberArithmetic::Divide || float_operand {
                    let left = self.operand_number(state, left, exit)?;
                    let right = self.operand_number(state, right, exit)?;
                    let number = emit_number_arithmetic_operation(&mut self.builder, arithmetic, left, right);
                    self.set_number(state, destination, number);
                } else {
                    let float = self.builder.create_block();
                    self.builder.set_cold_block(float);
                    let mut float_state = state.clone();
                    let left_int = self.operand_int(state, left, float).expect("supported operand");
                    let right_int = self.operand_int(state, right, float).expect("supported operand");
                    let (result, overflow) = arithmetic.apply(&mut EmitArithmetic::<true> { builder: &mut self.builder }, left_int, right_int);
                    let int_done = self.builder.create_block();
                    self.ins().brif(overflow, float, &[], int_done, &[]);
                    self.builder.switch_to_block(int_done);
                    if arithmetic == super::operations::NumberArithmetic::Multiply {
                        let zero = self.ins().icmp_imm_u(IntCC::Equal, result, 0);
                        let signs = self.ins().bor(left_int, right_int);
                        let negative = self.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
                        let negative_zero = self.ins().band(zero, negative);
                        emit_js_guard_not(&mut self.builder, negative_zero, float);
                    }
                    let join = self.builder.create_block();
                    let number = self.builder.append_block_param(join, types::F64);
                    let result = self.ins().fcvt_from_sint(types::F64, result);
                    self.ins().jump(join, &[ir::BlockArg::Value(result)]);
                    self.builder.switch_to_block(float);
                    let left_number = self.operand_number(&mut float_state, left, exit)?;
                    let right_number = self.operand_number(&mut float_state, right, exit)?;
                    let result = emit_number_arithmetic_operation(&mut self.builder, arithmetic, left_number, right_number);
                    self.ins().jump(join, &[ir::BlockArg::Value(result)]);
                    self.builder.switch_to_block(join);
                    for (index, incoming) in float_state.values.iter().copied().enumerate() {
                        let current = state.values[index];
                        if current != incoming {
                            let representation = if current.value.representation == incoming.value.representation {
                                current.value.representation
                            } else {
                                self.valid &= current.tagged_current && incoming.tagged_current;
                                Representation::Tagged
                            };
                            let mut fact = RegisterFact::definition(current.value.ty.join(incoming.value.ty), representation);
                            fact.tagged_current = current.tagged_current && incoming.tagged_current;
                            self.valid &= fact.value.verify();
                            state.values[index] = fact;
                        }
                    }
                    self.set_number(state, destination, number);
                }
            }
            (_, Some(bitwise)) => {
                let left_int = self.operand_int(state, left, exit).expect("supported operand");
                let right_int = self.operand_int(state, right, exit).expect("supported operand");
                if bitwise == super::operations::NumberBitwise::ShiftRightUnsigned {
                    let number = emit_number_bitwise_operation(&mut self.builder, bitwise, left_int, right_int);
                    self.set_number(state, destination, number);
                } else {
                    let result = emit_number_bitwise_bits(&mut self.builder, bitwise, left_int, right_int);
                    self.set_int(state, destination, result);
                }
            }
            _ => return None,
        }
        if let Some(terminal) = after {
            self.native += 1;
            self.continue_at(state, terminal);
            return Some(false);
        }
        Some(true)
    }
}

/// Continue only when `condition` is false; `fallback` otherwise.
fn emit_js_guard_not(builder: &mut FunctionBuilder<'_>, condition: ir::Value, fallback: ir::Block) {
    let passed = builder.create_block();
    builder.ins().brif(condition, fallback, &[], passed, &[]);
    builder.switch_to_block(passed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliased_own_fields_share_the_complete_storage_bound() {
        let program = crate::Engine::specialize(
            include_str!("../../../tests/fixtures/own-slot-proofs.js"), "own-slot-proofs.js",
        ).unwrap();
        let id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "readPair")).unwrap();
        let fields: Vec<_> = program.function(id).instructions().map(|instruction| {
            if instruction.op() == Op::GetField
                && let Some(FieldLookup::Atom { atom, .. }) = instruction.field_lookup()
                && let Some(slot) = ["left", "right"].iter().position(|name| *name == &program.atoms[atom as usize]) {
                vec![NativeFieldLoad { shape: 0, slot: slot as u16, storage_len: 2, storage: None }]
            } else { Vec::new() }
        }).collect();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let unit = super::super::lane_unit::unit(program.function(id), &[], &|_| None).unwrap();
        let (native, _) = lower(&program, id as u32,
            &unit, LaneHelpers { field_get: 0, field_set: 0, call: 0 }, &fields, &*isa).unwrap();
        let bounds = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
            .filter(|&instruction| {
                let ir::InstructionData::IntCompare { cond: IntCC::UnsignedLessThanOrEqual, args, .. }
                    = native.dfg.insts[instruction] else { return false };
                let ir::ValueDef::Result(length, _) = native.dfg.value_def(native.dfg.resolve_aliases(args[1]))
                    else { return false };
                matches!(native.dfg.insts[length], ir::InstructionData::Load { offset, .. }
                    if i32::from(offset) == std::mem::offset_of!(JsStepDispatch, property_len) as i32)
            }).count();
        assert_eq!(bounds, 1, "two aliased own fields share one complete arena bound:\n{native}");
    }

    #[test]
    fn number_loops_keep_f64_values_and_proofs_across_the_backedge() {
        let program = crate::Engine::specialize(
            include_str!("../../../tests/fixtures/typed-lane-numbers.js"), "typed-lane-numbers.js",
        ).unwrap();
        let id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "accumulate")).unwrap();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let unit = super::super::lane_unit::unit(program.function(id), &[], &|_| None).unwrap();
        let (native, count) = lower(&program, id as u32,
            &unit, LaneHelpers { field_get: 0, field_set: 0, call: 0 }, &[], &*isa).expect("typed Number residual");
        assert!(count > 0);
        let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(&native);
        let mut dominators = cranelift_codegen::dominator_tree::DominatorTree::new();
        dominators.compute(&native, &cfg);
        let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
        loops.compute(&native, &cfg, &dominators);
        assert_eq!(loops.loops().count(), 1, "{native}");
        let mut arithmetic = 0;
        for block in native.layout.blocks().filter(|block| loops.innermost_loop(*block).is_some()) {
            for instruction in native.layout.block_insts(block) {
                let opcode = native.dfg.insts[instruction].opcode();
                assert!(!matches!(opcode, ir::Opcode::Bitcast | ir::Opcode::Band),
                    "no boxing/unboxing or repeated tag check in the Number loop: {instruction}: {native}");
                arithmetic += usize::from(matches!(opcode, ir::Opcode::Fadd | ir::Opcode::Fsub | ir::Opcode::Fmul));
            }
        }
        assert_eq!(arithmetic, 3, "{native}");
    }
}
