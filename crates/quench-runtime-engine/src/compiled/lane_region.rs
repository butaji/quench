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
#[allow(unused_imports)]
use crate::ext_traits::*;
use crate::compiled::regions;
use super::*;
use quench_runtime::vm::specializer::Specializer;
use quench_runtime::typed_ir::{BindingTime, Fact, Projection, Proof, Reentry, Representation, Type, VerifiedFact};
use super::object_slots::{emit, CheckedObject, CheckedObjectTable, Emit, ObjectHeader, ObjectRecord, ObjectTable};
use quench_runtime::typed_ir::Division;
use quench_runtime::staged::typed_elements::{NativeKind, ViewKind};
use quench_runtime::staged::typed_view::{prove_typed_view, TypedRecords, ViewAccess};
use std::collections::{BTreeMap, BTreeSet};
/// The direct callees of a body's call sites, by unit PC.
/// The direct callees of each call site, by PC, in the order a chain of identity tests takes them
/// (`Lowering::polymorphic_call`); one for a site that is not polymorphic.
pub(crate) type DirectSites = BTreeMap<usize, Vec<*const DirectCallee>>;
use cranelift_codegen::bitset::CompoundBitSet;
use cranelift_codegen::entity::EntityRef;

#[path = "lane_region/known_branches.rs"]
pub(crate) mod known_branches;
#[path = "lane_region/clif_census.rs"]
pub(crate) mod clif_census;
#[path = "lane_region/packed_ranges.rs"]
pub(crate) mod packed_ranges;
#[path = "lane_region/int_ranges.rs"]
pub(crate) mod int_ranges;
#[cfg(test)]
#[path = "lane_region/int_ranges_tests.rs"]
pub(crate) mod int_ranges_tests;
#[path = "lane_region/global_reads.rs"]
pub(crate) mod global_reads;
#[path = "lane_region/native_calls.rs"] pub(crate) mod native_calls;
#[path = "lane_region/builtin_calls.rs"] pub(crate) mod builtin_calls;
#[path = "lane_region/data_view_calls.rs"] pub(crate) mod data_view_calls;
pub(crate) use native_calls::{inlines_in_loop, inlines_in_loop_call, native_method_call, IN_PLACE_CALL};
#[path = "lane_region/stable.rs"]
pub(crate) mod stable;
#[path = "lane_region/polls.rs"]
pub(crate) mod polls;
#[path = "lane_region/scalar.rs"] pub(crate) mod scalar;
#[path = "lane_region/trips.rs"] pub(crate) mod trips;
#[path = "lane_region/function_names.rs"] pub(crate) mod function_names;
#[path = "lane_region/literals.rs"] pub(crate) mod literals;
#[path = "lane_region/allocations.rs"] pub(crate) mod allocations;
#[path = "lane_region/edges.rs"] pub(crate) mod edges;
#[path = "lane_region/demand.rs"] pub(crate) mod demand;
#[path = "lane_region/arithmetic.rs"] pub(crate) mod arithmetic;
#[path = "lane_region/index_math.rs"] pub(crate) mod index_math;
#[path = "lane_region/int_inputs.rs"] pub(crate) mod int_inputs;
#[path = "lane_region/loop_versions.rs"] pub(crate) mod loop_versions;
#[path = "lane_region/exits.rs"] pub(crate) mod exits;
#[cfg(test)] #[path = "lane_region/exit_tests.rs"] pub(crate) mod exit_tests;
#[cfg(test)] #[path = "lane_region/home_tests.rs"] pub(crate) mod home_tests;
#[path = "lane_region/catches.rs"] pub(crate) mod catches;
#[path = "lane_region/store_domains.rs"] pub(crate) mod store_domains;
pub(crate) use store_domains::store_domains;
#[path = "lane_region/arguments.rs"] pub(crate) mod arguments;
#[path = "lane_region/captures.rs"] pub(crate) mod captures;
#[path = "lane_region/compare_jumps.rs"] pub(crate) mod compare_jumps;
#[path = "lane_region/relocs.rs"] pub(crate) mod relocs;
use relocs::RelocKind; pub(crate) use relocs::{atom_symbol, numeric_symbol};
use captures::RootedRead;
#[path = "lane_region/constructions.rs"] pub(crate) mod constructions;
#[path = "lane_region/virtual_objects.rs"] pub(crate) mod virtual_objects;
#[path = "lane_region/dense_elements.rs"] pub(crate) mod dense_elements;
#[path = "lane_region/dense_appends.rs"] pub(crate) mod dense_appends;
#[path = "lane_region/bindings.rs"] pub(crate) mod bindings;
#[path = "lane_region/frame.rs"] pub(crate) mod frame;
#[path = "lane_region/frame_fields.rs"] pub(crate) mod frame_fields;
#[path = "lane_region/frame_elements.rs"] pub(crate) mod frame_elements;
#[path = "lane_region/frame_stores.rs"] pub(crate) mod frame_stores;
#[path = "lane_region/field_plans.rs"] pub(crate) mod field_plans;
#[path = "lane_region/headers.rs"] pub(crate) mod headers;
#[path = "lane_region/hoisted_reads.rs"] pub(crate) mod hoisted_reads;
#[path = "lane_region/field_reads.rs"] pub(crate) mod field_reads;
#[path = "lane_region/own_constants.rs"] pub(crate) mod own_constants;
#[path = "lane_region/number_domains.rs"] pub(crate) mod number_domains;
#[path = "lane_region/loop_arrays.rs"] pub(crate) mod loop_arrays;
#[path = "lane_region/loop_strings.rs"] pub(crate) mod loop_strings;
#[path = "lane_region/call_frames.rs"] pub(crate) mod call_frames;
#[path = "lane_region/declared.rs"] pub(crate) mod declared;
use call_frames::{CallSite, DirectSite, Leave, ReturnSite, Site};
pub(crate) use call_frames::ProvenCall;
#[path = "lane_region/call_outs.rs"] pub(crate) mod call_outs;
#[path = "lane_region/jumps.rs"] pub(crate) mod jumps;
#[path = "lane_region/call_rows.rs"] pub(crate) mod call_rows;
#[path = "lane_region/register_rows.rs"] pub(crate) mod register_rows;
#[path = "lane_region/window_locals.rs"] pub(crate) mod window_locals;
#[path = "lane_region/element_rows.rs"] pub(crate) mod element_rows;
#[path = "lane_region/fixed_arrays.rs"] pub(crate) mod fixed_arrays;
#[cfg(test)] #[path = "lane_region/element_tests.rs"] pub(crate) mod element_tests;
#[cfg(test)] #[path = "lane_region/loop_shape_tests.rs"] pub(crate) mod loop_shape_tests;
use loop_arrays::{ArraySource, LoopArray};
use loop_strings::{LoopString, StringVariables};
#[path = "lane_region/bound_this.rs"] pub(crate) mod bound_this;
use frame::EmitFrame;
use frame_fields::{FieldFacts, HeldRead, StableHold};
use field_plans::{with_read_plan, LanePlans, LinkInput};
use quench_runtime::staged::property::{StoreArm, StoreJoin, StorePlan};
use polls::specialize_closed_polls;
#[cfg(test)]
use polls::retain_closed_polls;
pub(crate) use stable::StableFacts;
use quench_runtime::typed_ir::IntRange;
use quench_runtime::staged::machine::{Binary, Extend, Lane, MachineAccess, MachineFlow, MachineScalar, Relation, Unary};
use super::emit_machine::Builds;

/// The call context, step dispatch and result words a lane body or trampoline reads and writes.
pub(crate) const DISPATCH: quench_runtime::staged::Access = quench_runtime::staged::Region::Dispatch.mutable();
pub(crate) const LANE_REGION_NAMESPACE: u32 = u32::from_be_bytes(*b"QLRG");
/// Inline scratch capacity for the small field-site sets typical of native units;
/// larger units spill normally, without a semantic or compilation limit.
const INLINE_FIELD_BINDINGS: usize = 4;
/// Common small effect frontiers stay inline; larger control-flow graphs spill normally.
const INLINE_EFFECT_EDGES: usize = 4;
/// Most blocks project only a few numbers; larger blocks spill without a limit.
const INLINE_NUMBER_PROJECTIONS: usize = 4;
/// A loop body usually indexes only a few arrays; more spill without a limit.
const INLINE_ARRAY_VIEWS: usize = 4;
/// A block reads the captures of a few inlined callees; more spill without a limit.
const INLINE_CAPTURE_ADMISSIONS: usize = 4;
/// A header and a short latch usually need only a few guarded inputs.
const INLINE_LOOP_INPUTS: usize = 4;
/// A jump's step proves a fact of one tested register, along one or both of its edges.
const INLINE_EDGE_REFINEMENTS: usize = 2;
/// The sums of int32 constants a truncated chain passes through (`Lowering::truncated_through_sums`):
/// from a product under 2^52, each adds under 2^31 and the doubles stay under 2^53.
const TRUNCATION_CHAIN: usize = 4;
/// Common native-entry failure status; a pure scalar miss restarts its call.
const NUMBER_ENTRY_MISS: i64 = 1;
/// The machine bytes an IR instruction of a lane body costs at most, measured over the V8-v7
/// bodies (6.7 to 7.3), which a plan's size is judged by before it compiles.
pub(crate) const LANE_BODY_CODE_BYTES_PER_IR: usize = 8;

/// Semantic proof and active machine view; tagged bits may also be current.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegisterFact {
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
    pub tagged: Vec<Variable>,
    /// Each register's int32, 64-bit integer and float64 views, declared at their first use: most
    /// registers never take one, and the SSA builder's tables grow with variables times blocks. A
    /// view read before any definition reads zero, as only a state that defined it reads it.
    pub int: Vec<Option<Variable>>,
    pub long: Vec<Option<Variable>>,
    pub float: Vec<Option<Variable>>,
}

/// The compile-time state of the instruction being lowered.
#[derive(Clone)]
struct State {
    values: Vec<RegisterFact>,
    proofs: LocalProofs,
    /// What the jump just stepped proved of a register along each of its edges
    /// (`Frame::refine`), applied to the successors' states (`State::narrow`) once it branches.
    refinements: smallvec::SmallVec<[(quench_runtime::staged::Edge, Register, Type); INLINE_EDGE_REFINEMENTS]>,
    /// Receivers kept as their field values (`virtual_objects`), each live in its register.
    virtuals: smallvec::SmallVec<[virtual_objects::VirtualObject; 1]>,
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
#[derive(Clone, Default)]
struct LocalProofs {
    /// Direct-object proofs by owner, until an effect that may change the object retires
    /// them: a register's, and in the final entry fixed `this`'s.
    headers: Vec<Option<ObjectHeader>>,
    /// The owners of `headers` whose proof a read made, which may name a function's record
    /// (`ObjectHeader::prove_readable`): a store proves them again as objects.
    readable: CompoundBitSet,
    /// A fixed receiver (`None`) or register's complete own-slot projection.
    layouts: BTreeMap<(Option<Register>, u32), CheckedObject>,
    /// The exact shape a register's object had before a call (`kept_shapes.rs`), which the code
    /// holds again at the register's next use where the VM holds the shape stable.
    kept_shapes: BTreeMap<Register, u32>,
    /// Registers holding the receiver, whose facts the entry proved.
    receivers: BTreeSet<Register>,
    /// Immutable reads; receiver definitions and memory effects retire these facts.
    fields: smallvec::SmallVec<[FieldValue; INLINE_FIELD_BINDINGS]>,
    /// Pure ToInt32 results, separate from each source value's semantic type.
    int32_projections: smallvec::SmallVec<[(ir::Value, quench_runtime::staged::Int32<ir::Value>); INLINE_NUMBER_PROJECTIONS]>,
    /// Number views of tagged values a test proved (`frame_operands::register_number_value`): a
    /// second register defined as the same value (`p.x * p.x`) takes the double without a test.
    numbers: smallvec::SmallVec<[(ir::Value, quench_runtime::staged::Float64<ir::Value>); INLINE_NUMBER_PROJECTIONS]>,
    /// Dense array views by the tagged base they prove, until an instruction that could change an array's storage (`retains_array_views`).
    arrays: smallvec::SmallVec<[(ir::Value, super::DenseArrayView); INLINE_ARRAY_VIEWS]>,
    /// The dense capture views (values, length) a frameless admission of an inlined callee gave, by the
    /// callee's value: its frame's reads take them, all dominated by the one admission (`Lowering::admitted_captures`).
    admitted_captures: smallvec::SmallVec<[(ir::Value, (ir::Value, ir::Value)); INLINE_CAPTURE_ADMISSIONS]>,
    /// Whether this block read the code's invalidation word since the last helper that may change a Stable fact's subject
    /// (run JavaScript, write a prototype, name a method): every Stable fact the code folded holds until such a helper (`Lowering::hold_stable`).
    stable: bool,
    /// Whether this block proved the realm its global reads were compiled against active (`Lowering::hold_global`).
    realm: bool,
    /// Whether this block proved the run's stores specialized (`JsStepDispatch::stores_specialized`, a
    /// constant of the run), so a store of an unnamed atom tests only the invalidation word (`Lowering::online_store_held`).
    specialized: bool,
    /// Registers whose lane-frame slot holds their current tagged value (`Lowering::home_written`): a
    /// definition retires one, so a call writes back only what changed since the last write-back.
    homed: CompoundBitSet,
}

impl LocalProofs {
    /// Notes that tagged `bits` read as the double `number`: the most recent views win a full
    /// list (a loop body that proves many Numbers keeps the ones its next reads name).
    pub(crate) fn note_number(&mut self, bits: ir::Value, number: quench_runtime::staged::Float64<ir::Value>) {
        if self.numbers.len() >= 4 * INLINE_NUMBER_PROJECTIONS { self.numbers.remove(0); }
        self.numbers.push((bits, number));
    }

    /// Notes the dense capture view `callee`'s admission gave; the oldest view goes once the list is full.
    pub(crate) fn note_admitted_captures(&mut self, callee: ir::Value, view: (ir::Value, ir::Value)) {
        if self.admitted_captures.len() >= INLINE_CAPTURE_ADMISSIONS { self.admitted_captures.remove(0); }
        self.admitted_captures.push((callee, view));
    }

    fn set_readable(&mut self, owner: usize, readable: bool) {
        if readable { self.readable.insert(owner); } else { self.readable.remove(owner); }
    }

    /// An admitted existing own-slot write changes only this name. Retire it for every receiver, since distinct receiver values may alias.
    fn write_field(&mut self, atom: Atom) {
        self.fields.retain(|field| field.atom != atom);
    }

    /// Keeps the homes `other` also holds, for a state both arrivals reach.
    fn meet_homed(&mut self, other: &CompoundBitSet) {
        let dropped: Vec<usize> = self.homed.iter().filter(|register| !other.contains(*register)).collect();
        for register in dropped { self.homed.remove(register); }
    }
}

impl State {
    /// Takes back the tagged views `before` held where one path made its own (`TaggedView::Value`, a word
    /// it boxed or read): that word is defined in the path's blocks, which a sibling path or the join past
    /// both does not see. The next read derives the view again where it is made (`frame::register_tagged`).
    fn drop_path_views(&mut self, before: &State) {
        for (value, before) in self.values.iter_mut().zip(&before.values) {
            if matches!(value.tagged, TaggedView::Value(_)) && value.tagged != before.tagged {
                value.tagged = before.tagged;
            }
        }
    }

    fn canonical(registers: usize) -> Self {
        Self { values: vec![RegisterFact::canonical(); registers], refinements: smallvec::SmallVec::new(), virtuals: smallvec::SmallVec::new(), proofs: LocalProofs {
            headers: vec![None; registers + 1], readable: CompoundBitSet::with_capacity(registers + 1),
            layouts: BTreeMap::new(), kept_shapes: BTreeMap::new(), receivers: BTreeSet::new(),
            fields: smallvec::SmallVec::new(), int32_projections: smallvec::SmallVec::new(), numbers: smallvec::SmallVec::new(), stable: false, realm: false, specialized: false,
            arrays: smallvec::SmallVec::new(), admitted_captures: smallvec::SmallVec::new(), homed: CompoundBitSet::new() } }
    }

    /// The facts a jump proved along `edge` (`refinements`), each narrowing its register's type for
    /// the edge's successor.
    fn narrow(&mut self, edge: quench_runtime::staged::Edge, refinements: &[(quench_runtime::staged::Edge, Register, Type)]) {
        for &(_, register, ty) in refinements.iter().filter(|(on, ..)| *on == edge) {
            let fact = self.values[usize::from(register)];
            if fact.value.representation == Representation::Tagged && ty != fact.value.ty && ty.is_subtype(fact.value.ty) {
                self.values[usize::from(register)] = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(ty, Representation::Tagged) };
            }
        }
    }

    /// The `headers` index of an owner: a register, or fixed `this` (`None`).
    fn header_index(&self, owner: Option<Register>) -> usize {
        owner.map_or(self.values.len(), usize::from)
    }

    /// Native reentry retires shape/storage and function-body admission proofs;
    /// object records and language families stay.
    fn call_effect(&mut self) {
        self.proofs.keep_shapes(&self.values);
        self.proofs.fields.clear();
        self.proofs.int32_projections.clear();
        self.proofs.arrays.clear();
        // Guest code may move a callee's captures or start another admission epoch: the next read probes again.
        self.proofs.admitted_captures.clear();
        self.proofs.headers.fill(None);
        self.proofs.readable.clear();
        self.proofs.layouts.clear();
        self.proofs.stable = false;
        for fact in &mut self.values {
            let unshaped = fact.value.ty.without_object_shape();
            if unshaped != fact.value.ty {
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(unshaped, fact.value.representation) };
            } else if fact.value.ty.body_index().is_some() {
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(Type::language(quench_runtime::ValueType::Function), fact.value.representation) };
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReceiverRecovery { Object, ObjectOrNullish }

impl ReceiverRecovery {
    fn for_body(body: &quench_runtime::bytecode::Function) -> Self {
        if body.instructions().all(|instruction| {
            !instruction.writes_current_this()
                && !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::LoadThis { .. }))
                && !instruction.stores_to_this()
                && (!matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) || matches!(instruction.field_lookup(),
                    Some(FieldLookup::Atom { base, .. }) if base != quench_runtime::bytecode::FieldBase::THIS))
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
    /// The word of the storage the activation's environment-owned locals live in (`JsStepDispatch::environment_storage`),
    /// read once at entry and again after an operation that makes an environment (`Lowering::reload_environment`).
    environment: Variable,
    this: Variable,
    /// Flat per-site immutable slot inputs; no per-operation probe.
    fields: smallvec::SmallVec<[(u16, FieldSlotBinding); INLINE_FIELD_BINDINGS]>,
    /// Function-bound numeric calls whose invariant closure was admitted here.
    calls: CompoundBitSet,
    /// Inlined frames whose invariant receiver needs no boxing on recovery.
    call_receivers: CompoundBitSet,
}

/// A lane body lowered for the code generator, which may run it on another thread.
#[derive(Default)]
pub(crate) struct LoweredLane {
    pub(crate) region: LoweredRegion,
    pub(crate) clif: Option<String>,
    /// The function's scalar entry (`scalar`), which its direct calls of itself call: compiled beside the body.
    pub(crate) scalar: Option<ir::Function>,
    /// The body's proven entry (`ProvenEntry`): the same body past its parameter guards, and the entry types those guards
    /// test (`LoweredRegion::entry_types`). Its code must be the body's dependencies (`LoweredRegion::stable`).
    pub(crate) proven: Option<(ir::Function, Box<[Type]>)>,
}

/// The direct calls whose result a published callee's summary types (`TypeSummary::result`), as the
/// facts' observed reads: each candidate's summary joins the result, and a candidate without one
/// types none. A returning call (its result is its activation's) and a construction are not typed: the
/// first completes the body with a value its completion does not check, and a construction's result is its receiver.
pub(crate) fn call_results(direct: &DirectSites, instructions: &[WideInstruction]) -> Vec<super::facts::ObservedRead> {
    direct.iter().filter_map(|(&pc, callees)| {
        let instruction = *instructions.get(pc)?;
        let Some(quench_runtime::staged::Operation::Call(quench_runtime::staged::CallOperation::Site(site))) = crate::compiled::decoded(instruction) else { return None };
        if site.constructs() || site.returns || instruction.returns_from_frame() { return None; }
        let ty = callees.iter().try_fold(Type::NEVER, |ty, callee| {
            // SAFETY: the plan owns its direct callees for as long as it lowers (`LoweringJob::direct_sites`).
            let callee = unsafe { &**callee };
            callee.summary().map(|summary| ty.join(summary.result))
        })?;
        (ty != Type::NEVER && ty != Type::ANY).then_some(super::facts::ObservedRead { pc, ty })
    }).collect()
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
    /// The word an induction value (`representation` `Int64`) advances by at each step of its counter.
    step: Option<Variable>,
}

#[derive(Clone, Copy)]
struct InvariantRead {
    pc: usize,
    /// The canonical caller home supplying a field receiver at loop entry.
    owner: Option<Register>,
    /// Whether a root-held binding it reads stays put: its loop stores no global object field.
    rooted: bool,
    /// The caller register a function-bound inlined capture read takes its closure from, where the
    /// loop neither writes it nor reads the callee again: the closure it holds at entry is the one
    /// every iteration reads from, so the read is hoisted to the entry edge (`entry_callee`).
    callee: Option<Register>,
}

/// A hoisted view: its base, its elements' address and count, and in a typed region the
/// array's `TypedArrayKind` discriminant.
#[derive(Clone, Copy)]
struct ArrayVariables {
    bits: Variable,
    values: Variable,
    /// The element count: a dense backing's `u32` as `i32`, a typed array's as `i64`.
    length: Variable,
    kind: Variable,
    /// A dense array's room for appends (`dense_appends`) as `i32`: zero until a reserve proves it.
    room: Variable,
}

/// A STORE-site projection admitted on entry to a storage-preserving loop.
/// The record is physical receiver identity, not merely an equal shape.
#[derive(Clone, Copy)]
struct LoopStore {
    header: ObjectHeader,
    address: ir::Value,
    packed_range: Option<IntRange>,
    /// The domain the entry admitted for the store (`Type::NUMBER` where the slot keeps raw
    /// doubles, which the carried Number stores as is; `Type::INT32` otherwise).
    domain: Type,
}

pub(crate) struct LoweredRegion {
    pub number_result: Option<NativeNumberResult>,
    pub function: ir::Function,
    pub native: usize,
    pub fault_poll: bool,
    /// Whether a loop of the region hoists an array view, so a typed variant may differ.
    pub hoists_arrays: bool,
    pub stable: StableFacts,
    /// The entry types of the function's parameters, as a proven entry's lowering holds them (`VmFacts::proven_entry`):
    /// the types its entry guards test, which a caller must prove each argument has. Empty elsewhere.
    pub entry_types: Box<[Type]>,
    /// The body's type summary (`TypeSummary`), where it is a whole function's body.
    pub summary: Option<TypeSummary>,
    /// The leaves the code takes, with the values each stores (`ExitTable::of_unit`, the emitter's own store sets).
    pub exits: ExitTable,
}

pub(crate) use quench_runtime::vm::engine_types::TypeSummary;

impl Default for LoweredRegion {
    /// A region that lowered nothing (`LoweredLane`'s default): an empty function.
    fn default() -> Self {
        Self { number_result: None, function: ir::Function::new(), native: 0, fault_poll: false, hoists_arrays: false, stable: StableFacts::default(), summary: None, entry_types: Box::default(), exits: ExitTable::default() }
    }
}

/// A guard's failure block and the state it leaves from: the registers' facts, and which frame slots
/// already held their value (`LocalProofs::homed`), at the guard.
struct GuardExit {
    block: ir::Block,
    values: Vec<RegisterFact>,
    homed: CompoundBitSet,
    virtuals: smallvec::SmallVec<[virtual_objects::VirtualObject; 1]>,
    pc: u32,
    line: u32,
    /// The fact the guard tests (`guard_exit_for`); `Unclassified` where it names none.
    fact: ExitFact,
}

/// What a step's frame needs beyond its edges: the plans and held read of a named field access,
/// what a slow case needs of the lowering (`sites`: a call-out's record, a capture access's reach,
/// an `instanceof`'s prototype link).
#[derive(Default)]
struct StepWith<'a> {
    access: Option<(&'a LanePlans<'a>, HeldRead)>,
    sites: frame::Sites<'a>,
    /// The call or return a step of the call family runs for (`call_frames`).
    site: Option<call_frames::Site<'a>>,
}

pub(crate) struct Lowering<'a, 'f> {
    builder: FunctionBuilder<'f>,
    program: &'a ResidualProgram,
    unit: &'a super::lane_unit::Unit,
    /// The function's parameter count: an entry guard on a parameter declines the region
    /// when a call's value does not match the compiled entry fact.
    params: usize,
    /// The lowered function, which an exception it raises names as its source (`Lowering::throw`).
    function_id: u32,
    /// The entry function's annotated locals: the register each lives in and the language type its
    /// annotation seeds (`declared::declared_local_types`).
    declared_definitions: Vec<(Register, Type)>,
    /// The entry function's declared `this` type, which its `LoadThis` checks where it enters the destination.
    declared_this: Option<Type>,
    /// The entry function's unwritten rest parameter register and its declared element type (`declared::declared_rest_read`).
    declared_rest: Option<(Register, Type)>,
    /// The entry function's unwritten non-rest parameter registers, each with its parameter index
    /// (`declared::declared_member_reads`), whose named reads the declared member types check.
    declared_members: Vec<(Register, u16)>,
    /// The entry function's signature key (its source span start), which `declared_members` are typed by.
    declared_member_start: Option<u32>,
    helpers: LaneHelpers,
    /// The proved shapes and slots of each field cache site the unit reads or writes.
    fields: &'a FieldSites,
    /// An OSR region's field reads answered from the entry record, by unit PC.
    entry_fields: &'a EntryWords,
    /// The element reads observed to hold Numbers (`RegionSpecializer::number_reads`).
    observed: super::facts::Observed<'a>,
    /// The registers observed to hold int32s at entry (`RegionSpecializer::int_entries`).
    int_entries: &'a [(Register, IntRange)],
    /// The registers observed to hold doubles at entry (`RegionSpecializer::number_entries`).
    number_entries: &'a [Register],
    /// The capture slots observed to hold int32s (`RegionSpecializer::int_captures`).
    int_captures: &'a [u16],
    /// The by-name loads that read the global scope (`Vm::global_reads_in_lane`).
    global_reads: &'a quench_runtime::vm::global_facts::GlobalReads,
    /// The VM's facts: its `length` name, whose read of a direct array is its record's
    /// (`array_length`), and whether an IsHTMLDDA object can exist (`emit_nullish_branch`).
    vm: VmFacts<'a>,
    /// The Stable facts the code folded, each a code dependency of its publication.
    stable: stable::StableUses,
    /// The unit PCs a previous body left at (`Vm::compile_lane_method`): a construction there
    /// runs by the call protocol (`Lowering::construct`).
    generic: &'a [usize],
    /// Whether the entry is proving a lane body's inputs: its guards share `entry_exit`.
    proving_entry: bool,
    /// Whether a previous body left at the instruction being lowered (`generic`): its reads probe the VM's caches.
    left_here: bool,
    /// The view of the loop's hoisted dense array the next element row reads or writes through, which
    /// its entering edge proved once (`EmitFrame::held`).
    held_view: Option<frame_elements::Held>,
    /// Whether the held view's read leaves at a hole (`EmitFrame::hole_leaves`).
    hole_leaves: bool,
    /// The shared leave of a failed input proof (`LaneExit::Entry`), made at its first guard
    /// and filled with the other exits.
    entry_exit: Option<ir::Block>,
    direct: &'a DirectSites,
    /// Per inlined frame, its callee's dense capture slots (values and count), fixed by the
    /// callee's identity, which its call guards; null where they are not dense.
    frame_captures: &'a [(usize, usize)],
    context: ir::Value,
    /// An entry-bound poll page, or the ordinary atomic termination flag.
    interrupt: ir::Value,
    fault_poll: bool,
    /// A loop region's completed iterations, flushed to its entry record where it leaves (`trips.rs`).
    trips: Option<Variable>,
    /// Callback completion edges can retire the native entry's interrupt ownership proof.
    reentries: smallvec::SmallVec<[ir::Block; INLINE_EFFECT_EDGES]>,
    polls: smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>,
    /// Already-conforming backedges meet before their common fault poll.
    poll_latches: super::SecondaryMap<ir::Block, Option<ir::Block>>,
    shared_headers: CompoundBitSet,
    payload: ir::Value,
    registers_base: ir::Value,
    /// The running body's receiver and capture words its caller passes (`entry_cell::capture_word`).
    entry_words: (ir::Value, ir::Value),
    callee_slots: Vec<(u16, ir::Value)>,
    registers: Registers,
    written: CompoundBitSet,
    /// Source operations already consumed by an exact, unobservable scalar pair.
    consumed: CompoundBitSet,
    blocks: Vec<Option<ir::Block>>,
    /// Per resume PC, its leave blocks, one per set of registers stored (`exits.rs`).
    exits: Vec<Vec<exits::ExitBlock>>,
    /// The leave blocks shared by every caller-frame PC that stores the same registers, which
    /// take the exit word as their parameter (`Lowering::shared_exit_block`).
    shared_exits: Vec<exits::ExitBlock>,
    /// Each block leader's register use demand (`demand::analyze`).
    demands: demand::Demands,
    entry: Option<Entry>,
    native: usize,
    /// Whether this entry's caller loop reaches its native latch.
    backedge: bool,
    /// Guard failure blocks to fill once the region is lowered (`guard_exit`).
    // Recovery boxes register values; object/layout proofs authorize forward execution
    // only and must not be retained in every guard's snapshot. With the lowering line that
    // made each guard, which a diagnostics build records.
    pending: Vec<GuardExit>,
    /// Whether the unit makes native calls (`Lowering::call`): a callee may change `this`,
    /// so no read or store folds the receiver facts its entry proved.
    native_calls: bool,
    /// Entry-bound guest depth for a region whose helpers cannot reenter.
    call_budget: Option<ir::Value>,
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
    /// The unit's block leaders (`leaders`), made once for the first loop whose reads hoist.
    unit_leaders: std::cell::OnceCell<std::rc::Rc<CompoundBitSet>>,
    /// The calls that run inline as Math operations (`native_calls::intrinsic_calls`).
    intrinsics: native_calls::Intrinsics,
    hoisted: BTreeMap<usize, LoopRead>,
    /// The scalar native calls a loop's entry computed once, by PC (`native_calls::hoist_scalar_calls`).
    scalar_values: BTreeMap<usize, native_calls::HoistedScalar>,
    /// Per dense array a loop searches, whether its prototypes hold no index, proved on the loop's entry
    /// (`hoist_arrays`) and unset by an invalidation (`forget_append_rooms`).
    prototype_clear: BTreeMap<ArraySource, Variable>,
    /// The hoisted dense arrays, by loop header and source, whose entering edge guarded every element
    /// an int32 (`int_only_array`, `hoist_arrays`): their reads take the element's word as it is.
    int_only: BTreeSet<(usize, ArraySource)>,
    /// The loops whose entering edge tests the construct plan of each callee their kept
    /// constructions build by, once for the loop (`virtual_objects::hoist_constructions`).
    construct_ready: Vec<virtual_objects::ConstructReady>,
    /// Loop-invariant array bases whose views their loop's entry proves (`loop_arrays`).
    loop_arrays: Vec<LoopArray>,
    /// Loop-invariant strings whose flat facts their loop's entry proves (`loop_strings`).
    loop_strings: Vec<LoopString>,
    /// The variables each held string's facts live in, by register (`loop_strings`).
    string_vars: BTreeMap<Register, StringVariables>,
    /// The unit a string element read left for the comparison at the next PC, which compares it
    /// as a unit and never makes the one-character string (`Lowering::unit_compare`).
    pending_unit: Option<PendingUnit>,
    /// The shapes loops proved of their invariant receivers on their entering edges (`loop_receivers`).
    loop_shapes: Vec<loop_receivers::LoopShape>,
    /// Whether no operation of the unit may move an array's backing or the array record table
    /// (`arrays_stay_put`), so the record table's base is read-only throughout.
    arrays_fixed: bool,
    /// Each hoisted base's tagged value and view, by register, across its loop's iterations.
    array_vars: BTreeMap<ArraySource, ArrayVariables>,
    views: ElementViews,
    /// Whether loops version their bounds on entry (`RegionPlan::entry_bounds`).
    entry_bounds: bool,
    /// Successful write-through stores defining a loop-header field's SSA variable.
    loop_fields: Vec<LoopField>,
    loop_stores: BTreeMap<usize, LoopStore>,
    /// The unit's integer proofs, computed once before lowering (`Lowering::prove_ranges`): the
    /// arithmetic that cannot overflow and the hoisted element accesses within their array.
    ranges: int_ranges::IntRanges,
    /// The registers whose int32 range the entry guards once and the ranges pass takes as
    /// the entry state (`Lowering::entering_ranges`).
    entering: Vec<(Register, IntRange)>,
    /// The PC being lowered, whose edges to a header enter its loop or close it.
    at: usize,
    /// Per block leader, its forward edges (`forward_edges`), and the states the lowered
    /// ones arrived with: a block they all reach keeps the local proofs they share.
    forward: Vec<Option<usize>>,
    arrived: BTreeMap<usize, Vec<LocalProofs>>,
    /// The block leaders an entry or a lowered edge enters: a block none does is not
    /// lowered, its code reached only past a leave.
    entered: CompoundBitSet,
    /// The block leaders sealed when lowered, which only edges lowered before enter
    /// (`edges::leader_edges`): a variable read in one resolves without a parameter.
    sealable: CompoundBitSet,
    /// Whether a run can do anything but leave: complete natively or run a loop natively. A
    /// body that can do neither declines (`lower`).
    finishes: bool,
    /// The one epilogue every stop shares: it returns its status parameter.
    leave: ir::Block,
    /// The stop at a record: it writes its result word parameter and leaves stopped.
    stop: ir::Block,
    /// The block the call-outs' statuses of a leave in a callee, or a termination, reach with the
    /// status, made at the first and filled at the region's end (`call_outs`).
    call_out_stop: Option<ir::Block>,
    /// The throws native code catches itself, by unit PC (`catches::catch_edges`).
    catches: catches::Catches,
    /// The same throws as the analysis's exception edges (`facts::Raise`), ascending by PC.
    raises: Vec<super::facts::Raise>,
}

enum Liveness<'a> {
    Unit(Option<std::borrow::Cow<'a, quench_runtime::compile::liveness::RegisterMasks>>),
    Frames {
        caller: Option<&'a quench_runtime::compile::liveness::RegisterMasks>,
        frames: Vec<Option<&'a quench_runtime::compile::liveness::RegisterMasks>>,
    },
}

/// How much of the unit must run natively: a prefix (a lane body, which leaves to the lane
/// anywhere), or all of it (a numeric region).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Coverage {
    Prefix,
    Full,
    /// One loop of the function's unit, from its header through its backward jump. Only
    /// instructions in the loop lower; an edge out of it leaves at the exact PC. The
    /// backedge is a native branch to the header with the tier's poll.
    Loop { header: usize, backedge: usize, poll: bool },
    /// A caller-origin entry into the original unit CFG. Opaque operations
    /// leave at their exact origin; earlier loops and effects are not replayed.
    PrefixFrom(usize),
}

impl Coverage {
    pub(crate) fn entry(self) -> usize {
        match self { Self::Loop { header, .. } => header, Self::PrefixFrom(pc) => pc, _ => 0 }
    }
    fn full(self) -> bool { self == Self::Full }
    fn selected_entry(self) -> bool {
        matches!(self, Self::PrefixFrom(_) | Self::Loop { .. })
    }
    /// The PC after the region this coverage selects: where a path that falls off its end
    /// leaves. A loop leaves after its backward jump.
    fn exit_pc(self, unit_len: usize) -> usize {
        match self {
            Self::Loop { backedge, .. } => backedge.saturating_add(1).min(unit_len),
            _ => unit_len,
        }
    }

    /// Whether instruction `pc` is part of the region this coverage selects.
    fn lowered(self, pc: usize) -> bool {
        match self {
            Self::Loop { header, backedge, .. } => (header..=backedge).contains(&pc),
            _ => true,
        }
    }
}

/// How a run ends. Every kind runs the lane body ABI (`body_signature_in_lane`): it takes its
/// inputs in the register window and returns its status and result word in registers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Completion {
    /// A status and a PC (`LaneExit`): a lane body, or a loop region.
    Body,
    /// Pure, restartable scalar entry: borrowed inputs in the window's first slots, proved
    /// Numbers once at entry, and one Number result word; a miss restarts the original call.
    Number,
    /// A numeric function's entry that takes its numbers in registers (`scalar`): the same
    /// results and misses as `Number`, for a call whose arguments are proved those numbers.
    Scalar(scalar::ScalarAbi),
}

impl Completion {
    /// Whether a run completes with a status and a PC (a lane body or a loop region).
    pub(crate) fn status_and_pc(self) -> bool { self == Self::Body }
}

pub(crate) use quench_runtime::vm::engine_types::ElementViews;

#[quench_macros::ext(ElementViewsOps1)]
impl ElementViews {
    /// The views of arrays viewed both as `self` and as `other`: the same dense views, or typed
    /// views of the kind both name and of no kind where they differ; `None` for a mix.
    pub(crate) fn common(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Typed(left), Self::Typed(right)) => Some(Self::Typed(if left == right { left } else { None })),
            _ => (self == other).then_some(self),
        }
    }

    /// The kind a typed view's site observed.
    fn observed_kind(self) -> Option<quench_runtime::tier_surface::TypedArrayKind> {
        match self {
            Self::Typed(kind) => kind,
            // A double backing's words are a `Float64Array`'s.
            Self::Double => Some(quench_runtime::tier_surface::TypedArrayKind::Float64),
            Self::Dense => None,
        }
    }
}

pub(crate) fn restartable_numeric_function(program: &ResidualProgram, function: &quench_runtime::bytecode::Function,
    instructions: impl IntoIterator<Item = WideInstruction>) -> bool {
    crate::js_compiled::function_candidate(function, program) && numeric_instructions(program, instructions)
}

fn numeric_instructions(program: &ResidualProgram, instructions: impl IntoIterator<Item = WideInstruction>) -> bool {
    instructions.into_iter().all(|instruction| numeric_instruction(program, instruction))
}

fn numeric_instruction(program: &ResidualProgram, instruction: WideInstruction) -> bool {
    if instruction.writes_current_this()
        || (instruction.result_flags() != 0 && !instruction.returns_from_frame()) { return false; }
    match () {
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) => program.constants.get(instruction.constant_index())
            .and_then(super::facts::immediate).is_some(),
        _ if instruction.op().traits().has(quench_runtime::bytecode::Traits::SCALAR_REGION) => true,
        _ => quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary)
            && instruction.numeric_local_target().is_none(),
    }
}

fn register_offset(register: Register) -> i32 {
    i32::from(register) * std::mem::size_of::<u64>() as i32
}

pub(crate) fn int_tagged(builder: &mut FunctionBuilder<'_>, int: ir::Value) -> ir::Value {
    let mut stage = emit(builder);
    let payload = stage.raw_unary(Unary::ZeroExtend(Lane::I64), int);
    quench_runtime::staged::value::int_word(&mut stage, payload)
}

pub(crate) fn is_int_tagged(builder: &mut FunctionBuilder<'_>, bits: ir::Value) -> ir::Value {
    quench_runtime::staged::value::is_int(&mut emit(builder), bits)
}

/// The tagged Boolean a predicate `holds` selects: the boxed `true` where it holds, else the boxed `false`.
pub(crate) fn boolean_word(builder: &mut FunctionBuilder<'_>, holds: ir::Value) -> ir::Value {
    let yes = builder.ins().iconst(types::I64, Value::TRUE.to_bits() as i64);
    let no = builder.ins().iconst(types::I64, Value::FALSE.to_bits() as i64);
    builder.ins().select(holds, yes, no)
}

/// A block marked cold before any code is lowered into it, for the exits and slow paths the hot path leaves to.
pub(crate) fn cold_block(builder: &mut FunctionBuilder<'_>) -> ir::Block {
    let block = builder.create_block();
    builder.set_cold_block(block);
    block
}

/// The registers an instruction writes, by its operand layouts.
pub(crate) fn writes(instruction: WideInstruction) -> super::facts::Writes {
    instruction.written_registers().collect()
}

impl<'f> Builds<'f> for Lowering<'_, 'f> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'f> {
        &mut self.builder
    }
}

impl<'a, 'f> Lowering<'a, 'f> {
    fn tagged(&mut self, state: &mut State, register: Register) -> ir::Value {
        frame::register_tagged(&mut self.builder, &mut self.registers, state, register)
    }

    fn define(&mut self, state: &mut State, register: Register, value: ir::Value, fact: RegisterFact) {
        frame::register_define(&mut self.builder, &mut self.registers, &mut self.written, state, register, value, fact);
    }

    /// Whether `pc` is the entry function's own record: an inlined callee's records read that
    /// callee's activation, which no window of the entry's holds.
    fn caller_record(&self, pc: usize) -> bool {
        matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_)))
    }

    fn set_tagged(&mut self, state: &mut State, register: Register, bits: ir::Value) {
        self.define(state, register, bits, RegisterFact::definition(Type::ANY, Representation::Tagged));
    }

    /// The tagged Boolean a predicate `holds` selects.
    fn boolean(&mut self, holds: ir::Value) -> ir::Value {
        boolean_word(&mut self.builder, holds)
    }

    fn set_boolean(&mut self, state: &mut State, register: Register, boolean: ir::Value) {
        self.define(state, register, boolean, RegisterFact::definition(Type::language(quench_runtime::ValueType::Boolean), Representation::Tagged));
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
        frame::register_int_value(&mut self.builder, &mut self.registers, state, register, otherwise).word()
    }

    /// Prove Number only for a new untrusted value; later uses read its unboxed view.
    fn number(&mut self, state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
        frame::register_number_value(&mut self.builder, &mut self.registers, state, register, otherwise).word()
    }

    fn canonicalize(&mut self, state: &mut State, pc: usize) {
        for register in 0..state.values.len() {
            if self.is_live(pc, register as Register) && !state.values[register].tagged.current() {
                self.tagged(state, register as Register);
            }
        }
    }

    /// `leave` naming the lowering line that decided the leave, which a diagnostics run reports
    /// (`JsStepDispatch::exit_line`).
    #[track_caller]
    fn exit(&mut self, state: &mut State, pc: u32) {
        if self.vm.diagnostics && !self.completion.numeric() {
            let line = self.raw_const(Lane::I64, i64::from(std::panic::Location::caller().line()));
            self.store_word(regions::dispatch(), line, self.payload, std::mem::offset_of!(JsStepDispatch, exit_line) as i32);
        }
        self.leave_at(state, pc);
    }

    /// Leave at `pc` with the interpreter's state: every written register back in the lane's file, but those it already holds.
    fn leave_at(&mut self, state: &mut State, pc: u32) {
        if self.completion.numeric() {
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
        // A kept receiver the interpreter reads is an object again (`virtual_objects`).
        if !state.virtuals.is_empty() { self.materialize_virtuals(state, pc); }
        let stored = self.stored_at(state, pc);
        self.materialize_inlined(state, pc);
        // A caller-frame leave of a body differs from another storing the same registers only
        // by its exit word: one block stores them for all, the word its parameter.
        let origin = self.unit.origins.get(pc).copied().unwrap_or(super::lane_unit::Origin::Caller(self.unit.end));
        if let (Completion::Body, super::lane_unit::Origin::Caller(record)) = (self.completion, origin) {
            let block = self.shared_exit_block(stored);
            let word = self.raw_const(Lane::I64, LaneExit::At(record as usize).word() as i64);
            self.goto(block, &[word]);
            return;
        }
        let block = self.exit_block(pc, stored);
        self.goto(block, &[]);
    }

    /// Lowers into `block` from here, sealed where every edge into it is lowered.
    fn switch_to(&mut self, block: ir::Block, sealed: bool) {
        self.builder.switch_to_block(block);
        if sealed { self.builder.seal_block(block); }
    }

    fn continue_at(&mut self, state: &mut State, pc: usize) {
        // No block's entry state keeps a receiver as its fields (`virtual_objects`): the
        // instruction that ends a block allocates any live one first, so one reaching an
        // edge is a lowering this code does not make.
        if state.virtuals.iter().any(|kept| self.is_live(pc, kept.register)) { self.valid = false; declined("a kept receiver reaches an edge", pc); }
        state.virtuals.clear();
        if !self.coverage.lowered(pc) {
            self.exit(state, pc as u32);
            return;
        }
        // A block before this one that no edge entered was not lowered.
        let lowered = pc > self.at || self.entered.contains(pc);
        match self.blocks.get(pc).copied().flatten().filter(|_| lowered) {
            // An edge the forward analysis did not see reaches a sealed block: decline.
            Some(_) if pc <= self.at && self.sealable.contains(pc) => { self.valid = false; declined("an unseen edge reaches a sealed block", pc); }
            Some(block) => {
                self.entered.insert(pc);
                self.finishes |= pc <= self.at;
                if self.at < pc { self.hoist_entry(state, pc); }
                self.conform(state, pc);
                if self.at < pc { self.arrive(state, pc); }
                self.goto(block, &[]);
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
        state.proofs.readable.clear();
        for arrived in &arrived {
            for owner in arrived.readable.iter() { state.proofs.readable.insert(owner); }
        }
        state.proofs.layouts = first.layouts.iter().filter(|(key, projection)| rest.iter()
            .all(|other| other.layouts.get(key) == Some(projection))).map(|(key, projection)| (*key, *projection)).collect();
        state.proofs.receivers = first.receivers.iter().filter(|register| rest.iter()
            .all(|other| other.receivers.contains(register))).copied().collect();
        state.proofs.fields = first.fields.iter().copied().filter(|field| rest.iter()
            .all(|other| other.fields.contains(field))).collect();
        state.proofs.int32_projections = first.int32_projections.iter().copied().filter(|projection| rest.iter()
            .all(|other| other.int32_projections.contains(projection))).collect();
        state.proofs.numbers = first.numbers.iter().copied().filter(|projection| rest.iter()
            .all(|other| other.numbers.contains(projection))).collect();
        state.proofs.admitted_captures = first.admitted_captures.iter().copied().filter(|view| rest.iter()
            .all(|other| other.admitted_captures.contains(view))).collect();
        state.proofs.arrays = first.arrays.iter().copied().filter(|view| rest.iter()
            .all(|other| other.arrays.contains(view))).collect();
        state.proofs.stable = first.stable && rest.iter().all(|other| other.stable);
        state.proofs.realm = first.realm && rest.iter().all(|other| other.realm);
        state.proofs.specialized = first.specialized && rest.iter().all(|other| other.specialized);
        // A block a rotated latch also enters has an edge `rest` does not hold: only a sealable block keeps the homes.
        if self.sealable.contains(pc) {
            state.proofs.homed.clone_from(&first.homed);
            for other in rest { state.proofs.meet_homed(&other.homed); }
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
            // A parameter's entry fact varies per call: a mismatched value declines the
            // region (the lane runs the header) instead of exiting native code.
            let saved = self.proving_entry;
            if target == self.coverage.entry() && index < self.params { self.proving_entry = true; }
            let proved = self.conform_register(state, target, register, ty);
            self.proving_entry = saved;
            if !proved { return; }
        }
    }

    /// One live register conformed to `target`'s fact `ty`; whether the edge still reaches it.
    fn conform_register(&mut self, state: &mut State, target: usize, register: Register, ty: Type) -> bool {
        let index = usize::from(register);
        let held = state.values[index].value.representation;
        match number_domains::target_representation(ty) {
            Representation::Int32 if held != Representation::Int32 => {
                let exit = self.guard_exit(state, target as u32);
                self.int(state, register, exit);
            }
            Representation::Float64 if held != Representation::Float64 => {
                let exit = self.guard_exit(state, target as u32);
                let number = self.number(state, register, exit);
                let variable = self.float_var(index);
                self.builder.def_var(variable, number);
            }
            Representation::Tagged => { self.tagged(state, register); }
            Representation::Int32 | Representation::Int64 | Representation::Float64 => {}
        }
        self.conform_object(state, target, register, ty);
        self.conform_language(state, target, register, ty);
        // A fact this edge does not prove and cannot guard leaves at the target, exactly:
        // the edge loses native code, never the body (AGENTS.md, check once).
        if self.proves(state, register, ty, target) { return true; }
        let exit = self.guard_exit(state, target as u32);
        self.goto(exit, &[]);
        let unreached = self.builder.create_block();
        self.builder.switch_to_block(unreached);
        false
    }

    /// An object fact of the target the edge has not proved: the direct-object family by one
    /// guard, an exact shape guarded on this edge with the owner's header read once, so the
    /// edge keeps native code.
    fn conform_object(&mut self, state: &mut State, target: usize, register: Register, ty: Type) {
        let index = usize::from(register);
        if ty == Type::DIRECT_OBJECT_FAMILY && !state.values[index].value.ty.is_direct_object_family() {
            let exit = self.guard_exit(state, target as u32);
            let bits = self.tagged(state, register);
            let object = quench_runtime::staged::value::is_direct_object(&mut super::object_slots::emit(&mut self.builder), bits);
            emit_js_guard(&mut self.builder, object, exit);
            self.refine_object_family(state, bits);
        }
        if let Some(shape) = ty.object_shape() && !state.values[index].value.ty.is_subtype(ty) {
            let exit = self.guard_exit(state, target as u32);
            let header = self.object_header(state, Some(register), false, exit);
            headers::prove_header_shape(&mut self.builder, state, Some(register), header, shape, exit);
            state.values[index] = RegisterFact { tagged: state.values[index].tagged,
                ..RegisterFact::definition(Type::direct_object(shape), Representation::Tagged) };
        }
    }

    /// A language fact of the target the edge has not proved, when `ty` is exactly a set
    /// `scalar::checkable` names: one guard that the register holds a member, then the set as its
    /// fact. A String member is proved by its direct tag, and any other string by the VM's `typeof`
    /// helper on a slow path only. An array or plain object member (`ARRAY`, `OBJECT_PLAIN`, bits no
    /// `ValueType` names) is proved by its family tag alone. A Number, BigInt or Symbol set is not tested here and leaves to `proves`.
    fn conform_language(&mut self, state: &mut State, target: usize, register: Register, ty: Type) {
        use quench_runtime::ValueType::{Boolean, Function, Null, Object, String as Text, Undefined};
        use quench_runtime::staged::Stage;
        use quench_runtime::typed_ir::Languages;
        use quench_runtime::staged::value::{is_boolean, is_direct_array, is_direct_function, is_direct_object, is_direct_string, is_null, is_undefined};
        use super::declared_checks::{ARRAY, OBJECT_PLAIN};
        let index = usize::from(register);
        let kinds = [Boolean, Undefined, Null, Object, Function, Text];
        let families = [ARRAY, OBJECT_PLAIN];
        let bits = kinds.into_iter().filter(|kind| Type::language(*kind).is_subtype(ty))
            .fold(Languages::EMPTY, |set, kind| set | Languages::of(kind)).bits()
            | families.into_iter().filter(|family| Type::languages(*family).is_subtype(ty)).fold(0u16, |set, family| set | family);
        if !scalar::checkable(bits) || Type::languages(bits) != ty || state.values[index].value.ty.is_subtype(ty) { return; }
        let value = self.tagged(state, register);
        let exit = self.guard_exit(state, target as u32);
        // The one-word tests of the set's members other than String, any of which proves the value.
        let others = {
            let stage = &mut super::object_slots::emit(&mut self.builder);
            let mut member: Option<ir::Value> = None;
            for kind in kinds.into_iter().filter(|kind| *kind != Text && Languages::from_bits(bits).contains(*kind)) {
                let test = match kind {
                    Boolean => is_boolean(stage, value),
                    Undefined => is_undefined(stage, value),
                    Null => is_null(stage, value),
                    Object => is_direct_object(stage, value),
                    Function => is_direct_function(stage, value),
                    // Not in `kinds`: no exact one-word test.
                    _ => continue,
                };
                member = Some(match member { Some(any) => stage.any(any, test), None => test });
            }
            // A plain object is the plain family's tag, as `Object` is above; an array is the array tag.
            for family in families.into_iter().filter(|family| bits & *family != 0) {
                let test = if family == ARRAY { is_direct_array(stage, value) } else { is_direct_object(stage, value) };
                member = Some(match member { Some(any) => stage.any(any, test), None => test });
            }
            member
        };
        if Languages::from_bits(bits).contains(Text) {
            // A direct string is accepted by its tag; any other value falls to the VM's `typeof`
            // test, which only that slow path pays for.
            let direct = is_direct_string(&mut super::object_slots::emit(&mut self.builder), value);
            let accepted = match others {
                Some(others) => super::object_slots::emit(&mut self.builder).any(others, direct),
                None => direct,
            };
            let slow = self.builder.create_block();
            let done = self.builder.create_block();
            emit(&mut self.builder).branch_if(accepted, (done, &[]), (slow, &[]));
            self.builder.switch_to_block(slow);
            let name = self.raw_const(Lane::I64, quench_runtime::bytecode::typeof_name_index("string").expect("a typeof name") as i64);
            let named = self.helper_call(LaneHelper::TypeofIs, &[value, name]);
            let holds = self.raw_compare_imm(Relation::NotEqual, named, 0);
            emit_js_guard(&mut self.builder, holds, exit);
            self.goto(done, &[]);
            self.builder.switch_to_block(done);
        } else {
            let member = others.expect("a checkable set names a member");
            emit_js_guard(&mut self.builder, member, exit);
        }
        state.values[index] = RegisterFact { tagged: state.values[index].tagged,
            ..RegisterFact::definition(ty, Representation::Tagged) };
    }

    fn entry_state(&mut self, pc: usize, registers: usize) -> State {
        let mut state = State::canonical(registers);
        self.home_entry_registers(&mut state, pc);
        let Some(types) = self.facts.at(pc) else { return state };
        for (index, ty) in types.iter().copied().enumerate() {
            if !self.is_live(pc, index as Register) { continue; }
            let Some(fact) = self.entry_fact(pc, ty) else { self.valid = false; declined("no entry fact for a live register", pc); return state; };
            state.values[index] = fact;
        }
        state
    }

    /// The live caller registers a once-entered region entry loaded from their frame slots.
    fn home_entry_registers(&self, state: &mut State, pc: usize) {
        if pc == self.coverage.entry() && !self.completion.numeric()
            && edges::entered_once(&self.unit.instructions, pc, |at| self.coverage.lowered(at)) {
            for register in (0..self.unit.caller_registers).filter(|register| self.is_live(pc, *register)) {
                state.proofs.homed.insert(usize::from(register));
            }
        }
    }

    /// The fact a register of type `ty` has at `pc`: guarded at the region entry, a definition after.
    fn entry_fact(&self, pc: usize, ty: Type) -> Option<RegisterFact> {
        let entry = pc == self.coverage.entry();
        RegisterFact::admit(Fact { ty, representation: number_domains::target_representation(ty),
            binding: if entry { BindingTime::Location } else { BindingTime::Dynamic },
            proof: if !entry { Proof::Definition }
                else if ty == Type::ANY { Proof::CanonicalValue } else { Proof::EntryGuard },
            projection: Projection::Exact,
        })
    }

    /// The unit's block leaders (`leaders`), made once for the unit.
    fn unit_leaders(&self) -> &CompoundBitSet {
        self.unit_leaders.get_or_init(|| std::rc::Rc::new(leaders(&self.unit.instructions)))
    }

    /// The one-unit string constant `operand` names, for a strict comparison that decides on its unit
    /// (`Lowering::unit_compare`).
    fn one_unit_constant(&self, operand: Operand) -> Option<u16> {
        if operand.kind()? != OperandKind::Constant { return None; }
        match self.program.constants.get(usize::from(operand.payload()))? {
            quench_runtime::bytecode::Constant::String(text) => {
                let mut units = text.encode_utf16();
                let unit = units.next()?;
                units.next().is_none().then_some(unit)
            }
            quench_runtime::bytecode::Constant::StringUnits(units) => match units[..] {
                [unit] => Some(unit),
                _ => None,
            },
            _ => None,
        }
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
        frame::operand_number(&mut self.builder, &mut self.registers, state, &self.program.constants,
            operand, otherwise).map(quench_runtime::staged::Float64::word)
    }

    /// An operand's int32 value, statically where it is a constant int32.
    fn operand_int(&mut self, state: &mut State, operand: Operand, otherwise: ir::Block) -> Option<ir::Value> {
        frame::operand_int(&mut self.builder, &mut self.registers, state, &self.program.constants,
            operand, otherwise).map(quench_runtime::staged::Int32::word)
    }

    /// An operand's tagged bits: its register's, or its immediate constant's.
    fn operand_tagged(&mut self, state: &mut State, operand: Operand) -> Option<ir::Value> {
        match operand.kind()? {
            OperandKind::Register => Some(self.tagged(state, operand.register_index()?)),
            OperandKind::Constant => {
                let (value, _) = self.constant_value(usize::from(operand.payload()))?;
                Some(self.identity_constant(value))
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

/// A string element read whose unit its next instruction compares with a one-unit constant
/// (`Lowering::unit_compare`): the register the read would define, the unit it read, and the
/// constant unit and sense of the comparison.
#[derive(Clone, Copy)]
struct PendingUnit {
    pc: usize,
    register: Register,
    unit: ir::Value,
    constant: u16,
    negated: bool,
}

/// The PCs that begin basic blocks: the entry, every jump target and every instruction
/// after a jump.
pub(crate) fn leaders(instructions: &[WideInstruction]) -> CompoundBitSet {
    let mut leaders = CompoundBitSet::with_capacity(instructions.len());
    if !instructions.is_empty() { leaders.insert(0); }
    for (pc, instruction) in instructions.iter().enumerate() {
        if instruction.op().control_flow_layout() == ControlFlowLayout::BranchTable {
            // Every entry of a branch table is a leader: its `Jump` rows start blocks.
            for target in successors(pc, *instruction).into_iter().filter(|t| *t < instructions.len()) { leaders.insert(target); }
        }
        if instruction.op().control_flow_layout().jumps() {
            if let Some(target) = usize::try_from(instruction.jump_target()).ok().filter(|t| *t < instructions.len()) {
                leaders.insert(target);
            }
            if pc + 1 < instructions.len() { leaders.insert(pc + 1); }
        }
    }
    leaders
}

/// The register whose value `register` copies at the comparison at `pc`: following the moves and
/// nops back to `header` within one block, so copies share the comparison's SSA value.
fn move_root(instructions: &[WideInstruction], leaders: &CompoundBitSet, (header, pc): (usize, usize), mut register: Register) -> Register {
    for at in (header..pc).rev() {
        if at + 1 < pc && leaders.contains(at + 1) { break; }
        let instruction = instructions[at];
        match () {
            _ if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) && instruction.result_flags() == 0 => {
                if instruction.register_a() == register { register = instruction.register_b(); }
            }
            _ if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Nop)) => {},
            _ => break,
        }
    }
    register
}

/// Whether `instruction` can change no heap value natively: anything else it would do (a
/// non-Number operand, a missed cache) leaves the body first.
fn reads_only(constants: &[quench_runtime::bytecode::Constant], instruction: WideInstruction) -> bool {
    reads_slot(constants, instruction).unwrap_or_else(|| match () {
        // ToNumeric of a Number is the Number; any other operand leaves (`Lowering::instruction`). A
        // dense element is read natively, any other leaving (`emit_dense_element_read`); a null or
        // undefined operand leaves before any effect (`staged::require_object_coercible`).
        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::ToNumeric { .. }))) || matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementRead { .. })) || matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::RequireObjectCoercible { .. })) => !instruction.returns_from_frame(),
        // A Number operator of a Number; any other operand leaves before any effect (`Lowering::unary`).
        // A dense switch's case slot and its branch table read one frame register; an operand that
        // is no int leaves before any effect (`Lowering::int_case_index`, `Lowering::branch_table`).
        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::IntCase { .. }))) => instruction.result_flags() == 0,
        _ if instruction.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::BranchTable => instruction.result_flags() == 0,
        _ if instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::UnaryOperator
            && crate::compiled::number_unary(instruction, instruction.unary_operator()).is_some() =>
            instruction.result_flags() == 0 && !instruction.writes_current_this(),
        _ => quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) && !instruction.returns_from_frame(),
    })
}

/// Whether `instruction` reads only a frame slot, a constant, a capture or a field, where it is
/// one; `None` for the other instructions.
fn reads_slot(constants: &[quench_runtime::bytecode::Constant], instruction: WideInstruction) -> Option<bool> {
    Some(match () {
        _ if instruction.op().traits().has(Traits::FRAME_ONLY) => true,
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) => constants.get(instruction.constant_index()).and_then(super::facts::immediate).is_some(),
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::IncDec) => instruction.numeric_local_target().is_none(),
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) => instruction.result_flags() == 0 && instruction.capture_depth() == 0,
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) => !instruction.writes_current_this() && !instruction.returns_from_frame(),
        _ => return None,
    })
}

/// Whether `instruction` is a store natively: of its atom, into a slot the object already
/// has, so it changes no shape, layout or other property; anything else leaves first.
fn stores_own(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) && !instruction.returns_from_frame()
}

/// Whether `instruction` is an element store natively: into a record array, whose elements
/// no named field read, capture or object shape observes (`Lowering::element_write`); anything
/// else leaves first.
fn stores_element(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. })) && !instruction.returns_from_frame()
}

/// Whether an array view (`LocalProofs::arrays`) survives `instruction`: it reads no heap
/// value but an element, or replaces one in place (`Lowering::element_write` retires the views
/// itself on its general path), so no array's storage changes.
fn retains_array_views(constants: &[quench_runtime::bytecode::Constant], instruction: WideInstruction) -> bool {
    match () {
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. } | quench_runtime::staged::Operation::LoadCapture { .. })) => false,
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. })) => true,
        _ => reads_only(constants, instruction),
    }
}

/// Whether a loop may hold `instruction` natively: a read, an own-field or element store, or a
/// call lowered as an intrinsic the loop holds (`native_calls::holds_in_loop`).
fn stays_native(constants: &[Constant], instruction: WideInstruction, intrinsic: bool) -> bool {
    intrinsic || reads_only(constants, instruction) || stores_own(instruction) || stores_element(instruction)
}

/// Loops with one external entry and no interior entry, whose native operations
/// only read or replace existing own fields and array elements. Misses leave before guest effects.
/// A sole native entry at the header can supply the same entry proofs.
fn local_loops(unit: &super::lane_unit::Unit, constants: &[quench_runtime::bytecode::Constant], start: usize,
    intrinsics: &native_calls::Intrinsics, global_reads: &quench_runtime::vm::global_facts::GlobalReads) -> Vec<(usize, usize)> {
    let instructions = &unit.instructions;
    let target = |instruction: &WideInstruction| instruction.op().control_flow_layout().jumps().then(|| instruction.jump_target() as usize);
    let falls = |instruction: &WideInstruction| instruction.op().control_flow_layout().falls_through();
    let mut loops = Vec::new();
    for (back, header) in quench_runtime::bytecode::back_edges_of(instructions.iter().copied()) {
        let entry_header = start == header;
        let body = &instructions[header..=back];
        let entries = usize::from(header > 0 && falls(&instructions[header - 1]))
            + instructions[..header].iter().filter(|candidate| target(candidate) == Some(header)).count();
        let side = instructions.iter().enumerate().any(|(pc, candidate)| !(header..=back).contains(&pc)
            && target(candidate).is_some_and(|to| (header + 1..=back).contains(&to) || (to == header && pc > back)));
        if (entries != 1 && !entry_header) || side
            || (header + 1..=back).contains(&start)
            || !body.iter().enumerate().all(|(offset, candidate)| {
                let pc = header + offset;
                stays_native(constants, *candidate, native_calls::holds_in_loop(intrinsics, pc) || native_calls::scalar_name_read(global_reads, pc))
            }) { continue }
        loops.push((header, back));
    }
    loops
}

fn invariant_reads(program: &ResidualProgram, unit: &super::lane_unit::Unit, loops: &[(usize, usize)],
    intrinsics: &native_calls::Intrinsics, quiet_store: &dyn Fn(usize) -> bool,
    global_reads: &quench_runtime::vm::global_facts::GlobalReads) -> BTreeMap<usize, Vec<InvariantRead>> {
    let mut hoists = BTreeMap::new();
    let boundaries = leaders(&unit.instructions);
    for &(header, back) in loops {
        let body = &unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|candidate| writes(*candidate)).collect();
        let stored: BTreeSet<Atom> = body.iter().filter(|candidate| stores_own(**candidate))
            .map(|candidate| candidate.atom_index()).collect();
        // An element store or an append may grow its array, which changes its `length`; a store
        // `quiet_store` proves through a typed view changes none.
        let resizes = body.iter().enumerate().any(|(offset, candidate)| {
            let pc = header + offset;
            (stores_element(*candidate) && !quiet_store(pc)) || native_calls::appends(intrinsics, pc)
        });
        let mut reads: Vec<InvariantRead> = body.iter().enumerate().filter_map(|(offset, candidate)| {
            let pc = header + offset;
            if matches!(crate::compiled::performed(candidate), Some(quench_runtime::staged::Operation::LoadCapture { .. }))
                && matches!(unit.origins[pc], super::lane_unit::Origin::Caller(_)) {
                return Some(InvariantRead { pc, owner: None, rooted: stored.is_empty(), callee: None });
            }
            if !matches!(crate::compiled::performed(candidate), Some(quench_runtime::staged::Operation::GetField { .. })) { return None; }
            // A native method's read folds to the function its holder holds (`fold_native_member`): its receiver,
            // a string primitive for `charCodeAt`, is no object whose field the entry would hoist.
            if matches!(global_reads.get(&pc), Some(quench_runtime::vm::global_facts::GlobalRead::NativeMember { .. })) { return None; }
            match candidate.field_lookup() {
            Some(FieldLookup::Atom { atom, base, .. }) if !stored.contains(&atom) && !(resizes && names_length(program, atom)) => {
                let owner = invariant_field_owner(unit, header, pc, base.register_index()?, &written, &boundaries)?;
                Some(InvariantRead { pc, owner: Some(owner), rooted: false, callee: None })
            }
            _ => None,
        }}).collect();
        // An inlined capture belongs to the actual closure admitted at its
        // marker, not merely to a function body or an observed environment.
        // Its invariant callee read must run first on the entry edge.
        let captures: Vec<_> = body.iter().enumerate().filter_map(|(offset, instruction)| {
            let pc = header + offset;
            let super::lane_unit::Origin::Inlined { frame, .. } = unit.origins[pc] else { return None };
            if !matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) { return None; }
            let read = invariant_callee_read(unit, reads.iter().map(|read| read.pc), frame, &boundaries).is_some();
            // A function-bound callee the loop neither writes nor reads again is held at entry.
            let callee = (!read).then(|| entry_callee(unit, frame, &written, body)).flatten();
            (read || callee.is_some()).then_some(InvariantRead { pc, owner: None, rooted: false, callee })
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
            if !matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) || instruction.result_register() != owner { return None; }
            owner = instruction.register_b();
        }
    }
    (owner < unit.caller_registers && !written.contains(&owner)).then_some(owner)
}

/// The caller register a function-bound inlined frame's closure sits in, where the loop writes
/// it nowhere and stores no capture: the closure it holds at loop entry is every iteration's
/// callee, so its captures are read once on the entry edge (`Lowering::admitted_callee_read`).
/// `None` for an identity-bound or alternative frame, or a callee the loop may change.
fn entry_callee(unit: &super::lane_unit::Unit, frame: usize, written: &BTreeSet<Register>,
    body: &[WideInstruction]) -> Option<Register> {
    let call = unit.frames[frame];
    let marker = unit.markers[frame];
    let register = call.callee_register;
    (call.target.callee == CalleeBinding::Function && call.alternative.is_none()
        && !unit.frames.iter().any(|other| other.alternative == Some(marker))
        && register < unit.caller_registers && !written.contains(&register)
        && !body.iter().any(|instruction| matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::StoreCapture { .. }))))
        .then_some(register)
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
                if !matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) || instruction.result_register() != source { return false; }
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

/// The successors of `instruction` at `pc`: a branch table's entries, else its fall-through and jump target.
fn successors(pc: usize, instruction: WideInstruction) -> smallvec::SmallVec<[usize; 2]> {
    let layout = instruction.op().control_flow_layout();
    if layout == ControlFlowLayout::BranchTable {
        return quench_runtime::bytecode::branch_table_entries(pc, instruction).collect();
    }
    [layout.falls_through().then_some(pc + 1), layout.jumps().then(|| instruction.jump_target() as usize)]
        .into_iter().flatten().collect()
}

/// Forward must-alias analysis through inline argument copies and branch joins.
/// Only a single backedge is admitted, so every incoming state is complete when
/// visited. Unknown writes decline promotion; no runtime alias cache is needed.
fn loop_fields(program: &ResidualProgram, unit: &super::lane_unit::Unit,
    loops: &[(usize, usize)]) -> Vec<LoopField> {
    let mut fields: Vec<LoopField> = Vec::new();
    for &(header, back) in loops {
        if loops.iter().any(|&(other, end)| (other, end) != (header, back) && other <= back && header <= end)
            || (header..=back).any(|pc| successors(pc, unit.instructions[pc]).into_iter()
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
            if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) && instruction.result_flags() == 0
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
                let receiver = match instruction.field_store_register() {
                    Some(register) => Some(register),
                    None => match unit.origins[pc] {
                        super::lane_unit::Origin::Inlined { frame, .. } => Some(unit.frames[frame].receiver),
                        _ => None,
                    },
                };
                stores.push((receiver.and_then(|register| aliases[usize::from(register)]), instruction.atom_index(), pc));
            }
            let copied = (matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. }))).then(|| aliases[usize::from(instruction.register_b())]).flatten();
            for register in writes(instruction) { aliases[usize::from(register)] = None; }
            if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) { aliases[usize::from(instruction.result_register())] = copied; }
            let mut targets = successors(pc, instruction).into_iter().filter(|target| *target > pc && *target <= back).peekable();
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

/// What a region specializer is asked to produce: how much of the unit must run, how its
/// runs end, and which element views it may assume. The division and the facts it folds are
/// `VmFacts`, so AOT (`Frozen`) and JIT (`Online`) are two plans over one lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegionPlan {
    pub(crate) coverage: Coverage,
    pub(crate) completion: Completion,
    pub(crate) views: ElementViews,
    /// Whether loops may version their bounds on entry (`int_ranges::prove_loops`): an OSR site
    /// whose entries kept missing compiles again without (`LoopTier::missed_entry`).
    pub(crate) entry_bounds: bool,
}

/// Futamura's first projection for one region: the same `lower` a lane body, a static typed
/// region and a numeric region run, parameterized by its `RegionPlan`. AOT and JIT both call
/// this specializer; only the plan and the `VmFacts` division differ.
/// No entry-snapshot field reads: a lane body or a static typed region reads its fields
/// from the registers and caches, not from an OSR entry record.
pub(crate) static NO_ENTRY_FIELDS: [Option<u32>; 0] = [];

/// The proved loads of each field-cache site, flat and indexed by the site (`site_loads`).
pub(crate) type FieldSites = [Option<Vec<NativeFieldLoad>>];
/// The entry-record word of each unit PC whose field read the record answers, flat and
/// indexed by the PC.
pub(crate) type EntryWords = [Option<u32>];

/// The loads of field-cache `site`, if it has any.
pub(crate) fn site_loads(fields: &FieldSites, site: u16) -> Option<&Vec<NativeFieldLoad>> {
    fields.get(usize::from(site))?.as_ref()
}

/// The flat table holding each `(key, value)` at its key, sized to the largest key.
pub(crate) fn flat_table<T>(entries: impl IntoIterator<Item = (usize, T)>) -> Vec<Option<T>> {
    let entries: Vec<(usize, T)> = entries.into_iter().collect();
    let mut table: Vec<Option<T>> = Vec::new();
    table.resize_with(entries.iter().map(|(key, _)| key + 1).max().unwrap_or(0), || None);
    for (key, value) in entries { table[key] = Some(value); }
    table
}

pub(crate) struct RegionSpecializer<'p> {
    pub(crate) function_id: u32,
    pub(crate) unit: &'p super::lane_unit::Unit,
    pub(crate) helpers: LaneHelpers,
    pub(crate) fields: &'p FieldSites,
    pub(crate) global_reads: &'p quench_runtime::vm::global_facts::GlobalReads,
    pub(crate) facts: VmFacts<'p>,
    pub(crate) generic: &'p [usize],
    pub(crate) direct: &'p DirectSites,
    pub(crate) frame_captures: &'p [(usize, usize)],
    pub(crate) isa: &'p dyn cranelift_codegen::isa::TargetIsa,
    /// An OSR region's field reads answered from the entry record, by unit PC and entry
    /// word (`osr_loop::field_word`).
    pub(crate) entry_fields: &'p EntryWords,
    /// The unit PCs of element reads observed to hold Numbers (sorted): each leaves where its
    /// element is not one, and its result is typed a Number.
    pub(crate) number_reads: &'p [super::facts::ObservedRead],
    /// The registers whose values at entry were int32s: candidates for an int32 entry contract
    /// (`refine_inductive_inputs`), kept where every definition in the loop yields an int32.
    pub(crate) int_entries: &'p [(Register, IntRange)],
    /// The registers whose values at entry were doubles: with `int_entries`, candidates for a
    /// Number entry contract (`refine_inductive_inputs`) where an int32 one fails.
    pub(crate) number_entries: &'p [Register],
    /// The registers whose values at entry were strings: a loop array they hold is no view, its
    /// reads take the string row (`Lowering::string_or_row`), which reads any value.
    pub(crate) string_entries: &'p [Register],
    /// The capture slots that held typed arrays at compilation (`Observation::typed_captures`).
    pub(crate) typed_captures: &'p [u16],
    /// The capture slots that held int32s at compilation (`Observation::int_captures`).
    pub(crate) int_captures: &'p [u16],
    pub(crate) plan: RegionPlan,
    /// The gate's proof that this region's compile may run (minted by `specialize`).
    pub(crate) admitted: super::admission::CompiledUnit,
}

impl Specializer for RegionSpecializer<'_> {
    type Residual = LoweredRegion;

    fn specialize(self, program: &ResidualProgram) -> Result<Self::Residual, String> {
        lower(program, self.function_id, self.unit, self.helpers, self.fields,
            self.global_reads, self.facts, self.generic, self.direct, self.frame_captures,
            self.isa, self.entry_fields, self.number_reads, (self.int_entries, self.number_entries, self.string_entries), self.typed_captures, self.int_captures, self.plan.coverage, self.plan.completion, self.plan.views, self.plan.entry_bounds)
            .ok_or_else(|| format!("function {} does not lower under {:?}", self.function_id, self.plan))
    }
}

thread_local! {
    /// The first reason this thread's current lowering declined, and its PC (`usize::MAX`: none).
    static DECLINED: std::cell::Cell<Option<(&'static str, usize)>> = const { std::cell::Cell::new(None) };
    /// The opcodes this thread's current lowering leaves at because it has no lowering for them.
    static UNLOWERED: std::cell::RefCell<std::collections::BTreeSet<String>> = const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
}

/// Notes why the lowering declines, for `QUENCH_TRACE=aot-lanes`; the first reason stands.
fn declined(why: &'static str, pc: usize) {
    DECLINED.with(|declined| if declined.get().is_none() { declined.set(Some((why, pc))); });
}

/// Lowers `unit` (`lane_unit::unit`) as a lane body, whose field cache sites `fields`
/// gives the proved shapes and slots of; `None` where it does not lower.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_body(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &FieldSites,
    global_reads: &quench_runtime::vm::global_facts::GlobalReads,
    vm: VmFacts<'_>,
    generic: &[usize],
    direct: &DirectSites,
    frame_captures: &[(usize, usize)], entry: Option<usize>,
    number_reads: &[super::facts::ObservedRead],
    admitted: super::admission::CompiledUnit,
) -> Option<LoweredLane> {
    let isa = super::jit_backend::target_isa()?;
    DECLINED.with(|declined| declined.set(None));
    UNLOWERED.with(|unlowered| unlowered.borrow_mut().clear());
    let body = RegionSpecializer { function_id, unit, helpers, fields, global_reads, facts: vm,
        generic, direct, frame_captures, isa: &*isa, entry_fields: &NO_ENTRY_FIELDS, number_reads, int_entries: &[], number_entries: &[], string_entries: &[], typed_captures: &[], int_captures: &[], admitted,
        plan: RegionPlan { entry_bounds: true, coverage: entry.map_or(Coverage::Prefix, Coverage::PrefixFrom),
            completion: Completion::Body, views: ElementViews::Dense } };
    // The scalar entry lowers first: the body calls it only where it exists.
    let abi = scalar::ScalarAbi::of(program, function_id, unit, direct).filter(|_| entry.is_none());
    let scalar = abi.and_then(|abi| scalar::lowered(program, RegionSpecializer { generic: &[], facts: VmFacts { scalar: Some(abi), ..vm }, plan: scalar::plan(abi), ..body }));
    let region = RegionSpecializer { facts: VmFacts { scalar: abi.filter(|_| scalar.is_some()), ..vm }, ..body }.specialize(program)
        .inspect_err(|_| if helpers == LaneHelpers::Ahead && quench_runtime::trace::Channel::AotLanes.on() {
            let (why, pc) = DECLINED.with(|declined| declined.get()).unwrap_or(("a decline no note names", usize::MAX));
            let op = unit.instructions.get(pc).map(|instruction| format!("{:?}", instruction.op())).unwrap_or_default();
            let unlowered = UNLOWERED.with(|unlowered| unlowered.borrow().iter().cloned().collect::<Vec<_>>().join(","));
            eprintln!("aot-lanes: function {function_id}: lowering declines: {why} {op} [leaves at {unlowered}]");
        }).ok()?;
    // The proven entry lowers the same body with its parameters' entry guards taken as proved (`VmFacts::proven_entry`).
    // It is kept only where it folds exactly the body's Stable facts, which its publication takes once for both.
    let params = program.function_get(function_id as usize).is_some_and(|function| function.params > 0);
    let proven = (entry.is_none() && params).then(|| RegionSpecializer { facts: VmFacts { proven_entry: true, ..vm }, ..body }
        .specialize(program).ok()).flatten()
        .filter(|proven| proven.stable == region.stable && proven.fault_poll == region.fault_poll && proven.hoists_arrays == region.hoists_arrays
            && proven.entry_types.iter().any(|ty| *ty != Type::ANY))
        .map(|proven| (proven.function, proven.entry_types));
    super::residuals::report_clif(function_id, &region.function);
    let clif = native_dump_directory().map(|_| region.function.to_string());
    Some(LoweredLane { region, clif, scalar, proven })
}

/// What an ahead lane body reads of its image. Its leaves return their exit words, as a JIT lane's
/// do: a base body is entered only at its start (a record inside it has no resume stub to load
/// its pinned registers), and a base body leaves the same way.
#[derive(Clone, Copy)]
pub(crate) struct AheadLeaves<'a> {
    /// By unit PC, the calls the closed world proves whose records the body reads (`ProvenCall`).
    pub(crate) calls: &'a BTreeMap<usize, ProvenCall>,
    /// By constant pool slot, the heap value key (`HeapValueRecipe::String`) of each string
    /// constant the unit loads.
    pub(crate) strings: &'a BTreeMap<usize, u32>,
}

/// The position a failed entry contract takes in a lane body's leave set
/// (`LaneExit::Entry`): past every instruction, so it names none of them. A plan with it
/// proves no input contract.
pub(crate) fn entry_leave(unit: &super::lane_unit::Unit) -> usize {
    unit.instructions.len()
}

/// Build the typed residual and validate every value/CFG edge before publication.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &FieldSites,
    global_reads: &quench_runtime::vm::global_facts::GlobalReads,
    vm: VmFacts<'_>,
    generic: &[usize],
    direct: &DirectSites,
    frame_captures: &[(usize, usize)],
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    entry_fields: &EntryWords,
    number_reads: &[super::facts::ObservedRead],
    (int_entries, number_entries, string_entries): (&[(Register, IntRange)], &[Register], &[Register]),
    typed_captures: &[u16],
    int_captures: &[u16],
    coverage: Coverage,
    completion: Completion,
    views: ElementViews,
    entry_bounds: bool,
) -> Option<LoweredRegion> {
    // A loop entry may take the rotated residual of its proven condition.
    let residual = super::static_loops::specialize(program, unit, coverage.entry())
        .map(|instructions| super::lane_unit::Unit { instructions, origins: unit.origins.clone(),
            frames: unit.frames.clone(), markers: unit.markers.clone(), caller_registers: unit.caller_registers,
            registers: unit.registers, end: unit.end });
    let unit = residual.as_ref().unwrap_or(unit);
    let naming = super::NamingSymbols::enter(helpers);
    crate::dispatch_fast::lane_body_stats::lowering_function(function_id);
    let function = program.function_get(function_id as usize)?;
    let numeric = completion.numeric();
    if numeric && (!crate::js_compiled::function_candidate(function, program)
        || !scalar::numeric_unit(program, unit, completion != Completion::Number, direct) || function.params > unit.caller_registers) { return None; }
    if fields.iter().flatten().flatten().any(|field| !field.valid()) { declined("an invalid field site", usize::MAX); return None; }
    if unit.instructions.is_empty() || unit.instructions.len() > MAX_NATIVE_RECORDS {
        declined("past the native record count", usize::MAX);
        return None;
    }
    let signature = completion.signature(isa.pointer_type(), isa.default_call_conv());
    let mut native = ir::Function::with_name_signature(UserFuncName::user(LANE_REGION_NAMESPACE, function_id), signature);
    // Every access this lowering emits is tagged with its alias region (`regions::flags`), so the
    // function names them before anything verifies or compiles it (declaring again is a no-op).
    super::regions::declare_regions(&mut native);
    // The SSA builder's tables come back from the last lowering that finished, which
    // `finalize` left empty; one that gave up drops its context with it.
    let mut builder_context = super::jit_backend::builder::take_context();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let entry_block = builder.create_block();
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    let parameters = builder.block_params(entry_block).to_vec();
    let [context, entry_this, entry_captures, registers_base] = scalar::entry_words(&mut builder, completion, &parameters)?;
    let mut stage = emit(&mut builder);
    let payload = if numeric { stage.raw_const(Lane::I64, 0) } else {
        stage.raw_load(Lane::I64, regions::dispatch(), context, std::mem::offset_of!(CallContext, head.adapter_payload) as i32) };
    let fault_poll = match coverage { Coverage::Loop { poll, .. } => poll, _ => true }
        && super::FAULT_POLL_SUPPORTED && quench_runtime::bytecode::back_edges_of(unit.instructions.iter().copied()).next().is_some();
    let mut seen_headers: CompoundBitSet = CompoundBitSet::with_capacity(unit.instructions.len());
    let mut shared_headers = CompoundBitSet::with_capacity(unit.instructions.len());
    for (pc, instruction) in unit.instructions.iter().enumerate() {
        if !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Jump)) { continue; }
        let target = instruction.jump_target() as usize;
        if target > pc { continue; }
        if seen_headers.contains(target) { shared_headers.insert(target); }
        seen_headers.insert(target);
    }
    let interrupt = super::native_interrupt_pointer(&mut builder, context, fault_poll);
    let helpers = if let LaneHelpers::Linked(symbol, narrow_table) = helpers {
        let signature = builder.import_signature(native_lane_helpers_signature(isa));
        let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
            super::native_object::NATIVE_FUNCTION_NAMESPACE, symbol));
        let target = builder.import_function(ir::ExtFuncData { name: ir::ExternalName::user(name),
            signature, colocated: false, patchable: false });
        LaneHelpers::Bound(emit(&mut builder).call_function(target, &[])[0], narrow_table)
    } else { helpers };
    let interrupt_owner = fault_poll.then(|| polls::interrupt_owner_call(&mut builder, helpers, context, isa));
    let count = usize::from(unit.registers);
    let live = if unit.registers > quench_runtime::compile::liveness::ROOT_MAP_REGISTERS {
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
        long: vec![None; count],
        float: vec![None; count],
    };
    // An alternative of a call's switch starts a block: a callee other than the one before
    // it branches there (`Lowering::inline_call`).
    let mut leaders = leaders(&unit.instructions);
    let entry_pc = coverage.entry();
    if !matches!(unit.origins.get(entry_pc), Some(super::lane_unit::Origin::Caller(_))) { declined("the entry is not the caller's", entry_pc); return None; }
    leaders.insert(entry_pc);
    for alternative in unit.frames.iter().filter_map(|frame| frame.alternative) {
        if alternative < unit.instructions.len() { leaders.insert(alternative); }
    }
    // A handler's block is entered by the throws it catches, edges no jump names.
    let catches = catches::catch_edges(program, function, unit, coverage, completion);
    for catch in catches.iter().flatten() { leaders.insert(catch.target); }
    let blocks: Vec<_> = (0..unit.instructions.len()).map(|pc| leaders.contains(pc).then(|| builder.create_block())).collect();
    let (leave, stop) = (builder.create_block(), builder.create_block());
    builder.append_block_param(leave, types::I32);
    builder.append_block_param(leave, types::I64);
    builder.append_block_param(stop, types::I64);
    builder.set_cold_block(leave);
    builder.set_cold_block(stop);
    // A numeric region leaves at its calls (`native_calls` is off), so it types none of them.
    let numeric_reads: Vec<super::facts::ObservedRead>;
    let number_reads = if numeric {
        numeric_reads = number_reads.iter().copied()
            .filter(|read| !unit.instructions.get(read.pc).is_some_and(|instruction| matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Call(_)))))
            .collect();
        &numeric_reads[..]
    } else { number_reads };
    let observed = super::facts::Observed(number_reads);
    let mut lowering = Lowering {
        builder,
        program,
        unit,
        params: usize::from(function.params),
        function_id,
        helpers,
        fields,
        entry_fields,
        observed,
        int_entries,
        number_entries,
        int_captures,
        interrupt,
        fault_poll,
        trips: None,
        reentries: smallvec::SmallVec::new(),
        polls: smallvec::SmallVec::new(),
        poll_latches: super::SecondaryMap::new(),
        shared_headers,
        global_reads,
        vm,
        stable: Default::default(),
        generic,
        proving_entry: false, left_here: false, held_view: None, hole_leaves: false, entry_exit: None,
        direct,
        frame_captures,
        context,
        payload,
        registers_base,
        entry_words: (entry_this, entry_captures),
        callee_slots: Vec::new(),
        declared_definitions: Vec::new(),
        declared_this: None,
        declared_rest: None,
        declared_members: Vec::new(),
        declared_member_start: None,
        registers,
        written: CompoundBitSet::with_capacity(count),
        consumed: CompoundBitSet::with_capacity(unit.instructions.len()),
        blocks,
        exits: std::iter::repeat_with(Vec::new).take(unit.instructions.len() + 1).collect(),
        shared_exits: Vec::new(),
        demands: demand::analyze(program, unit, &leaders, &live),
        entry: None,
        native: 0,
        backedge: false,
        pending: Vec::new(),
        call_out_stop: None,
        // A static typed region's calls are guarded leaves (`guarded_call_instruction`); here any call-like one is native.
        native_calls: completion.status_and_pc()
            && unit.instructions.iter().any(|instruction| instruction.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::Call),
        call_budget: None,
        facts: super::facts::Facts::default(),
        live,
        valid: true,
        coverage,
        completion,
        number_returns: smallvec::SmallVec::new(),
        hoists: BTreeMap::new(),
        unit_leaders: std::cell::OnceCell::new(),
        intrinsics: Vec::new(),
        hoisted: BTreeMap::new(),
        scalar_values: BTreeMap::new(),
        prototype_clear: BTreeMap::new(),
        int_only: BTreeSet::new(),
        construct_ready: Vec::new(),
        loop_arrays: Vec::new(), loop_strings: Vec::new(), string_vars: BTreeMap::new(), pending_unit: None, loop_shapes: Vec::new(), arrays_fixed: false,
        array_vars: BTreeMap::new(),
        views,
        entry_bounds,
        loop_fields: Vec::new(),
        loop_stores: BTreeMap::new(),
        ranges: int_ranges::IntRanges::new(0),
        entering: Vec::new(),
        at: 0,
        forward: Vec::new(), sealable: CompoundBitSet::new(),
        arrived: BTreeMap::new(),
        entered: CompoundBitSet::with_capacity(unit.instructions.len()),
        finishes: false,
        leave,
        stop,
        raises: catches::raises(&catches),
        catches,
    };
    let instructions = unit.instructions.clone();
    for instruction in &instructions {
        for register in writes(*instruction).into_iter().map(usize::from).filter(|r| *r < count) {
            lowering.written.insert(register);
        }
    }
    let mut initial = completion.initial_types(count, usize::from(function.params));
    if let Coverage::Loop { header, backedge, .. } = coverage {
        seed_constant_entries(&instructions, program, (header, backedge), &mut initial);
    }
    let heap = literals::heap_primitive_pcs(entry_pc, &initial, &instructions, &leaders, &program.constants, lowering.native_calls, &lowering.raises);
    if !numeric {
        scalar::require_entry_types(program, function, &instructions, |pc| generic.contains(&entry_leave(unit)) || heap.contains(pc), &lowering.written, &mut initial);
        scalar::declared_entry_types(function, &lowering.written, &mut initial);
        int_inputs::closed_entry_types(vm, function_id, function, &mut initial);
        lowering.declared_definitions = declared::declared_local_types(program, function);
        int_inputs::closed_local_types(vm, function_id, function, &mut lowering.declared_definitions);
        lowering.declared_this = declared::declared_this(program, function);
        lowering.declared_rest = declared::declared_rest_read(program, function, &lowering.written);
        lowering.declared_members = declared::declared_member_reads(program, function, &lowering.written);
        lowering.declared_member_start = function.source_span.map(|span| span.start);
    }
    let own_shape = |pc: usize| {
        let instruction = instructions[pc];
        if !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) || instruction.result_flags() != 0 { return None; }
        // A native method's read names its holder's function whatever its receiver (`fold_native_member`):
        // a string primitive's `charCodeAt` proves no object shape of it.
        if matches!(global_reads.get(&pc), Some(quench_runtime::vm::global_facts::GlobalRead::NativeMember { .. })) { return None; }
        let FieldLookup::Atom { base, cache_site, .. } = instruction.field_lookup()? else { return None };
        let [field] = site_loads(fields, cache_site)?.as_slice() else { return None };
        field.own_direct().then_some((base.register_index()?, field.shape))
    };
    lowering.facts = super::facts::analyze(entry_pc, &initial, &instructions, &leaders, &program.constants, &writes, &own_shape, lowering.native_calls, &lowering.raises, super::facts::Seeds::observing(observed));
    lowering.intrinsics = native_calls::intrinsic_calls(unit, global_reads, |pc| direct.contains_key(&pc));
    let loops = local_loops(unit, &program.constants, entry_pc, &lowering.intrinsics, global_reads);
    // One entry query of the call depth replaces each binding's helper with its cached
    // target probe (`cached_call_target`). A completed call leaves the depth it found.
    if !numeric && unit.frames.iter().any(|frame| frame.target.callee == CalleeBinding::Function) {
        let helper = lowering.helpers.target(&mut lowering.builder, LaneHelper::CallBudget);
        lowering.call_budget = Some(emit_lane_helper_call(&mut lowering.builder, lowering.payload, helper, &[]));
    }
    if coverage.selected_entry() {
        lowering.written = lowering.facts.written(&instructions, &writes, count);
        // Backedge definitions choose a numeric entry signature. Conformance
        // proves the external input once; every subsequent edge carries the
        // same verified SSA view. This is an admission requirement, not a fact
        // assumed about the source program (a declared parameter type is checked
        // the same way: `scalar::declared_entry_types`).
        let mut refined = false;
        for (input, carried) in initial.iter_mut().take(usize::from(unit.caller_registers)).zip(lowering.facts.backedge().iter().map(|ty| ty.machine())) {
            if matches!(carried, Type::INT32 | Type::NUMBER) && *input != carried {
                *input = carried;
                refined = true;
            }
        }
        // An integer induction value can compare against an invariant integer
        // input directly. This selects a checked entry contract; a fractional
        // or wider bound resumes the original header with its value unchanged.
        let instruction = instructions[entry_pc];
        if !lowering.facts.backedge().is_empty() && matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::CompareJump { .. }))
            && matches!(super::operations::NumberRelation::from_operator(instruction.binary_operator_field()),
                    Some(super::operations::NumberRelation::LessThan | super::operations::NumberRelation::LessEqual
                        | super::operations::NumberRelation::GreaterThan | super::operations::NumberRelation::GreaterEqual)) {
            let operands = [instruction.operand_b(), instruction.operand_c()];
            if operands.iter().any(|operand| super::facts::operand_type(&program.constants, *operand,
                |register| lowering.facts.backedge().get(usize::from(register)).copied().unwrap_or(Type::ANY)).is_int32()) {
                for register in operands.into_iter().filter_map(Operand::register_index)
                    .filter(|register| *register < unit.caller_registers && !lowering.written.contains(usize::from(*register))) {
                    if initial[usize::from(register)] == Type::NUMBER {
                        initial[usize::from(register)] = Type::INT32;
                        refined = true;
                    }
                }
            }
        }
        refined |= lowering.seed_loop_ints(&mut initial, &loops);
        if refined { lowering.facts = super::facts::analyze(entry_pc, &initial, &instructions, &leaders, &program.constants, &writes, &own_shape, lowering.native_calls, &lowering.raises, super::facts::Seeds::observing(observed)); }
        lowering.refine_inductive_inputs(&mut initial, &instructions, &leaders, &loops, &own_shape);
    }
    lowering.refine_loop_inputs(entry_pc, &initial, &instructions, &leaders, &loops, &own_shape);
    lowering.require_object_latches(&instructions, &leaders, &loops);
    for &pc in generic { lowering.facts.widen(pc); }
    if !numeric { lowering.entry = Some(lowering.entry_facts(&instructions, &loops, &leaders)); }
    // A preliminary run of the facts over the arrays the loops index without their captured reads
    // (`loop_arrays` before the hoists): a typed or double view's store the run proves a Number store
    // changes no length, so it does not keep a loop's `length` from its entry (`invariant_reads`).
    lowering.loop_arrays = loop_arrays::loop_arrays(unit, &loops, &leaders, |_, _| false,
        lowering.views, typed_captures, &lowering.intrinsics, |pc| coverage.lowered(pc));
    lowering.entering = lowering.entering_ranges(entry_pc);
    lowering.entering = lowering.kept_entering(entry_pc, &initial, &instructions, &leaders, &own_shape);
    let preliminary = lowering.prove_ranges(entry_pc, &initial, &instructions, &leaders, &own_shape, &loops);
    let quiet_store = |pc: usize| preliminary.numeric_store(pc) && lowering.typed_access(pc) == Some(super::facts::TypedAccess::Store);
    let hoists = invariant_reads(program, unit, &loops, &lowering.intrinsics, &quiet_store, global_reads);
    lowering.hoists = hoists;
    lowering.loop_arrays = loop_arrays::loop_arrays(unit, &loops, &leaders,
        |header, at| lowering.hoists.get(&header).is_some_and(|reads| reads.iter().any(|read|
            read.pc == at && read.owner.is_none() && matches!(crate::compiled::performed(&instructions[at]), Some(quench_runtime::staged::Operation::LoadCapture { .. })))),
        lowering.views, typed_captures, &lowering.intrinsics, |pc| coverage.lowered(pc));
    let arrays = std::mem::take(&mut lowering.loop_arrays);
    lowering.loop_arrays = arrays.into_iter().filter(|array| !lowering.string_base(array)
        && !matches!(array.source, ArraySource::Register(register) if string_entries.contains(&register))).collect();
    lowering.loop_strings = loop_strings::loop_strings(unit, &loops, &lowering.intrinsics, |pc| coverage.lowered(pc));
    lowering.entering = lowering.entering_ranges(entry_pc);
    lowering.entering = lowering.kept_entering(entry_pc, &initial, &instructions, &leaders, &own_shape);
    lowering.ranges = lowering.prove_ranges(entry_pc, &initial, &instructions, &leaders, &own_shape, &loops);
    lowering.arrays_fixed = lowering.arrays_stay_put(&leaders, &writes);
    lowering.loop_fields = loop_fields(program, unit, &loops);
    for field in &lowering.loop_fields {
        lowering.hoists.entry(field.header).or_default().push(InvariantRead { pc: field.reads[0], owner: Some(field.owner), rooted: false, callee: None });
    }
    // Field admissions establish the actual counterpart types used to select
    // capture views. Callee reads/bindings must precede their inlined captures.
    for reads in lowering.hoists.values_mut() {
        reads.sort_by_key(|read| (matches!(crate::compiled::performed(&instructions[read.pc]), Some(quench_runtime::staged::Operation::LoadCapture { .. })),
            matches!(unit.origins[read.pc], super::lane_unit::Origin::Inlined { .. })));
    }
    (lowering.forward, lowering.sealable) = edges::leader_edges(&instructions, &leaders, entry_pc);
    // No forward edge of the bytecode's enters a handler's block: its proofs are none, and it
    // seals once the region ends.
    for (pc, catch) in lowering.catches.iter().enumerate().filter_map(|(pc, catch)| Some((pc, (*catch)?))) {
        lowering.forward[catch.target] = None;
        lowering.sealable.remove(catch.target);
        // A throw of an inlined callee reaches its caller's handler block after the walk passed
        // it: the block is lowered when the walk reaches it, as one an edge entered.
        if catch.target < pc { lowering.entered.insert(catch.target); }
    }
    lowering.entered.insert(entry_pc);
    lowering.stable.start = entry_pc;
    let mut proven_types: Box<[Type]> = Box::default();
    {
        let pc = entry_pc;
        let block = lowering.builder.create_block();
        lowering.goto(block, &[]);
        lowering.builder.switch_to_block(block);
        lowering.begin_trips();
        if !matches!(lowering.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_))) {
            return None;
        }
        let undefined = lowering.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
        for register in 0..count {
            // A numeric entry's window holds its parameters alone; a scalar entry has none.
            let in_window = match completion {
                Completion::Scalar(_) => false,
                Completion::Number => register < usize::from(function.params),
                Completion::Body => register < usize::from(lowering.unit.caller_registers) && lowering.is_live(pc, register as Register),
            };
            let bits = if in_window {
                lowering.raw_load(Lane::I64, regions::frame(), registers_base, register_offset(register as Register))
            } else {
                undefined
            };
            lowering.builder.def_var(lowering.registers.tagged[register], bits);
        }
        let target = lowering.blocks.get(pc).copied().flatten()?;
        let mut state = State::canonical(count);
        if let Some(budget) = lowering.call_budget {
            // Every body edge is dominated by this admission. The closed region
            // cannot change thread call depth before leaving to materialize.
            let depth = lowering.unit.recovery_depth().max(1) as i64;
            let admitted = lowering.raw_compare_imm(Relation::UnsignedGreaterEqual, budget, depth);
            let miss = lowering.guard_exit(&state, pc as u32);
            emit_js_guard(&mut lowering.builder, admitted, miss);
        }
        if numeric { lowering.enter_numeric_parameters(&mut state, pc, &parameters); }
        if fault_poll {
            // Publication lends a resumable page for this entire no-GC entry.
            // If unavailable, leave before any guest effect through ordinary recovery.
            let miss = lowering.guard_exit(&state, pc as u32);
            let available = lowering.raw_compare_imm(Relation::NotEqual, interrupt, 0);
            emit_js_guard(&mut lowering.builder, available, miss);
        }
        // A proven entry's caller proved each argument has its parameter's entry type (`call_frames::call_target`), so the
        // guards those types make are not run: the state holds them, and the entry conform proves them with no test. A
        // shape is not held this way: its guard runs in every entry.
        if lowering.vm.proven_entry {
            let entered: Vec<Type> = lowering.facts.at(pc).map(|types| types.iter().copied().take(usize::from(function.params)).collect()).unwrap_or_default();
            for (index, ty) in entered.iter().copied().enumerate() {
                if !lowering.is_live(pc, index as Register) || ty.object_shape().is_some() { continue; }
                state.values[index] = RegisterFact { tagged: state.values[index].tagged, ..RegisterFact::definition(ty, Representation::Tagged) };
            }
            proven_types = entered.into_boxed_slice();
        }
        lowering.entry_conform(&mut state, pc, coverage);
        // A region whose sole entry is this loop header has no earlier source
        // edge on which to hoist its invariant reads. Admit them on the native
        // entry edge, before the header and every possible loop effect. A failed
        // entry proof declines the region (the lane runs the header itself), not
        // an exit: the old lowering's entry admission declines the same arrays.
        // A bounded range reads its fields at their sites; hoisting them to a shared entry
        // guard would make every call leave at the entry.
        lowering.proving_entry = true;
        lowering.hoist_entry(&mut state, pc);
        lowering.proving_entry = false;
        lowering.goto(target, &[]);
    }
    let summarized = completion == Completion::Body && matches!(coverage, Coverage::Prefix);
    let mut census = clif_census::Census::new();
    let emitted = |lowering: &Lowering<'_, '_>| lowering.builder.func.dfg.num_insts();
    let mut mark = emitted(&lowering);
    let mut block_mark = lowering.builder.func.dfg.num_blocks();
    if let Some(census) = census.as_mut() { census.part(clif_census::Part::Entry, mark); }
    let mut state = State::canonical(count);
    let mut reachable = false;
    for (pc, instruction) in instructions.iter().copied().enumerate() {
        if !coverage.lowered(pc) { continue; }
        if let Some(block) = lowering.blocks[pc] {
            if reachable {
                lowering.hoist_entry(&mut state, pc);
                lowering.conform(&mut state, pc);
                lowering.arrive(&state, pc);
                lowering.entered.insert(pc);
                lowering.goto(block, &[]);
            }
            // Marker alternatives add real native edges beyond the bytecode
            // analysis's Nop fallthrough. Missing type facts select canonical
            // guarded lowering; they do not make an entered block unreachable.
            if !lowering.entered.contains(pc) {
                reachable = false;
                continue;
            }
            lowering.switch_to(block, lowering.sealable.contains(pc));
            state = lowering.entry_state(pc, count);
            lowering.keep_shared_proofs(&mut state, pc);
            lowering.seed_loop_shapes(&mut state, pc);
            reachable = true;
        }
        if !reachable {
            continue;
        }
        lowering.at = pc;
        if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Edges, now - mark); mark = now; block_mark = lowering.builder.func.dfg.num_blocks(); }
        reachable = lowering.instruction(&mut state, pc, instruction);
        if let Some(census) = census.as_mut() {
            let now = emitted(&lowering);
            if census.sample(instruction.op()) {
                let insts: Vec<String> = (mark..now).map(|index| format!("{}", lowering.builder.func.dfg.display_inst(ir::Inst::new(index)))).collect();
                eprintln!("QUENCH_CLIF_SAMPLE {:?} pc={pc} n={}\n  {}", instruction.op(), now - mark, insts.join("\n  "));
            }
            census.op(instruction.op(), now - mark); mark = now;
            let blocks = lowering.builder.func.dfg.num_blocks();
            census.blocks(instruction.op(), blocks - block_mark); block_mark = blocks;
        }
    }
    if reachable {
        lowering.exit(&mut state, coverage.exit_pc(instructions.len()) as u32);
    }
    for (header, latch) in std::mem::take(&mut lowering.poll_latches).iter() {
        if let Some(latch) = latch {
            lowering.builder.switch_to_block(*latch);
            lowering.emit_fault_poll();
            lowering.goto(header, &[]);
        }
    }
    // A prefix pays for its entry with a loop, or with the calls it runs inlined.
    if matches!(coverage, Coverage::PrefixFrom(_)) && !lowering.backedge && unit.frames.is_empty() { return None; }
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Tail, now - mark); mark = now; }
    let named = lowering.named_leaves();
    for GuardExit { block, values, homed, virtuals, pc, line, .. } in lowering.take_leaves() {
        lowering.switch_to(block, true);
        if helpers == LaneHelpers::Ahead && quench_runtime::trace::Channel::AotLanes.on() {
            eprintln!("aot-lanes: function {function_id}: a leave at {pc} (lowering line {line})");
        }
        let mut leaving = State { values, proofs: LocalProofs { homed, ..LocalProofs::default() }, refinements: smallvec::SmallVec::new(), virtuals };
        lowering.record_line(line);
        lowering.leave_at(&mut leaving, pc);
    }
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Leaves, now - mark); mark = now; }
    lowering.fill_call_out_stop();
    if lowering.fill_exits().is_none() { declined("an exit fill declines", usize::MAX); return None; }
    let exits = ExitTable::of_unit(unit, |pc, frame| lowering.inlined_stores(pc, frame), |pc| named.get(pc).copied().flatten());
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Exits, now - mark); mark = now; }
    lowering.builder.switch_to_block(stop);
    let word = lowering.builder.block_params(stop)[0];
    let status = lowering.raw_const(Lane::I32, i64::from(NATIVE_JS_STOPPED));
    lowering.leave_with(status, Some(word));
    lowering.builder.switch_to_block(leave);
    let results = lowering.builder.block_params(leave).to_vec();
    lowering.flush_trips();
    lowering.leave(&results);
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Tail, now - mark); census.report(function_id, now);
        let mut opcodes: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
        for index in 0..now { *opcodes.entry(format!("{:?}", lowering.builder.func.dfg.insts[ir::Inst::new(index)].opcode())).or_default() += 1; }
        let blocks = lowering.builder.func.layout.blocks().count();
        let cold = lowering.builder.func.layout.blocks().filter(|block| lowering.builder.func.layout.is_cold(*block)).count();
        eprintln!("QUENCH_CLIF_BLOCKS {blocks} {cold}");
        eprintln!("QUENCH_CLIF_OPCODES {}", opcodes.iter().map(|(name, count)| format!("{name}={count}")).collect::<Vec<_>>().join(" ")); }
    // A lane body is a whole function body and must complete; so must an inlined unit, whose
    // callees rejoin it. A loop or range over the function's own code completes by its exits
    // and backedge, which the plan emits where the interpreter resumes.
    let must_finish = match coverage {
        Coverage::Prefix | Coverage::PrefixFrom(_) | Coverage::Full => true,
        Coverage::Loop { .. } => !unit.frames.is_empty(),
    };
    if !lowering.valid || (completion.status_and_pc() && must_finish && !lowering.finishes) {
        if lowering.valid { declined("the body does not finish", usize::MAX); }
        return None;
    }
    let number_result = numeric.then(|| lowering.finish_number_returns());
    let summary = summarized.then(|| TypeSummary { result: lowering.returned_type(&instructions, &leaders, &writes) });
    let native_count = lowering.native;
    let hoists_arrays = !lowering.loop_arrays.is_empty();
    let mut polls = std::mem::take(&mut lowering.polls);
    let reentries = std::mem::take(&mut lowering.reentries);
    let stable = lowering.stable.take();
    lowering.builder.seal_all_blocks();
    lowering.builder.finalize(isa.frontend_config());
    super::jit_backend::builder::keep_context(builder_context);
    let decided = known_branches::decide(&mut native);
    if clif_census::enabled() { eprintln!("QUENCH_CLIF_DECIDED {decided}"); }
    if let Some(owner) = interrupt_owner { specialize_closed_polls(&mut native, &reentries, &mut polls, owner); }
    if naming.folded() { declined("a slow path no helper names", usize::MAX); return None; }
    Some(LoweredRegion { function: native, native: native_count, fault_poll, number_result, hoists_arrays, stable, summary, entry_types: proven_types, exits })
}

impl Lowering<'_, '_> {
    /// A bitwise operand's ToInt32 (`frame::operand_to_int32`).
    fn operand_to_int32(&mut self, state: &mut State, operand: Operand, exit: ir::Block) -> Option<ir::Value> {
        frame::operand_to_int32(&mut self.builder, &mut self.registers, state, &self.program.constants, operand, exit).map(quench_runtime::staged::Int32::word)
    }

    /// Whether only the result's ToInt32 is read of `register`, written at `pc`: the next
    /// instruction is its one reader, a bitwise operator, which applies ToInt32 to its operands.
    /// A sum's or difference's wrapping int32 result then serves for int32 operands, and
    /// ToInt32 of its exact double for any others. Not through a further sum: ToInt32 does not
    /// commute with adding a non-integer (trunc(-3 + 0.5) differs from trunc(-3 + 2^32 + 0.5)
    /// mod 2^32), so V8 truncates chains only of int32 inputs.
    fn truncated(&self, pc: usize, register: Register) -> bool {
        let (at, register) = self.reader(pc + 1, register);
        let Some(next) = self.unit.instructions.get(at).copied() else { return false };
        if !quench_runtime::staged::meaning::means(next, quench_runtime::staged::meaning::Meaning::Binary) || next.returns_from_frame() || next.writes_current_this() { return false; }
        let reads = [next.operand_b(), next.operand_c()].iter().filter(|operand| operand.register_index() == Some(register)).count();
        if reads == 0 || !(next.result_register() == register || self.dead_after(at, register)) { return false; }
        let operator = next.binary_operator();
        if crate::compiled::number_bitwise(next, operator)
            .is_some_and(|bitwise| bitwise != super::operations::NumberBitwise::ShiftRightUnsigned) {
            return true;
        }
        false
    }

    /// Whether the int32 remainder written to `register` at `pc` is read only by a sum or difference
    /// (`Add` or `Subtract`) whose own result only its ToInt32 is read of (`truncated`), and `register`
    /// is dead past that sum unless the sum writes it. ToInt32 reads a `-0` and a `0` alike, and a sum
    /// with either differs from the other in the sign of a zero only, so the remainder's sign is unread.
    pub(crate) fn truncated_sum_operand(&self, pc: usize, register: Register) -> bool {
        let (at, reader) = self.reader(pc + 1, register);
        let Some(next) = self.unit.instructions.get(at).copied() else { return false };
        if !quench_runtime::staged::meaning::means(next, quench_runtime::staged::meaning::Meaning::Binary) || next.returns_from_frame()
            || next.writes_current_this() || next.result_flags() != 0 { return false; }
        if !matches!(crate::compiled::number_arithmetic(next, next.binary_operator()),
            Some(quench_runtime::vm::operations::NumberArithmetic::Add | quench_runtime::vm::operations::NumberArithmetic::Subtract)) { return false; }
        let reads = [next.operand_b(), next.operand_c()].iter().filter(|operand| operand.register_index() == Some(reader)).count();
        reads > 0 && (next.result_register() == reader || self.dead_after(at, reader)) && self.truncated(at, next.result_register())
    }

    /// Whether only the ToInt32 of the int32 result written to `register` at `pc` is read: through
    /// at most `TRUNCATION_CHAIN` sums or differences with an int32 constant, into a bitwise
    /// operator (`truncated`). Each sum adds less than 2^31 to an exact integer, so a chain from a
    /// value under 2^52 keeps every double exact, and ToInt32 of an exact integer is its low 32
    /// bits: the wrapping int32 steps compute it. Only int32 constants join the chain: a
    /// non-integer would not commute with ToInt32 (`truncated`).
    pub(crate) fn truncated_through_sums(&self, pc: usize, register: Register) -> bool {
        let (mut pc, mut register) = (pc, register);
        for _ in 0..=TRUNCATION_CHAIN {
            if self.truncated(pc, register) { return true; }
            let Some(next) = self.int32_constant_sum(pc, register) else { return false };
            (pc, register) = next;
        }
        false
    }

    /// Whether the int32 product written to `register` at `pc` is read only through its ToInt32, by
    /// a sum or difference as its next instruction whose other operand is another int32 register
    /// (`state`, the facts before `pc`, prove it int32 and the product does not write it), and then
    /// through `truncated_through_sums`. Each added int32 keeps the chain's sums exact in binary64
    /// as an int32 constant does, so the wrapping product and sum give the low 32 bits of the
    /// exact sum: its ToInt32, as `(a * 3 + i) | 0` (`int_mul`).
    pub(crate) fn truncated_through_int32_sum(&self, state: &State, pc: usize, register: Register) -> bool {
        let (at, reader) = self.reader(pc + 1, register);
        if at != pc + 1 || reader != register { return false; }
        let Some(next) = self.unit.instructions.get(at).copied() else { return false };
        if !quench_runtime::staged::meaning::means(next, quench_runtime::staged::meaning::Meaning::Binary) || next.returns_from_frame()
            || next.writes_current_this() || next.result_flags() != 0 { return false; }
        let Some(operation) = crate::compiled::number_arithmetic(next, next.binary_operator()) else { return false };
        if !operation.int32_projection_is_wrapping() { return false; }
        let int32_register = |operand: Operand| operand.register_index().filter(|other| *other != register)
            .is_some_and(|other| state.values[usize::from(other)].value.ty.is_subtype(Type::INT32));
        let [first, second] = [next.operand_b(), next.operand_c()];
        let paired = (first.register_index() == Some(register) && int32_register(second))
            || (second.register_index() == Some(register) && int32_register(first));
        if !paired || !(next.result_register() == register || self.dead_after(at, register)) { return false; }
        self.truncated_through_sums(at, next.result_register())
    }

    /// The sum or difference that next reads `register`, written at `pc`, with an int32 constant
    /// as its other operand and `register` dead past it: its PC and result register.
    fn int32_constant_sum(&self, pc: usize, register: Register) -> Option<(usize, Register)> {
        let (at, register) = self.reader(pc + 1, register);
        let next = self.unit.instructions.get(at).copied()?;
        use quench_runtime::bytecode::ImmediateRole as Role;
        let operator = matches!(next.op().immediate_role(), Role::BinaryOperator | Role::AdditionOperator | Role::MultiplicationOperator);
        if !operator || next.returns_from_frame() || next.writes_current_this() || next.result_flags() != 0 { return None; }
        let operation = crate::compiled::number_arithmetic(next, next.binary_operator())?;
        if !operation.int32_projection_is_wrapping() { return None; }
        let operands = [next.operand_b(), next.operand_c()];
        let int32_constant = |operand: Operand| operand.kind() == Some(OperandKind::Constant)
            && self.program.constants.get(usize::from(operand.payload())).and_then(super::facts::immediate).is_some_and(super::facts::is_int32);
        let [first, second] = operands;
        let paired = (first.register_index() == Some(register) && int32_constant(second))
            || (second.register_index() == Some(register) && int32_constant(first));
        (paired && (next.result_register() == register || self.dead_after(at, register))).then(|| (at, next.result_register()))
    }

    /// Whether `register` is dead past the instruction at `at`. An inlined callee's returning
    /// instruction is followed by its completion (`lane_unit`): moves and a jump out of the
    /// callee that share its origin, whose liveness row is the returning instruction's own. Past
    /// that completion the callee's frame is gone, so its registers are dead unless a completion
    /// move reads them.
    fn dead_after(&self, at: usize, register: Register) -> bool {
        use super::lane_unit::Origin;
        let origin = self.unit.origins.get(at).copied();
        if let Some(Origin::Inlined { frame, .. }) = origin
            && self.unit.origins.get(at + 1).copied() == origin {
            let inlined = self.unit.frames[frame];
            let own = (inlined.base..inlined.base + inlined.registers).contains(&register);
            let mut pc = at + 1;
            while own && self.unit.origins.get(pc).copied() == origin {
                let Some(instruction) = self.unit.instructions.get(pc).copied() else { return false };
                if instruction.op().copies_register() {
                    if instruction.register_b() == register { return false; }
                } else if instruction.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::Jump {
                    let target = instruction.jump_target() as usize;
                    let leaves = !matches!(self.unit.origins.get(target).copied(), Some(Origin::Inlined { frame: to, .. }) if to == frame);
                    return leaves;
                } else {
                    return false;
                }
                pc += 1;
            }
        }
        !self.is_live(at + 1, register)
    }

    /// Where `register` at `at` is next read, past an inlined completion's move and jump (`lane_unit`).
    fn reader(&self, at: usize, register: Register) -> (usize, Register) {
        let instruction = |at: usize| self.unit.instructions.get(at).copied();
        let (at, register) = match instruction(at) {
            Some(moved) if moved.op().copies_register() && moved.register_b() == register && !self.is_live(at + 1, register) => (at + 1, moved.result_register()),
            _ => (at, register),
        };
        match instruction(at) { Some(jump) if jump.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::Jump => (jump.jump_target() as usize, register), _ => (at, register) }
    }

    /// `state` past a join with `incoming`: each register's facts as both paths prove them.
    /// A join no verified fact describes invalidates the region.
    fn join_states(&mut self, state: &mut State, incoming: &State, pc: usize) {
        // A kept receiver never reaches a join: the block that would carry it allocates it.
        if !state.virtuals.is_empty() || !incoming.virtuals.is_empty() { self.valid = false; declined("a kept receiver reaches a join", pc); }
        // Only the same pre-branch SSA definition can survive both numeric arms.
        state.proofs.fields.retain(|field| incoming.proofs.fields.contains(field));
        state.join_kept_shapes(incoming);
        state.proofs.int32_projections.retain(|projection| incoming.proofs.int32_projections.contains(projection));
        state.proofs.numbers.retain(|projection| incoming.proofs.numbers.contains(projection));
        state.proofs.admitted_captures.retain(|view| incoming.proofs.admitted_captures.contains(view));
        state.proofs.meet_homed(&incoming.proofs.homed);
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
                }) else { self.valid = false; declined("a join no fact describes", pc); return; };
                // A tagged view no later use reads is dropped (`demand`): a leave rebuilds it from the unboxed view.
                fact.tagged = if current.tagged == incoming.tagged { current.tagged }
                    else if current.tagged.current() && incoming.tagged.current()
                        && (representation == Representation::Tagged || self.demand_from(pc + 1, index).tagged()) { TaggedView::Variable }
                    else { TaggedView::Unavailable };
                state.values[index] = fact;
            }
        }
    }

    /// The ToInt32 of `number` (`emit_number_to_int32`).
    fn number_to_int32(&mut self, number: ir::Value, exit: ir::Block) -> ir::Value {
        super::emit_number_to_int32(&mut self.builder, number, exit)
    }

    /// `register`'s int32 view (`Registers::int`).
    fn int_var(&mut self, register: usize) -> Variable {
        frame::register_int(&mut self.builder, &mut self.registers, register)
    }

    /// `register`'s float64 view (`Registers::float`).
    fn float_var(&mut self, register: usize) -> Variable {
        frame::register_float(&mut self.builder, &mut self.registers, register)
    }

    /// Leaves with `word`: a lane body stops with it as its result word (`stop`); a static
    /// typed region returns it as the source PC its canonical code continues at.
    fn return_at(&mut self, word: u64) {
        match self.completion {
            Completion::Body => {
                let word = self.raw_const(Lane::I64, word as i64);
                let stop = self.stop;
                self.goto(stop, &[word]);
            }
            Completion::Number | Completion::Scalar(_) => {
                let miss = self.raw_const(Lane::I32, NUMBER_ENTRY_MISS);
                let no_word = self.raw_const(Lane::I64, 0);
                self.leave(&[miss, no_word]);
            }
        }
    }

    /// A Number return of `register`: the staged return, after the interrupt check and the
    /// type proof the lane keeps (a register not proved Number invalidates the region).
    fn return_number(&mut self, state: &mut State, register: Register, pc: u32) {
        self.finishes = true;
        if !state.values[usize::from(register)].value.ty.is_subtype(Type::NUMBER) {
            self.valid = false;
            declined("a number return of a non-number", pc as usize);
            self.return_at(0);
            return;
        }
        self.interrupt_check(state, pc);
        let exit = self.guard_exit(state, pc);
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, quench_runtime::staged::Operation::Return { source: register });
    }

    /// The join of the types the body's own activation returns, from the facts at each return
    /// (`TypeSummary::result`). A return of the function's own record (`Origin::Caller`) returns the
    /// value its `Return` names, typed before it, or the result an operator flagged to return defines,
    /// typed after it. A return the facts do not type joins `ANY`; a body that never returns natively is `ANY`.
    fn returned_type(&self, instructions: &[WideInstruction], leaders: &CompoundBitSet,
        written: &dyn Fn(WideInstruction) -> super::facts::Writes) -> Type {
        let native_calls = self.native_calls;
        let mut defined: Vec<Option<Type>> = vec![None; instructions.len()];
        self.facts.for_each_definition(instructions, leaders, &self.program.constants, written, native_calls, 0..=instructions.len().saturating_sub(1), |pc, register, ty| {
            if register == instructions[pc].result_register() { defined[pc] = Some(ty); }
        });
        let mut returned = Type::NEVER;
        for (pc, instruction) in instructions.iter().enumerate() {
            if !matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_))) { continue; }
            let ty = match crate::compiled::decoded(instruction) {
                Some(quench_runtime::staged::Operation::Return { source }) => self.facts.before(pc, source, instructions,
                    leaders, &self.program.constants, written, native_calls),
                _ if instruction.returns_from_frame() => defined[pc].unwrap_or(Type::ANY),
                _ => continue,
            };
            returned = returned.join(ty);
        }
        if returned == Type::NEVER { Type::ANY } else { returned }
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
                        Representation::Float64 => self.raw_unary(Unary::ToSignedSaturating(Lane::I32), value),
                        Representation::Int64 | Representation::Tagged => unreachable!("typed numeric return"),
                    };
                    self.raw_unary(Unary::ZeroExtend(Lane::I64), integer)
                }
                (NativeNumberResult::Float64, Representation::Int32) => {
                    let number = self.raw_unary(Unary::FromSigned(Lane::F64), value);
                    self.raw_unary(Unary::Bitcast(Lane::I64), number)
                }
                (NativeNumberResult::Float64, Representation::Float64) => self.raw_unary(Unary::Bitcast(Lane::I64), value),
                (_, Representation::Int64 | Representation::Tagged) => unreachable!("typed numeric return"),
            };
            let success = self.raw_const(Lane::I32, 0);
            self.leave(&[success, value]);
        }
        format
    }

    /// The receiver bits a run starts with: a body entry's word, which its driver passes (a lane
    /// body's caller, the OSR region's `entry_protocol` descriptor, the AOT trampoline); a scalar
    /// entry, which takes none, reads its window's header.
    fn entry_receiver_bits(&mut self) -> ir::Value {
        if self.completion == Completion::Body { return self.entry_words.0; }
        let window = self.loop_window();
        emit_frame_word(&mut self.builder, window, quench_runtime::staged::frame::CONTEXT_THIS)
    }

    /// The running activation's dense capture slots (address, count): a lane body's caller
    /// packs them in the entry's word (`entry_cell::capture_word`), a loop region reads its window's header.
    fn entry_capture_words(&mut self) -> (ir::Value, ir::Value) {
        if !self.body_entry() {
            let window = self.loop_window();
            let word = emit_frame_word(&mut self.builder, window, quench_runtime::staged::frame::CONTEXT_BODY);
            return self.capture_slots_of(word);
        }
        self.entry_capture_slots()
    }

    /// The window a loop region runs in: its payload's register file, whose header holds its receiver and captures word.
    fn loop_window(&mut self) -> ir::Value {
        emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, native_registers))
    }

    /// Whether this run is a lane body, whose callers pass its receiver and captures as the
    /// entry's words; a loop region reads the payload's.
    fn body_entry(&self) -> bool { matches!(self.coverage, Coverage::Prefix | Coverage::PrefixFrom(_)) }

    /// The facts every run proves once at entry.
    fn entry_facts(&mut self, instructions: &[WideInstruction], _loops: &[(usize, usize)], _leaders: &CompoundBitSet) -> Entry {
        let this_bits = match self.helpers {
            LaneHelpers::Numeric => unreachable!("Number signatures have no receiver"),
            LaneHelpers::Bound(..) => self.fixed_this(),
            LaneHelpers::Linked(..) => unreachable!("helper table bound before entry facts"),
            _ => self.entry_receiver_bits(),
        };
        let accesses_fields = self.accesses_fields(instructions);
        let receiver = if accesses_fields { emit_lane_receiver(&mut self.builder, self.payload, this_bits) } else { self.absent_receiver() };
        let this = self.builder.declare_var(types::I64);
        self.builder.def_var(this, this_bits);
        let receiver_shapes = self.entry_receiver_shapes(&receiver);
        let store_atoms = self.entry_store_atoms(instructions);
        // The running activation's dense capture slots (`EnvironmentSlots::native_dense_view`),
        // fixed for the run; each read loads its slot where it runs, after any call that may
        // have stored to it.
        let capture_values = self.builder.declare_var(types::I64);
        let capture_len = self.builder.declare_var(types::I64);
        let (values, len) = self.entry_capture_words();
        self.builder.def_var(capture_values, values);
        self.builder.def_var(capture_len, len);
        let environment = self.builder.declare_var(types::I64);
        let storage = self.environment_word();
        self.builder.def_var(environment, storage);
        let mut miss = None;
        let object_table = accesses_fields
            .then(|| {
                let base = emit_js_record_base(&mut self.builder, self.payload,
                    std::mem::offset_of!(JsStepDispatch, head.object_record_base));
                let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                CheckedObjectTable::prove(&mut self.builder, base, exit)
            });
        let mut calls = CompoundBitSet::with_capacity(self.unit.frames.len());
        let mut call_receivers = CompoundBitSet::with_capacity(self.unit.frames.len());
        if !self.completion.numeric() {
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
                if !self.admits_at_entry(pc) { continue; }
                if let Some(frame) = self.unit.markers.iter().position(|marker| *marker == pc) {
                    let call = self.unit.frames[frame];
                    let body = self.program.function(call.target.function as usize);
                    let numeric = restartable_numeric_function(self.program, body, body.instructions());
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
                        && !self.closed_proves(frame)
                        && let Some(root) = aliases[usize::from(call.callee_register)] {
                        let binding = (root, call.target);
                        if !admitted.contains(&binding) {
                            let callee = self.raw_load(Lane::I64, regions::frame(), self.registers_base, register_offset(root));
                            let matched = match call.target.callee {
                                CalleeBinding::Function => self.function_binding(callee, call.target.function),
                                CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } => self.is_identity(callee, identity),
                            };
                            let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                            emit_js_guard(&mut self.builder, matched, exit);
                            admitted.push(binding);
                        }
                        calls.insert(frame);
                    }
                }
                let root = (matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })))
                    .then(|| aliases[usize::from(instruction.register_b())]).flatten();
                for register in writes(instruction) { aliases[usize::from(register)] = root; }
            }
            for (root, recovery) in receivers {
                let receiver = self.raw_load(Lane::I64, regions::frame(), self.registers_base, register_offset(root));
                let matched = self.receiver_recovery(receiver, recovery);
                let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                emit_js_guard(&mut self.builder, matched, exit);
            }
        }
        let fields = {
            let mut fields = smallvec::SmallVec::new();
            if matches!(self.helpers, LaneHelpers::Bound(..)) {
                let mut uses = smallvec::SmallVec::<[(u16, bool, Type); INLINE_FIELD_BINDINGS]>::new();
                for (pc, instruction) in instructions.iter().enumerate() {
                    // An inlined call's later alternatives are entered by its switch, an edge
                    // the bytecode facts do not see: their sites bind too, or they would read
                    // through the VM's caches for the whole run.
                    if !self.facts.reached(pc) && !matches!(self.unit.origins[pc], super::lane_unit::Origin::Inlined { .. }) { continue; }
                    // A field chain names sequential sites, not alternatives for
                    // one polymorphic access. Bind each distinct column once.
                    let mut sites = smallvec::SmallVec::<[u16; 3]>::new();
                    match () {
                        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) => match instruction.field_lookup() {
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
                        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) => sites.push(instruction.cache_site_index()),
                        _ => {},
                    }
                    let repeated = _loops.iter().any(|&(header, back)| (header..=back).contains(&pc));
                    for site in sites {
                        let writes = if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) {
                            self.facts.before(pc, instruction.register_a(), instructions, _leaders,
                                &self.program.constants, &writes, self.native_calls).machine()
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
                    // A slot whose writes the run does not admit leaves only at its stores
                    // (`EmitFrame::bound_store`): the rest of the run, which reads it by shape, holds.
                    if writes != Type::NEVER { binding.admitted = Some(self.field_writes_admitted(&mut binding, writes)); }
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
        self.admit_store_domains(instructions, &mut miss);
        let records = self.entry_field_records(instructions, &receiver, object_table, &mut miss);
        if let Some(miss) = miss {
            let admitted = self.builder.create_block();
            self.goto(admitted, &[]);
            self.builder.switch_to_block(miss);
            self.builder.set_cold_block(miss);
            // No guest operation has run and canonical registers are untouched.
            // Decline the whole run once, at its start, even for a speculative unused binding.
            let origin = |pc: usize| match self.unit.origins[pc] {
                super::lane_unit::Origin::Caller(pc) => u64::from(pc),
                super::lane_unit::Origin::Inlined { .. } => unreachable!("a run starts in the caller"),
            };
            self.return_at(origin(self.coverage.entry()));
            self.builder.switch_to_block(admitted);
        }
        Entry { receiver, object_table, records, receiver_shapes, store_atoms, capture_values, capture_len, environment, this, fields, calls, call_receivers }
    }

    /// Per receiver shape a field site folds, whether the receiver has it.
    fn entry_receiver_shapes(&mut self, receiver: &LaneReceiver) -> BTreeMap<u32, Variable> {
        let mut receiver_shapes = BTreeMap::new();
        for field in self.fields.iter().flatten().flatten() {
            if receiver_shapes.contains_key(&field.shape) {
                continue;
            }
            let shape = self.builder.use_var(receiver.shape);
            let same = self.raw_compare_imm(Relation::Equal, shape, i64::from(field.shape));
            let variable = self.builder.declare_var(types::I8);
            self.builder.def_var(variable, same);
            receiver_shapes.insert(field.shape, variable);
        }
        receiver_shapes
    }

    /// Per atom the unit stores, whether the run admits direct stores of it. A direct store of an
    /// atom needs a specialized run and no method cache naming it; a run that makes no call
    /// changes neither (`emit_js_own_field_set`, `Lowering::site_store`).
    fn entry_store_atoms(&mut self, instructions: &[WideInstruction]) -> BTreeMap<Atom, Variable> {
        let mut store_atoms = BTreeMap::new();
        let stored = instructions.iter().filter_map(|instruction| match () {
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) => Some(instruction.atom_index()),
            _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) && instruction.writes_current_this() => match instruction.field_lookup() {
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
        store_atoms
    }

    /// Whether the entry edge admits the inlined call at `pc`, a call a run may skip
    /// (`packet != null`) and so not one every run may be refused for: a body that left at
    /// its entry once admits them at their markers instead (`Lowering::inline_call`).
    fn admits_at_entry(&self, pc: usize) -> bool {
        self.facts.reached(pc) && !self.generic.contains(&entry_leave(self.unit))
    }

    /// `invariant_field_records`, unless the body left at its entry: it binds no receiver record
    /// there, the reads it hoisted ran under a branch (`packet != null`) and the run declined at
    /// its start.
    fn entry_field_records(&mut self, instructions: &[WideInstruction], receiver: &LaneReceiver,
        object_table: Option<CheckedObjectTable>, miss: &mut Option<ir::Block>) -> Vec<Option<ObjectRecord>> {
        if self.generic.contains(&entry_leave(self.unit)) { return vec![None; usize::from(self.unit.registers) + 1]; }
        self.invariant_field_records(instructions, receiver, object_table, miss)
    }

    /// The register (or `unit.registers` for `this`) whose object the field access at `pc` reads or
    /// writes, where `instruction` is one.
    fn field_owner(&self, pc: usize, instruction: WideInstruction) -> Option<usize> {
        let count = usize::from(self.unit.registers);
        match () {
            // A native method's read folds to its holder's function whatever its receiver (`fold_native_member`):
            // a string primitive's `charCodeAt` reads no object's field.
            _ if matches!(self.global_reads.get(&pc), Some(quench_runtime::vm::global_facts::GlobalRead::NativeMember { .. })) => None,
            _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) && !super::reads_length(self.program, instruction) => {
                let base = self.field_base(pc, instruction);
                base.and_then(|base| if base == quench_runtime::bytecode::FieldBase::THIS { Some(count) }
                    else { base.register_index().map(usize::from) }).filter(|_| self.reads_own_field(instruction))
            }
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) => Some(match (instruction.field_store_register(), self.unit.origins[pc]) {
                (Some(register), _) => usize::from(register),
                (None, super::lane_unit::Origin::Inlined { frame, .. }) => usize::from(self.unit.frames[frame].receiver),
                (None, _) => count,
            }),
            _ => None,
        }
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
            let owner = self.field_owner(pc, *instruction);
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
                let present = self.raw_compare_imm(Relation::NotEqual, record, 0);
                emit_js_guard(&mut self.builder, present, exit);
                ObjectRecord::Address(record)
            } else {
                let records = match object_table {
                    Some(table) => ObjectTable::Checked(table),
                    None => ObjectTable::Unchecked(*records.get_or_insert_with(|| emit_js_record_base(&mut self.builder,
                        self.payload, std::mem::offset_of!(JsStepDispatch, head.object_record_base)))),
                };
                // An unwritten register holds its entry value at either start, in the file.
                let base = self.registers_base;
                let bits = self.raw_load(Lane::I64, regions::frame(), base, register_offset(owner as Register));
                ObjectRecord::prove(&mut self.builder, bits, records, None, exit)
            });
        }
        admitted
    }

    /// Admit once at the edge; later raw stores consume the compile-time domain proof.
    fn admit_field_writes(&mut self, binding: &mut FieldSlotBinding, writes: Type, exit: ir::Block) {
        if binding.writes.is_some_and(|domain| writes.is_subtype(domain)) {
            if let Some(admitted) = binding.admitted { emit_js_guard(&mut self.builder, admitted, exit); }
            return;
        }
        let admitted = self.field_writes_admitted(binding, writes);
        emit_js_guard(&mut self.builder, admitted, exit);
    }

    /// Whether the slot `binding` names admits `writes` (`LaneHelper::FieldWriteAdmission`),
    /// the domain then recorded on the binding.
    fn field_writes_admitted(&mut self, binding: &mut FieldSlotBinding, writes: Type) -> ir::Value {
        let shape = self.raw_unary(Unary::ZeroExtend(Lane::I64), binding.receiver);
        let slot = binding.slot.read(&mut self.builder);
        let (admitted, domain) = self.slot_writes_admitted(shape, slot, writes);
        binding.writes = Some(binding.writes.map_or(domain, |held| held.join(domain)));
        admitted
    }

    /// Admits at entry the writes `writes` to `slot` of `shape`, failing through `exit`; a slot whose
    /// words are raw doubles where `raw_double`, which the unit's in-place arms store as such
    /// (`Heap::admit_slot_writes_as`).
    pub(crate) fn admit_slot_writes_as(&mut self, (shape, slot): (ir::Value, ir::Value), writes: Type, raw_double: bool, exit: ir::Block) {
        let (admitted, _) = self.slot_writes_admitted_as(shape, slot, writes, raw_double);
        emit_js_guard(&mut self.builder, admitted, exit);
    }

    fn slot_writes_admitted(&mut self, shape: ir::Value, slot: ir::Value, writes: Type) -> (ir::Value, Type) {
        self.slot_writes_admitted_as(shape, slot, writes, false)
    }

    fn slot_writes_admitted_as(&mut self, shape: ir::Value, slot: ir::Value, writes: Type, raw_double: bool) -> (ir::Value, Type) {
        let domain = quench_runtime::typed_ir::SlotType::from_type(writes);
        let encoded = self.raw_const(Lane::I64, domain as i64 | if raw_double { RAW_DOUBLE_WRITES as i64 } else { 0 });
        let helper = self.helpers.target(&mut self.builder, LaneHelper::FieldWriteAdmission);
        let admitted = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[shape, slot, encoded]);
        (self.raw_compare_imm(Relation::NotEqual, admitted, 0), domain.ty())
    }

    fn emit_store_atom_admitted(&mut self, atom: Atom) -> ir::Value {
        if let Some(held) = self.loop_held_atom(atom) { return held; }
        let specialized = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.stores_specialized));
        let specialized = self.raw_compare_imm(Relation::NotEqual, specialized, 0);
        if let Some(held) = self.store_held_unnamed(atom, specialized) { return held; }
        let free = self.atom_unnamed(atom);
        self.raw_binary(Binary::And, specialized, free)
    }

    /// Conform the entry's state to the facts at `pc`. A lane body's failed input proof
    /// leaves through one shared block with `LaneExit::Entry`, its specialization's feedback
    /// that the inputs need no contract; other regions leave at `pc`.
    fn entry_conform(&mut self, state: &mut State, pc: usize, coverage: Coverage) {
        self.proving_entry = matches!(coverage, Coverage::Prefix) && self.completion.status_and_pc();
        self.conform(state, pc);
        self.proving_entry = false;
        self.guard_entering_ranges(state, pc);
    }

    /// The guards' leaves to fill, once the entry's shared leave (`entry_conform`) is filled:
    /// it leaves with the plan's entry leave, with no register written yet to write back.
    fn take_leaves(&mut self) -> Vec<GuardExit> {
        if let Some(block) = self.entry_exit.take() {
            self.switch_to(block, true);
            self.return_entry();
        }
        std::mem::take(&mut self.pending)
    }

    /// The leave of a failed entry proof, before any instruction ran: a lane plan names it
    /// (`LaneExit::Entry`), so the activation's specialization drops the contract that
    /// failed; a Number entry misses, restarting the original call.
    fn return_entry(&mut self) {
        match self.completion {
            Completion::Body => self.return_at(super::LaneExit::Entry.word()),
            Completion::Number | Completion::Scalar(_) => self.return_at(0),
        }
    }

    /// Lower the instruction at `pc`; whether control falls through to the next.
    fn instruction(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        if self.consumed.contains(pc) { return true; }
        // A construction readiness a loop's entry tested holds only across quiet instructions
        // (`virtual_objects::hoist_constructions`): any other forgets it here, before its code.
        if !self.construct_ready.is_empty() && !self.keeps_constructions_ready(state, pc, instruction) {
            self.unready_constructions();
        }
        self.left_here = self.generic.contains(&pc);
        self.advance_inductions(pc);
        if self.unit.markers.contains(&pc) || !retains_array_views(&self.program.constants, instruction) {
            state.proofs.arrays.clear();
        }
        if let Some(read) = self.hoisted.get(&pc).copied() {
            self.hoisted_read(state, pc, instruction, read);
            return true;
        }
        // Kept receivers (`virtual_objects`): a dead one is dropped, a read of a field it keeps
        // is its value, and an instruction that needs it as an object allocates it first.
        if !state.virtuals.is_empty() {
            state.virtuals.retain(|kept| self.is_live(pc, kept.register));
            if crate::compiled::meaning_compiled::is_get_field(instruction) && let Some(falls) = self.virtual_field(state, pc, instruction) {
                self.native += 1;
                return falls;
            }
            if !state.virtuals.is_empty() {
                if self.keeps_virtuals(state, pc, instruction) { self.drop_overwritten_virtuals(state, instruction); } else { self.materialize_virtuals(state, pc); }
            }
        }
        if let Some(plan) = self.virtual_construct_plan(pc, instruction) && self.virtual_construct(state, pc, instruction, plan) {
            self.native += 1;
            return true;
        }
        let pc32 = pc as u32;
        let plain = instruction.result_flags() == 0;
        let ran = match () {
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Throw) => self.throw(state, pc, instruction),
            // A call, a construction in the caller's window and a return: the staged operations
            // the tier's facts admit (`call_rows`).
            _ if self.scalar_call_admitted(state, pc, instruction) => Some(self.scalar_call(state, pc, instruction)),
            _ if plain && self.is_scalar_callee_load(pc, instruction) => Some(self.scalar_callee_load(state, pc, instruction)),
            _ if self.is_call_row(instruction) => self.call_row_step(state, pc, instruction),
            _ if self.is_call_out(instruction) => Some(self.call_out_step(state, pc, instruction)),
            _ if self.is_register_row(instruction) => Some(self.register_step(state, pc, instruction)),
            _ if self.is_window_row(pc, instruction) => Some(self.window_step(state, pc, instruction)),
            _ if matches!(quench_runtime::staged::Operation::<(), ()>::decode_load_name(instruction, ()), Some(quench_runtime::staged::Operation::LoadName { .. }))
                && plain && self.global_reads.contains_key(&pc) => Some(self.global_read(state, pc, instruction)),
            _ if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Nop)) && plain => match self.unit.markers.iter().position(|marker| *marker == pc) {
                Some(frame) => Some(self.inline_call(state, pc, frame)),
                None => Some(true),
            },
            _ if plain && quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) => {
                let Some(constant) = self.program.constants.get(instruction.constant_index()) else { return false };
                self.load_constant(state, pc, instruction, constant)
            }
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Return { .. })) && plain && self.completion.numeric() => {
                self.return_number(state, instruction.register_a(), pc32);
                Some(false)
            }
            // A capture's read or store, by its staged row.
            // A read of a written-once root binding whose function is held (`root_constant_read`).
            _ if plain && self.root_constant_at(pc).is_some() => self.root_constant_read(state, pc, instruction),
            _ if self.is_capture_row(pc, instruction) => Some(self.capture_step(state, pc, instruction)),
            _ if crate::compiled::meaning_compiled::is_get_field(instruction) && instruction.result_flags() & !(quench_runtime::bytecode::SET_THIS_REGISTER | quench_runtime::bytecode::RETURN_REGISTER) == 0 => {
                let ran = self.fold_native_member(state, pc, instruction)
                    .or_else(|| self.fold_own_constant(state, pc, instruction)).or_else(|| self.get_field(state, pc, instruction));
                // A named read of a declared parameter's member enters its destination with the member's type.
                if ran == Some(true) && !instruction.writes_current_this() && !instruction.returns_from_frame() {
                    if let (Some(base), Some((parts, None))) = (self.field_base(pc, instruction).and_then(|base| base.register_index()), self.field_chain(instruction)) {
                        if let [(atom, _)] = parts.as_slice() {
                            self.conform_declared_member_read(state, pc, base, *atom, instruction.result_register());
                        }
                    }
                }
                ran
            }
            _ if self.is_element_row(instruction) => Some(self.element_step(state, pc, instruction)),
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::SetFunctionName) && plain && self.completion.status_and_pc() => self.function_name(state, pc, instruction),
            _ if matches!((plain, instruction.op().field_store()), (true, Some(_))) => self.set_field(state, pc, instruction),
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) && instruction.numeric_local_target().is_none() => {
                self.operator(state, pc, instruction).or_else(|| self.binary_call_out(state, pc, instruction))
            }
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Unary) && plain && instruction.unary_operator() == quench_runtime::bytecode::UNARY_TYPEOF_INDEX
                && instruction.register_b() < self.unit.registers => self.typeof_index(state, pc, instruction),
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Unary) && quench_runtime::bytecode::typeof_test(instruction.unary_operator()).is_some()
                && !self.helpers.is_numeric() => self.typeof_test(state, pc, instruction),
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericUnary)
                && instruction.register_b() < self.unit.registers => self.unary(state, pc, instruction),
            _ if self.allocation_row(pc, instruction) => Some(self.allocation(state, pc, instruction)),
            _ if self.is_construction(instruction) => self.construct(state, pc, instruction),
            // The entry function's own `arguments` binding, whose values the activation kept
            // or an unmaterialized binding holds; an inlined callee's reads its own, which no
            // window of this activation holds.
            _ if plain && matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::ArgumentsLength { .. })) && self.own_arguments_read(pc, instruction) =>
                self.arguments_length(state, pc32, instruction),
            _ if plain && quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::ArgumentGet) && self.own_arguments_read(pc, instruction) =>
                self.arguments_get(state, pc32, instruction),
            _ if plain && quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Jump) => return self.jump(state, pc, instruction),
            _ if plain && instruction.op().control_flow_layout() == ControlFlowLayout::BranchTable => return self.branch_table(state, pc, instruction),
            _ if plain && matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::IntCase { .. }))) => self.int_case_index(state, pc, instruction),
            _ if plain && matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::JumpIf { .. })) => {
                let target = instruction.jump_target() as usize;
                let exit = self.guard_exit(state, pc32);
                let taken_block = self.builder.create_block();
                let fall_block = self.builder.create_block();
                self.step(state, frame::Edges::Branch { target: taken_block, following: fall_block, miss: exit },
                    quench_runtime::staged::Operation::JumpIf { register: instruction.register_a(), taken: matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::JumpIf { taken: true, .. })) });
                self.continue_branch(state, pc, target, taken_block, fall_block);
                return false;
            }
            _ if plain && self.is_compare_jump(instruction) => {
                if self.compare_jump(state, pc, instruction) { return false; }
                None
            }
            _ => None,
        };
        match ran {
            Some(falls) => {
                self.native += 1;
                if falls { self.conform_declared_definitions(state, pc, instruction); }
                falls
            }
            None => {
                super::dispatch_fast::lane_body_stats::unlowered(instruction.op());
                if self.helpers == LaneHelpers::Ahead && quench_runtime::trace::Channel::AotLanes.on() {
                    let name = instruction.op().loads_name().then(|| self.program.atoms.get(instruction.atom_index() as usize).map(|atom| format!("({atom})"))).flatten();
                    UNLOWERED.with(|unlowered| unlowered.borrow_mut().insert(format!("{:?}{}", instruction.op(), name.unwrap_or_default())));
                }
                if self.coverage.full() && (self.completion.numeric()
                    || !((matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Return { .. })) && plain) || instruction.returns_from_frame())) {
                    self.valid = false;
                    declined("an unlowered instruction in full coverage", pc);
                }
                self.exit(state, pc32);
                false
            }
        }
    }

    /// The end of a body whose helper neither completed nor missed: a termination stops with its
    /// status, and any other `status` is a leave in a callee.
    fn stop_by_status(&mut self, status: ir::Value) {
        let terminated = self.raw_compare_imm(Relation::Equal, status, LANE_CALL_TERMINATED as i64);
        let stopped = self.builder.create_block();
        let callee = self.builder.create_block();
        self.branch_if(terminated, (stopped, &[]), (callee, &[]));
        self.builder.switch_to_block(stopped);
        let terminate = self.raw_const(Lane::I32, TERMINATED_STATUS);
        self.leave_with(terminate, None);
        self.builder.switch_to_block(callee);
        let word = self.raw_const(Lane::I64, LaneExit::Callee.word() as i64);
        let stop = self.stop;
        self.goto(stop, &[word]);
    }

    /// A helper's result, read from the lane's register `destination` as the register's value.
    fn reload_result(&mut self, state: &mut State, destination: Register) {
        let base = self.registers_base;
        let bits = self.raw_load(Lane::I64, regions::frame(), base, register_offset(destination));
        self.set_tagged(state, destination, bits);
        state.proofs.homed.insert(usize::from(destination));
    }

    /// The registers the body wrote that are live at `pc`: what a helper reads from the lane's file.
    fn spilled(&self, pc: usize) -> Vec<Register> {
        self.written.iter().map(|register| register as Register)
            .take_while(|register| *register < self.unit.caller_registers)
            .filter(|register| self.is_live(pc, *register)).collect()
    }

    /// The live registers the body wrote since their last write-back, stored to the lane's registers for a
    /// helper to read. A register whose slot is current (`LocalProofs::homed`) is neither boxed nor stored.
    fn home_written(&mut self, state: &mut State, pc: usize) {
        for register in self.spilled(pc) {
            let index = usize::from(register);
            if state.proofs.homed.contains(index) { continue; }
            let bits = self.tagged(state, register);
            let base = self.registers_base;
            self.store_word(regions::frame(), bits, base, register_offset(register));
            state.proofs.homed.insert(index);
        }
    }

    /// A relation's predicate: `instanceof` by the prototype chain (`instance_of`),
    /// (in)equality of operands not both Numbers (`equality`), every other relation of Numbers.
    fn relation(&mut self, state: &mut State, pc: u32, operator: u32,
        left: Operand, right: Operand) -> Option<ir::Value> {
        let numbers = [left, right].iter().all(|operand| super::facts::operand_type(
            &self.program.constants, *operand, |register| state.values[usize::from(register)].value.ty)
            .is_subtype(Type::NUMBER));
        // A frameless Number region has no VM helper capability (`LaneHelpers::Numeric`).
        let helpers = !self.helpers.is_numeric();
        if operator == oxc_ast::ast::BinaryOperator::Instanceof as u32 {
            return helpers.then(|| self.instance_of(state, pc, left, right)).flatten();
        }
        let typed = |operand: Operand| super::facts::operand_type(&self.program.constants, operand,
            |register| state.values[usize::from(register)].value.ty).is_subtype(Type::NUMBER);
        match super::operations::equality_operator(operator) {
            Some((true, negated)) if !numbers && (typed(left) || typed(right)) && self.operand_supported(left) && self.operand_supported(right) => {
                let (number, other) = if typed(left) { (left, right) } else { (right, left) };
                let equal = self.number_strict_equality(state, pc, number, other)?;
                Some(if negated { self.raw_binary_imm(Binary::Xor, equal, 1) } else { equal })
            }
            Some((strict, negated)) if !numbers && helpers && self.operand_supported(left) && self.operand_supported(right) => {
                let equal = self.equality(state, pc, left, right, strict)?;
                Some(if negated { self.raw_binary_imm(Binary::Xor, equal, 1) } else { equal })
            }
            // A relation of operands not both Numbers (strings, `in`): the interpreter's own
            // rule (`lane_decision`) through the pure `Decide` helper; where only the full step
            // decides (a coercion that may run guest code), the region leaves at the test.
            None if !numbers && helpers && self.operand_supported(left) && self.operand_supported(right) =>
                self.decided_relation(state, pc, operator, left, right),
            _ => self.number_relation(state, pc, operator, left, right),
        }
    }

    /// A helper's call, by its address in the running image's table (`LaneHelpers::address`).
    fn helper_call(&mut self, helper: LaneHelper, arguments: &[ir::Value]) -> ir::Value {
        let address = self.helpers.target(&mut self.builder, helper);
        emit_lane_helper_call(&mut self.builder, self.payload, address, arguments)
    }

    /// The property arena's base again, which an allocation may have moved.
    fn reload_property_arena(&mut self) {
        let properties = self.entry.as_ref().expect("entry facts").receiver.properties;
        let (values, _) = emit_js_property_values(&mut self.builder, self.payload);
        self.builder.def_var(properties, values);
    }

    /// After a helper that allocated, which may move the property arena: the projections
    /// into it retire and its base reloads.
    fn allocated(&mut self, state: &mut State) {
        state.proofs.layouts.clear();
        self.reload_property_arena();
    }

    /// After a helper that may also have changed any object's shape or storage (a
    /// construction, an adding store): every proof of an object retires, as another register
    /// may name it.
    fn reshaped(&mut self, state: &mut State) {
        state.call_effect();
        self.reload_property_arena();
    }

    /// A fused `typeof value === name` (`bytecode::typeof_test`): an immediate value's tag
    /// decides it, as `Vm::typeof_test_value` would, and a heap value's cell the VM reads
    /// (`typeof_is_in_lane`). A returning test completes the activation with its Boolean.
    fn typeof_test(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let (index, negated) = quench_runtime::bytecode::typeof_test(instruction.unary_operator())?;
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) if instruction.result_flags() == 0 => None,
            (true, Some(super::lane_unit::Origin::Caller(record))) if self.completion.status_and_pc() => Some(record),
            _ => return None,
        };
        let value = self.tagged(state, instruction.register_b());
        // The immediate classes whose name is the tested one, by the VM's own rule.
        let named = |sample: Value| quench_runtime::vm::coercion::immediate_typeof_index(sample) == Some(index);
        let mut immediate = self.raw_const(Lane::I8, 0);
        if named(Value::number(0.0)) {
            let double = quench_runtime::staged::value::is_double(&mut emit(&mut self.builder), value);
            let int = is_int_tagged(&mut self.builder, value);
            let number = self.raw_binary(Binary::Or, double, int);
            immediate = self.raw_binary(Binary::Or, immediate, number);
        }
        for sample in [Value::UNDEFINED, Value::DELETED, Value::NULL] {
            if named(sample) {
                let same = self.raw_compare_imm(Relation::Equal, value, sample.to_bits() as i64);
                immediate = self.raw_binary(Binary::Or, immediate, same);
            }
        }
        if named(Value::TRUE) {
            let boolean = quench_runtime::staged::value::is_boolean(&mut emit(&mut self.builder), value);
            immediate = self.raw_binary(Binary::Or, immediate, boolean);
        }
        let heap = quench_runtime::staged::value::is_heap(&mut emit(&mut self.builder), value);
        let (cell, join) = (self.builder.create_block(), self.builder.create_block());
        let holds = self.builder.append_block_param(join, types::I8);
        self.branch_if(heap, (cell, &[]), (join, &[immediate]));
        self.builder.switch_to_block(cell);
        let tested = self.raw_const(Lane::I64, index as i64);
        let is = self.helper_call(LaneHelper::TypeofIs, &[value, tested]);
        let is = self.raw_unary(Unary::Narrow(Lane::I8), is);
        self.goto(join, &[is]);
        self.builder.switch_to_block(join);
        let holds = if negated { self.raw_binary_imm(Binary::Xor, holds, 1) } else { holds };
        let boolean = self.boolean(holds);
        if let Some(record) = completes {
            self.return_value(record, boolean);
            return Some(false);
        }
        self.set_boolean(state, instruction.result_register(), boolean);
        Some(true)
    }

    /// `operation` by the interpreter's semantics (`staged::step`) on the emit stage, with
    /// `edges` as its control edges. The lane specializer decides every truthiness itself, so
    /// no step is left unresolved.
    fn step(&mut self, state: &mut State, edges: frame::Edges, operation: quench_runtime::staged::Operation<ir::Value, EmitProof>)
        -> quench_runtime::staged::Step<ir::Value, ()> {
        self.step_with(state, edges, operation, StepWith::default())
    }

    /// `step` of a named field access: the frame holds the `plans` the sites proved and what
    /// the access already has (`HeldRead`), and the stage addresses the property arena.
    fn field_step(&mut self, state: &mut State, edges: frame::Edges, operation: quench_runtime::staged::Operation<ir::Value, EmitProof>,
        access: (&LanePlans<'_>, HeldRead)) -> quench_runtime::staged::Step<ir::Value, ()> {
        self.step_with(state, edges, operation, StepWith { access: Some(access), ..StepWith::default() })
    }

    /// `step` with what the operation's frame needs beyond its edges (`StepWith`).
    fn step_with(&mut self, state: &mut State, edges: frame::Edges, operation: quench_runtime::staged::Operation<ir::Value, EmitProof>,
        StepWith { access, sites, site }: StepWith<'_>) -> quench_runtime::staged::Step<ir::Value, ()> {
        let locals = !self.completion.numeric();
        let makes_environment = operation.makes_environment();
        let property_values = access.and_then(|_| self.entry.as_ref()).map(|entry| self.builder.use_var(entry.receiver.properties));
        let gate = access.map(|_| self.payload);
        let mut stage = Emit { builder: &mut self.builder, property_values, gate, payload: Some(self.payload) };
        let fields = access.map(|(plans, held)| FieldFacts { plans, entry: self.entry.as_ref(), native_calls: self.native_calls,
            held,
            stable: StableHold { holders: &mut self.stable.holders, shapes: &mut self.stable.shapes, loops: &self.stable.stable_loops, online: self.vm.online, at: self.at } });
        let mut frame = EmitFrame { registers: &mut self.registers, written: &mut self.written,
            state, edges, constants: &self.program.constants, this: self.entry.as_ref().map(|entry| entry.this),
            payload: self.payload, helpers: self.helpers, locals, polls: &mut self.polls,
            roots: &mut self.stable.roots, number_returns: &mut self.number_returns, fields,
            call_out: sites.call_out, capture: sites.capture, html_dda: self.vm.html_dda, tiers: self.vm.tiers,
            prototype: sites.prototype, table_entry: self.entry.as_ref(),
            held: self.held_view.take(), hole_leaves: std::mem::take(&mut self.hole_leaves), arrays_fixed: self.arrays_fixed, site, proven_arguments: Default::default() };
        super::record_boxed_numbers();
        let step = quench_runtime::staged::step(&mut stage, &mut frame, operation);
        // A Number the step boxed (a raw-double slot's read) reads back as the word it boxed.
        for (bits, number) in super::take_boxed_numbers() {
            let bits = self.builder.func.dfg.resolve_aliases(bits);
            state.proofs.note_number(bits, number);
        }
        debug_assert!(!matches!(step, quench_runtime::staged::Step::Unresolved), "the lane specializer decides every truthiness");
        // A step that ends in a branch makes no environment: only a closure's allocation, a value step, does.
        if makes_environment && matches!(edges, frame::Edges::Value | frame::Edges::ValueGuarded { .. }) { self.reload_environment(); }
        step
    }

    /// Whether a literal's allocation lowers here: plain, or the one that returns its value
    /// from a caller record, which completes the activation (`literal_done`).
    fn literal_completes(&self, instruction: WideInstruction, pc: usize) -> bool {
        self.completion.status_and_pc() && (instruction.result_flags() == 0
            || (instruction.returns_from_frame() && matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_)))))
    }

    /// A literal's allocation ran: a returning one completes its activation with the result.
    fn literal_done(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(super::lane_unit::Origin::Caller(record)) = self.unit.origins.get(pc).copied().filter(|_| instruction.returns_from_frame()) else { return true };
        let value = self.tagged(state, instruction.result_register());
        self.return_value(record, value);
        false
    }

    fn typeof_index(&mut self, state: &mut State, _pc: usize, instruction: WideInstruction) -> Option<bool> {
        let source = instruction.register_b();
        let ty = state.values[usize::from(source)].value.ty;
        let known = [quench_runtime::ValueType::Number, quench_runtime::ValueType::Boolean, quench_runtime::ValueType::Null,
            quench_runtime::ValueType::Undefined, quench_runtime::ValueType::String, quench_runtime::ValueType::BigInt, quench_runtime::ValueType::Symbol]
            .into_iter().find(|kind| ty != Type::NEVER && ty.is_subtype(Type::language(*kind)));
        let Some(kind) = known else {
            // A value of no one known kind: the VM names it, a heap value by its cell (`typeof_index_in_lane`).
            // The staged row's heap arm needs a frame that proves a cell's kind, which this one does not.
            let bits = self.tagged(state, source);
            let index = self.helper_call(LaneHelper::TypeofIndex, &[bits]);
            let int = self.raw_unary(Unary::Narrow(Lane::I32), index);
            self.define(state, instruction.result_register(), int, RegisterFact::definition(Type::INT32, Representation::Int32));
            return Some(true);
        };
        let index = quench_runtime::bytecode::typeof_name_index(quench_runtime::vm::coercion::typeof_text(kind)).expect("a language typeof name");
        let int = self.raw_const(Lane::I32, i64::try_from(index).expect("index fits i64"));
        self.define(state, instruction.result_register(), int, RegisterFact::definition(Type::INT32, Representation::Int32));
        Some(true)
    }

    /// `value instanceof constructor` by the VM's steady case (`Vm::steady_instanceof`), which
    /// calls nothing; a case only the full algorithm decides leaves at the test.
    fn instance_of(&mut self, state: &mut State, pc: u32, value: Operand, constructor: Operand) -> Option<ir::Value> {
        let exit = self.guard_exit(state, pc);
        let (value, constructor) = (self.operand_tagged(state, value)?, self.operand_tagged(state, constructor)?);
        let holds = self.helper_call(LaneHelper::InstanceOf, &[value, constructor]);
        let decided = self.raw_compare_imm(Relation::NotEqual, holds, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, decided, exit);
        Some(self.raw_unary(Unary::Narrow(Lane::I8), holds))
    }

    /// A relation the `Decide` helper answers by the interpreter's rule (`lane_decision`): its
    /// boolean, or a leave at the test where only the full step decides.
    fn decided_relation(&mut self, state: &mut State, pc: u32, operator: u32, left: Operand, right: Operand) -> Option<ir::Value> {
        let exit = self.guard_exit(state, pc);
        let (left, right) = (self.operand_tagged(state, left)?, self.operand_tagged(state, right)?);
        // A relational operator of operands no fact types (a field's value, a call's result): two
        // int-tagged words compare as int32 in place, as the interpreter's integer path does
        // (`operations::integer_binary`); any other pair is the helper's.
        let Some(relation) = super::operations::NumberRelation::from_operator(operator) else {
            return Some(self.decided_words(left, right, operator, exit));
        };
        let (left_int, right_int) = (is_int_tagged(&mut self.builder, left), is_int_tagged(&mut self.builder, right));
        let ints = self.raw_binary(Binary::And, left_int, right_int);
        let (int_block, other, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        let holds = self.builder.append_block_param(join, types::I8);
        self.branch_if(ints, (int_block, &[]), (other, &[]));
        self.builder.switch_to_block(int_block);
        let (left_bits, right_bits) = (self.raw_unary(Unary::Narrow(Lane::I32), left), self.raw_unary(Unary::Narrow(Lane::I32), right));
        let int_holds = quench_runtime::staged::number_machine::number_relation(&mut super::object_slots::emit(&mut self.builder), relation, left_bits, right_bits, NumberRelationDomain::Int32);
        self.goto(join, &[int_holds]);
        self.builder.switch_to_block(other);
        let decided = self.decided_words(left, right, operator, exit);
        self.goto(join, &[decided]);
        self.builder.switch_to_block(join);
        Some(holds)
    }

    /// The `Decide` helper's Boolean of `left operator right`, leaving at `exit` where only the
    /// full algorithm decides it (`comparison_decision`).
    fn decided_words(&mut self, left: ir::Value, right: ir::Value, operator: u32, exit: ir::Block) -> ir::Value {
        let operator = self.raw_const(Lane::I64, i64::from(operator));
        let holds = self.helper_call(LaneHelper::Decide, &[left, right, operator]);
        let decided = self.raw_compare_imm(Relation::NotEqual, holds, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, decided, exit);
        self.raw_unary(Unary::Narrow(Lane::I8), holds)
    }

    /// `number === other` where `number` is a Number: equal only where `other` is a Number too
    /// and the two compare equal as Numbers; nothing leaves.
    fn number_strict_equality(&mut self, state: &mut State, pc: u32, number: Operand, other: Operand) -> Option<ir::Value> {
        let exit = self.guard_exit(state, pc);
        let number = self.operand_number(state, number, exit)?;
        let bits = self.operand_tagged(state, other)?;
        let [decode, join] = std::array::from_fn(|_| self.builder.create_block());
        let equal = self.builder.append_block_param(join, types::I8);
        let double = quench_runtime::staged::value::is_double(&mut emit(&mut self.builder), bits);
        let int = is_int_tagged(&mut self.builder, bits);
        let is_number = self.raw_binary(Binary::Or, double, int);
        let unequal = self.raw_const(Lane::I8, 0);
        self.branch_if(is_number, (decode, &[]), (join, &[unequal]));
        self.builder.switch_to_block(decode);
        let value = emit_known_number(&mut self.builder, bits);
        let same = self.raw_compare(Relation::FloatEqual, number, value);
        self.goto(join, &[same]);
        self.builder.switch_to_block(join);
        Some(equal)
    }

    /// `===` or `==` of two operands: a strict pair of strings the loop holds compares their held
    /// facts (`held_strings_equal`), and any other pair is decided by `equality_words`.
    fn equality(&mut self, state: &mut State, pc: u32, left: Operand, right: Operand, strict: bool) -> Option<ir::Value> {
        let Some(pair) = self.held_pair(pc as usize, left, right, strict) else {
            return self.equality_words(state, pc, left, right, strict);
        };
        let bits = [self.operand_tagged(state, left)?, self.operand_tagged(state, right)?];
        Some(self.held_strings_equal(pair, bits))
    }

    /// `Vm::strict_equal` (`strict`) or `==` of two tagged operands, by the staged equality row's
    /// word decision (`staged::equality_decided`). A pair its words leave: loosely, a leave at `pc`;
    /// strictly, two heap values compare by contents where they are strings or legacy cells
    /// (BigInts) of one family (`strict_equal_cells_in_lane`), and are unequal otherwise.
    fn equality_words(&mut self, state: &mut State, pc: u32, left: Operand, right: Operand, strict: bool) -> Option<ir::Value> {
        pub use quench_runtime::value::ValueLayout;
        let exit = self.guard_exit(state, pc);
        let (left, right) = (self.operand_tagged(state, left)?, self.operand_tagged(state, right)?);
        let (decided, equal) = quench_runtime::staged::equality_decided(&mut emit(&mut self.builder), [left, right], strict);
        if !strict {
            emit_js_guard(&mut self.builder, decided, exit);
            return Some(equal);
        }
        let [open, cells, join] = std::array::from_fn(|_| self.builder.create_block());
        let joined = self.builder.append_block_param(join, types::I8);
        self.builder.set_cold_block(open);
        self.branch_if(decided, (join, &[equal]), (open, &[]));
        self.builder.switch_to_block(open);
        let family = self.raw_binary_imm(Binary::And, left, ValueLayout::HEAP_FAMILY_MASK as i64);
        let right_family = self.raw_binary_imm(Binary::And, right, ValueLayout::HEAP_FAMILY_MASK as i64);
        let one_family = self.raw_compare(Relation::Equal, family, right_family);
        let string = quench_runtime::staged::value::is_direct_string(&mut emit(&mut self.builder), left);
        let legacy = self.raw_compare_imm(Relation::Equal, family, ValueLayout::HEAP_TAG as i64);
        let contents = self.raw_binary(Binary::Or, string, legacy);
        let compared = self.raw_binary(Binary::And, one_family, contents);
        let unequal = self.raw_const(Lane::I8, 0);
        self.branch_if(compared, (cells, &[]), (join, &[unequal]));
        self.builder.switch_to_block(cells);
        // Two strings both held inline (`FlatUnits::Short`, zero-padded) are equal exactly
        // when their records' words are: decided here, the helper left for other forms.
        let [records, short, compare] = std::array::from_fn(|_| self.builder.create_block());
        self.branch_if(string, (records, &[]), (compare, &[]));
        self.builder.switch_to_block(records);
        let base = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.string_record_base));
        let mut decided = self.raw_compare_imm(Relation::NotEqual, base, 0);
        let mut addresses = [base; 2];
        for (address, bits) in addresses.iter_mut().zip([left, right]) {
            let index = self.raw_binary_imm(Binary::And, bits, i64::from(u32::MAX));
            let small = self.raw_binary_imm(Binary::And, index, i64::from(quench_runtime::tier_surface::SMALL_STRING_ID_MASK));
            let recorded = self.raw_compare_imm(Relation::NotEqual, small, i64::from(quench_runtime::tier_surface::SMALL_STRING_ID_BASE));
            decided = self.raw_binary(Binary::And, decided, recorded);
            let offset = self.raw_binary_imm(Binary::Mul, index, quench_runtime::staged::string_form::RECORD_SIZE as i64);
            *address = self.raw_binary(Binary::Add, base, offset);
        }
        let shorts = self.builder.create_block();
        self.branch_if(decided, (shorts, &[]), (compare, &[]));
        self.builder.switch_to_block(shorts);
        let meta = regions::flags(quench_runtime::staged::region::Region::HeapMeta.mutable());
        let tags = addresses.map(|address| self.raw_load(Lane::I8, meta, address, quench_runtime::vm::wtf16::native_string::TAG_OFFSET));
        let either = self.raw_binary(Binary::Or, tags[0], tags[1]);
        let both_short = self.raw_compare_imm(Relation::Equal, either, i64::from(quench_runtime::vm::wtf16::native_string::TAG_SHORT));
        self.branch_if(both_short, (short, &[]), (compare, &[]));
        self.builder.switch_to_block(short);
        let mut differ = self.raw_const(Lane::I64, 0);
        for word in 0..quench_runtime::staged::string_form::RECORD_SIZE / 8 {
            let [a, b] = addresses.map(|address| self.raw_load(Lane::I64, meta, address, (word * 8) as i32));
            let bits = self.raw_binary(Binary::Xor, a, b);
            differ = self.raw_binary(Binary::Or, differ, bits);
        }
        let same_text = self.raw_compare_imm(Relation::Equal, differ, 0);
        self.goto(join, &[same_text]);
        self.builder.switch_to_block(compare);
        let result = self.helper_call(LaneHelper::StrictEqual, &[left, right]);
        let cells_equal = self.raw_unary(Unary::Narrow(Lane::I8), result);
        self.goto(join, &[cells_equal]);
        self.builder.switch_to_block(join);
        Some(joined)
    }

    fn number_relation(&mut self, state: &mut State, pc: u32, operator: u32,
        left: Operand, right: Operand) -> Option<ir::Value> {
        let relation = super::operations::NumberRelation::from_operator(operator)?;
        if !self.operand_supported(left) || !self.operand_supported(right) || !self.numbers_supported(left, right) {
            return None;
        }
        let exit = self.guard_exit(state, pc);
        if let Some(domain) = self.number_domain(state, left, right) {
            return Some(self.number_relation_step(state, exit, relation, left, right, domain));
        }
        // Operands only their tags type compare as int32 where both tags say so, else as
        // Numbers; neither path proves a fact the other does not.
        let (left, right) = (self.operand_tagged(state, left)?, self.operand_tagged(state, right)?);
        let (left_int, right_int) = (is_int_tagged(&mut self.builder, left), is_int_tagged(&mut self.builder, right));
        let ints = self.raw_binary(Binary::And, left_int, right_int);
        let (int_block, number_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        let holds = self.builder.append_block_param(join, types::I8);
        self.branch_if(ints, (int_block, &[]), (number_block, &[]));
        self.builder.switch_to_block(int_block);
        let (left_bits, right_bits) = (self.raw_unary(Unary::Narrow(Lane::I32), left), self.raw_unary(Unary::Narrow(Lane::I32), right));
        let relation = super::operations::NumberRelation::from_operator(operator)?;
        let int_holds = quench_runtime::staged::number_machine::number_relation(&mut super::object_slots::emit(&mut self.builder), relation, left_bits, right_bits, NumberRelationDomain::Int32);
        self.goto(join, &[int_holds]);
        self.builder.switch_to_block(number_block);
        let left = frame::number_word(&mut self.builder, state, left, exit);
        let right = frame::number_word(&mut self.builder, state, right, exit);
        let number_holds = quench_runtime::staged::number_machine::number_relation(&mut super::object_slots::emit(&mut self.builder), relation, left, right, NumberRelationDomain::Float64);
        self.goto(join, &[number_holds]);
        self.builder.switch_to_block(join);
        Some(holds)
    }

    /// A Number relation of `domain`, whose operands its staged step proves.
    fn number_relation_step(&mut self, state: &mut State, miss: ir::Block,
        relation: super::operations::NumberRelation, left: Operand, right: Operand,
        domain: quench_runtime::staged::Domain) -> ir::Value {
        let operation = quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::Relation { left, right, relation, domain });
        match self.step(state, frame::Edges::ValueGuarded { miss }, operation) {
            quench_runtime::staged::Step::Value(value) => value,
            _ => unreachable!("a relation produces a predicate"),
        }
    }

    /// `source`'s value into `destination`, with every fact proved of it.
    fn copy(&mut self, state: &mut State, destination: Register, source: Register) -> ir::Value {
        frame::register_copy(&mut self.builder, &mut self.registers, &mut self.written, state,
            destination, source)
    }

    /// The inlined call's receiver admitted where no dominating proof covers it: the body's own
    /// receiver recovery, a guard leaving at `exit`, but for a restartable numeric callee.
    fn guard_inlined_receiver(&mut self, state: &mut State, frame: usize, exit: ir::Block) {
        let inlined = self.unit.frames[frame];
        let receiver = self.tagged(state, inlined.receiver);
        // A numeric body has no entry facts: only a dominating proof admits its receiver.
        let entry = self.entry.as_ref();
        let admitted = entry.is_some_and(|entry| entry.call_receivers.contains(frame))
            || state.proofs.headers[usize::from(inlined.receiver)].is_some()
            || entry.is_some_and(|entry| entry.records[usize::from(inlined.receiver)].is_some()
                || (state.proofs.receivers.contains(&inlined.receiver) && entry.records[state.values.len()].is_some()));
        let callee = self.program.function(inlined.target.function as usize);
        let numeric = inlined.recovery == super::lane_unit::CallRecovery::RestartCall
            && restartable_numeric_function(self.program, callee, callee.instructions());
        if !admitted && !numeric {
            let admitted = self.receiver_recovery(receiver, ReceiverRecovery::for_body(callee));
            emit_js_guard(&mut self.builder, admitted, exit);
        }
    }

    /// The call marker of inlined `frame` (`lane_unit::unit`): its callee is the function
    /// the call's site linked (an identity, compared) or a closure of the statically bound
    /// function (admitted by the VM's binding rule), and its receiver a direct object, which
    /// a plain call binds as `this`; a callee other than this frame's goes to the next
    /// alternative, and anything else leaves at the call, before its effects. The callee's
    /// parameters take the call's arguments and its other registers `undefined`, as its
    /// activation would (`push_passed_in_lane`).
    fn inline_call(&mut self, state: &mut State, pc: usize, frame: usize) -> bool {
        let inlined = self.unit.frames[frame];
        let exit = self.guard_exit(state, pc as u32);
        // An OSR region's callee comes from the entry snapshot: the caller's read is
        // resolved at compile time and dropped, and its register is reused by the loop.
        let callee = match inlined.callee_entry_word {
            Some(word) => self.loop_entry_word(word),
            None => self.tagged(state, inlined.callee_register),
        };
        let callee_index = usize::from(inlined.callee_register);
        let (same, proved) = match inlined.target.callee {
            // A dominating proof of the identity needs no compare.
            CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } if state.values[callee_index].value.ty.function_identity() == Some(identity)
                || self.entry.as_ref().is_some_and(|entry| entry.calls.contains(frame)) =>
                    (None, Type::function(identity)),
            CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } => (
                Some(self.is_identity(callee, identity)),
                Type::function(identity),
            ),
            CalleeBinding::Function => (
                if state.values[callee_index].value.ty.body_index() == Some(inlined.target.function)
                    || self.entry.as_ref().is_some_and(|entry| entry.calls.contains(frame))
                    || self.closed_call(state, exit, frame) { None }
                    else { Some(self.function_binding(callee, inlined.target.function)) },
                Some(Type::function_body(inlined.target.function)),
            ),
        };
        if let Some(same) = same {
            match inlined.alternative {
                Some(next) => {
                    let (matched, other) = (self.builder.create_block(), self.builder.create_block());
                    self.branch_if(same, (matched, &[]), (other, &[]));
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
        // A forwarded call is the call of the function it forwards to while its native is the one seen.
        if let Some((register, through)) = inlined.function_call {
            let native = self.tagged(state, register);
            let seen = self.is_identity(native, through);
            emit_js_guard(&mut self.builder, seen, exit);
        }
        let callee = self.program.function(inlined.target.function as usize);
        self.guard_inlined_receiver(state, frame, exit);
        for register in 0..inlined.registers {
            let destination = inlined.base + register;
            let undefined = self.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
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
        let object = quench_runtime::staged::value::is_direct_object(&mut super::object_slots::emit(&mut self.builder), receiver);
        match recovery {
            ReceiverRecovery::Object => object,
            ReceiverRecovery::ObjectOrNullish => {
                let undefined = self.raw_compare_imm(Relation::Equal, receiver, Value::UNDEFINED.to_bits() as i64);
                let null = self.raw_compare_imm(Relation::Equal, receiver, Value::NULL.to_bits() as i64);
                let nullish = self.raw_binary(Binary::Or, undefined, null);
                self.raw_binary(Binary::Or, object, nullish)
            }
        }
    }

    /// A call or a construction the body does not inline, by the staged call (`staged::invoke`):
    /// the body's registers written back, then a direct call of a compiled callee or the VM's
    /// call; its completion continues, a leave in the callee stops the body, and a miss leaves
    /// at `pc`. The callee may allocate, which may move the property arena, whose base and
    /// length the body reloads.
    fn call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        use quench_runtime::staged::{CallOperation, Operation};
        let decoded = Operation::<ir::Value, EmitProof>::decode(instruction, quench_runtime::staged::DecodeContext::of(self.program));
        // A callee's exception must go where native code takes it (`Lowering::raises_from`): a call
        // it cannot unwind from is made by the lane.
        let origin = self.unit.origins.get(pc).copied().filter(|_| self.raises_from(pc));
        let (Some(super::lane_unit::Origin::Caller(record)), Some(Operation::Call(CallOperation::Site(site)))) = (origin, decoded) else {
            self.exit(state, pc as u32);
            return false;
        };
        let missed = self.guard_exit(state, pc as u32);
        self.home_written(state, pc);
        let completed = self.builder.create_block();
        let raised = cold_block(&mut self.builder);
        let entered = self.builder.create_block();
        // The callee's exception block is filled before the call's blocks, from the state the call is made in.
        self.goto(entered, &[]);
        self.raised_call(state, pc, record, raised);
        self.builder.switch_to_block(entered);
        // SAFETY: the body owns its callees for as long as its code runs.
        let callees: Vec<&DirectCallee> = self.direct.get(&pc).map(|sites| sites.iter().map(|callee| unsafe { &**callee }).collect()).unwrap_or_default();
        // An image's call the closed world proves reads its callee's words from the site's record (`ProvenCall`): a
        // construction takes the VM's call, as the proven callee is not a construction's.
        let proven = self.vm.ahead.and_then(|ahead| ahead.calls.get(&pc).copied()).filter(|_| !site.constructs());
        // The declared result the call's completion conforms to: the callee's, where every candidate declares the same one.
        let returns = callees.iter().filter(|_| !site.constructs())
            .map(|callee| declared::declared_return(self.program, callee.target.function))
            .reduce(|first, next| if first == next { first } else { None }).flatten();
        // The result a bound callee's summary types (`call_results`): its native return continues at `returned` with
        // the word, and the helper's completion tests it (`typed_completion`).
        let mut typed = None;
        match site.callee {
            quench_runtime::staged::forms::Callee::Register(register) if callees.len() >= 2 && !site.constructs() =>
                self.polymorphic_call(state, site, record, register, &callees, (completed, missed, raised)),
            _ => {
                let owned = callees.first().copied();
                let callee = owned.map(DirectSite::Owned).or(proven.map(DirectSite::Proven));
                if callee.is_some() && !site.returns && !site.constructs() && let Some(ty) = self.observed.at(pc).filter(|ty| self.typed_native_result(pc, *ty)) {
                    let returned = self.builder.create_block();
                    self.builder.append_block_param(returned, types::I64);
                    typed = Some((ty, returned));
                }
                let generic = self.builder.create_block();
                // A construction of a stores-only constructor, whose arguments are a window of the caller's
                // registers (`CallSite::initializing`): its receiver is allocated finished and its body not entered.
                let initializing = match (site.arguments, owned) {
                    // A forwarding site's target is the forwarded initializer, whose receiver its forwarder's plan
                    // allocates (`DirectCallee::forwarder`): that path keeps its body.
                    (quench_runtime::staged::Arguments::Window { first, count }, Some(callee))
                        if site.constructs() && callee.forwarder.is_none() && self.initializes_on_construction(callee.target.function) => Some((first, count)),
                    _ => None,
                };
                let returned = typed.map(|(_, returned)| returned);
                let call = CallSite { callee, record, context: self.context, registers: self.registers_base, completed, returned, missed, raised,
                    stop: self.call_out_stop(), leave: self.leave_site(), initializing };
                self.step_with(state, frame::Edges::ValueGuarded { miss: generic }, Operation::Call(CallOperation::Site(site)), StepWith { site: Some(Site::Call(call)), ..StepWith::default() });
            }
        }
        self.reentries.push(completed);
        // A returning call's completion is its activation's: the lane completes it with the
        // result, which the call left in its register (`LaneExit::Returned`).
        if site.returns {
            self.reload_result(state, site.result);
            return self.return_register(state, record, site.result);
        }
        state.call_effect();
        // A direct call whose callee's summary types its result (`call_results`): the facts assume that type after
        // the call. The helper's completion checks it on its own path; the native return already holds it.
        let typed = typed.map(|(ty, returned)| (ty, self.typed_completion(state, pc, site.result, ty, returned)));
        self.reload_property_arena();
        match typed {
            Some((ty, value)) => self.define_typed_result(state, site.result, value, ty),
            None => self.reload_result(state, site.result),
        }
        if let Some(ty) = returns { self.conform_declared_result(state, pc, site.result, ty); }
        // Any other call's result is checked once where the call completes, whichever path completed it.
        if typed.is_none() && let Some(ty) = self.observed.at(pc) { self.conform_declared_result(state, pc, site.result, ty); }
        true
    }

    /// Whether a direct call's completion takes the result `ty` its callee's summary types without a test on the native
    /// return (`typed_completion`): the resumed point is a completion, and the fact is a Number or a tagged type the
    /// completion's test checks by its tag. An int32 keeps its test, as an integral double is a Number and not its tag.
    fn typed_native_result(&self, pc: usize, ty: Type) -> bool {
        self.resumes_after(pc) && ty.object_shape().is_none() && ty != Type::DIRECT_OBJECT_FAMILY
            && (ty == Type::NUMBER || number_domains::target_representation(ty) == Representation::Tagged)
    }

    /// Joins a direct call's native return and its helper's completion, both continuing with the call's result in the
    /// representation `ty` takes. The native return's word is the callee's, which its summary types, so it is only
    /// decoded; the helper's completion tests the result on its own path, from a copy of the state. Continues in the join,
    /// whose parameter is the result's value.
    fn typed_completion(&mut self, state: &State, pc: usize, result: Register, ty: Type, returned: ir::Block) -> ir::Value {
        let float = number_domains::target_representation(ty) == Representation::Float64;
        let join = self.builder.create_block();
        let param = self.builder.append_block_param(join, if float { types::F64 } else { types::I64 });
        let base = self.registers_base;
        let bits = self.raw_load(Lane::I64, regions::frame(), base, register_offset(result));
        let mut helper = state.clone();
        helper.call_effect();
        self.set_tagged(&mut helper, result, bits);
        self.conform_register(&mut helper, pc + 1, result, ty);
        let helper_value = if float {
            let variable = self.float_var(usize::from(result));
            self.builder.use_var(variable)
        } else { bits };
        self.goto(join, &[helper_value]);
        self.reentries.push(returned);
        self.builder.switch_to_block(returned);
        let word = self.builder.block_params(returned)[0];
        let native = if float { emit_known_number(&mut self.builder, word) } else { word };
        self.goto(join, &[native]);
        self.builder.switch_to_block(join);
        param
    }

    /// Defines a direct call's result `value` as its typed fact `ty` (`typed_completion`).
    fn define_typed_result(&mut self, state: &mut State, result: Register, value: ir::Value, ty: Type) {
        let representation = number_domains::target_representation(ty);
        self.define(state, result, value, RegisterFact::definition(ty, representation));
        state.proofs.homed.insert(usize::from(result));
    }

    /// A call of a register callee that one of `callees` may be (several direct candidates of the
    /// site): a chain of tests on the callee register, each leading to its candidate's direct call,
    /// and the last to a call by the VM alone. A test's candidate is the one its step guards again,
    /// which holds. Each step gets its own miss block, which its VM call fills (`CallFrame::call_helper`
    /// appends the call to the block it is given, so blocks are not shared). The steps share `completed`,
    /// `missed` and `raised`, and the chain ends at `completed`.
    fn polymorphic_call(&mut self, state: &mut State, site: quench_runtime::staged::Invocation, record: u32, register: Register,
        callees: &[&DirectCallee], (completed, missed, raised): (ir::Block, ir::Block, ir::Block)) {
        use quench_runtime::staged::{CallOperation, Operation};
        let (stop, leave, context, registers) = (self.call_out_stop(), self.leave_site(), self.context, self.registers_base);
        // The callee register is read once: the steps write nothing before a test runs.
        let word = self.raw_load(Lane::I64, regions::frame(), self.registers_base, register_offset(register));
        let tests: Vec<ir::Block> = callees.iter().map(|_| self.builder.create_block()).collect();
        let fallback = self.builder.create_block();
        self.goto(tests[0], &[]);
        for (index, &callee) in callees.iter().enumerate() {
            self.builder.switch_to_block(tests[index]);
            let arm = self.builder.create_block();
            let next = tests.get(index + 1).copied().unwrap_or(fallback);
            let matched = self.raw_compare_imm(Relation::Equal, word, callee.callee.to_bits() as i64);
            self.branch_if(matched, (arm, &[]), (next, &[]));
            self.builder.switch_to_block(arm);
            let miss = cold_block(&mut self.builder);
            let call = CallSite { callee: Some(DirectSite::Owned(callee)), record, context, registers, completed, returned: None, missed, raised, stop, leave, initializing: None };
            self.step_with(state, frame::Edges::ValueGuarded { miss }, Operation::Call(CallOperation::Site(site)), StepWith { site: Some(Site::Call(call)), ..StepWith::default() });
        }
        // No candidate's identity: the VM's call alone, as a site with no direct entry makes it.
        self.builder.switch_to_block(fallback);
        let miss = cold_block(&mut self.builder);
        let call = CallSite { callee: None, record, context, registers, completed, returned: None, missed, raised, stop, leave, initializing: None };
        self.step_with(state, frame::Edges::ValueGuarded { miss }, Operation::Call(CallOperation::Site(site)), StepWith { site: Some(Site::Call(call)), ..StepWith::default() });
    }

    /// The activation completes with `register`, by the staged return (`Frame::ret`) at the
    /// record that completes it (`LaneExit::Returned`). Never falls through.
    fn return_register(&mut self, state: &mut State, record: u32, register: Register) -> bool {
        let site = Site::Return(ReturnSite { record, leave: self.leave_site() });
        self.step_with(state, frame::Edges::Value, quench_runtime::staged::Operation::Return { source: register }, StepWith { site: Some(site), ..StepWith::default() });
        self.finishes = true;
        false
    }

    /// The activation completes with `register`, which the record at `pc` names. Only a caller
    /// record can: an inlined callee's `Return` is the unit's move and jump.
    fn complete(&mut self, state: &mut State, pc: usize, register: Register) -> bool {
        let Some(super::lane_unit::Origin::Caller(record)) = self.unit.origins.get(pc).copied() else {
            self.exit(state, pc as u32);
            return false;
        };
        self.native += 1;
        self.return_register(state, record, register)
    }

    /// Stops with the activation's completion: `value` in the result word, `record` the
    /// record that completes it (`LANE_BODY_RETURNED`).
    fn return_value(&mut self, record: u32, value: ir::Value) {
        self.finishes = true;
        let leave = self.leave_site();
        ReturnSite { record, leave }.complete(&mut self.builder, self.payload, value);
    }

    /// Where this run's completion goes (`Leave`).
    fn leave_site(&self) -> Leave {
        Leave { block: self.leave }
    }

    /// Leaves with `status`, and with `word` as the result word where a run publishes one.
    pub(crate) fn leave_with(&mut self, status: ir::Value, word: Option<ir::Value>) {
        self.leave_site().with(&mut self.builder, status, word);
    }

    fn continue_branch(&mut self, state: &mut State, pc: usize, target: usize,
        taken_block: ir::Block, fall_block: ir::Block) {
        let back = target <= pc && target == self.coverage.entry();
        self.backedge |= back;
        let refinements = std::mem::take(&mut state.refinements);
        self.switch_to(taken_block, true);
        if back { self.count_trip(); }
        let mut taken_state = state.clone();
        taken_state.narrow(quench_runtime::staged::Edge::Target, &refinements);
        self.continue_at(&mut taken_state, target);
        self.switch_to(fall_block, true);
        let mut fall_state = state.clone();
        fall_state.narrow(quench_runtime::staged::Edge::Following, &refinements);
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
            if !matches!(crate::compiled::decoded(instructions[back]), Some(quench_runtime::staged::Operation::Jump))
                || self.unit.markers.iter().any(|pc| (header..=back).contains(pc)) { continue; }
            let first = instructions[header];
            if !matches!(crate::compiled::decoded(first), Some(quench_runtime::staged::Operation::GetField { .. })) || first.result_flags() != 0 { continue; }
            let Some(FieldLookup::Atom { base, .. }) = first.field_lookup() else { continue };
            let Some(receiver) = base.register_index() else { continue };
            let mut required = receiver;
            let mut inputs = smallvec::SmallVec::<[(usize, Register); INLINE_LOOP_INPUTS]>::new();
            inputs.push((header, receiver));
            let mut predicate = None;
            for pc in (header + 1..back).rev() {
                let instruction = instructions[pc];
                match () {
                    _ if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) && instruction.result_flags() == 0 => {
                        if instruction.register_a() == required { required = instruction.register_b(); }
                    }
                    _ if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Nop)) => {},
                    _ => { predicate = Some(pc); break; }
                }
                if leaders.contains(pc) { inputs.push((pc, required)); }
            }
            let Some(pc) = predicate else { continue };
            let Some(comparison) = nullish_comparison(&self.program.constants, instructions[pc], |_| Type::ANY)
                .filter(|comparison| !comparison.strict && comparison.negated
                    && instructions[pc].jump_target() as usize > back) else { continue };
            let root = |register| move_root(instructions, leaders, (header, pc), register);
            if root(required) != root(comparison.source) { continue; }
            if !inputs.iter().any(|&(leader, _)| leader == pc + 1) { inputs.push((pc + 1, required)); }
            if !inputs.iter().all(|&(pc, register)| self.facts.at(pc)
                .and_then(|types| types.get(usize::from(register)))
                .is_some_and(|ty| ty.is_subtype(Type::DIRECT_OBJECT_FAMILY)
                    || Type::DIRECT_OBJECT_FAMILY.is_subtype(*ty))) { continue; }
            for (pc, register) in inputs { self.facts.require(pc, register, Type::DIRECT_OBJECT_FAMILY); }
        }
    }

    /// Whether a loop definition of `register` of type `ty` makes it a candidate for an int32
    /// entry contract: the definition yields an int32, or the register held one at entry.
    fn int32_candidate(&self, register: Register, ty: Type) -> bool {
        ty.is_int32() || self.int_entries.iter().any(|(entry, _)| *entry == register)
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
        self.facts.for_each_definition(instructions, boundaries, &self.program.constants, &writes, self.native_calls, header..=back,
            |pc, register, ty| {
                let index = usize::from(register);
                if (header..=back).contains(&pc) && register < self.unit.caller_registers
                    && self.is_live(header, register) && self.written.contains(index)
                    && initial[index].is_any_or_number() && self.int32_candidate(register, ty) {
                    candidates.insert(index);
                }
            });
        if let Some(facts) = super::facts::inductive_int32(initial, candidates, header..=back,
            instructions, boundaries, &self.program.constants, &writes, own_shape, self.native_calls, &self.raises, self.observed) {
            self.facts = facts;
        }
        // An accumulator that entered as a Number (an int32 or a double) and that every
        // definition keeps a Number is carried as a double, as C carries its `double` sum: the
        // entering edge proves it once, and the loop neither boxes nor tests it.
        let mut numbers: CompoundBitSet = CompoundBitSet::with_capacity(initial.len());
        self.facts.for_each_definition(instructions, boundaries, &self.program.constants, &writes, self.native_calls, header..=back,
            |pc, register, _| {
                let index = usize::from(register);
                let entered = self.int_entries.iter().any(|(entry, _)| *entry == register) || self.number_entries.contains(&register);
                if (header..=back).contains(&pc) && register < self.unit.caller_registers && entered
                    && self.is_live(header, register) && self.written.contains(index) && initial[index] == Type::ANY {
                    numbers.insert(index);
                }
            });
        if !numbers.is_empty() && let Some(facts) = super::facts::inductive(initial, numbers, Type::NUMBER, header..=back,
            instructions, boundaries, &self.program.constants, &writes, own_shape, self.native_calls, &self.raises, self.observed) {
            self.facts = facts;
        }
    }

    /// The registers a loop never writes whose every use proves them Numbers, each a Number header
    /// contract: proved once on the entering edge, so the body holds it as a double and tests it no more.
    fn number_inputs(&self, header: usize, back: usize, defined: &CompoundBitSet, candidates: &mut Vec<(super::facts::Contract, std::ops::RangeInclusive<usize>)>) {
        let Some(types) = self.facts.at(header) else { return };
        for instruction in &self.unit.instructions[header..=back] {
            for register in super::facts::proves_numbers(*instruction, types.len(), &self.program.constants, &writes, self.native_calls) {
                let index = usize::from(register);
                let contract = (header, register, Type::NUMBER);
                if !defined.contains(index) && !types[index].is_subtype(Type::NUMBER) && self.is_live(header, register)
                    && !candidates.iter().any(|(known, _)| *known == contract) {
                    candidates.push((contract, header..=back));
                }
            }
        }
    }

    /// An inner loop's counters and indices carried as int32s: a register its header joins as
    /// a Number (an index computed before the loop, say) while every definition in the loop
    /// yields an int32 takes an int32 header contract. The edge entering the loop proves it
    /// once (`conform`), instead of each use re-proving an exact int32 of a double.
    fn refine_loop_inputs(&mut self, entry: usize, initial: &[Type], instructions: &[WideInstruction],
        boundaries: &CompoundBitSet, loops: &[(usize, usize)],
        own_shape: &dyn Fn(usize) -> Option<(Register, u32)>) {
        let mut candidates = Vec::new();
        for &(header, back) in loops.iter().filter(|(header, _)| *header != self.coverage.entry()) {
            let Some(types) = self.facts.at(header) else { continue };
            let mut defined: CompoundBitSet = CompoundBitSet::with_capacity(types.len());
            for instruction in &instructions[header..=back] {
                for register in writes(*instruction) { defined.insert(usize::from(register)); }
            }
            for (index, ty) in types.iter().enumerate() {
                if *ty == Type::NUMBER && defined.contains(index) && self.is_live(header, index as Register) {
                    candidates.push(((header, index as Register, Type::INT32), header..=back));
                }
            }
            self.number_inputs(header, back, &defined, &mut candidates);
        }
        if let Some(facts) = super::facts::loop_int32(entry, initial, candidates, instructions, boundaries,
            &self.program.constants, &writes, own_shape, self.native_calls, &self.raises, self.observed) {
            self.facts = facts;
        }
    }

    /// The type already proved for an operand at a use. Feedback is not enough:
    /// an admitted field read carries its actual loop-entry contract, even when
    /// the general analysis has no field feedback (as in linked AOT).
    fn int32_operand_at(&self, pc: usize, operand: Operand, boundaries: &CompoundBitSet) -> bool {
        if super::facts::operand_type(&self.program.constants, operand, |_| Type::ANY).is_int32() { return true; }
        let Some(mut register) = operand.register_index() else { return false };
        if self.facts.before(pc, register, &self.unit.instructions, boundaries,
            &self.program.constants, &writes, self.native_calls).is_int32() { return true; }
        for at in (0..pc).rev() {
            if boundaries.contains(at + 1) || self.unit.markers.contains(&at) { return false; }
            let instruction = self.unit.instructions[at];
            if instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { return false; }
            if writes(instruction).contains(&register) {
                if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) && instruction.result_register() == register {
                    register = instruction.register_b();
                } else {
                    return instruction.defined_register() == Some(register)
                        && self.hoisted.get(&at).is_some_and(|read| read.ty == Type::INT32);
                }
            }
        }
        false
    }

    /// Whether the capture `instruction` reads held an int32 when this body compiled, so its
    /// entry guard takes the checked integer view (a guess the guard checks, as for `int_entries`).
    fn capture_observed_int(&self, instruction: WideInstruction) -> bool {
        instruction.capture_depth() == 0 && self.int_captures.contains(&quench_runtime::bytecode::ImmediateLayout::capture_slot(instruction.imm()))
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
            let operator = match () {
                _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::CompareJump { .. })) => Some(instruction.binary_operator_field()),
                _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) => Some(instruction.binary_operator()),
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
            let copied = matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) && aliases.contains(usize::from(instruction.register_b()));
            for register in writes(instruction) { aliases.remove(usize::from(register)); }
            if copied { aliases.insert(usize::from(instruction.result_register())); }
            if aliases.is_empty() { break; }
        }
        false
    }

    /// Whether an enclosing loop's entry hoisted the capture read at `pc` already (its body holds
    /// this loop's, so the read is invariant there too): reading it again at each entry here would
    /// be a helper call inside the outer loop, spilling every value live across it.
    fn hoisted_by_enclosing_loop(&self, pc: usize, owner: Option<Register>) -> bool {
        owner.is_none() && matches!(crate::compiled::performed(&self.unit.instructions[pc]), Some(quench_runtime::staged::Operation::LoadCapture { .. })) && self.hoisted.contains_key(&pc)
    }

    /// The hoisted read of the callee an inlined capture read belongs to, where it is the callee
    /// the frame's call binds: `Ok(None)` for a caller's own read, `Err` where the read is not
    /// admitted (its loop then reads it where it runs).
    fn admitted_callee_read(&mut self, state: &mut State, pc: usize, reads: &[InvariantRead], boundaries: &CompoundBitSet,
        exit: ir::Block, held: Option<Register>) -> Result<Option<ir::Value>, ()> {
        let super::lane_unit::Origin::Inlined { frame, .. } = self.unit.origins[pc] else { return Ok(None) };
        if let Some(register) = held {
            // The closure the entry holds, admitted by its function on this edge: the capture view
            // every iteration reads through is the one it resolves here.
            let callee = self.tagged(state, register);
            let function = self.unit.frames[frame].target.function;
            let matched = self.function_binding(callee, function);
            emit_js_guard(&mut self.builder, matched, exit);
            return Ok(Some(callee));
        }
        let read = invariant_callee_read(self.unit, reads.iter().map(|read| read.pc), frame, boundaries)
            .and_then(|read| self.hoisted.get(&read).copied()).ok_or(())?;
        let target = self.unit.frames[frame].target;
        let admitted = match target.callee {
            CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } => read.ty.function_identity() == Some(identity),
            CalleeBinding::Function => read.ty.body_index() == Some(target.function),
        };
        if admitted { Ok(Some(self.builder.use_var(read.value))) } else { Err(()) }
    }

    /// The loop's invariant captures and fields, read once on its entry edge.
    fn hoist_reads(&mut self, state: &mut State, header: usize) {
        let Some(reads) = self.hoists.get(&header).cloned() else { return };
        let boundaries = leaders(&self.unit.instructions);
        for InvariantRead { pc, owner, rooted, callee } in reads.iter().copied() {
            let instruction = self.unit.instructions[pc];
            if self.hoisted_by_enclosing_loop(pc, owner) { continue; }
            let field = self.loop_fields.iter().position(|field| field.reads[0] == pc);
            let exit = self.guard_exit(state, header as u32);
            let (value, int32) = if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) {
                let Ok(admitted_callee) = self.admitted_callee_read(state, pc, &reads, &boundaries, exit, callee) else { continue };
                let value = self.load_capture(state, pc, instruction, exit, admitted_callee, if rooted { RootedRead::OnEntry } else { RootedRead::No });
                let int32 = self.capture_int32_use(pc, &boundaries) || self.capture_observed_int(instruction);
                (value, int32)
            } else {
                let Some(FieldLookup::Atom { atom, base, cache_site }) = instruction.field_lookup() else { continue };
                let register = owner.or_else(|| base.register_index());
                // A `length` read is made once, natively where its base is a direct array.
                match register.filter(|_| super::names_length(self.program, atom)) {
                    Some(register) => {
                        let length = self.length_read(state, pc, register, (atom, cache_site), exit, Some(header));
                        (int_tagged(&mut self.builder, length), true)
                    }
                    None => {
                        let base = register.map_or(quench_runtime::bytecode::FieldBase::THIS, quench_runtime::bytecode::FieldBase::register);
                        let lookup = FieldLookup::Atom { atom, base, cache_site };
                        let Some(value) = self.field_value(state, (base, lookup), &[(atom, cache_site)], HeldRead::default(), exit) else { continue };
                        (value, field_reads::int32_read(self.program, self.fields, instruction))
                    }
                }
            };
            let carried = field.is_some();
            let int32 = if carried {
                self.loop_fields[field.unwrap()].stores.iter().all(|store| {
                    self.facts.before(*store, self.unit.instructions[*store].register_a(),
                        &self.unit.instructions, &boundaries, &self.program.constants, &writes, self.native_calls).is_int32()
                })
            } else { int32 };
            let number = carried && !int32 && self.carries_number(&self.loop_fields[field.unwrap()], instruction, &boundaries);
            if int32 {
                let integer = is_int_tagged(&mut self.builder, value);
                emit_js_guard(&mut self.builder, integer, exit);
            }
            let mut ty = if int32 { Type::INT32 } else if number { Type::NUMBER } else { Type::ANY };
            if !int32 && !number {
                // This is a candidate, never a proof from source syntax. Admit the
                // loaded closure on the same entry edge that already hoists its
                // immutable read. A failed admission exits before loop effects.
                let target = self.unit.markers.iter().enumerate().find_map(|(frame, marker)| {
                    let call = self.unit.frames[frame];
                    let supplies = if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) {
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
                        CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } => (
                            self.is_identity(value, identity),
                            Type::function(identity).expect("admitted callee locator")),
                    };
                    emit_js_guard(&mut self.builder, matched, exit);
                    ty = admitted;
                }
            }
            let representation = if int32 { Representation::Int32 } else if number { Representation::Float64 } else { Representation::Tagged };
            let raw = match representation {
                Representation::Int32 => self.raw_unary(Unary::Narrow(Lane::I32), value),
                Representation::Float64 => {
                    frame::number_word(&mut self.builder, state, value, exit)
                }
                _ => value,
            };
            let variable = self.builder.declare_var(match representation {
                Representation::Int32 => types::I32,
                Representation::Float64 => types::F64,
                _ => types::I64,
            });
            self.builder.def_var(variable, raw);
            let receiver = if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) {
                match instruction.field_lookup() {
                    Some(FieldLookup::Atom { base, .. }) => owner.or_else(|| base.register_index())
                        .map(|owner| (owner, state.values[usize::from(owner)].value.ty)),
                    _ => None,
                }
            } else { None };
            let stores: smallvec::SmallVec<[(usize, Option<FieldSlotBinding>); INLINE_FIELD_BINDINGS]> =
                field.into_iter().flat_map(|index| self.loop_fields[index].stores.iter()).filter_map(|store| {
                    if !int32 && !number { return None; }
                    let site = self.unit.instructions[*store].cache_site_index();
                    let binding = self.entry.as_ref()?.fields.iter().find_map(|(known, binding)|
                        (*known == site).then_some(*binding));
                    Some((*store, binding))
                }).collect();
            let object_header = if !stores.is_empty() {
                receiver.map(|(owner, _)| self.object_header(state, Some(owner), false, exit))
            } else { None };
            if let (Some((owner, _)), Some(object_header)) = (receiver, object_header) {
                let object = self.tagged(state, owner);
                for (store, binding) in stores {
                    let instruction = self.unit.instructions[store];
                    let site = instruction.cache_site_index();
                    let mut binding = binding.unwrap_or_else(|| emit_js_field_binding(
                        &mut self.builder, self.payload, site, exit, None));
                    if number {
                        // The slot keeps raw doubles, which the carried Number stores as they are.
                        let shape = self.raw_unary(Unary::ZeroExtend(Lane::I64), binding.receiver);
                        let slot = binding.slot.read(&mut self.builder);
                        self.admit_slot_writes_as((shape, slot), Type::NUMBER, true, exit);
                    } else {
                        self.admit_field_writes(&mut binding, Type::INT32, exit);
                    }
                    let allowed = self.store_atom_admitted(state, instruction.atom_index(), exit);
                    emit_js_guard(&mut self.builder, allowed, exit);
                    let packed = self.builder.create_block();
                    let packed_address = self.builder.append_block_param(packed, types::I64);
                    let done = self.builder.create_block();
                    let address = self.builder.append_block_param(done, types::I64);
                    let (arena_address, _) = emit_js_cached_field_slot(&mut self.builder,
                        self.payload, object, site,
                        false, exit, Some(binding), Some(object_header), Some(packed));
                    self.goto(done, &[arena_address]);
                    self.builder.switch_to_block(packed);
                    if number {
                        // A packed record holds narrow values, never a double: the loop runs in the lane.
                        self.goto(exit, &[]);
                    } else {
                        self.goto(done, &[packed_address]);
                    }
                    self.builder.switch_to_block(done);
                    let packed_range = field.and_then(|index| {
                        let field = &self.loop_fields[index];
                        field.stores.iter().position(|pc| *pc == store).and_then(|index| field.store_ranges[index])
                    });
                    let domain = if number { Type::NUMBER } else { Type::INT32 };
                    self.loop_stores.insert(store, LoopStore { header: object_header, address, packed_range, domain });
                }
            }
            let read = LoopRead { value: variable, ty, representation, receiver, header: object_header, step: None };
            if let Some(index) = field {
                for pc in &self.loop_fields[index].reads { self.hoisted.insert(*pc, read); }
            } else { self.hoisted.insert(pc, read); }
        }
    }

    /// Nested sites retain callee coordinates; atom operands are already renamed by lane_unit.
    fn field_base(&self, pc: usize, instruction: WideInstruction) -> Option<quench_runtime::bytecode::FieldBase> {
        use quench_runtime::bytecode::FieldBase;
        match instruction.field_lookup()? {
            FieldLookup::Atom { base, .. } => Some(base),
            FieldLookup::Site(index) => {
                let base = self.program.field_sites.get(index)?.base;
                match self.unit.origins.get(pc)? {
                    super::lane_unit::Origin::Inlined { frame, .. } => {
                        let frame = self.unit.frames[*frame];
                        Some(FieldBase::register(if base == FieldBase::THIS { frame.receiver }
                            else { frame.base.checked_add(base.register_index()?)? }))
                    }
                    _ => Some(base),
                }
            }
        }
    }

    /// Prove the views of the arrays the loop at `header` indexes without redefining them, on
    /// its entering edge; a base that is no such array leaves at the header.
    fn hoist_arrays(&mut self, state: &mut State, header: usize, captured: bool) {
        let arrays: Vec<LoopArray> = self.loop_arrays.iter().copied()
            .filter(|array| array.header == header && matches!(array.source, ArraySource::Capture(_)) == captured).collect();
        if arrays.is_empty() { return; }
        let exit = self.guard_exit(state, header as u32);
        for array in arrays {
            let Some(bits) = self.hoisted_array_bits(state, array) else { continue };
            let variables = self.array_variables(array);
            self.builder.def_var(variables.bits, bits);
            match array.views {
                ElementViews::Dense => {
                    let view = super::emit_dense_array_view(&mut self.builder, self.payload, bits, array.writable, exit);
                    if self.int32_loop(array) {
                        // An int32 loop keeps the backing's exact count of others, which is zero: guarded here
                        // before any mark, and kept by its int32 stores (`int32_loop`), so the view marks nothing.
                        let others = {
                            let mut stage = super::object_slots::emit(&mut self.builder);
                            stage.load_u32(quench_runtime::staged::region::Region::Elements.mutable(), view.values, quench_runtime::tier_surface::OTHERS_FROM_VALUES)
                        };
                        let none = self.raw_compare_imm(Relation::Equal, others, 0);
                        emit_js_guard(&mut self.builder, none, exit);
                    } else {
                        // Stores through the hoisted view then write the element alone.
                        if array.writable { super::emit_dense_array_uncounted(&mut self.builder, view); }
                        // A read-only array no store or append of its loop changes, read only as int32s, is
                        // proved once here: a backing with others (a non-int32 or a hole) leaves at the entry, so
                        // its reads need no tag test (`proved_int_element_read`).
                        if self.int_only_array(array) {
                            let mut stage = super::object_slots::emit(&mut self.builder);
                            quench_runtime::staged::elements::int_only_backing(&mut stage, view.values, exit)
                                .expect("the emit stage continues past its guards");
                            self.int_only.insert((array.header, array.source));
                        }
                    }
                    self.builder.def_var(variables.values, view.values);
                    self.builder.def_var(variables.length, view.length);
                    let unproved = self.raw_const(Lane::I32, 0);
                    self.builder.def_var(variables.room, unproved);
                    if (array.header..=array.back).any(|pc| native_calls::searches(&self.intrinsics, pc)) {
                        let base = self.builder.use_var(variables.bits);
                        let held = self.helper_call(LaneHelper::PrototypesHoldNoIndex, &[base]);
                        let clear = self.raw_compare_imm(Relation::NotEqual, held, 0);
                        let checked = match self.prototype_clear.get(&array.source).copied() {
                            Some(checked) => checked,
                            None => {
                                let checked = self.builder.declare_var(types::I8);
                                self.prototype_clear.insert(array.source, checked);
                                checked
                            }
                        };
                        self.builder.def_var(checked, clear);
                    }
                }
                ElementViews::Typed(_) => self.hoist_typed_view(array, bits, variables, exit),
                ElementViews::Double => {
                    let (values, length) = super::emit_double_array_view(&mut self.builder, self.payload, bits, array.writable, exit);
                    self.builder.def_var(variables.values, values);
                    self.builder.def_var(variables.length, length);
                }
            }
        }
    }

    /// The tagged value of `array`'s source on the loop's entry edge: its register's, or its
    /// hoisted capture read's (none where that read was not hoisted tagged).
    fn hoisted_array_bits(&mut self, state: &mut State, array: LoopArray) -> Option<ir::Value> {
        match array.source {
            ArraySource::Register(register) => Some(self.tagged(state, register)),
            ArraySource::Capture(pc) => {
                let read = self.hoisted.get(&pc).copied().filter(|read| read.representation == Representation::Tagged)?;
                Some(self.builder.use_var(read.value))
            }
        }
    }

    /// The typed array `bits` holds, proved from its record once on the loop's entering edge
    /// (`prove_typed_view`): a proof that leaves takes the slow arm, where the VM publishes the
    /// record (`LaneHelper::TypedArrayView`) and the proof is made again, leaving at the loop's
    /// entry where it fails still.
    fn hoist_typed_view(&mut self, array: LoopArray, bits: ir::Value, variables: ArrayVariables, exit: ir::Block) {
        let access = ViewAccess { kind: array.views.observed_kind().and_then(NativeKind::new), writable: array.writable };
        let publish = cold_block(&mut self.builder);
        let done = self.builder.create_block();
        self.prove_typed_view_into(variables, bits, access, publish);
        self.goto(done, &[]);
        self.builder.switch_to_block(publish);
        self.builder.seal_block(publish);
        let published = self.helper_call(LaneHelper::TypedArrayView, &[bits]);
        let admitted = self.raw_compare_imm(Relation::NotEqual, published, 0);
        emit_js_guard(&mut self.builder, admitted, exit);
        self.prove_typed_view_into(variables, bits, access, exit);
        self.goto(done, &[]);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
    }

    /// The proof of the typed array `bits` for `access`, leaving at `miss`, whose view the
    /// array's variables then hold.
    fn prove_typed_view_into(&mut self, variables: ArrayVariables, bits: ir::Value, access: ViewAccess, miss: ir::Block) {
        let records = TypedRecords {
            directory: emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, typed_view_directory)),
            pages: emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, typed_view_pages)),
            relocations: emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, buffer_relocations)),
        };
        let view = prove_typed_view(&mut super::object_slots::emit(&mut self.builder), records, bits, access, miss)
            .expect("the emit stage continues past its guards");
        self.builder.def_var(variables.values, view.values);
        self.builder.def_var(variables.length, view.length);
        if let ViewKind::Dynamic(kind) = view.kind { self.builder.def_var(variables.kind, kind); }
    }

    fn array_variables(&mut self, array: LoopArray) -> ArrayVariables {
        let builder = &mut self.builder;
        let length = if array.views == ElementViews::Dense { types::I32 } else { types::I64 };
        *self.array_vars.entry(array.source).or_insert_with(|| ArrayVariables {
            bits: builder.declare_var(types::I64),
            values: builder.declare_var(types::I64),
            length: builder.declare_var(length),
            kind: builder.declare_var(types::I64),
            room: builder.declare_var(types::I32),
        })
    }

    /// After the element store's helper, which may grow any record array's backing, through
    /// whichever register names it: every dense or double view hoisted around `pc` reads its
    /// backing again (`emit_dense_array_reload`, `emit_double_array_reload`), and its append room is proved again
    /// (`forget_append_rooms`). A typed region's views, which no such store moves, stay.
    fn reload_dense_arrays(&mut self, pc: usize) {
        self.forget_append_rooms(pc);
        let arrays: Vec<LoopArray> = self.loop_arrays.iter().copied()
            .filter(|array| matches!(array.views, ElementViews::Dense | ElementViews::Double) && (array.header..=array.back).contains(&pc)).collect();
        for array in arrays {
            // An int-only array has no store or append in its loop (`int_only_array`), so no reload reaches it.
            debug_assert!(!self.int_only.contains(&(array.header, array.source)), "a reload of an int-only array");
            let variables = self.array_variables(array);
            let bits = self.builder.use_var(variables.bits);
            if array.views == ElementViews::Double {
                let (values, length) = super::emit_double_array_reload(&mut self.builder, self.payload, bits);
                self.builder.def_var(variables.values, values);
                self.builder.def_var(variables.length, length);
                continue;
            }
            let (values, length) = super::emit_dense_array_reload(&mut self.builder, self.payload, bits);
            // A grown backing may count its values again; stores through the view keep none. An int32
            // loop's count stays exact (`hoist_arrays`), so its view marks nothing.
            if array.writable && !self.int32_loop(array) {
                super::emit_dense_array_uncounted(&mut self.builder, super::DenseArrayView { values, length, writable: true, uncounted: false, double: None });
            }
            self.builder.def_var(variables.values, values);
            self.builder.def_var(variables.length, length);
        }
    }

    /// The hoisted view's elements' address and count.
    fn hoisted_view(&mut self, array: LoopArray) -> (ir::Value, ir::Value) {
        let variables = self.array_variables(array);
        (self.builder.use_var(variables.values), self.builder.use_var(variables.length))
    }

    /// A typed region's hoisted array kind: the site's, which its proof compared once, or the record's.
    fn hoisted_kind(&mut self, array: LoopArray) -> ViewKind<ir::Value> {
        match array.views.observed_kind().and_then(NativeKind::new) {
            Some(native) => ViewKind::Static(native),
            None => {
                let variables = self.array_variables(array);
                ViewKind::Dynamic(self.builder.use_var(variables.kind))
            }
        }
    }

    /// A store through a hoisted dense array view at a site that left (`left_here`): a Number in place,
    /// else appended inline (`dense_append`), else the VM's store. A reference first passes the barrier
    /// gate (`frame_elements::store_gate`); a gate miss (a marked or non-direct base) takes the VM's
    /// store directly, and the in-place and appended paths only run past the gate. The VM's store,
    /// which may move any backing, is followed by the reload of every view around `pc`.
    fn dense_element_write(&mut self, state: &mut State, array: LoopArray, pc: u32, instruction: WideInstruction,
        number: bool, exit: ir::Block) {
        let index = self.int(state, instruction.register_c(), exit);
        let (values, length) = self.hoisted_view(array);
        let in_bounds = self.ranges.in_bounds(pc as usize);
        let value = self.tagged(state, instruction.register_a());
        let base = self.tagged(state, instruction.register_b());
        let key = self.tagged(state, instruction.register_c());
        let (slow, helper, done) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(slow);
        self.builder.set_cold_block(helper);
        if !number {
            let mut stage = super::object_slots::emit(&mut self.builder);
            frame_elements::store_gate(&mut stage, self.payload, base, value, helper)
                .expect("the emit stage continues past its guards");
        }
        let view = super::DenseArrayView { values, length, writable: true, uncounted: true, double: None };
        // The view's backing counts no values (`hoist_arrays`, reload): the store writes the element,
        // and the unknown count stays unknown under the store of a reference as under a Number.
        self.index_write(state, index, value, view, slow, in_bounds);
        self.goto(done, &[]);
        self.builder.switch_to_block(slow);
        self.builder.seal_block(slow);
        self.dense_append(array, pc, index, value, base, done);
        self.goto(helper, &[]);
        self.builder.switch_to_block(helper);
        self.builder.seal_block(helper);
        if self.int32_loop(array) {
            // An int32 loop's store past the end would make a hole, which its reads do not test for
            // (`int32_loop`): it leaves before any write, and the VM makes the store.
            self.goto(exit, &[]);
        } else {
            let proof = super::EmitProof::IndexSetHelper { miss: exit };
            self.step(state, frame::Edges::ValueGuarded { miss: exit },
                quench_runtime::staged::Operation::IndexSetHelper { base, key, value, proof });
            self.reload_dense_arrays(pc as usize);
            self.goto(done, &[]);
        }
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
    }

    /// The reads of `instruction`'s lookup, one per site, and the sink a nested read of the
    /// current `this` also stores to.
    fn field_chain(&self, instruction: WideInstruction) -> Option<(Vec<(Atom, u16)>, Option<(Atom, u16)>)> {
        Some(match instruction.field_lookup()? {
            FieldLookup::Atom { atom, cache_site, .. } => (vec![(atom, cache_site)], None),
            FieldLookup::Site(index) => {
                let site = *self.program.field_sites.get(index)?;
                let reads = [Some(site.first), site.second].into_iter().flatten().collect();
                (reads, instruction.writes_current_this().then_some(site.sink).flatten())
            }
        })
    }

    /// The owner and name of a plain read of `instruction`'s lookup (no sink, no completion):
    /// `None` where the lookup names a site the program does not have.
    fn plain_field(&self, instruction: WideInstruction, owner: Option<Register>, plain: bool) -> Option<Option<(Option<Register>, Atom)>> {
        Some(match instruction.field_lookup() {
            Some(FieldLookup::Atom { atom, .. }) if plain => Some((owner, atom)),
            Some(FieldLookup::Site(index)) if plain => Some((owner, self.program.field_sites.get(index)?.first.0)),
            _ => None,
        })
    }

    /// A named read: what the instruction does around its value (completing the activation,
    /// storing a sink, a direct array's `length`, a whole-program prototype value, the proofs
    /// the value carries), with the value itself one `GetField` row (`field_value`).
    fn get_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        // A returning read completes the activation with its value, at a caller record; an
        // inlined callee's is the unit's read, move and jump (`lane_unit`).
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) => None,
            (true, Some(super::lane_unit::Origin::Caller(record))) if self.completion.status_and_pc() => Some(record),
            (true, _) => return None,
        };
        // A nested lookup is a chain of reads from one base, each through its own site; a
        // guard that fails anywhere in it leaves at the instruction, whose reads have no effect.
        // A nested lookup that writes the current `this` stores the chain's value to `this`'s
        // sink field too, after its reads (`FieldSite::sink`).
        // An inlined frame's nested site names its base, and its sink's `this`, by the callee's
        // registers: they are the frame's (`lane_unit::InlinedCall`).
        let frame = match self.unit.origins.get(pc) {
            Some(super::lane_unit::Origin::Inlined { frame, .. }) => Some(self.unit.frames[*frame]),
            _ => None,
        };
        let base = self.field_base(pc, instruction)?;
        let (parts, sink) = self.field_chain(instruction)?;
        let sink_owner = frame.map(|frame| frame.receiver);
        if instruction.writes_current_this() && sink.is_none() {
            return None;
        }
        let register = base.register_index();
        if register.is_some_and(|register| register >= self.unit.registers)
            || (base != quench_runtime::bytecode::FieldBase::THIS && register.is_none())
        {
            return None;
        }
        let destination = instruction.result_register();
        let lookup = instruction.field_lookup()?;
        // An OSR region's static field read answers from the entry record, as the verified
        // lowering's does: the value the field held at entry, which the loop does not write.
        if sink.is_none() && completes.is_none() && !instruction.writes_current_this()
            && let Some(Some(word)) = self.entry_fields.get(pc).copied()
        {
            let exit = self.guard_exit(state, pc as u32);
            let held = HeldRead { entry_word: Some(word), ..HeldRead::default() };
            let bits = self.field_value(state, (base, lookup), &[], held, exit)?;
            // A register's `length` read is an int32 to the facts (`osr_compile::observed_reads`).
            if register.is_some() && super::reads_length(self.program, instruction) {
                let integer = is_int_tagged(&mut self.builder, bits);
                emit_js_guard(&mut self.builder, integer, exit);
                let int = self.raw_unary(Unary::Narrow(Lane::I32), bits);
                self.set_int(state, destination, int);
            } else {
                self.set_tagged(state, destination, bits);
            }
            return Some(true);
        }
        let exit = self.guard_exit(state, pc as u32);
        let (first, rest) = parts.split_first()?;
        // A direct array's `length` is its record's, an int32.
        if let (Some(register), [], None, None) = (register, rest, sink, completes)
            && super::names_length(self.program, first.0)
        {
            let length = self.length_read(state, pc, register, *first, exit, None);
            self.set_int(state, destination, length);
            return Some(true);
        }
        let field = self.plain_field(instruction, register, sink.is_none() && completes.is_none())?;
        // A whole-program prototype value: read the field once here as data, require the
        // value the closed program gives it, then fold it. A changed prototype fails the
        // guard and leaves to the shared body.
        if rest.is_empty() && sink.is_none() && completes.is_none()
            && !instruction.writes_current_this()
            && self.fold_frozen_field(state, (base, lookup), *first, destination, exit)
        {
            return Some(true);
        }
        let known = field.and_then(|(owner, atom)| state.proofs.fields.iter().copied()
            .find(|field| field.owner == owner && field.atom == atom));
        #[cfg(test)] if known.is_some() { record_field_forward(first.1); }
        let held = HeldRead { known: known.map(|field| field.value), ..HeldRead::default() };
        let value = self.field_value(state, (base, lookup), &parts, held, exit)?;
        if let Some((atom, site)) = sink {
            self.sink_store(state, sink_owner, (atom, site), value, exit)?;
        }
        if let Some(record) = completes {
            self.return_value(record, value);
            return Some(false);
        }
        self.define_field_read(state, pc, instruction, value, (field, rest.is_empty(), known), exit);
        Some(true)
    }

    /// The value `base`'s lookup reads through `links`, the sites of its chain: one `GetField`
    /// row under the plans their caches proved, or the value `held` already has. A guard of it
    /// that fails leaves through `exit`.
    fn field_value(&mut self, state: &mut State, (base, lookup): (quench_runtime::bytecode::FieldBase, FieldLookup), links: &[(Atom, u16)],
        held: HeldRead, exit: ir::Block) -> Option<ir::Value> {
        let mut inputs: Vec<LinkInput> = links.iter().map(|link| self.link_input(*link)).collect();
        if let Some(first) = inputs.first_mut() {
            first.probed |= self.inherited_off_record(state, base, &first.loads);
        }
        let folds = self.division().folds(quench_runtime::staged::division::INHERITED_FIELD);
        if folds { field_plans::embed_inherited(&inputs, &mut self.stable); }
        with_read_plan(&inputs, (folds, held.known.is_some()), |plans| {
            let edges = frame::Edges::ValueGuarded { miss: exit };
            match self.field_step(state, edges, quench_runtime::staged::Operation::GetField { base, lookup, destination: 0 }, (plans, held)) {
                quench_runtime::staged::Step::Value(value) => Some(value),
                _ => None,
            }
        })
    }

    /// A chain link's site, with the loads its cache proved and whether its misses probe.
    fn link_input(&self, (atom, site): (Atom, u16)) -> LinkInput {
        let loads = site_loads(self.fields, site).cloned().unwrap_or_default();
        let probed = self.probes_field_misses(&loads);
        LinkInput { atom, site, loads, probed }
    }

    /// The `length` read through `read` of the value `register` holds, as an int32: a direct
    /// array's from its record (`emit_array_length_read`), a flat string's from its own
    /// (`string_length`), any other's the VM's read, which leaves through `exit` where it
    /// misses or is not an int32.
    /// The `length` read at `pc` of `register`: a typed array a loop's entry proved
    /// (`hoist_arrays`) has the view's element count, which no store in the loop resizes, so the
    /// read is that word, as C's length is; any other base is `array_length`'s. On the entry
    /// edge of `entering`, only a view that edge already proved counts (captures are proved
    /// after the invariant reads).
    fn length_read(&mut self, state: &mut State, pc: usize, register: Register, read: (Atom, u16), exit: ir::Block,
        entering: Option<usize>) -> ir::Value {
        if let Some(string) = self.loop_string(pc, Some(register)).filter(|string| entering != Some(string.header)) {
            return self.held_length(string);
        }
        let proved = |array: &LoopArray| matches!(array.views, ElementViews::Typed(_))
            && (entering != Some(array.header) || matches!(array.source, ArraySource::Register(_)));
        if let Some(array) = self.loop_array(pc, Some(register)).filter(proved) {
            let (_, count) = self.hoisted_view(array);
            let fits = self.raw_compare_imm(Relation::UnsignedLessEqual, count, i64::from(i32::MAX));
            emit_js_guard(&mut self.builder, fits, exit);
            return self.raw_unary(Unary::Narrow(Lane::I32), count);
        }
        self.array_length(state, register, read, exit)
    }

    fn array_length(&mut self, state: &mut State, register: Register, read: (Atom, u16), exit: ir::Block) -> ir::Value {
        let base = self.tagged(state, register);
        let generic = cold_block(&mut self.builder);
        let join = self.builder.create_block();
        let length = self.builder.append_block_param(join, types::I32);
        let native = super::emit_array_length_read(&mut self.builder, self.payload, base, generic);
        self.goto(join, &[native]);
        self.builder.switch_to_block(generic);
        let probe = cold_block(&mut self.builder);
        let string = self.string_length(base, probe);
        self.goto(join, &[string]);
        self.builder.switch_to_block(probe);
        let (atom, site) = read;
        let probe = quench_runtime::staged::Operation::FieldGetHost { object: base, atom, site };
        let quench_runtime::staged::Step::Value(value) = self.field_step(state, frame::Edges::ValueGuarded { miss: exit }, probe,
            (&LanePlans::NONE, HeldRead::default())) else { unreachable!("the lane's field read is a host call") };
        let integer = is_int_tagged(&mut self.builder, value);
        emit_js_guard(&mut self.builder, integer, exit);
        let int = self.raw_unary(Unary::Narrow(Lane::I32), value);
        self.goto(join, &[int]);
        self.builder.switch_to_block(join);
        length
    }

    /// Whether the read `instruction` makes has an own-field entry to fold: a read of prototype
    /// holders only (a method of an array, a `Map`) proves no record at entry, which such a
    /// receiver would fail at every run, and asks the VM's entries on a guard miss instead.
    fn reads_own_field(&self, instruction: WideInstruction) -> bool {
        let Some(FieldLookup::Atom { cache_site, .. }) = instruction.field_lookup() else { return true };
        site_loads(self.fields, cache_site).is_none_or(|loads| loads.iter().any(|load| load.holder.is_none()))
    }

    /// Whether `register`, whose type no object proof covers, may hold a receiver the object
    /// table has no record of (a `Map`'s, a `Set`'s) at a read of inherited constants: its miss
    /// would leave at every entry, so it asks the VM's proved inherited entry instead.
    fn inherited_off_record(&self, state: &State, base: quench_runtime::bytecode::FieldBase, shapes: &[NativeFieldLoad]) -> bool {
        shapes.iter().any(|load| load.inherited.is_some())
            && base.register_index().is_some_and(|register| !state.proofs.receivers.contains(&register)
                && !state.values[usize::from(register)].value.ty.is_direct_object_family())
    }

    /// Whether a read of these shapes asks the VM's caches on a guard miss rather than leaves.
    fn probes_field_misses(&self, shapes: &[NativeFieldLoad]) -> bool {
        self.left_here || self.left_a_field_read() || NativeFieldLoad::probes_misses(shapes)
    }

    /// Whether a previous body left at some field read of this unit: its shapes were not all
    /// seen when it was planned, so every read now asks the VM's caches on a guard miss rather
    /// than leave, as this unit's other reads would leave one recompile at a time.
    fn left_a_field_read(&self) -> bool {
        self.generic.iter().any(|pc| self.unit.instructions.get(*pc).is_some_and(|instruction| matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. }))))
    }

    /// Word `word` of the loop's entry record (`frame_fields::entry_record_word`).
    fn loop_entry_word(&mut self, word: u32) -> ir::Value {
        frame_fields::entry_record_word(&mut self.builder, self.payload, word)
    }

    /// A dominating proof of `owner`'s object (`headers::object_header`).
    fn object_header(&mut self, state: &mut State, owner: Option<Register>, read: bool, exit: ir::Block) -> ObjectHeader {
        let entry = self.entry.as_ref().expect("entry facts");
        headers::object_header(&mut self.builder, &mut self.registers, state, (entry, self.payload), owner, read, exit)
    }

    fn set_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let source = instruction.register_a();
        if source >= self.unit.registers || instruction.writes_current_this() || instruction.returns_from_frame() {
            return None;
        }
        let register = self.store_owner(pc, instruction)?;
        let carried = match self.loop_fields.iter().find(|field| field.stores.contains(&pc)) {
            Some(field) => Some(*self.hoisted.get(&field.reads[0])?),
            None => None,
        };
        let exit = self.guard_exit(state, pc as u32);
        let carried_int = carried.filter(|read| read.representation == Representation::Int32)
            .map(|_| self.int(state, source, exit))
            .or_else(|| carried.filter(|read| read.representation == Representation::Float64)
                .map(|_| self.number(state, source, exit)));
        let ty = state.values[usize::from(source)].value.ty;
        let operation = quench_runtime::staged::Operation::SetField { value: source, base: register,
            atom: instruction.atom_index(), site: instruction.cache_site_index(),
            strict: matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { strict: true, .. })) };
        // A store a previous body left at may add its property (`compiled::field_add_in_lane`),
        // which changes the object's shape and may move the property arena: every proof of
        // an object retires, as another register may name it, and `this`'s facts are read
        // again.
        // The site's own shapes still store in place first: a body that left here once (before
        // the site cached a shape, say) keeps its arms, and only their miss adds.
        if self.generic.contains(&pc) && self.completion.status_and_pc() {
            let (arms, _) = self.site_store_plan(instruction.cache_site_index(), ty);
            let admitted = (!arms.is_empty()).then(|| self.store_atom_admitted(state, instruction.atom_index(), exit));
            state.proofs.write_field(instruction.atom_index());
            self.store_step(state, operation, (StorePlan::of(&arms, StoreJoin::Add), HeldRead { admitted, ..HeldRead::default() }), exit)?;
            self.after_store(state, StoreJoin::Add);
            return Some(true);
        }
        let (atom, site) = (instruction.atom_index(), instruction.cache_site_index());
        let slot = self.projected_slot(state, pc, (register, ty));
        if slot.is_some() {
            state.proofs.write_field(atom);
            self.store_step(state, operation, (StorePlan::Entry, HeldRead { slot, ..HeldRead::default() }), exit)?;
        } else {
            self.site_store(state, operation, (atom, site, ty), exit)?;
        }
        if let Some(read) = carried {
            self.carry_store(state, source, (read, carried_int));
        }
        Some(true)
    }

    /// The loop-carried variable of the field a store writes takes the stored value: the int32
    /// view proved before the store where the carried read is unboxed, else the tagged word.
    fn carry_store(&mut self, state: &mut State, source: Register, (read, int): (LoopRead, Option<ir::Value>)) {
        let bits = int.unwrap_or_else(|| self.tagged(state, source));
        self.builder.def_var(read.value, bits);
        #[cfg(test)] if read.representation == Representation::Int32 {
            LOOP_INT32_PHIS.with(|count| count.set(count.get() + 1));
        }
    }

    /// The object `instruction` stores to: a register, or the fixed `this` for `None`; `None`
    /// for a register the unit does not have.
    fn store_owner(&self, pc: usize, instruction: WideInstruction) -> Option<Option<Register>> {
        let register = if let Some(register) = instruction.field_store_register() {
            Some(register)
        } else if let Some(super::lane_unit::Origin::Inlined { frame, .. }) = self.unit.origins.get(pc) {
            Some(self.unit.frames[*frame].receiver)
        } else { None };
        (!register.is_some_and(|register| register >= self.unit.registers)).then_some(register)
    }

    /// The slot a loop's entry projected for the int32 store at `pc` (`LoopStore`), where the
    /// store's object is the record the projection addresses.
    fn projected_slot(&self, state: &State, pc: usize, (register, ty): (Option<Register>, Type)) -> Option<LoopStore> {
        self.loop_stores.get(&pc).copied().filter(|slot| ty.is_subtype(slot.domain)
            && state.proofs.headers[state.header_index(register)].is_some_and(|header| header.record == slot.header.record))
    }

    /// The named store of `operation` through its site's plan: `ty` is the stored value's type,
    /// and the site's own shapes, binding and completion decide how it writes (`site_store_plan`).
    fn site_store(&mut self, state: &mut State, operation: quench_runtime::staged::Operation<ir::Value, EmitProof>,
        (atom, site, ty): (Atom, u16, Type), exit: ir::Block) -> Option<()> {
        let (arms, join) = self.site_store_plan(site, ty);
        let plan = StorePlan::of(&arms, join);
        let admitted = (!arms.is_empty() || join == StoreJoin::Bound).then(|| self.store_atom_admitted(state, atom, exit));
        let leaves = join == StoreJoin::Host && self.store_leaves();
        state.proofs.write_field(atom);
        self.store_step(state, operation, (plan, HeldRead { admitted, leaves, ..HeldRead::default() }), exit)?;
        if leaves { return Some(()); }
        self.after_store(state, join);
        Some(())
    }

    /// The store of `value`, a word no register names, to the sink field `atom` of `owner`'s
    /// object (`this` for `None`) that a nested lookup writes (`FieldSite::sink`), through its
    /// site's plan. It is the only store of a value with no register, so it runs the row's word
    /// form (`Operation::StoreField`).
    fn sink_store(&mut self, state: &mut State, owner: Option<Register>, (atom, site): (Atom, u16), value: ir::Value,
        exit: ir::Block) -> Option<()> {
        let operation = quench_runtime::staged::Operation::StoreField { base: owner, value, fact: Type::ANY, atom, site };
        self.site_store(state, operation, (atom, site, Type::ANY), exit)
    }

    /// The in-place arms and the VM's completion of a store of a value of type `ty` through
    /// `site`, which the row runs (`StorePlan::of`): one arm for each of the site's own shapes
    /// whose layout the plan admits and whose slot the value's type fits or widens by a test
    /// (`StoreArm::of`); the VM's own-field hit where no arm takes the store, through the slot
    /// the entry bound for the site where there is one; and in a body that completes through
    /// the VM's polymorphic entries (`status_and_pc`) a site of several shapes ends in its
    /// adding entry.
    fn site_store_plan(&self, site: u16, ty: Type) -> (Vec<StoreArm>, StoreJoin) {
        let own: Vec<&NativeFieldLoad> = site_loads(self.fields, site).into_iter().flatten().filter(|load| load.holder.is_none()).collect();
        let bound = self.entry.as_ref().is_some_and(|entry| entry.fields.iter().any(|(known, _)| *known == site));
        let arms = own.iter().filter(|load| load.valid())
            .filter_map(|load| StoreArm::of_rep((load.shape, load.slot), load.ty, ty,
                (load.raw_double, quench_runtime::staged::Form::of(load.storage, load.storage_len)))).collect();
        let join = if own.len() > 1 && self.completion.status_and_pc() { StoreJoin::Add }
            else if bound { StoreJoin::Bound } else { StoreJoin::Host };
        (arms, join)
    }

    /// Whether a store of a loop region that its in-place arms miss leaves at its PC rather than
    /// calling the VM's store: the call would spill every value live across the loop and keep the
    /// loop's poll (a reentry). A store left at often enough is widened (`LoopTier::left_at`), and
    /// the recompile takes the VM's store there.
    fn store_leaves(&self) -> bool {
        matches!(self.coverage, Coverage::Loop { .. }) && !self.generic.contains(&self.at)
    }

    /// What a store the VM may have completed changes: an adding entry reshapes any object, so
    /// every proof of one retires and the receiver's facts load again; the VM's own-field hit
    /// writes a field, which a prototype's Stable facts may have named.
    fn after_store(&mut self, state: &mut State, join: StoreJoin) {
        self.forget_append_rooms(self.at);
        match join {
            StoreJoin::Add => {
                self.reshaped(state);
                self.reload_receiver();
            }
            StoreJoin::Host | StoreJoin::Bound => state.proofs.stable = false,
        }
    }

    /// The store `operation` as one row step under `plan` and the `held` facts of its access.
    fn store_step(&mut self, state: &mut State, operation: quench_runtime::staged::Operation<ir::Value, EmitProof>,
        (plan, held): (StorePlan, HeldRead), exit: ir::Block) -> Option<()> {
        let plans = LanePlans { store: Some(plan), ..LanePlans::NONE };
        match self.field_step(state, frame::Edges::ValueGuarded { miss: exit }, operation, (&plans, held)) {
            quench_runtime::staged::Step::Value(_) => Some(()),
            _ => None,
        }
    }

    /// The receiver's shape and storage kind again, and the entry's tests of its shape, after
    /// a store that may have changed them (`emit_lane_receiver`, `Lowering::entry_facts`).
    fn reload_receiver(&mut self) {
        let entry = self.entry.as_ref().expect("entry facts");
        let (object_var, shape_var, inline_var) = (entry.receiver.object, entry.receiver.shape, entry.receiver.inline);
        let tests: Vec<(u32, Variable)> = entry.receiver_shapes.iter().map(|(shape, variable)| (*shape, *variable)).collect();
        let object = self.builder.use_var(object_var);
        let (found, done) = (self.builder.create_block(), self.builder.create_block());
        let present = self.raw_compare_imm(Relation::NotEqual, object, 0);
        self.branch_if(present, (found, &[]), (done, &[]));
        self.builder.switch_to_block(found);
        let (shape, _, inline, _) = super::object_slots::shape_and_inline(&mut self.builder, object);
        self.builder.def_var(shape_var, shape);
        self.builder.def_var(inline_var, inline);
        self.goto(done, &[]);
        self.builder.switch_to_block(done);
        let shape = self.builder.use_var(shape_var);
        for (expected, variable) in tests {
            let same = self.raw_compare_imm(Relation::Equal, shape, i64::from(expected));
            self.builder.def_var(variable, same);
        }
    }

    /// Whether the run admits a direct store of `atom`. A run that calls may meet a method
    /// cache that names the atom only after a call; one that does not tests it once, at
    /// entry (`Lowering::entry_facts`).
    fn store_atom_admitted(&mut self, state: &mut State, atom: Atom, exit: ir::Block) -> ir::Value {
        if self.native_calls {
            if let Some(held) = self.called_held_atom(state, atom, exit) { return held; }
            if let Some(held) = self.online_store_held(state, atom, exit) { return held; }
            self.emit_store_atom_admitted(atom)
        } else {
            self.builder.use_var(self.entry.as_ref().expect("entry facts").store_atoms[&atom])
        }
    }

    /// `+source` or `-source` of a Number: the shared Number unary denotation, with the
    /// source's representation. A non-Number source leaves at its exact PC for the general
    /// coercion, and an int32 negation that overflows widens and leaves.
    fn unary(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        // A returning result completes the activation as a returning binary's does.
        let returns = instruction.returns_from_frame();
        if instruction.writes_current_this() || (instruction.result_flags() != 0 && !returns)
            || (returns && (self.completion.numeric() || !matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_))))) {
            return None;
        }
        let operation = crate::compiled::number_unary(instruction, instruction.unary_operator())?;
        let source = instruction.register_b();
        let domain = if state.values[usize::from(source)].value.ty.is_subtype(Type::INT32) {
            quench_runtime::staged::Domain::Int32
        } else {
            quench_runtime::staged::Domain::Float64
        };
        let exit = self.guard_exit(state, pc as u32);
        let operation = quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::Unary {
            destination: instruction.result_register(),
            source: quench_runtime::bytecode::Operand::register(source),
            operation, domain,
        });
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, operation);
        if returns {
            self.complete_result(state, pc, instruction.result_register());
            return Some(false);
        }
        Some(true)
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
        let arithmetic = crate::compiled::number_arithmetic(instruction, operator);
        let bitwise = crate::compiled::number_bitwise(instruction, operator);
        // `%` is the shared Number remainder; the opcode's denotation is the catalog's, so the
        // meaning is not a second hand-written `Binary` arm.
        let remainder = quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary)
            && operator == super::operations::REMAINDER;
        // Arithmetic takes Number constants; a relation decides its own (`relation`).
        if (arithmetic.is_some() || bitwise.is_some() || remainder) && [left, right].iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
            && self.program.constants.get(usize::from(operand.payload())).and_then(super::facts::immediate).and_then(Value::as_number).is_none()) {
            return None;
        }
        let exit = self.guard_exit(state, pc as u32);
        if let Some(arithmetic) = arithmetic
            && arithmetic.int32_projection_is_wrapping() && !self.left_here
            && instruction.result_flags() == 0
            && let Some(next) = self.unit.instructions.get(pc + 1).copied()
            && next.result_flags() == 0
            && quench_runtime::staged::meaning::means(next, quench_runtime::staged::meaning::Meaning::Binary)
            && crate::compiled::number_bitwise(next, next.binary_operator())
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
            let bits = quench_runtime::staged::number_machine::wrapping_int32_arithmetic(&mut super::object_slots::emit(&mut self.builder), arithmetic, left, right);
            self.set_int(state, next.result_register(), bits);
            self.consumed.insert(pc + 1);
            return Some(true);
        }
        match (arithmetic, bitwise) {
            _ if remainder => { self.remainder(state, pc, destination, [left, right], exit); }
            (Some(arithmetic), _) => { self.arithmetic(state, pc, instruction, arithmetic, exit)?; }
            (_, Some(bitwise)) => {
                let left_int = self.operand_to_int32(state, left, exit)?;
                let right_int = self.operand_to_int32(state, right, exit)?;
                let right_constant = super::facts::constant_number(&self.program.constants, right);
                if !bitwise.result_is_int32(right_constant) {
                    let number = quench_runtime::staged::number_machine::number_bitwise(&mut super::object_slots::emit(&mut self.builder), bitwise, left_int, right_int);
                    self.set_number(state, destination, number);
                } else {
                    let result = quench_runtime::staged::number_machine::int32_bitwise(&mut super::object_slots::emit(&mut self.builder), bitwise, left_int, right_int);
                    self.set_int(state, destination, result);
                }
            }
            // A relation's Boolean, from the predicate a branch on it would test.
            _ => {
                let holds = self.relation(state, pc as u32, operator, left, right)?;
                let boolean = self.boolean(holds);
                self.set_boolean(state, destination, boolean);
            }
        }
        if returns && self.completion.numeric() {
            self.return_number(state, destination, pc as u32);
            return Some(false);
        }
        if returns {
            self.complete_result(state, pc, destination);
            self.native += 1;
            return Some(false);
        }
        Some(true)
    }

    /// Completes the activation with `destination`, the result of the returning operator at the
    /// caller record `pc` (`RETURN_REGISTER`). The completion names the function's terminal
    /// `Return` where it has one, whose register the lane reads it from, else the operator's own
    /// record.
    fn complete_result(&mut self, state: &mut State, pc: usize, destination: Register) {
        let terminal = self.unit.instructions.len().checked_sub(1)
            .filter(|terminal| matches!(crate::compiled::decoded(self.unit.instructions[*terminal]), Some(quench_runtime::staged::Operation::Return { .. })))
            .and_then(|terminal| match self.unit.origins.get(terminal) {
                Some(super::lane_unit::Origin::Caller(record)) => Some((*record, self.unit.instructions[terminal].register_a())),
                _ => None,
            });
        let Some(super::lane_unit::Origin::Caller(own)) = self.unit.origins.get(pc).copied() else { unreachable!("a returning operator is a caller record's") };
        let bits = self.tagged(state, destination);
        // The activation's `Return` register holds it too, as the record it names reads.
        if let Some((_, register)) = terminal {
            let base = self.registers_base;
            self.store_word(regions::frame(), bits, base, register_offset(register));
        }
        self.return_value(terminal.map_or(own, |(record, _)| record), bits);
    }
}

/// Continue only when `condition` is false; `fallback` otherwise.
fn emit_js_guard_not(builder: &mut FunctionBuilder<'_>, condition: ir::Value, fallback: ir::Block) {
    let passed = builder.create_block();
    emit(builder).branch_if(condition, (fallback, &[]), (passed, &[]));
    builder.switch_to_block(passed);
}

#[cfg(test)]
#[path = "lane_region/tests.rs"]
pub(crate) mod tests;
#[cfg(test)]
#[path = "lane_region/lowering_tests.rs"]
pub(crate) mod lowering_tests;
#[path = "lane_region/kept_shapes.rs"] pub(crate) mod kept_shapes;
#[path = "lane_region/loop_receivers.rs"] pub(crate) mod loop_receivers;
#[cfg(test)]
#[path = "lane_region/slot_proof_tests.rs"]
pub(crate) mod slot_proof_tests;

/// A loop's entry types the function's bytecode proves: a register the loop reads but never writes,
/// whose every definition before or after the loop loads a constant that is no Number (a string,
/// a boolean, `null`), has the join of those constants' types, which the entry guards once. A
/// relation reading it is then decided by the interpreter's rule (`Lowering::decided_relation`)
/// instead of being taken for a Number comparison whose entry guard always fails.
fn seed_constant_entries(instructions: &[WideInstruction], program: &ResidualProgram,
    (header, backedge): (usize, usize), initial: &mut [Type]) {
    let inside = header..=backedge;
    let written: std::collections::BTreeSet<Register> = instructions.get(inside.clone()).into_iter().flatten()
        .filter_map(|instruction| instruction.defined_register()).collect();
    let mut seeded = std::collections::BTreeMap::<Register, Option<Type>>::new();
    for (_, instruction) in instructions.iter().enumerate().filter(|(pc, _)| !inside.contains(pc)) {
        let Some(register) = instruction.defined_register() else { continue };
        if written.contains(&register) { continue; }
        // A definition by a constant-table index (the staged row of a heap constant, a string, has no
        // stage word to decode, so the instruction's layout names it); the table types it.
        let ty = (instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::ConstantIndex)
            .then(|| program.constants.get(instruction.constant_index())).flatten()
            .map(super::facts::constant_type).filter(|ty| super::facts::numberless(*ty));
        let slot = seeded.entry(register).or_insert(ty);
        *slot = slot.zip(ty).map(|(left, right)| left.join(right));
    }
    for (register, ty) in seeded {
        if let (Some(ty), Some(slot)) = (ty, initial.get_mut(usize::from(register))) && *slot == Type::ANY { *slot = ty; }
    }
}
