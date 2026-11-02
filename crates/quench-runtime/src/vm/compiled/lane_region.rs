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
use crate::typed_ir::{BindingTime, Fact, Projection, Proof, Representation, Type, VerifiedFact};
use super::object_slots::{CheckedObject, ObjectHeader, ObjectRecord, PackedAccess};
use std::collections::{BTreeMap, BTreeSet};
use cranelift_codegen::bitset::CompoundBitSet;

const LANE_REGION_NAMESPACE: u32 = u32::from_be_bytes(*b"QLRG");
/// Inline scratch capacity for the small field-site sets typical of native units;
/// larger units spill normally, without a semantic or compilation limit.
const INLINE_FIELD_BINDINGS: usize = 4;
/// Common native-entry failure status; a pure scalar miss restarts its call.
const NUMBER_ENTRY_MISS: i64 = 1;

/// Semantic proof and active machine view; tagged bits may also be current.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RegisterFact {
    value: VerifiedFact,
    tagged_current: bool,
}

impl RegisterFact {
    fn admit(value: Fact) -> Option<Self> {
        Some(Self { value: value.verified()?, tagged_current: value.representation == Representation::Tagged })
    }

    fn definition(ty: Type, representation: Representation) -> Self {
        Self::admit(Fact { ty, representation, binding: BindingTime::Dynamic,
            proof: Proof::Definition, projection: Projection::Exact })
            .expect("semantic operation's result type matches its representation")
    }

    fn canonical() -> Self {
        Self::admit(Fact::tagged(BindingTime::Location, Proof::CanonicalValue))
            .expect("canonical tagged value")
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
    /// Direct-object proofs by register; the final entry belongs to fixed `this`.
    headers: Vec<Option<ObjectHeader>>,
    /// A fixed receiver (`None`) or register's complete own-slot projection.
    layouts: BTreeMap<(Option<Register>, u32), CheckedObject>,
    /// Registers holding the receiver, whose facts the entry proved.
    receivers: BTreeSet<Register>,
}

impl State {
    fn canonical(registers: usize) -> Self {
        Self { values: vec![RegisterFact::canonical(); registers], headers: vec![None; registers + 1],
            layouts: BTreeMap::new(), receivers: BTreeSet::new() }
    }
}

/// An intermediate chain value has no register lifetime to memoize.
#[derive(Clone, Copy)]
enum FieldOwner { This, Register(Register), Temporary }

/// Region-wide facts proved at entry.
struct Entry {
    receiver: LaneReceiver,
    /// Entry receiver certificates survive every block in this no-GC,
    /// layout-preserving unit. Written registers never receive one.
    records: Vec<Option<ObjectRecord>>,
    /// Per folded receiver shape, whether the receiver has it.
    receiver_shapes: BTreeMap<u32, Variable>,
    /// Per stored atom, whether the run admits direct stores of it.
    store_atoms: BTreeMap<Atom, Variable>,
    /// Per caller (`None`) or inlined closure's capture slot, its value or `DELETED`.
    captures: BTreeMap<(Option<usize>, u16), Variable>,
    this: Variable,
    /// Flat per-site immutable slot inputs; no per-operation probe.
    fields: smallvec::SmallVec<[(u16, FieldSlotBinding); INLINE_FIELD_BINDINGS]>,
}

pub(super) struct LaneRegion {
    pub(super) code: Vec<u8>,
    /// Instructions the region runs natively, for admission.
    pub(super) native: usize,
    pub(super) clif: Option<String>,
    pub(super) fault_poll: bool,
}

pub(super) struct LoweredRegion {
    pub(super) function: ir::Function,
    pub(super) native: usize,
    pub(super) fault_poll: bool,
}

struct Lowering<'a, 'f> {
    builder: FunctionBuilder<'f>,
    program: &'a ResidualProgram,
    unit: &'a super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &'a [Vec<NativeFieldLoad>],
    /// An entry-bound poll page, or the ordinary atomic termination flag.
    interrupt: ir::Value,
    fault_poll: bool,
    payload: ir::Value,
    registers_base: ir::Value,
    result: ir::Value,
    registers: Registers,
    written: CompoundBitSet,
    /// Source operations already consumed by an exact, unobservable scalar pair.
    consumed: CompoundBitSet,
    blocks: Vec<Option<ir::Block>>,
    exits: Vec<Option<ir::Block>>,
    entry: Option<Entry>,
    native: usize,
    /// Guard failure blocks to fill once the region is lowered (`guard_exit`).
    pending: Vec<(ir::Block, State, u32)>,
    /// The registers' types at each block leader (`facts::analyze`).
    facts: super::facts::Facts,
    live: Option<&'a crate::compile::liveness::RegisterMasks>,
    valid: bool,
    coverage: Coverage,
    completion: Completion,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Coverage { Prefix, Full }

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Completion {
    StatusAndPc,
    SourcePc,
    /// Pure, restartable scalar entry: borrowed Number inputs and one Number
    /// output through the common native ABI; a miss restarts the original call.
    Number,
}

/// No heap, environment or guest-call effect may precede scalar-call recovery.
fn numeric_unit(program: &ResidualProgram, unit: &super::lane_unit::Unit) -> bool {
    unit.frames.is_empty() && unit.instructions.iter().all(|instruction| {
        if instruction.writes_current_this()
            || (instruction.result_flags() != 0 && !instruction.returns_from_frame()) { return false; }
        match instruction.op() {
            Op::LoadConst => program.constants.get(instruction.constant_index())
                .and_then(super::facts::immediate).is_some(),
            Op::Nop | Op::Move | Op::IncDec | Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse | Op::Return => true,
            op => op.js_denotation() == JsDenotation::NumericBinary
                && instruction.numeric_local_target().is_none(),
        }
    })
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
        let index = usize::from(register);
        let variable = match fact.value.representation {
            Representation::Tagged => self.registers.tagged[index],
            Representation::Int32 => self.registers.int[index],
            Representation::Float64 => self.registers.float[index],
        };
        self.builder.def_var(variable, value);
        state.values[index] = fact;
        state.headers[index] = None;
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
        if self.completion == Completion::Number {
            self.return_at(0);
            return;
        }
        self.canonicalize(state, pc as usize);
        let block = match self.exits[pc as usize] {
            Some(block) => block,
            None => {
                let block = self.builder.create_block();
                self.builder.set_cold_block(block);
                self.exits[pc as usize] = Some(block);
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
                let Some(fact) = RegisterFact::admit(Fact { ty, representation,
                    binding: if pc == 0 { BindingTime::Location } else { BindingTime::Dynamic },
                    proof: if pc != 0 { Proof::Definition }
                        else if ty == Type::ANY { Proof::CanonicalValue } else { Proof::EntryGuard },
                    projection: Projection::Exact,
                }) else { self.valid = false; return state; };
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
fn leaders(instructions: &[WideInstruction]) -> CompoundBitSet {
    let mut leaders = CompoundBitSet::with_capacity(instructions.len());
    if !instructions.is_empty() { leaders.insert(0); }
    for (pc, instruction) in instructions.iter().enumerate() {
        if matches!(instruction.op().control_flow_layout(), ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump) {
            if let Some(target) = usize::try_from(instruction.jump_target()).ok().filter(|t| *t < instructions.len()) {
                leaders.insert(target);
            }
            if pc + 1 < instructions.len() { leaders.insert(pc + 1); }
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
    let region = lower(program, function_id, unit, helpers, fields, &*isa, Coverage::Prefix, Completion::StatusAndPc)?;
    let clif = native_dump_directory().map(|_| region.function.to_string());
    let code = compile_native_function(region.function, &*isa, super::js_compiled::MAX_CODE_BYTES, |code| Some(code.to_vec())).ok()?;
    Some(LaneRegion { code, native: region.native, clif, fault_poll: region.fault_poll })
}

/// Build the typed residual and validate every value/CFG edge before publication.
pub(super) fn lower(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &[Vec<NativeFieldLoad>],
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    coverage: Coverage,
    completion: Completion,
) -> Option<LoweredRegion> {
    let function = program.function_get(function_id as usize)?;
    let numeric = completion == Completion::Number;
    if numeric && (!super::super::js_compiled::function_candidate(function, program)
        || !numeric_unit(program, unit) || function.params > unit.caller_registers) { return None; }
    if fields.iter().flatten().any(|field| !field.valid())
        || (completion == Completion::SourcePc && unit.frames.iter().any(|frame|
            frame.target.callee != CalleeBinding::Function)) { return None; }
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
    let payload = if numeric { builder.ins().iconst(types::I64, 0) } else { builder.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        context,
        std::mem::offset_of!(CallContext, adapter_payload) as i32,
    ) };
    let fault_poll = super::FAULT_POLL_SUPPORTED && unit.instructions.iter().enumerate().any(|(pc, instruction)|
        matches!(instruction.op().control_flow_layout(), ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump)
            && instruction.jump_target() as usize <= pc);
    let interrupt_offset = if fault_poll { std::mem::offset_of!(CallContext, poll_start) }
        else { std::mem::offset_of!(CallContext, interrupt) };
    let interrupt = builder.ins().load(types::I64, invariant(), context, interrupt_offset as i32);
    #[cfg(feature = "aot")]
    let helpers = if let LaneHelpers::Linked(symbol) = helpers {
        let signature = builder.import_signature(native_lane_helpers_signature(isa));
        let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
            super::native_object::NATIVE_FUNCTION_NAMESPACE, symbol));
        let target = builder.import_function(ir::ExtFuncData { name: ir::ExternalName::user(name),
            signature, colocated: false, patchable: false });
        let call = builder.ins().call(target, &[]);
        LaneHelpers::Bound(builder.inst_results(call)[0])
    } else { helpers };
    let count = usize::from(unit.registers);
    let masks = unit.register_masks(program, function_id);
    let live = masks.as_deref();
    let registers = Registers {
        tagged: (0..count).map(|_| builder.declare_var(types::I64)).collect(),
        int: (0..count).map(|_| builder.declare_var(types::I32)).collect(),
        float: (0..count).map(|_| builder.declare_var(types::F64)).collect(),
    };
    let zero = builder.ins().iconst(types::I32, 0);
    let float_zero = builder.ins().f64const(0.0);
    let undefined = builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
    for register in 0..count {
        let bits = if !numeric && register < usize::from(unit.caller_registers)
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
    let blocks: Vec<_> = (0..unit.instructions.len()).map(|pc| leaders.contains(pc).then(|| builder.create_block())).collect();
    let mut lowering = Lowering {
        builder,
        program,
        unit,
        helpers,
        fields,
        interrupt,
        fault_poll,
        payload,
        registers_base,
        result,
        registers,
        written: CompoundBitSet::with_capacity(count),
        consumed: CompoundBitSet::with_capacity(unit.instructions.len()),
        blocks,
        exits: vec![None; unit.instructions.len() + 1],
        entry: None,
        native: 0,
        pending: Vec::new(),
        facts: super::facts::Facts::default(),
        live,
        valid: true,
        coverage,
        completion,
    };
    let instructions = unit.instructions.clone();
    for instruction in &instructions {
        for register in writes(*instruction).into_iter().map(usize::from).filter(|r| *r < count) {
            lowering.written.insert(register);
        }
    }
    let mut initial = vec![Type::ANY; count];
    if numeric { initial[..usize::from(function.params)].fill(Type::NUMBER); }
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
                && !numeric && !lowering.written.contains(usize::from(*r))) {
            let current = &mut initial[usize::from(register)];
            if *current != Type::INT32 { *current = required; }
        }
    }
    lowering.facts = super::facts::analyze(&initial, &instructions, &leaders, &program.constants, &writes, &|pc| {
        let instruction = instructions[pc];
        if instruction.op() != Op::GetField || instruction.result_flags() != 0 { return None; }
        let [field] = fields.get(pc)?.as_slice() else { return None };
        let FieldLookup::Atom { base, .. } = instruction.field_lookup()? else { return None };
        Some((base.register_index()?, field.shape))
    });
    if !numeric { lowering.entry = Some(lowering.entry_facts(&instructions)); }
    let first = lowering.blocks[0].expect("pc 0 leads");
    let mut state = State::canonical(count);
    if numeric {
        for register in 0..function.params {
            let number = lowering.builder.ins().load(types::F64, MemFlagsData::trusted(),
                registers_base, register_offset(register));
            lowering.builder.def_var(lowering.registers.float[usize::from(register)], number);
            state.values[usize::from(register)] = RegisterFact::admit(Fact {
                ty: Type::NUMBER, representation: Representation::Float64,
                binding: BindingTime::Location, proof: Proof::EntryGuard, projection: Projection::Exact,
            }).expect("Number input signature is verified by its entry caller");
        }
    }
    if fault_poll {
        // Publication lends a resumable page for this entire no-GC entry.
        // If unavailable, leave before any guest effect through ordinary recovery.
        let miss = lowering.guard_exit(&state, 0);
        let available = lowering.builder.ins().icmp_imm_u(IntCC::NotEqual, interrupt, 0);
        emit_js_guard(&mut lowering.builder, available, miss);
    }
    lowering.conform(&mut state, 0);
    lowering.builder.ins().jump(first, &[]);
    let mut reachable = false;
    for (pc, instruction) in instructions.iter().copied().enumerate() {
        if let Some(block) = lowering.blocks[pc] {
            if reachable {
                lowering.conform(&mut state, pc);
                lowering.builder.ins().jump(block, &[]);
            }
            if numeric && lowering.facts.at(pc).is_none() { reachable = false; continue; }
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
    let snapshot = (lowering.completion == Completion::SourcePc).then(||
        unit.frames.iter().filter(|frame| frame.recovery == super::lane_unit::CallRecovery::Materialize)
            .map(|frame| frame.registers).max()).flatten().map(|registers| {
        let word = types::I64.bytes();
        lowering.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot, u32::from(registers) * word,
            word.trailing_zeros() as u8))
    });
    for unit_pc in 0..lowering.exits.len() {
        let Some(block) = lowering.exits[unit_pc] else { continue };
        lowering.builder.switch_to_block(block);
        for register in lowering.written.iter().map(|register| register as Register)
            .take_while(|register| *register < unit.caller_registers) {
            if !lowering.is_live(unit_pc, register) { continue; }
            let bits = lowering.builder.use_var(lowering.registers.tagged[usize::from(register)]);
            lowering.builder.ins().store(MemFlagsData::trusted(), bits, lowering.registers_base, register_offset(register));
        }
        let word = match unit.origins.get(unit_pc).copied().unwrap_or(super::lane_unit::Origin::Caller(unit.end)) {
            super::lane_unit::Origin::Caller(pc) => u64::from(pc),
            super::lane_unit::Origin::Inlined { frame, pc: source_pc } => {
                let inlined = unit.frames[frame];
                if lowering.completion == Completion::SourcePc
                    && inlined.recovery == super::lane_unit::CallRecovery::RestartCall {
                    // Function-bound leaves are pure or have one final store.
                    // Every possible callee miss precedes any guest effect.
                    lowering.return_at(u64::from(inlined.call));
                    continue;
                }
                let scratch = if lowering.completion == Completion::SourcePc {
                    lowering.builder.ins().stack_addr(types::I64, snapshot.expect("materialized callee snapshot"), 0)
                } else {
                    emit_js_payload_word(&mut lowering.builder, lowering.payload, std::mem::offset_of!(JsStepDispatch, inlined))
                };
                for register in 0..inlined.registers {
                    let bits = if lowering.is_live(unit_pc, inlined.base + register) {
                        lowering.builder.use_var(lowering.registers.tagged[usize::from(inlined.base + register)])
                    } else {
                        lowering.builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64)
                    };
                    lowering.builder.ins().store(MemFlagsData::trusted(), bits, scratch, register_offset(register));
                }
                if lowering.completion == Completion::SourcePc {
                    let function = lowering.builder.ins().iconst(types::I64, i64::from(inlined.target.function));
                    let call = lowering.builder.ins().iconst(types::I64, i64::from(inlined.call));
                    let source = lowering.builder.ins().iconst(types::I64, i64::from(source_pc));
                    let helper = lowering.helpers.address(&mut lowering.builder, LaneHelper::MaterializeInline);
                    emit_lane_helper_call(&mut lowering.builder, lowering.payload, helper,
                        &[lowering.payload, function, call, source, scratch]);
                    lowering.return_at(u64::from(JS_INLINE_MATERIALIZED_PC));
                    continue;
                }
                LaneExit::inlined_word(frame, source_pc)
            }
        };
        lowering.return_at(word);
    }
    if !lowering.valid { return None; }
    let native_count = lowering.native;
    lowering.builder.seal_all_blocks();
    lowering.builder.finalize(isa.frontend_config());
    Some(LoweredRegion { function: native, native: native_count, fault_poll })
}

impl Lowering<'_, '_> {
    /// Boundary projections share one exit PC. Published lane entries retain
    /// their status/word ABI; inlined regions carry the PC directly in SSA.
    fn return_at(&mut self, word: u64) {
        let result = match self.completion {
            Completion::StatusAndPc => {
                let pc = self.builder.ins().iconst(types::I64, word as i64);
                self.builder.ins().store(MemFlagsData::trusted(), pc, self.result, 0);
                self.builder.ins().iconst(types::I32, i64::from(NATIVE_JS_STOPPED))
            }
            Completion::SourcePc => self.builder.ins().iconst(types::I32,
                i64::from(u32::try_from(word).expect("source PC has no inlined-frame tag"))),
            Completion::Number => self.builder.ins().iconst(types::I32, NUMBER_ENTRY_MISS),
        };
        self.builder.ins().return_(&[result]);
    }

    fn return_number(&mut self, state: &mut State, register: Register, pc: u32) {
        if !state.values[usize::from(register)].value.ty.is_subtype(Type::NUMBER) {
            self.valid = false;
            self.return_at(0);
            return;
        }
        self.interrupt_check(state, pc);
        let exit = self.guard_exit(state, pc);
        let number = self.number(state, register, exit);
        let result = self.result;
        self.ins().store(MemFlagsData::trusted(), number, result, 0);
        let success = self.ins().iconst(types::I32, 0);
        self.ins().return_(&[success]);
    }

    fn entry_facts(&mut self, instructions: &[WideInstruction]) -> Entry {
        let this_bits = match self.helpers {
            LaneHelpers::Process(_) => emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, receiver)),
            #[cfg(feature = "aot")]
            LaneHelpers::Bound(_) => {
                let slot = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, fixed_this_slot));
                let present = self.builder.ins().icmp_imm_u(IntCC::NotEqual, slot, 0);
                let bound = self.builder.create_block();
                let absent = self.builder.create_block();
                let join = self.builder.create_block();
                let bits = self.builder.append_block_param(join, types::I64);
                self.builder.ins().brif(present, bound, &[], absent, &[]);
                self.builder.switch_to_block(bound);
                let value = self.builder.ins().load(types::I64, MemFlagsData::trusted(), slot, 0);
                self.builder.ins().jump(join, &[ir::BlockArg::Value(value)]);
                self.builder.switch_to_block(absent);
                let deleted = self.builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
                self.builder.ins().jump(join, &[ir::BlockArg::Value(deleted)]);
                self.builder.switch_to_block(join);
                bits
            }
            #[cfg(feature = "aot")]
            LaneHelpers::Linked(_) => unreachable!("helper table bound before entry facts"),
        };
        let receiver = emit_lane_receiver(&mut self.builder, self.payload, this_bits);
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
            let atom = match instruction.op() {
                Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict => Some(instruction.atom_index()),
                Op::GetField if instruction.writes_current_this() => match instruction.field_lookup() {
                    Some(FieldLookup::Site(site)) => self.program.field_sites.get(site).and_then(|site| site.sink.map(|sink| sink.0)),
                    _ => None,
                },
                _ => None,
            };
            if let Some(atom) = atom.filter(|atom| !store_atoms.contains_key(atom)) {
                let admitted = self.emit_store_atom_admitted(atom);
                let variable = self.builder.declare_var(types::I8);
                self.builder.def_var(variable, admitted);
                store_atoms.insert(atom, variable);
            }
        }
        let mut captures = BTreeMap::new();
        let mut capture_miss = None;
        for (pc, instruction) in instructions.iter().enumerate() {
            if instruction.op() == Op::LoadCapture && crate::bytecode::ImmediateLayout::capture_depth(instruction.imm()) == 0 {
                let key = self.unit.capture_key(pc);
                let (frame, slot) = key;
                if frame.is_some_and(|frame| self.unit.frames[frame].target.identity().is_none()) {
                    continue; // The actual closure exists only at its call edge.
                }
                if captures.contains_key(&key) {
                    continue;
                }
                let slot_value = self.builder.ins().iconst(types::I64, i64::from(slot));
                let (helper, arguments) = if let Some(frame) = frame {
                    let vm = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, vm));
                    let callee = self.builder.ins().iconst(types::I64, self.unit.frames[frame].target.identity().expect("published capture").to_bits() as i64);
                    (LaneHelper::FunctionCapture, vec![vm, callee, slot_value])
                } else {
                    let slots = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, captures));
                    (LaneHelper::Capture, vec![slots, slot_value])
                };
                let helper = self.helpers.address(&mut self.builder, helper);
                let value = emit_lane_call(&mut self.builder, helper, &arguments);
                let miss = *capture_miss.get_or_insert_with(|| self.builder.create_block());
                let admitted = self.builder.create_block();
                let read = self.builder.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
                self.builder.ins().brif(read, admitted, &[], miss, &[]);
                self.builder.switch_to_block(admitted);
                let variable = self.builder.declare_var(types::I64);
                self.builder.def_var(variable, value);
                captures.insert(key, variable);
            }
        }
        #[cfg(feature = "aot")]
        let fields = {
            let mut fields = smallvec::SmallVec::new();
            if matches!(self.helpers, LaneHelpers::Bound(_)) {
                for instruction in instructions {
                    // A field chain names sequential sites, not alternatives for
                    // one polymorphic access. Bind each distinct column once.
                    let mut sites = smallvec::SmallVec::<[u16; 3]>::new();
                    match instruction.op() {
                        Op::GetField => match instruction.field_lookup() {
                            Some(FieldLookup::Atom { cache_site, .. }) => sites.push(cache_site),
                            Some(FieldLookup::Site(site)) => {
                                let site = self.program.field_sites[site];
                                sites.push(site.first.1);
                                if let Some((_, cache)) = site.second { sites.push(cache); }
                                if instruction.writes_current_this()
                                    && let Some((_, cache)) = site.sink { sites.push(cache); }
                            }
                            _ => {},
                        },
                        Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict => sites.push(instruction.cache_site_index()),
                        _ => {},
                    }
                    for site in sites {
                        if fields.iter().any(|(known, _)| *known == site) { continue; }
                        let miss = *capture_miss.get_or_insert_with(|| self.builder.create_block());
                        let mut binding = emit_js_field_binding(&mut self.builder, self.payload, site, miss);
                        binding.slot = FieldSlot::Bound(binding.slot.read(&mut self.builder));
                        fields.push((site, binding));
                    }
                }
            }
            fields
        };
        #[cfg(not(feature = "aot"))]
        let fields = smallvec::SmallVec::new();
        let records = self.invariant_field_records(instructions, &receiver, &mut capture_miss);
        if let Some(miss) = capture_miss {
            let admitted = self.builder.create_block();
            self.builder.ins().jump(admitted, &[]);
            self.builder.switch_to_block(miss);
            self.builder.set_cold_block(miss);
            // No guest operation has run and canonical registers are untouched.
            // Decline the whole unit once, even for a speculative unused capture.
            let super::lane_unit::Origin::Caller(pc) = self.unit.origins[0] else { unreachable!("unit entry belongs to caller") };
            self.return_at(u64::from(pc));
            self.builder.switch_to_block(admitted);
        }
        Entry { receiver, records, receiver_shapes, store_atoms, captures, this, fields }
    }

    /// Bind physical receiver facts once. The expanded unit's write set owns
    /// invariance, including renamed callee registers and synthesized returns.
    fn invariant_field_records(&mut self, instructions: &[WideInstruction], receiver: &LaneReceiver,
        miss: &mut Option<ir::Block>) -> Vec<Option<ObjectRecord>> {
        let count = usize::from(self.unit.registers);
        let mut owners: CompoundBitSet = CompoundBitSet::with_capacity(count + 1);
        let mut reachable = false;
        for (pc, instruction) in instructions.iter().enumerate() {
            if self.blocks[pc].is_some() { reachable = self.facts.at(pc).is_some(); }
            if !reachable { continue; }
            let owner = match instruction.op() {
                Op::GetField => {
                    let base = match instruction.field_lookup() {
                        Some(FieldLookup::Atom { base, .. }) => Some(base),
                        Some(FieldLookup::Site(site)) => self.program.field_sites.get(site).map(|site| site.base),
                        _ => None,
                    };
                    base.and_then(|base| if base == crate::bytecode::FieldBase::THIS { Some(count) }
                        else { base.register_index().map(usize::from) })
                }
                Op::SetField | Op::SetFieldStrict => Some(usize::from(instruction.register_b())),
                Op::SetThisField | Op::SetThisFieldStrict => Some(match self.unit.origins[pc] {
                    super::lane_unit::Origin::Inlined { frame, .. } => usize::from(self.unit.frames[frame].receiver),
                    _ => count,
                }),
                _ => None,
            };
            if let Some(owner) = owner.filter(|owner| *owner == count
                || (*owner < usize::from(self.unit.caller_registers) && !self.written.contains(*owner))) {
                owners.insert(owner);
            }
            if instruction.returns_from_frame() || instruction.op().control_flow_layout() == ControlFlowLayout::Terminal {
                reachable = false;
            }
        }
        let mut admitted = vec![None; count + 1];
        let mut records = None;
        for owner in owners.iter() {
            let exit = *miss.get_or_insert_with(|| self.builder.create_block());
            admitted[owner] = Some(if owner == count {
                // emit_lane_receiver already owns family/range/base admission.
                let record = self.builder.use_var(receiver.object);
                let present = self.ins().icmp_imm_u(IntCC::NotEqual, record, 0);
                emit_js_guard(&mut self.builder, present, exit);
                ObjectHeader { record, shape: self.builder.use_var(receiver.shape),
                    inline: self.builder.use_var(receiver.inline) }.record()
            } else {
                let records = *records.get_or_insert_with(|| emit_js_payload_word(&mut self.builder,
                    self.payload, std::mem::offset_of!(JsStepDispatch, object_record_base)));
                let bits = self.builder.use_var(self.registers.tagged[owner]);
                ObjectRecord::prove(&mut self.builder, bits, records, None, exit)
            });
        }
        admitted
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
        if self.consumed.contains(pc) {
            return true;
        }
        let pc32 = pc as u32;
        let plain = instruction.result_flags() == 0;
        let ran = match instruction.op() {
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
                    let fact = RegisterFact::admit(Fact { ty, representation,
                        binding: BindingTime::Static, proof: Proof::Static, projection: Projection::Exact })
                        .expect("constant's canonical semantic type");
                    self.define(state, instruction.result_register(), native, fact);
                    true
                })
            }
            Op::Move if plain => {
                self.copy(state, instruction.result_register(), instruction.register_b());
                Some(true)
            }
            Op::Return if plain && self.completion == Completion::Number => {
                self.return_number(state, instruction.register_a(), pc32);
                Some(false)
            }
            Op::Call if guarded_call_instruction(instruction) => {
                let exit = self.guard_exit(state, pc32);
                let callee = self.tagged(state, instruction.register_b());
                let receiver = self.tagged(state, instruction.register_c());
                let helper = self.helpers.address(&mut self.builder, LaneHelper::GuardedCall);
                let program = self.builder.ins().load(types::I32, MemFlagsData::trusted(), self.payload,
                    std::mem::offset_of!(JsStepDispatch, program_id) as i32);
                let program = self.builder.ins().uextend(types::I64, program);
                let result = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[program, callee, receiver]);
                let hit = self.ins().icmp_imm_u(IntCC::NotEqual, result, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, hit, exit);
                self.set_tagged(state, instruction.result_register(), result);
                Some(true)
            }
            Op::LoadThis if plain && !instruction.writes_current_this() => {
                let exit = self.guard_exit(state, pc32);
                let entry = self.entry.as_ref().expect("entry facts");
                let this = self.builder.use_var(entry.this);
                let bound = self.ins().icmp_imm_u(IntCC::NotEqual, this, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, bound, exit);
                self.set_tagged(state, instruction.result_register(), this);
                state.headers[usize::from(instruction.result_register())] = state.headers[state.values.len()];
                state.receivers.insert(instruction.result_register());
                Some(true)
            }
            Op::LoadCapture if plain && crate::bytecode::ImmediateLayout::capture_depth(instruction.imm()) == 0 => {
                let key = self.unit.capture_key(pc);
                if let Some(frame) = key.0.filter(|frame| self.unit.frames[*frame].target.identity().is_none()) {
                    let exit = self.guard_exit(state, pc32);
                    let callee = self.tagged(state, self.unit.frames[frame].callee_register);
                    let vm = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, vm));
                    let slot = self.ins().iconst(types::I64, i64::from(key.1));
                    let helper = self.helpers.address(&mut self.builder, LaneHelper::FunctionCapture);
                    let value = emit_lane_call(&mut self.builder, helper, &[vm, callee, slot]);
                    let hit = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
                    emit_js_guard(&mut self.builder, hit, exit);
                    self.set_tagged(state, instruction.result_register(), value);
                    Some(true)
                } else {
                    let variable = self.entry.as_ref().and_then(|entry| entry.captures.get(&key)).copied();
                    variable.map(|variable| {
                        let value = self.builder.use_var(variable);
                        self.set_tagged(state, instruction.result_register(), value);
                        true
                    })
                }
            }
            Op::GetField if plain || instruction.result_flags() == crate::bytecode::SET_THIS_REGISTER => {
                match instruction.field_lookup() {
                    Some(FieldLookup::Site(site)) => self.get_field_site(state, pc, instruction, site),
                    _ if plain => self.get_field(state, pc, instruction),
                    _ => None,
                }
            },
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
                    if let Some(header) = instructions.get(target).copied()
                        && matches!(header.op(), Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse)
                        && header.result_flags() == 0
                        && header.jump_target() as usize > pc
                    {
                        // Rotate only the pure header test. The first visit
                        // keeps the zero-trip guard; each backedge keeps its
                        // poll and exact source-header recovery state.
                        return self.instruction(state, target, header, instructions);
                    }
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
                if self.coverage == Coverage::Full && (self.completion == Completion::Number
                    || !((instruction.op() == Op::Return && plain) || instruction.returns_from_frame())) {
                    self.valid = false;
                }
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
        let header = state.headers[usize::from(source)];
        let receiver = state.receivers.contains(&source);
        let layouts: Vec<_> = state.layouts.iter().filter_map(|((owner, shape), projection)|
            (*owner == Some(source)).then_some((*shape, *projection))).collect();
        self.define(state, destination, value, fact);
        if let Some(bits) = tagged { self.builder.def_var(self.registers.tagged[usize::from(destination)], bits); }
        state.headers[usize::from(destination)] = header;
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
        if let Some(identity) = inlined.target.identity() {
            if state.values[usize::from(inlined.callee_register)].value.ty.function_identity() != Some(identity) {
                let same = self.ins().icmp_imm_u(IntCC::Equal, callee, identity.to_bits() as i64);
                emit_js_guard(&mut self.builder, same, exit);
                state.values[usize::from(inlined.callee_register)] = RegisterFact::admit(Fact {
                    ty: Type::function(identity).expect("admitted callee locator"),
                    representation: Representation::Tagged,
                    binding: BindingTime::Location, proof: Proof::EntryGuard, projection: Projection::Exact,
                }).expect("published function identity and dominating value check");
            }
        } else {
            let program = self.builder.ins().load(types::I32, MemFlagsData::trusted(), self.payload,
                std::mem::offset_of!(JsStepDispatch, program_id) as i32);
            let program = self.ins().uextend(types::I64, program);
            let function = self.ins().iconst(types::I64, i64::from(inlined.target.function));
            let helper = self.helpers.address(&mut self.builder, LaneHelper::FunctionBinding);
            let admitted = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[program, callee, function]);
            let hit = self.ins().icmp_imm_u(IntCC::NotEqual, admitted, 0);
            emit_js_guard(&mut self.builder, hit, exit);
            state.values[usize::from(inlined.callee_register)] = RegisterFact::admit(Fact {
                ty: Type::language(crate::ValueType::Function), representation: Representation::Tagged,
                binding: BindingTime::Location, proof: Proof::EntryGuard, projection: Projection::Exact,
            }).expect("admitted dynamic closure of the static function");
        }
        let receiver = self.tagged(state, inlined.receiver);
        let entry = self.entry.as_ref().expect("entry facts");
        let admitted = state.headers[usize::from(inlined.receiver)].is_some()
            || entry.records[usize::from(inlined.receiver)].is_some()
            || (state.receivers.contains(&inlined.receiver) && entry.records[state.values.len()].is_some());
        if !admitted {
            let family = self.ins().band_imm_u(receiver, crate::value::ValueLayout::HEAP_FAMILY_MASK as i64);
            let object = self.ins().icmp_imm_u(IntCC::Equal, family, crate::value::ValueLayout::DIRECT_OBJECT_TAG as i64);
            emit_js_guard(&mut self.builder, object, exit);
        }
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

    fn interrupt_check(&mut self, state: &State, pc: u32) {
        let flag = self.interrupt;
        let requested = self.ins().atomic_load(types::I8, MemFlagsData::trusted(), flag);
        if self.fault_poll { return; }
        let exit = self.guard_exit(state, pc);
        let requested = self.ins().uextend(types::I32, requested);
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
        let receiver = base == crate::bytecode::FieldBase::THIS
            || register.is_some_and(|register| state.receivers.contains(&register));
        if !shapes.is_empty() {
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
                let header = self.object_header(state, Some(register.expect("a register base")), exit);
                (header.record, header.shape, header.inline)
            };
            let owner = if receiver { None } else { register };
            let value = self.shape_switch_read(state, owner, object, shape, inline, &shapes, exit)?;
            if let [field] = shapes.as_slice() && let Some(register) = register {
                state.values[usize::from(register)] = RegisterFact::definition(
                    Type::direct_object(field.shape), Representation::Tagged);
            }
            self.set_tagged(state, destination, value);
            return Some(true);
        }
        let object_bits = if receiver && register.is_none() {
            let entry = self.entry.as_ref().expect("entry facts");
            self.builder.use_var(entry.this)
        } else {
            self.tagged(state, register?)
        };
        let owner = if receiver { FieldOwner::This } else { FieldOwner::Register(register?) };
        let value = self.field_probe(state, owner, object_bits, cache_site, exit);
        self.set_tagged(state, destination, value);
        Some(true)
    }

    /// The same fused read/read/sink order as `resolve_field` and the fast lane.
    /// Intermediate reads remain SSA temporaries: any miss can safely repeat
    /// them at the original PC, even when the destination aliases the base.
    fn get_field_site(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, site: usize) -> Option<bool> {
        let site = *self.program.field_sites.get(site)?;
        let register = site.base.register_index();
        let (base, owner) = match register {
            Some(register) if register < self.unit.registers => {
                let owner = if state.receivers.contains(&register) { FieldOwner::This } else { FieldOwner::Register(register) };
                (self.tagged(state, register), owner)
            }
            None if site.base == crate::bytecode::FieldBase::THIS =>
                (self.builder.use_var(self.entry.as_ref().expect("entry facts").this), FieldOwner::This),
            _ => return None,
        };
        let sink = if instruction.writes_current_this() { Some(site.sink?) } else { None };
        let exit = self.guard_exit(state, pc as u32);
        let mut value = self.field_probe(state, owner, base, site.first.1, exit);
        if let Some((_, cache)) = site.second {
            value = self.field_probe(state, FieldOwner::Temporary, value, cache, exit);
        }
        if let Some((atom, cache)) = sink {
            let object = self.builder.use_var(self.entry.as_ref().expect("entry facts").this);
            let bound = self.entry.as_ref().is_some_and(|entry| entry.fields.iter().any(|(site, _)| *site == cache));
            let header = bound.then(|| self.object_header(state, None, exit));
            self.store_helper(object, atom, cache, value, exit, header);
        }
        // The only write succeeded; no fallible operation may follow it at
        // this PC. Successor edge checks recover at the successor's source PC.
        self.set_tagged(state, instruction.result_register(), value);
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

    /// A dominating direct-object proof for this value's lifetime in the block.
    fn object_header(&mut self, state: &mut State, owner: Option<Register>, exit: ir::Block) -> ObjectHeader {
        let index = owner.map_or(state.values.len(), usize::from);
        if let Some(header) = state.headers[index] { return header; }
        let fact = owner.map(|register| state.values[usize::from(register)].value);
        let header = if let Some(record) = self.entry.as_ref().expect("entry facts").records[index] {
            record.header(&mut self.builder, fact)
        } else {
            let bits = match owner {
                Some(register) => self.tagged(state, register),
                None => self.builder.use_var(self.entry.as_ref().expect("entry facts").this),
            };
            let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
            ObjectHeader::prove(&mut self.builder, bits, records, fact, exit)
        };
        state.headers[index] = Some(header);
        if let Some(fact) = fact {
            let ty = fact.ty;
            state.values[index] = RegisterFact::definition(
                if ty.object_shape().is_some() { ty } else { Type::language(crate::ValueType::Object) },
                Representation::Tagged);
        }
        header
    }

    /// The VM's own field-cache read (`Vm::cached_field_hit`), prototype holders included;
    /// a miss leaves through `exit`.
    fn field_probe(&mut self, state: &mut State, owner: FieldOwner, object: ir::Value, site: u16, exit: ir::Block) -> ir::Value {
        let bound = self.entry.as_ref().and_then(|entry| entry.fields.iter()
            .find_map(|(known, binding)| (*known == site).then_some(*binding)));
        if let Some(binding) = bound {
            let header = match owner {
                FieldOwner::This => Some(self.object_header(state, None, exit)),
                FieldOwner::Register(register) => Some(self.object_header(state, Some(register), exit)),
                FieldOwner::Temporary => None,
            };
            let slow = self.builder.create_block();
            let done = self.builder.create_block();
            let result = self.builder.append_block_param(done, types::I64);
            let (address, _) = emit_js_cached_field_slot(&mut self.builder, self.registers_base, self.payload,
                JsFieldReceiver::Value(object), site, true, slow, Some(binding), header);
            let value = self.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
            self.ins().jump(done, &[ir::BlockArg::Value(value)]);
            self.builder.switch_to_block(slow);
            let value = self.field_probe_unbound(object, site, exit);
            self.ins().jump(done, &[ir::BlockArg::Value(value)]);
            self.builder.switch_to_block(done);
            return result;
        }
        self.field_probe_unbound(object, site, exit)
    }

    fn field_probe_unbound(&mut self, object: ir::Value, site: u16, exit: ir::Block) -> ir::Value {
        let site = self.ins().iconst(types::I64, i64::from(site));
        let helper = self.helpers.address(&mut self.builder, LaneHelper::FieldGet);
        let value = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[object, site]);
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
        let receiver = register.is_none() || register.is_some_and(|register| state.receivers.contains(&register));
        let owner = if receiver { None } else { register };
        let exit = self.guard_exit(state, pc as u32);
        let atom = instruction.atom_index();
        let barrier_free = state.values[usize::from(source)].value.ty.is_subtype(Type::NUMBER);
        let value = self.tagged(state, source);
        let object_bits = if receiver {
            self.builder.use_var(self.entry.as_ref().expect("entry facts").this)
        } else { self.tagged(state, register.expect("register receiver")) };
        let field = self.fields.get(pc).and_then(|shapes| (shapes.len() == 1).then(|| shapes[0]));
        let header = if field.is_some() && !receiver {
            let header = self.object_header(state, Some(register.expect("register receiver")), exit);
            Some((header.record, header.shape, header.inline))
        } else { None };
        // Prove before either store branch so reuse always has a dominating
        // certificate, including the branch that performs the helper fallback.
        let bound = self.entry.as_ref().is_some_and(|entry| entry.fields.iter()
            .any(|(site, _)| *site == instruction.cache_site_index()));
        let dynamic_header = bound.then(|| self.object_header(state, owner, exit));
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
            let address = projection.arena_address(&mut self.builder, field.slot, general)?;
            self.ins().store(MemFlagsData::trusted(), value, address, 0);
            self.ins().jump(done, &[]);
            self.builder.switch_to_block(general);
            self.store_helper(object_bits, atom, instruction.cache_site_index(), value, exit, dynamic_header);
            self.ins().jump(done, &[]);
            self.builder.switch_to_block(done);
            return Some(true);
        }
        self.store_helper(object_bits, atom, instruction.cache_site_index(), value, exit, dynamic_header);
        Some(true)
    }

    /// The lane's logged own-field store hit (`Vm::primary_own_field_store_hit`), barrier
    /// included; anything else leaves through `exit`.
    fn store_helper(&mut self, object: ir::Value, atom: Atom, site: u16, value: ir::Value, exit: ir::Block, header: Option<ObjectHeader>) {
        let bound = self.entry.as_ref().and_then(|entry| entry.fields.iter()
            .find_map(|(known, binding)| (*known == site).then_some(*binding)));
        if let Some(binding) = bound {
            let slow = self.builder.create_block();
            let done = self.builder.create_block();
            let allowed = self.builder.use_var(self.entry.as_ref().expect("entry").store_atoms[&atom]);
            emit_js_guard(&mut self.builder, allowed, slow);
            let (address, record) = emit_js_cached_field_slot(&mut self.builder, self.registers_base, self.payload,
                JsFieldReceiver::Value(object), site, false, slow, Some(binding), header);
            emit_unlogged_property_store_guard(&mut self.builder, self.payload, record, value, slow);
            self.ins().store(MemFlagsData::trusted(), value, address, 0);
            self.ins().jump(done, &[]);
            self.builder.switch_to_block(slow);
            self.store_helper_unbound(object, atom, site, value, exit);
            self.ins().jump(done, &[]);
            self.builder.switch_to_block(done);
            return;
        }
        self.store_helper_unbound(object, atom, site, value, exit);
    }

    fn store_helper_unbound(&mut self, object: ir::Value, atom: Atom, site: u16, value: ir::Value, exit: ir::Block) {
        let atom = self.ins().iconst(types::I64, i64::from(atom));
        let site = self.ins().iconst(types::I64, i64::from(site));
        let helper = self.helpers.address(&mut self.builder, LaneHelper::FieldSet);
        let stored = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[object, atom, site, value]);
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
        if let Some(arithmetic) = arithmetic
            && arithmetic.int32_projection_is_wrapping()
            && instruction.result_flags() == 0
            && let Some(next) = instructions.get(pc + 1).copied()
            && next.result_flags() == 0
            && next.op() == Op::Binary
            && super::operations::NumberBitwise::from_js_instruction(next.op(), next.binary_operator())
                == Some(super::operations::NumberBitwise::Or)
            && next.operand_b().register_index() == Some(destination)
            && super::facts::constant_number(&self.program.constants, next.operand_c()) == Some(0.0)
            && self.blocks[pc + 1].is_none()
            && self.live.is_some_and(|live| next.result_register() == destination
                || !live.is_live(pc + 2, destination))
        {
            // No source edge or observer can see the intermediate Number.
            // A failed input proof resumes before the arithmetic, and the
            // consumer cannot fail once these signed-int32 inputs are proved.
            let left = self.operand_int(state, left, exit)?;
            let right = self.operand_int(state, right, exit)?;
            let bits = arithmetic.apply(&mut EmitWord32Arithmetic { builder: &mut self.builder }, left, right);
            self.set_int(state, next.result_register(), bits);
            self.consumed.insert(pc + 1);
            return Some(true);
        }
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
                            let Some(mut fact) = RegisterFact::admit(Fact {
                                ty: current.value.ty.join(incoming.value.ty), representation,
                                binding: BindingTime::Dynamic, proof: Proof::Definition,
                                projection: Projection::Exact,
                            }) else { self.valid = false; return None; };
                            fact.tagged_current = current.tagged_current && incoming.tagged_current;
                            state.values[index] = fact;
                        }
                    }
                    self.set_number(state, destination, number);
                }
            }
            (_, Some(bitwise)) => {
                let left_int = self.operand_int(state, left, exit).expect("supported operand");
                let right_int = self.operand_int(state, right, exit).expect("supported operand");
                let right_constant = super::facts::constant_number(&self.program.constants, right);
                if !bitwise.result_is_int32(right_constant) {
                    let number = emit_number_bitwise_operation(&mut self.builder, bitwise, left_int, right_int);
                    self.set_number(state, destination, number);
                } else {
                    let result = emit_number_bitwise_bits(&mut self.builder, bitwise, left_int, right_int);
                    self.set_int(state, destination, result);
                }
            }
            _ if super::operations::NumberRelation::from_operator(operator).is_some() => {
                let integer = [left, right].iter().all(|operand| super::facts::operand_type(
                    &self.program.constants, *operand, |register| state.values[usize::from(register)].value.ty)
                    .is_subtype(Type::INT32));
                let (left, right, domain) = if integer {
                    (self.operand_int(state, left, exit)?, self.operand_int(state, right, exit)?, NumberRelationDomain::Int32)
                } else {
                    (self.operand_number(state, left, exit)?, self.operand_number(state, right, exit)?, NumberRelationDomain::Float64)
                };
                let holds = emit_number_relation(&mut self.builder, operator, left, right, domain)
                    .expect("admitted numeric relation");
                let yes = self.ins().iconst(types::I64, Value::TRUE.to_bits() as i64);
                let no = self.ins().iconst(types::I64, Value::FALSE.to_bits() as i64);
                let value = self.ins().select(holds, yes, no);
                self.define(state, destination, value, RegisterFact::definition(
                    Type::language(crate::ValueType::Boolean), Representation::Tagged));
            }
            _ => return None,
        }
        if let Some(terminal) = after {
            self.native += 1;
            if self.completion == Completion::Number {
                self.return_number(state, destination, pc as u32);
                return Some(false);
            }
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
    fn unavailable_fault_page_declines_before_changing_canonical_registers() {
        if !super::super::FAULT_POLL_SUPPORTED { return; }
        let program = crate::Engine::specialize(
            "function count(n){n=n|0;let sum=0;for(let i=0;i<n;i=(i+1)|0)sum=(sum+1)|0;return sum;}",
            "missing-poll.js").unwrap();
        let function = program.function(1);
        let unit = super::super::lane_unit::unit(&program, function, &|_| None, &|_| None).unwrap();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let helpers = LaneHelpers::Process((*LaneHelper::addresses::<crate::test_host::TestHost>()).map(|address| address as usize));
        let region = lower(&program, 1, &unit, helpers, &[], &*isa, Coverage::Full, Completion::StatusAndPc).unwrap();
        assert!(region.fault_poll);
        let code = compile_native_function(region.function, &*isa, usize::MAX, CodeMemory::new).unwrap();
        let entry = unsafe { std::mem::transmute::<*mut u8, super::super::Entry>(code.pointer()) };
        let mut frame = super::super::LaneFrame::new();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        frame.dispatch.refresh_object_bases(&vm);
        frame.dispatch.bind_property_buffer(&mut vm);
        frame.context.adapter_payload = (&mut frame.dispatch as *mut JsStepDispatch).cast();
        assert_eq!(frame.context.poll_start, 0);
        let before = vec![Value::number(7.0); usize::from(unit.caller_registers)];
        let mut registers = before.clone();
        let mut word = u64::MAX;
        // No resumable page is lent, so this entry must leave before touching a
        // poll or executing any guest operation. The canonical inputs stay exact.
        let status = unsafe { entry(&mut frame.context, 0, 0, registers.as_mut_ptr().cast(), &mut word) };
        assert_eq!(status, NATIVE_JS_STOPPED);
        assert_eq!(word, 0);
        assert_eq!(registers, before);
    }

    #[test]
    fn object_shape_proofs_survive_verified_control_flow_edges() {
        let program = crate::Engine::specialize(
            "function subject(o, flag) { let value=o.left; if(flag) value+=1; return value+o.right; }",
            "shape-edges.js",
        ).unwrap();
        let function = program.function(1);
        let fields: Vec<_> = function.instructions().map(|instruction| {
            if instruction.op() == Op::GetField
                && let Some(FieldLookup::Atom { atom, .. }) = instruction.field_lookup()
                && let Some(slot) = ["left", "right"].iter().position(|name| *name == &program.atoms[atom as usize]) {
                vec![NativeFieldLoad { shape: 0, slot: slot as u16, storage_len: 2, storage: None }]
            } else { Vec::new() }
        }).collect();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let unit = super::super::lane_unit::unit(&program, function, &|_| None, &|_| None).unwrap();
        let native = lower(&program, 1, &unit,
            LaneHelpers::Process([0; LaneHelper::COUNT]), &fields, &*isa, Coverage::Prefix, Completion::StatusAndPc).unwrap().function;
        let shapes = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
            .filter(|&instruction| matches!(native.dfg.insts[instruction], ir::InstructionData::Load { offset, .. }
                if i32::from(offset) == crate::heap::NATIVE_OBJECT_LAYOUT.auxiliary_offset as i32
                    && native.dfg.value_type(native.dfg.inst_results(instruction)[0]) == types::I32))
            .count();
        // The implicit receiver has its entry projection; o needs one shape
        // proof, shared by both paths to its second field read.
        assert_eq!(shapes, 2, "the CFG must carry the proven shape:\n{native}");
    }

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
        let unit = super::super::lane_unit::unit(&program, program.function(id), &|_| None, &|_| None).unwrap();
        let native = lower(&program, id as u32,
            &unit, LaneHelpers::Process([0; LaneHelper::COUNT]), &fields, &*isa, Coverage::Prefix, Completion::StatusAndPc).unwrap().function;
        let bounds = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
            .filter(|&instruction| {
                let ir::InstructionData::IntCompare { cond: IntCC::UnsignedLessThanOrEqual, args, .. }
                    = native.dfg.insts[instruction] else { return false };
                let ir::ValueDef::Result(length, _) = native.dfg.value_def(native.dfg.resolve_aliases(args[1]))
                    else { return false };
                matches!(native.dfg.insts[length], ir::InstructionData::Load { offset, .. }
                    if i32::from(offset) == crate::value_vec::NATIVE_PROPERTY_BUFFER_LAYOUT.length as i32)
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
        let unit = super::super::lane_unit::unit(&program, program.function(id), &|_| None, &|_| None).unwrap();
        let region = lower(&program, id as u32,
            &unit, LaneHelpers::Process([0; LaneHelper::COUNT]), &[], &*isa, Coverage::Prefix, Completion::StatusAndPc).expect("typed Number residual");
        assert!(region.native > 0);
        let native = region.function;
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
