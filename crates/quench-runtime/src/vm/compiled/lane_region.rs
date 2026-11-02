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
/// The executable memory a lane body may take: what a unit at its instruction bound
/// (`lane_unit::MAX_UNIT_INLINED_INSTRUCTIONS`) lowers to, at the bytes per instruction its
/// guards and exits cost. A larger body declines, and its function runs in the lane.
const LANE_BODY_CODE_BYTES_PER_INSTRUCTION: usize = 128;
const LANE_BODY_MAX_CODE_BYTES: usize =
    super::lane_unit::MAX_UNIT_INLINED_INSTRUCTIONS * LANE_BODY_CODE_BYTES_PER_INSTRUCTION;

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
    /// Each register's int32 and float64 views, declared at their first use: most registers
    /// never take one, and the SSA builder's tables grow with variables times blocks. A view
    /// read before any definition reads zero, as only a state that defined it reads it.
    int: Vec<Option<Variable>>,
    float: Vec<Option<Variable>>,
}

/// The compile-time state of the instruction being lowered.
#[derive(Clone)]
struct State {
    values: Vec<RegisterFact>,
    /// Registers proved to hold a direct object, this block: its record address and shape.
    /// The proved records of objects by owner: a register's, or `this`'s (`None`).
    shapes: BTreeMap<Option<Register>, (ir::Value, ir::Value, ir::Value)>,
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
    capture_values: Variable,
    capture_len: Variable,
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
    /// The proved shapes and slots of each field cache site the unit reads or writes.
    fields: &'a BTreeMap<u16, Vec<NativeFieldLoad>>,
    /// The calls, by unit PC, that call a callee's compiled entry (`DirectCallee`).
    direct: &'a BTreeMap<usize, *const DirectCallee>,
    /// Per inlined frame, its callee's dense capture slots (values and count), fixed by the
    /// callee's identity, which its call guards; null where they are not dense.
    frame_captures: &'a [(usize, usize)],
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
    /// The caller's register liveness, by its PCs; per inlined frame, its callee's
    /// (`Lowering::is_live`). A function without masks keeps every register live.
    live: Option<&'a crate::compile::liveness::RegisterMasks>,
    frame_live: Vec<Option<&'a crate::compile::liveness::RegisterMasks>>,
    valid: bool,
    /// The one epilogue every stop shares: it returns its status parameter.
    leave: ir::Block,
    /// The stop at a record: it writes its result word parameter and leaves stopped.
    stop: ir::Block,
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
                    let int = { let variable = self.int_var(index); self.builder.use_var(variable) };
                    int_tagged(&mut self.builder, int)
                }
                Representation::Float64 => {
                    let number = { let variable = self.float_var(index); self.builder.use_var(variable) };
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
            Representation::Int32 => self.int_var(index),
            Representation::Float64 => self.float_var(index),
        };
        self.builder.def_var(variable, value);
        state.values[index] = fact;
        state.shapes.remove(&Some(register));
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
            return { let variable = self.int_var(index); self.builder.use_var(variable) };
        }
        let bits = self.tagged(state, register);
        if !state.values[index].value.ty.is_subtype(Type::INT32) {
            let integer = is_int_tagged(&mut self.builder, bits);
            emit_js_guard(&mut self.builder, integer, otherwise);
        }
        let int = self.ins().ireduce(types::I32, bits);
        { let variable = self.int_var(index); self.builder.def_var(variable, int); }
        state.values[index] = RegisterFact { tagged_current: true,
            ..RegisterFact::definition(Type::INT32, Representation::Int32) };
        int
    }

    /// Prove Number only for a new untrusted value; later uses read its unboxed view.
    fn number(&mut self, state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
        let index = usize::from(register);
        let fact = state.values[index].value;
        match fact.representation {
            Representation::Float64 => { let variable = self.float_var(index); self.builder.use_var(variable) },
            Representation::Int32 => {
                let int = { let variable = self.int_var(index); self.builder.use_var(variable) };
                self.ins().fcvt_from_sint(types::F64, int)
            }
            Representation::Tagged => {
                let bits = self.tagged(state, register);
                let number = if fact.ty.is_subtype(Type::NUMBER) {
                    emit_known_number(&mut self.builder, bits)
                } else {
                    emit_js_tagged_number(&mut self.builder, bits, otherwise)
                };
                { let variable = self.float_var(index); self.builder.def_var(variable, number); }
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
                { let variable = self.float_var(index); self.builder.def_var(variable, number); }
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

    /// Whether unit `register` is live at unit `pc`: the frame that owns it is in the chain of
    /// frames `pc` runs in, and its function's masks keep it live at the frame's own PC there
    /// (the PC itself for the innermost, its call's for each frame around it).
    fn is_live(&self, pc: usize, register: Register) -> bool {
        let (mut frame, mut at) = match self.unit.origins.get(pc).copied() {
            Some(super::lane_unit::Origin::Inlined { frame, pc }) => (Some(frame), pc),
            Some(super::lane_unit::Origin::Caller(pc)) => (None, pc),
            None => (None, self.unit.end),
        };
        while let Some(index) = frame {
            let inlined = self.unit.frames[index];
            if (inlined.base..inlined.base + inlined.registers).contains(&register) {
                return self.frame_live[index].is_none_or(|live| live.is_live(at as usize, register - inlined.base));
            }
            (frame, at) = (inlined.parent, inlined.call);
        }
        register < self.unit.caller_registers && self.live.is_none_or(|live| live.is_live(at as usize, register))
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

    /// An operand's tagged bits: its register's, or its immediate constant's.
    fn operand_tagged(&mut self, state: &mut State, operand: Operand) -> Option<ir::Value> {
        match operand.kind()? {
            OperandKind::Register => Some(self.tagged(state, operand.register_index()?)),
            OperandKind::Constant => {
                let value = self.program.constants.get(usize::from(operand.payload())).and_then(super::facts::immediate)?;
                Some(self.ins().iconst(types::I64, value.to_bits() as i64))
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
    fields: &BTreeMap<u16, Vec<NativeFieldLoad>>,
    direct: &BTreeMap<usize, *const DirectCallee>,
    frame_captures: &[(usize, usize)],
    loop_entry: Option<usize>,
) -> Option<LaneRegion> {
    let isa = super::jit_backend::target_isa("speed")?;
    let (native, count) = lower(program, function_id, unit, helpers, fields, direct, frame_captures, loop_entry, &*isa)?;
    let clif = native_dump_directory().map(|_| native.to_string());
    let code = compile_native_function(native, &*isa, LANE_BODY_MAX_CODE_BYTES, |code| Some(code.to_vec())).ok()?;
    Some(LaneRegion { code, native: count, clif })
}

/// Build the typed residual and validate every value/CFG edge before publication.
fn lower(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &BTreeMap<u16, Vec<NativeFieldLoad>>,
    direct: &BTreeMap<usize, *const DirectCallee>,
    frame_captures: &[(usize, usize)],
    loop_entry: Option<usize>,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
) -> Option<(ir::Function, usize)> {
    let function = program.function_get(function_id as usize)?;
    if fields.values().flatten().any(|field| !field.valid()) { return None; }
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
    let (context, start, registers_base, result) = (parameters[0], parameters[1], parameters[3], parameters[4]);
    let payload = builder.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        context,
        std::mem::offset_of!(CallContext, adapter_payload) as i32,
    );
    let count = usize::from(unit.registers);
    // Original masks address the caller's PCs. Inlined units conservatively
    // retain caller/callee state until a shared remapped mask is available.
    let live = program.register_root_masks(function_id);
    let frame_live = unit.frames.iter().map(|frame| program.register_root_masks(frame.target.function)).collect();
    let registers = Registers {
        tagged: (0..count).map(|_| builder.declare_var(types::I64)).collect(),
        int: vec![None; count],
        float: vec![None; count],
    };
    // An alternative of a call's switch starts a block: a callee other than the one before
    // it branches there (`Lowering::inline_call`).
    let mut leaders = leaders(&unit.instructions);
    for alternative in unit.frames.iter().filter_map(|frame| frame.alternative) {
        if let Some(leader) = leaders.get_mut(alternative) {
            *leader = true;
        }
    }
    let blocks: Vec<_> = leaders.iter().map(|leader| leader.then(|| builder.create_block())).collect();
    let (leave, stop) = (builder.create_block(), builder.create_block());
    builder.append_block_param(leave, types::I32);
    builder.append_block_param(stop, types::I64);
    builder.set_cold_block(leave);
    builder.set_cold_block(stop);
    let mut lowering = Lowering {
        builder,
        program,
        unit,
        helpers,
        fields,
        direct,
        frame_captures,
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
        frame_live,
        valid: true,
        leave,
        stop,
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
    // An entry at a loop header (`loop_entry`, the entry's `start` word nonzero) starts
    // from the registers live there, each proved to its header facts or leaving at it.
    let starts: Vec<usize> = std::iter::once(0).chain(loop_entry).collect();
    let mut start_blocks: Vec<ir::Block> = starts.iter().map(|_| lowering.builder.create_block()).collect();
    if let [at_first, at_header] = start_blocks[..] {
        let from_header = lowering.builder.ins().icmp_imm_u(IntCC::NotEqual, start, 0);
        lowering.builder.ins().brif(from_header, at_header, &[], at_first, &[]);
    } else {
        lowering.builder.ins().jump(start_blocks[0], &[]);
    }
    for (pc, block) in starts.iter().copied().zip(start_blocks.drain(..)) {
        lowering.builder.switch_to_block(block);
        let origin = match lowering.unit.origins.get(pc) {
            Some(super::lane_unit::Origin::Caller(origin)) => *origin as usize,
            _ => return None,
        };
        let undefined = lowering.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
        for register in 0..count {
            let bits = if register < usize::from(lowering.unit.caller_registers)
                && lowering.live.is_none_or(|live| live.is_live(origin, register as Register)) {
                lowering.builder.ins().load(types::I64, MemFlagsData::trusted(), registers_base, register_offset(register as Register))
            } else {
                undefined
            };
            lowering.builder.def_var(lowering.registers.tagged[register], bits);
        }
        let Some(target) = lowering.blocks.get(pc).copied().flatten() else { return None };
        let mut state = State::canonical(count);
        lowering.conform(&mut state, pc);
        lowering.builder.ins().jump(target, &[]);
    }
    let mut state = State::canonical(count);
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
        let pc = pc as usize;
        for register in lowering.written.clone().into_iter().filter(|register| *register < unit.caller_registers) {
            if !lowering.is_live(pc, register) { continue; }
            let bits = lowering.builder.use_var(lowering.registers.tagged[usize::from(register)]);
            lowering.builder.ins().store(MemFlagsData::trusted(), bits, lowering.registers_base, register_offset(register));
        }
        let word = match unit.origins.get(pc).copied().unwrap_or(super::lane_unit::Origin::Caller(unit.end)) {
            super::lane_unit::Origin::Caller(record) => u64::from(record),
            super::lane_unit::Origin::Inlined { frame, pc: record } => {
                // Each register of the frame and the frames around it, at its unit register
                // past the caller's (`LaneMethod::scratch_width`).
                let scratch = emit_js_payload_word(&mut lowering.builder, lowering.payload, std::mem::offset_of!(JsStepDispatch, inlined));
                // Only the live ones: the activation reads no other (`dispatch_fast::
                // lane_body_inlined_activation`).
                for index in unit.chain(frame) {
                    let inlined = unit.frames[index];
                    for register in inlined.base..inlined.base + inlined.registers {
                        if !lowering.is_live(pc, register) { continue; }
                        let bits = lowering.builder.use_var(lowering.registers.tagged[usize::from(register)]);
                        lowering.builder.ins().store(MemFlagsData::trusted(), bits, scratch, register_offset(register - unit.caller_registers));
                    }
                }
                LaneExit::Inlined { frame, pc: record }.word()
            }
        };
        let pc_value = lowering.builder.ins().iconst(types::I64, word as i64);
        lowering.builder.ins().jump(stop, &[pc_value.into()]);
    }
    lowering.builder.switch_to_block(stop);
    let word = lowering.builder.block_params(stop)[0];
    lowering.builder.ins().store(MemFlagsData::trusted(), word, lowering.result, 0);
    let status = lowering.builder.ins().iconst(types::I32, i64::from(NATIVE_JS_STOPPED));
    lowering.builder.ins().jump(leave, &[status.into()]);
    lowering.builder.switch_to_block(leave);
    let status = lowering.builder.block_params(leave)[0];
    lowering.builder.ins().return_(&[status]);
    if !lowering.valid { return None; }
    let native_count = lowering.native;
    lowering.builder.seal_all_blocks();
    lowering.builder.finalize(isa.frontend_config());
    Some((native, native_count))
}

impl Lowering<'_, '_> {
    /// `register`'s int32 view (`Registers::int`).
    fn int_var(&mut self, register: usize) -> Variable {
        *self.registers.int[register].get_or_insert_with(|| self.builder.declare_var(types::I32))
    }

    /// `register`'s float64 view (`Registers::float`).
    fn float_var(&mut self, register: usize) -> Variable {
        *self.registers.float[register].get_or_insert_with(|| self.builder.declare_var(types::F64))
    }

    fn entry_facts(&mut self, instructions: &[WideInstruction]) -> Entry {
        let receiver_offset = i32::try_from(std::mem::offset_of!(JsStepDispatch, receiver)).expect("dispatch offset fits i32");
        let receiver = emit_lane_receiver(&mut self.builder, self.payload, receiver_offset);
        let this_bits = self.builder.ins().load(types::I64, MemFlagsData::trusted(), self.payload, receiver_offset);
        let this = self.builder.declare_var(types::I64);
        self.builder.def_var(this, this_bits);
        let mut receiver_shapes = BTreeMap::new();
        for field in self.fields.values().flatten() {
            if receiver_shapes.contains_key(&field.shape) {
                continue;
            }
            let shape = self.builder.use_var(receiver.shape);
            let same = self.builder.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
            let variable = self.builder.declare_var(types::I8);
            self.builder.def_var(variable, same);
            receiver_shapes.insert(field.shape, variable);
        }
        // A direct store of an atom needs a specialized run and no method cache naming it; a
        // run that makes no call changes neither (`emit_js_own_field_set`, `store_field`).
        let mut store_atoms = BTreeMap::new();
        let stored = instructions.iter().filter_map(|instruction| match instruction.op() {
            Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict => Some(instruction.atom_index()),
            Op::GetField if instruction.writes_current_this() => match instruction.field_lookup() {
                Some(FieldLookup::Site(index)) => self.program.field_sites.get(index).and_then(|site| site.sink).map(|(atom, _)| atom),
                _ => None,
            },
            _ => None,
        }).collect::<Vec<_>>();
        for atom in stored {
            if !store_atoms.contains_key(&atom) {
                let admitted = self.emit_store_atom_admitted(atom);
                let variable = self.builder.declare_var(types::I8);
                self.builder.def_var(variable, admitted);
                store_atoms.insert(atom, variable);
            }
        }
        // The running activation's dense capture slots (`EnvironmentSlots::native_dense_view`),
        // fixed for the run; each read loads its slot where it runs, after any call that may
        // have stored to it.
        let capture_values = self.builder.declare_var(types::I64);
        let capture_len = self.builder.declare_var(types::I64);
        let values = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, capture_values));
        let len = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, capture_len));
        self.builder.def_var(capture_values, values);
        self.builder.def_var(capture_len, len);
        Entry { receiver, receiver_shapes, store_atoms, capture_values, capture_len, this }
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
            Op::Return if plain && instruction.register_a() < self.unit.registers => {
                Some(self.complete(state, pc, instruction.register_a()))
            }
            Op::Call if (plain || instruction.result_flags() == crate::bytecode::RETURN_REGISTER)
                && !instruction.direct_eval()
                && !instruction.parameter_eval() => Some(self.call(state, pc, instruction)),
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
                let slot = usize::from(crate::bytecode::ImmediateLayout::capture_slot(instruction.imm()));
                let (values, len) = match self.unit.origins.get(pc).copied() {
                    Some(super::lane_unit::Origin::Inlined { frame, .. }) => {
                        let (values, len) = self.frame_captures.get(frame).copied().unwrap_or((0, 0));
                        (self.ins().iconst(types::I64, values as i64), self.ins().iconst(types::I64, len as i64))
                    }
                    _ => {
                        let entry = self.entry.as_ref().expect("entry facts");
                        (self.builder.use_var(entry.capture_values), self.builder.use_var(entry.capture_len))
                    }
                };
                let exit = self.guard_exit(state, pc32);
                let in_bounds = self.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, len, slot as i64);
                emit_js_guard(&mut self.builder, in_bounds, exit);
                let offset = i32::try_from(slot * std::mem::size_of::<Value>()).expect("capture slot offset fits i32");
                let value = self.ins().load(types::I64, MemFlagsData::trusted(), values, offset);
                let tag = self.ins().band_imm_u(value, crate::value::ValueLayout::TAG_MASK as i64);
                let ordinary = self.ins().icmp_imm_u(IntCC::NotEqual, tag, crate::value::ValueLayout::INTERNAL_TAG as i64);
                emit_js_guard(&mut self.builder, ordinary, exit);
                self.set_tagged(state, instruction.result_register(), value);
                Some(true)
            }
            Op::GetField if instruction.result_flags() & !(crate::bytecode::SET_THIS_REGISTER | crate::bytecode::RETURN_REGISTER) == 0 => self.get_field(state, pc, instruction),
            Op::GetIndex if plain && self.operand_supported(instruction.operand_b()) && self.operand_supported(instruction.operand_c()) => {
                // A present dense element, the lane's own fast read (`Vm::direct_dense_element`).
                let exit = self.guard_exit(state, pc32);
                let operands = (self.operand_tagged(state, instruction.operand_b()), self.operand_tagged(state, instruction.operand_c()));
                operands.0.zip(operands.1).map(|(base, key)| {
                    let payload = self.payload;
                    let value = super::emit_dense_element_read(&mut self.builder, payload, base, key, exit);
                    self.set_tagged(state, instruction.result_register(), value);
                    true
                })
            }
            Op::SetIndex if [instruction.register_a(), instruction.register_b(), instruction.register_c()]
                .iter().all(|register| *register < self.unit.registers) => {
                // The lane's in-place element replacement; a miss leaves at the store.
                let exit = self.guard_exit(state, pc32);
                let value = self.tagged(state, instruction.register_a());
                let base = self.tagged(state, instruction.register_b());
                let key = self.tagged(state, instruction.register_c());
                let stored = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.index_set, &[base, key, value]);
                let hit = self.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
                emit_js_guard(&mut self.builder, hit, exit);
                Some(true)
            }
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
                        let int = { let variable = self.int_var(usize::from(register)); self.builder.use_var(variable) };
                        self.ins().icmp_imm_u(IntCC::NotEqual, int, 0)
                    }
                    Representation::Float64 => {
                        let number = { let variable = self.float_var(usize::from(register)); self.builder.use_var(variable) };
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
                if let Some(holds) = self.number_relation(state, pc32, operator, left, right) {
                    let taken = self.ins().icmp_imm_u(IntCC::Equal, holds, 0);
                    let target = instruction.jump_target() as usize;
                    if target <= pc {
                        self.interrupt_check(state, pc32);
                    }
                    self.branch(state, pc, target, taken);
                    return false;
                } else {
                    None
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

    /// Whether the numeric relation `operator` holds of `left` and `right`, as an `i8`: in
    /// int32 where both are proved int32, else in Float64, each operand proved a Number or
    /// leaving at `pc`; `None` for any other operator or operand.
    fn number_relation(&mut self, state: &mut State, pc: u32, operator: u32,
        left: Operand, right: Operand) -> Option<ir::Value> {
        if super::operations::NumberRelation::from_operator(operator).is_none()
            || !self.operand_supported(left)
            || !self.operand_supported(right)
            || [left, right].iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
                && !super::facts::operand_type(&self.program.constants, *operand, |_| Type::ANY).is_subtype(Type::NUMBER))
        {
            return None;
        }
        let exit = self.guard_exit(state, pc);
        let integer = [left, right].iter().all(|operand| super::facts::operand_type(
            &self.program.constants, *operand, |register| state.values[usize::from(register)].value.ty)
            .is_subtype(Type::INT32));
        let (left, right, domain) = if integer {
            (self.operand_int(state, left, exit)?, self.operand_int(state, right, exit)?, NumberRelationDomain::Int32)
        } else {
            (self.operand_number(state, left, exit)?, self.operand_number(state, right, exit)?, NumberRelationDomain::Float64)
        };
        emit_number_relation(&mut self.builder, operator, left, right, domain)
    }

    /// `source`'s value into `destination`, with every fact proved of it.
    fn copy(&mut self, state: &mut State, destination: Register, source: Register) {
        let fact = state.values[usize::from(source)];
        let variable = match fact.value.representation {
            Representation::Tagged => self.registers.tagged[usize::from(source)],
            Representation::Int32 => self.int_var(usize::from(source)),
            Representation::Float64 => self.float_var(usize::from(source)),
        };
        let value = self.builder.use_var(variable);
        let tagged = (fact.tagged_current && fact.value.representation != Representation::Tagged)
            .then(|| self.builder.use_var(self.registers.tagged[usize::from(source)]));
        let shape = state.shapes.get(&Some(source)).copied();
        let receiver = state.receivers.contains(&source);
        let layouts: Vec<_> = state.layouts.iter().filter_map(|((owner, shape), projection)|
            (*owner == Some(source)).then_some((*shape, *projection))).collect();
        self.define(state, destination, value, fact);
        if let Some(bits) = tagged { self.builder.def_var(self.registers.tagged[usize::from(destination)], bits); }
        if let Some(shape) = shape { state.shapes.insert(Some(destination), shape); }
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
        match inlined.alternative {
            Some(next) => {
                let (matched, other) = (self.builder.create_block(), self.builder.create_block());
                self.ins().brif(same, matched, &[], other, &[]);
                self.builder.switch_to_block(other);
                let mut other_state = state.clone();
                self.continue_at(&mut other_state, next);
                self.builder.switch_to_block(matched);
            }
            None => emit_js_guard(&mut self.builder, same, exit),
        }
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
        let completed = self.builder.create_block();
        let helper = self.builder.create_block();
        match self.direct.get(&pc).copied() {
            // SAFETY: the body owns the callee for as long as its code runs.
            Some(callee) => self.direct_call(instruction, unsafe { &*callee }, helper, completed),
            None => { self.ins().jump(helper, &[]); }
        }
        self.builder.switch_to_block(helper);
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
        let call_word = self.ins().iconst(types::I64, i64::from(call_pc));
        let status = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.call, &[context, call_word]);
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
        let stop = self.stop;
        self.ins().jump(stop, &[word.into()]);
        self.builder.switch_to_block(completed);
        // A returning call's completion is its activation's: the lane completes it with the
        // result, which the call left in its register (`LaneExit::Returned`).
        if instruction.returns_from_frame() {
            let base = self.registers_base;
            let value = self.ins().load(types::I64, MemFlagsData::trusted(), base, register_offset(instruction.result_register()));
            self.return_value(call_pc, value);
            return false;
        }
        // The callee may have changed any object: block-local proofs of them retire.
        state.shapes.clear();
        state.layouts.clear();
        let destination = instruction.result_register();
        let base = self.registers_base;
        let bits = self.ins().load(types::I64, MemFlagsData::trusted(), base, register_offset(destination));
        self.set_tagged(state, destination, bits);
        true
    }

    /// The call of `callee` by its compiled entry: the callee's registers in the lane stack's
    /// free values, a native frame the lane makes an activation of only should the callee
    /// leave (`compiled::ShadowFrame`), and the per-entry words the callee runs with. A
    /// completion stores its value to the call's register and continues at `completed`; a
    /// leave stops the caller too, its frames kept; any other callee, receiver, or a full
    /// stack takes `helper`. The caller wrote back its registers.
    fn direct_call(&mut self, instruction: WideInstruction, callee: &DirectCallee, helper: ir::Block, completed: ir::Block) {
        use crate::value::ValueLayout;
        let word = std::mem::size_of::<Value>() as i64;
        let frame_size = std::mem::size_of::<ShadowFrame>() as i64;
        let offset = |field: usize| i32::try_from(field).expect("dispatch offset fits i32");
        let (base, payload, result) = (self.registers_base, self.payload, self.result);
        let callee_bits = self.ins().load(types::I64, MemFlagsData::trusted(), base, register_offset(instruction.register_b()));
        let same = self.ins().icmp_imm_u(IntCC::Equal, callee_bits, callee.callee.to_bits() as i64);
        let this = self.ins().load(types::I64, MemFlagsData::trusted(), base, register_offset(instruction.register_c()));
        let admitted = if callee.sloppy {
            // A sloppy callee binds an object receiver itself (`lane_receiver_this`).
            let family = self.ins().band_imm_u(this, ValueLayout::HEAP_FAMILY_MASK as i64);
            let object = self.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_OBJECT_TAG as i64);
            self.ins().band(same, object)
        } else { same };
        let top = self.ins().load(types::I64, MemFlagsData::trusted(), payload, offset(std::mem::offset_of!(JsStepDispatch, native_top)));
        let end = self.ins().load(types::I64, MemFlagsData::trusted(), payload, offset(std::mem::offset_of!(JsStepDispatch, native_end)));
        let next = self.ins().iadd_imm_u(top, i64::from(callee.registers) * word);
        let reserved = self.ins().iadd_imm_u(next, i64::from(callee.reserve) * word);
        let fits = self.ins().icmp(IntCC::UnsignedLessThanOrEqual, reserved, end);
        let shadow = self.ins().load(types::I64, MemFlagsData::trusted(), payload, offset(std::mem::offset_of!(JsStepDispatch, shadow_top)));
        let shadow_end = self.ins().load(types::I64, MemFlagsData::trusted(), payload, offset(std::mem::offset_of!(JsStepDispatch, shadow_end)));
        let room = self.ins().icmp(IntCC::UnsignedLessThan, shadow, shadow_end);
        let admitted = self.ins().band(admitted, fits);
        let admitted = self.ins().band(admitted, room);
        let enter = self.builder.create_block();
        self.ins().brif(admitted, enter, &[], helper, &[]);
        self.builder.switch_to_block(enter);
        // Parameter `k` lives in register `k` of a plain activation; the rest start undefined.
        let undefined = self.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
        let window = crate::bytecode::ImmediateLayout::call_window_base(instruction.imm());
        let argc = crate::bytecode::ImmediateLayout::argument_count(instruction.imm());
        for register in 0..callee.registers {
            let value = if register < argc {
                self.ins().load(types::I64, MemFlagsData::trusted(), base, register_offset(window + register))
            } else { undefined };
            self.ins().store(MemFlagsData::trusted(), value, top, register_offset(register));
        }
        let call = self.ins().iconst(types::I64, callee.call as i64);
        let descriptor = self.ins().iconst(types::I64, callee as *const DirectCallee as i64);
        for (field, value) in [
            (std::mem::offset_of!(ShadowFrame, call), call),
            (std::mem::offset_of!(ShadowFrame, callee), descriptor),
            (std::mem::offset_of!(ShadowFrame, window), top),
            (std::mem::offset_of!(ShadowFrame, this), this),
        ] {
            self.ins().store(MemFlagsData::trusted(), value, shadow, offset(field));
        }
        let pushed = self.ins().iadd_imm_u(shadow, frame_size);
        let words = [
            std::mem::offset_of!(JsStepDispatch, receiver),
            std::mem::offset_of!(JsStepDispatch, capture_values),
            std::mem::offset_of!(JsStepDispatch, capture_len),
        ];
        let saved: Vec<ir::Value> = words.iter()
            .map(|field| self.ins().load(types::I64, MemFlagsData::trusted(), payload, offset(*field)))
            .collect();
        let capture_values = self.ins().iconst(types::I64, callee.capture_values as i64);
        let capture_len = self.ins().iconst(types::I64, callee.capture_len as i64);
        for (field, value) in words.iter().zip([this, capture_values, capture_len]) {
            self.ins().store(MemFlagsData::trusted(), value, payload, offset(*field));
        }
        self.ins().store(MemFlagsData::trusted(), pushed, payload, offset(std::mem::offset_of!(JsStepDispatch, shadow_top)));
        self.ins().store(MemFlagsData::trusted(), next, payload, offset(std::mem::offset_of!(JsStepDispatch, native_top)));
        let signature = native_entry_signature(types::I64, self.builder.func.signature.call_conv);
        let signature = self.builder.import_signature(signature);
        // SAFETY: the callee's body lives while the caller's does (`DirectCallee::method`).
        let entry = self.ins().iconst(types::I64, unsafe { &*callee.method }.entry_address() as i64);
        let zero = self.ins().iconst(types::I64, 0);
        let context = self.context;
        let status = self.ins().call_indirect(signature, entry, &[context, zero, zero, top, result]);
        let status = self.builder.inst_results(status)[0];
        for (field, value) in words.iter().zip(saved) {
            self.ins().store(MemFlagsData::trusted(), value, payload, offset(*field));
        }
        self.ins().store(MemFlagsData::trusted(), top, payload, offset(std::mem::offset_of!(JsStepDispatch, native_top)));
        let returned = self.ins().icmp_imm_u(IntCC::Equal, status, i64::from(LANE_BODY_RETURNED));
        let back = self.builder.create_block();
        let left = self.builder.create_block();
        self.builder.set_cold_block(left);
        self.ins().brif(returned, back, &[], left, &[]);
        // A leave inside the callee stops the caller with it: the lane makes the frames'
        // activations and continues in the innermost (`dispatch_fast::lane_body_left`).
        self.builder.switch_to_block(left);
        let leave = self.leave;
        self.ins().jump(leave, &[status.into()]);
        self.builder.switch_to_block(back);
        self.ins().store(MemFlagsData::trusted(), shadow, payload, offset(std::mem::offset_of!(JsStepDispatch, shadow_top)));
        let value = self.ins().load(types::I64, MemFlagsData::trusted(), result, 0);
        self.ins().store(MemFlagsData::trusted(), value, base, register_offset(instruction.result_register()));
        self.ins().jump(completed, &[]);
    }

    /// The activation completes with `register`, which the record at `pc` names
    /// (`LaneExit::Returned`). Only a caller record can: an inlined callee's `Return` is the
    /// unit's move and jump.
    fn complete(&mut self, state: &mut State, pc: usize, register: Register) -> bool {
        let Some(super::lane_unit::Origin::Caller(record)) = self.unit.origins.get(pc).copied() else {
            self.exit(state, pc as u32);
            return false;
        };
        let bits = self.tagged(state, register);
        self.return_value(record, bits);
        self.native += 1;
        false
    }

    /// Stops with the activation's completion: `value` in the result word, `record` the
    /// record that completes it (`LANE_BODY_RETURNED`).
    fn return_value(&mut self, record: u32, value: ir::Value) {
        let result = self.result;
        self.ins().store(MemFlagsData::trusted(), value, result, 0);
        let record = self.ins().iconst(types::I64, i64::from(record));
        let payload = self.payload;
        self.ins().store(MemFlagsData::trusted(), record, payload,
            i32::try_from(std::mem::offset_of!(JsStepDispatch, returned_pc)).expect("dispatch offset fits i32"));
        let returned = self.ins().iconst(types::I32, i64::from(LANE_BODY_RETURNED));
        let leave = self.leave;
        self.ins().jump(leave, &[returned.into()]);
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
        // A returning read completes the activation with its value, at a caller record; an
        // inlined callee's is the unit's read, move and jump (`lane_unit`).
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) => None,
            (true, Some(super::lane_unit::Origin::Caller(record))) => Some(record),
            (true, _) => return None,
        };
        // A nested lookup is a chain of reads from one base, each through its own site; a
        // guard that fails anywhere in it leaves at the instruction, whose reads have no effect.
        // A nested lookup that writes the current `this` stores the chain's value to `this`'s
        // sink field too, after its reads (`FieldSite::sink`).
        // An inlined frame's nested site names its base, and its sink's `this`, by the callee's
        // registers: they are the frame's (`lane_unit::Frame`).
        let frame = match self.unit.origins.get(pc) {
            Some(super::lane_unit::Origin::Inlined { frame, .. }) => Some(self.unit.frames[*frame]),
            _ => None,
        };
        let (base, parts, sink): (_, Vec<u16>, _) = match instruction.field_lookup()? {
            FieldLookup::Atom { base, cache_site, .. } => (base, vec![cache_site], None),
            FieldLookup::Site(index) => {
                let site = *self.program.field_sites.get(index)?;
                let reads = [Some(site.first), site.second].into_iter().flatten().map(|(_, cache)| cache).collect();
                let base = match frame {
                    Some(frame) if site.base == crate::bytecode::FieldBase::THIS => crate::bytecode::FieldBase::register(frame.receiver),
                    Some(frame) => crate::bytecode::FieldBase::register(frame.base + site.base.register_index()?),
                    None => site.base,
                };
                (base, reads, instruction.writes_current_this().then_some(site.sink).flatten())
            }
        };
        let sink_owner = frame.map(|frame| frame.receiver);
        if instruction.writes_current_this() && sink.is_none() {
            return None;
        }
        let register = base.register_index();
        if register.is_some_and(|register| register >= self.unit.registers)
            || (base != crate::bytecode::FieldBase::THIS && register.is_none())
        {
            return None;
        }
        let destination = instruction.result_register();
        let exit = self.guard_exit(state, pc as u32);
        let receiver = !self.native_calls
            && (base == crate::bytecode::FieldBase::THIS
                || register.is_some_and(|register| state.receivers.contains(&register)));
        let (first, rest) = parts.split_first()?;
        let mut value = self.base_field_read(state, register, receiver, *first, exit);
        for site in rest {
            value = self.object_field_read(state, value, *site, exit)?;
        }
        if let Some((atom, site)) = sink {
            self.store_field(state, sink_owner, atom, site, value, false, exit)?;
        }
        if let Some(record) = completes {
            self.return_value(record, value);
            return Some(false);
        }
        self.set_tagged(state, destination, value);
        Some(true)
    }

    /// The read through `site` of the base `register` names (`this` for `None`): folded to
    /// the site's proved shapes and slots, with the base's record proved once (the
    /// receiver's at entry), else the VM's cached read.
    fn base_field_read(&mut self, state: &mut State, register: Option<Register>, receiver: bool, site: u16, exit: ir::Block) -> ir::Value {
        let object_bits = match register {
            None => self.builder.use_var(self.entry.as_ref().expect("entry facts").this),
            Some(register) => self.tagged(state, register),
        };
        let shapes = self.fields.get(&site).cloned().unwrap_or_default();
        if shapes.is_empty() {
            return self.field_probe(object_bits, site, exit);
        }
        let owner = if receiver { None } else { register };
        let probed = shapes.iter().any(|field| field.holder.is_some());
        self.probed_read(state, owner, probed, object_bits, site, exit, |lowering, state, generic| {
            let (object, shape, inline) = if receiver {
                let entry = lowering.entry.as_ref().expect("entry facts");
                (
                    lowering.builder.use_var(entry.receiver.object),
                    lowering.builder.use_var(entry.receiver.shape),
                    lowering.builder.use_var(entry.receiver.inline),
                )
            } else {
                lowering.object_record(state, register, generic)
            };
            lowering.shape_switch_read(state, Some(owner), object, shape, inline, &shapes, generic)
        })
    }

    /// The read through `site` of the object a previous read of a chain produced, whose
    /// record nothing else names, so it is proved for this read alone.
    fn object_field_read(&mut self, state: &mut State, object_bits: ir::Value, site: u16, exit: ir::Block) -> Option<ir::Value> {
        let shapes = self.fields.get(&site).cloned().unwrap_or_default();
        if shapes.is_empty() {
            return Some(self.field_probe(object_bits, site, exit));
        }
        let probed = shapes.iter().any(|field| field.holder.is_some());
        Some(self.probed_read(state, None, probed, object_bits, site, exit, |lowering, state, generic| {
            let records = emit_js_payload_word(&mut lowering.builder, lowering.payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
            let header = ObjectHeader::prove(&mut lowering.builder, object_bits, records, generic);
            lowering.shape_switch_read(state, None, header.record, header.shape, header.inline, &shapes, generic)
        }))
    }

    /// A read by the site's proved shapes (`fast`), whose misses leave through `exit`; where
    /// `probed` (the site names prototype-held slots, which prove only the immediate prototype,
    /// `Vm::ordinary_holder_entry_value`), a miss first asks the VM's caches, whose other tiers
    /// read deeper holders (`field_probe`). Past that join only what both paths prove holds:
    /// the fast path's proofs of `owner`'s object do not.
    #[allow(clippy::too_many_arguments)]
    fn probed_read(&mut self, state: &mut State, owner: Option<Register>, probed: bool, object_bits: ir::Value, site: u16,
        exit: ir::Block, fast: impl FnOnce(&mut Self, &mut State, ir::Block) -> Option<ir::Value>) -> ir::Value {
        if !probed {
            return match fast(self, state, exit) {
                Some(read) => read,
                None => self.field_probe(object_bits, site, exit),
            };
        }
        let (generic, join) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(generic);
        let value = self.builder.append_block_param(join, types::I64);
        let before = state.clone();
        match fast(self, state, generic) {
            Some(read) => { self.ins().jump(join, &[ir::BlockArg::Value(read)]); }
            None => { self.ins().jump(generic, &[]); }
        }
        self.builder.switch_to_block(generic);
        let probed = self.field_probe(object_bits, site, exit);
        self.ins().jump(join, &[ir::BlockArg::Value(probed)]);
        self.builder.switch_to_block(join);
        state.shapes.retain(|known, _| before.shapes.contains_key(known));
        state.layouts.retain(|key, _| before.layouts.contains_key(key));
        if let Some(register) = owner {
            state.values[usize::from(register)] = before.values[usize::from(register)];
        }
        value
    }

    /// Read the slot `shapes` names for the record's `shape`; `exit` for any other shape. A
    /// single shape's layout proof is kept for `owner` (a register's object, or `this`'s for
    /// `Some(None)`) until an effect retires it; an object no owner names keeps none.
    #[allow(clippy::too_many_arguments)]
    fn shape_switch_read(
        &mut self,
        state: &mut State,
        owner: Option<Option<Register>>,
        object: ir::Value,
        shape: ir::Value,
        inline: ir::Value,
        shapes: &[NativeFieldLoad],
        exit: ir::Block,
    ) -> Option<ir::Value> {
        let entry = self.entry.as_ref().expect("entry facts");
        let properties = self.builder.use_var(entry.receiver.properties);
        let properties_len = self.builder.use_var(entry.receiver.properties_len);
        if let [field] = shapes && field.holder.is_none() {
            let known = owner.and_then(|owner| state.layouts.get(&(owner, field.shape)).copied());
            let projection = match known {
                Some(projection) if usize::from(field.slot) < projection.storage_len => projection,
                _ => {
                    let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
                    emit_js_guard(&mut self.builder, same, exit);
                    let projection = CheckedObject::prove(&mut self.builder, object, inline, properties,
                        properties_len, *field, PackedAccess::Reload, exit)?;
                    if let Some(owner) = owner {
                        state.layouts.insert((owner, field.shape), projection);
                    }
                    projection
                }
            };
            return Some(projection.read(&mut self.builder, field.slot));
        }
        // A layout family: own fields at one slot in every shape. One membership test and one
        // read serve them all, bounded by the shortest storage, which still holds the slot.
        if let [first, ..] = shapes
            && shapes.iter().all(|field| field.holder.is_none() && field.slot == first.slot && field.storage == first.storage) {
            let mut member = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(first.shape));
            for field in &shapes[1..] {
                let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
                member = self.ins().bor(member, same);
            }
            emit_js_guard(&mut self.builder, member, exit);
            let shortest = shapes.iter().copied().min_by_key(|field| field.storage_len).expect("a family has a shape");
            let projection = CheckedObject::prove(&mut self.builder, object, inline, properties,
                properties_len, shortest, PackedAccess::Reload, exit)?;
            return Some(projection.read(&mut self.builder, first.slot));
        }
        let join = self.builder.create_block();
        let value = self.builder.append_block_param(join, types::I64);
        for field in shapes {
            let matched = self.builder.create_block();
            let next = self.builder.create_block();
            let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
            self.ins().brif(same, matched, &[], next, &[]);
            self.builder.switch_to_block(matched);
            let read = match field.holder {
                None => CheckedObject::prove(&mut self.builder, object, inline, properties,
                    properties_len, *field, PackedAccess::Reload, exit)?.read(&mut self.builder, field.slot),
                Some(holder) => self.holder_read(object, holder, *field, exit)?,
            };
            self.ins().jump(join, &[ir::BlockArg::Value(read)]);
            self.builder.switch_to_block(next);
        }
        self.ins().jump(exit, &[]);
        self.builder.switch_to_block(join);
        Some(value)
    }

    /// The slot `field` names in the prototype of the object record `record`: an object
    /// record of the exact shape `holder`, else `exit` (`Vm::record_field_entry_hit`).
    fn holder_read(&mut self, record: ir::Value, holder: u32, field: NativeFieldLoad, exit: ir::Block) -> Option<ir::Value> {
        use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
        let meta = self.ins().load(types::I32, MemFlagsData::trusted(), record, OBJECT.meta_offset as i32);
        let proto = self.ins().band_imm_u(meta, i64::from(OBJECT.proto_mask));
        let from = self.ins().icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, proto, i64::from(OBJECT.proto_record_base));
        let below = self.ins().icmp_imm_u(IntCC::UnsignedLessThan, proto, i64::from(OBJECT.proto_record_end));
        let object_proto = self.ins().band(from, below);
        emit_js_guard(&mut self.builder, object_proto, exit);
        let id = self.ins().iadd_imm_s(proto, -i64::from(OBJECT.proto_record_base));
        let id = self.ins().uextend(types::I64, id);
        let offset = self.ins().imul_imm_u(id, OBJECT.record_size as i64);
        let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
        let prototype = self.ins().iadd(records, offset);
        let shape = self.ins().load(types::I32, MemFlagsData::trusted(), prototype, OBJECT.auxiliary_offset as i32);
        let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(holder));
        emit_js_guard(&mut self.builder, same, exit);
        // A prototype keeps its properties in the arena past its extras (`emit_js_cached_field_slot`).
        let holder_meta = self.ins().load(types::I32, MemFlagsData::trusted(), prototype, OBJECT.meta_offset as i32);
        let inline = self.ins().band_imm_u(holder_meta, i64::from(OBJECT.inline_storage_mask));
        let arena = self.ins().icmp_imm_u(IntCC::Equal, inline, 0);
        emit_js_guard(&mut self.builder, arena, exit);
        let storage = self.ins().load(types::I64, MemFlagsData::trusted(), prototype, OBJECT.payload_offset as i32);
        let tag = self.ins().band_imm_u(storage, OBJECT.extras_tag_mask as i64);
        let tagged = self.ins().icmp_imm_u(IntCC::Equal, tag, OBJECT.extras_tag as i64);
        emit_js_guard(&mut self.builder, tagged, exit);
        let extras = self.ins().band_imm_u(storage, !OBJECT.extras_tag_mask as i64);
        let present = self.ins().icmp_imm_u(IntCC::NotEqual, extras, 0);
        emit_js_guard(&mut self.builder, present, exit);
        let start = self.ins().load(types::I32, MemFlagsData::trusted(), extras, OBJECT.extras_arena_start_offset as i32);
        let start = self.ins().uextend(types::I64, start);
        let entry = self.entry.as_ref().expect("entry facts");
        let properties = self.builder.use_var(entry.receiver.properties);
        let properties_len = self.builder.use_var(entry.receiver.properties_len);
        let end = self.ins().iadd_imm_u(start, field.storage_len as i64);
        let inside = self.ins().icmp(IntCC::UnsignedLessThanOrEqual, end, properties_len);
        emit_js_guard(&mut self.builder, inside, exit);
        let index = self.ins().iadd_imm_u(start, i64::from(field.slot));
        let offset = self.ins().imul_imm_u(index, OBJECT.property_value_size as i64);
        let address = self.ins().iadd(properties, offset);
        Some(self.ins().load(types::I64, MemFlagsData::trusted(), address, 0))
    }

    /// The record of `owner`'s object (a register's, or `this`'s), proved once until an
    /// effect that may change it retires the proof.
    fn object_record(&mut self, state: &mut State, owner: Option<Register>, exit: ir::Block) -> (ir::Value, ir::Value, ir::Value) {
        if let Some(known) = state.shapes.get(&owner).copied() { return known; }
        let bits = match owner {
            Some(register) => self.tagged(state, register),
            None => self.builder.use_var(self.entry.as_ref().expect("entry facts").this),
        };
        let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
        let header = ObjectHeader::prove(&mut self.builder, bits, records, exit);
        let projection = (header.record, header.shape, header.inline);
        state.shapes.insert(owner, projection);
        if let Some(register) = owner {
            state.values[usize::from(register)] = RegisterFact::definition(
                Type::language(crate::ValueType::Object), Representation::Tagged);
        }
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
        let exit = self.guard_exit(state, pc as u32);
        let barrier_free = state.values[usize::from(source)].value.ty.is_subtype(Type::NUMBER);
        let value = self.tagged(state, source);
        self.store_field(state, register, instruction.atom_index(), instruction.cache_site_index(), value, barrier_free, exit)?;
        Some(true)
    }

    /// The store of `value` through `site` to the field `atom` of the object `register` names
    /// (`this` for `None`): the site's one proved shape and slot written directly, with the
    /// object's record proved once (the receiver's at entry, in a body that makes no call),
    /// else the VM's own-field store hit; anything else leaves through `exit`.
    #[allow(clippy::too_many_arguments)]
    fn store_field(&mut self, state: &mut State, register: Option<Register>, atom: Atom, site: u16,
        value: ir::Value, barrier_free: bool, exit: ir::Block) -> Option<()> {
        let receiver = !self.native_calls
            && (register.is_none() || register.is_some_and(|register| state.receivers.contains(&register)));
        let owner = if receiver { None } else { register };
        let object_bits = match register {
            None => self.builder.use_var(self.entry.as_ref().expect("entry facts").this),
            Some(register) => self.tagged(state, register),
        };
        let field = self.fields.get(&site).and_then(|shapes| (shapes.len() == 1).then(|| shapes[0]));
        let Some(field) = field else {
            self.store_helper(object_bits, atom, site, value, exit);
            return Some(());
        };
        let header = (!receiver).then(|| self.object_record(state, register, exit));
        // A run that calls may meet a method cache that names the atom only after a call;
        // one that does not tests it once, at entry (`Lowering::entry_facts`).
        let admitted = if self.native_calls {
            self.emit_store_atom_admitted(atom)
        } else {
            self.builder.use_var(self.entry.as_ref().expect("entry facts").store_atoms[&atom])
        };
        let entry = self.entry.as_ref().expect("entry facts");
        let projection = state.layouts.get(&(owner, field.shape)).copied();
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
        let address = projection.arena_address(&mut self.builder, field.slot, general)?;
        self.ins().store(MemFlagsData::trusted(), value, address, 0);
        self.ins().jump(done, &[]);
        self.builder.switch_to_block(general);
        self.store_helper(object_bits, atom, site, value, exit);
        self.ins().jump(done, &[]);
        self.builder.switch_to_block(done);
        Some(())
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
            // A relation's Boolean, from the predicate a branch on it would test.
            _ => {
                let holds = self.number_relation(state, pc as u32, operator, left, right)?;
                let (yes, no) = (self.ins().iconst(types::I64, Value::TRUE.to_bits() as i64), self.ins().iconst(types::I64, Value::FALSE.to_bits() as i64));
                let boolean = self.ins().select(holds, yes, no);
                self.set_tagged(state, destination, boolean);
            }
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
        let fields: BTreeMap<u16, Vec<NativeFieldLoad>> = program.function(id).instructions().filter_map(|instruction| {
            if instruction.op() == Op::GetField
                && let Some(FieldLookup::Atom { atom, cache_site, .. }) = instruction.field_lookup()
                && let Some(slot) = ["left", "right"].iter().position(|name| *name == &program.atoms[atom as usize]) {
                Some((cache_site, vec![NativeFieldLoad { shape: 0, holder: None, slot: slot as u16, storage_len: 2, storage: None }]))
            } else { None }
        }).collect();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let unit = super::super::lane_unit::unit(id as u32, program.function(id), &|_, _| Vec::new(), &|_| None).unwrap();
        let (native, _) = lower(&program, id as u32,
            &unit, LaneHelpers { field_get: 0, field_set: 0, call: 0, index_set: 0 }, &fields, &BTreeMap::new(), &[], None, &*isa).unwrap();
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
        let unit = super::super::lane_unit::unit(id as u32, program.function(id), &|_, _| Vec::new(), &|_| None).unwrap();
        let (native, count) = lower(&program, id as u32,
            &unit, LaneHelpers { field_get: 0, field_set: 0, call: 0, index_set: 0 }, &BTreeMap::new(), &BTreeMap::new(), &[], None, &*isa).expect("typed Number residual");
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
