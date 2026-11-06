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
use crate::value_vec::SlotRepresentation;
use std::collections::{BTreeMap, BTreeSet};

const LANE_REGION_NAMESPACE: u32 = u32::from_be_bytes(*b"QLRG");
/// The executable memory a lane body may take: what a unit at its instruction bound
/// (`lane_unit::MAX_UNIT_INLINED_INSTRUCTIONS`) lowers to, at the bytes per instruction its
/// guards and exits cost. A larger body is planned again with less inlined
/// (`Vm::compile_lane_method`).
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
    /// The owners of `shapes` whose proof a read made, which may name a function's record
    /// (`ObjectHeader::prove_readable`): a store proves them again as objects.
    readable: BTreeSet<Option<Register>>,
    /// A fixed receiver (`None`) or register's complete own-slot projection.
    layouts: BTreeMap<(Option<Register>, u32), CheckedObject>,
    /// Registers holding the receiver, whose facts the entry proved.
    receivers: BTreeSet<Register>,
    /// The `heap::prototype_value_epoch` this block proved current. Only a helper that may
    /// run JavaScript or write a prototype changes it; each such helper retires the proof.
    epoch: Option<u64>,
}

impl State {
    fn canonical(registers: usize) -> Self {
        Self { values: vec![RegisterFact::canonical(); registers], shapes: BTreeMap::new(),
            readable: BTreeSet::new(), layouts: BTreeMap::new(), receivers: BTreeSet::new(), epoch: None }
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
    /// The run's interrupt flag's address (`CallContext::interrupt`), fixed for the run.
    interrupt: Variable,
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
    /// The by-name loads that read the global scope (`Vm::lane_global_reads`).
    global_reads: &'a BTreeSet<usize>,
    /// The VM's facts: its `length` name, whose read of a direct array is its record's
    /// (`array_length`), and whether an IsHTMLDDA object can exist (`emit_nullish_branch`).
    vm: VmFacts,
    /// The unit PCs a previous body left at (`Vm::compile_lane_method`): a construction there
    /// runs by the call protocol (`Lowering::construct`).
    generic: &'a [usize],
    /// The calls, by unit PC, that call a callee's compiled entry (`DirectCallee`).
    direct: &'a BTreeMap<usize, *const DirectCallee>,
    /// Per inlined frame, its callee's dense capture slots (values and count), fixed by the
    /// callee's identity, which its call guards; null where they are not dense.
    frame_captures: &'a [super::FrameCaptures],
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
    /// With the lowering line that made each guard, which a diagnostics build records.
    pending: Vec<(ir::Block, State, u32, u32)>,
    /// Whether the unit makes native calls (`Lowering::call`): a callee may change `this`,
    /// so no read or store folds the receiver facts its entry proved.
    native_calls: bool,
    /// The registers' types at each block leader (`facts::analyze`).
    facts: super::facts::Facts,
    /// The PCs whose int32 sum or difference may wrap (`Lowering::wraps_at`), resolved once.
    wrapping: PcSet,
    /// The caller's register liveness, by its PCs; per inlined frame, its callee's
    /// (`Lowering::is_live`). A function without masks keeps every register live.
    live: Option<&'a crate::compile::liveness::RegisterMasks>,
    frame_live: Vec<Option<&'a crate::compile::liveness::RegisterMasks>>,
    valid: bool,
    /// The lowering line that first found the body invalid, for the diagnostics report.
    invalid_at: u32,
    /// Per loop header, the reads its loop repeats unchanged (`invariant_reads`), made once
    /// on the loop's one entry edge; each read's PC names the value it made there and
    /// whether its int tag was tested.
    hoists: BTreeMap<usize, Hoist>,
    hoisted: BTreeMap<usize, (Variable, bool)>,
    /// Per array register a loop reads elements of, its elements' view, proved at the loop's
    /// entry (`Lowering::hoist_reads`): a header and a length, zero where the register held
    /// no direct array, so every read of it takes the general one.
    /// Its loop's extent bounds the reads that take it.
    hoisted_arrays: BTreeMap<Register, (Variable, Variable, std::ops::RangeInclusive<usize>)>,
    /// The PC being lowered, whose edges to a header enter its loop or close it.
    at: usize,
    /// Per block leader, its forward edges (`forward_edges`), and the states the lowered
    /// ones arrived with: a block they all reach keeps the object proofs they share.
    forward: Vec<Option<usize>>,
    arrived: BTreeMap<usize, Vec<State>>,
    /// The block leaders an entry or a lowered edge enters: a block none does is not
    /// lowered, its code reached only past a leave.
    entered: Vec<bool>,
    /// Whether a run can do anything but leave: complete natively or run a loop natively. A
    /// body that can do neither declines (`lower`).
    finishes: bool,
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
        self.require(fact.value.verify());
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

    /// The tagged Boolean a predicate `holds` selects.
    fn boolean(&mut self, holds: ir::Value) -> ir::Value {
        let (yes, no) = (self.ins().iconst(types::I64, Value::TRUE.to_bits() as i64), self.ins().iconst(types::I64, Value::FALSE.to_bits() as i64));
        self.ins().select(holds, yes, no)
    }

    fn set_boolean(&mut self, state: &mut State, register: Register, boolean: ir::Value) {
        self.define(state, register, boolean, RegisterFact::definition(Type::language(crate::ValueType::Boolean), Representation::Tagged));
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
        self.unbox_int(state, register, bits)
    }

    /// The int32 of tagged `bits` that `register` holds and a test proved int32, its
    /// unboxed view from here on.
    fn unbox_int(&mut self, state: &mut State, register: Register, bits: ir::Value) -> ir::Value {
        let index = usize::from(register);
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

    /// The lane's own read of `LoadCapture` `instruction`'s binding (`dispatch_fast::lane_body_capture`):
    /// the value, or `Value::DELETED` where the lane leaves for it.
    fn capture_read(&mut self, instruction: WideInstruction) -> ir::Value {
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
        let depth = u64::from(crate::bytecode::ImmediateLayout::capture_depth(instruction.imm()));
        let slot = u64::from(crate::bytecode::ImmediateLayout::capture_slot(instruction.imm()));
        let packed = self.ins().iconst(types::I64, (depth << u16::BITS | slot) as i64);
        emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.capture_get, &[context, packed])
    }

    /// A guard's failure block: it leaves at `pc` from the state where the guard sits. It is
    /// filled after the region, since the current block is still open.
    #[track_caller]
    fn guard_exit(&mut self, state: &State, pc: u32) -> ir::Block {
        let block = self.builder.create_block();
        self.builder.set_cold_block(block);
        self.pending.push((block, state.clone(), pc, std::panic::Location::caller().line()));
        block
    }

    fn continue_at(&mut self, state: &mut State, pc: usize) {
        // A block before this one that no edge entered was not lowered.
        let lowered = pc > self.at || self.entered.get(pc).copied().unwrap_or(false);
        match self.blocks.get(pc).copied().flatten().filter(|_| lowered) {
            Some(block) => {
                self.entered[pc] = true;
                self.finishes |= pc <= self.at;
                if self.at < pc { self.hoist_reads(state, pc); }
                self.conform(state, pc);
                if self.at < pc { self.arrive(state, pc); }
                self.ins().jump(block, &[]);
            }
            None => self.exit(state, pc as u32),
        }
    }

    /// Record `state` arriving at block leader `pc` by one of its forward edges.
    fn arrive(&mut self, state: &State, pc: usize) {
        if self.forward.get(pc).copied().flatten().is_some() {
            self.arrived.entry(pc).or_default().push(state.clone());
        }
    }

    /// The object proofs every forward edge into `pc` arrived with, as the same values, for
    /// a block no other edge enters: each was made in a block over all of them, so over this.
    fn keep_shared_proofs(&mut self, state: &mut State, pc: usize) {
        let Some(edges) = self.forward.get(pc).copied().flatten() else { return };
        let Some(arrived) = self.arrived.remove(&pc) else { return };
        let Some((first, rest)) = arrived.split_first().filter(|_| arrived.len() == edges) else { return };
        state.shapes = first.shapes.iter().filter(|(owner, proof)| rest.iter()
            .all(|other| other.shapes.get(owner) == Some(proof))).map(|(owner, proof)| (*owner, *proof)).collect();
        state.readable = arrived.iter().flat_map(|arrived| arrived.readable.iter().copied()).collect();
        state.layouts = first.layouts.iter().filter(|(key, projection)| rest.iter()
            .all(|other| other.layouts.get(key) == Some(projection))).map(|(key, projection)| (*key, *projection)).collect();
        state.receivers = first.receivers.iter().filter(|register| rest.iter()
            .all(|other| other.receivers.contains(register))).copied().collect();
        state.epoch = first.epoch.filter(|epoch| rest.iter().all(|other| other.epoch == Some(*epoch)));
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
        if let Some(types) = self.facts.at(pc).map(<[Type]>::to_vec) {
            for (index, ty) in types.into_iter().enumerate() {
                if !self.is_live(pc, index as Register) { continue; }
                let representation = if ty == Type::INT32 { Representation::Int32 }
                    else if ty == Type::NUMBER { Representation::Float64 }
                    else { Representation::Tagged };
                let mut fact = RegisterFact::definition(ty, representation);
                if pc == 0 {
                    fact.value.binding = BindingTime::Location;
                    fact.value.proof = if ty == Type::ANY { Proof::CanonicalValue } else { Proof::EntryGuard };
                }
                self.require(fact.value.verify());
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

/// Whether `instruction` reads a field as an int32 (`Lowering::get_field`): a lookup with no
/// sink or completion whose last read's slots have only held int32 values, its value's tag
/// tested at the read.
fn int32_read(program: &ResidualProgram, fields: &BTreeMap<u16, Vec<NativeFieldLoad>>, instruction: WideInstruction) -> bool {
    if instruction.op() != Op::GetField || instruction.returns_from_frame() || instruction.writes_current_this() {
        return false;
    }
    let last = match instruction.field_lookup() {
        Some(FieldLookup::Atom { cache_site, .. }) => cache_site,
        Some(FieldLookup::Site(index)) => match program.field_sites.get(index) {
            Some(site) => site.second.unwrap_or(site.first).1,
            None => return false,
        },
        None => return false,
    };
    fields.get(&last).is_some_and(|shapes| !shapes.is_empty()
        && shapes.iter().all(|field| field.representation == SlotRepresentation::Int32))
}

/// Whether `instruction` can change no heap value natively: anything else it would do (a
/// non-Number operand, a missed cache) leaves the body first.
fn reads_only(constants: &[crate::bytecode::Constant], instruction: WideInstruction) -> bool {
    match instruction.op() {
        Op::Move | Op::Nop | Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse => true,
        Op::LoadConst => constants.get(instruction.constant_index()).and_then(super::facts::immediate).is_some(),
        Op::IncDec => instruction.numeric_local_target().is_none(),
        Op::GetField => !instruction.writes_current_this() && !instruction.returns_from_frame(),
        // A dense element, read natively; any other element leaves (`emit_dense_element_read`).
        Op::GetIndex => !instruction.returns_from_frame(),
        op => op.js_denotation() == crate::bytecode::JsDenotation::NumericBinary && !instruction.returns_from_frame(),
    }
}

/// Whether `instruction` is a store natively: of its atom, into a slot the object already
/// has, so it changes no shape, layout or other property; anything else leaves first.
fn stores_own(instruction: WideInstruction) -> bool {
    matches!(instruction.op(), Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict)
        && !instruction.returns_from_frame()
}

/// What a loop repeats unchanged, made once at its entry: its field reads (by PC), and the
/// elements view of each array register it reads elements of.
#[derive(Clone, Default)]
struct Hoist {
    reads: Vec<usize>,
    arrays: Vec<Register>,
    /// The loop's last PC, its backward jump.
    back: usize,
}

/// Per loop header, the field reads its loop repeats unchanged: a loop `[header, back]`
/// entered by one edge from before it and no other, of instructions that only read
/// (`reads_only`) or store their own atom (`stores_own`), whose reads of a register it never
/// writes, by one cached atom it stores to no object, see the same object and value every
/// iteration. No entry (`starts`) lies in such a loop past its header.
fn invariant_reads(instructions: &[WideInstruction], constants: &[crate::bytecode::Constant], starts: &[usize])
    -> BTreeMap<usize, Hoist> {
    let target = |instruction: &WideInstruction| matches!(instruction.op().control_flow_layout(),
        ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump).then(|| instruction.jump_target() as usize);
    let falls = |instruction: &WideInstruction| !matches!(instruction.op().control_flow_layout(),
        ControlFlowLayout::Jump | ControlFlowLayout::Terminal | ControlFlowLayout::BranchTable);
    let mut hoists = BTreeMap::new();
    for (back, instruction) in instructions.iter().enumerate() {
        // An entry inside the loop would run its body without the entering edges' reads; an
        // entry at its header makes them (`Lowering::hoist_reads`).
        let Some(header) = target(instruction)
            .filter(|header| *header <= back && !starts.iter().any(|start| (*header + 1..=back).contains(start))) else { continue };
        let body = &instructions[header..=back];
        let entries = usize::from(header > 0 && falls(&instructions[header - 1]))
            + instructions[..header].iter().filter(|candidate| target(candidate) == Some(header)).count();
        let side = instructions.iter().enumerate().any(|(pc, candidate)| !(header..=back).contains(&pc)
            && target(candidate).is_some_and(|to| (header + 1..=back).contains(&to) || (to == header && pc > back)));
        if entries != 1 || side
            || !body.iter().all(|candidate| reads_only(constants, *candidate) || stores_own(*candidate)) { continue }
        let written: BTreeSet<Register> = body.iter().flat_map(|candidate| writes(*candidate)).collect();
        let stored: BTreeSet<Atom> = body.iter().filter(|candidate| stores_own(**candidate))
            .map(|candidate| candidate.atom_index()).collect();
        let reads: Vec<usize> = body.iter().enumerate().filter_map(|(offset, candidate)| match candidate.field_lookup() {
            Some(FieldLookup::Atom { atom, base, .. }) if candidate.op() == Op::GetField && !stored.contains(&atom)
                && base.register_index().is_some_and(|register| !written.contains(&register)) => Some(header + offset),
            _ => None,
        }).collect();
        // A loop that only reads stores no element, so an array it never rebinds keeps its
        // elements' backing.
        let arrays: BTreeSet<Register> = body.iter().filter(|candidate| candidate.op() == Op::GetIndex)
            .filter_map(|candidate| candidate.operand_b().register_index())
            .filter(|register| !written.contains(register)).collect();
        if !reads.is_empty() || !arrays.is_empty() {
            hoists.insert(header, Hoist { reads, arrays: arrays.into_iter().collect(), back });
        }
    }
    hoists
}

/// Per block leader, the edges into it, all from before it; `None` for one an entry
/// (`starts`) or a later PC enters, whose proofs no earlier edge can give.
fn forward_edges(instructions: &[WideInstruction], leaders: &[bool], starts: &[usize]) -> Vec<Option<usize>> {
    let mut edges: Vec<Option<usize>> = leaders.iter().map(|leader| leader.then_some(0)).collect();
    for start in starts { if let Some(slot) = edges.get_mut(*start) { *slot = None; } }
    for (pc, instruction) in instructions.iter().enumerate() {
        let layout = instruction.op().control_flow_layout();
        let falls = !matches!(layout, ControlFlowLayout::Jump | ControlFlowLayout::Terminal | ControlFlowLayout::BranchTable);
        let jumps = matches!(layout, ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump)
            .then(|| instruction.jump_target() as usize);
        for target in [falls.then_some(pc + 1), jumps].into_iter().flatten() {
            let Some(slot) = edges.get_mut(target) else { continue };
            *slot = if target > pc { slot.map(|count| count + 1) } else { None };
        }
    }
    edges
}

/// Why a unit has no lane body: it does not lower, or its machine code is rejected.
#[derive(Debug)]
pub(super) enum LaneDecline {
    Unlowered,
    Rejected(super::super::jit_backend::NativeCompileRejection),
}

/// Specializes `unit` (`lane_unit::unit`), whose PCs `fields` gives the folded receiver
/// fields of.
pub(super) fn compile(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &BTreeMap<u16, Vec<NativeFieldLoad>>,
    global_reads: &BTreeSet<usize>,
    vm: VmFacts,
    generic: &[usize],
    direct: &BTreeMap<usize, *const DirectCallee>,
    frame_captures: &[super::FrameCaptures],
    loop_entry: Option<usize>,
) -> Result<LaneRegion, LaneDecline> {
    let isa = super::jit_backend::target_isa("speed").ok_or(LaneDecline::Unlowered)?;
    let (native, count) = lower(program, function_id, unit, helpers, fields, global_reads, vm, generic, direct, frame_captures, loop_entry, &*isa)
        .ok_or(LaneDecline::Unlowered)?;
    let clif = native_dump_directory().map(|_| native.to_string());
    let code = compile_native_function(native, &*isa, LANE_BODY_MAX_CODE_BYTES, |code| Some(code.to_vec()))
        .map_err(LaneDecline::Rejected)?;
    Ok(LaneRegion { code, native: count, clif })
}

/// Build the typed residual and validate every value/CFG edge before publication.
fn lower(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &BTreeMap<u16, Vec<NativeFieldLoad>>,
    global_reads: &BTreeSet<usize>,
    vm: VmFacts,
    generic: &[usize],
    direct: &BTreeMap<usize, *const DirectCallee>,
    frame_captures: &[super::FrameCaptures],
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
        global_reads,
        vm,
        generic,
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
        wrapping: PcSet::default(),
        live,
        frame_live,
        valid: true,
        invalid_at: 0,
        hoists: BTreeMap::new(),
        hoisted: BTreeMap::new(),
        hoisted_arrays: BTreeMap::new(),
        at: 0,
        forward: Vec::new(),
        arrived: BTreeMap::new(),
        entered: vec![false; unit.instructions.len()],
        finishes: false,
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
                if super::operations::NumberRelation::from_operator(instruction.binary_operator_field()).is_none()
                    || super::operations::equality_operator(instruction.binary_operator_field()).is_some()
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
    lowering.wrapping = PcSet::from_fn(instructions.len(), |pc| lowering.wraps_at(pc));
    lowering.entry = Some(lowering.entry_facts(&instructions));
    // An entry at a loop header (`loop_entry`, the entry's `start` word nonzero) starts
    // from the registers live there, each proved to its header facts or leaving at it.
    let starts: Vec<usize> = std::iter::once(0).chain(loop_entry).collect();
    lowering.hoists = invariant_reads(&instructions, &program.constants, &starts);
    lowering.forward = forward_edges(&instructions, &leaders, &starts);
    for start in &starts { if let Some(entered) = lowering.entered.get_mut(*start) { *entered = true; } }
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
        lowering.hoist_reads(&mut state, pc);
        lowering.conform(&mut state, pc);
        lowering.builder.ins().jump(target, &[]);
    }
    let mut state = State::canonical(count);
    let mut reachable = false;
    for (pc, instruction) in instructions.iter().copied().enumerate() {
        if let Some(block) = lowering.blocks[pc] {
            if reachable {
                lowering.hoist_reads(&mut state, pc);
                lowering.conform(&mut state, pc);
                lowering.arrive(&state, pc);
                lowering.entered[pc] = true;
                lowering.builder.ins().jump(block, &[]);
            }
            if !lowering.entered[pc] {
                reachable = false;
                continue;
            }
            lowering.builder.switch_to_block(block);
            state = lowering.entry_state(pc, count);
            lowering.keep_shared_proofs(&mut state, pc);
            reachable = true;
        }
        if !reachable {
            continue;
        }
        lowering.at = pc;
        reachable = lowering.instruction(&mut state, pc, instruction);
    }
    if reachable {
        lowering.exit(&mut state, instructions.len() as u32);
    }
    for (block, mut leaving, pc, line) in std::mem::take(&mut lowering.pending) {
        lowering.builder.switch_to_block(block);
        if lowering.vm.diagnostics {
            let line = lowering.builder.ins().iconst(types::I64, i64::from(line));
            lowering.builder.ins().store(MemFlagsData::trusted(), line, lowering.payload, std::mem::offset_of!(JsStepDispatch, exit_line) as i32);
        }
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
    if !lowering.valid || !lowering.finishes {
        if lowering.vm.diagnostics {
            eprintln!("QUENCH_LANE_DECLINE {{\"function\":{function_id},\"invalid_at\":{},\"finishes\":{}}}", lowering.invalid_at, lowering.finishes);
        }
        return None;
    }
    let native_count = lowering.native;
    lowering.builder.seal_all_blocks();
    lowering.builder.finalize(isa.frontend_config());
    Some((native, native_count))
}

/// 2^63: below it in magnitude a double truncates to an int64 exactly, whose low 32 bits are its
/// ToInt32 (ECMA-262 7.1.6).
const TO_INT32_EXACT_BOUND: f64 = 9_223_372_036_854_775_808.0;

/// A set of a unit's PCs, one bit each.
#[derive(Default)]
struct PcSet(Vec<u64>);

impl PcSet {
    fn from_fn(len: usize, mut member: impl FnMut(usize) -> bool) -> Self {
        let mut words = vec![0u64; len.div_ceil(u64::BITS as usize)];
        for pc in (0..len).filter(|pc| member(*pc)) {
            words[pc / u64::BITS as usize] |= 1 << (pc % u64::BITS as usize);
        }
        Self(words)
    }

    fn contains(&self, pc: usize) -> bool {
        self.0.get(pc / u64::BITS as usize).is_some_and(|word| word >> (pc % u64::BITS as usize) & 1 == 1)
    }
}

impl Lowering<'_, '_> {
    /// Whether the int32 sum or difference the instruction at `pc` computes may wrap: its
    /// result is read only through ToInt32 (`truncated`), or it is a loop induction its
    /// header bounds (`bounded_increment`), which no overflow reaches.
    fn wraps_at(&self, pc: usize) -> bool {
        let instruction = self.unit.instructions[pc];
        let destination = instruction.result_register();
        self.truncated(pc, destination) || instruction.op() == Op::IncDec
            && instruction.boolean_flag() == Some(false)
            && self.bounded_increment(pc, instruction.register_b(), destination)
    }

    /// Whether the increment at `pc` of `source` into `register` is a loop's induction that its
    /// header bounds: the loop's header leaves unless `register < bound`, with `bound` an
    /// int32 the loop never writes, and this increment is the loop's only write of `register`,
    /// run once per test (in no nested loop). Then `register + 1 <= bound`, which no int32
    /// overflow reaches.
    fn bounded_increment(&self, pc: usize, source: Register, register: Register) -> bool {
        if source != register { return false; }
        let instructions = &self.unit.instructions;
        instructions.iter().enumerate().any(|(end, jump)| {
            if jump.op().immediate_role() != crate::bytecode::ImmediateRole::JumpTarget { return false; }
            let header = jump.imm() as usize;
            if header > end || !(header..=end).contains(&pc) { return false; }
            let test = instructions[header];
            if test.op() != Op::JumpBinaryFalse || test.jump_target() as usize <= end
                || super::operations::NumberRelation::from_operator(test.binary_operator_field())
                    != Some(super::operations::NumberRelation::LessThan) {
                return false;
            }
            let (left, bound) = (test.operand_b(), test.operand_c());
            let int_bound = match bound.kind() {
                Some(OperandKind::Register) => {
                    let bound = bound.payload();
                    self.facts.at(header).is_some_and(|types| types.get(usize::from(bound)).is_some_and(|ty| ty.is_subtype(Type::INT32)))
                        && !instructions[header..=end].iter().any(|instruction| writes(*instruction).contains(&bound))
                }
                Some(OperandKind::Constant) => self.program.constants.get(usize::from(bound.payload()))
                    .and_then(super::facts::immediate).is_some_and(super::facts::is_int32),
                _ => false,
            };
            // Once per test: in no loop nested inside this one.
            let nested = instructions.iter().enumerate().any(|(inner_end, inner)| {
                inner.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget
                    && (inner.imm() as usize) <= inner_end && header < inner.imm() as usize
                    && (inner.imm() as usize..=inner_end).contains(&pc)
            });
            !nested && left.register_index() == Some(register) && int_bound
                && instructions[header..=end].iter().enumerate()
                    .filter(|(_, instruction)| writes(**instruction).contains(&register))
                    .all(|(offset, _)| header + offset == pc)
        })
    }

    /// Whether only the result's ToInt32 is read of `register`, written at `pc`: the next
    /// instruction is its one reader, a bitwise operator, which applies ToInt32 to its operands.
    /// A sum's or difference's wrapping int32 result then serves for int32 operands, and
    /// ToInt32 of its exact double for any others. Not through a further sum: ToInt32 does not
    /// commute with adding a non-integer (trunc(-3 + 0.5) differs from trunc(-3 + 2^32 + 0.5)
    /// mod 2^32), so V8 truncates chains only of int32 inputs.
    fn truncated(&self, pc: usize, register: Register) -> bool {
        let Some(next) = self.unit.instructions.get(pc + 1).copied() else { return false };
        if next.op() != Op::Binary || next.returns_from_frame() || next.writes_current_this() { return false; }
        let reads = [next.operand_b(), next.operand_c()].iter().filter(|operand| operand.register_index() == Some(register)).count();
        if reads == 0 || !(next.result_register() == register || !self.is_live(pc + 2, register)) { return false; }
        let operator = next.binary_operator();
        if super::operations::NumberBitwise::from_js_instruction(next.op(), operator)
            .is_some_and(|bitwise| bitwise != super::operations::NumberBitwise::ShiftRightUnsigned) {
            return true;
        }
        false
    }

    /// Keeps the body valid only where `holds`; the first line that does not is reported.
    #[track_caller]
    fn require(&mut self, holds: bool) {
        if !holds && self.valid {
            self.invalid_at = std::panic::Location::caller().line();
        }
        self.valid &= holds;
    }

    /// `state` past a join with `incoming`: each register's facts as both paths prove them.
    fn join_states(&mut self, state: &mut State, incoming: &State) {
        for (index, incoming) in incoming.values.iter().copied().enumerate() {
            let current = state.values[index];
            if current != incoming {
                let representation = if current.value.representation == incoming.value.representation {
                    current.value.representation
                } else {
                    self.require(current.tagged_current && incoming.tagged_current);
                    Representation::Tagged
                };
                let mut fact = RegisterFact::definition(current.value.ty.join(incoming.value.ty), representation);
                fact.tagged_current = current.tagged_current && incoming.tagged_current;
                self.require(fact.value.verify());
                state.values[index] = fact;
            }
        }
    }

    /// The ToInt32 of `number`, natively below 2^63 in magnitude; beyond, infinities and NaN
    /// leave through `exit` (`TO_INT32_EXACT_BOUND`).
    fn number_to_int32(&mut self, number: ir::Value, exit: ir::Block) -> ir::Value {
        let magnitude = self.ins().fabs(number);
        let bound = self.ins().f64const(TO_INT32_EXACT_BOUND);
        let exact = self.ins().fcmp(ir::condcodes::FloatCC::LessThan, magnitude, bound);
        emit_js_guard(&mut self.builder, exact, exit);
        let wide = self.ins().fcvt_to_sint_sat(types::I64, number);
        self.ins().ireduce(types::I32, wide)
    }

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
        // A direct store of an atom needs a specialized run and no method cache naming it,
        // which hold until guest code runs: tested here, and again after each call
        // (`Lowering::after_guest_code`), never at the stores (`emit_js_own_field_set`,
        // `store_field`).
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
        let interrupt = self.builder.declare_var(types::I64);
        let flag = self.builder.ins().load(types::I64, MemFlagsData::trusted(), self.context,
            std::mem::offset_of!(CallContext, interrupt) as i32);
        self.builder.def_var(interrupt, flag);
        Entry { receiver, receiver_shapes, store_atoms, capture_values, capture_len, this, interrupt }
    }

    /// The facts guest code may change, proved again once after it ran: each stored atom's
    /// admission (`Entry::store_atoms`).
    fn after_guest_code(&mut self) {
        let atoms: Vec<_> = self.entry.as_ref().expect("entry facts").store_atoms.iter().map(|(atom, variable)| (*atom, *variable)).collect();
        for (atom, variable) in atoms {
            let admitted = self.emit_store_atom_admitted(atom);
            self.builder.def_var(variable, admitted);
        }
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
    fn instruction(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let pc32 = pc as u32;
        let plain = instruction.result_flags() == 0;
        let ran = match instruction.op() {
            Op::Return if plain && instruction.register_a() < self.unit.registers => {
                Some(self.complete(state, pc, instruction.register_a()))
            }
            Op::Call if (plain || instruction.result_flags() == crate::bytecode::RETURN_REGISTER)
                && !instruction.direct_eval()
                && !instruction.parameter_eval() => Some(self.call(state, pc, instruction, self.helpers.call)),
            // ToNumeric of a Number is the Number: a typed operand is copied with its facts,
            // and an untyped one is taken for an int, tested once, which leaves where it is not.
            Op::ToNumeric if plain && instruction.register_b() < self.unit.registers => {
                let source = instruction.register_b();
                if !state.values[usize::from(source)].value.ty.is_subtype(Type::NUMBER) {
                    let exit = self.guard_exit(state, pc as u32);
                    self.int(state, source, exit);
                }
                self.copy(state, instruction.result_register(), source);
                Some(true)
            }
            // Anything but `null` and `undefined` is coercible; those take the lane's TypeError.
            Op::RequireObjectCoercible if plain && instruction.register_b() < self.unit.registers => {
                let exit = self.guard_exit(state, pc as u32);
                let bits = self.tagged(state, instruction.register_b());
                let null = self.ins().icmp_imm_u(IntCC::Equal, bits, Value::NULL.to_bits() as i64);
                let undefined = self.ins().icmp_imm_u(IntCC::Equal, bits, Value::UNDEFINED.to_bits() as i64);
                let nullish = self.ins().bor(null, undefined);
                let coercible = self.ins().icmp_imm_u(IntCC::Equal, nullish, 0);
                emit_js_guard(&mut self.builder, coercible, exit);
                Some(true)
            }
            Op::LoadName | Op::LoadNameCall if plain && self.global_reads.contains(&pc) => {
                let exit = self.guard_exit(state, pc as u32);
                let atom = self.ins().iconst(types::I64, i64::from(instruction.atom_index()));
                let site = self.ins().iconst(types::I64, i64::from(instruction.cache_site_index()));
                let value = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.global_get, &[atom, site]);
                let hit = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, hit, exit);
                self.set_tagged(state, instruction.result_register(), value);
                // A call's receiver is `undefined` for a binding of the global scope.
                if instruction.op() == Op::LoadNameCall {
                    let undefined = self.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
                    self.define(state, instruction.register_b(), undefined,
                        RegisterFact::definition(Type::immediate(Value::UNDEFINED), Representation::Tagged));
                }
                Some(true)
            }
            Op::Nop if plain => match self.unit.markers.iter().position(|marker| *marker == pc) {
                Some(frame) => Some(self.inline_call(state, pc, frame)),
                None => Some(true),
            },
            // The interpreter's own semantics, run on the register state (`staged::step`).
            Op::LoadConst | Op::Move | Op::IncDec | Op::Jump | Op::JumpFalse | Op::JumpTrue if plain =>
                super::lane_operation(&self.program.constants, instruction).and_then(|operation| self.run(state, pc, operation)),
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
                let inlined = match self.unit.origins.get(pc).copied() {
                    Some(super::lane_unit::Origin::Inlined { frame, .. }) => Some(self.frame_captures.get(frame).copied().unwrap_or(super::FrameCaptures::NONE)),
                    _ => None,
                };
                let (values, len) = match inlined {
                    Some(captures) => (self.ins().iconst(types::I64, captures.values as i64), self.ins().iconst(types::I64, captures.len as i64)),
                    _ => {
                        let entry = self.entry.as_ref().expect("entry facts");
                        (self.builder.use_var(entry.capture_values), self.builder.use_var(entry.capture_len))
                    }
                };
                let exit = self.guard_exit(state, pc32);
                // The body's own captures the dense view does not hold (a root, projected or
                // sparse environment, or an uninitialized binding) are read by the lane's own
                // capture read, the interpreter's rule, which leaves only where it does.
                let general = match inlined {
                    Some(captures) if captures.env.is_deleted() => exit,
                    _ => self.builder.create_block(),
                };
                let done = self.builder.create_block();
                self.builder.append_block_param(done, types::I64);
                // The dense view's read, as every tier makes it (`staged::dense_capture`).
                let mut stage = super::object_slots::Emit::new(&mut self.builder);
                let value = crate::vm::staged::dense_capture(&mut stage, values, len, slot, general)
                    .expect("an emitting stage's guards continue");
                self.ins().jump(done, &[value.into()]);
                if general != exit {
                    self.builder.switch_to_block(general);
                    self.builder.set_cold_block(general);
                    let value = match inlined {
                        // An inlined frame's environment, which its call guard fixes.
                        Some(captures) => {
                            let env = self.ins().iconst(types::I64, captures.env.to_bits() as i64);
                            let packed = self.ins().iconst(types::I64, i64::from(crate::bytecode::ImmediateLayout::capture_slot(instruction.imm())));
                            emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.env_capture_get, &[env, packed])
                        }
                        None => self.capture_read(instruction),
                    };
                    let hit = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
                    emit_js_guard(&mut self.builder, hit, exit);
                    self.ins().jump(done, &[value.into()]);
                }
                self.builder.switch_to_block(done);
                let value = self.builder.block_params(done)[0];
                self.set_tagged(state, instruction.result_register(), value);
                Some(true)
            }
            // An outer capture of the caller's activation, by the lane's own lookup.
            Op::LoadCapture if plain && !matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Inlined { .. })) => {
                let exit = self.guard_exit(state, pc32);
                let value = self.capture_read(instruction);
                let hit = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, hit, exit);
                self.set_tagged(state, instruction.result_register(), value);
                Some(true)
            }
            // A capture's store, by the lane's own store; a binding it does not take leaves.
            Op::StoreCapture if matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_))) => {
                let exit = self.guard_exit(state, pc32);
                let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
                let depth = u64::from(crate::bytecode::ImmediateLayout::capture_depth(instruction.imm()));
                let slot = u64::from(crate::bytecode::ImmediateLayout::capture_slot(instruction.imm()));
                let packed = self.ins().iconst(types::I64, (depth << u16::BITS | slot) as i64);
                let value = self.tagged(state, instruction.register_a());
                let stored = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.capture_set, &[context, packed, value]);
                let stored = self.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
                emit_js_guard(&mut self.builder, stored, exit);
                Some(true)
            }
            Op::GetField if instruction.result_flags() & !(crate::bytecode::SET_THIS_REGISTER | crate::bytecode::RETURN_REGISTER) == 0 => self.get_field(state, pc, instruction),
            Op::GetIndex if plain && self.operand_supported(instruction.operand_b()) && self.operand_supported(instruction.operand_c()) => {
                // A present dense element, the lane's own fast read (`Vm::direct_dense_element`),
                // of an int32 key, proved once.
                // Any other element, a hole's or one past the dense end included, is the lane's
                // own read (`compiled::lane_index_get`), which leaves only where it does.
                let exit = self.guard_exit(state, pc32);
                // The key's tagged bits are taken here, where they dominate both paths: a view
                // the cold path alone made would not hold on the hot one.
                let base = self.operand_tagged(state, instruction.operand_b());
                let key = self.operand_tagged(state, instruction.operand_c());
                // The key's index, as every tier takes it (`staged::index`).
                let index = super::lane_source(&self.program.constants, instruction.operand_c())
                    .and_then(|source| crate::vm::staged::index(&mut LaneMachine { lowering: self, state, pc }, source, exit));
                base.zip(key).zip(index).map(|((base, key), index)| {
                    let payload = self.payload;
                    let (general, join) = (self.builder.create_block(), self.builder.create_block());
                    self.builder.set_cold_block(general);
                    let read = self.builder.append_block_param(join, types::I64);
                    // A view its loop proved at entry, else this read's own.
                    let hoisted = instruction.operand_b().register_index().and_then(|register| self.hoisted_arrays.get(&register))
                        .filter(|(_, _, extent)| extent.contains(&pc)).map(|(header, length, _)| (*header, *length));
                    let value = match hoisted {
                        Some((header, length)) => {
                            let elements = crate::vm::staged::Elements { header: self.builder.use_var(header), length: self.builder.use_var(length) };
                            let index = self.ins().uextend(types::I64, index);
                            crate::vm::staged::element_in(&mut super::object_slots::Emit::new(&mut self.builder), elements, index, general)
                                .expect("an emitting stage's guards continue")
                        }
                        None => super::emit_dense_element_read(&mut self.builder, payload, base, index, general),
                    };
                    self.ins().jump(join, &[ir::BlockArg::Value(value)]);
                    self.builder.switch_to_block(general);
                    let value = emit_lane_helper_call(&mut self.builder, payload, self.helpers.index_get, &[base, key]);
                    let hit = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
                    emit_js_guard(&mut self.builder, hit, exit);
                    self.ins().jump(join, &[ir::BlockArg::Value(value)]);
                    self.builder.switch_to_block(join);
                    self.set_tagged(state, instruction.result_register(), read);
                    true
                })
            }
            Op::SetIndex if [instruction.register_a(), instruction.register_b(), instruction.register_c()]
                .iter().all(|register| *register < self.unit.registers) => {
                // The in-place replacement every tier makes (`staged::dense_element_store`), of an
                // index key; any other store is the lane's own (`compiled::lane_index_set`), whose
                // miss leaves at the store.
                let exit = self.guard_exit(state, pc32);
                let value = self.tagged(state, instruction.register_a());
                let base = self.tagged(state, instruction.register_b());
                let key = self.tagged(state, instruction.register_c());
                let (general, done) = (self.builder.create_block(), self.builder.create_block());
                self.builder.set_cold_block(general);
                let index = crate::vm::staged::index(&mut LaneMachine { lowering: self, state, pc }, crate::vm::staged::Source::Register(instruction.register_c()), general)
                    .expect("an emitting stage's guards continue");
                let index = self.ins().uextend(types::I64, index);
                let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, array_record_base));
                let mut stage = super::object_slots::Emit::new(&mut self.builder).with(crate::vm::staged::Table::ArrayRecords, records);
                crate::vm::staged::array_record(&mut stage, base, general)
                    .and_then(|record| crate::vm::staged::dense_element_store(&mut stage, record, index, value, general))
                    .expect("an emitting stage's guards continue");
                self.ins().jump(done, &[]);
                self.builder.switch_to_block(general);
                let stored = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.index_set, &[base, key, value]);
                let hit = self.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
                emit_js_guard(&mut self.builder, hit, exit);
                self.ins().jump(done, &[]);
                self.builder.switch_to_block(done);
                state.epoch = None;
                Some(true)
            }
            Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict if plain => self.set_field(state, pc, instruction),
            op if op.js_denotation() == JsDenotation::NumericBinary && instruction.numeric_local_target().is_none() => {
                self.binary(state, pc, instruction)
            }
            Op::Unary if crate::bytecode::typeof_test(instruction.unary_operator()).is_some() => self.typeof_test(state, pc, instruction),
            // A literal's allocation, by the lane's own (`compiled::lane_new_object`); a due
            // collection leaves first.
            Op::MakeObject | Op::MakeArray | Op::MakeObject2 if plain => {
                let exit = self.guard_exit(state, pc32);
                let payload = self.payload;
                let value = match instruction.op() {
                    Op::MakeObject => emit_lane_helper_call(&mut self.builder, payload, self.helpers.new_object, &[payload]),
                    Op::MakeArray => {
                        let length = self.ins().iconst(types::I64, instruction.array_length() as i64);
                        emit_lane_helper_call(&mut self.builder, payload, self.helpers.new_array, &[payload, length])
                    }
                    _ => {
                        let site = self.ins().iconst(types::I64, i64::from(instruction.imm()));
                        let (first, second) = (self.tagged(state, instruction.register_b()), self.tagged(state, instruction.register_c()));
                        emit_lane_helper_call(&mut self.builder, payload, self.helpers.new_object_pair, &[payload, site, first, second])
                    }
                };
                let made = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, made, exit);
                self.reload_property_arena();
                state.layouts.clear();
                self.set_tagged(state, instruction.result_register(), value);
                Some(true)
            }
            Op::Construct if !instruction.is_super_construct()
                && matches!(instruction.construct_arguments(), crate::bytecode::ConstructArguments::Registers(_)) => self.construct(state, pc, instruction),
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
                        emit_nullish_branch(&mut self.builder, value, comparison, self.vm.html_dda, exit)
                    };
                    self.follow(state, pc, instruction.jump_target() as usize, Some(taken));
                    return false;
                }
                // A Number relation is the interpreter's own (`staged::step`); an equality of
                // operands not both Numbers, or `instanceof`, the lowering's (`relation`).
                let numbers = [left, right].iter().all(|operand| super::facts::operand_type(
                    &self.program.constants, *operand, |register| state.values[usize::from(register)].value.ty)
                    .is_subtype(Type::NUMBER));
                match super::lane_operation(&self.program.constants, instruction) {
                    Some(operation) if numbers || super::operations::equality_operator(operator).is_none() => self.run(state, pc, operation),
                    _ => self.relation(state, pc32, operator, left, right).map(|holds| {
                        let taken = self.ins().icmp_imm_u(IntCC::Equal, holds, 0);
                        self.follow(state, pc, instruction.jump_target() as usize, Some(taken));
                        false
                    }),
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

    /// `operation` by the interpreter's semantics on the register state (`staged::step`),
    /// leaving at `pc` where it misses; whether control falls through to the next.
    fn run(&mut self, state: &mut State, pc: usize, operation: crate::vm::staged::Operation) -> Option<bool> {
        use crate::vm::staged::Flow;
        let exit = self.guard_exit(state, pc as u32);
        let (target, taken) = match crate::vm::staged::step(&mut LaneMachine { lowering: self, state, pc }, operation, exit)? {
            Flow::Next => return Some(true),
            Flow::Jump(target) => (target, None),
            Flow::Branch { taken, target } => (target, Some(taken)),
        };
        self.follow(state, pc, target as usize, taken);
        Some(false)
    }

    /// Control from `pc` to `target`, where `taken` holds if it is a branch: a backward edge
    /// first checks for an interrupt.
    fn follow(&mut self, state: &mut State, pc: usize, target: usize, taken: Option<ir::Value>) {
        if target <= pc {
            self.interrupt_check(state, pc as u32);
        }
        match taken {
            Some(taken) => self.branch(state, pc, target, taken),
            None => self.continue_at(state, target),
        }
    }

    /// Whether the numeric relation `operator` holds of `left` and `right`, as an `i8`: in
    /// int32 where both are proved int32, else in Float64, each operand proved a Number or
    /// leaving at `pc`; `None` for any other operator or operand.
    /// A relation's predicate: `instanceof` by the prototype chain (`instance_of`),
    /// (in)equality of operands not both Numbers (`equality`), every other relation of Numbers.
    fn relation(&mut self, state: &mut State, pc: u32, operator: u32,
        left: Operand, right: Operand) -> Option<ir::Value> {
        let numbers = [left, right].iter().all(|operand| super::facts::operand_type(
            &self.program.constants, *operand, |register| state.values[usize::from(register)].value.ty)
            .is_subtype(Type::NUMBER));
        if operator == oxc_ast::ast::BinaryOperator::Instanceof as u32 {
            return self.instance_of(state, pc, left, right);
        }
        match super::operations::equality_operator(operator) {
            Some((strict, negated)) if !numbers && self.operand_supported(left) && self.operand_supported(right) => {
                let equal = self.equality(state, pc, left, right, strict)?;
                Some(if negated { self.ins().bxor_imm_u(equal, 1) } else { equal })
            }
            _ => self.number_relation(state, pc, operator, left, right),
        }
    }

    /// `new callee(...)` by the callee's construct plan (`compiled::lane_construct`): the
    /// arguments pass in the body's registers, written back for the helper to read; a
    /// construction the plan does not make leaves at the record, before any effect. The
    /// allocation may move the property arena, whose base and length the body reloads, and
    /// the slot addresses its proofs hold retire.
    /// The property arena's base and length again, which an allocation may have moved.
    fn reload_property_arena(&mut self) {
        let receiver = &self.entry.as_ref().expect("entry facts").receiver;
        let (properties, properties_len) = (receiver.properties, receiver.properties_len);
        let values = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, property_values));
        let len = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, property_len));
        self.builder.def_var(properties, values);
        self.builder.def_var(properties_len, len);
    }

    fn construct(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let crate::bytecode::ConstructArguments::Registers(window) = instruction.construct_arguments() else { return None };
        // A construction a previous body left at runs as a call does
        // (`dispatch_fast::lane_body_construct`): the callee's plan, else its activation.
        if self.generic.contains(&pc)
            && matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_)))
            && window.base + window.count <= self.unit.caller_registers
            && matches!(instruction.result_flags(), 0 | crate::bytecode::RETURN_REGISTER) {
            return Some(self.call(state, pc, instruction, self.helpers.construct_call));
        }
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) if instruction.result_flags() == 0 => None,
            (true, Some(super::lane_unit::Origin::Caller(record))) if instruction.result_flags() == crate::bytecode::RETURN_REGISTER => Some(record),
            _ => return None,
        };
        // The body's register memory holds the caller's registers alone (`Lowering::call`).
        if window.base + window.count > self.unit.caller_registers {
            return None;
        }
        let exit = self.guard_exit(state, pc as u32);
        let base = self.registers_base;
        for register in window.base..window.base + window.count {
            let bits = self.tagged(state, register);
            self.ins().store(MemFlagsData::trusted(), bits, base, register_offset(register));
        }
        let callee = self.tagged(state, instruction.register_b());
        let arguments = self.ins().iadd_imm_u(base, i64::from(register_offset(window.base)));
        let argc = self.ins().iconst(types::I64, i64::from(window.count));
        let payload = self.payload;
        let this = emit_lane_helper_call(&mut self.builder, payload, self.helpers.construct, &[payload, callee, arguments, argc]);
        let built = self.ins().icmp_imm_u(IntCC::NotEqual, this, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, built, exit);
        self.reload_property_arena();
        state.layouts.clear();
        state.epoch = None;
        self.after_guest_code();
        if let Some(record) = completes {
            self.return_value(record, this);
            return Some(false);
        }
        self.set_tagged(state, instruction.result_register(), this);
        Some(true)
    }

    /// A fused `typeof value === name` (`bytecode::typeof_test`): an immediate value's tag
    /// decides it, as `Vm::typeof_test_value` would, and a heap value's cell the VM reads
    /// (`lane_typeof_is`). A returning test completes the activation with its Boolean.
    fn typeof_test(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        use crate::value::ValueLayout;
        let (index, negated) = crate::bytecode::typeof_test(instruction.unary_operator())?;
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) if instruction.result_flags() == 0 => None,
            (true, Some(super::lane_unit::Origin::Caller(record))) => Some(record),
            _ => return None,
        };
        let value = self.tagged(state, instruction.register_b());
        // The immediate classes whose name is the tested one, by the VM's own rule.
        let named = |sample: Value| super::super::coercion::immediate_typeof_index(sample) == Some(index);
        let mut immediate = self.ins().iconst(types::I8, 0);
        if named(Value::number(0.0)) {
            let field = self.ins().ushr_imm_u(value, i64::from(ValueLayout::TAG_SHIFT));
            let offset = self.ins().iadd_imm_s(field, -(ValueLayout::FIRST_TAG_FIELD as i64));
            let double = self.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, offset, ValueLayout::TAG_FIELD_SPAN as i64);
            let int = is_int_tagged(&mut self.builder, value);
            let number = self.ins().bor(double, int);
            immediate = self.ins().bor(immediate, number);
        }
        for sample in [Value::UNDEFINED, Value::DELETED, Value::NULL] {
            if named(sample) {
                let same = self.ins().icmp_imm_u(IntCC::Equal, value, sample.to_bits() as i64);
                immediate = self.ins().bor(immediate, same);
            }
        }
        if named(Value::TRUE) {
            let tag = self.ins().band_imm_u(value, ValueLayout::TAG_MASK as i64);
            let boolean = self.ins().icmp_imm_u(IntCC::Equal, tag, (Value::TRUE.to_bits() & ValueLayout::TAG_MASK) as i64);
            immediate = self.ins().bor(immediate, boolean);
        }
        let tag = self.ins().band_imm_u(value, ValueLayout::TAG_MASK as i64);
        let heap = self.ins().icmp_imm_u(IntCC::Equal, tag, ValueLayout::HEAP_TAG as i64);
        let (cell, join) = (self.builder.create_block(), self.builder.create_block());
        let holds = self.builder.append_block_param(join, types::I8);
        self.ins().brif(heap, cell, &[], join, &[ir::BlockArg::Value(immediate)]);
        self.builder.switch_to_block(cell);
        let tested = self.ins().iconst(types::I64, index as i64);
        let is = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.typeof_is, &[value, tested]);
        let is = self.ins().ireduce(types::I8, is);
        self.ins().jump(join, &[ir::BlockArg::Value(is)]);
        self.builder.switch_to_block(join);
        let holds = if negated { self.ins().bxor_imm_u(holds, 1) } else { holds };
        let boolean = self.boolean(holds);
        if let Some(record) = completes {
            self.return_value(record, boolean);
            return Some(false);
        }
        self.set_boolean(state, instruction.result_register(), boolean);
        Some(true)
    }

    /// `value instanceof constructor` by the VM's steady case (`Vm::steady_instanceof`), which
    /// calls nothing; a case only the full algorithm decides leaves at the test.
    fn instance_of(&mut self, state: &mut State, pc: u32, value: Operand, constructor: Operand) -> Option<ir::Value> {
        let exit = self.guard_exit(state, pc);
        let (value, constructor) = (self.operand_tagged(state, value)?, self.operand_tagged(state, constructor)?);
        let holds = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.instance_of, &[value, constructor]);
        let decided = self.ins().icmp_imm_u(IntCC::NotEqual, holds, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, decided, exit);
        Some(self.ins().ireduce(types::I8, holds))
    }

    /// `Vm::strict_equal` (`strict`) or `==` of two tagged operands. Identical bits are equal
    /// unless NaN, and two Numbers compare as Numbers. Strictly, a pair of strings or of
    /// legacy cells (BigInts) of one family compares by contents (`lane_strict_equal_cells`)
    /// and every other pair is unequal. Loosely, without conversions only: `null` and
    /// `undefined` equal each other and no object record, and two distinct object records are
    /// unequal; any other pair (a string, a Boolean, a function, which may be the IsHTMLDDA
    /// object) leaves at `pc`.
    fn equality(&mut self, state: &mut State, pc: u32, left: Operand, right: Operand, strict: bool) -> Option<ir::Value> {
        use crate::value::ValueLayout;
        let exit = self.guard_exit(state, pc);
        let (left, right) = (self.operand_tagged(state, left)?, self.operand_tagged(state, right)?);
        let [identical, distinct, numbers, others, cells, join] = std::array::from_fn(|_| self.builder.create_block());
        let equal = self.builder.append_block_param(join, types::I8);
        let same = self.ins().icmp(IntCC::Equal, left, right);
        self.ins().brif(same, identical, &[], distinct, &[]);
        self.builder.switch_to_block(identical);
        let ordered = self.ins().icmp_imm_u(IntCC::NotEqual, left, ValueLayout::CANONICAL_NAN as i64);
        self.ins().jump(join, &[ir::BlockArg::Value(ordered)]);
        self.builder.switch_to_block(distinct);
        // Distinct int-tagged bits are distinct int32 values: an int32 has one tagged form,
        // so the pair is unequal without a Number decode, which only other Numbers take.
        let (left_int, right_int) = (is_int_tagged(&mut self.builder, left), is_int_tagged(&mut self.builder, right));
        let ints = self.ins().band(left_int, right_int);
        let (decoded, unequal_ints) = (self.builder.create_block(), self.ins().iconst(types::I8, 0));
        self.ins().brif(ints, join, &[ir::BlockArg::Value(unequal_ints)], decoded, &[]);
        self.builder.switch_to_block(decoded);
        self.builder.set_cold_block(numbers);
        let is_number = |builder: &mut FunctionBuilder<'_>, bits: ir::Value| {
            let field = builder.ins().ushr_imm_u(bits, i64::from(ValueLayout::TAG_SHIFT));
            let offset = builder.ins().iadd_imm_s(field, -(ValueLayout::FIRST_TAG_FIELD as i64));
            let double = builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, offset, ValueLayout::TAG_FIELD_SPAN as i64);
            let int = is_int_tagged(builder, bits);
            builder.ins().bor(double, int)
        };
        let (left_number, right_number) = (is_number(&mut self.builder, left), is_number(&mut self.builder, right));
        let both = self.ins().band(left_number, right_number);
        self.ins().brif(both, numbers, &[], others, &[]);
        self.builder.switch_to_block(numbers);
        let (left_value, right_value) = (emit_js_tagged_number(&mut self.builder, left, exit), emit_js_tagged_number(&mut self.builder, right, exit));
        let number_equal = self.ins().fcmp(FloatCC::Equal, left_value, right_value);
        self.ins().jump(join, &[ir::BlockArg::Value(number_equal)]);
        self.builder.switch_to_block(others);
        if !strict {
            let nullish = |builder: &mut FunctionBuilder<'_>, bits: ir::Value| {
                let null = builder.ins().icmp_imm_u(IntCC::Equal, bits, Value::NULL.to_bits() as i64);
                let undefined = builder.ins().icmp_imm_u(IntCC::Equal, bits, Value::UNDEFINED.to_bits() as i64);
                builder.ins().bor(null, undefined)
            };
            let record = |builder: &mut FunctionBuilder<'_>, bits: ir::Value| {
                let family = builder.ins().band_imm_u(bits, ValueLayout::HEAP_FAMILY_MASK as i64);
                let object = builder.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_OBJECT_TAG as i64);
                let array = builder.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_ARRAY_TAG as i64);
                builder.ins().bor(object, array)
            };
            let (left_nullish, right_nullish) = (nullish(&mut self.builder, left), nullish(&mut self.builder, right));
            let (left_record, right_record) = (record(&mut self.builder, left), record(&mut self.builder, right));
            // Each side nullish or an object record: decided without a conversion.
            let left_known = self.ins().bor(left_nullish, left_record);
            let right_known = self.ins().bor(right_nullish, right_record);
            let known = self.ins().band(left_known, right_known);
            emit_js_guard(&mut self.builder, known, exit);
            // Distinct bits: equal only as `null` and `undefined`.
            let nullish_pair = self.ins().band(left_nullish, right_nullish);
            self.ins().jump(join, &[ir::BlockArg::Value(nullish_pair)]);
            self.builder.switch_to_block(join);
            return Some(equal);
        }
        let family = self.ins().band_imm_u(left, ValueLayout::HEAP_FAMILY_MASK as i64);
        let right_family = self.ins().band_imm_u(right, ValueLayout::HEAP_FAMILY_MASK as i64);
        let one_family = self.ins().icmp(IntCC::Equal, family, right_family);
        let string = self.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_STRING_TAG as i64);
        let legacy = self.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::HEAP_TAG as i64);
        let contents = self.ins().bor(string, legacy);
        let compared = self.ins().band(one_family, contents);
        let unequal = self.ins().iconst(types::I8, 0);
        self.ins().brif(compared, cells, &[], join, &[ir::BlockArg::Value(unequal)]);
        self.builder.switch_to_block(cells);
        let result = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.strict_equal, &[left, right]);
        let cells_equal = self.ins().ireduce(types::I8, result);
        self.ins().jump(join, &[ir::BlockArg::Value(cells_equal)]);
        self.builder.switch_to_block(join);
        Some(equal)
    }

    fn number_relation(&mut self, state: &mut State, pc: u32, operator: u32,
        left: Operand, right: Operand) -> Option<ir::Value> {
        if !self.operand_supported(left) || !self.operand_supported(right) {
            return None;
        }
        let relation = super::operations::NumberRelation::from_operator(operator)?;
        let (left, right) = (super::lane_source(&self.program.constants, left)?, super::lane_source(&self.program.constants, right)?);
        let exit = self.guard_exit(state, pc);
        // The interpreter's own Number relation, run on the register state (`staged`).
        crate::vm::staged::relation(&mut LaneMachine { lowering: self, state, pc: pc as usize }, relation, left, right, exit)
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
        if state.readable.contains(&Some(source)) { state.readable.insert(Some(destination)); }
        for (shape, projection) in layouts { state.layouts.insert((Some(destination), shape), projection); }
        if receiver { state.receivers.insert(destination); }
    }

    /// The call marker of inlined `frame` (`lane_unit::unit`): its callee is the function
    /// the call's cache linked and, where the callee observes `this`, its receiver a direct
    /// object, which a plain call binds as `this`; anything else leaves at the call, before
    /// its effects. The callee's parameters
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
        // A call through `call` is the call of the function it names while `call` is the realm's.
        if let Some(register) = inlined.function_call {
            let (_, call) = self.vm.function_call.expect("a call through `call` plans only where the realm's is known");
            let native = self.tagged(state, register);
            let realm = self.ins().icmp_imm_u(IntCC::Equal, native, call.to_bits() as i64);
            emit_js_guard(&mut self.builder, realm, exit);
        }
        // A callee that observes no `this` takes any receiver: its binding is unobserved.
        if inlined.observes_this {
            let receiver = self.tagged(state, inlined.receiver);
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

    /// A call the unit does not inline, made natively (`dispatch_fast::lane_body_call`) from
    /// the caller's registers, which it writes back first: a completed call's result is in
    /// its register; a missed one leaves at the call; one that left the lane in its callee's
    /// activation leaves the body there too (`LaneExit::Callee`).
    /// A call (or a construction, by `helper`) the body does not inline, by `helper`'s
    /// protocol: the body's registers written back, then a direct call of a `Call`'s compiled
    /// callee or the helper; its completion continues, a leave in the callee stops the body,
    /// and a miss leaves at `pc`. The callee may allocate, which may move the property arena,
    /// whose base and length the body reloads.
    fn call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, helper: usize) -> bool {
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
        let generic = self.builder.create_block();
        match self.direct.get(&pc).copied().filter(|_| instruction.op() == Op::Call) {
            // SAFETY: the body owns the callee for as long as its code runs.
            Some(callee) => self.direct_call(instruction, unsafe { &*callee }, generic, completed),
            None => { self.ins().jump(generic, &[]); }
        }
        self.builder.switch_to_block(generic);
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
        let call_word = self.ins().iconst(types::I64, i64::from(call_pc));
        let status = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[context, call_word]);
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
        state.epoch = None;
        self.reload_property_arena();
        self.after_guest_code();
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
        // A sloppy callee binds an object receiver itself and an `undefined` or `null` one
        // to the global object (`lane_receiver_this`); any other receiver takes the helper.
        let (admitted, this) = match callee.sloppy {
            Some(global) => {
                let family = self.ins().band_imm_u(this, ValueLayout::HEAP_FAMILY_MASK as i64);
                let object = self.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_OBJECT_TAG as i64);
                let undefined = self.ins().icmp_imm_u(IntCC::Equal, this, Value::UNDEFINED.to_bits() as i64);
                let null = self.ins().icmp_imm_u(IntCC::Equal, this, Value::NULL.to_bits() as i64);
                let nullish = self.ins().bor(undefined, null);
                let bound = self.ins().bor(object, nullish);
                let global = self.ins().iconst(types::I64, global.to_bits() as i64);
                (self.ins().band(same, bound), self.ins().select(object, this, global))
            }
            None => (same, this),
        };
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
        // A call of the body itself reads its entry, bound once the code exists.
        let entry = if callee.entry == 0 {
            self.ins().load(types::I64, MemFlagsData::trusted(), descriptor, offset(std::mem::offset_of!(DirectCallee, entry)))
        } else {
            self.ins().iconst(types::I64, callee.entry as i64)
        };
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
        self.finishes = true;
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
        let flag = self.builder.use_var(self.entry.as_ref().expect("entry facts").interrupt);
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

    /// The reads `header`'s loop repeats unchanged, made on an edge entering it from `state`:
    /// its one edge from before it, and the body's entry where the body starts at `header`.
    /// Each read's value is one variable every entering edge defines; a guard that fails
    /// leaves at the header, before the loop runs.
    fn hoist_reads(&mut self, state: &mut State, header: usize) {
        let Some(Hoist { reads, arrays, back }) = self.hoists.get(&header).cloned() else { return };
        for register in arrays {
            let bits = self.tagged(state, register);
            let (header_bits, length) = self.array_view(bits);
            let (header_var, length_var) = match self.hoisted_arrays.get(&register) {
                Some((header_var, length_var, _)) => (*header_var, *length_var),
                None => (self.builder.declare_var(types::I64), self.builder.declare_var(types::I64)),
            };
            self.builder.def_var(header_var, header_bits);
            self.builder.def_var(length_var, length);
            self.hoisted_arrays.insert(register, (header_var, length_var, header..=back));
        }
        for pc in reads {
            let instruction = self.unit.instructions[pc];
            let Some(FieldLookup::Atom { atom, base, cache_site }) = instruction.field_lookup() else { continue };
            let register = base.register_index();
            let exit = self.guard_exit(state, header as u32);
            let receiver = !self.native_calls
                && (base == crate::bytecode::FieldBase::THIS || register.is_some_and(|register| state.receivers.contains(&register)));
            let (value, int32) = match register.filter(|_| atom == self.vm.length) {
                Some(register) => {
                    let length = self.array_length(state, register, (atom, cache_site), exit);
                    (int_tagged(&mut self.builder, length), true)
                }
                None => {
                    let value = self.base_field_read(state, register, receiver, (atom, cache_site), exit);
                    let int32 = int32_read(self.program, self.fields, instruction);
                    if int32 {
                        let integer = is_int_tagged(&mut self.builder, value);
                        emit_js_guard(&mut self.builder, integer, exit);
                    }
                    (value, int32)
                }
            };
            let variable = match self.hoisted.get(&pc) {
                Some((variable, _)) => *variable,
                None => self.builder.declare_var(types::I64),
            };
            self.builder.def_var(variable, value);
            self.hoisted.insert(pc, (variable, int32));
        }
    }

    /// The elements view of the value `bits` (`staged::elements`): a direct array's, else a
    /// view of none, which no index is within.
    fn array_view(&mut self, bits: ir::Value) -> (ir::Value, ir::Value) {
        use crate::vm::staged::Stage;
        let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, array_record_base));
        let family = self.ins().band_imm_u(bits, crate::value::ValueLayout::HEAP_FAMILY_MASK as i64);
        let array = self.ins().icmp_imm_u(IntCC::Equal, family, crate::value::ValueLayout::DIRECT_ARRAY_TAG as i64);
        let reserved = self.ins().icmp_imm_u(IntCC::NotEqual, records, 0);
        let viewed = self.ins().band(array, reserved);
        let mut stage = super::object_slots::Emit::new(&mut self.builder).with(crate::vm::staged::Table::ArrayRecords, records);
        let view = |stage: &mut super::object_slots::Emit<'_, '_>| {
            let id = stage.and_imm(bits, u64::from(u32::MAX));
            let offset = stage.mul_imm(id, crate::heap::NATIVE_ARRAY_LAYOUT.record_size as u64);
            let record = stage.add(records, offset);
            crate::vm::staged::elements(stage, record)
        };
        let header = stage.choose(viewed, |stage| Some(view(stage).header), |stage| Some(stage.constant(0))).expect("both arms make a word");
        let length = stage.choose(viewed, |stage| Some(stage.load_u32(header, crate::heap::NATIVE_ARRAY_LAYOUT.header_length_offset as i32)),
            |stage| Some(stage.constant(0))).expect("both arms make a word");
        (header, length)
    }

    fn get_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        if let Some((variable, int32)) = self.hoisted.get(&pc).copied() {
            let value = self.builder.use_var(variable);
            let destination = instruction.result_register();
            self.set_tagged(state, destination, value);
            if int32 {
                let int = self.ins().ireduce(types::I32, value);
                let variable = self.int_var(usize::from(destination));
                self.builder.def_var(variable, int);
                state.values[usize::from(destination)] = RegisterFact { tagged_current: true,
                    ..RegisterFact::definition(Type::INT32, Representation::Int32) };
            }
            return Some(true);
        }
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
        let (base, parts, sink): (_, Vec<(Atom, u16)>, _) = match instruction.field_lookup()? {
            FieldLookup::Atom { atom, base, cache_site } => (base, vec![(atom, cache_site)], None),
            FieldLookup::Site(index) => {
                let site = *self.program.field_sites.get(index)?;
                let reads = [Some(site.first), site.second].into_iter().flatten().collect();
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
        if let (Some(register), [], None, None) = (register, rest, sink, completes)
            && first.0 == self.vm.length
        {
            let length = self.array_length(state, register, *first, exit);
            self.set_int(state, destination, length);
            return Some(true);
        }
        let mut value = self.base_field_read(state, register, receiver, *first, exit);
        for site in rest {
            value = self.object_field_read(state, value, *site, exit)?;
        }
        if let Some((atom, site)) = sink {
            self.store_field(state, sink_owner, atom, site, value, Type::ANY, exit)?;
        }
        if let Some(record) = completes {
            self.return_value(record, value);
            return Some(false);
        }
        // A slot that has only held int32 values is read as one: its value's tag is tested
        // here, once, and a value it no longer holds leaves before the read is defined.
        if !int32_read(self.program, self.fields, instruction) {
            self.set_tagged(state, destination, value);
            return Some(true);
        }
        let integer = is_int_tagged(&mut self.builder, value);
        emit_js_guard(&mut self.builder, integer, exit);
        self.set_tagged(state, destination, value);
        let int = self.ins().ireduce(types::I32, value);
        let variable = self.int_var(usize::from(destination));
        self.builder.def_var(variable, int);
        state.values[usize::from(destination)] = RegisterFact { tagged_current: true,
            ..RegisterFact::definition(Type::INT32, Representation::Int32) };
        Some(true)
    }

    /// The `length` read through `read` of the value `register` holds, as an int32: a direct
    /// array's from its record (`emit_array_length_read`), any other's the VM's read, which
    /// leaves through `exit` where it misses or is not an int32.
    fn array_length(&mut self, state: &mut State, register: Register, read: (Atom, u16), exit: ir::Block) -> ir::Value {
        let base = self.tagged(state, register);
        let (generic, join) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(generic);
        let length = self.builder.append_block_param(join, types::I32);
        let native = super::emit_array_length_read(&mut self.builder, self.payload, base, generic);
        self.ins().jump(join, &[ir::BlockArg::Value(native)]);
        self.builder.switch_to_block(generic);
        let value = self.field_probe(base, read, exit);
        let integer = is_int_tagged(&mut self.builder, value);
        emit_js_guard(&mut self.builder, integer, exit);
        let int = self.ins().ireduce(types::I32, value);
        self.ins().jump(join, &[ir::BlockArg::Value(int)]);
        self.builder.switch_to_block(join);
        length
    }

    /// The read through `site` of the base `register` names (`this` for `None`): folded to
    /// the site's proved shapes and slots, with the base's record proved once (the
    /// receiver's at entry), else the VM's cached read.
    fn base_field_read(&mut self, state: &mut State, register: Option<Register>, receiver: bool, read: (Atom, u16), exit: ir::Block) -> ir::Value {
        let site = read.1;
        let object_bits = match register {
            None => self.builder.use_var(self.entry.as_ref().expect("entry facts").this),
            Some(register) => self.tagged(state, register),
        };
        let shapes = self.fields.get(&site).cloned().unwrap_or_default();
        if shapes.is_empty() {
            return self.field_probe(object_bits, read, exit);
        }
        let owner = if receiver { None } else { register };
        let probed = NativeFieldLoad::probes_misses(&shapes);
        self.probed_read(state, owner, probed, object_bits, read, exit, |lowering, state, generic| {
            let (object, shape, _) = if receiver {
                let entry = lowering.entry.as_ref().expect("entry facts");
                (
                    lowering.builder.use_var(entry.receiver.object),
                    lowering.builder.use_var(entry.receiver.shape),
                    lowering.builder.use_var(entry.receiver.inline),
                )
            } else {
                lowering.object_record(state, register, true, generic)
            };
            lowering.shape_switch_read(state, Some(owner), object, shape, &shapes, generic)
        })
    }

    /// The read through `site` of the object a previous read of a chain produced, whose
    /// record nothing else names, so it is proved for this read alone.
    fn object_field_read(&mut self, state: &mut State, object_bits: ir::Value, read: (Atom, u16), exit: ir::Block) -> Option<ir::Value> {
        let site = read.1;
        let shapes = self.fields.get(&site).cloned().unwrap_or_default();
        if shapes.is_empty() {
            return Some(self.field_probe(object_bits, read, exit));
        }
        let probed = NativeFieldLoad::probes_misses(&shapes);
        Some(self.probed_read(state, None, probed, object_bits, read, exit, |lowering, state, generic| {
            let records = emit_js_payload_word(&mut lowering.builder, lowering.payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
            let functions = emit_js_payload_word(&mut lowering.builder, lowering.payload, std::mem::offset_of!(JsStepDispatch, function_record_base));
            let header = ObjectHeader::prove_readable(&mut lowering.builder, object_bits, records, functions, generic);
            lowering.shape_switch_read(state, None, header.record, header.shape, &shapes, generic)
        }))
    }

    /// A read by the site's proved shapes (`fast`), whose misses leave through `exit`; where
    /// `probed` (the site names prototype-held slots, which prove only the immediate prototype,
    /// `Vm::ordinary_holder_entry_value`), a miss first asks the VM's caches, whose other tiers
    /// read deeper holders (`field_probe`). Past that join only what both paths prove holds:
    /// the fast path's proofs of `owner`'s object do not.
    #[allow(clippy::too_many_arguments)]
    fn probed_read(&mut self, state: &mut State, owner: Option<Register>, probed: bool, object_bits: ir::Value, read: (Atom, u16),
        exit: ir::Block, fast: impl FnOnce(&mut Self, &mut State, ir::Block) -> Option<ir::Value>) -> ir::Value {
        if !probed {
            return match fast(self, state, exit) {
                Some(read) => read,
                None => self.field_probe(object_bits, read, exit),
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
        let probed = self.field_probe(object_bits, read, exit);
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
        shapes: &[NativeFieldLoad],
        exit: ir::Block,
    ) -> Option<ir::Value> {
        let entry = self.entry.as_ref().expect("entry facts");
        let properties = self.builder.use_var(entry.receiver.properties);
        // One shape: its test is a guard, and the shape it proves is the owner's from then on
        // (`prove_shape`), so a later read of the owner tests nothing.
        if let [field] = shapes && let Some(holder) = field.holder {
            self.prove_shape(state, owner, shape, field.shape, exit);
            return self.holder_read(state, object, holder, *field, exit);
        }
        if let [field] = shapes {
            let known = owner.and_then(|owner| state.layouts.get(&(owner, field.shape)).copied());
            let projection = match known {
                Some(projection) if usize::from(field.slot) < projection.storage_len => projection,
                _ => {
                    self.prove_shape(state, owner, shape, field.shape, exit);
                    let projection = CheckedObject::prove(&mut self.builder, object, properties, *field, PackedAccess::Reload)?;
                    if let Some(owner) = owner {
                        state.layouts.insert((owner, field.shape), projection);
                    }
                    projection
                }
            };
            return Some(projection.read(&mut self.builder, field.slot));
        }
        // Several shapes: the shared dispatch over the site's cases (`staged::field_read`).
        // A holder case that folds to its prototype's constant is the lowering's own.
        if shapes.iter().any(|field| field.inherited.is_some() || !field.valid()) {
            return None;
        }
        let cases: Vec<_> = shapes.iter().map(|field| field.case()).collect();
        let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
        let mut stage = super::object_slots::Emit::new(&mut self.builder).with(crate::vm::staged::Table::PropertyValues, properties).with(crate::vm::staged::Table::ObjectRecords, records);
        crate::vm::staged::field_read(&mut stage, object, shape, &cases, exit)
    }

    /// Guards that `shape`, the shape word of `owner`'s object, is `expected`; the owner's
    /// proof then holds the shape as that constant, which folds every later test of it.
    fn prove_shape(&mut self, state: &mut State, owner: Option<Option<Register>>, shape: ir::Value, expected: u32, exit: ir::Block) {
        let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(expected));
        emit_js_guard(&mut self.builder, same, exit);
        let constant = self.ins().iconst(types::I32, i64::from(expected));
        if let Some(proof) = owner.and_then(|owner| state.shapes.get_mut(&owner)) {
            proof.1 = constant;
        }
    }

    /// The slot `field` names in the prototype of the object record `record`: an object
    /// record of the exact shape `holder`, else `exit` (`Vm::record_field_entry_hit`).
    fn holder_read(&mut self, state: &mut State, record: ir::Value, holder: u32, field: NativeFieldLoad, exit: ir::Block) -> Option<ir::Value> {
        // A prototype that is the one compiled against, unchanged since, holds the value it
        // held then: the read is that constant (`NativeFieldLoad::inherited`).
        if let Some(constant) = field.inherited {
            use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
            let meta = self.ins().load(types::I32, MemFlagsData::trusted(), record, OBJECT.meta_offset as i32);
            let proto = self.ins().band_imm_u(meta, i64::from(OBJECT.proto_mask));
            if state.epoch != Some(constant.epoch) {
                let address = self.ins().iconst(types::I64, crate::heap::prototype_value_epoch_address() as i64);
                let epoch = self.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
                let unchanged = self.ins().icmp_imm_u(IntCC::Equal, epoch, constant.epoch as i64);
                emit_js_guard(&mut self.builder, unchanged, exit);
                state.epoch = Some(constant.epoch);
            }
            // The receiver's link selects its prototype's value; one prototype's is a constant.
            let ((first, value), rest) = constant.values().split_first()?;
            let mut matched = self.ins().icmp_imm_u(IntCC::Equal, proto, i64::from(*first));
            let mut read = self.ins().iconst(types::I64, value.to_bits() as i64);
            for (prototype, value) in rest {
                let same = self.ins().icmp_imm_u(IntCC::Equal, proto, i64::from(*prototype));
                let candidate = self.ins().iconst(types::I64, value.to_bits() as i64);
                read = self.ins().select(same, candidate, read);
                matched = self.ins().bor(matched, same);
            }
            emit_js_guard(&mut self.builder, matched, exit);
            return Some(read);
        }
        // The prototype's own value, read as every tier reads it (`staged::holder_slot`).
        let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
        let properties = self.builder.use_var(self.entry.as_ref().expect("entry facts").receiver.properties);
        let mut stage = super::object_slots::Emit::new(&mut self.builder).with(crate::vm::staged::Table::PropertyValues, properties).with(crate::vm::staged::Table::ObjectRecords, records);
        crate::vm::staged::holder_slot(&mut stage, record, holder, field.slot, field.storage_len, exit)
    }

    /// The record of `owner`'s object (a register's, or `this`'s), proved once until an
    /// effect that may change it retires the proof.
    /// The proved record of `owner`'s object, its shape and storage kind, proved once: for a
    /// `read`, an object's or a function's (`ObjectHeader::prove_readable`); for a store, an
    /// object's only, which a read's proof is not known to be.
    fn object_record(&mut self, state: &mut State, owner: Option<Register>, read: bool, exit: ir::Block) -> (ir::Value, ir::Value, ir::Value) {
        let bits = |lowering: &mut Self, state: &mut State| match owner {
            Some(register) => lowering.tagged(state, register),
            None => lowering.builder.use_var(lowering.entry.as_ref().expect("entry facts").this),
        };
        if let Some(known) = state.shapes.get(&owner).copied() {
            // A store of a read's proof needs only that it names an object's record: its
            // family, tested once, after which the proof is the object's.
            if !read && state.readable.remove(&owner) {
                let value = bits(self, state);
                let family = self.ins().band_imm_u(value, crate::value::ValueLayout::HEAP_FAMILY_MASK as i64);
                let object = self.ins().icmp_imm_u(IntCC::Equal, family, crate::value::ValueLayout::DIRECT_OBJECT_TAG as i64);
                emit_js_guard(&mut self.builder, object, exit);
            }
            return known;
        }
        let bits = bits(self, state);
        let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
        let header = if read {
            let functions = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, function_record_base));
            state.readable.insert(owner);
            ObjectHeader::prove_readable(&mut self.builder, bits, records, functions, exit)
        } else {
            state.readable.remove(&owner);
            ObjectHeader::prove(&mut self.builder, bits, records, exit)
        };
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
    /// The VM's own read of `read`'s atom through its cache site (`compiled::lane_field_get`),
    /// whose miss leaves through `exit`.
    fn field_probe(&mut self, object: ir::Value, read: (Atom, u16), exit: ir::Block) -> ir::Value {
        let (atom, site) = read;
        let site = self.ins().iconst(types::I64, (u64::from(atom) << u16::BITS | u64::from(site)) as i64);
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
        let ty = state.values[usize::from(source)].value.ty;
        let value = self.tagged(state, source);
        // A store a previous body left at may add its property (`compiled::lane_field_add`),
        // which changes the object's shape: every proof of an object retires, as another
        // register may name it, and `this`'s facts are read again.
        if self.generic.contains(&pc) {
            let object = match register {
                None => self.builder.use_var(self.entry.as_ref().expect("entry facts").this),
                Some(register) => self.tagged(state, register),
            };
            let write = self.ins().iconst(types::I64, (u64::from(instruction.atom_index()) << u16::BITS
                | u64::from(instruction.cache_site_index())) as i64);
            let payload = self.payload;
            let stored = emit_lane_helper_call(&mut self.builder, payload, self.helpers.field_add, &[payload, object, write, value]);
            let hit = self.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
            emit_js_guard(&mut self.builder, hit, exit);
            state.shapes.clear();
            state.layouts.clear();
            state.epoch = None;
            self.reload_property_arena();
            self.reload_receiver();
            return Some(true);
        }
        self.store_field(state, register, instruction.atom_index(), instruction.cache_site_index(), value, ty, exit)?;
        Some(true)
    }

    /// The receiver's shape and storage kind again, and the entry's tests of its shape, after
    /// a store that may have changed them (`emit_lane_receiver`, `Lowering::entry_facts`).
    fn reload_receiver(&mut self) {
        use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
        let entry = self.entry.as_ref().expect("entry facts");
        let (object_var, shape_var, inline_var) = (entry.receiver.object, entry.receiver.shape, entry.receiver.inline);
        let tests: Vec<(u32, Variable)> = entry.receiver_shapes.iter().map(|(shape, variable)| (*shape, *variable)).collect();
        let object = self.builder.use_var(object_var);
        let (found, done) = (self.builder.create_block(), self.builder.create_block());
        let present = self.ins().icmp_imm_u(IntCC::NotEqual, object, 0);
        self.ins().brif(present, found, &[], done, &[]);
        self.builder.switch_to_block(found);
        let shape = self.ins().load(types::I32, MemFlagsData::trusted(), object, OBJECT.auxiliary_offset as i32);
        let inline = self.ins().band_imm_u(shape, i64::from(OBJECT.packed_storage_mask));
        self.builder.def_var(shape_var, shape);
        self.builder.def_var(inline_var, inline);
        self.ins().jump(done, &[]);
        self.builder.switch_to_block(done);
        let shape = self.builder.use_var(shape_var);
        for (expected, variable) in tests {
            let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(expected));
            self.builder.def_var(variable, same);
        }
    }

    /// The store of `value` through `site` to the field `atom` of the object `register` names
    /// (`this` for `None`): the site's one proved shape and slot written directly, with the
    /// object's record proved once (the receiver's at entry, in a body that makes no call),
    /// else the VM's own-field store hit; anything else leaves through `exit`. A value of
    /// type `ty` the slot's representation may not hold takes the hit too, which joins it.
    #[allow(clippy::too_many_arguments)]
    fn store_field(&mut self, state: &mut State, register: Option<Register>, atom: Atom, site: u16,
        value: ir::Value, ty: Type, exit: ir::Block) -> Option<()> {
        let receiver = !self.native_calls
            && (register.is_none() || register.is_some_and(|register| state.receivers.contains(&register)));
        let owner = if receiver { None } else { register };
        let object_bits = match register {
            None => self.builder.use_var(self.entry.as_ref().expect("entry facts").this),
            Some(register) => self.tagged(state, register),
        };
        let shapes: Vec<NativeFieldLoad> = self.fields.get(&site).map(|shapes| shapes.iter()
            .filter(|field| field.holder.is_none()).copied().collect()).unwrap_or_default();
        let field = match shapes.as_slice() {
            [field] => *field,
            [] => {
                self.store_helper(state, object_bits, atom, site, value, exit);
                return Some(());
            }
            // A shape switch, as a read's (`shape_switch_read`): each own shape stores directly,
            // any other takes the VM's hit.
            _ => {
                let (object, shape, _) = self.object_record(state, register, false, exit);
                let admitted = self.emit_store_atom_admitted(atom);
                let properties = { let entry = self.entry.as_ref().expect("entry facts"); self.builder.use_var(entry.receiver.properties) };
                let (general, done) = (self.builder.create_block(), self.builder.create_block());
                for field in shapes {
                    let (matched, next) = (self.builder.create_block(), self.builder.create_block());
                    let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
                    let ok = self.ins().band(same, admitted);
                    self.ins().brif(ok, matched, &[], next, &[]);
                    self.builder.switch_to_block(matched);
                    self.store_direct(field, object_bits, object, properties, None, value, ty, done, general);
                    self.builder.switch_to_block(next);
                }
                self.ins().jump(general, &[]);
                self.builder.switch_to_block(general);
                self.store_helper(state, object_bits, atom, site, value, exit);
                self.ins().jump(done, &[]);
                self.builder.switch_to_block(done);
                return Some(());
            }
        };
        let header = (!receiver).then(|| self.object_record(state, register, false, exit));
        // A run that calls may meet a method cache that names the atom only after a call;
        // one that does not tests it once, at entry (`Lowering::entry_facts`).
        let admitted = self.builder.use_var(self.entry.as_ref().expect("entry facts").store_atoms[&atom]);
        let entry = self.entry.as_ref().expect("entry facts");
        let projection = state.layouts.get(&(owner, field.shape)).copied();
        let (object, shape) = header.map_or_else(|| (
            self.builder.use_var(entry.receiver.object),
            self.builder.use_var(entry.receiver.shape),
        ), |(object, shape, _)| (object, shape));
        let properties = self.builder.use_var(entry.receiver.properties);
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
        self.store_direct(field, object_bits, object, properties, projection, value, ty, done, general);
        self.builder.switch_to_block(general);
        self.store_helper(state, object_bits, atom, site, value, exit);
        self.ins().jump(done, &[]);
        self.builder.switch_to_block(done);
        Some(())
    }

    /// The store of `value`, of type `ty`, into `field`'s slot of the object `object` whose
    /// exact shape the caller proved `field.shape`, continuing at `done`; one the slot's
    /// representation, the barrier or the shape's storage form does not admit continues at
    /// `general`.
    #[allow(clippy::too_many_arguments)]
    fn store_direct(&mut self, field: NativeFieldLoad, object_bits: ir::Value, object: ir::Value,
        properties: ir::Value, projection: Option<CheckedObject>, value: ir::Value, ty: Type, done: ir::Block, general: ir::Block) {
        self.represented(field.representation, ty, value, general);
        if !ty.is_subtype(Type::NUMBER) {
            let record_id = self.ins().ireduce(types::I32, object_bits);
            let record_id = self.ins().uextend(types::I64, record_id);
            let payload = self.payload;
            emit_unlogged_property_store_guard(&mut self.builder, payload, record_id, value, general);
        }
        // The store every tier makes of a proved own field (`staged::store_slot`); a value the
        // shared store does not take continues at `general`.
        let layout = projection.or_else(|| CheckedObject::prove(&mut self.builder, object, properties, field, PackedAccess::Reload));
        let int = ty.is_subtype(Type::INT32);
        let stored = layout.and_then(|layout| crate::vm::staged::store_slot(
            &mut super::object_slots::Emit::new(&mut self.builder), layout, field.slot, value, int, general));
        let next = if stored.is_some() { done } else { general };
        self.ins().jump(next, &[]);
    }

    /// Continue where the slot representation `representation` holds `value`, of type `ty`,
    /// else at `otherwise`: statically where `ty` proves it, else by `value`'s int tag.
    fn represented(&mut self, representation: SlotRepresentation, ty: Type, value: ir::Value, otherwise: ir::Block) {
        let class = if ty.is_subtype(Type::INT32) { SlotRepresentation::Int32 } else { SlotRepresentation::Tagged };
        if class <= representation { return; }
        // An int is held by any representation but `None`, a slot no value reached yet.
        let held = if representation == SlotRepresentation::None { self.ins().iconst(types::I8, 0) }
            else { is_int_tagged(&mut self.builder, value) };
        emit_js_guard(&mut self.builder, held, otherwise);
    }

    /// The lane's logged own-field store hit (`Vm::primary_own_field_store_hit`), barrier
    /// included; anything else leaves through `exit`.
    fn store_helper(&mut self, state: &mut State, object: ir::Value, atom: Atom, site: u16, value: ir::Value, exit: ir::Block) {
        // The store may write a prototype's own field.
        state.epoch = None;
        let atom = self.ins().iconst(types::I64, i64::from(atom));
        let site = self.ins().iconst(types::I64, i64::from(site));
        let stored = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.field_set, &[object, atom, site, value]);
        let ok = self.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
        emit_js_guard(&mut self.builder, ok, exit);
    }

    fn binary(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let returns = instruction.returns_from_frame();
        if instruction.writes_current_this() || (instruction.result_flags() != 0 && !returns) {
            return None;
        }
        // A returning result completes the activation from its own register, at its caller
        // record (`Lowering::complete`); an inlined callee's is the unit's binary, move and
        // jump (`lane_unit`). A jump to the function's dead terminal `Return` instead would
        // leave there: no block starts at it.
        if returns && !matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_))) {
            return None;
        }
        let destination = instruction.result_register();
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        if !self.operand_supported(left) || !self.operand_supported(right) {
            return None;
        }
        let operator = instruction.binary_operator();
        let exit = self.guard_exit(state, pc as u32);
        use crate::vm::staged::NumberOperator;
        match NumberOperator::from_js_instruction(instruction.op(), operator) {
            // The interpreter's own Number arithmetic and bitwise operators, run on the
            // register state (`staged::step`); `None` for a constant no Number.
            Some(NumberOperator::Arithmetic(_) | NumberOperator::Bitwise(_)) => {
                let operation = super::lane_operation(&self.program.constants, instruction)?;
                crate::vm::staged::step(&mut LaneMachine { lowering: self, state, pc }, operation, exit)?;
            }
            // A relation's Boolean, from the predicate a branch on it would test.
            Some(NumberOperator::Relation(_)) | None => {
                let holds = self.relation(state, pc as u32, operator, left, right)?;
                let boolean = self.boolean(holds);
                self.set_boolean(state, destination, boolean);
            }
        }
        if returns {
            // The completion names the function's terminal `Return` where it has one, whose
            // register the lane reads it from, else the binary's own record.
            let terminal = self.unit.instructions.len().checked_sub(1)
                .filter(|terminal| self.unit.instructions[*terminal].op() == Op::Return)
                .and_then(|terminal| match self.unit.origins.get(terminal) {
                    Some(super::lane_unit::Origin::Caller(record)) => Some((*record, self.unit.instructions[terminal].register_a())),
                    _ => None,
                });
            let Some(super::lane_unit::Origin::Caller(own)) = self.unit.origins.get(pc).copied() else { unreachable!("a returning binary is a caller record's") };
            let bits = self.tagged(state, destination);
            // The activation's `Return` register holds it too, as the record it names reads.
            if let Some((_, register)) = terminal {
                let base = self.registers_base;
                self.ins().store(MemFlagsData::trusted(), bits, base, register_offset(register));
            }
            self.return_value(terminal.map_or(own, |(record, _)| record), bits);
            self.native += 1;
            return Some(false);
        }
        Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_field_reads_take_no_arena_bound() {
        let program = crate::Engine::specialize(
            include_str!("../../../tests/fixtures/own-slot-proofs.js"), "own-slot-proofs.js",
        ).unwrap();
        let id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "readPair")).unwrap();
        let fields: BTreeMap<u16, Vec<NativeFieldLoad>> = program.function(id).instructions().filter_map(|instruction| {
            if instruction.op() == Op::GetField
                && let Some(FieldLookup::Atom { atom, cache_site, .. }) = instruction.field_lookup()
                && let Some(slot) = ["left", "right"].iter().position(|name| *name == &program.atoms[atom as usize]) {
                Some((cache_site, vec![NativeFieldLoad { shape: 0, holder: None, slot: slot as u16, storage_len: 2,
                    representation: SlotRepresentation::Tagged, inherited: None }]))
            } else { None }
        }).collect();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let unit = super::super::lane_unit::unit(id as u32, program.function(id), 0, &|_, _| Vec::new(), &|_| None, None).unwrap();
        let (native, _) = lower(&program, id as u32,
            &unit, LaneHelpers::default(), &fields, &BTreeSet::new(), VmFacts { length: Atom::MAX, html_dda: true, diagnostics: false, function_call: None }, &[], &BTreeMap::new(), &[], None, &*isa).unwrap();
        let bounds = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
            .filter(|&instruction| {
                let ir::InstructionData::IntCompare { cond: IntCC::UnsignedLessThanOrEqual, args, .. }
                    = native.dfg.insts[instruction] else { return false };
                let ir::ValueDef::Result(length, _) = native.dfg.value_def(native.dfg.resolve_aliases(args[1]))
                    else { return false };
                matches!(native.dfg.insts[length], ir::InstructionData::Load { offset, .. }
                    if i32::from(offset) == std::mem::offset_of!(JsStepDispatch, property_len) as i32)
            }).count();
        // A live object's run lies inside the arena (`object_slots::arena_range`): neither read
        // compares it with the arena's length.
        assert_eq!(bounds, 0, "own field reads take no arena bound:\n{native}");
    }

    #[test]
    fn number_loops_keep_f64_values_and_proofs_across_the_backedge() {
        let program = crate::Engine::specialize(
            include_str!("../../../tests/fixtures/typed-lane-numbers.js"), "typed-lane-numbers.js",
        ).unwrap();
        let id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "accumulate")).unwrap();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let unit = super::super::lane_unit::unit(id as u32, program.function(id), 0, &|_, _| Vec::new(), &|_| None, None).unwrap();
        let (native, count) = lower(&program, id as u32,
            &unit, LaneHelpers::default(), &BTreeMap::new(), &BTreeSet::new(), VmFacts { length: Atom::MAX, html_dda: true, diagnostics: false, function_call: None }, &[], &BTreeMap::new(), &[], None, &*isa).expect("typed Number residual");
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

/// The specializer's machine (`staged::Machine`): the lowering's register state, on which the
/// interpreter's semantics (`staged::step`) emits the residual, a register's facts folded.
struct LaneMachine<'l, 'a, 'f> {
    lowering: &'l mut Lowering<'a, 'f>,
    state: &'l mut State,
    /// The unit PC of the instruction the machine runs.
    pc: usize,
}

impl<'f> LaneMachine<'_, '_, 'f> {
    fn emit(&mut self) -> super::object_slots::Emit<'_, 'f> {
        super::object_slots::Emit::new(&mut self.lowering.builder)
    }
}

macro_rules! delegate_stage {
    ($($name:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)*) => {
        $(fn $name(&mut self, $($arg: $ty),*) $(-> $ret)? { crate::vm::staged::Stage::$name(&mut self.emit(), $($arg),*) })*
    };
}

impl crate::vm::staged::Stage for LaneMachine<'_, '_, '_> {
    type Word = ir::Value;
    type Cond = ir::Value;
    type Miss = ir::Block;
    delegate_stage! {
        constant(bits: u64) -> ir::Value;
        load_u32(base: ir::Value, offset: i32) -> ir::Value;
        load_u64(base: ir::Value, offset: i32) -> ir::Value;
        store_u32(base: ir::Value, offset: i32, value: ir::Value);
        store_u64(base: ir::Value, offset: i32, value: ir::Value);
        load_table(address: ir::Value) -> ir::Value;
        add(a: ir::Value, b: ir::Value) -> ir::Value;
        or(a: ir::Value, b: ir::Value) -> ir::Value;
        add_imm(a: ir::Value, imm: u64) -> ir::Value;
        mul_imm(a: ir::Value, imm: u64) -> ir::Value;
        and_imm(a: ir::Value, imm: u64) -> ir::Value;
        or_imm(a: ir::Value, imm: u64) -> ir::Value;
        shr_imm(a: ir::Value, shift: u32) -> ir::Value;
        sar32_imm(a: ir::Value, shift: u32) -> ir::Value;
        eq_imm(a: ir::Value, imm: u64) -> ir::Value;
        below_imm(a: ir::Value, imm: u64) -> ir::Value;
        ne_imm(a: ir::Value, imm: u64) -> ir::Value;
        above_imm(a: ir::Value, imm: u64) -> ir::Value;
        below(a: ir::Value, b: ir::Value) -> ir::Value;
        guard(cond: ir::Value, miss: ir::Block) -> Option<()>;
    }
    fn choose(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> Option<ir::Value>, otherwise: impl FnOnce(&mut Self) -> Option<ir::Value>) -> Option<ir::Value> {
        let (then_block, otherwise_block, join) = (self.lowering.builder.create_block(), self.lowering.builder.create_block(), self.lowering.builder.create_block());
        let result = self.lowering.builder.append_block_param(join, types::I64);
        self.lowering.builder.ins().brif(cond, then_block, &[], otherwise_block, &[]);
        self.lowering.builder.switch_to_block(then_block);
        let value = then(self)?;
        self.lowering.builder.ins().jump(join, &[ir::BlockArg::Value(value)]);
        self.lowering.builder.switch_to_block(otherwise_block);
        let value = otherwise(self)?;
        self.lowering.builder.ins().jump(join, &[ir::BlockArg::Value(value)]);
        self.lowering.builder.switch_to_block(join);
        Some(result)
    }
    fn either(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.lowering.ins().bor(a, b) }
    /// The arena base the body keeps current across allocations; the record reservations'
    /// from the dispatch payload.
    fn table(&mut self, table: crate::vm::staged::Table) -> ir::Value {
        use crate::vm::staged::Table;
        let word = match table {
            Table::PropertyValues => {
                let entry = self.lowering.entry.as_ref().expect("entry facts");
                return self.lowering.builder.use_var(entry.receiver.properties);
            }
            Table::ObjectRecords => std::mem::offset_of!(JsStepDispatch, object_record_base),
            Table::ArrayRecords => std::mem::offset_of!(JsStepDispatch, array_record_base),
        };
        emit_js_payload_word(&mut self.lowering.builder, self.lowering.payload, word)
    }
}

impl crate::vm::staged::Machine for LaneMachine<'_, '_, '_> {
    /// A Boolean's bits are its truthiness; any other value's is the shared tagged test,
    /// which leaves for a heap value.
    fn value_truthy(&mut self, register: Register, miss: ir::Block) -> Option<ir::Value> {
        let bits = self.lowering.tagged(self.state, register);
        Some(if self.state.values[usize::from(register)].value.ty.is_subtype(Type::language(crate::ValueType::Boolean)) {
            self.lowering.ins().icmp_imm_u(IntCC::Equal, bits, Value::TRUE.to_bits() as i64)
        } else {
            emit_js_tagged_truthiness(&mut self.lowering.builder, bits, miss)
        })
    }
    fn register(&mut self, register: Register) -> ir::Value { self.lowering.tagged(self.state, register) }
    fn set_register(&mut self, register: Register, value: ir::Value) { self.lowering.set_tagged(self.state, register, value) }
    fn set_static(&mut self, register: Register, value: Value) {
        // A known value takes its narrowest representation, a static fact the residual folds.
        let ty = Type::immediate(value);
        let (representation, native) = if let Some(int) = value.as_int() {
            (Representation::Int32, self.lowering.ins().iconst(types::I32, i64::from(int)))
        } else if let Some(number) = value.as_number() {
            (Representation::Float64, self.lowering.ins().f64const(number))
        } else {
            (Representation::Tagged, self.lowering.ins().iconst(types::I64, value.to_bits() as i64))
        };
        let mut fact = RegisterFact::definition(ty, representation);
        fact.value.binding = BindingTime::Static;
        fact.value.proof = Proof::Static;
        self.lowering.define(self.state, register, native, fact);
    }
    /// A copy keeps every fact of the source: its representation and proofs.
    fn copy_register(&mut self, destination: Register, source: Register) { self.lowering.copy(self.state, destination, source) }
}

impl LaneMachine<'_, '_, '_> {
    /// `then`'s value where `cond` holds, else `otherwise`'s, the second arm cold: both
    /// arms' values in the type `join_type` picks from them (`coerce` converts each), and
    /// their register states joined after.
    fn branch<T: Copy>(
        &mut self,
        cond: ir::Value,
        then: impl FnOnce(&mut Self) -> Option<T>,
        otherwise: impl FnOnce(&mut Self) -> Option<T>,
        join_type: impl FnOnce(T, T) -> ir::Type,
        coerce: impl Fn(&mut Lowering<'_, '_>, T, ir::Type) -> ir::Value,
    ) -> Option<ir::Value> {
        let (then_block, otherwise_block) = (self.lowering.builder.create_block(), self.lowering.builder.create_block());
        self.lowering.builder.set_cold_block(otherwise_block);
        self.lowering.ins().brif(cond, then_block, &[], otherwise_block, &[]);
        let mut other_state = self.state.clone();
        self.lowering.builder.switch_to_block(then_block);
        let first = then(self)?;
        let first_end = self.lowering.builder.current_block().expect("an arm ends in a block");
        self.lowering.builder.switch_to_block(otherwise_block);
        // The second arm runs on the state the branch left, then gives back the first's.
        std::mem::swap(self.state, &mut other_state);
        let second = otherwise(self);
        std::mem::swap(self.state, &mut other_state);
        let second = second?;
        let ty = join_type(first, second);
        let join = self.lowering.builder.create_block();
        let parameter = self.lowering.builder.append_block_param(join, ty);
        let second = coerce(self.lowering, second, ty);
        self.lowering.ins().jump(join, &[ir::BlockArg::Value(second)]);
        self.lowering.builder.switch_to_block(first_end);
        let first = coerce(self.lowering, first, ty);
        self.lowering.ins().jump(join, &[ir::BlockArg::Value(first)]);
        self.lowering.builder.switch_to_block(join);
        self.lowering.join_states(self.state, &other_state);
        Some(parameter)
    }
}

impl crate::vm::staged::Numbers for LaneMachine<'_, '_, '_> {
    type Int = ir::Value;
    type Float = ir::Value;
    fn known_int(&self, register: Register) -> bool { self.state.values[usize::from(register)].value.ty.is_subtype(Type::INT32) }
    fn known_float(&self, register: Register) -> bool {
        self.state.values[usize::from(register)].value.representation == Representation::Float64
    }
    fn speculates(&self) -> bool { true }
    fn wraps(&self) -> bool { self.lowering.wrapping.contains(self.pc) }
    fn int_register(&mut self, register: Register, miss: ir::Block) -> Option<ir::Value> { Some(self.lowering.int(self.state, register, miss)) }
    fn float_register(&mut self, register: Register, miss: ir::Block) -> Option<ir::Value> { Some(self.lowering.number(self.state, register, miss)) }
    fn int_held(&mut self, register: Register) -> ir::Value {
        let bits = self.lowering.tagged(self.state, register);
        is_int_tagged(&mut self.lowering.builder, bits)
    }
    fn assume_int(&mut self, register: Register) {
        let bits = self.lowering.tagged(self.state, register);
        self.lowering.unbox_int(self.state, register, bits);
    }
    fn float_constant(&mut self, value: f64) -> ir::Value { self.lowering.ins().f64const(value) }
    fn cond_constant(&mut self, value: bool) -> ir::Value { self.lowering.ins().iconst(types::I8, i64::from(value)) }
    /// A comparison's negation is the comparison of the complement condition (for doubles,
    /// the unordered one, so NaN still negates), which a branch takes as it is; any other
    /// condition's is its flip.
    fn not_cond(&mut self, a: ir::Value) -> ir::Value {
        use cranelift_codegen::ir::condcodes::CondCode;
        let dfg = &self.lowering.builder.func.dfg;
        if let ir::ValueDef::Result(inst, 0) = dfg.value_def(a) {
            match dfg.insts[inst] {
                ir::InstructionData::IntCompare { cond, args: [left, right], .. } => return self.lowering.ins().icmp(cond.complement(), left, right),
                ir::InstructionData::FloatCompare { cond, args: [left, right], .. } => return self.lowering.ins().fcmp(cond.complement(), left, right),
                _ => {}
            }
        }
        self.lowering.ins().bxor_imm_u(a, 1)
    }
    fn set_boolean(&mut self, register: Register, cond: ir::Value) {
        let boolean = self.lowering.boolean(cond);
        self.lowering.set_boolean(self.state, register, boolean);
    }
    fn int_checked(&mut self, operation: super::operations::NumberArithmetic, left: ir::Value, right: ir::Value) -> (ir::Value, ir::Value) {
        operation.apply(&mut EmitArithmetic::<true> { builder: &mut self.lowering.builder }, left, right)
    }
    fn int_wrapping(&mut self, operation: super::operations::NumberArithmetic, left: ir::Value, right: ir::Value) -> ir::Value {
        if operation == super::operations::NumberArithmetic::Add { self.lowering.ins().iadd(left, right) } else { self.lowering.ins().isub(left, right) }
    }
    fn int_constant(&mut self, value: i32) -> ir::Value { self.lowering.ins().iconst(types::I32, i64::from(value)) }
    fn or_cond(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.lowering.ins().bor(a, b) }
    fn and_cond(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.lowering.ins().band(a, b) }
    fn int_bitwise(&mut self, operation: super::operations::NumberBitwise, left: ir::Value, right: ir::Value) -> ir::Value {
        emit_number_bitwise_bits(&mut self.lowering.builder, operation, left, right)
    }
    fn float_of_int(&mut self, int: ir::Value) -> ir::Value { self.lowering.ins().fcvt_from_sint(types::F64, int) }
    fn unsigned(&mut self, bits: ir::Value) -> ir::Value {
        let unsigned = self.lowering.ins().uextend(types::I64, bits);
        self.lowering.ins().fcvt_from_uint(types::F64, unsigned)
    }
    fn float_op(&mut self, operation: super::operations::NumberArithmetic, left: ir::Value, right: ir::Value) -> ir::Value {
        emit_number_arithmetic_operation(&mut self.lowering.builder, operation, left, right)
    }
    fn float_to_int32(&mut self, number: ir::Value, miss: ir::Block) -> Option<ir::Value> { Some(self.lowering.number_to_int32(number, miss)) }
    fn choose_number(
        &mut self,
        cond: ir::Value,
        then: impl FnOnce(&mut Self) -> Option<crate::vm::staged::Number<ir::Value, ir::Value>>,
        otherwise: impl FnOnce(&mut Self) -> Option<crate::vm::staged::Number<ir::Value, ir::Value>>,
    ) -> Option<crate::vm::staged::Number<ir::Value, ir::Value>> {
        use crate::vm::staged::Number;
        // One representation joins both: an int32 where both are, else the double.
        let mut int = false;
        let parameter = self.branch(cond, then, otherwise, |first, second| {
            int = matches!((first, second), (Number::Int(_), Number::Int(_)));
            if int { types::I32 } else { types::F64 }
        }, |lowering, number, ty| match number {
            Number::Int(value) if ty == types::F64 => lowering.ins().fcvt_from_sint(types::F64, value),
            Number::Int(value) | Number::Float(value) => value,
        })?;
        Some(if int { Number::Int(parameter) } else { Number::Float(parameter) })
    }
    fn choose_cond(
        &mut self,
        cond: ir::Value,
        then: impl FnOnce(&mut Self) -> Option<ir::Value>,
        otherwise: impl FnOnce(&mut Self) -> Option<ir::Value>,
    ) -> Option<ir::Value> {
        self.branch(cond, then, otherwise, |_, _| types::I8, |_, cond, _| cond)
    }
    fn int_relation(&mut self, relation: super::operations::NumberRelation, left: ir::Value, right: ir::Value) -> ir::Value {
        emit_relation(&mut self.lowering.builder, relation, left, right, NumberRelationDomain::Int32)
    }
    fn float_relation(&mut self, relation: super::operations::NumberRelation, left: ir::Value, right: ir::Value) -> ir::Value {
        emit_relation(&mut self.lowering.builder, relation, left, right, NumberRelationDomain::Float64)
    }
    fn set_number(&mut self, register: Register, number: crate::vm::staged::Number<ir::Value, ir::Value>) {
        match number {
            crate::vm::staged::Number::Int(int) => self.lowering.set_int(self.state, register, int),
            crate::vm::staged::Number::Float(number) => self.lowering.set_number(self.state, register, number),
        }
    }
}
