//! The lane region specializer: Futamura's first projection of the interpreter onto one
//! JavaScript function, run on the function's frameless lane activations under Ershov's
//! online division (Linear TOD-1232).
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
use crate::typed_ir::{BindingTime, Fact, Projection, Proof, Reentry, Representation, Type, VerifiedFact};
use super::object_slots::{CheckedObject, CheckedObjectTable, ObjectHeader, ObjectRecord, ObjectTable, PackedAccess};
use crate::value::EXACT_I64_TRUNCATION_LIMIT;
use std::collections::{BTreeMap, BTreeSet};
use cranelift_codegen::bitset::CompoundBitSet;
use cranelift_codegen::entity::EntityRef;

#[path = "lane_region/packed_ranges.rs"]
mod packed_ranges;
use crate::typed_ir::IntRange;

pub(super) const LANE_REGION_NAMESPACE: u32 = u32::from_be_bytes(*b"QLRG");
/// Inline scratch capacity for the small field-site sets typical of native units;
/// larger units spill normally, without a semantic or compilation limit.
const INLINE_FIELD_BINDINGS: usize = 4;
/// Common small effect frontiers stay inline; larger control-flow graphs spill normally.
const INLINE_EFFECT_EDGES: usize = 4;
/// Most blocks project only a few numbers; larger blocks spill without a limit.
const INLINE_NUMBER_PROJECTIONS: usize = 4;
/// A header and a short latch usually need only a few guarded inputs.
const INLINE_LOOP_INPUTS: usize = 4;
/// Common native-entry failure status; a pure scalar miss restarts its call.
const NUMBER_ENTRY_MISS: i64 = 1;
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
    value: VerifiedFact,
    tagged: TaggedView,
}

/// A current tagged view is either an entry/join variable or a dominating SSA value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TaggedView { Unavailable, Variable, Value(ir::Value) }

impl TaggedView {
    fn current(self) -> bool { self != Self::Unavailable }
}

impl RegisterFact {
    fn admit(value: Fact) -> Option<Self> {
        Some(Self { value: value.verified()?, tagged: if value.representation == Representation::Tagged {
            TaggedView::Variable
        } else { TaggedView::Unavailable } })
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
    proofs: LocalProofs,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FieldValue {
    /// Receiver home until its next definition (`None` is the fixed `this`).
    owner: Option<Register>,
    atom: Atom,
    value: ir::Value,
    ty: Type,
}

/// Available object and value proofs saved at control-flow edges. Register types
/// belong to the shared dataflow analysis and need no duplicate snapshots.
#[derive(Clone)]
struct LocalProofs {
    /// Direct-object proofs by owner, until an effect that may change the object retires
    /// them: a register's, and in the final entry fixed `this`'s.
    headers: Vec<Option<ObjectHeader>>,
    /// A fixed receiver (`None`) or register's complete own-slot projection.
    layouts: BTreeMap<(Option<Register>, u32), CheckedObject>,
    /// Registers holding the receiver, whose facts the entry proved.
    receivers: BTreeSet<Register>,
    /// Immutable reads; receiver definitions and memory effects retire these facts.
    fields: smallvec::SmallVec<[FieldValue; INLINE_FIELD_BINDINGS]>,
    /// Pure ToInt32 results, separate from each source value's semantic type.
    int32_projections: smallvec::SmallVec<[(ir::Value, ir::Value); INLINE_NUMBER_PROJECTIONS]>,
}

impl State {
    fn canonical(registers: usize) -> Self {
        Self { values: vec![RegisterFact::canonical(); registers], proofs: LocalProofs {
            headers: vec![None; registers + 1], layouts: BTreeMap::new(), receivers: BTreeSet::new(),
            fields: smallvec::SmallVec::new(), int32_projections: smallvec::SmallVec::new() } }
    }

    /// The `headers` index of an owner: a register, or fixed `this` (`None`).
    fn header_index(&self, owner: Option<Register>) -> usize {
        owner.map_or(self.values.len(), usize::from)
    }

    /// Native reentry retires shape/storage and function-body admission proofs;
    /// object records and language families stay.
    fn call_effect(&mut self) {
        self.proofs.fields.clear();
        self.proofs.int32_projections.clear();
        self.proofs.headers.fill(None);
        self.proofs.layouts.clear();
        for fact in &mut self.values {
            let unshaped = fact.value.ty.without_object_shape();
            if unshaped != fact.value.ty {
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(unshaped, fact.value.representation) };
            } else if fact.value.ty.body_index().is_some() {
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(Type::language(crate::ValueType::Function), fact.value.representation) };
            }
        }
    }
}

/// An intermediate chain value has no register lifetime to memoize.
#[derive(Clone, Copy)]
enum FieldOwner { This, Register(Register), Temporary }

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReceiverRecovery { Object, ObjectOrNullish }

impl ReceiverRecovery {
    fn for_body(body: &crate::bytecode::Function) -> Self {
        if body.instructions().all(|instruction| {
            !instruction.writes_current_this()
                && !matches!(instruction.op(), Op::LoadThis | Op::SetThisField | Op::SetThisFieldStrict)
                && (instruction.op() != Op::GetField || matches!(instruction.field_lookup(),
                    Some(FieldLookup::Atom { base, .. }) if base != crate::bytecode::FieldBase::THIS))
        }) { Self::ObjectOrNullish } else { Self::Object }
    }
}

/// Region-wide facts proved at entry.
struct Entry {
    receiver: LaneReceiver,
    /// This heap's non-null record reservation, fixed for the native run.
    object_table: Option<CheckedObjectTable>,
    /// Entry receiver certificates: record locators of `this` (the final entry) and of the
    /// caller registers no unit instruction writes. Records stay put in a run, which
    /// collects nothing; shapes are derived per use.
    records: Vec<Option<ObjectRecord>>,
    /// Per folded receiver shape, whether the receiver has it.
    receiver_shapes: BTreeMap<u32, Variable>,
    /// Per stored atom, whether the run admits direct stores of it.
    store_atoms: BTreeMap<Atom, Variable>,
    /// The running activation's dense capture slots (`EnvironmentSlots::native_dense_view`).
    capture_values: Variable,
    capture_len: Variable,
    this: Variable,
    /// Flat per-site immutable slot inputs; no per-operation probe.
    fields: smallvec::SmallVec<[(u16, FieldSlotBinding); INLINE_FIELD_BINDINGS]>,
    /// Function-bound numeric calls whose invariant closure was admitted here.
    calls: CompoundBitSet,
    /// Inlined frames whose invariant receiver needs no boxing on recovery.
    call_receivers: CompoundBitSet,
}

pub(super) struct LaneRegion {
    pub(super) code: Vec<u8>,
    /// Instructions the region runs natively, for admission.
    pub(super) native: usize,
    pub(super) clif: Option<String>,
    pub(super) fault_poll: bool,
    pub(super) first_entry: bool,
}

/// One typed numeric return edge, materialized after joining all return facts.
struct NumberReturn {
    block: ir::Block,
    fact: VerifiedFact,
}

#[derive(Clone, Copy)]
struct LoopRead {
    value: Variable,
    ty: Type,
    representation: Representation,
    /// The read's actual entry proof, valid while the qualified loop cannot
    /// replace this receiver or change its shape. Never inferred from feedback.
    receiver: Option<(Register, Type)>,
    header: Option<ObjectHeader>,
}

#[derive(Clone, Copy)]
struct InvariantRead {
    pc: usize,
    /// The canonical caller home supplying a field receiver at loop entry.
    owner: Option<Register>,
}

/// A STORE-site projection admitted on entry to a storage-preserving loop.
/// The record is physical receiver identity, not merely an equal shape.
#[derive(Clone, Copy)]
struct LoopStore {
    header: ObjectHeader,
    address: ir::Value,
    packed_range: Option<IntRange>,
}

pub(super) struct LoweredRegion {
    pub(super) number_result: Option<NativeNumberResult>,
    pub(super) function: ir::Function,
    pub(super) native: usize,
    pub(super) fault_poll: bool,
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
    /// The calls, by unit PC, that call a callee's compiled entry (`DirectCallee`).
    direct: &'a BTreeMap<usize, *const DirectCallee>,
    /// Per inlined frame, its callee's dense capture slots (values and count), fixed by the
    /// callee's identity, which its call guards; null where they are not dense.
    frame_captures: &'a [(usize, usize)],
    context: ir::Value,
    /// An entry-bound poll page, or the ordinary atomic termination flag.
    interrupt: ir::Value,
    fault_poll: bool,
    /// Callback completion edges can retire the native entry's interrupt ownership proof.
    reentries: smallvec::SmallVec<[ir::Block; INLINE_EFFECT_EDGES]>,
    polls: smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>,
    /// Already-conforming backedges meet before their common fault poll.
    poll_latches: super::SecondaryMap<ir::Block, Option<ir::Block>>,
    shared_headers: CompoundBitSet,
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
    /// Whether this entry's caller loop reaches its native latch.
    backedge: bool,
    /// Guard failure blocks to fill once the region is lowered (`guard_exit`).
    pending: Vec<(ir::Block, State, u32)>,
    /// Whether the unit makes native calls (`Lowering::call`): a callee may change `this`,
    /// so no read or store folds the receiver facts its entry proved.
    native_calls: bool,
    /// The registers' types at each block leader (`facts::analyze`).
    facts: super::facts::Facts,
    /// The unit's register liveness by unit PC (`Unit::register_masks`), where the unit
    /// fits one register set; otherwise the caller's by its PCs and per inlined frame its
    /// callee's, projected per query (`Lowering::is_live`). A function without masks keeps
    /// every register live.
    live: Liveness<'a>,
    valid: bool,
    coverage: Coverage,
    completion: Completion,
    /// One return stays inline; additional returns spill without a region limit.
    number_returns: smallvec::SmallVec<[NumberReturn; 1]>,
    /// Per loop header, the reads its loop repeats unchanged (`invariant_reads`), made once
    /// on the loop's one entry edge; each read's PC names the value it made there and
    /// whether its int tag was tested.
    hoists: BTreeMap<usize, Vec<InvariantRead>>,
    hoisted: BTreeMap<usize, LoopRead>,
    /// Successful write-through stores defining a loop-header field's SSA variable.
    loop_fields: Vec<LoopField>,
    loop_stores: BTreeMap<usize, LoopStore>,
    /// The PC being lowered, whose edges to a header enter its loop or close it.
    at: usize,
    /// Per block leader, its forward edges (`forward_edges`), and the states the lowered
    /// ones arrived with: a block they all reach keeps the local proofs they share.
    forward: Vec<Option<usize>>,
    arrived: BTreeMap<usize, Vec<LocalProofs>>,
    /// The block leaders an entry or a lowered edge enters: a block none does is not
    /// lowered, its code reached only past a leave.
    entered: CompoundBitSet,
    /// Whether a run can do anything but leave: complete natively or run a loop natively. A
    /// body that can do neither declines (`lower`).
    finishes: bool,
    /// The one epilogue every stop shares: it returns its status parameter.
    leave: ir::Block,
    /// The stop at a record: it writes its result word parameter and leaves stopped.
    stop: ir::Block,
}

enum Liveness<'a> {
    Unit(Option<std::borrow::Cow<'a, crate::compile::liveness::RegisterMasks>>),
    Frames {
        caller: Option<&'a crate::compile::liveness::RegisterMasks>,
        frames: Vec<Option<&'a crate::compile::liveness::RegisterMasks>>,
    },
}

/// How much of the unit must run natively: a prefix (a lane body, which leaves to the lane
/// anywhere), or all of it but the leaves to its source (a static typed region).
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "aot"), allow(dead_code))]
pub(super) enum Coverage {
    Prefix,
    Full,
    /// A caller-origin entry into the original unit CFG. Opaque operations
    /// leave at their exact origin; earlier loops and effects are not replayed.
    PrefixFrom(usize),
    FullFrom(usize),
    /// A loop-header-only body. Normal activation entry resumes baseline PC0.
    Osr(usize),
}

impl Coverage {
    pub(super) fn entry(self) -> usize { match self { Self::PrefixFrom(pc) | Self::FullFrom(pc) | Self::Osr(pc) => pc, _ => 0 } }
    fn full(self) -> bool { matches!(self, Self::Full | Self::FullFrom(_)) }
    fn selected_entry(self) -> bool { matches!(self, Self::PrefixFrom(_) | Self::FullFrom(_) | Self::Osr(_)) }
}

/// How a run ends: a lane body's status and result word (`LaneExit`), or a static typed
/// region's source PC, which its canonical code continues at.
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
    unit.frames.is_empty() && numeric_instructions(program, &unit.instructions)
}

pub(super) fn restartable_numeric_function(program: &ResidualProgram, function: &crate::bytecode::Function,
    instructions: &[WideInstruction]) -> bool {
    super::super::js_compiled::function_candidate(function, program) && numeric_instructions(program, instructions)
}

fn numeric_instructions(program: &ResidualProgram, instructions: &[WideInstruction]) -> bool {
    instructions.iter().all(|instruction| {
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
pub(super) fn writes(instruction: WideInstruction) -> Vec<Register> {
    instruction.written_registers().collect()
}

impl<'a, 'f> Lowering<'a, 'f> {
    fn ins(&mut self) -> cranelift_frontend::FuncInstBuilder<'_, 'f> {
        self.builder.ins()
    }

    fn tagged(&mut self, state: &mut State, register: Register) -> ir::Value {
        let index = usize::from(register);
        let fact = state.values[index];
        if let TaggedView::Value(value) = fact.tagged { return value; }
        if !fact.tagged.current() {
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
            state.values[index].tagged = TaggedView::Value(bits);
            return bits;
        }
        let bits = self.builder.use_var(self.registers.tagged[index]);
        state.values[index].tagged = TaggedView::Value(bits);
        bits
    }

    fn define(&mut self, state: &mut State, register: Register, value: ir::Value, mut fact: RegisterFact) {
        let index = usize::from(register);
        // Residual definitions include synthetic destinations, such as the
        // canonical Return home of a fused returning arithmetic operation.
        self.written.insert(index);
        let variable = match fact.value.representation {
            Representation::Tagged => self.registers.tagged[index],
            Representation::Int32 => self.int_var(index),
            Representation::Float64 => self.float_var(index),
        };
        self.builder.def_var(variable, value);
        if fact.value.representation == Representation::Tagged { fact.tagged = TaggedView::Value(value); }
        state.values[index] = fact;
        state.proofs.headers[index] = None;
        state.proofs.layouts.retain(|(owner, _), _| *owner != Some(register));
        state.proofs.receivers.remove(&register);
        state.proofs.fields.retain(|field| field.owner != Some(register));
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

    /// Prove an exact int32 view once; a Number already in a float register
    /// needs no canonical encoding or tag test to establish that projection.
    fn int(&mut self, state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
        let index = usize::from(register);
        let fact = state.values[index];
        let int = match fact.value.representation {
            Representation::Int32 => {
                let variable = self.int_var(index);
                return self.builder.use_var(variable);
            }
            Representation::Float64 if !fact.tagged.current() => {
                let variable = self.float_var(index);
                let number = self.builder.use_var(variable);
                let (int, exact) = emit_js_exact_int32(&mut self.builder, number);
                if !fact.value.ty.is_subtype(Type::INT32) {
                    emit_js_guard(&mut self.builder, exact, otherwise);
                }
                int
            }
            Representation::Tagged | Representation::Float64 => {
                let bits = self.tagged(state, register);
                if !fact.value.ty.is_subtype(Type::INT32) {
                    let integer = is_int_tagged(&mut self.builder, bits);
                    emit_js_guard(&mut self.builder, integer, otherwise);
                }
                self.ins().ireduce(types::I32, bits)
            }
        };
        let variable = self.int_var(index);
        self.builder.def_var(variable, int);
        state.values[index] = RegisterFact { tagged: state.values[index].tagged,
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
                state.values[index] = RegisterFact { tagged: state.values[index].tagged,
                    ..RegisterFact::definition(Type::NUMBER, Representation::Float64) };
                number
            }
        }
    }

    fn canonicalize(&mut self, state: &mut State, pc: usize) {
        for register in 0..state.values.len() {
            if self.is_live(pc, register as Register) && !state.values[register].tagged.current() {
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
        // A pure callee miss restarts its call. A nested marker names the
        // parent's call PC, so recovery keeps any already-performed parent effects.
        let mut pc = pc as usize;
        while let Some(super::lane_unit::Origin::Inlined { frame, .. }) = self.unit.origins.get(pc).copied() {
            if self.unit.frames[frame].recovery != super::lane_unit::CallRecovery::RestartCall { break; }
            pc = self.unit.markers[frame];
        }
        self.canonicalize(state, pc);
        let block = match self.exits[pc] {
            Some(block) => block,
            None => {
                let block = self.builder.create_block();
                self.builder.set_cold_block(block);
                self.exits[pc] = Some(block);
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
        // A block before this one that no edge entered was not lowered.
        let lowered = pc > self.at || self.entered.contains(pc);
        match self.blocks.get(pc).copied().flatten().filter(|_| lowered) {
            Some(block) => {
                self.entered.insert(pc);
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
            self.arrived.entry(pc).or_default().push(state.proofs.clone());
        }
    }

    /// The local proofs every forward edge into `pc` arrived with, as the same values, for
    /// a block no other edge enters: each was made in a block over all of them, so over this.
    fn keep_shared_proofs(&mut self, state: &mut State, pc: usize) {
        let Some(edges) = self.forward.get(pc).copied().flatten() else { return };
        let Some(arrived) = self.arrived.remove(&pc) else { return };
        let Some((first, rest)) = arrived.split_first().filter(|_| arrived.len() == edges) else { return };
        for (index, header) in state.proofs.headers.iter_mut().enumerate() {
            *header = first.headers[index].filter(|proof| rest.iter()
                .all(|other| other.headers[index] == Some(*proof)));
        }
        state.proofs.layouts = first.layouts.iter().filter(|(key, projection)| rest.iter()
            .all(|other| other.layouts.get(key) == Some(projection))).map(|(key, projection)| (*key, *projection)).collect();
        state.proofs.receivers = first.receivers.iter().filter(|register| rest.iter()
            .all(|other| other.receivers.contains(register))).copied().collect();
        state.proofs.fields = first.fields.iter().copied().filter(|field| rest.iter()
            .all(|other| other.fields.contains(field))).collect();
        state.proofs.int32_projections = first.int32_projections.iter().copied().filter(|projection| rest.iter()
            .all(|other| other.int32_projections.contains(projection))).collect();
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
            if ty == Type::DIRECT_OBJECT_FAMILY && !state.values[index].value.ty.is_direct_object_family() {
                let exit = self.guard_exit(state, target as u32);
                let bits = self.tagged(state, register);
                let family = self.ins().band_imm_u(bits, crate::value::ValueLayout::HEAP_FAMILY_MASK as i64);
                let object = self.ins().icmp_imm_u(IntCC::Equal, family, crate::value::ValueLayout::DIRECT_OBJECT_TAG as i64);
                emit_js_guard(&mut self.builder, object, exit);
                self.refine_object_family(state, bits);
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
                    binding: if pc == self.coverage.entry() { BindingTime::Location } else { BindingTime::Dynamic },
                    proof: if pc != self.coverage.entry() { Proof::Definition }
                        else if ty == Type::ANY { Proof::CanonicalValue } else { Proof::EntryGuard },
                    projection: Projection::Exact,
                }) else { self.valid = false; return state; };
                state.values[index] = fact;
            }
        }
        state
    }

    /// Whether unit `register` is live at unit `pc`. Without unit masks: the frame that owns
    /// it is in the chain of frames `pc` runs in, and its function's masks keep it live at
    /// the frame's own PC there (the PC itself for the innermost, its call's for each frame
    /// around it).
    fn is_live(&self, pc: usize, register: Register) -> bool {
        let (caller, frames) = match &self.live {
            Liveness::Unit(masks) => return masks.as_deref().is_none_or(|live| live.is_live(pc, register)),
            Liveness::Frames { caller, frames } => (caller, frames),
        };
        let (mut frame, mut at) = match self.unit.origins.get(pc).copied() {
            Some(super::lane_unit::Origin::Inlined { frame, pc }) => (Some(frame), pc),
            Some(super::lane_unit::Origin::Caller(pc)) => (None, pc),
            None => (None, self.unit.end),
        };
        while let Some(index) = frame {
            let inlined = self.unit.frames[index];
            if (inlined.base..inlined.base + inlined.registers).contains(&register) {
                return frames[index].is_none_or(|live| live.is_live(at as usize, register - inlined.base));
            }
            (frame, at) = (inlined.parent, inlined.call);
        }
        register < self.unit.caller_registers && caller.is_none_or(|live| live.is_live(at as usize, register))
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
pub(super) fn leaders(instructions: &[WideInstruction]) -> CompoundBitSet {
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
            Some(site) if site.sink.is_none() => site.second.unwrap_or(site.first).1,
            _ => return false,
        },
        None => return false,
    };
    fields.get(&last).is_some_and(|shapes| !shapes.is_empty()
        && shapes.iter().all(|field| field.ty == Type::INT32))
}

/// Whether `instruction` can change no heap value natively: anything else it would do (a
/// non-Number operand, a missed cache) leaves the body first.
fn reads_only(constants: &[crate::bytecode::Constant], instruction: WideInstruction) -> bool {
    match instruction.op() {
        Op::Move | Op::Nop | Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse => true,
        Op::LoadConst => constants.get(instruction.constant_index()).and_then(super::facts::immediate).is_some(),
        Op::IncDec => instruction.numeric_local_target().is_none(),
        Op::LoadCapture => instruction.result_flags() == 0 && instruction.capture_depth() == 0,
        Op::GetField => !instruction.writes_current_this() && !instruction.returns_from_frame(),
        op => op.js_denotation() == crate::bytecode::JsDenotation::NumericBinary && !instruction.returns_from_frame(),
    }
}

/// Whether `instruction` is a store natively: of its atom, into a slot the object already
/// has, so it changes no shape, layout or other property; anything else leaves first.
fn stores_own(instruction: WideInstruction) -> bool {
    matches!(instruction.op(), Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict)
        && !instruction.returns_from_frame()
}

/// Loops with one external entry and no interior entry, whose native operations
/// only read or replace existing own fields. Misses leave before guest effects.
/// A sole native entry at the header can supply the same entry proofs.
fn local_loops(unit: &super::lane_unit::Unit, constants: &[crate::bytecode::Constant], starts: &[usize])
    -> Vec<(usize, usize)> {
    let instructions = &unit.instructions;
    let target = |instruction: &WideInstruction| matches!(instruction.op().control_flow_layout(),
        ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump).then(|| instruction.jump_target() as usize);
    let falls = |instruction: &WideInstruction| !matches!(instruction.op().control_flow_layout(),
        ControlFlowLayout::Jump | ControlFlowLayout::Terminal | ControlFlowLayout::BranchTable);
    let mut loops = Vec::new();
    for (back, instruction) in instructions.iter().enumerate() {
        let Some(header) = target(instruction).filter(|header| *header <= back
            && (!starts.contains(header) || starts == [*header])) else { continue };
        let entry_header = starts == [header];
        let body = &instructions[header..=back];
        let entries = usize::from(header > 0 && falls(&instructions[header - 1]))
            + instructions[..header].iter().filter(|candidate| target(candidate) == Some(header)).count();
        let side = instructions.iter().enumerate().any(|(pc, candidate)| !(header..=back).contains(&pc)
            && target(candidate).is_some_and(|to| (header + 1..=back).contains(&to) || (to == header && pc > back)));
        if (entries != 1 && !entry_header) || side
            || starts.iter().any(|start| (header + 1..=back).contains(start))
            || !body.iter().all(|candidate| reads_only(constants, *candidate) || stores_own(*candidate)) { continue }
        loops.push((header, back));
    }
    loops
}

fn invariant_reads(unit: &super::lane_unit::Unit, loops: &[(usize, usize)]) -> BTreeMap<usize, Vec<InvariantRead>> {
    let mut hoists = BTreeMap::new();
    let boundaries = leaders(&unit.instructions);
    for &(header, back) in loops {
        let body = &unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|candidate| writes(*candidate)).collect();
        let stored: BTreeSet<Atom> = body.iter().filter(|candidate| stores_own(**candidate))
            .map(|candidate| candidate.atom_index()).collect();
        let mut reads: Vec<InvariantRead> = body.iter().enumerate().filter_map(|(offset, candidate)| {
            let pc = header + offset;
            if candidate.op() == Op::LoadCapture
                && matches!(unit.origins[pc], super::lane_unit::Origin::Caller(_)) {
                return Some(InvariantRead { pc, owner: None });
            }
            if candidate.op() != Op::GetField { return None; }
            match candidate.field_lookup() {
            Some(FieldLookup::Atom { atom, base, .. }) if !stored.contains(&atom) => {
                let owner = invariant_field_owner(unit, header, pc, base.register_index()?, &written, &boundaries)?;
                Some(InvariantRead { pc, owner: Some(owner) })
            }
            _ => None,
        }}).collect();
        // An inlined capture belongs to the actual closure admitted at its
        // marker, not merely to a function body or an observed environment.
        // Its invariant callee read must run first on the entry edge.
        let captures: Vec<_> = body.iter().enumerate().filter_map(|(offset, instruction)| {
            let pc = header + offset;
            let super::lane_unit::Origin::Inlined { frame, .. } = unit.origins[pc] else { return None };
            (instruction.op() == Op::LoadCapture
                && invariant_callee_read(unit, reads.iter().map(|read| read.pc), frame, &boundaries).is_some())
                .then_some(InvariantRead { pc, owner: None })
        }).collect();
        reads.extend(captures);
        if !reads.is_empty() { hoists.insert(header, reads); }
    }
    hoists
}

/// Resolve only block-local receiver copies to a caller home unchanged by the
/// whole loop. A branch or implicit frame initialization ends this proof.
fn invariant_field_owner(unit: &super::lane_unit::Unit, header: usize, pc: usize,
    mut owner: Register, written: &BTreeSet<Register>, boundaries: &CompoundBitSet) -> Option<Register> {
    for at in (header..pc).rev() {
        if owner < unit.caller_registers && !written.contains(&owner) { return Some(owner); }
        let instruction = unit.instructions[at];
        if boundaries.contains(at + 1) || unit.markers.contains(&at)
            || instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { return None; }
        if writes(instruction).contains(&owner) {
            if instruction.op() != Op::Move || instruction.result_register() != owner { return None; }
            owner = instruction.register_b();
        }
    }
    (owner < unit.caller_registers && !written.contains(&owner)).then_some(owner)
}

/// A monomorphic frame whose callee comes from an entry read through only
/// block-local copies. Other markers implicitly initialize scratch homes.
fn invariant_callee_read(unit: &super::lane_unit::Unit, reads: impl IntoIterator<Item = usize>, frame: usize,
    boundaries: &CompoundBitSet) -> Option<usize> {
    let call = unit.frames[frame];
    let marker = unit.markers[frame];
    if call.alternative.is_some() || unit.frames.iter().any(|other| other.alternative == Some(marker)) {
        return None;
    }
    reads.into_iter().find(|&pc| {
        if pc >= marker || (pc + 1..=marker).any(|at| boundaries.contains(at)) { return false; }
        let mut source = call.callee_register;
        for at in (pc + 1..marker).rev() {
            let instruction = unit.instructions[at];
            if unit.markers.contains(&at)
                || instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { return false; }
            if writes(instruction).contains(&source) {
                if instruction.op() != Op::Move || instruction.result_register() != source { return false; }
                source = instruction.register_b();
            }
        }
        unit.instructions[pc].result_register() == source
    })
}

#[cfg(test)]
fn record_field_forward(cache_site: u16) {
    FIELD_VALUE_FORWARDS.with(|count| count.set(count.get() + 1));
    FIELD_FORWARD_SITE.with(|site| {
        if let Some((expected, count)) = site.get() && expected == cache_site {
            site.set(Some((expected, count + 1)));
        }
    });
}

/// One authoritative alias plan; lowering derives entry probes and typed SSA
/// homes from it. Different stable roots are not proofs of disjoint objects.
struct LoopField {
    header: usize,
    owner: Register,
    atom: Atom,
    reads: smallvec::SmallVec<[usize; INLINE_FIELD_BINDINGS]>,
    stores: smallvec::SmallVec<[usize; INLINE_FIELD_BINDINGS]>,
    /// Conditional on this field's packed/no-extras entry admission.
    store_ranges: smallvec::SmallVec<[Option<IntRange>; INLINE_FIELD_BINDINGS]>,
}

fn successors(pc: usize, instruction: WideInstruction) -> [Option<usize>; 2] {
    let layout = instruction.op().control_flow_layout();
    let falls = !matches!(layout, ControlFlowLayout::Jump | ControlFlowLayout::Terminal | ControlFlowLayout::BranchTable);
    let jumps = matches!(layout, ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump)
        .then(|| instruction.jump_target() as usize);
    [falls.then_some(pc + 1), jumps]
}

/// Forward must-alias analysis through inline argument copies and branch joins.
/// Only a single backedge is admitted, so every incoming state is complete when
/// visited. Unknown writes decline promotion; no runtime alias cache is needed.
fn loop_fields(program: &ResidualProgram, unit: &super::lane_unit::Unit,
    loops: &[(usize, usize)]) -> Vec<LoopField> {
    let mut fields: Vec<LoopField> = Vec::new();
    for &(header, back) in loops {
        if loops.iter().any(|&(other, end)| (other, end) != (header, back) && other <= back && header <= end)
            || (header..=back).any(|pc| successors(pc, unit.instructions[pc]).into_iter().flatten()
                .any(|target| target <= pc && (pc, target) != (back, header)))
            || unit.markers.iter().zip(&unit.frames).any(|(marker, frame)|
                (header..=back).contains(marker) && frame.alternative.is_some()) { continue; }
        let mut written: CompoundBitSet = CompoundBitSet::with_capacity(usize::from(unit.registers));
        for instruction in &unit.instructions[header..=back] {
            for register in writes(*instruction) { written.insert(usize::from(register)); }
        }
        let roots: Vec<Option<Register>> = (0..unit.registers)
            .map(|register| (register < unit.caller_registers && !written.contains(usize::from(register))).then_some(register)).collect();
        let mut incoming = vec![None; back - header + 1];
        incoming[0] = Some(roots);
        let first_field = fields.len();
        let mut stores = Vec::new();
        for pc in header..=back {
            let Some(mut aliases) = incoming[pc - header].take() else { continue };
            if let Some(frame) = unit.markers.iter().position(|marker| *marker == pc).map(|index| unit.frames[index]) {
                for register in frame.base..frame.base + frame.registers { aliases[usize::from(register)] = None; }
                for (destination, source) in frame.passed_registers(program.function(frame.target.function as usize)) {
                    aliases[usize::from(destination)] = aliases[usize::from(source)];
                }
            }
            let instruction = unit.instructions[pc];
            if instruction.op() == Op::GetField && instruction.result_flags() == 0
                && let Some(FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup()
                && let Some(owner) = base.register_index().and_then(|register| aliases[usize::from(register)]) {
                if let Some(field) = fields[first_field..].iter_mut().find(|field| field.owner == owner && field.atom == atom) {
                    field.reads.push(pc);
                } else {
                    fields.push(LoopField { header, owner, atom, reads: smallvec::smallvec![pc],
                        stores: smallvec::SmallVec::new(), store_ranges: smallvec::SmallVec::new() });
                }
            }
            if stores_own(instruction) {
                let receiver = match instruction.op() {
                    Op::SetField | Op::SetFieldStrict => Some(instruction.register_b()),
                    _ => match unit.origins[pc] {
                        super::lane_unit::Origin::Inlined { frame, .. } => Some(unit.frames[frame].receiver),
                        _ => None,
                    },
                };
                stores.push((receiver.and_then(|register| aliases[usize::from(register)]), instruction.atom_index(), pc));
            }
            let copied = (instruction.op() == Op::Move).then(|| aliases[usize::from(instruction.register_b())]).flatten();
            for register in writes(instruction) { aliases[usize::from(register)] = None; }
            if instruction.op() == Op::Move { aliases[usize::from(instruction.result_register())] = copied; }
            let targets = successors(pc, instruction).map(|target| target.filter(|target| *target > pc && *target <= back));
            let mut targets = targets.into_iter().flatten().peekable();
            while let Some(target) = targets.next() {
                // Linear control transfers ownership of its flat state; only a
                // branch needs a second copy for the second successor.
                let incoming_roots = if targets.peek().is_some() { aliases.clone() } else { std::mem::take(&mut aliases) };
                let slot = &mut incoming[target - header];
                if let Some(known) = slot {
                    for (known, root) in known.iter_mut().zip(incoming_roots) {
                        if *known != root { *known = None; }
                    }
                } else { *slot = Some(incoming_roots); }
            }
        }
        for field in &mut fields[first_field..] {
            if stores.iter().any(|(owner, atom, _)| *atom == field.atom && *owner != Some(field.owner)) { continue; }
            field.stores.extend(stores.iter().filter_map(|(_, atom, pc)| (*atom == field.atom).then_some(*pc)));
        }
        fields.retain(|field| !field.stores.is_empty());
        for field in fields.iter_mut().filter(|field| field.header == header) {
            field.store_ranges = packed_ranges::analyze(program, unit, field, back);
        }
    }
    fields
}

/// Per block leader, the edges into it, all from before it; `None` for one an entry
/// (`starts`) or a later PC enters, whose proofs no earlier edge can give.
fn forward_edges(instructions: &[WideInstruction], leaders: &CompoundBitSet, starts: &[usize]) -> Vec<Option<usize>> {
    let mut edges: Vec<Option<usize>> = (0..instructions.len()).map(|pc| leaders.contains(pc).then_some(0)).collect();
    for start in starts { if let Some(slot) = edges.get_mut(*start) { *slot = None; } }
    for (pc, instruction) in instructions.iter().enumerate() {
        for target in successors(pc, *instruction).into_iter().flatten() {
            let Some(slot) = edges.get_mut(target) else { continue };
            *slot = if target > pc { slot.map(|count| count + 1) } else { None };
        }
    }
    edges
}

/// Why a unit has no lane body: it does not lower, or its machine code is rejected.
pub(super) enum LaneDecline {
    Unlowered,
    Rejected(super::super::jit_backend::NativeCompileRejection),
}

/// Specializes `unit` (`lane_unit::unit`) as a lane body, whose field cache sites `fields`
/// gives the proved shapes and slots of.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &BTreeMap<u16, Vec<NativeFieldLoad>>,
    global_reads: &BTreeSet<usize>,
    direct: &BTreeMap<usize, *const DirectCallee>,
    frame_captures: &[(usize, usize)],
    loop_entry: Option<usize>,
    allow_first_entry: bool,
) -> Result<LaneRegion, LaneDecline> {
    let isa = super::jit_backend::target_isa("speed").ok_or(LaneDecline::Unlowered)?;
    let osr = loop_entry.filter(|_| !unit.frames.is_empty() || allow_first_entry).and_then(|header| lower(program, function_id, unit, helpers, fields, global_reads, direct,
        frame_captures, None, &*isa, Coverage::Osr(header), Completion::StatusAndPc));
    let first_entry = osr.is_none();
    let region = osr.or_else(|| allow_first_entry.then(|| lower(program, function_id, unit, helpers, fields,
        global_reads, direct, frame_captures, loop_entry, &*isa, Coverage::Prefix, Completion::StatusAndPc)).flatten())
        .ok_or(LaneDecline::Unlowered)?;
    let clif = native_dump_directory().map(|_| region.function.to_string());
    let code = compile_native_function(region.function, &*isa, LANE_BODY_MAX_CODE_BYTES, |code| Some(code.to_vec()))
        .map_err(LaneDecline::Rejected)?;
    Ok(LaneRegion { code, native: region.native, clif, fault_poll: region.fault_poll, first_entry })
}

/// Build the typed residual and validate every value/CFG edge before publication.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &BTreeMap<u16, Vec<NativeFieldLoad>>,
    global_reads: &BTreeSet<usize>,
    direct: &BTreeMap<usize, *const DirectCallee>,
    frame_captures: &[(usize, usize)],
    loop_entry: Option<usize>,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    coverage: Coverage,
    completion: Completion,
) -> Option<LoweredRegion> {
    let residual = super::static_loops::specialize(program, unit, coverage.entry()).map(|instructions|
        super::lane_unit::Unit { instructions, origins: unit.origins.clone(), frames: unit.frames.clone(),
            markers: unit.markers.clone(), caller_registers: unit.caller_registers, registers: unit.registers, end: unit.end });
    let unit = residual.as_ref().unwrap_or(unit);
    let function = program.function_get(function_id as usize)?;
    let numeric = completion == Completion::Number;
    if numeric && (!super::super::js_compiled::function_candidate(function, program)
        || !numeric_unit(program, unit) || function.params > unit.caller_registers) { return None; }
    // Source-PC recovery admits each statically bound callee at its call edge and
    // materializes one callee frame (`lane_materialize_inline`).
    if fields.values().flatten().any(|field| !field.valid())
        || (completion == Completion::SourcePc && unit.frames.iter().any(|frame|
            frame.target.callee != CalleeBinding::Function || frame.parent.is_some())) { return None; }
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
    let payload = if numeric { builder.ins().iconst(types::I64, 0) } else { builder.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        context,
        std::mem::offset_of!(CallContext, adapter_payload) as i32,
    ) };
    let fault_poll = super::FAULT_POLL_SUPPORTED && unit.instructions.iter().enumerate().any(|(pc, instruction)|
        matches!(instruction.op().control_flow_layout(), ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump)
            && instruction.jump_target() as usize <= pc);
    let mut seen_headers: CompoundBitSet = CompoundBitSet::with_capacity(unit.instructions.len());
    let mut shared_headers = CompoundBitSet::with_capacity(unit.instructions.len());
    for (pc, instruction) in unit.instructions.iter().enumerate() {
        if instruction.op() != Op::Jump { continue; }
        let target = instruction.jump_target() as usize;
        if target > pc { continue; }
        if seen_headers.contains(target) { shared_headers.insert(target); }
        seen_headers.insert(target);
    }
    let interrupt = super::native_interrupt_pointer(&mut builder, context, fault_poll);
    #[cfg(feature = "aot")]
    let helpers = if let LaneHelpers::Linked(symbol, narrow_table) = helpers {
        let signature = builder.import_signature(native_lane_helpers_signature(isa));
        let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
            super::native_object::NATIVE_FUNCTION_NAMESPACE, symbol));
        let target = builder.import_function(ir::ExtFuncData { name: ir::ExternalName::user(name),
            signature, colocated: false, patchable: false });
        let call = builder.ins().call(target, &[]);
        LaneHelpers::Bound(builder.inst_results(call)[0], narrow_table)
    } else { helpers };
    let count = usize::from(unit.registers);
    let live = if unit.registers > crate::compile::liveness::ROOT_MAP_REGISTERS {
        Liveness::Frames {
            caller: program.register_root_masks(function_id),
            frames: unit.frames.iter().map(|frame| program.register_root_masks(frame.target.function)).collect(),
        }
    } else {
        Liveness::Unit(unit.register_masks(program, function_id))
    };
    let registers = Registers {
        tagged: (0..count).map(|_| builder.declare_var(types::I64)).collect(),
        int: vec![None; count],
        float: vec![None; count],
    };
    // An alternative of a call's switch starts a block: a callee other than the one before
    // it branches there (`Lowering::inline_call`).
    let mut leaders = leaders(&unit.instructions);
    let entry_pc = coverage.entry();
    if !matches!(unit.origins.get(entry_pc), Some(super::lane_unit::Origin::Caller(_))) { return None; }
    leaders.insert(entry_pc);
    for alternative in unit.frames.iter().filter_map(|frame| frame.alternative) {
        if alternative < unit.instructions.len() { leaders.insert(alternative); }
    }
    let blocks: Vec<_> = (0..unit.instructions.len()).map(|pc| leaders.contains(pc).then(|| builder.create_block())).collect();
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
        interrupt,
        fault_poll,
        reentries: smallvec::SmallVec::new(),
        polls: smallvec::SmallVec::new(),
        poll_latches: super::SecondaryMap::new(),
        shared_headers,
        global_reads,
        direct,
        frame_captures,
        context,
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
        backedge: false,
        pending: Vec::new(),
        // A static typed region's calls are guarded leaves (`guarded_call_instruction`).
        native_calls: completion == Completion::StatusAndPc
            && unit.instructions.iter().any(|instruction| instruction.op() == Op::Call),
        facts: super::facts::Facts::default(),
        live,
        valid: true,
        coverage,
        completion,
        number_returns: smallvec::SmallVec::new(),
        hoists: BTreeMap::new(),
        hoisted: BTreeMap::new(),
        loop_fields: Vec::new(),
        loop_stores: BTreeMap::new(),
        at: 0,
        forward: Vec::new(),
        arrived: BTreeMap::new(),
        entered: CompoundBitSet::with_capacity(unit.instructions.len()),
        finishes: false,
        leave,
        stop,
    };
    if matches!(coverage, Coverage::Osr(_)) {
        let (selected, first) = (lowering.builder.create_block(), lowering.builder.create_block());
        let osr = lowering.builder.ins().icmp_imm_u(IntCC::NotEqual, start, 0);
        lowering.builder.ins().brif(osr, selected, &[], first, &[]);
        lowering.builder.switch_to_block(first);
        lowering.return_at(0);
        lowering.builder.switch_to_block(selected);
    }
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
    let own_shape = |pc: usize| {
        let instruction = instructions[pc];
        if instruction.op() != Op::GetField || instruction.result_flags() != 0 { return None; }
        let FieldLookup::Atom { base, cache_site, .. } = instruction.field_lookup()? else { return None };
        let [field] = fields.get(&cache_site)?.as_slice() else { return None };
        field.holder.is_none().then_some((base.register_index()?, field.shape))
    };
    lowering.facts = super::facts::analyze(entry_pc, &initial, &instructions, &leaders, &program.constants, &writes, &own_shape, lowering.native_calls);
    let starts: Vec<usize> = std::iter::once(entry_pc).chain(loop_entry).collect();
    let loops = local_loops(unit, &program.constants, &starts);
    if coverage.selected_entry() {
        lowering.written = CompoundBitSet::with_capacity(count);
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            if !lowering.facts.reached(pc) { continue; }
            for register in writes(instruction) { lowering.written.insert(usize::from(register)); }
        }
        // Backedge definitions choose a numeric entry signature. Conformance
        // proves the external input once; every subsequent edge carries the
        // same verified SSA view. This is an admission requirement, not a fact
        // assumed about the source program or a TypeScript annotation.
        let mut refined = false;
        for (input, carried) in initial.iter_mut().take(usize::from(unit.caller_registers)).zip(lowering.facts.backedge()) {
            if matches!(*carried, Type::INT32 | Type::NUMBER) && *input != *carried {
                *input = *carried;
                refined = true;
            }
        }
        // An integer induction value can compare against an invariant integer
        // input directly. This selects a checked entry contract; a fractional
        // or wider bound resumes the original header with its value unchanged.
        let instruction = instructions[entry_pc];
        if !lowering.facts.backedge().is_empty() && instruction.op() == Op::JumpBinaryFalse
            && matches!(super::operations::NumberRelation::from_operator(instruction.binary_operator_field()),
                    Some(super::operations::NumberRelation::LessThan | super::operations::NumberRelation::LessEqual
                        | super::operations::NumberRelation::GreaterThan | super::operations::NumberRelation::GreaterEqual)) {
            let operands = [instruction.operand_b(), instruction.operand_c()];
            if operands.iter().any(|operand| super::facts::operand_type(&program.constants, *operand,
                |register| lowering.facts.backedge().get(usize::from(register)).copied().unwrap_or(Type::ANY)) == Type::INT32) {
                for register in operands.into_iter().filter_map(Operand::register_index)
                    .filter(|register| *register < unit.caller_registers && !lowering.written.contains(usize::from(*register))) {
                    if initial[usize::from(register)] == Type::NUMBER {
                        initial[usize::from(register)] = Type::INT32;
                        refined = true;
                    }
                }
            }
        }
        if refined {
            lowering.facts = super::facts::analyze(entry_pc, &initial, &instructions, &leaders, &program.constants, &writes, &own_shape, lowering.native_calls);
        }
        lowering.refine_inductive_inputs(&mut initial, &instructions, &leaders, &loops, &own_shape);
    }
    lowering.require_object_latches(&instructions, &leaders, &loops);
    if !numeric { lowering.entry = Some(lowering.entry_facts(&instructions, start, loop_entry, &loops, &leaders)); }
    // An entry at a loop header (`loop_entry`, the entry's `start` word nonzero) starts
    // from the registers live there, each proved to its header facts or leaving at it.
    lowering.hoists = invariant_reads(unit, &loops);
    lowering.loop_fields = loop_fields(program, unit, &loops);
    for field in &lowering.loop_fields {
        lowering.hoists.entry(field.header).or_default().push(InvariantRead { pc: field.reads[0], owner: Some(field.owner) });
    }
    // Field admissions establish the actual counterpart types used to select
    // capture views. Callee reads/bindings must precede their inlined captures.
    for reads in lowering.hoists.values_mut() {
        reads.sort_by_key(|read| (instructions[read.pc].op() == Op::LoadCapture,
            matches!(unit.origins[read.pc], super::lane_unit::Origin::Inlined { .. })));
    }
    lowering.forward = forward_edges(&instructions, &leaders, &starts);
    for start in &starts { lowering.entered.insert(*start); }
    let mut start_blocks: Vec<ir::Block> = starts.iter().map(|_| lowering.builder.create_block()).collect();
    if let [at_first, at_header] = start_blocks[..] {
        let from_header = lowering.builder.ins().icmp_imm_u(IntCC::NotEqual, start, 0);
        lowering.builder.ins().brif(from_header, at_header, &[], at_first, &[]);
    } else {
        lowering.builder.ins().jump(start_blocks[0], &[]);
    }
    for (pc, block) in starts.iter().copied().zip(start_blocks.drain(..)) {
        lowering.builder.switch_to_block(block);
        if !matches!(lowering.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_))) {
            return None;
        }
        let undefined = lowering.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
        for register in 0..count {
            let bits = if !numeric && register < usize::from(lowering.unit.caller_registers)
                && lowering.is_live(pc, register as Register) {
                lowering.builder.ins().load(types::I64, MemFlagsData::trusted(), registers_base, register_offset(register as Register))
            } else {
                undefined
            };
            lowering.builder.def_var(lowering.registers.tagged[register], bits);
        }
        let target = lowering.blocks.get(pc).copied().flatten()?;
        let mut state = State::canonical(count);
        if numeric {
            for register in 0..function.params {
                let number = lowering.builder.ins().load(types::F64, MemFlagsData::trusted(),
                    registers_base, register_offset(register));
                let variable = lowering.float_var(usize::from(register));
                lowering.builder.def_var(variable, number);
                state.values[usize::from(register)] = RegisterFact::admit(Fact {
                    ty: Type::NUMBER, representation: Representation::Float64,
                    binding: BindingTime::Location, proof: Proof::EntryGuard, projection: Projection::Exact,
                }).expect("Number input signature is verified by its entry caller");
            }
        }
        if fault_poll {
            // Publication lends a resumable page for this entire no-GC entry.
            // If unavailable, leave before any guest effect through ordinary recovery.
            let miss = lowering.guard_exit(&state, pc as u32);
            let available = lowering.builder.ins().icmp_imm_u(IntCC::NotEqual, interrupt, 0);
            emit_js_guard(&mut lowering.builder, available, miss);
        }
        lowering.conform(&mut state, pc);
        // A region whose sole entry is this loop header has no earlier source
        // edge on which to hoist its invariant reads. Admit them on the native
        // entry edge, before the header and every possible loop effect.
        lowering.hoist_reads(&mut state, pc);
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
                lowering.entered.insert(pc);
                lowering.builder.ins().jump(block, &[]);
            }
            // Marker alternatives add real native edges beyond the bytecode
            // analysis's Nop fallthrough. Missing type facts select canonical
            // guarded lowering; they do not make an entered block unreachable.
            if !lowering.entered.contains(pc) {
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
    for (header, latch) in std::mem::take(&mut lowering.poll_latches).iter() {
        if let Some(latch) = latch {
            lowering.builder.switch_to_block(*latch);
            lowering.emit_fault_poll();
            lowering.builder.ins().jump(header, &[]);
        }
    }
    if matches!(coverage, Coverage::PrefixFrom(_) | Coverage::Osr(_)) && !lowering.backedge { return None; }
    for (block, mut leaving, pc) in std::mem::take(&mut lowering.pending) {
        lowering.builder.switch_to_block(block);
        lowering.exit(&mut leaving, pc);
    }
    // The exit blocks: write back every caller register the unit writes, and an inlined
    // callee's registers to the window its recovery makes the activation from, then leave
    // at the origin.
    let snapshot = (lowering.completion == Completion::SourcePc).then(||
        unit.frames.iter().filter(|frame| frame.recovery == super::lane_unit::CallRecovery::Materialize)
            .map(|frame| frame.registers).max()).flatten().map(|registers| {
        let word = types::I64.bytes();
        lowering.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot, u32::from(registers) * word,
            word.trailing_zeros() as u8))
    });
    for pc in 0..lowering.exits.len() {
        let Some(block) = lowering.exits[pc] else { continue };
        lowering.builder.switch_to_block(block);
        for register in lowering.written.iter().map(|register| register as Register)
            .take_while(|register| *register < unit.caller_registers) {
            if !lowering.is_live(pc, register) { continue; }
            let bits = lowering.builder.use_var(lowering.registers.tagged[usize::from(register)]);
            lowering.builder.ins().store(MemFlagsData::trusted(), bits, lowering.registers_base, register_offset(register));
        }
        let word = match unit.origins.get(pc).copied().unwrap_or(super::lane_unit::Origin::Caller(unit.end)) {
            super::lane_unit::Origin::Caller(record) => u64::from(record),
            super::lane_unit::Origin::Inlined { frame, pc: record } if lowering.completion == Completion::SourcePc => {
                let inlined = unit.frames[frame];
                let scratch = lowering.builder.ins().stack_addr(types::I64, snapshot.expect("materialized callee snapshot"), 0);
                for register in 0..inlined.registers {
                    let bits = if lowering.is_live(pc, inlined.base + register) {
                        lowering.builder.use_var(lowering.registers.tagged[usize::from(inlined.base + register)])
                    } else {
                        lowering.builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64)
                    };
                    lowering.builder.ins().store(MemFlagsData::trusted(), bits, scratch, register_offset(register));
                }
                let function = lowering.builder.ins().iconst(types::I64, i64::from(inlined.target.function));
                let call = lowering.builder.ins().iconst(types::I64, i64::from(inlined.call));
                let source = lowering.builder.ins().iconst(types::I64, i64::from(record));
                let helper = lowering.helpers.address(lowering.builder.ins(), LaneHelper::MaterializeInline);
                emit_lane_helper_call(&mut lowering.builder, lowering.payload, helper,
                    &[lowering.payload, function, call, source, scratch]);
                lowering.return_at(u64::from(JS_INLINE_MATERIALIZED_PC));
                continue;
            }
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
        lowering.return_at(word);
    }
    lowering.builder.switch_to_block(stop);
    let word = lowering.builder.block_params(stop)[0];
    lowering.builder.ins().store(MemFlagsData::trusted(), word, lowering.result, 0);
    let status = lowering.builder.ins().iconst(types::I32, i64::from(NATIVE_JS_STOPPED));
    lowering.builder.ins().jump(leave, &[status.into()]);
    lowering.builder.switch_to_block(leave);
    let status = lowering.builder.block_params(leave)[0];
    lowering.builder.ins().return_(&[status]);
    if !lowering.valid || (completion == Completion::StatusAndPc && !lowering.finishes) { return None; }
    let number_result = (completion == Completion::Number).then(|| lowering.finish_number_returns());
    let native_count = lowering.native;
    let mut polls = std::mem::take(&mut lowering.polls);
    let reentries = std::mem::take(&mut lowering.reentries);
    let helpers = lowering.helpers;
    lowering.builder.seal_all_blocks();
    lowering.builder.finalize(isa.frontend_config());
    if fault_poll && !polls.is_empty() {
        retain_closed_polls(&native, &reentries, &mut polls);
        if !polls.is_empty() { specialize_interrupt_owner(&mut native, helpers, context, &polls, isa); }
    }
    Some(LoweredRegion { function: native, native: native_count, fault_poll, number_result })
}

/// A callback retires ownership only on paths reachable from its completion. A callback
/// after a closed loop cannot invalidate that loop's entry proof retroactively.
fn retain_closed_polls(function: &ir::Function, reentries: &[ir::Block], polls: &mut smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>) {
    if reentries.is_empty() { return; }
    let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(function);
    let mut possible: CompoundBitSet = CompoundBitSet::with_capacity(function.dfg.num_blocks());
    let mut pending = smallvec::SmallVec::<[ir::Block; INLINE_EFFECT_EDGES]>::new();
    for &block in reentries {
        if possible.insert(block.index()) { pending.push(block); }
    }
    while let Some(block) = pending.pop() {
        for successor in cfg.succ_iter(block) {
            if possible.insert(successor.index()) { pending.push(successor); }
        }
    }
    polls.retain(|poll| {
        let block = function.layout.inst_block(*poll).expect("published poll block");
        let effect = if possible.contains(block.index()) { Reentry::Possible } else { Reentry::Closed };
        effect == Reentry::Closed
    });
}

/// Bind the current owner's proof once, after helper binding and before guest execution.
/// Each selected poll lies before any reachable callback completion.
fn specialize_interrupt_owner(function: &mut ir::Function, helpers: LaneHelpers, context: ir::Value,
    polls: &[ir::Inst], isa: &dyn cranelift_codegen::isa::TargetIsa)
{
    use cranelift_codegen::cursor::{Cursor, FuncCursor};
    let entry = function.layout.entry_block().expect("native entry");
    let after = match helpers {
        #[cfg(feature = "aot")]
        LaneHelpers::Bound(table, _) => match function.dfg.value_def(table) {
            ir::ValueDef::Result(inst, _) => Some(inst), _ => unreachable!("bound helper table"),
        },
        LaneHelpers::Numeric | LaneHelpers::Process(_) => None,
        #[cfg(feature = "aot")]
        LaneHelpers::Linked(..) => unreachable!("helpers bind before lowering"),
    };
    let mut cursor = FuncCursor::new(function);
    if let Some(inst) = after { cursor.goto_inst(inst); cursor.next_inst(); }
    else { cursor.goto_inst(cursor.func.layout.first_inst(entry).expect("entry instruction")); }
    let address = helpers.address(cursor.ins(), LaneHelper::InterruptOwner);
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(ir::AbiParam::new(types::I64));
    signature.returns.push(ir::AbiParam::new(types::I64));
    let signature = cursor.func.import_signature(signature);
    let call = cursor.ins().call_indirect(signature, address, &[context]);
    let exclusive = cursor.func.dfg.inst_results(call)[0];
    let mut gates = Vec::with_capacity(polls.len());
    for &poll in polls {
        let before = cursor.func.layout.inst_block(poll).expect("poll block");
        let polled = cursor.func.dfg.make_block();
        cursor.func.layout.split_block(polled, poll);
        let next = cursor.func.layout.next_inst(poll).expect("poll continuation");
        let resume = cursor.func.dfg.make_block();
        cursor.func.layout.split_block(resume, next);
        cursor.goto_bottom(before); let gate = cursor.ins().brif(exclusive, resume, &[], polled, &[]);
        gates.push((gate, false));
        cursor.goto_bottom(polled); cursor.ins().jump(resume, &[]);
    }
    cursor.func.effect_specializations.push(ir::function::EffectSpecialization { proof: call, gates });
}

impl Lowering<'_, '_> {
    /// A bitwise operand's ToInt32: an int32 as it is, a number by truncation modulo 2^32. A
    /// double of magnitude 2^63 or more, an infinity or NaN leaves (`EXACT_I64_TRUNCATION_LIMIT`).
    fn operand_to_int32(&mut self, state: &mut State, operand: Operand, exit: ir::Block) -> Option<ir::Value> {
        let integral = match operand.kind()? {
            OperandKind::Register => state.values[usize::from(operand.payload())].value.ty.is_subtype(Type::INT32),
            OperandKind::Constant => self.program.constants.get(usize::from(operand.payload()))
                .and_then(super::facts::immediate).is_some_and(super::facts::is_int32),
            OperandKind::Field | OperandKind::Local => return None,
        };
        if integral {
            return self.operand_int(state, operand, exit);
        }
        // A register only its tag types is its int where the tag says so, else its Number's
        // ToInt32; neither path proves a fact the other does not.
        let tagged = operand.kind() == Some(OperandKind::Register)
            && state.values[usize::from(operand.payload())].value.representation == Representation::Tagged;
        if !tagged {
            let number = self.operand_number(state, operand, exit)?;
            return Some(self.number_to_int32(number, exit));
        }
        let bits = self.operand_tagged(state, operand)?;
        if let Some((_, int32)) = state.proofs.int32_projections.iter().find(|(source, _)| *source == bits) {
            #[cfg(test)] INT32_PROJECTION_REUSES.with(|count| count.set(count.get() + 1));
            return Some(*int32);
        }
        let integer = is_int_tagged(&mut self.builder, bits);
        let (int_block, number_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        let int32 = self.builder.append_block_param(join, types::I32);
        self.ins().brif(integer, int_block, &[], number_block, &[]);
        self.builder.switch_to_block(int_block);
        let int = self.ins().ireduce(types::I32, bits);
        self.ins().jump(join, &[ir::BlockArg::Value(int)]);
        self.builder.switch_to_block(number_block);
        let number = emit_js_tagged_number(&mut self.builder, bits, exit);
        let truncated = self.number_to_int32(number, exit);
        self.ins().jump(join, &[ir::BlockArg::Value(truncated)]);
        self.builder.switch_to_block(join);
        state.proofs.int32_projections.push((bits, int32));
        Some(int32)
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

    /// `state` past a join with `incoming`: each register's facts as both paths prove them.
    /// A join no verified fact describes invalidates the region.
    fn join_states(&mut self, state: &mut State, incoming: &State) {
        // Only the same pre-branch SSA definition can survive both numeric arms.
        state.proofs.fields.retain(|field| incoming.proofs.fields.contains(field));
        state.proofs.int32_projections.retain(|projection| incoming.proofs.int32_projections.contains(projection));
        for (index, incoming) in incoming.values.iter().copied().enumerate() {
            let current = state.values[index];
            if current != incoming {
                let representation = if current.value.representation == incoming.value.representation {
                    current.value.representation
                } else {
                    self.valid &= current.tagged.current() && incoming.tagged.current();
                    Representation::Tagged
                };
                let Some(mut fact) = RegisterFact::admit(Fact {
                    ty: current.value.ty.join(incoming.value.ty), representation,
                    binding: BindingTime::Dynamic, proof: Proof::Definition,
                    projection: Projection::Exact,
                }) else { self.valid = false; return; };
                fact.tagged = if current.tagged == incoming.tagged { current.tagged }
                    else if current.tagged.current() && incoming.tagged.current() { TaggedView::Variable }
                    else { TaggedView::Unavailable };
                state.values[index] = fact;
            }
        }
    }

    /// The ToInt32 of `number`, natively below 2^63 in magnitude; beyond, infinities and NaN
    /// leave through `exit` (`EXACT_I64_TRUNCATION_LIMIT`).
    fn number_to_int32(&mut self, number: ir::Value, exit: ir::Block) -> ir::Value {
        let magnitude = self.ins().fabs(number);
        let bound = self.ins().f64const(EXACT_I64_TRUNCATION_LIMIT);
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

    /// Leaves with `word`: a lane body stops with it as its result word (`stop`); a static
    /// typed region returns it as the source PC its canonical code continues at.
    fn return_at(&mut self, word: u64) {
        match self.completion {
            Completion::StatusAndPc => {
                let word = self.builder.ins().iconst(types::I64, word as i64);
                let stop = self.stop;
                self.builder.ins().jump(stop, &[word.into()]);
            }
            Completion::Number => {
                let miss = self.builder.ins().iconst(types::I32, NUMBER_ENTRY_MISS);
                self.builder.ins().return_(&[miss]);
            }
            // Each exit returns its own constant, which a caller that inlines the region
            // folds into a direct continuation.
            Completion::SourcePc => {
                let pc = self.builder.ins().iconst(types::I32,
                    i64::from(u32::try_from(word).expect("source PC has no inlined-frame tag")));
                self.builder.ins().return_(&[pc]);
            }
        }
    }

    fn return_number(&mut self, state: &mut State, register: Register, pc: u32) {
        self.finishes = true;
        if !state.values[usize::from(register)].value.ty.is_subtype(Type::NUMBER) {
            self.valid = false;
            self.return_at(0);
            return;
        }
        self.interrupt_check(state, pc);
        let exit = self.guard_exit(state, pc);
        let index = usize::from(register);
        let fact = state.values[index].value;
        let value = if fact.ty.is_subtype(Type::INT32) || fact.representation == Representation::Int32 {
            self.int(state, register, exit)
        } else {
            self.number(state, register, exit)
        };
        let fact = state.values[index].value;
        let block = self.builder.create_block();
        let machine = match fact.representation {
            Representation::Int32 => types::I32,
            Representation::Float64 => types::F64,
            Representation::Tagged => unreachable!("Number return has an unboxed view"),
        };
        self.builder.append_block_param(block, machine);
        self.ins().jump(block, &[value.into()]);
        self.number_returns.push(NumberReturn { block, fact });
    }

    /// The semantic join selects one ABI format for every successful return.
    /// Guards and termination leave without publishing a result.
    fn finish_number_returns(&mut self) -> NativeNumberResult {
        let ty = self.number_returns.iter().fold(Type::NEVER, |ty, returned| ty.join(returned.fact.ty));
        let format = if ty != Type::NEVER && ty.is_subtype(Type::INT32) {
            NativeNumberResult::Int32
        } else { NativeNumberResult::Float64 };
        for returned in std::mem::take(&mut self.number_returns) {
            self.builder.switch_to_block(returned.block);
            let value = self.builder.block_params(returned.block)[0];
            let value = match (format, returned.fact.representation) {
                (NativeNumberResult::Int32, representation) => {
                    let integer = match representation {
                        Representation::Int32 => value,
                        Representation::Float64 => self.ins().fcvt_to_sint_sat(types::I32, value),
                        Representation::Tagged => unreachable!("typed numeric return"),
                    };
                    self.ins().uextend(types::I64, integer)
                }
                (NativeNumberResult::Float64, Representation::Int32) => self.ins().fcvt_from_sint(types::F64, value),
                (NativeNumberResult::Float64, Representation::Float64) => value,
                (_, Representation::Tagged) => unreachable!("typed numeric return"),
            };
            let result = self.result;
            self.ins().store(MemFlagsData::trusted(), value, result, 0);
            let success = self.ins().iconst(types::I32, 0);
            self.ins().return_(&[success]);
        }
        format
    }

    /// The facts every run proves once at entry, before either start (`lower`'s loop entry).
    fn entry_facts(&mut self, instructions: &[WideInstruction], start: ir::Value,
        loop_entry: Option<usize>, _loops: &[(usize, usize)], _leaders: &CompoundBitSet) -> Entry {
        let this_bits = match self.helpers {
            LaneHelpers::Numeric => unreachable!("Number signatures have no receiver"),
            LaneHelpers::Process(_) => emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, receiver)),
            #[cfg(feature = "aot")]
            LaneHelpers::Bound(..) => {
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
            LaneHelpers::Linked(..) => unreachable!("helper table bound before entry facts"),
        };
        let receiver = emit_lane_receiver(&mut self.builder, self.payload, this_bits);
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
            if let std::collections::btree_map::Entry::Vacant(slot) = store_atoms.entry(atom) {
                let admitted = self.emit_store_atom_admitted(atom);
                let variable = self.builder.declare_var(types::I8);
                self.builder.def_var(variable, admitted);
                slot.insert(variable);
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
        let mut miss = None;
        let object_table = instructions.iter().enumerate().any(|(pc, instruction)|
            self.facts.reached(pc) && matches!(instruction.op(),
                Op::GetField | Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict))
            .then(|| {
                let base = emit_js_payload_word(&mut self.builder, self.payload,
                    std::mem::offset_of!(JsStepDispatch, object_record_base));
                let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                CheckedObjectTable::prove(&mut self.builder, base, exit)
            });
        let mut calls = CompoundBitSet::with_capacity(self.unit.frames.len());
        let mut call_receivers = CompoundBitSet::with_capacity(self.unit.frames.len());
        if self.completion != Completion::Number {
            // Roots name canonical caller homes never written by this unit.
            // Local Move aliases are valid only within their basic block.
            let roots: Vec<_> = (0..self.unit.registers).map(|register|
                (register < self.unit.caller_registers && !self.written.contains(usize::from(register)))
                    .then_some(register)).collect();
            let mut aliases = roots.clone();
            let mut admitted = smallvec::SmallVec::<[(Register, super::lane_unit::CallTarget); 2]>::new();
            let mut receivers = smallvec::SmallVec::<[(Register, ReceiverRecovery); 2]>::new();
            for (pc, instruction) in instructions.iter().copied().enumerate() {
                if self.blocks[pc].is_some() { aliases.copy_from_slice(&roots); }
                if !self.facts.reached(pc) { continue; }
                if let Some(frame) = self.unit.markers.iter().position(|marker| *marker == pc) {
                    let call = self.unit.frames[frame];
                    let body = self.program.function(call.target.function as usize);
                    let numeric = restartable_numeric_function(self.program, body, &body.instructions().collect::<Vec<_>>());
                    if !(call.recovery == super::lane_unit::CallRecovery::RestartCall && numeric)
                        && let Some(root) = aliases[usize::from(call.receiver)] {
                        let recovery = ReceiverRecovery::for_body(body);
                        if let Some((_, required)) = receivers.iter_mut().find(|(owner, _)| *owner == root) {
                            if recovery == ReceiverRecovery::Object { *required = recovery; }
                        } else { receivers.push((root, recovery)); }
                        call_receivers.insert(frame);
                    }
                    if call.alternative.is_none()
                        && numeric
                        && let Some(root) = aliases[usize::from(call.callee_register)] {
                        let binding = (root, call.target);
                        if !admitted.contains(&binding) {
                            let callee = self.builder.ins().load(types::I64, MemFlagsData::trusted(),
                                self.registers_base, register_offset(root));
                            let matched = match call.target.callee {
                                CalleeBinding::Function => self.function_binding(callee, call.target.function),
                                CalleeBinding::Identity(identity) => self.builder.ins().icmp_imm_u(IntCC::Equal, callee, identity.to_bits() as i64),
                            };
                            let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                            emit_js_guard(&mut self.builder, matched, exit);
                            admitted.push(binding);
                        }
                        calls.insert(frame);
                    }
                }
                let root = (instruction.op() == Op::Move)
                    .then(|| aliases[usize::from(instruction.register_b())]).flatten();
                for register in writes(instruction) { aliases[usize::from(register)] = root; }
            }
            for (root, recovery) in receivers {
                let receiver = self.builder.ins().load(types::I64, MemFlagsData::trusted(),
                    self.registers_base, register_offset(root));
                let matched = self.receiver_recovery(receiver, recovery);
                let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                emit_js_guard(&mut self.builder, matched, exit);
            }
        }
        #[cfg(feature = "aot")]
        let fields = {
            let mut fields = smallvec::SmallVec::new();
            if matches!(self.helpers, LaneHelpers::Bound(..)) {
                let mut uses = smallvec::SmallVec::<[(u16, bool, Type); INLINE_FIELD_BINDINGS]>::new();
                for (pc, instruction) in instructions.iter().enumerate() {
                    if !self.facts.reached(pc) { continue; }
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
                    let repeated = _loops.iter().any(|&(header, back)| (header..=back).contains(&pc));
                    for site in sites {
                        let writes = if matches!(instruction.op(), Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict) {
                            self.facts.before(pc, instruction.register_a(), instructions, _leaders,
                                &self.program.constants, &writes, self.native_calls)
                        } else if instruction.writes_current_this()
                            && matches!(instruction.field_lookup(), Some(FieldLookup::Site(index))
                                if self.program.field_sites[index].sink.is_some_and(|(_, sink)| sink == site)) {
                            Type::ANY
                        } else { Type::NEVER };
                        if let Some((_, reused, domain)) = uses.iter_mut().find(|(known, _, _)| *known == site) {
                            *reused |= repeated;
                            *domain = domain.join(writes);
                        } else { uses.push((site, repeated, writes)); }
                    }
                }
                for (site, repeated, writes) in uses {
                    let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                    let mut binding = emit_js_field_binding(&mut self.builder, self.payload, site, exit, object_table);
                    let slot = binding.slot.read(&mut self.builder);
                    binding.slot = FieldSlot::Bound(slot);
                    if writes != Type::NEVER { self.admit_field_writes(&mut binding, writes, exit); }
                    // The extra entry arithmetic pays for repeated receiver loads.
                    // A one-off access keeps its cheaper storage-plus-slot form.
                    if repeated {
                        binding.arena = binding.arena.bind(&mut self.builder, slot);
                    }
                    fields.push((site, binding));
                }
            }
            fields
        };
        #[cfg(not(feature = "aot"))]
        let fields = smallvec::SmallVec::new();
        // Profiled raw stores preserve their declared field type. Register those possible
        // writes once per physical slot, independently of cache-row binding availability.
        let mut domains = BTreeMap::<(u32, u16), Type>::new();
        for instruction in instructions {
            if !matches!(instruction.op(), Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict) { continue; }
            if let Some(fields) = self.fields.get(&instruction.cache_site_index())
                && let [field] = fields.as_slice()
                && field.ty != Type::NEVER {
                let domain = domains.entry((field.shape, field.slot)).or_insert(Type::NEVER);
                *domain = domain.join(field.ty);
            }
        }
        for ((shape, slot), domain) in domains {
            let exit = *miss.get_or_insert_with(|| self.builder.create_block());
            let shape = self.builder.ins().iconst(types::I64, i64::from(shape));
            let slot = self.builder.ins().iconst(types::I64, i64::from(slot));
            self.admit_slot_writes(shape, slot, domain, exit);
        }
        let records = self.invariant_field_records(instructions, &receiver, object_table, &mut miss);
        if let Some(miss) = miss {
            let admitted = self.builder.create_block();
            self.builder.ins().jump(admitted, &[]);
            self.builder.switch_to_block(miss);
            self.builder.set_cold_block(miss);
            // No guest operation has run and canonical registers are untouched.
            // Decline the whole run once, at its start, even for a speculative unused binding.
            let origin = |pc: usize| match self.unit.origins[pc] {
                super::lane_unit::Origin::Caller(pc) => u64::from(pc),
                super::lane_unit::Origin::Inlined { .. } => unreachable!("a run starts in the caller"),
            };
            let first = origin(self.coverage.entry());
            match loop_entry {
                None => self.return_at(first),
                Some(header) => {
                    let header = origin(header);
                    let (at_first, at_header) = (self.builder.create_block(), self.builder.create_block());
                    let from_header = self.builder.ins().icmp_imm_u(IntCC::NotEqual, start, 0);
                    self.builder.ins().brif(from_header, at_header, &[], at_first, &[]);
                    self.builder.switch_to_block(at_first);
                    self.return_at(first);
                    self.builder.switch_to_block(at_header);
                    self.return_at(header);
                }
            }
            self.builder.switch_to_block(admitted);
        }
        Entry { receiver, object_table, records, receiver_shapes, store_atoms, capture_values, capture_len, this, fields, calls, call_receivers }
    }

    /// Bind physical receiver facts once. The expanded unit's write set owns
    /// invariance, including renamed callee registers and synthesized returns.
    fn invariant_field_records(&mut self, instructions: &[WideInstruction], receiver: &LaneReceiver,
        object_table: Option<CheckedObjectTable>, miss: &mut Option<ir::Block>) -> Vec<Option<ObjectRecord>> {
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
                ObjectRecord::Address(record)
            } else {
                let records = match object_table {
                    Some(table) => ObjectTable::Checked(table),
                    None => ObjectTable::Unchecked(*records.get_or_insert_with(|| emit_js_payload_word(&mut self.builder,
                        self.payload, std::mem::offset_of!(JsStepDispatch, object_record_base)))),
                };
                // An unwritten register holds its entry value at either start, in the file.
                let base = self.registers_base;
                let bits = self.ins().load(types::I64, MemFlagsData::trusted(), base, register_offset(owner as Register));
                ObjectRecord::prove(&mut self.builder, bits, records, None, exit)
            });
        }
        admitted
    }

    /// Admit once at the edge; later raw stores consume the compile-time domain proof.
    fn admit_field_writes(&mut self, binding: &mut FieldSlotBinding, writes: Type, exit: ir::Block) {
        if binding.writes.is_some_and(|domain| writes.is_subtype(domain)) { return; }
        let shape = self.builder.ins().uextend(types::I64, binding.receiver);
        let slot = binding.slot.read(&mut self.builder);
        let domain = self.admit_slot_writes(shape, slot, writes, exit);
        binding.writes = Some(binding.writes.map_or(domain, |held| held.join(domain)));
    }

    fn admit_slot_writes(&mut self, shape: ir::Value, slot: ir::Value, writes: Type, exit: ir::Block) -> Type {
        let domain = crate::typed_ir::SlotType::from_type(writes);
        let encoded = self.builder.ins().iconst(types::I64, domain as i64);
        let helper = self.helpers.address(self.builder.ins(), LaneHelper::FieldWriteAdmission);
        let admitted = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[shape, slot, encoded]);
        let admitted = self.builder.ins().icmp_imm_u(IntCC::NotEqual, admitted, 0);
        emit_js_guard(&mut self.builder, admitted, exit);
        domain.ty()
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
        if self.consumed.contains(pc) {
            return true;
        }
        if let Some(LoopRead { value: variable, ty, representation, receiver, header }) = self.hoisted.get(&pc).copied() {
            if let Some((owner, ty)) = receiver {
                let owner = match instruction.field_lookup() {
                    Some(FieldLookup::Atom { base, .. }) => base.register_index().unwrap_or(owner),
                    _ => owner,
                };
                let fact = &mut state.values[usize::from(owner)];
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(ty, Representation::Tagged) };
                if let Some(header) = header { state.proofs.headers[usize::from(owner)] = Some(header); }
            }
            let raw = self.builder.use_var(variable);
            if representation == Representation::Int32 && instruction.op() == Op::LoadCapture {
                // Captures have no tagged field-cache consumer. Preserve the
                // admitted view; recovery or a tagged use boxes it on demand.
                self.set_int(state, instruction.result_register(), raw);
                self.native += 1;
                return true;
            }
            let value = if representation == Representation::Int32 { int_tagged(&mut self.builder, raw) } else { raw };
            let destination = instruction.result_register();
            if instruction.op() == Op::GetField
                && let Some(FieldLookup::Atom { atom, base, cache_site }) = instruction.field_lookup() {
                #[cfg(test)] {
                    record_field_forward(cache_site);
                    if representation == Representation::Int32 {
                        LOOP_INT32_READ_SITE.with(|site| {
                            if let Some((expected, count)) = site.get() && expected == cache_site {
                                site.set(Some((expected, count + 1)));
                            }
                        });
                    }
                }
                #[cfg(not(test))] let _ = cache_site;
                state.proofs.fields.push(FieldValue { owner: base.register_index(), atom, value, ty });
            }
            self.define(state, destination, value, RegisterFact::definition(ty, Representation::Tagged));
            if ty == Type::INT32 {
                let int = if representation == Representation::Int32 { raw } else { self.ins().ireduce(types::I32, value) };
                let variable = self.int_var(usize::from(destination));
                self.builder.def_var(variable, int);
                state.values[usize::from(destination)] = RegisterFact { tagged: TaggedView::Value(value),
                    ..RegisterFact::definition(Type::INT32, Representation::Int32) };
            }
            self.native += 1;
            return true;
        }
        let instructions = &self.unit.instructions;
        let pc32 = pc as u32;
        let plain = instruction.result_flags() == 0;
        let ran = match instruction.op() {
            // A static typed region's canonical code completes its activation.
            Op::Return if plain && self.completion == Completion::StatusAndPc && instruction.register_a() < self.unit.registers => {
                Some(self.complete(state, pc, instruction.register_a()))
            }
            Op::Call if self.completion == Completion::StatusAndPc
                && (plain || instruction.result_flags() == crate::bytecode::RETURN_REGISTER)
                && !instruction.direct_eval()
                && !instruction.parameter_eval() => Some(self.call(state, pc, instruction)),
            Op::Call if self.completion == Completion::SourcePc && guarded_call_instruction(instruction) => {
                let exit = self.guard_exit(state, pc32);
                let callee = self.tagged(state, instruction.register_b());
                let receiver = self.tagged(state, instruction.register_c());
                let helper = self.helpers.address(self.builder.ins(), LaneHelper::GuardedCall);
                let program = self.builder.ins().load(types::I32, MemFlagsData::trusted(), self.payload,
                    std::mem::offset_of!(JsStepDispatch, program_id) as i32);
                let program = self.builder.ins().uextend(types::I64, program);
                let result = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[program, callee, receiver]);
                let hit = self.ins().icmp_imm_u(IntCC::NotEqual, result, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, hit, exit);
                state.call_effect();
                self.set_tagged(state, instruction.result_register(), result);
                Some(true)
            }
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
                let global_get = self.helpers.address(self.builder.ins(), LaneHelper::GlobalGet);
                let value = emit_lane_helper_call(&mut self.builder, self.payload, global_get, &[atom, site]);
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
            Op::LoadThis if plain && !instruction.writes_current_this() => {
                let exit = self.guard_exit(state, pc32);
                let entry = self.entry.as_ref().expect("entry facts");
                let this = self.builder.use_var(entry.this);
                let bound = self.ins().icmp_imm_u(IntCC::NotEqual, this, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, bound, exit);
                self.set_tagged(state, instruction.result_register(), this);
                state.proofs.headers[usize::from(instruction.result_register())] = state.proofs.headers[state.values.len()];
                state.proofs.receivers.insert(instruction.result_register());
                Some(true)
            }
            Op::LoadCapture if plain && crate::bytecode::ImmediateLayout::capture_depth(instruction.imm()) == 0 => {
                let exit = self.guard_exit(state, pc as u32);
                let value = self.load_capture(state, pc, instruction, exit, None);
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
                state.proofs.fields.clear();
                // The lane's in-place element replacement; a miss leaves at the store.
                let exit = self.guard_exit(state, pc32);
                let value = self.tagged(state, instruction.register_a());
                let base = self.tagged(state, instruction.register_b());
                let key = self.tagged(state, instruction.register_c());
                let helper = self.helpers.address(self.builder.ins(), LaneHelper::IndexSet);
                let stored = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[base, key, value]);
                let hit = self.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
                emit_js_guard(&mut self.builder, hit, exit);
                Some(true)
            }
            Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict if plain => self.set_field(state, pc, instruction),
            op if op.js_denotation() == JsDenotation::NumericBinary && instruction.numeric_local_target().is_none() => {
                self.binary(state, pc, instruction)
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
                    if decrement || !self.bounded_increment(pc, instruction.register_b(), instruction.result_register()) {
                        emit_js_guard_not(&mut self.builder, overflow, exit);
                    }
                    self.set_int(state, instruction.result_register(), result);
                    true
                })
            }
            Op::Jump if plain => {
                let target = instruction.jump_target() as usize;
                if target <= pc {
                    self.backedge |= target == self.coverage.entry();
                    if self.shared_backedge(state, target) { return false; }
                    self.interrupt_check(state, pc32);
                    if let Some(header) = instructions.get(target).copied()
                        && matches!(header.op(), Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse)
                        && header.result_flags() == 0
                        && header.jump_target() as usize > pc
                    {
                        // Rotate only the pure header test. The first visit
                        // keeps the zero-trip guard; each backedge keeps its
                        // poll and exact source-header recovery state.
                        return self.instruction(state, target, header);
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
                    let target = instruction.jump_target() as usize;
                    if let Some(taken) = comparison.known_outcome(ty) {
                        let taken = self.ins().iconst(types::I8, i64::from(taken));
                        if target <= pc { self.interrupt_check(state, pc32); }
                        self.branch(state, pc, target, taken);
                    } else if target > pc {
                        let exit = self.guard_exit(state, pc32);
                        let value = self.tagged(state, comparison.source);
                        let taken = self.builder.create_block();
                        let fall = self.builder.create_block();
                        let unequal = if comparison.negated { pc + 1 } else { target };
                        let identity = self.builder.func.dfg.resolve_aliases(value);
                        let direct = (!comparison.strict && self.facts.at(unequal)
                            .is_some_and(|types| types.iter().enumerate().any(|(index, ty)|
                                *ty == Type::DIRECT_OBJECT_FAMILY && self.is_live(unequal, index as Register)
                                && matches!(state.values[index].tagged, TaggedView::Value(alias)
                                    if self.builder.func.dfg.resolve_aliases(alias) == identity))))
                            .then(|| self.builder.create_block());
                        emit_nullish_edges(&mut self.builder, value, comparison, exit, taken, fall, direct);
                        if let Some(direct) = direct {
                            self.builder.switch_to_block(direct);
                            let mut direct_state = state.clone();
                            self.refine_object_family(&mut direct_state, value);
                            self.continue_at(&mut direct_state, unequal);
                            // The other unequal arm has already excluded this family.
                            // Resume before the latch's moves, with the original homes.
                            self.builder.switch_to_block(if comparison.negated { fall } else { taken });
                            self.exit(state, unequal as u32);
                            self.builder.switch_to_block(if comparison.negated { taken } else { fall });
                            self.continue_at(&mut state.clone(), if comparison.negated { target } else { pc + 1 });
                        } else {
                            self.continue_branch(state, pc, target, taken, fall);
                        }
                    } else {
                        let exit = self.guard_exit(state, pc32);
                        let value = self.tagged(state, comparison.source);
                        let taken = emit_nullish_branch(&mut self.builder, value, comparison, exit);
                        self.interrupt_check(state, pc32);
                        self.branch(state, pc, target, taken);
                    }
                    return false;
                }
                if let Some(operation) = super::operations::NumberBitwise::from_operator(operator)
                    && self.operand_supported(left) && self.operand_supported(right)
                    && [left, right].iter().all(|operand| operand.kind() != Some(OperandKind::Constant)
                        || super::facts::operand_type(&self.program.constants, *operand, |_| Type::ANY).is_subtype(Type::NUMBER))
                {
                    let exit = self.guard_exit(state, pc32);
                    let left = self.operand_to_int32(state, left, exit).expect("supported numeric operand");
                    let right = self.operand_to_int32(state, right, exit).expect("supported numeric operand");
                    // Signed and unsigned bitwise results are false exactly when all bits
                    // are zero. Reuse the value operation without boxing its branch result.
                    let bits = emit_number_bitwise_bits(&mut self.builder, operation, left, right);
                    let taken = self.ins().icmp_imm_u(IntCC::Equal, bits, 0);
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
                if self.coverage.full() && (self.completion == Completion::Number
                    || !((instruction.op() == Op::Return && plain) || instruction.returns_from_frame())) {
                    self.valid = false;
                }
                self.exit(state, pc32);
                false
            }
        }
    }

    /// A depth-zero capture read: one load from the dense slots of the activation that reads
    /// it, the running one's (`Entry::capture_values`) or a published callee's, fixed by the
    /// identity its call guards (`Lowering::frame_captures`), where the slot holds an
    /// ordinary value; otherwise, in an inlined frame, the VM's own projection of the closure
    /// the call bound (`lane_function_capture_read`), which also resolves shared and
    /// statically bound callees' slots; any other miss leaves at the read. Each read loads
    /// where it runs, after any call that may have stored to the slot.
    fn load_capture(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, exit: ir::Block,
        admitted_callee: Option<ir::Value>) -> ir::Value {
        let slot = crate::bytecode::ImmediateLayout::capture_slot(instruction.imm());
        let frame = match self.unit.origins.get(pc).copied() {
            Some(super::lane_unit::Origin::Inlined { frame, .. }) => Some(frame),
            _ => None,
        };
        let callee = admitted_callee.or_else(|| frame.map(|frame| self.tagged(state, self.unit.frames[frame].callee_register)));
        let dense = match frame {
            None => {
                let entry = self.entry.as_ref().expect("entry facts");
                Some((self.builder.use_var(entry.capture_values), self.builder.use_var(entry.capture_len)))
            }
            Some(frame) if self.unit.frames[frame].target.identity().is_some() => {
                let (values, len) = self.frame_captures.get(frame).copied().unwrap_or((0, 0));
                Some((self.ins().iconst(types::I64, values as i64), self.ins().iconst(types::I64, len as i64)))
            }
            Some(_) => None,
        };
        let (slow, join) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(slow);
        let value = self.builder.append_block_param(join, types::I64);
        match dense {
            Some((values, len)) => {
                let load = self.builder.create_block();
                let in_bounds = self.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, len, i64::from(slot));
                self.ins().brif(in_bounds, load, &[], slow, &[]);
                self.builder.switch_to_block(load);
                let offset = i32::from(slot) * std::mem::size_of::<Value>() as i32;
                let read = self.ins().load(types::I64, MemFlagsData::trusted(), values, offset);
                let tag = self.ins().band_imm_u(read, crate::value::ValueLayout::TAG_MASK as i64);
                let ordinary = self.ins().icmp_imm_u(IntCC::NotEqual, tag, crate::value::ValueLayout::INTERNAL_TAG as i64);
                self.ins().brif(ordinary, join, &[ir::BlockArg::Value(read)], slow, &[]);
            }
            None => { self.ins().jump(slow, &[]); }
        }
        self.builder.switch_to_block(slow);
        match callee {
            Some(callee) => {
                let vm = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, vm));
                let slot = self.ins().iconst(types::I64, i64::from(slot));
                let helper = self.helpers.address(self.builder.ins(), LaneHelper::FunctionCapture);
                let read = emit_lane_call(&mut self.builder, helper, &[vm, callee, slot]);
                let hit = self.ins().icmp_imm_u(IntCC::NotEqual, read, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, hit, exit);
                self.ins().jump(join, &[ir::BlockArg::Value(read)]);
            }
            None => { self.ins().jump(exit, &[]); }
        }
        self.builder.switch_to_block(join);
        value
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
        if integer {
            let (left, right) = (self.operand_int(state, left, exit)?, self.operand_int(state, right, exit)?);
            return emit_number_relation(&mut self.builder, operator, left, right, NumberRelationDomain::Int32);
        }
        // Operands only their tags type compare as int32 where both tags say so, else as
        // Numbers; neither path proves a fact the other does not.
        let tagged = |operand: &Operand| operand.register_index()
            .is_none_or(|register| state.values[usize::from(register)].value.representation == Representation::Tagged);
        if !(tagged(&left) && tagged(&right)) {
            let (left, right) = (self.operand_number(state, left, exit)?, self.operand_number(state, right, exit)?);
            return emit_number_relation(&mut self.builder, operator, left, right, NumberRelationDomain::Float64);
        }
        let (left, right) = (self.operand_tagged(state, left)?, self.operand_tagged(state, right)?);
        let (left_int, right_int) = (is_int_tagged(&mut self.builder, left), is_int_tagged(&mut self.builder, right));
        let ints = self.ins().band(left_int, right_int);
        let (int_block, number_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        let holds = self.builder.append_block_param(join, types::I8);
        self.ins().brif(ints, int_block, &[], number_block, &[]);
        self.builder.switch_to_block(int_block);
        let (left_bits, right_bits) = (self.ins().ireduce(types::I32, left), self.ins().ireduce(types::I32, right));
        let int_holds = emit_number_relation(&mut self.builder, operator, left_bits, right_bits, NumberRelationDomain::Int32)?;
        self.ins().jump(join, &[ir::BlockArg::Value(int_holds)]);
        self.builder.switch_to_block(number_block);
        let (left, right) = (emit_js_tagged_number(&mut self.builder, left, exit), emit_js_tagged_number(&mut self.builder, right, exit));
        let number_holds = emit_number_relation(&mut self.builder, operator, left, right, NumberRelationDomain::Float64)?;
        self.ins().jump(join, &[ir::BlockArg::Value(number_holds)]);
        self.builder.switch_to_block(join);
        Some(holds)
    }

    /// `source`'s value into `destination`, with every fact proved of it.
    fn copy(&mut self, state: &mut State, destination: Register, source: Register) {
        let fact = state.values[usize::from(source)];
        let variable = match fact.value.representation {
            Representation::Tagged => self.registers.tagged[usize::from(source)],
            Representation::Int32 => self.int_var(usize::from(source)),
            Representation::Float64 => self.float_var(usize::from(source)),
        };
        let value = if fact.value.representation == Representation::Tagged { self.tagged(state, source) }
            else { self.builder.use_var(variable) };
        let tagged = (fact.tagged.current() && fact.value.representation != Representation::Tagged)
            .then(|| self.tagged(state, source));
        let header = state.proofs.headers[usize::from(source)];
        let receiver = state.proofs.receivers.contains(&source);
        let layouts: Vec<_> = state.proofs.layouts.iter().filter_map(|((owner, shape), projection)|
            (*owner == Some(source)).then_some((*shape, *projection))).collect();
        let fields: smallvec::SmallVec<[_; INLINE_FIELD_BINDINGS]> = state.proofs.fields.iter()
            .filter(|field| field.owner == Some(source))
            .map(|field| FieldValue { owner: Some(destination), ..*field }).collect();
        self.define(state, destination, value, fact);
        if let Some(bits) = tagged {
            self.builder.def_var(self.registers.tagged[usize::from(destination)], bits);
            state.values[usize::from(destination)].tagged = TaggedView::Value(bits);
        }
        state.proofs.headers[usize::from(destination)] = header;
        for (shape, projection) in layouts { state.proofs.layouts.insert((Some(destination), shape), projection); }
        if receiver { state.proofs.receivers.insert(destination); }
        state.proofs.fields.extend(fields);
    }

    /// The call marker of inlined `frame` (`lane_unit::unit`): its callee is the function
    /// the call's site linked (an identity, compared) or a closure of the statically bound
    /// function (admitted by the VM's binding rule), and its receiver a direct object, which
    /// a plain call binds as `this`; a callee other than this frame's goes to the next
    /// alternative, and anything else leaves at the call, before its effects. The callee's
    /// parameters take the call's arguments and its other registers `undefined`, as its
    /// activation would (`lane_push_passed`).
    fn inline_call(&mut self, state: &mut State, pc: usize, frame: usize) -> bool {
        let inlined = self.unit.frames[frame];
        let exit = self.guard_exit(state, pc as u32);
        let callee = self.tagged(state, inlined.callee_register);
        let callee_index = usize::from(inlined.callee_register);
        let (same, proved) = match inlined.target.callee {
            // A dominating proof of the identity needs no compare.
            CalleeBinding::Identity(identity) if state.values[callee_index].value.ty.function_identity() == Some(identity)
                || self.entry.as_ref().expect("entry facts").calls.contains(frame) =>
                    (None, Some(Type::function(identity).expect("admitted callee locator"))),
            CalleeBinding::Identity(identity) => (
                Some(self.ins().icmp_imm_u(IntCC::Equal, callee, identity.to_bits() as i64)),
                Some(Type::function(identity).expect("admitted callee locator")),
            ),
            CalleeBinding::Function => (
                if state.values[callee_index].value.ty.body_index() == Some(inlined.target.function)
                    || self.entry.as_ref().expect("entry facts").calls.contains(frame) { None }
                    else { Some(self.function_binding(callee, inlined.target.function)) },
                Some(Type::function_body(inlined.target.function)),
            ),
        };
        if let Some(same) = same {
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
        }
        if let Some(ty) = proved {
            state.values[callee_index] = RegisterFact::admit(Fact {
                ty, representation: Representation::Tagged,
                binding: BindingTime::Location, proof: Proof::EntryGuard, projection: Projection::Exact,
            }).expect("an admitted callee and its dominating check");
        }
        let receiver = self.tagged(state, inlined.receiver);
        let entry = self.entry.as_ref().expect("entry facts");
        let admitted = entry.call_receivers.contains(frame)
            || state.proofs.headers[usize::from(inlined.receiver)].is_some()
            || entry.records[usize::from(inlined.receiver)].is_some()
            || (state.proofs.receivers.contains(&inlined.receiver) && entry.records[state.values.len()].is_some());
        let callee = self.program.function(inlined.target.function as usize);
        let numeric = inlined.recovery == super::lane_unit::CallRecovery::RestartCall
            && restartable_numeric_function(self.program, callee, &callee.instructions().collect::<Vec<_>>());
        if !admitted && !numeric {
            let admitted = self.receiver_recovery(receiver, ReceiverRecovery::for_body(callee));
            emit_js_guard(&mut self.builder, admitted, exit);
        }
        for register in 0..inlined.registers {
            let destination = inlined.base + register;
            let undefined = self.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
            self.set_tagged(state, destination, undefined);
        }
        for (destination, source) in inlined.passed_registers(callee) {
            self.copy(state, destination, source);
        }
        true
    }

    /// Nullish receivers need no boxing only when the body ignores `this`.
    /// All other primitives retain interpreter recovery's boxing semantics.
    fn receiver_recovery(&mut self, receiver: ir::Value, recovery: ReceiverRecovery) -> ir::Value {
        let family = self.ins().band_imm_u(receiver, crate::value::ValueLayout::HEAP_FAMILY_MASK as i64);
        let object = self.ins().icmp_imm_u(IntCC::Equal, family, crate::value::ValueLayout::DIRECT_OBJECT_TAG as i64);
        match recovery {
            ReceiverRecovery::Object => object,
            ReceiverRecovery::ObjectOrNullish => {
                let undefined = self.ins().icmp_imm_u(IntCC::Equal, receiver, Value::UNDEFINED.to_bits() as i64);
                let null = self.ins().icmp_imm_u(IntCC::Equal, receiver, Value::NULL.to_bits() as i64);
                let nullish = self.ins().bor(undefined, null);
                self.ins().bor(object, nullish)
            }
        }
    }

    fn function_binding(&mut self, callee: ir::Value, function: u32) -> ir::Value {
        let program = self.builder.ins().load(types::I32, MemFlagsData::trusted(), self.payload,
            std::mem::offset_of!(JsStepDispatch, program_id) as i32);
        let program = self.ins().uextend(types::I64, program);
        let function = self.ins().iconst(types::I64, i64::from(function));
        let helper = self.helpers.address(self.builder.ins(), LaneHelper::FunctionBinding);
        let admitted = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[program, callee, function]);
        self.ins().icmp_imm_u(IntCC::NotEqual, admitted, 0)
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
        let written: Vec<Register> = self.written.iter().map(|register| register as Register)
            .take_while(|register| *register < caller_registers).collect();
        for register in written {
            if !self.is_live(pc, register) { continue; }
            let bits = self.builder.use_var(self.registers.tagged[usize::from(register)]);
            let base = self.registers_base;
            self.ins().store(MemFlagsData::trusted(), bits, base, register_offset(register));
        }
        let completed = self.builder.create_block();
        self.reentries.push(completed);
        let helper = self.builder.create_block();
        match self.direct.get(&pc).copied() {
            // SAFETY: the body owns the callee for as long as its code runs.
            Some(callee) => self.direct_call(instruction, unsafe { &*callee }, helper, completed),
            None => { self.ins().jump(helper, &[]); }
        }
        self.builder.switch_to_block(helper);
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
        let call_word = self.ins().iconst(types::I64, i64::from(call_pc));
        let helper_address = self.helpers.address(self.builder.ins(), LaneHelper::Call);
        let status = emit_lane_helper_call(&mut self.builder, self.payload, helper_address, &[context, call_word]);
        let other = self.builder.create_block();
        let left = self.builder.create_block();
        self.builder.set_cold_block(left);
        let done = self.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_COMPLETED as i64);
        self.ins().brif(done, completed, &[], other, &[]);
        self.builder.switch_to_block(other);
        let miss = self.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_MISSED as i64);
        self.ins().brif(miss, missed, &[], left, &[]);
        self.builder.switch_to_block(left);
        let terminated = self.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_TERMINATED as i64);
        let stopped = self.builder.create_block();
        let callee = self.builder.create_block();
        self.ins().brif(terminated, stopped, &[], callee, &[]);
        self.builder.switch_to_block(stopped);
        let status = self.ins().iconst(types::I32, TERMINATED_STATUS);
        let leave = self.leave;
        self.ins().jump(leave, &[status.into()]);
        self.builder.switch_to_block(callee);
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
        // The callee may have changed any object: proofs of their shapes and storage retire.
        state.call_effect();
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
        let reserved = self.ins().iadd_imm_u(next, callee.reserve.values as i64 * word);
        let fits = self.ins().icmp(IntCC::UnsignedLessThanOrEqual, reserved, end);
        let shadow = self.ins().load(types::I64, MemFlagsData::trusted(), payload, offset(std::mem::offset_of!(JsStepDispatch, shadow_top)));
        let shadow_end = self.ins().load(types::I64, MemFlagsData::trusted(), payload, offset(std::mem::offset_of!(JsStepDispatch, shadow_end)));
        let recovered = self.ins().iadd_imm_u(shadow, callee.reserve.activations.max(1) as i64 * frame_size);
        let room = self.ins().icmp(IntCC::UnsignedLessThanOrEqual, recovered, shadow_end);
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
        // The caller owns the exact callee body whose entry it embeds.
        let entry = self.ins().iconst(types::I64, callee.body.entry_address() as i64);
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
        if self.fault_poll {
            self.emit_fault_poll();
            return;
        }
        let flag = self.interrupt;
        let requested = self.ins().atomic_load(types::I8, MemFlagsData::trusted(), flag);
        let exit = self.guard_exit(state, pc);
        let requested = self.ins().uextend(types::I32, requested);
        let clear = self.ins().icmp_imm_u(IntCC::Equal, requested, 0);
        emit_js_guard(&mut self.builder, clear, exit);
    }

    fn emit_fault_poll(&mut self) {
        let flag = self.interrupt;
        let requested = self.ins().atomic_load(types::I8, MemFlagsData::trusted(), flag);
        let ir::ValueDef::Result(inst, _) = self.builder.func.dfg.value_def(requested) else { unreachable!("poll instruction") };
        self.polls.push(inst);
    }

    /// Joining edges must not move a guard, conversion or recovery across the
    /// interrupt check. Only already-conforming SSA views may share this latch;
    /// flag-based checks retain their exact-PC recovery on the original edge.
    fn shared_backedge(&mut self, state: &State, target: usize) -> bool {
        if !self.fault_poll || !self.shared_headers.contains(target) || !self.entered.contains(target) { return false; }
        let Some(block) = self.blocks.get(target).copied().flatten() else { return false };
        let Some(types) = self.facts.at(target) else { return false };
        if !types.iter().copied().enumerate().all(|(index, ty)| {
            if !self.is_live(target, index as Register) { return true; }
            let fact = state.values[index];
            fact.value.ty.is_subtype(ty) && if ty == Type::INT32 {
                fact.value.representation == Representation::Int32
            } else if ty == Type::NUMBER {
                fact.value.representation == Representation::Float64
            } else { fact.tagged.current() }
        }) { return false; }
        let latch = match self.poll_latches[block] {
            Some(latch) => latch,
            None => {
                let latch = self.builder.create_block();
                self.poll_latches[block] = Some(latch);
                latch
            }
        };
        self.finishes = true;
        self.ins().jump(latch, &[]);
        true
    }

    fn branch(&mut self, state: &mut State, pc: usize, target: usize, taken: ir::Value) {
        let taken_block = self.builder.create_block();
        let fall_block = self.builder.create_block();
        self.ins().brif(taken, taken_block, &[], fall_block, &[]);
        self.continue_branch(state, pc, target, taken_block, fall_block);
    }

    fn continue_branch(&mut self, state: &mut State, pc: usize, target: usize,
        taken_block: ir::Block, fall_block: ir::Block) {
        self.backedge |= target <= pc && target == self.coverage.entry();
        self.builder.switch_to_block(taken_block);
        let mut taken_state = state.clone();
        self.continue_at(&mut taken_state, target);
        self.builder.switch_to_block(fall_block);
        let mut fall_state = state.clone();
        self.continue_at(&mut fall_state, pc + 1);
    }

    /// A family edge refines the value and existing SSA copies, not heap contents.
    fn refine_object_family(&mut self, state: &mut State, bits: ir::Value) {
        let bits = self.builder.func.dfg.resolve_aliases(bits);
        for fact in &mut state.values {
            let TaggedView::Value(value) = fact.tagged else { continue };
            if self.builder.func.dfg.resolve_aliases(value) == bits && !fact.value.ty.is_direct_object_family() {
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(fact.value.ty.direct_object_part().unwrap_or(Type::DIRECT_OBJECT_FAMILY), Representation::Tagged) };
            }
        }
    }

    /// Move-only latches can carry the nullish test's direct-object proof to
    /// the next field receiver. These are guarded input contracts, including
    /// the first iteration, never assumptions about values stored in a field.
    fn require_object_latches(&mut self, instructions: &[WideInstruction], leaders: &CompoundBitSet,
        loops: &[(usize, usize)]) {
        for &(header, back) in loops {
            if instructions[back].op() != Op::Jump
                || self.unit.markers.iter().any(|pc| (header..=back).contains(pc)) { continue; }
            let first = instructions[header];
            if first.op() != Op::GetField || first.result_flags() != 0 { continue; }
            let Some(FieldLookup::Atom { base, .. }) = first.field_lookup() else { continue };
            let Some(receiver) = base.register_index() else { continue };
            let mut required = receiver;
            let mut inputs = smallvec::SmallVec::<[(usize, Register); INLINE_LOOP_INPUTS]>::new();
            inputs.push((header, receiver));
            let mut predicate = None;
            for pc in (header + 1..back).rev() {
                let instruction = instructions[pc];
                match instruction.op() {
                    Op::Move if instruction.result_flags() == 0 => {
                        if instruction.register_a() == required { required = instruction.register_b(); }
                    }
                    Op::Nop => {},
                    _ => { predicate = Some(pc); break; }
                }
                if leaders.contains(pc) { inputs.push((pc, required)); }
            }
            let Some(pc) = predicate else { continue };
            let Some(comparison) = nullish_comparison(&self.program.constants, instructions[pc], |_| Type::ANY)
                .filter(|comparison| !comparison.strict && comparison.negated
                    && instructions[pc].jump_target() as usize > back) else { continue };
            // Copies before the comparison share its SSA value as well.
            let root = |mut register| {
                for at in (header..pc).rev() {
                    if at + 1 < pc && leaders.contains(at + 1) { break; }
                    let instruction = instructions[at];
                    match instruction.op() {
                        Op::Move if instruction.result_flags() == 0 => {
                            if instruction.register_a() == register { register = instruction.register_b(); }
                        }
                        Op::Nop => {},
                        _ => break,
                    }
                }
                register
            };
            if root(required) != root(comparison.source) { continue; }
            if !inputs.iter().any(|&(leader, _)| leader == pc + 1) { inputs.push((pc + 1, required)); }
            if !inputs.iter().all(|&(pc, register)| self.facts.at(pc)
                .and_then(|types| types.get(usize::from(register)))
                .is_some_and(|ty| ty.is_subtype(Type::DIRECT_OBJECT_FAMILY)
                    || Type::DIRECT_OBJECT_FAMILY.is_subtype(*ty))) { continue; }
            for (pc, register) in inputs { self.facts.require(pc, register, Type::DIRECT_OBJECT_FAMILY); }
        }
    }

    /// An unchanged arm preserves a checked entry hypothesis. Every definition
    /// and backedge must preserve it too; failed hypotheses are removed before
    /// reanalysis so a dependent candidate cannot keep using their assumptions.
    fn refine_inductive_inputs(&mut self, initial: &mut [Type], instructions: &[WideInstruction],
        boundaries: &CompoundBitSet, loops: &[(usize, usize)],
        own_shape: &dyn Fn(usize) -> Option<(Register, u32)>) {
        let header = self.coverage.entry();
        let Some((_, back)) = loops.iter().copied().filter(|(start, _)| *start == header)
            .max_by_key(|(_, back)| *back) else { return };
        // Alternative markers add edges beyond the bytecode Nop transfer.
        // Keep their canonical lowering until this analysis models those edges.
        if self.unit.markers.iter().zip(&self.unit.frames).any(|(pc, frame)|
            (header..=back).contains(pc) && frame.alternative.is_some()) { return; }
        let mut candidates: CompoundBitSet = CompoundBitSet::with_capacity(initial.len());
        self.facts.for_each_definition(instructions, boundaries, &self.program.constants, &writes, self.native_calls,
            |pc, register, ty| {
                let index = usize::from(register);
                if (header..=back).contains(&pc) && register < self.unit.caller_registers
                    && self.is_live(header, register) && self.written.contains(index)
                    && matches!(initial[index], Type::ANY | Type::NUMBER) && ty == Type::INT32 {
                    candidates.insert(index);
                }
            });
        if let Some(facts) = super::facts::inductive_int32(initial, candidates, header..=back,
            instructions, boundaries, &self.program.constants, &writes, own_shape, self.native_calls) {
            self.facts = facts;
        }
    }

    /// The type already proved for an operand at a use. Feedback is not enough:
    /// an admitted field read carries its actual loop-entry contract, even when
    /// the general analysis has no field feedback (as in linked AOT).
    fn int32_operand_at(&self, pc: usize, operand: Operand, boundaries: &CompoundBitSet) -> bool {
        if super::facts::operand_type(&self.program.constants, operand, |_| Type::ANY) == Type::INT32 { return true; }
        let Some(mut register) = operand.register_index() else { return false };
        if self.facts.before(pc, register, &self.unit.instructions, boundaries,
            &self.program.constants, &writes, self.native_calls) == Type::INT32 { return true; }
        for at in (0..pc).rev() {
            if boundaries.contains(at + 1) || self.unit.markers.contains(&at) { return false; }
            let instruction = self.unit.instructions[at];
            if instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { return false; }
            if writes(instruction).contains(&register) {
                if instruction.op() == Op::Move && instruction.result_register() == register {
                    register = instruction.register_b();
                } else {
                    return instruction.defined_register() == Some(register)
                        && self.hoisted.get(&at).is_some_and(|read| read.ty == Type::INT32);
                }
            }
        }
        false
    }

    /// A capture may choose the same checked integer view as a bitwise use, or
    /// as the already-proved integer on the other side of a Number relation.
    /// This selects an entry guard; it does not prove the capture's type.
    fn capture_int32_use(&self, pc: usize, boundaries: &CompoundBitSet) -> bool {
        let mut aliases: CompoundBitSet = CompoundBitSet::with_capacity(usize::from(self.unit.registers));
        aliases.insert(usize::from(self.unit.instructions[pc].result_register()));
        for at in pc + 1..self.unit.instructions.len() {
            if boundaries.contains(at) || self.unit.markers.contains(&at) { break; }
            let instruction = self.unit.instructions[at];
            let operator = match instruction.op() {
                Op::JumpBinaryFalse => Some(instruction.binary_operator_field()),
                op if op.js_denotation() == JsDenotation::NumericBinary => Some(instruction.binary_operator()),
                _ => None,
            };
            if let Some(operator) = operator {
                let operands = [instruction.operand_b(), instruction.operand_c()];
                let captured = |operand: Operand| operand.register_index()
                    .is_some_and(|register| aliases.contains(usize::from(register)));
                if operands.iter().copied().any(captured) {
                    if super::operations::NumberBitwise::from_operator(operator).is_some() { return true; }
                    if super::operations::NumberRelation::from_operator(operator).is_some()
                        && operands.iter().copied().enumerate().any(|(index, operand)| captured(operand)
                            && self.int32_operand_at(at, operands[1 - index], boundaries)) { return true; }
                }
            }
            if instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { break; }
            let copied = instruction.op() == Op::Move && aliases.contains(usize::from(instruction.register_b()));
            for register in writes(instruction) { aliases.remove(usize::from(register)); }
            if copied { aliases.insert(usize::from(instruction.result_register())); }
            if aliases.is_empty() { break; }
        }
        false
    }

    /// The reads `header`'s loop repeats unchanged, made on its entry edge from `state`: a
    /// guard that fails there leaves at the header, before the loop runs.
    fn hoist_reads(&mut self, state: &mut State, header: usize) {
        let Some(reads) = self.hoists.get(&header).cloned() else { return };
        let boundaries = leaders(&self.unit.instructions);
        for InvariantRead { pc, owner } in reads.iter().copied() {
            let instruction = self.unit.instructions[pc];
            let field = self.loop_fields.iter().position(|field| field.reads[0] == pc);
            let exit = self.guard_exit(state, header as u32);
            let (value, int32) = if instruction.op() == Op::LoadCapture {
                let admitted_callee = if let super::lane_unit::Origin::Inlined { frame, .. } = self.unit.origins[pc] {
                    let Some(read) = invariant_callee_read(self.unit, reads.iter().map(|read| read.pc), frame, &boundaries)
                        .and_then(|read| self.hoisted.get(&read).copied()) else { continue };
                    let target = self.unit.frames[frame].target;
                    let admitted = match target.callee {
                        CalleeBinding::Identity(identity) => read.ty.function_identity() == Some(identity),
                        CalleeBinding::Function => read.ty.body_index() == Some(target.function),
                    };
                    if !admitted { continue; }
                    Some(self.builder.use_var(read.value))
                } else { None };
                let value = self.load_capture(state, pc, instruction, exit, admitted_callee);
                let int32 = self.capture_int32_use(pc, &boundaries);
                (value, int32)
            } else {
                let Some(FieldLookup::Atom { base, cache_site, .. }) = instruction.field_lookup() else { continue };
                let register = owner.or_else(|| base.register_index());
                let receiver = !self.native_calls
                    && (base == crate::bytecode::FieldBase::THIS || register.is_some_and(|register| state.proofs.receivers.contains(&register)));
                (self.base_field_read(state, register, receiver, cache_site, exit), int32_read(self.program, self.fields, instruction))
            };
            let carried = field.is_some();
            let int32 = if carried {
                self.loop_fields[field.unwrap()].stores.iter().all(|store| {
                    self.facts.before(*store, self.unit.instructions[*store].register_a(),
                        &self.unit.instructions, &boundaries, &self.program.constants, &writes, self.native_calls) == Type::INT32
                })
            } else { int32 };
            if int32 {
                let integer = is_int_tagged(&mut self.builder, value);
                emit_js_guard(&mut self.builder, integer, exit);
            }
            let mut ty = if int32 { Type::INT32 } else { Type::ANY };
            if !int32 {
                // This is a candidate, never a proof from source syntax. Admit the
                // loaded closure on the same entry edge that already hoists its
                // immutable read. A failed admission exits before loop effects.
                let target = self.unit.markers.iter().enumerate().find_map(|(frame, marker)| {
                    let call = self.unit.frames[frame];
                    let supplies = if instruction.op() == Op::LoadCapture {
                        // Preserve the existing caller-capture admission, including
                        // function-bound alternatives. Hoisting a callee's own
                        // captures additionally requires the monomorphic proof below.
                        let destination = instruction.result_register();
                        *marker > pc && call.callee_register == destination
                            && (call.target.callee == CalleeBinding::Function
                                || (call.alternative.is_none() && !self.unit.frames.iter()
                                    .any(|other| other.alternative == Some(*marker))))
                            && !self.unit.instructions[pc + 1..*marker].iter()
                                .any(|instruction| writes(*instruction).contains(&destination))
                    } else { invariant_callee_read(self.unit, [pc], frame, &boundaries) == Some(pc) };
                    supplies.then_some(call.target)
                });
                if let Some(target) = target {
                    let (matched, admitted) = match target.callee {
                        CalleeBinding::Function => (self.function_binding(value, target.function),
                            Type::function_body(target.function)),
                        CalleeBinding::Identity(identity) => (
                            self.ins().icmp_imm_u(IntCC::Equal, value, identity.to_bits() as i64),
                            Type::function(identity).expect("admitted callee locator")),
                    };
                    emit_js_guard(&mut self.builder, matched, exit);
                    ty = admitted;
                }
            }
            let representation = if int32 { Representation::Int32 } else { Representation::Tagged };
            let raw = if representation == Representation::Int32 { self.ins().ireduce(types::I32, value) } else { value };
            let variable = self.builder.declare_var(if representation == Representation::Int32 { types::I32 } else { types::I64 });
            self.builder.def_var(variable, raw);
            let receiver = if instruction.op() == Op::GetField {
                match instruction.field_lookup() {
                    Some(FieldLookup::Atom { base, .. }) => owner.or_else(|| base.register_index())
                        .map(|owner| (owner, state.values[usize::from(owner)].value.ty)),
                    _ => None,
                }
            } else { None };
            let stores: smallvec::SmallVec<[(usize, Option<FieldSlotBinding>); INLINE_FIELD_BINDINGS]> =
                field.into_iter().flat_map(|index| self.loop_fields[index].stores.iter()).filter_map(|store| {
                    if !int32 { return None; }
                    let site = self.unit.instructions[*store].cache_site_index();
                    let binding = self.entry.as_ref()?.fields.iter().find_map(|(known, binding)|
                        (*known == site).then_some(*binding));
                    Some((*store, binding))
                }).collect();
            let object_header = if !stores.is_empty() {
                receiver.map(|(owner, _)| self.object_header(state, Some(owner), exit))
            } else { None };
            if let (Some((owner, _)), Some(object_header)) = (receiver, object_header) {
                let object = self.tagged(state, owner);
                for (store, binding) in stores {
                    let instruction = self.unit.instructions[store];
                    let site = instruction.cache_site_index();
                    let mut binding = binding.unwrap_or_else(|| emit_js_field_binding(
                        &mut self.builder, self.payload, site, exit, None));
                    self.admit_field_writes(&mut binding, Type::INT32, exit);
                    let allowed = self.store_atom_admitted(instruction.atom_index());
                    emit_js_guard(&mut self.builder, allowed, exit);
                    let packed = self.builder.create_block();
                    let packed_address = self.builder.append_block_param(packed, types::I64);
                    let done = self.builder.create_block();
                    let address = self.builder.append_block_param(done, types::I64);
                    let (arena_address, _) = emit_js_cached_field_slot(&mut self.builder,
                        self.registers_base, self.payload, JsFieldReceiver::Value(object), site,
                        false, exit, Some(binding), Some(object_header), Some(packed));
                    self.ins().jump(done, &[ir::BlockArg::Value(arena_address)]);
                    self.builder.switch_to_block(packed);
                    self.ins().jump(done, &[ir::BlockArg::Value(packed_address)]);
                    self.builder.switch_to_block(done);
                    let packed_range = field.and_then(|index| {
                        let field = &self.loop_fields[index];
                        field.stores.iter().position(|pc| *pc == store).and_then(|index| field.store_ranges[index])
                    });
                    self.loop_stores.insert(store, LoopStore { header: object_header, address, packed_range });
                }
            }
            let read = LoopRead { value: variable, ty, representation, receiver, header: object_header };
            if let Some(index) = field {
                for pc in &self.loop_fields[index].reads { self.hoisted.insert(*pc, read); }
            } else { self.hoisted.insert(pc, read); }
        }
    }

    fn get_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        // A returning read completes the activation with its value, at a caller record; an
        // inlined callee's is the unit's read, move and jump (`lane_unit`).
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) => None,
            (true, Some(super::lane_unit::Origin::Caller(record))) if self.completion == Completion::StatusAndPc => Some(record),
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
                || register.is_some_and(|register| state.proofs.receivers.contains(&register)));
        let (first, rest) = parts.split_first()?;
        let field = match instruction.field_lookup() {
            Some(FieldLookup::Atom { atom, .. }) if sink.is_none() && completes.is_none() => {
                Some((register, atom))
            }
            _ => None,
        };
        let known = field.and_then(|(owner, atom)| state.proofs.fields.iter().copied()
            .find(|field| field.owner == owner && field.atom == atom));
        let mut value = match known {
            Some(field) => {
                #[cfg(test)] record_field_forward(*first);
                field.value
            }
            None => self.base_field_read(state, register, receiver, *first, exit),
        };
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
        let integer = known.is_some_and(|field| field.ty == Type::INT32);
        let ty = if integer || int32_read(self.program, self.fields, instruction) {
            if !integer {
                let integer = is_int_tagged(&mut self.builder, value);
                emit_js_guard(&mut self.builder, integer, exit);
            }
            Type::INT32
        } else { Type::ANY };
        if let Some((owner, atom)) = field {
            let read = FieldValue { owner, atom, value, ty };
            match state.proofs.fields.iter_mut().find(|field| field.owner == owner && field.atom == atom) {
                Some(field) => *field = read,
                None => state.proofs.fields.push(read),
            }
        }
        if ty != Type::INT32 {
            self.set_tagged(state, destination, value);
            return Some(true);
        }
        self.set_tagged(state, destination, value);
        let int = self.ins().ireduce(types::I32, value);
        let variable = self.int_var(usize::from(destination));
        self.builder.def_var(variable, int);
        state.values[usize::from(destination)] = RegisterFact { tagged: TaggedView::Value(value),
            ..RegisterFact::definition(Type::INT32, Representation::Int32) };
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
        let probe_owner = match register {
            Some(register) if !receiver => FieldOwner::Register(register),
            _ => FieldOwner::This,
        };
        let shapes = self.fields.get(&site).cloned().unwrap_or_default();
        if shapes.is_empty() {
            return self.field_probe(state, probe_owner, object_bits, site, exit);
        }
        let owner = if receiver { None } else { register };
        let probed = shapes.iter().any(|field| field.holder.is_some());
        self.probed_read(state, owner, probe_owner, probed, object_bits, site, exit, |lowering, state, generic| {
            let (object, shape, inline) = if receiver {
                let entry = lowering.entry.as_ref().expect("entry facts");
                (
                    lowering.builder.use_var(entry.receiver.object),
                    lowering.builder.use_var(entry.receiver.shape),
                    lowering.builder.use_var(entry.receiver.inline),
                )
            } else {
                let header = lowering.object_header(state, register, generic);
                (header.record.address(&mut lowering.builder), header.shape, header.inline)
            };
            let read = lowering.shape_switch_read(state, Some(owner), object, shape, inline, &shapes, generic)?;
            // A single own shape read proves the register's object has it, for the block, as
            // the facts the CFG carries say (`facts::analyze`), a receiver alias's too.
            if let ([field], Some(register)) = (shapes.as_slice(), register) {
                state.values[usize::from(register)] = RegisterFact::definition(
                    Type::direct_object(field.shape), Representation::Tagged);
            }
            Some(read)
        })
    }

    /// The read through `site` of the object a previous read of a chain produced, whose
    /// record nothing else names, so it is proved for this read alone.
    fn object_field_read(&mut self, state: &mut State, object_bits: ir::Value, site: u16, exit: ir::Block) -> Option<ir::Value> {
        let shapes = self.fields.get(&site).cloned().unwrap_or_default();
        if shapes.is_empty() {
            return Some(self.field_probe(state, FieldOwner::Temporary, object_bits, site, exit));
        }
        let probed = shapes.iter().any(|field| field.holder.is_some());
        Some(self.probed_read(state, None, FieldOwner::Temporary, probed, object_bits, site, exit, |lowering, state, generic| {
            let records = lowering.object_table();
            let header = ObjectHeader::prove(&mut lowering.builder, object_bits, records, None, generic);
            let record = header.record.address(&mut lowering.builder);
            lowering.shape_switch_read(state, None, record, header.shape, header.inline, &shapes, generic)
        }))
    }

    /// A read by the site's proved shapes (`fast`), whose misses leave through `exit`; where
    /// `probed` (the site names prototype-held slots, which prove only the immediate prototype,
    /// `Vm::ordinary_holder_entry_value`), a miss first asks the VM's caches, whose other tiers
    /// read deeper holders (`field_probe`). Past that join only what both paths prove holds:
    /// the fast path's proofs of `owner`'s object do not.
    #[allow(clippy::too_many_arguments)]
    fn probed_read(&mut self, state: &mut State, owner: Option<Register>, probe_owner: FieldOwner, probed: bool,
        object_bits: ir::Value, site: u16, exit: ir::Block,
        fast: impl FnOnce(&mut Self, &mut State, ir::Block) -> Option<ir::Value>) -> ir::Value {
        if !probed {
            return match fast(self, state, exit) {
                Some(read) => read,
                None => self.field_probe(state, probe_owner, object_bits, site, exit),
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
        // The generic path starts from any of the fast path's guards: only the proofs from
        // before the read dominate it.
        let probed = self.field_probe(&mut before.clone(), probe_owner, object_bits, site, exit);
        self.ins().jump(join, &[ir::BlockArg::Value(probed)]);
        self.builder.switch_to_block(join);
        for (header, known) in state.proofs.headers.iter_mut().zip(&before.proofs.headers) {
            if known.is_none() { *header = None; }
        }
        state.proofs.layouts.retain(|key, _| before.proofs.layouts.contains_key(key));
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
            let known = owner.and_then(|owner| state.proofs.layouts.get(&(owner, field.shape)).copied());
            let projection = match known {
                Some(projection) if usize::from(field.slot) < projection.storage_len => projection,
                _ => {
                    let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
                    emit_js_guard(&mut self.builder, same, exit);
                    let projection = CheckedObject::prove(&mut self.builder, object, inline, properties,
                        properties_len, *field, PackedAccess::Reload, exit)?;
                    if let Some(owner) = owner {
                        state.proofs.layouts.insert((owner, field.shape), projection);
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
        let records = match self.object_table() {
            ObjectTable::Checked(table) => table.base(),
            ObjectTable::Unchecked(base) => base,
        };
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

    /// A dominating direct-object proof of `owner`'s object (a register's, or `this`'s), for
    /// its lifetime until an effect that may change it retires the proof (`State::headers`),
    /// derived from the entry's record certificate where it has one, otherwise proved here.
    fn object_header(&mut self, state: &mut State, owner: Option<Register>, exit: ir::Block) -> ObjectHeader {
        let index = state.header_index(owner);
        if let Some(header) = state.proofs.headers[index] { return header; }
        let fact = owner.map(|register| state.values[usize::from(register)].value);
        let header = if let Some(record) = self.entry.as_ref().expect("entry facts").records[index] {
            record.header(&mut self.builder, fact)
        } else {
            let bits = match owner {
                Some(register) => self.tagged(state, register),
                None => self.builder.use_var(self.entry.as_ref().expect("entry facts").this),
            };
            let records = self.object_table();
            ObjectHeader::prove(&mut self.builder, bits, records, fact, exit)
        };
        state.proofs.headers[index] = Some(header);
        if let Some(fact) = fact {
            let ty = fact.ty;
            state.values[index] = RegisterFact { tagged: state.values[index].tagged,
                ..RegisterFact::definition(
                    ty.direct_object_part().unwrap_or(Type::DIRECT_OBJECT_FAMILY),
                    Representation::Tagged) };
        }
        header
    }

    fn object_table(&mut self) -> ObjectTable {
        match self.entry.as_ref().and_then(|entry| entry.object_table) {
            Some(table) => ObjectTable::Checked(table),
            None => ObjectTable::Unchecked(emit_js_payload_word(&mut self.builder, self.payload,
                std::mem::offset_of!(JsStepDispatch, object_record_base))),
        }
    }

    /// The VM's own field-cache read (`Vm::cached_field_hit`), prototype holders included,
    /// through the site's bound slot first where the entry bound one; a miss leaves through
    /// `exit`.
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
            self.builder.set_cold_block(slow);
            let done = self.builder.create_block();
            let packed = self.builder.create_block();
            let packed_address = self.builder.append_block_param(packed, types::I64);
            let result = self.builder.append_block_param(done, types::I64);
            let (address, _) = emit_js_cached_field_slot(&mut self.builder, self.registers_base, self.payload,
                JsFieldReceiver::Value(object), site, true, slow, Some(binding), header, Some(packed));
            let value = self.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
            self.ins().jump(done, &[ir::BlockArg::Value(value)]);
            self.builder.switch_to_block(packed);
            let narrow = self.ins().load(types::I32, MemFlagsData::trusted(), packed_address, 0);
            let table = match self.helpers {
                #[cfg(feature = "aot")]
                LaneHelpers::Bound(_, symbol) => NarrowTable::Linked(symbol),
                _ => NarrowTable::Process,
            };
            emit_narrow_read(&mut self.builder, narrow, table, done);
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
        let helper = self.helpers.address(self.builder.ins(), LaneHelper::FieldGet);
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
        let carried = match self.loop_fields.iter().find(|field| field.stores.contains(&pc)) {
            Some(field) => Some(*self.hoisted.get(&field.reads[0])?),
            None => None,
        };
        let exit = self.guard_exit(state, pc as u32);
        let carried_int = carried.filter(|read| read.representation == Representation::Int32)
            .map(|_| self.int(state, source, exit));
        let ty = state.values[usize::from(source)].value.ty;
        let value = self.tagged(state, source);
        let projected = self.loop_stores.get(&pc).copied().filter(|slot|
            ty.is_subtype(Type::INT32) && state.proofs.headers[state.header_index(register)]
                .is_some_and(|header| header.record == slot.header.record));
        if let Some(slot) = projected {
            #[cfg(test)] LOOP_SLOT_STORES.with(|count| count.set(count.get() + 1));
            state.proofs.fields.clear();
            let packed = self.builder.create_block();
            let arena = self.builder.create_block();
            let done = self.builder.create_block();
            self.ins().brif(slot.header.inline, packed, &[], arena, &[]);
            self.builder.switch_to_block(packed);
            #[cfg(test)] if slot.packed_range.is_some() { PACKED_RANGE_SITE.with(|site| {
                if let Some((expected, count)) = site.get() && expected == instruction.cache_site_index() {
                    site.set(Some((expected, count + 1)));
                }
            }); }
            let narrow = emit_narrow_integer_write(&mut self.builder, value, exit, slot.packed_range);
            // Ordinary JS own fields are not shared memory. Observation requires
            // a guest/host effect or poll; the backend must prove their absence
            // before deferring this numeric, barrier-free write to an exit.
            let store = self.ins().store(MemFlagsData::trusted(), narrow, slot.address, 0);
            self.builder.func.private_stores.push(store);
            self.ins().jump(done, &[]);
            self.builder.switch_to_block(arena);
            // Verified integers neither widen an arena slot nor create a heap edge.
            let store = self.ins().store(MemFlagsData::trusted(), value, slot.address, 0);
            self.builder.func.private_stores.push(store);
            self.ins().jump(done, &[]);
            self.builder.switch_to_block(done);
        } else {
            self.store_field(state, register, instruction.atom_index(), instruction.cache_site_index(), value, ty, exit)?;
        }
        if let Some(read) = carried {
            self.builder.def_var(read.value, carried_int.unwrap_or(value));
            #[cfg(test)] if read.representation == Representation::Int32 {
                LOOP_INT32_PHIS.with(|count| count.set(count.get() + 1));
            }
        }
        Some(true)
    }

    /// The store of `value` through `site` to the field `atom` of the object `register` names
    /// (`this` for `None`): the site's one proved shape and slot written directly, with the
    /// object's record proved once (the receiver's at entry, in a body that makes no call),
    /// else the VM's own-field store hit; anything else leaves through `exit`. A value of
    /// type `ty` the slot's representation may not hold takes the hit too, which joins it.
    #[allow(clippy::too_many_arguments)]
    fn store_field(&mut self, state: &mut State, register: Option<Register>, atom: Atom, site: u16,
        value: ir::Value, ty: Type, exit: ir::Block) -> Option<()> {
        // Different receiver values can alias; without a disjointness proof any
        // write retires this memory version, including writes through inlinees.
        state.proofs.fields.clear();
        let receiver = !self.native_calls
            && (register.is_none() || register.is_some_and(|register| state.proofs.receivers.contains(&register)));
        let owner = if receiver { None } else { register };
        let object_bits = match register {
            None => self.builder.use_var(self.entry.as_ref().expect("entry facts").this),
            Some(register) => self.tagged(state, register),
        };
        // A bound slot's store needs a dominating proof of the object, for both its branches.
        let bound = self.entry.as_ref().is_some_and(|entry| entry.fields.iter().any(|(known, _)| *known == site));
        let dynamic_header = bound.then(|| self.object_header(state, register, exit));
        let field = self.fields.get(&site).and_then(|shapes| (shapes.len() == 1).then(|| shapes[0]));
        let Some(field) = field else {
            self.store_helper(object_bits, atom, site, value, ty, exit, dynamic_header);
            return Some(());
        };
        let header = (!receiver).then(|| {
            let header = self.object_header(state, register, exit);
            (header.record.address(&mut self.builder), header.shape, header.inline)
        });
        let admitted = self.store_atom_admitted(atom);
        let entry = self.entry.as_ref().expect("entry facts");
        let projection = state.proofs.layouts.get(&(owner, field.shape)).copied();
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
        // A value the slot's type may not hold takes the VM's hit, which joins its type
        // (`ValueArena::note_slot_value`); a bound slot's raw store would not.
        let widen = (!ty.is_subtype(field.ty)).then(|| {
            let widen = self.builder.create_block();
            self.builder.set_cold_block(widen);
            self.represented(field.ty, value, widen);
            widen
        });
        let record_id = self.ins().ireduce(types::I32, object_bits);
        let record_id = self.ins().uextend(types::I64, record_id);
        let payload = self.payload;
        emit_unlogged_property_store_guard(&mut self.builder, payload, record_id, value, ty, general);
        // An int32 into a packed object's inline slot is its narrow word where it fits and
        // the object has no extras (`Object::store_inline_narrow`); it needs no barrier.
        if ty.is_subtype(Type::INT32) && usize::from(field.slot) < crate::heap::NATIVE_OBJECT_LAYOUT.inline_property_count {
            self.store_narrow_int(object, inline, field.slot, value, done, general);
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
        self.store_helper(object_bits, atom, site, value, ty, exit, dynamic_header);
        self.ins().jump(done, &[]);
        if let Some(widen) = widen {
            self.builder.switch_to_block(widen);
            self.store_helper_unbound(object_bits, atom, site, value, exit);
            self.ins().jump(done, &[]);
        }
        self.builder.switch_to_block(done);
        Some(())
    }

    /// Whether the run admits a direct store of `atom`. A run that calls may meet a method
    /// cache that names the atom only after a call; one that does not tests it once, at
    /// entry (`Lowering::entry_facts`).
    fn store_atom_admitted(&mut self, atom: Atom) -> ir::Value {
        if self.native_calls {
            self.emit_store_atom_admitted(atom)
        } else {
            self.builder.use_var(self.entry.as_ref().expect("entry facts").store_atoms[&atom])
        }
    }

    /// Store the int32 `value` (tagged) into inline `slot` of the packed object `record` and
    /// continue at `done`; an object with arena storage continues here, one with extras or a
    /// value past the narrow range takes `general`.
    fn store_narrow_int(&mut self, record: ir::Value, inline: ir::Value, slot: u16, value: ir::Value,
        done: ir::Block, general: ir::Block) {
        use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
        let (packed, arena) = (self.builder.create_block(), self.builder.create_block());
        self.ins().brif(inline, packed, &[], arena, &[]);
        self.builder.switch_to_block(packed);
        let payload = self.ins().load(types::I64, MemFlagsData::trusted(), record, OBJECT.payload_offset as i32);
        let tag = self.ins().band_imm_u(payload, OBJECT.extras_tag_mask as i64);
        let plain = self.ins().icmp_imm_u(IntCC::NotEqual, tag, OBJECT.extras_tag as i64);
        emit_js_guard(&mut self.builder, plain, general);
        let narrow = emit_narrow_integer_write(&mut self.builder, value, general, None);
        let offset = OBJECT.payload_offset + usize::from(slot) * std::mem::size_of::<u32>();
        self.ins().store(MemFlagsData::trusted(), narrow, record, offset as i32);
        self.ins().jump(done, &[]);
        self.builder.switch_to_block(arena);
    }

    /// Continue where a slot of type `slot` (`NativeFieldLoad::ty`) holds `value`, by its int
    /// tag, else at `otherwise`.
    fn represented(&mut self, slot: Type, value: ir::Value, otherwise: ir::Block) {
        // Only the type algebra may establish that an int fits. In particular,
        // nullable object contracts must not inherit the old numeric-chain rule.
        let held = if Type::INT32.is_subtype(slot) { is_int_tagged(&mut self.builder, value) }
            else { self.ins().iconst(types::I8, 0) };
        emit_js_guard(&mut self.builder, held, otherwise);
    }

    /// The lane's logged own-field store hit (`Vm::primary_own_field_store_hit`), barrier
    /// included, through the site's bound slot first where the entry bound one; anything
    /// else leaves through `exit`.
    fn store_helper(&mut self, object: ir::Value, atom: Atom, site: u16, value: ir::Value, ty: Type, exit: ir::Block, header: Option<ObjectHeader>) {
        let bound = self.entry.as_ref().and_then(|entry| entry.fields.iter()
            .find_map(|(known, binding)| (*known == site).then_some(*binding)));
        if let Some(binding) = bound.filter(|binding| binding.writes.is_some_and(|domain| ty.is_subtype(domain))) {
            let slow = self.builder.create_block();
            let done = self.builder.create_block();
            let packed = self.builder.create_block();
            let packed_address = self.builder.append_block_param(packed, types::I64);
            let allowed = self.store_atom_admitted(atom);
            emit_js_guard(&mut self.builder, allowed, slow);
            let (address, record) = emit_js_cached_field_slot(&mut self.builder, self.registers_base, self.payload,
                JsFieldReceiver::Value(object), site, false, slow, Some(binding), header, Some(packed));
            emit_unlogged_property_store_guard(&mut self.builder, self.payload, record, value, ty, slow);
            self.ins().store(MemFlagsData::trusted(), value, address, 0);
            self.ins().jump(done, &[]);
            self.builder.switch_to_block(packed);
            if !ty.is_subtype(Type::INT32) {
                let integer = is_int_tagged(&mut self.builder, value);
                emit_js_guard(&mut self.builder, integer, slow);
            }
            let narrow = emit_narrow_integer_write(&mut self.builder, value, slow, None);
            // Immediate integers cannot introduce a heap edge or require a GC barrier.
            self.ins().store(MemFlagsData::trusted(), narrow, packed_address, 0);
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
        let helper = self.helpers.address(self.builder.ins(), LaneHelper::FieldSet);
        let stored = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[object, atom, site, value]);
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
        // A static typed region's activation completes in its canonical code, at the
        // function's terminal `Return`, whose register takes the value.
        let terminal = self.unit.instructions.len().checked_sub(1)
            .filter(|terminal| self.unit.instructions[*terminal].op() == Op::Return
                && self.unit.instructions[*terminal].result_flags() == 0);
        if returns && (!matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_)))
            || (self.completion == Completion::SourcePc && terminal.is_none())) {
            return None;
        }
        let destination = match terminal {
            Some(terminal) if returns && self.completion == Completion::SourcePc => self.unit.instructions[terminal].register_a(),
            _ => instruction.result_register(),
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
            && let Some(next) = self.unit.instructions.get(pc + 1).copied()
            && next.result_flags() == 0
            && next.op() == Op::Binary
            && super::operations::NumberBitwise::from_js_instruction(next.op(), next.binary_operator())
                == Some(super::operations::NumberBitwise::Or)
            && next.operand_b().register_index() == Some(destination)
            && super::facts::constant_number(&self.program.constants, next.operand_c()) == Some(0.0)
            && self.blocks[pc + 1].is_none()
            && (next.result_register() == destination || !self.is_live(pc + 2, destination))
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
                // Operands proved int32 stay int32, as C keeps them: an overflow, or a product's
                // negative zero, is a value no int32 holds and leaves, rather than every result
                // becoming a double that the next operation converts back.
                let proved_int = [left, right].iter().all(|operand| match operand.kind() {
                    Some(OperandKind::Register) => state.values[usize::from(operand.payload())].value.ty.is_subtype(Type::INT32),
                    Some(OperandKind::Constant) => self.program.constants.get(usize::from(operand.payload()))
                        .and_then(super::facts::immediate).is_some_and(super::facts::is_int32),
                    _ => false,
                });
                if arithmetic == super::operations::NumberArithmetic::Divide || float_operand {
                    let left = self.operand_number(state, left, exit)?;
                    let right = self.operand_number(state, right, exit)?;
                    let number = emit_number_arithmetic_operation(&mut self.builder, arithmetic, left, right);
                    self.set_number(state, destination, number);
                } else if proved_int {
                    let left_int = self.operand_int(state, left, exit).expect("supported operand");
                    let right_int = self.operand_int(state, right, exit).expect("supported operand");
                    let (result, overflow) = arithmetic.apply(&mut EmitArithmetic::<true> { builder: &mut self.builder }, left_int, right_int);
                    // A sum or difference only `| 0` reads is its ToInt32: the wrapping int32
                    // result, which needs no overflow exit.
                    if !(matches!(arithmetic, super::operations::NumberArithmetic::Add | super::operations::NumberArithmetic::Subtract)
                        && self.truncated(pc, destination)) {
                        emit_js_guard_not(&mut self.builder, overflow, exit);
                    }
                    if arithmetic == super::operations::NumberArithmetic::Multiply {
                        let zero = self.ins().icmp_imm_u(IntCC::Equal, result, 0);
                        let signs = self.ins().bor(left_int, right_int);
                        let negative = self.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
                        let negative_zero = self.ins().band(zero, negative);
                        emit_js_guard_not(&mut self.builder, negative_zero, exit);
                    }
                    self.set_int(state, destination, result);
                } else if matches!(arithmetic, super::operations::NumberArithmetic::Add | super::operations::NumberArithmetic::Subtract)
                    && self.truncated(pc, destination) {
                    // Only its ToInt32 is read: int32 operands wrap, any other numbers take
                    // ToInt32 of their double result, and both join as an int32.
                    let float = self.builder.create_block();
                    self.builder.set_cold_block(float);
                    let mut float_state = state.clone();
                    let left_int = self.operand_int(state, left, float).expect("supported operand");
                    let right_int = self.operand_int(state, right, float).expect("supported operand");
                    let wrapped = if arithmetic == super::operations::NumberArithmetic::Add {
                        self.ins().iadd(left_int, right_int)
                    } else {
                        self.ins().isub(left_int, right_int)
                    };
                    let join = self.builder.create_block();
                    let int = self.builder.append_block_param(join, types::I32);
                    self.ins().jump(join, &[ir::BlockArg::Value(wrapped)]);
                    self.builder.switch_to_block(float);
                    let left_number = self.operand_number(&mut float_state, left, exit)?;
                    let right_number = self.operand_number(&mut float_state, right, exit)?;
                    let number = emit_number_arithmetic_operation(&mut self.builder, arithmetic, left_number, right_number);
                    let truncated = self.number_to_int32(number, exit);
                    self.ins().jump(join, &[ir::BlockArg::Value(truncated)]);
                    self.builder.switch_to_block(join);
                    self.join_states(state, &float_state);
                    self.set_int(state, destination, int);
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
                    self.join_states(state, &float_state);
                    self.set_number(state, destination, number);
                }
            }
            (_, Some(bitwise)) => {
                let left_int = self.operand_to_int32(state, left, exit)?;
                let right_int = self.operand_to_int32(state, right, exit)?;
                let right_constant = super::facts::constant_number(&self.program.constants, right);
                if !bitwise.result_is_int32(right_constant) {
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
                self.define(state, destination, boolean,
                    RegisterFact::definition(Type::language(crate::ValueType::Boolean), Representation::Tagged));
            }
        }
        if returns && self.completion == Completion::Number {
            self.return_number(state, destination, pc as u32);
            return Some(false);
        }
        if returns && self.completion == Completion::SourcePc {
            self.native += 1;
            self.continue_at(state, terminal.expect("checked above"));
            return Some(false);
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

/// Continue only when `condition` is false; `fallback` otherwise.
fn emit_js_guard_not(builder: &mut FunctionBuilder<'_>, condition: ir::Value, fallback: ir::Block) {
    let passed = builder.create_block();
    builder.ins().brif(condition, fallback, &[], passed, &[]);
    builder.switch_to_block(passed);
}

#[cfg(test)]
mod tests {
    #[test]
    fn call_effect_retires_nullable_shapes_without_losing_null_alternatives() {
        use super::*;
        let null = Type::language(crate::ValueType::Null);
        let mut state = State::canonical(1);
        state.values[0] = RegisterFact::definition(Type::direct_object(3).join(null), Representation::Tagged);
        state.call_effect();
        assert_eq!(state.values[0].value.ty, Type::DIRECT_OBJECT_FAMILY.join(null));
        assert_eq!(state.values[0].value.ty.direct_object_part(), Some(Type::DIRECT_OBJECT_FAMILY));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn typed_alternate_backedges_join_before_their_fault_poll() {
        let program = crate::Engine::specialize(
            "function subject(count,seed) { let n=count|0, x=seed|0; while(n>0) { n=(n-1)|0; if((x&1)===0) x=x>>1; else x=(x>>1)^13; } return x|0; }",
            "typed-poll-latch.js",
        ).unwrap();
        let id = program.functions.iter().position(|body| body.header().name
            .is_some_and(|name| &program.atoms[name as usize] == "subject")).unwrap();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let region = super::super::lower_static_typed_regions(&program, id as u32, &[], &*isa, 0)
            .into_iter().next().expect("typed loop region");
        cranelift_codegen::verify_function(&region.function, &*isa).unwrap();
        if !super::super::FAULT_POLL_SUPPORTED { return; }
        let native = region.function;
        let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(&native);
        let mut dominators = cranelift_codegen::dominator_tree::DominatorTree::new();
        dominators.compute(&native, &cfg);
        let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
        loops.compute(&native, &cfg, &dominators);
        assert_eq!(loops.loops().count(), 1, "{native}");
        let polls: Vec<_> = native.layout.blocks().filter(|block| loops.innermost_loop(*block).is_some())
            .flat_map(|block| native.layout.block_insts(block))
            .filter(|inst| native.dfg.insts[*inst].opcode() == ir::Opcode::AtomicLoad).collect();
        assert_eq!(polls.len(), 1, "{native}");
        let poll = native.layout.inst_block(polls[0]).unwrap();
        assert!(native.layout.blocks().any(|block| {
            loops.innermost_loop(block).is_some_and(|lp| loops.loop_header(lp) != block)
                && cfg.pred_iter(block).count() == 2
                && dominators.dominates(block, poll, &native.layout)
        }), "both arithmetic arms must join before the poll: {native}");
    }

    use super::*;

    #[test]
    fn callback_edges_retire_only_reachable_interrupt_ownership_proofs() {
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        for callback_returns_to_loop in [false, true] {
            let mut function = ir::Function::new();
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let blocks = [(); 4].map(|_| builder.create_block());
            let mut polls = smallvec::SmallVec::<[ir::Inst; INLINE_EFFECT_EDGES]>::new();
            for (index, block) in blocks.into_iter().enumerate() {
                builder.switch_to_block(block);
                let value = builder.ins().iconst(types::I8, 1);
                let ir::ValueDef::Result(inst, _) = builder.func.dfg.value_def(value) else { unreachable!() };
                polls.push(inst);
                match index {
                    0 => { builder.ins().jump(blocks[1], &[]); }
                    1 => { builder.ins().brif(value, blocks[1], &[], blocks[2], &[]); }
                    2 if callback_returns_to_loop => { builder.ins().brif(value, blocks[1], &[], blocks[3], &[]); }
                    2 => { builder.ins().jump(blocks[3], &[]); }
                    _ => { builder.ins().return_(&[]); }
                }
            }
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            for (edge, kept) in [(blocks[0], 0), (blocks[2], if callback_returns_to_loop { 1 } else { 2 }), (blocks[3], 3)] {
                let mut selected = polls.clone();
                retain_closed_polls(&function, &[edge], &mut selected);
                assert_eq!(selected.as_slice(), &polls[..kept]);
            }
        }
    }

    #[test]
    #[cfg(feature = "aot")]
    fn inlined_capture_reads_require_an_admitted_invariant_callee() {
        let source = "let held=4,suspended=2;let prototype={};prototype.test=function(){return (this.state&held)!==0||this.state===suspended;};function other(){return this.state===held;}function run(task,count){const owner=task;let n=count|0,result=0;while(n>0){n=(n-1)|0;owner.state=(owner.state+1)|0;if(owner.test())result=(result+1)|0;}return result;}";
        let program = crate::Engine::specialize(source, "capture-entry.js").unwrap();
        let named = |name: &str| program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
        let caller = named("run");
        let other = named("other") as u32;
        let targets = static_call_targets(&program);
        let unit_for = |polymorphic| super::super::lane_unit::unit(&program, caller as u32,
            program.function(caller), &|function, pc| {
                let mut found: Vec<_> = targets[function as usize].get(pc as usize).copied().flatten()
                    .map(|function| super::super::lane_unit::CallTarget { function, callee: CalleeBinding::Function })
                    .into_iter().collect();
                if polymorphic && !found.is_empty() {
                    found.push(super::super::lane_unit::CallTarget { function: other, callee: CalleeBinding::Function });
                }
                found
            }, &|id| program.function_get(id as usize)).unwrap();
        for polymorphic in [false, true] {
            let unit = unit_for(polymorphic);
            assert_eq!(unit.frames.len(), if polymorphic { 2 } else { 1 });
            let header = unit.instructions.iter().enumerate().filter_map(|(pc, instruction)|
                matches!(instruction.op().control_flow_layout(), ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump)
                    .then(|| instruction.jump_target() as usize).filter(|target| *target <= pc)).min().unwrap();
            let loops = local_loops(&unit, &program.constants, &[header]);
            let reads = invariant_reads(&unit, &loops);
            let captures: Vec<_> = reads.values().flatten().map(|read| read.pc).filter(|&pc|
                unit.instructions[pc].op() == Op::LoadCapture
                    && matches!(unit.origins[pc], super::super::lane_unit::Origin::Inlined { .. })).collect();
            assert_eq!(captures.len(), if polymorphic { 0 } else { 2 }, "{source}\n{}", program.disassemble());
            if polymorphic { continue; }
            let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
            let addresses = (*LaneHelper::addresses::<crate::test_host::TestHost>()).map(|address| address as usize);
            let region = lower(&program, caller as u32, &unit, LaneHelpers::Process(addresses),
                &BTreeMap::new(), &BTreeSet::new(), &BTreeMap::new(), &[], None, &*isa,
                Coverage::PrefixFrom(header), Completion::StatusAndPc).expect("capture loop region");
            let function = region.function;
            let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(&function);
            let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(&function, &cfg);
            let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
            loops.compute(&function, &cfg, &dom);
            let calls: Vec<_> = function.layout.blocks().flat_map(|block|
                function.layout.block_insts(block).map(move |inst| (block, inst))).filter(|&(_, inst)| {
                    if function.dfg.insts[inst].opcode() != ir::Opcode::CallIndirect { return false; }
                    let target = function.dfg.resolve_aliases(function.dfg.inst_args(inst)[0]);
                    let ir::ValueDef::Result(target, _) = function.dfg.value_def(target) else { return false };
                    matches!(function.dfg.insts[target], ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm }
                        if imm.bits() as usize == addresses[LaneHelper::FunctionCapture as usize])
                }).collect();
            assert_eq!(calls.len(), captures.len(), "one shared capture projection per entry read");
            for (block, _) in calls {
                assert!(loops.innermost_loop(block).is_none(), "capture admission must precede the loop");
            }
            let body: Vec<_> = function.layout.blocks().filter(|&block| loops.innermost_loop(block).is_some())
                .flat_map(|block| function.layout.block_insts(block)).collect();
            assert!(!body.iter().any(|&inst| matches!(function.dfg.insts[inst],
                ir::InstructionData::FloatCompare { cond: FloatCC::Equal, .. })),
                "the admitted equality capture must not decode or compare f64 in the loop: {function}");
            let entry_projection = |value| {
                let mut pending = vec![value];
                let mut seen = BTreeSet::new();
                let mut origin = None;
                while let Some(value) = pending.pop() {
                    let value = function.dfg.resolve_aliases(value);
                    if !seen.insert(value) { continue; }
                    match function.dfg.value_def(value) {
                        ir::ValueDef::Param(block, index) => {
                            let before = pending.len();
                            for predecessor in cfg.pred_iter(block) {
                                for branch in function.dfg.insts[predecessor.inst]
                                    .branch_destination(&function.dfg.jump_tables, &function.dfg.exception_tables) {
                                    if branch.block(&function.dfg.value_lists) == block {
                                        let Some(value) = branch.args(&function.dfg.value_lists).nth(index)
                                            .and_then(|arg| arg.as_value()) else { return false };
                                        pending.push(value);
                                    }
                                }
                            }
                            if pending.len() == before { return false; }
                        }
                        ir::ValueDef::Result(inst, _) if function.dfg.insts[inst].opcode() == ir::Opcode::Ireduce
                            && function.layout.inst_block(inst).is_some_and(|block| loops.innermost_loop(block).is_none()) => {
                                if origin.is_some_and(|other| other != value) { return false; }
                                origin = Some(value);
                            }
                        _ => return false,
                    }
                }
                origin.is_some()
            };
            assert!(body.iter().any(|&inst| {
                let ir::InstructionData::IntCompare { cond: IntCC::Equal, args, .. } = function.dfg.insts[inst]
                    else { return false };
                args.into_iter().any(|value| function.dfg.value_type(value) == types::I32 && entry_projection(value))
            }), "an entry-projected I32 capture must feed the native equality: {function}");
        }
    }

    #[test]
    #[cfg(feature = "aot")]
    fn inductive_accumulator_keeps_the_predicate_loop_unboxed() {
        let source = r#"(function(){
            let mask=4,threshold=2;
            class Cell{constructor(n,value){this.left=n|0;this.word=value|0;}
                accepts(){return (this.word&mask)!==0||this.word===threshold;}}
            function fold(n,value){const cell=new Cell(n,value);let total=0;
                while(cell.left>0){cell.left=(cell.left-1)|0;
                    cell.word=(cell.word>>1)^((cell.word&1)?21:0);
                    if(cell.accepts())total=(total+1)|0;}return total|0;}
        })();"#;
        for typescript in [false, true] {
            let program = if typescript { crate::Engine::specialize_typescript(source, "inductive-predicate.cts") }
                else { crate::Engine::specialize(source, "inductive-predicate.cjs") }.unwrap();
            let caller = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "fold")).unwrap();
            let targets = static_call_targets(&program);
            let unit = super::super::lane_unit::unit(&program, caller as u32, program.function(caller),
                &|function, pc| targets[function as usize].get(pc as usize).copied().flatten()
                    .map(|function| super::super::lane_unit::CallTarget { function, callee: CalleeBinding::Function })
                    .into_iter().collect(), &|id| program.function_get(id as usize)).unwrap();
            assert_eq!(unit.frames.len(), 1, "the Boolean method must inline");
            let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
            let regions = lower_static_typed_regions(&program, caller as u32, &targets[caller], &*isa, 0);
            assert!(!regions.is_empty(), "selected caller header must publish");
            let mut integer_add = false;
            for region in regions {
                let function = region.function;
                cranelift_codegen::verify_function(&function, &*isa).unwrap();
                let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(&function);
                let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(&function, &cfg);
                let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
                loops.compute(&function, &cfg, &dom);
                for block in function.layout.blocks().filter(|block| loops.innermost_loop(*block).is_some()) {
                    for inst in function.layout.block_insts(block) {
                        integer_add |= function.dfg.insts[inst].opcode() == ir::Opcode::Iadd
                            && function.dfg.inst_results(inst).first().is_some_and(|value| function.dfg.value_type(*value) == types::I32);
                        if let ir::InstructionData::IntCompare { cond: IntCC::Equal, args, .. } = function.dfg.insts[inst] {
                            assert!(!args.into_iter().any(|value| {
                                let ir::ValueDef::Result(def, _) = function.dfg.value_def(function.dfg.resolve_aliases(value)) else { return false };
                                matches!(function.dfg.insts[def], ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm }
                                    if imm.bits() as u64 == crate::value::ValueLayout::INT_TAG >> crate::value::ValueLayout::TAG_SHIFT)
                            }), "integer tag admission must leave the loop: {function}");
                        }
                    }
                }
            }
            assert!(integer_add, "the wrapping accumulator update must remain native I32");
        }
    }

    #[test]
    fn inductive_entry_preserves_misses_wrap_and_cold_checkpoints() {
        let program = crate::Engine::specialize(
            "function scan(n,seed,divisor){let acc=seed,i=0;while(i<n){i=(i+1)|0;if((i&1)!==0)acc=(acc+1)|0;let probe=(1/divisor)|0;}return acc;}",
            "inductive-entry.js").unwrap();
        let function = program.function(1);
        let unit = super::super::lane_unit::unit(&program, 1, function, &|_, _| Vec::new(), &|_| None).unwrap();
        let header = unit.instructions.iter().enumerate().filter_map(|(pc, instruction)|
            matches!(instruction.op().control_flow_layout(), ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump)
                .then(|| instruction.jump_target() as usize).filter(|target| *target <= pc)).min().unwrap();
        let induction = unit.instructions[header].operand_b().register_index().unwrap();
        let limit = unit.instructions[header].operand_c().register_index().unwrap();
        let accumulator = unit.instructions.iter().find(|instruction| instruction.op() == Op::Return).unwrap().register_a();
        let divisor = unit.instructions.iter().find(|instruction| instruction.op().js_denotation() == JsDenotation::NumericBinary
            && super::super::super::operations::NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator())
                == Some(super::super::super::operations::NumberArithmetic::Divide)).unwrap().operand_c().register_index().unwrap();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let helpers = LaneHelpers::Process((*LaneHelper::addresses::<crate::test_host::TestHost>()).map(|address| address as usize));
        let region = lower(&program, 1, &unit, helpers, &BTreeMap::new(), &BTreeSet::new(), &BTreeMap::new(),
            &[], None, &*isa, Coverage::Osr(header), Completion::StatusAndPc).expect("inductive native loop");
        let fault_poll = region.fault_poll;
        let code = compile_native_function(region.function, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: this code uses the shared entry ABI, and the mapping remains owned here.
        let entry = unsafe { std::mem::transmute::<*mut u8, super::super::Entry>(code.pointer()) };
        let mut frame = super::super::LaneFrame::new();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        frame.dispatch.refresh_object_bases(&vm);
        frame.dispatch.bind_native_storage(&mut vm);
        frame.context.adapter_payload = (&mut frame.dispatch as *mut JsStepDispatch).cast();
        frame.context.interrupt = vm.execution_interrupt.atomic_flag() as *const _;
        frame.context.interrupt_owner = &vm.execution_interrupt;
        if fault_poll {
            let (start, end) = super::super::native_poll_page(&vm.execution_interrupt).expect("owned poll capability");
            (frame.context.poll_start, frame.context.poll_end) = (start, end);
        }
        let inputs = |seed, count, division| {
            let mut registers = vec![Value::UNDEFINED; usize::from(unit.caller_registers)];
            registers[usize::from(accumulator)] = seed;
            registers[usize::from(induction)] = Value::integer(0);
            registers[usize::from(limit)] = Value::integer(count);
            registers[usize::from(divisor)] = Value::integer(division);
            registers
        };
        for count in [0, 7] {
            for seed in [Value::number(-0.0), Value::number(1.5), Value::number(2147483648.0),
                Value::number(f64::NAN), Value::number(f64::INFINITY), Value::number(f64::NEG_INFINITY),
                Value::UNDEFINED, Value::NULL, Value::TRUE] {
                let mut registers = inputs(seed, count, 1);
                let before = registers.clone();
                let mut word = u64::MAX;
                // SAFETY: frame, input homes and output remain live through the resumable entry.
                let status = unsafe { super::super::enter_native(&mut frame.context, 1, 0,
                    registers.as_mut_ptr().cast(), &mut word, NativeEntryMode::Resumable, entry) };
                assert_eq!((status, word), (NATIVE_JS_STOPPED, header as u64), "{seed:?}, count={count}");
                assert_eq!(registers, before, "entry miss must preserve every canonical input");
            }
        }
        for seed in [0_i32, i32::MAX, i32::MIN] {
            let mut registers = inputs(Value::integer(seed), 7, 1);
            let mut word = 0;
            let status = unsafe { super::super::enter_native(&mut frame.context, 1, 0,
                registers.as_mut_ptr().cast(), &mut word, NativeEntryMode::Resumable, entry) };
            assert_eq!(status, LANE_BODY_RETURNED);
            assert_eq!(Value::from_bits(word), Value::integer(seed.wrapping_add(4)));
        }
        let mut registers = inputs(Value::integer(i32::MAX), 7, 0);
        let mut word = 0;
        let status = unsafe { super::super::enter_native(&mut frame.context, 1, 0,
            registers.as_mut_ptr().cast(), &mut word, NativeEntryMode::Resumable, entry) };
        assert_eq!(status, NATIVE_JS_STOPPED);
        assert!(word > header as u64, "the unsupported infinity projection exits after the first update");
        assert_eq!(registers[usize::from(accumulator)], Value::integer(i32::MIN));
        assert_eq!(registers[usize::from(induction)], Value::integer(1));
    }

    #[test]
    fn relation_capture_entry_preserves_noninteger_values_and_effects() {
        for (operator, strict) in [("==", false), ("===", true)] {
            for reversed in [false, true] {
                let comparison = if reversed { format!("limit{operator}this.bits") }
                    else { format!("this.bits{operator}limit") };
                let source = format!(r#"
                    (function(){{
                        function make(limit){{class Entry{{constructor(){{this.bits=0;}}probe(){{return {comparison};}}}}
                            return {{entry:new Entry(),set(value){{limit=value;}},read(){{return limit;}}}};}}
                        function run(entry,count){{const owner=entry;let n=count|0,result=0;
                            while(n>0){{n=(n-1)|0;owner.bits=(owner.bits+1)|0;if(owner.probe())result=(result+1)|0;}}return result;}}
                        function expected(value){{let total=0;for(let i=1;i<=7;i++)if(i{operator}value)total++;return total;}}
                        const bundle=make(2);
                        for(let warm=0;warm<96;warm++){{bundle.entry.bits=0;if(run(bundle.entry,7)!==1)throw 'warm';}}
                        for(const value of [0,-0,1,2,2.5,NaN,Infinity,-Infinity,2n,'2',true,null,undefined,Symbol()]){{
                            bundle.set(value);bundle.entry.bits=0;
                            if(run(bundle.entry,7)!==expected(value)||bundle.entry.bits!==7||!Object.is(bundle.read(),value))throw 'capture value';
                        }}
                        let effects=0;
                        bundle.set({{valueOf(){{effects++;return 2;}}}});bundle.entry.bits=0;
                        if(run(bundle.entry,7)!=={}||effects!=={}||bundle.entry.bits!==7)throw 'coercion';
                        effects=0;bundle.set({{valueOf(){{effects++;bundle.set(3);return 2;}}}});bundle.entry.bits=0;
                        if(run(bundle.entry,7)!=={}||effects!=={}||bundle.entry.bits!==7)throw 'coercion mutation';
                        bundle.set({{valueOf(){{throw 'zero-trip coercion';}}}});bundle.entry.bits=0;
                        if(run(bundle.entry,0)!==0||bundle.entry.bits!==0)throw 'zero-trip';
                        bundle.set(2);bundle.entry.bits=0;if(run(bundle.entry,7)!==1)throw 'integer reentry';
                        function cold(count){{class Entry{{constructor(){{this.bits=0;}}probe(){{return this.bits{operator}hidden;}}}}
                            const result=run(new Entry(),count);let hidden=2;return result;}}
                        if(cold(0)!==0)throw 'zero-trip tdz';let caught=false;
                        try{{cold(1);}}catch(error){{caught=error instanceof ReferenceError;}}if(!caught)throw 'tdz';
                        throw 42;
                    }})();
                "#, usize::from(!strict), if strict { 0 } else { 7 }, usize::from(!strict), usize::from(!strict));
                for typescript in [false, true] {
                    let program = if typescript { crate::Engine::specialize_typescript(&source, "relation-capture.cts") }
                        else { crate::Engine::specialize(&source, "relation-capture.cjs") }.unwrap();
                    for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
                        let mut vm = Vm::new(crate::test_host::TestHost::default());
                        vm.set_execution_mode(mode);
                        vm.programs.enable_js_lane_methods();
                        let error = vm.execute(&program).unwrap_err();
                        assert_eq!(error.thrown_value(), Some(Value::integer(42)),
                            "{operator}, reverse={reversed}, TS={typescript}, {mode:?}: {error}");
                    }
                }
            }
        }
    }

    #[test]
    #[cfg(feature = "aot")]
    fn capture_entry_does_not_follow_a_copy_across_a_diamond() {
        let source = r#"
            let held=4;const prototype={};prototype.test=function(){return held|0;};
            function fieldChoice(a,b,flag,count){const first=a,second=b;let n=count|0,result=0;
                while(n>0){n=(n-1)|0;let selected=first;if(flag)selected=second;
                    if(selected.test())result=(result+1)|0;}return result;}
            function calleeChoice(a,b,flag,count){const first=a,second=b;let n=count|0,result=0;
                while(n>0){n=(n-1)|0;let selected=first.test;if(flag)selected=second.test;
                    if(selected())result=(result+1)|0;}return result;}
        "#;
        let program = crate::Engine::specialize(source, "capture-diamond.js").unwrap();
        let targets = static_call_targets(&program);
        for name in ["fieldChoice", "calleeChoice"] {
            let caller = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
            let unit = super::super::lane_unit::unit(&program, caller as u32, program.function(caller),
                &|function, pc| targets[function as usize].get(pc as usize).copied().flatten()
                    .map(|function| super::super::lane_unit::CallTarget { function, callee: CalleeBinding::Function })
                    .into_iter().collect(), &|id| program.function_get(id as usize)).unwrap();
            assert_eq!(unit.frames.len(), 1, "exercise an actual inline marker: {name}");
            let loops = local_loops(&unit, &program.constants, &[0]);
            assert!(!loops.is_empty(), "exercise an eligible loop: {name}");
            let reads = invariant_reads(&unit, &loops);
            assert!(!reads.values().flatten().any(|read|
                unit.instructions[read.pc].op() == Op::LoadCapture
                    && matches!(unit.origins[read.pc], super::super::lane_unit::Origin::Inlined { .. })),
                "a skipped copy cannot prove the closure environment: {name}\n{}", program.disassemble());
        }
    }

    #[test]
    fn capture_entry_preserves_distinct_environments_and_mutations() {
        let source = r#"
            function make(held,suspended){
                class State {constructor(state){this.state=state;}
                    test(){return (this.state&held)!==0||this.state===suspended;}}
                return {task:new State(5),set(a,b){held=a;suspended=b;}};
            }
            function run(task,count){let result=0;while(count>0){count=(count-1)|0;
                task.state=(task.state+1)|0;if(task.test())result=(result+1)|0;}return result;}
            function expected(state,count,held,suspended){let result=0;for(let i=0;i<count;i++){
                state=(state+1)|0;if((state&held)!==0||state===suspended)result++;}return result;}
            const a=make(4,2),b=make(8,3);
            for(let warm=0;warm<96;warm++){run(a.task,17);run(b.task,17);}
            for(let mask=0;mask<16;mask++){a.set(mask,7);a.task.state=0;b.task.state=0;
                if(run(a.task,257)!==expected(0,257,mask,7)||run(b.task,257)!==expected(0,257,8,3))throw 'environment';}
            let effects=0;a.set({valueOf(){effects++;return 4;}},2);a.task.state=0;
            if(run(a.task,17)!==expected(0,17,4,2)||effects!==17)throw 'coercion';
            function tdz(count){const task={state:0,test(){return (this.state&held)!==0;}};
                const result=run(task,count);let held=4;return result;}
            if(tdz(0)!==0)throw 'zero-trip';
            let caught=false;try{tdz(1);}catch(error){caught=error instanceof ReferenceError;}
            if(!caught)throw 'tdz';throw 42;
        "#;
        // CommonJS admits this source inside a function environment. Keep that
        // capture ownership/layout rather than testing only global bindings.
        let source = format!("(function(exports,require,module,__filename,__dirname){{{source}}})(undefined,undefined,undefined,undefined,undefined);");
        for typescript in [false, true] {
            let program = if typescript { crate::Engine::specialize_typescript(&source, "capture-entry.cts") }
                else { crate::Engine::specialize(&source, "capture-entry.cjs") }.unwrap();
            #[cfg(feature = "aot")]
            {
                // Exercise the linked unit/inliner path too: execution through
                // the dynamic VM alone need not select the same region entries.
                let module = crate::lower_native_js_module(&program, "capture_entry").unwrap();
                let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
                for function in module.functions {
                    if let super::super::super::native_object::NativeFunction::Definition { function, .. } = function {
                        super::super::super::jit_backend::compile_function(function, &*isa, None, |_, _| Ok(())).unwrap();
                    }
                }
            }
            for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
                let mut vm = Vm::new(crate::test_host::TestHost::default());
                vm.set_execution_mode(mode);
                vm.programs.enable_js_jit_diagnostics();
                vm.programs.enable_js_lane_methods();
                let error = vm.execute(&program).unwrap_err();
                assert_eq!(error.thrown_value(), Some(Value::integer(42)), "{mode:?}: {error}");
                #[cfg(not(quench_profiled))]
                if mode == crate::ExecutionMode::Compiled {
                    let caller = program.functions.iter().position(|body|
                        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "run")).unwrap();
                    let view = vm.programs.lane_views(super::super::super::program_store::ProgramId::MAIN).unwrap()
                        .get(caller).and_then(super::super::super::program_store::derived_lane_view).unwrap();
                    let report = view.body_stats.report(0).expect("native predicate loop must publish");
                    assert!(report.contains("\"frames\":2"), "exercise both closure alternatives: {report}");
                    assert!(!report.contains("\"completed\":0,"), "predicate loop must complete natively: {report}");
                }
            }
        }
    }

    #[test]
    fn comparison_boolean_proofs_survive_branch_joins() {
        let program = crate::Engine::specialize(
            "function predicate(x){let result;if(x<0)result=x===-1;else result=x===1;if(result)return 7;return 9;}",
            "boolean-join.js").unwrap();
        let unit = super::super::lane_unit::unit(&program, 1, program.function(1), &|_, _| Vec::new(), &|_| None).unwrap();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let region = lower(&program, 1, &unit, LaneHelpers::Process([0; LaneHelper::COUNT]),
            &BTreeMap::new(), &BTreeSet::new(), &BTreeMap::new(), &[], None, &*isa,
            Coverage::Full, Completion::StatusAndPc).unwrap();
        let code = compile_native_function(region.function, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: the lowered function uses the shared lane-entry ABI and only live frame/register storage.
        let entry = unsafe { std::mem::transmute::<*mut u8, super::super::Entry>(code.pointer()) };
        let mut frame = super::super::LaneFrame::new();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        frame.dispatch.refresh_object_bases(&vm);
        frame.dispatch.bind_native_storage(&mut vm);
        frame.dispatch.receiver = Value::UNDEFINED.to_bits();
        frame.context.adapter_payload = (&mut frame.dispatch as *mut JsStepDispatch).cast();
        for input in [-2.0, -1.0, 0.0, 1.0, 2.0] {
            let mut registers = vec![Value::UNDEFINED; usize::from(unit.caller_registers)];
            registers[0] = Value::number(input);
            let mut word = 0;
            let status = unsafe { entry(&mut frame.context, 0, 0, registers.as_mut_ptr().cast(), &mut word) };
            assert_eq!(status, LANE_BODY_RETURNED, "comparison result must continue natively: {input}, stopped at {word}");
            assert_eq!(Value::from_bits(word), Value::number(if input == -1.0 || input == 1.0 { 7.0 } else { 9.0 }));
        }
        // This region selects an int32 entry view. Other Number representations
        // must recover before any guest operation, preserving their exact bits.
        for input in [-0.0, f64::NEG_INFINITY, f64::INFINITY, f64::NAN] {
            let mut registers = vec![Value::UNDEFINED; usize::from(unit.caller_registers)];
            registers[0] = Value::number(input);
            let before = registers.clone();
            let mut word = u64::MAX;
            let status = unsafe { entry(&mut frame.context, 0, 0, registers.as_mut_ptr().cast(), &mut word) };
            assert_eq!((status, word), (NATIVE_JS_STOPPED, 0));
            assert_eq!(registers, before);
        }
    }

    #[test]
    fn osr_first_entry_bypasses_selected_header_proofs() {
        let program = crate::Engine::specialize(
            "function count(n){let limit=n|0,sum=0;for(let i=0;i<limit;i=(i+1)|0)sum=(sum+1)|0;return sum;}",
            "osr-entry.js").unwrap();
        let function = program.function(1);
        let unit = super::super::lane_unit::unit(&program, 1, function, &|_, _| Vec::new(), &|_| None).unwrap();
        let header = unit.instructions.iter().enumerate().find_map(|(pc, instruction)| {
            if instruction.op() != Op::Jump { return None; }
            let target = instruction.jump_target() as usize;
            (target <= pc).then_some(target)
        }).unwrap();
        assert_ne!(header, 0);
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let helpers = LaneHelpers::Process((*LaneHelper::addresses::<crate::test_host::TestHost>()).map(|address| address as usize));
        let region = lower(&program, 1, &unit, helpers, &BTreeMap::new(), &BTreeSet::new(), &BTreeMap::new(), &[], None,
            &*isa, Coverage::Osr(header), Completion::StatusAndPc).unwrap();
        let code = compile_native_function(region.function, &*isa, usize::MAX, CodeMemory::new).unwrap();
        let entry = unsafe { std::mem::transmute::<*mut u8, super::super::Entry>(code.pointer()) };
        let mut frame = super::super::LaneFrame::new();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        frame.dispatch.refresh_object_bases(&vm);
        frame.dispatch.bind_native_storage(&mut vm);
        frame.dispatch.receiver = Value::UNDEFINED.to_bits();
        frame.context.adapter_payload = (&mut frame.dispatch as *mut JsStepDispatch).cast();
        frame.context.interrupt = vm.execution_interrupt.atomic_flag() as *const _;
        let before = vec![Value::UNDEFINED; usize::from(unit.caller_registers)];
        for (start, expected) in [(0, 0), (1, header as u64)] {
            let mut registers = before.clone();
            let mut word = u64::MAX;
            let status = unsafe { entry(&mut frame.context, start, 0, registers.as_mut_ptr().cast(), &mut word) };
            assert_eq!(status, NATIVE_JS_STOPPED);
            assert_eq!(word, expected);
            assert_eq!(registers, before, "admission must not alter canonical inputs");
        }
    }

    #[test]
    fn native_to_int32_keeps_number_inputs_unboxed_and_misses_exactly() {
        let program = crate::Engine::specialize(
            "function words(x){x=x|0;return x|0;}", "unboxed-word-input.js").unwrap();
        let unit = super::super::lane_unit::unit(&program, 1, program.function(1), &|_, _| Vec::new(), &|_| None).unwrap();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let region = lower(&program, 1, &unit, LaneHelpers::Process([0; LaneHelper::COUNT]),
            &BTreeMap::new(), &BTreeSet::new(), &BTreeMap::new(), &[], None, &*isa,
            Coverage::Full, Completion::Number).unwrap();
        assert_eq!(region.number_result, Some(NativeNumberResult::Int32));
        for instruction in region.function.layout.blocks()
            .flat_map(|block| region.function.layout.block_insts(block)) {
            if let ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } = region.function.dfg.insts[instruction] {
                let bits = i64::from(imm) as u64;
                assert!(![crate::value::ValueLayout::INT_TAG, crate::value::ValueLayout::CANONICAL_NAN].contains(&bits),
                    "verified Number inputs must stay unboxed:\n{}", region.function);
            }
            if region.function.dfg.insts[instruction].opcode() == ir::Opcode::Store {
                let value = region.function.dfg.inst_args(instruction)[0];
                assert_eq!(region.function.dfg.value_type(value), types::I64, "int32 result must not round trip through f64");
            }
        }
        let code = compile_native_function(region.function, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: the region has the shared entry ABI and borrows one raw Number.
        let entry = unsafe { std::mem::transmute::<*mut u8, super::super::Entry>(code.pointer()) };
        let interrupt = crate::ExecutionInterrupt::default();
        let mut context = CallContext::interrupt_only(&interrupt);
        let boundary = [0.0, -0.0, 1.0, -1.0, f64::from(i32::MIN), f64::from(i32::MAX),
            f64::from(i32::MIN) - 1.0, f64::from(i32::MAX) + 1.0,
            1.5, -1.5, f64::MIN_POSITIVE, f64::from_bits(1), -f64::from_bits(1),
            f64::INFINITY, f64::NEG_INFINITY, f64::NAN, f64::MAX, -f64::MAX,
            EXACT_I64_TRUNCATION_LIMIT, -EXACT_I64_TRUNCATION_LIMIT,
            EXACT_I64_TRUNCATION_LIMIT.next_down(), -EXACT_I64_TRUNCATION_LIMIT.next_down()];
        let mut random = 0x517c_c1b7_2722_0a95_u64;
        let samples = boundary.into_iter().chain((0..256).map(|_| {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            f64::from_bits(random)
        }));
        for number in samples {
            let input = [number.to_bits()];
            let mut out = u64::MAX;
            let status = unsafe { entry(&mut context, 0, 0, input.as_ptr(), &mut out) };
            if number.abs() < EXACT_I64_TRUNCATION_LIMIT {
                    assert_eq!(status, 0, "{number:?}");
                    assert_eq!(out, u64::from(crate::value::number_to_u32(number)), "{number:?}");
            } else {
                    assert_eq!(i64::from(status), NUMBER_ENTRY_MISS, "{number:?}");
                    assert_eq!(out, u64::MAX, "miss may not publish a result");
            }
            assert_eq!(input[0], number.to_bits(), "borrowed Number is never canonicalized in place");
        }
    }

    #[test]
    fn unavailable_fault_page_declines_before_changing_canonical_registers() {
        if !super::super::FAULT_POLL_SUPPORTED { return; }
        let program = crate::Engine::specialize(
            "function count(n){n=n|0;let sum=0;for(let i=0;i<n;i=(i+1)|0)sum=(sum+1)|0;return sum;}",
            "missing-poll.js").unwrap();
        let function = program.function(1);
        let unit = super::super::lane_unit::unit(&program, 1, function, &|_, _| Vec::new(), &|_| None).unwrap();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let helpers = LaneHelpers::Process((*LaneHelper::addresses::<crate::test_host::TestHost>()).map(|address| address as usize));
        let region = lower(&program, 1, &unit, helpers, &BTreeMap::new(), &BTreeSet::new(), &BTreeMap::new(), &[], None, &*isa, Coverage::Full, Completion::StatusAndPc).unwrap();
        assert!(region.fault_poll);
        let code = compile_native_function(region.function, &*isa, usize::MAX, CodeMemory::new).unwrap();
        let entry = unsafe { std::mem::transmute::<*mut u8, super::super::Entry>(code.pointer()) };
        let mut frame = super::super::LaneFrame::new();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        frame.dispatch.refresh_object_bases(&vm);
        frame.dispatch.bind_native_storage(&mut vm);
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
        let fields: BTreeMap<u16, Vec<NativeFieldLoad>> = function.instructions().filter_map(|instruction| {
            if instruction.op() == Op::GetField
                && let Some(FieldLookup::Atom { atom, cache_site, .. }) = instruction.field_lookup()
                && let Some(slot) = ["left", "right"].iter().position(|name| *name == &program.atoms[atom as usize]) {
                Some((cache_site, vec![NativeFieldLoad { shape: 0, holder: None, slot: slot as u16, storage_len: 2, storage: None, ty: Type::ANY, target_shape: None }]))
            } else { None }
        }).collect();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let unit = super::super::lane_unit::unit(&program, 1, function, &|_, _| Vec::new(), &|_| None).unwrap();
        let region = lower(&program, 1, &unit, LaneHelpers::Process([0; LaneHelper::COUNT]), &fields, &BTreeSet::new(),
            &BTreeMap::new(), &[], None, &*isa, Coverage::Prefix, Completion::StatusAndPc).unwrap();
        let native = region.function;
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
        let fields: BTreeMap<u16, Vec<NativeFieldLoad>> = program.function(id).instructions().filter_map(|instruction| {
            if instruction.op() == Op::GetField
                && let Some(FieldLookup::Atom { atom, cache_site, .. }) = instruction.field_lookup()
                && let Some(slot) = ["left", "right"].iter().position(|name| *name == &program.atoms[atom as usize]) {
                Some((cache_site, vec![NativeFieldLoad { shape: 0, holder: None, slot: slot as u16, storage_len: 2, storage: None, target_shape: None,
                    ty: Type::ANY }]))
            } else { None }
        }).collect();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let unit = super::super::lane_unit::unit(&program, id as u32, program.function(id), &|_, _| Vec::new(), &|_| None).unwrap();
        let region = lower(&program, id as u32, &unit, LaneHelpers::Process([0; LaneHelper::COUNT]), &fields, &BTreeSet::new(),
            &BTreeMap::new(), &[], None, &*isa, Coverage::Prefix, Completion::StatusAndPc).unwrap();
        let native = region.function;
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
        let unit = super::super::lane_unit::unit(&program, id as u32, program.function(id), &|_, _| Vec::new(), &|_| None).unwrap();
        let region = lower(&program, id as u32, &unit, LaneHelpers::Process([0; LaneHelper::COUNT]),
            &BTreeMap::new(), &BTreeSet::new(), &BTreeMap::new(), &[], None, &*isa, Coverage::Prefix, Completion::StatusAndPc).expect("typed Number residual");
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
